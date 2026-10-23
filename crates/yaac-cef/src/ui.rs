//! UI-thread state and command execution. CEF objects live here, in a thread-local, and are never
//! handed to other threads. Callbacks and commands take short borrows of the state and never call
//! into CEF while a borrow is held, so a re-entrant callback cannot hit a RefCell conflict.

use crate::client::{EngineClient, NavState};
use crate::engine::{Shared, post_to_ui};
use crate::log::elog;
use crate::window::{ViewDelegateImpl, WindowDelegateImpl};
use cef::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use yaac_engine::{EngineCommand, EngineEvent, ViewKind, ViewSpec, WindowSpec};
use yaac_model::{ProfileId, ViewId, WindowId};

/// Logical width of the sidebar view. The content view takes the rest.
pub(crate) const SIDEBAR_WIDTH: i32 = 240;

/// A named profile's request context initializes asynchronously. A browser view created before
/// that fails, so `CreateView` is retried on a timer, up to this many attempts (about 5 s).
const CREATE_ATTEMPTS: u32 = 250;
const RETRY_MS: i64 = 20;

struct WindowEntry {
    window: Option<Window>,
    root: Panel,
    layout: BoxLayout,
    /// Preferred height of the browser views, so the window gets its height from them.
    height: i32,
    /// Views in layout order.
    views: Vec<ViewId>,
}

struct ViewEntry {
    window: WindowId,
    kind: ViewKind,
    view: BrowserView,
    /// Set in `on_after_created`, cleared in `on_before_close`.
    browser: Option<Browser>,
    /// Commands that arrived before the browser existed. Flushed on creation.
    pending: Vec<EngineCommand>,
}

#[derive(Default)]
struct State {
    shared: Option<Arc<Shared>>,
    windows: HashMap<WindowId, WindowEntry>,
    views: HashMap<ViewId, ViewEntry>,
    contexts: HashMap<ProfileId, RequestContext>,
    /// Commands waiting for a profile to initialize, in order. While it is non-empty, new commands
    /// queue behind it, so the order the core sent them is kept.
    deferred: VecDeque<(EngineCommand, u32)>,
    flush_scheduled: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

fn shared() -> Option<Arc<Shared>> {
    with(|s| s.shared.clone())
}

/// Called once on the UI thread, when CEF's context is initialized.
pub(crate) fn install(shared: Arc<Shared>) {
    with(|s| s.shared = Some(shared));
}

/// Runs one command in order. UI thread only.
pub(crate) fn execute(command: EngineCommand) {
    let queued = with(|s| !s.deferred.is_empty());
    if queued {
        with(|s| s.deferred.push_back((command, 0)));
        schedule_flush();
        return;
    }
    if let Some(retry) = execute_once(command, 0) {
        with(|s| s.deferred.push_back(retry));
        schedule_flush();
    }
}

/// Retries deferred commands in order. Stops at the first one that is still not ready.
pub(crate) fn flush_deferred() {
    with(|s| s.flush_scheduled = false);
    loop {
        let next = with(|s| s.deferred.pop_front());
        let Some((command, attempts)) = next else {
            return;
        };
        if let Some(retry) = execute_once(command, attempts) {
            with(|s| s.deferred.push_front(retry));
            schedule_flush();
            return;
        }
    }
}

fn schedule_flush() {
    let already = with(|s| std::mem::replace(&mut s.flush_scheduled, true));
    if already {
        return;
    }
    let mut task = FlushTask::new();
    post_delayed_task(ThreadId::UI, Some(&mut task), RETRY_MS);
}

wrap_task! {
    struct FlushTask {}

    impl Task {
        fn execute(&self) {
            flush_deferred();
        }
    }
}

/// Runs one command. Returns the command again, with its attempt count, if it must be retried.
fn execute_once(command: EngineCommand, attempts: u32) -> Option<(EngineCommand, u32)> {
    match command {
        EngineCommand::CreateWindow { window, spec } => create_window(window, &spec),
        EngineCommand::CreateView { view, window, spec } => {
            if !create_view(view, window, &spec) {
                if attempts + 1 >= CREATE_ATTEMPTS {
                    elog!("error create_view view={view} reason=gave-up attempts={attempts}");
                } else {
                    return Some((
                        EngineCommand::CreateView { view, window, spec },
                        attempts + 1,
                    ));
                }
            }
        }
        EngineCommand::Show { view } => show(view),
        EngineCommand::Close { view } => {
            with_browser(view, EngineCommand::Close { view }, |browser| {
                if let Some(host) = browser.host() {
                    let _ = host.try_close_browser();
                }
            });
        }
        EngineCommand::Load { view, url } => {
            let pending = EngineCommand::Load {
                view,
                url: url.clone(),
            };
            with_browser(view, pending, |browser| {
                if let Some(frame) = browser.main_frame() {
                    frame.load_url(Some(&CefString::from(url.as_str())));
                }
            });
        }
        EngineCommand::Reload { view } => {
            with_browser(view, EngineCommand::Reload { view }, |browser| {
                browser.reload()
            });
        }
        EngineCommand::Back { view } => {
            with_browser(view, EngineCommand::Back { view }, |browser| {
                browser.go_back()
            });
        }
        EngineCommand::Forward { view } => {
            with_browser(view, EngineCommand::Forward { view }, |browser| {
                browser.go_forward()
            });
        }
    }
    None
}

/// Runs `op` on the view's browser, or queues `pending` until the browser exists.
fn with_browser(view: ViewId, pending: EngineCommand, op: impl FnOnce(&Browser)) {
    let found = with(|s| match s.views.get_mut(&view) {
        None => Err("unknown view"),
        Some(entry) => match &entry.browser {
            Some(browser) => Ok(Some(browser.clone())),
            None => {
                entry.pending.push(pending);
                Ok(None)
            }
        },
    });
    match found {
        Ok(Some(browser)) => op(&browser),
        Ok(None) => elog!("queued command view={view} reason=browser-not-created"),
        Err(reason) => elog!("dropped command view={view} reason={reason}"),
    }
}

fn create_window(id: WindowId, spec: &WindowSpec) {
    let Some(root) = panel_create(None) else {
        elog!("error create_window window={id} reason=panel");
        return;
    };
    let Some(layout) = root.set_to_box_layout(Some(&BoxLayoutSettings {
        horizontal: 1,
        ..Default::default()
    })) else {
        elog!("error create_window window={id} reason=box-layout");
        return;
    };
    let mut delegate =
        WindowDelegateImpl::new(id, root.clone(), spec.width as i32, spec.height as i32);
    let window = window_create_top_level(Some(&mut delegate));
    let created = window.is_some();
    with(|s| {
        s.windows.insert(
            id,
            WindowEntry {
                window,
                root: root.clone(),
                layout,
                height: spec.height as i32,
                views: Vec::new(),
            },
        )
    });
    elog!(
        "window created window={id} size={}x{} ok={created}",
        spec.width,
        spec.height
    );
}

/// Creates a view. Returns false when the profile is not ready yet, so the command must be
/// retried. Every other outcome, success included, returns true.
fn create_view(view: ViewId, window: WindowId, spec: &ViewSpec) -> bool {
    let Some(shared) = shared() else {
        elog!("error create_view view={view} reason=not-installed");
        return true;
    };
    let window_known = with(|s| s.windows.contains_key(&window));
    if !window_known {
        elog!("error create_view view={view} reason=unknown-window window={window}");
        return true;
    }
    let Some(mut context) = request_context(spec.profile) else {
        elog!("error create_view view={view} reason=request-context");
        return true;
    };
    // Registered on every attempt. CEF ignores scheme registration on a context that is still
    // initializing, so the registration that counts is the one made just before the view is
    // created successfully.
    let mut factory = crate::scheme::AppSchemeFactory::new(shared.clone());
    context.register_scheme_handler_factory(
        Some(&CefString::from(crate::scheme::SCHEME)),
        Some(&CefString::from(crate::scheme::DOMAIN)),
        Some(&mut factory),
    );

    let mut client = EngineClient::new(
        view,
        spec.kind,
        shared.clone(),
        Arc::new(NavState::default()),
    );
    let width = match spec.kind {
        ViewKind::Sidebar => SIDEBAR_WIDTH,
        ViewKind::Content => 0,
    };
    let height = with(|s| s.windows.get(&window).map(|w| w.height)).unwrap_or(0);
    let mut delegate = ViewDelegateImpl::new(width, height);
    let settings = BrowserSettings::default();
    let url = CefString::from(spec.url.as_str());
    let Some(browser_view) = browser_view_create(
        Some(&mut client),
        Some(&url),
        Some(&settings),
        None,
        Some(&mut context),
        Some(&mut delegate),
    ) else {
        // Expected while the profile initializes; see CREATE_ATTEMPTS.
        return false;
    };

    shared.registry.add_view(view, spec.kind);
    let attached = with(|s| {
        let entry = s.windows.get_mut(&window)?;
        entry.views.push(view);
        Some((entry.root.clone(), entry.layout.clone()))
    });
    let Some((root, layout)) = attached else {
        // Window closed while the view was being created. Close the view.
        shared.registry.remove_view(view);
        return true;
    };
    with(|s| {
        s.views.insert(
            view,
            ViewEntry {
                window,
                kind: spec.kind,
                view: browser_view.clone(),
                browser: None,
                pending: Vec::new(),
            },
        )
    });

    let mut view_handle = View::from(&browser_view);
    match spec.kind {
        ViewKind::Sidebar => {
            // The sidebar is the first child, on the left.
            root.add_child_view_at(Some(&mut view_handle), 0);
        }
        ViewKind::Content => {
            // Content views stay hidden until `Show`. They keep loading in the background.
            view_handle.set_visible(0);
            root.add_child_view(Some(&mut view_handle));
            layout.set_flex_for_view(Some(&mut view_handle), 1);
        }
    }
    elog!(
        "view created view={view} window={window} kind={:?} profile={}",
        spec.kind,
        spec.profile
    );
    true
}

/// The request context for a profile, created on first use. Each profile has its own cache path,
/// and `app://ui` is registered on it, since scheme factories are per context.
fn request_context(profile: ProfileId) -> Option<RequestContext> {
    if let Some(existing) = with(|s| s.contexts.get(&profile).cloned()) {
        return Some(existing);
    }
    let shared = shared()?;
    let cache = shared.data_dir.join("cef").join(profile.to_string());
    // CEF does not create the profile directory itself; it fails to create the profile without it.
    if let Err(error) = std::fs::create_dir_all(&cache) {
        elog!("error profile-dir profile={profile} reason={error}");
        return None;
    }
    let settings = RequestContextSettings {
        cache_path: CefString::from(cache.to_string_lossy().as_ref()),
        ..Default::default()
    };
    let context = request_context_create_context(Some(&settings), None)?;
    elog!(
        "profile context profile={profile} cache={}",
        cache.display()
    );
    with(|s| s.contexts.insert(profile, context.clone()));
    Some(context)
}

/// Makes one content view visible in its window and hides the others. The previous view stays
/// alive. Sidebar views are not affected.
fn show(view: ViewId) {
    let plan = with(|s| {
        let kind = s.views.get(&view)?.kind;
        if kind != ViewKind::Content {
            return None;
        }
        let window = s.views.get(&view)?.window;
        let entry = s.windows.get(&window)?;
        let siblings: Vec<(ViewId, BrowserView)> = entry
            .views
            .iter()
            .filter_map(|id| {
                let candidate = s.views.get(id)?;
                (candidate.kind == ViewKind::Content).then(|| (*id, candidate.view.clone()))
            })
            .collect();
        Some((window, siblings))
    });
    let Some((window, siblings)) = plan else {
        elog!("error show view={view} reason=not-a-content-view");
        return;
    };
    for (id, browser_view) in siblings {
        View::from(&browser_view).set_visible(i32::from(id == view));
    }
    elog!("show view={view} window={window}");
    log_layout_later(window);
}

/// Logs the bounds of every view in a window after the layout pass.
fn log_layout_later(window: WindowId) {
    let mut task = LayoutTask::new(window);
    post_delayed_task(ThreadId::UI, Some(&mut task), 150);
}

wrap_task! {
    struct LayoutTask {
        window: WindowId,
    }

    impl Task {
        fn execute(&self) {
            log_layout(self.window);
        }
    }
}

fn log_layout(window: WindowId) {
    let rows = with(|s| {
        let entry = s.windows.get(&window)?;
        Some(
            entry
                .views
                .iter()
                .filter_map(|id| {
                    let view = s.views.get(id)?;
                    Some((*id, view.kind, view.view.clone()))
                })
                .collect::<Vec<_>>(),
        )
    });
    let Some(rows) = rows else { return };
    for (id, kind, browser_view) in rows {
        let handle = View::from(&browser_view);
        let rect = handle.bounds();
        let visible = handle.is_visible() != 0;
        elog!(
            "layout window={window} view={id} kind={kind:?} visible={visible} bounds={},{} {}x{}",
            rect.x,
            rect.y,
            rect.width,
            rect.height
        );
    }
}

/// `on_after_created`: the browser exists. Binds it to its view and flushes queued commands.
pub(crate) fn browser_created(view: ViewId, browser: Browser) {
    let Some(shared) = shared() else { return };
    let browser_id = browser.identifier();
    let bound = with(|s| {
        let entry = s.views.get_mut(&view)?;
        entry.browser = Some(browser.clone());
        Some((entry.kind, std::mem::take(&mut entry.pending)))
    });
    let Some((kind, pending)) = bound else {
        elog!("error browser_created view={view} reason=unknown-view");
        return;
    };
    shared.registry.bind_browser(browser_id, view, kind);
    let window_number = crate::macos::window_number(&browser)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "none".into());
    elog!(
        "browser created view={view} kind={kind:?} browser={browser_id} native_window={window_number}"
    );
    shared.emit(EngineEvent::Created { view });
    for command in pending {
        execute(command);
    }
}

/// `on_before_close`: the browser is gone. Removes the view, reports `Closed`, and closes the
/// window when it was the last view.
pub(crate) fn browser_closed(view: ViewId) {
    let Some(shared) = shared() else { return };
    let removed = with(|s| {
        let entry = s.views.remove(&view)?;
        let (root, empty) = match s.windows.get_mut(&entry.window) {
            Some(window) => {
                window.views.retain(|id| *id != view);
                (Some(window.root.clone()), window.views.is_empty())
            }
            None => (None, false),
        };
        Some((entry, root, empty))
    });
    let Some((entry, root, empty)) = removed else {
        return;
    };
    if let Some(browser) = &entry.browser {
        shared.registry.unbind_browser(browser.identifier());
    }
    shared.registry.remove_view(view);
    shared.emit(EngineEvent::Closed { view });
    elog!("view closed view={view} window_empty={empty}");

    // Detach and close outside the callback, so CEF finishes this close first.
    let window_id = entry.window;
    let browser_view = entry.view;
    let _ = post_to_ui(Box::new(move || {
        if let Some(root) = root {
            let mut handle = View::from(&browser_view);
            root.remove_child_view(Some(&mut handle));
        }
        if empty {
            close_window(window_id);
        }
    }));
}

/// Closes the window's native window, if it still exists.
fn close_window(window: WindowId) {
    let handle = with(|s| s.windows.get(&window).and_then(|w| w.window.clone()));
    if let Some(handle) = handle {
        handle.close();
    }
}

/// `WindowDelegate::can_close`: asks every browser in the window to close. The window closes
/// when all of them accept.
pub(crate) fn can_close_window(window: WindowId) -> bool {
    let browsers: Vec<Browser> = with(|s| {
        let Some(entry) = s.windows.get(&window) else {
            return Vec::new();
        };
        entry
            .views
            .iter()
            .filter_map(|id| s.views.get(id)?.browser.clone())
            .collect()
    });
    let mut allowed = true;
    for browser in browsers {
        if let Some(host) = browser.host() {
            allowed &= host.try_close_browser() != 0;
        }
    }
    allowed
}

/// `WindowDelegate::on_window_destroyed`. Quits the message loop after the last window.
pub(crate) fn window_destroyed(window: WindowId) {
    let remaining = with(|s| {
        s.windows.remove(&window);
        s.windows.len()
    });
    if let Some(shared) = shared() {
        shared.emit(EngineEvent::WindowClosed { window });
    }
    elog!("window closed window={window} remaining={remaining}");
    if remaining == 0 {
        quit_message_loop();
    }
}

/// Closes every window, or ends the loop if there are none. UI thread.
pub(crate) fn close_all_windows() {
    let windows: Vec<Window> = with(|s| {
        s.windows
            .values()
            .filter_map(|w| w.window.clone())
            .collect()
    });
    if windows.is_empty() {
        quit_message_loop();
        return;
    }
    for window in windows {
        window.close();
    }
}

/// A trusted left click at (x, y) in the view. CEF treats it as a user gesture, which a page needs
/// before `window.open` is allowed. Used by the harness. UI thread.
pub(crate) fn click(view: ViewId, x: i32, y: i32) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    let Some(host) = browser.and_then(|b| b.host()) else {
        elog!("dropped click view={view} reason=browser-not-created");
        return;
    };
    let event = MouseEvent { x, y, modifiers: 0 };
    host.send_mouse_move_event(Some(&event), 0);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 0, 1);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 1, 1);
}

/// Sends `script` to the view's main frame. UI thread.
pub(crate) fn push_script(view: ViewId, script: String) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    let Some(browser) = browser else {
        elog!("dropped push view={view} reason=browser-not-created");
        return;
    };
    if let Some(frame) = browser.main_frame() {
        frame.execute_java_script(
            Some(&CefString::from(script.as_str())),
            Some(&CefString::from("app://ui/")),
            0,
        );
    }
}
