//! UI-thread state and command execution. CEF objects live here, in a thread-local, and never
//! leave the UI thread. Callbacks and commands take short borrows of the state and do not call CEF
//! while a borrow is held, so a re-entrant callback cannot hit a RefCell conflict.
//!
//! Commands run strictly in the order they were sent. A command that needs something not ready yet
//! (a browser that is still being created, a profile that is still initializing) waits at the head
//! of the queue, and everything behind it waits too.

use crate::client::EngineClient;
use crate::engine::{Shared, post_after, post_to_ui};
use crate::log::elog;
use crate::nav::NavTracker;
use crate::window::{ViewDelegateImpl, WindowDelegateImpl};
use cef::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use yaac_engine::{EngineCommand, EngineEvent, ViewKind, ViewSpec, WindowSpec};
use yaac_model::{ProfileId, ViewId, WindowId};
use yaac_platform::ensure_private_dir as private_dir;

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
    /// The Views object. `None` once a tab close released it (see `tab_do_close`).
    view: Option<BrowserView>,
    /// Set in `on_after_created`, cleared in `on_before_close`.
    browser: Option<Browser>,
    /// Counted in the live totals (set at `Created`, cleared at close).
    counted: bool,
    /// The window closed before this view's browser existed. The browser is closed when it appears.
    cancelled: bool,
}

#[derive(Clone)]
struct Queued {
    command: EngineCommand,
    attempts: u32,
}

#[derive(Default)]
struct State {
    shared: Option<Arc<Shared>>,
    windows: HashMap<WindowId, WindowEntry>,
    views: HashMap<ViewId, ViewEntry>,
    contexts: HashMap<ProfileId, RequestContext>,
    queue: VecDeque<Queued>,
    pump_scheduled: bool,
    closing: HashSet<WindowId>,
    /// Browsers whose tab close was requested through `Close{view}`.
    tab_closing: HashSet<i32>,
    /// Windows whose close attempt committed.
    committed: HashSet<WindowId>,
    /// Close attempts in progress, with the `Closed` events they hold back.
    attempts: HashMap<WindowId, Vec<ViewId>>,
}

/// Test hook: until this instant, new profiles are not ready (see `engine::test_hooks`).
#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub(crate) static PROFILE_DELAY: std::sync::Mutex<
    Option<(std::time::Instant, std::time::Duration)>,
> = std::sync::Mutex::new(None);

#[cfg(any(debug_assertions, feature = "dev-tools"))]
fn profile_delayed() -> bool {
    PROFILE_DELAY
        .lock()
        .ok()
        .and_then(|slot| *slot)
        .is_some_and(|(since, delay)| since.elapsed() < delay)
}

#[cfg(not(any(debug_assertions, feature = "dev-tools")))]
fn profile_delayed() -> bool {
    false
}

/// Window close attempts entered (`can_close_window`). Read by the harness to show that closing a tab
/// did not start one.
pub(crate) static CLOSE_ATTEMPTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

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

/// Drops every CEF object this thread holds. Called on the UI thread after the message loop ends,
/// before `CefShutdown`.
pub(crate) fn release_ui_state() {
    let old = with(std::mem::take);
    drop(old);
}

fn quitting() -> bool {
    shared().is_some_and(|s| s.quitting.load(Ordering::SeqCst))
}

/// Accepts a command from the core. It runs when everything sent before it has run.
pub(crate) fn execute(command: EngineCommand) {
    if quitting() {
        elog!("dropped command reason=quitting");
        return;
    }
    with(|s| {
        s.queue.push_back(Queued {
            command,
            attempts: 0,
        })
    });
    pump();
}

/// Runs queued commands from the head while they are ready.
fn pump() {
    loop {
        let Some(item) = with(|s| s.queue.front().cloned()) else {
            return;
        };
        if !is_ready(&item.command) {
            schedule_pump();
            return;
        }
        with(|s| s.queue.pop_front());
        if let Some(retry) = run(item.command, item.attempts) {
            with(|s| s.queue.push_front(retry));
            schedule_pump();
            return;
        }
    }
}

fn schedule_pump() {
    let already = with(|s| std::mem::replace(&mut s.pump_scheduled, true));
    if already {
        return;
    }
    if !post_after(Box::new(retry_pump), RETRY_MS) {
        with(|s| s.pump_scheduled = false);
    }
}

fn retry_pump() {
    with(|s| s.pump_scheduled = false);
    pump();
}

/// A command can run when the browser it targets exists, or was cancelled (and is then dropped).
/// Creating and showing do not need a browser.
fn is_ready(command: &EngineCommand) -> bool {
    match command {
        EngineCommand::Load { view, .. }
        | EngineCommand::Reload { view }
        | EngineCommand::Back { view }
        | EngineCommand::Forward { view }
        | EngineCommand::Close { view } => with(|s| match s.views.get(view) {
            Some(entry) => entry.browser.is_some() || entry.cancelled,
            None => true,
        }),
        EngineCommand::CreateWindow { .. }
        | EngineCommand::CreateView { .. }
        | EngineCommand::Show { .. } => true,
    }
}

/// Runs one ready command. Returns the command again, with its attempt count, when it must wait.
fn run(command: EngineCommand, attempts: u32) -> Option<Queued> {
    match command {
        EngineCommand::CreateWindow { window, spec } => create_window(window, &spec),
        EngineCommand::CreateView { view, window, spec } => {
            match create_view(view, window, &spec) {
                Outcome::Created => {}
                Outcome::Retry if attempts + 1 < CREATE_ATTEMPTS => {
                    return Some(Queued {
                        command: EngineCommand::CreateView { view, window, spec },
                        attempts: attempts + 1,
                    });
                }
                Outcome::Retry => fail_create(view, "profile not ready"),
                Outcome::Failed(reason) => fail_create(view, reason),
            }
        }
        EngineCommand::Show { view } => show(view),
        EngineCommand::Close { view } => close_tab(view),
        EngineCommand::Load { view, url } => with_browser(view, |browser| {
            if let Some(frame) = browser.main_frame() {
                frame.load_url(Some(&CefString::from(url.as_str())));
            }
        }),
        EngineCommand::Reload { view } => with_browser(view, |browser| browser.reload()),
        EngineCommand::Back { view } => with_browser(view, |browser| browser.go_back()),
        EngineCommand::Forward { view } => with_browser(view, |browser| browser.go_forward()),
    }
    None
}

enum Outcome {
    Created,
    /// Not ready yet (the profile is still initializing). Try again.
    Retry,
    Failed(&'static str),
}

fn fail_create(view: ViewId, reason: &str) {
    elog!("error create_view view={view} reason={reason}");
    if let Some(shared) = shared() {
        shared.emit(EngineEvent::CreateFailed { view });
    }
}

/// Runs `op` on the view's browser. A command for a view that has no browser (never created, or
/// already closed) is dropped with a log line.
fn with_browser(view: ViewId, op: impl FnOnce(&Browser)) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    match browser {
        Some(browser) => op(&browser),
        None => elog!("dropped command view={view} reason=no-browser"),
    }
}

/// Closes one tab. CEF Views closes the parent window when a browser in it closes, unless the
/// browser's `on_do_close` returns true and the application then destroys the browser's
/// `BrowserView` itself (CEF documentation for `DoClose` with Views). So the browser is marked here,
/// and `tab_do_close` completes the close. The close is forced: the page's own close handling (a
/// `beforeunload` prompt) is not waited for, because a tab whose close button was pressed must
/// close. `on_before_close` then reports the view closed as usual.
fn close_tab(view: ViewId) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    match browser {
        Some(browser) => {
            with(|s| s.tab_closing.insert(browser.identifier()));
            elog!("tab close view={view}");
            close_browser(&browser, true);
        }
        None => elog!("dropped close view={view} reason=no-browser"),
    }
}

/// `on_do_close` for a browser. For a tab closed by `Close{view}` it returns true (handled) and
/// completes the close: the tab's `BrowserView` leaves the window and the engine releases its own
/// reference, so the view is destroyed and the browser reports `on_before_close`. For every other
/// browser it returns false, the default, so a real window close still reaches the window.
pub(crate) fn tab_do_close(browser: i32) -> bool {
    if !with(|s| s.tab_closing.remove(&browser)) {
        return false;
    }
    // A browser the engine never bound to a view cannot be released here, and returning true would
    // leave it open for good: take the default instead.
    let Some(shared) = shared() else { return false };
    let Some((view, _)) = shared.registry.browser(browser) else {
        return false;
    };
    post_to_ui(Box::new(move || {
        let released = with(|s| {
            let entry = s.views.get_mut(&view)?;
            let root = s.windows.get(&entry.window)?.root.clone();
            Some((root, entry.view.take()))
        });
        if let Some((root, Some(browser_view))) = released {
            let mut handle = View::from(&browser_view);
            root.remove_child_view(Some(&mut handle));
            // `browser_view` and `handle` drop here: the last reference to the Views object.
        }
    }));
    true
}

/// Closes a browser. `force` skips the page's own close handling, used for a crashed renderer.
fn close_browser(browser: &Browser, force: bool) {
    if let Some(host) = browser.host() {
        if force {
            host.close_browser(1);
        } else {
            let _ = host.try_close_browser();
        }
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
                root,
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

/// Creates a view. `Retry` means the profile is not ready; anything else is final.
fn create_view(view: ViewId, window: WindowId, spec: &ViewSpec) -> Outcome {
    let Some(shared) = shared() else {
        return Outcome::Failed("not installed");
    };
    if profile_delayed() {
        return Outcome::Retry;
    }
    if !with(|s| s.windows.contains_key(&window)) {
        return Outcome::Failed("unknown window");
    }
    let mut context = match request_context(spec.profile) {
        Ok(context) => context,
        Err(reason) => return Outcome::Failed(reason),
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
        Arc::new(Mutex::new(NavTracker::default())),
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
        // Expected while the profile initializes; the caller retries up to CREATE_ATTEMPTS.
        return Outcome::Retry;
    };

    shared.registry.add_view(view, spec.kind);
    let attached = with(|s| {
        let cancelled = s.closing.contains(&window);
        let entry = s.windows.get_mut(&window)?;
        entry.views.push(view);
        let parts = (entry.root.clone(), entry.layout.clone());
        s.views.insert(
            view,
            ViewEntry {
                window,
                kind: spec.kind,
                view: Some(browser_view.clone()),
                browser: None,
                counted: false,
                cancelled,
            },
        );
        Some(parts)
    });
    let Some((root, layout)) = attached else {
        shared.registry.remove_view(view);
        return Outcome::Failed("window closed during creation");
    };

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
        "view created view={view} window={window} kind={:?}",
        spec.kind
    );
    Outcome::Created
}

/// The request context for a profile, created on first use. Each profile has its own cache path
/// (a direct child of CEF's root), and every directory on the way is private to the user.
fn request_context(profile: ProfileId) -> Result<RequestContext, &'static str> {
    if let Some(existing) = with(|s| s.contexts.get(&profile).cloned()) {
        return Ok(existing);
    }
    let shared = shared().ok_or("not installed")?;
    let root = shared.data_dir.join("cef");
    let cache = root.join(profile.to_string());
    if private_dir(&root)
        .and_then(|()| private_dir(&cache))
        .is_err()
    {
        elog!("error profile-dir profile={profile}");
        return Err("profile directory");
    }
    let settings = RequestContextSettings {
        cache_path: CefString::from(cache.to_string_lossy().as_ref()),
        ..Default::default()
    };
    let context = request_context_create_context(Some(&settings), None).ok_or("request context")?;
    elog!("profile context profile={profile}");
    with(|s| s.contexts.insert(profile, context.clone()));
    Ok(context)
}

/// Makes one content view visible in its window and hides the others. The previous view stays
/// alive. Sidebar views are not affected.
fn show(view: ViewId) {
    let plan = with(|s| {
        let entry = s.views.get(&view)?;
        if entry.kind != ViewKind::Content {
            return None;
        }
        let window = entry.window;
        let siblings: Vec<(ViewId, BrowserView)> = s
            .windows
            .get(&window)?
            .views
            .iter()
            .filter_map(|id| {
                let candidate = s.views.get(id)?;
                candidate
                    .view
                    .clone()
                    .filter(|_| candidate.kind == ViewKind::Content)
                    .map(|v| (*id, v))
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
    // The window and its root panel, so a zero-height view can be traced to the layout above it.
    let frame = with(|s| {
        let entry = s.windows.get(&window)?;
        Some((View::from(&entry.root), entry.window.clone()))
    });
    if let Some((root, win)) = frame {
        let rect = root.bounds();
        let size = win.map(|w| {
            let s = ImplView::size(&w);
            format!("{}x{}", s.width, s.height)
        });
        elog!(
            "layout root window={window} bounds={},{} {}x{} window_size={}",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            size.unwrap_or_else(|| "none".into())
        );
    }
    let rows = with(|s| {
        let entry = s.windows.get(&window)?;
        Some(
            entry
                .views
                .iter()
                .filter_map(|id| {
                    let view = s.views.get(id)?;
                    Some((*id, view.kind, view.view.clone()?))
                })
                .collect::<Vec<_>>(),
        )
    });
    let Some(rows) = rows else { return };
    for (id, kind, browser_view) in rows {
        let handle = View::from(&browser_view);
        let rect = handle.bounds();
        let visible = handle.is_visible() != 0;
        let pref = handle.preferred_size();
        elog!(
            "layout window={window} view={id} kind={kind:?} visible={visible} bounds={},{} {}x{} preferred={}x{}",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            pref.width,
            pref.height
        );
    }
}

/// `on_after_created`: the browser exists. Binds it to its view and reports `Created`. If the window
/// is closing, the browser is closed at once and nothing is reported.
pub(crate) fn browser_created(view: ViewId, browser: Browser) {
    let Some(shared) = shared() else { return };
    let browser_id = browser.identifier();
    let bound = with(|s| {
        let entry = s.views.get_mut(&view)?;
        entry.browser = Some(browser.clone());
        Some((entry.kind, entry.cancelled))
    });
    let Some((kind, cancelled)) = bound else {
        elog!("error browser_created view={view} reason=unknown-view");
        close_browser(&browser, false);
        return;
    };
    if cancelled || quitting() {
        elog!("browser cancelled view={view}");
        close_browser(&browser, false);
        return;
    }
    shared.registry.bind_browser(browser_id, view, kind);
    shared.live[Shared::live_index(kind)].fetch_add(1, Ordering::SeqCst);
    with(|s| {
        if let Some(entry) = s.views.get_mut(&view) {
            entry.counted = true;
        }
    });
    let native = crate::os::window_number(&browser)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "none".into());
    elog!("browser created view={view} kind={kind:?} browser={browser_id} native_window={native}");
    shared.emit(EngineEvent::Created { view });
    post_to_ui(Box::new(pump));
}

/// `on_before_close`: the browser is gone. Removes the view from the layout. `Closed` is reported
/// unless the app is quitting. During a close attempt it is held until the attempt commits or is
/// cancelled, so `WindowClosing` always goes out before any `Closed` of the same window.
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
        let held = match s.attempts.get_mut(&entry.window) {
            Some(buffer) => {
                buffer.push(view);
                true
            }
            None => false,
        };
        Some((entry, root, empty, held))
    });
    let Some((entry, root, empty, held)) = removed else {
        return;
    };
    if let Some(browser) = &entry.browser {
        shared.registry.unbind_browser(browser.identifier());
    }
    if entry.counted {
        shared.live[Shared::live_index(entry.kind)].fetch_sub(1, Ordering::SeqCst);
    }
    shared.registry.remove_view(view);
    if !held && !quitting() {
        shared.emit(EngineEvent::Closed { view });
    }
    elog!("view closed view={view} window_empty={empty} held={held}");

    // Detach and close outside the callback, so CEF finishes this close first.
    let window_id = entry.window;
    let browser_view = entry.view;
    post_to_ui(Box::new(move || {
        if let (Some(root), Some(browser_view)) = (root, browser_view) {
            let mut handle = View::from(&browser_view);
            root.remove_child_view(Some(&mut handle));
        }
        // A close attempt in progress goes on with the browsers still open.
        if empty || held {
            close_window(window_id);
        }
        maybe_quit();
    }));
}

/// `on_render_process_terminated`: the renderer is gone. Report it, and close the browser so its
/// view leaves the layout. A crashed sidebar is not left behind next to its replacement. The close is
/// posted, not done inside the callback.
pub(crate) fn browser_crashed(view: ViewId) {
    let Some(shared) = shared() else { return };
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    if !quitting() {
        shared.emit(EngineEvent::Crashed { view });
    }
    match browser {
        Some(browser) => {
            // Marked like a tab close: without it the default `do_close` lets CEF Views close the
            // parent window, so a page that crashes its renderer would close the window.
            post_to_ui(Box::new(move || {
                with(|s| s.tab_closing.insert(browser.identifier()));
                close_browser(&browser, true);
            }));
        }
        None => elog!("crash without browser view={view}"),
    }
}

/// Closes the window's native window, if it still exists.
fn close_window(window: WindowId) {
    let handle = with(|s| s.windows.get(&window).and_then(|w| w.window.clone()));
    if let Some(handle) = handle {
        handle.close();
    }
}

/// `WindowDelegate::can_close`. The close has a commit point.
///
/// 1. A close attempt starts. Browsers are asked to close, content browsers first and the sidebar
///    last. A browser that does not close (its unload handler cancelled, or the close is pending)
///    ends the attempt: nothing is committed, the window stays open, and RPCs and commands keep
///    working. `Closed` events held during the attempt are then sent for the browsers that did close.
/// 2. Every browser agreed (or the close is forced). The commit: the engine enters the quitting
///    state, `WindowClosing` goes out first, creations still pending are cancelled, and the held
///    `Closed` events follow. The window then closes.
///
/// Once committed, the window closes without asking again.
pub(crate) fn can_close_window(window: WindowId) -> bool {
    CLOSE_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    let Some(shared) = shared() else { return true };
    if with(|s| s.committed.contains(&window)) {
        return true;
    }
    let forced = shared.force.load(Ordering::SeqCst);
    let browsers: Vec<Browser> = with(|s| {
        let Some(entry) = s.windows.get(&window) else {
            return Vec::new();
        };
        let mut found: Vec<(ViewKind, Browser)> = entry
            .views
            .iter()
            .filter_map(|id| {
                let view = s.views.get(id)?;
                Some((view.kind, view.browser.clone()?))
            })
            .collect();
        // Content browsers first: the sidebar is closed only after every content browser agreed.
        found.sort_by_key(|(kind, _)| *kind == ViewKind::Sidebar);
        found.into_iter().map(|(_, browser)| browser).collect()
    });
    with(|s| {
        s.attempts.entry(window).or_default();
    });
    let mut agreed = true;
    for browser in &browsers {
        if forced {
            close_browser(browser, true);
            continue;
        }
        let Some(host) = browser.host() else { continue };
        let closed = host.try_close_browser();
        elog!("try_close browser={} closed={closed}", browser.identifier());
        if closed == 0 {
            agreed = false;
            break;
        }
    }
    if agreed || forced {
        commit_close(window, &shared);
        return true;
    }
    // Not agreed: either a browser is still deciding (its beforeunload confirmation is pending) or
    // it stayed. The answer, not this return value, decides: `unload_answered` cancels the attempt
    // on "stay", and re-runs the close on "leave". The attempt stays open until one of those.
    false
}

/// True while a close attempt is in progress for the window of `browser`. A beforeunload prompt
/// that arrives while this is false belongs to a navigation or a reload, not to a close.
pub(crate) fn close_attempt_active(browser: i32) -> bool {
    let Some(shared) = shared() else {
        return false;
    };
    let Some((view, _)) = shared.registry.browser(browser) else {
        return false;
    };
    with(|s| {
        s.views
            .get(&view)
            .is_some_and(|v| s.attempts.contains_key(&v.window))
    })
}

/// A beforeunload confirmation was answered for `browser`. Only an answer that belongs to a close
/// attempt does anything: a page's own navigation or reload can raise the same prompt, and it must
/// never close the window. "Stay" cancels the close attempt of the browser's window: nothing has
/// committed, so the rest of the app keeps running, and the tabs that already closed in the attempt
/// are reported. "Leave" lets that browser close; the window's close runs again, so the browsers
/// still open are asked in turn.
pub(crate) fn unload_answered(browser: i32, allow: bool) {
    let Some(shared) = shared() else { return };
    let Some((view, _)) = shared.registry.browser(browser) else {
        return;
    };
    let Some(window) = with(|s| s.views.get(&view).map(|v| v.window)) else {
        return;
    };
    if !with(|s| s.attempts.contains_key(&window)) {
        return;
    }
    if allow {
        post_to_ui(Box::new(move || close_window(window)));
    } else {
        cancel_attempt(window, &shared);
    }
}

/// The commit point: every browser agreed to close.
fn commit_close(window: WindowId, shared: &Shared) {
    let held = with(|s| {
        s.committed.insert(window);
        s.attempts.remove(&window).unwrap_or_default()
    });
    shared.quitting.store(true, Ordering::SeqCst);
    shared.emit(EngineEvent::WindowClosing { window });
    elog!("window closing window={window}");
    cancel_pending(window, shared);
    for view in held {
        shared.emit(EngineEvent::Closed { view });
    }
}

/// A close that did not commit. The browsers that did close are reported, the rest keep running.
fn cancel_attempt(window: WindowId, shared: &Shared) {
    let held = with(|s| s.attempts.remove(&window).unwrap_or_default());
    elog!(
        "window close cancelled window={window} closed_views={}",
        held.len()
    );
    for view in held {
        shared.emit(EngineEvent::Closed { view });
    }
}

/// Cancels creation of a closing window's views. Queued creations are removed and reported as
/// `CreateFailed`, along with their queued commands. A view whose browser does not exist yet is
/// marked, and its browser is closed as soon as it appears.
fn cancel_pending(window: WindowId, shared: &Shared) {
    let failed = with(|s| {
        for entry in s.views.values_mut() {
            if entry.window == window && entry.browser.is_none() {
                entry.cancelled = true;
            }
        }
        let queued: Vec<Queued> = s.queue.drain(..).collect();
        let failed: Vec<ViewId> = queued
            .iter()
            .filter_map(|item| match &item.command {
                EngineCommand::CreateView {
                    view, window: w, ..
                } if *w == window => Some(*view),
                _ => None,
            })
            .collect();
        s.queue = queued
            .into_iter()
            .filter(|item| command_view(&item.command).is_none_or(|v| !failed.contains(&v)))
            .collect();
        failed
    });
    for view in failed {
        elog!("cancelled pending view={view} window={window}");
        shared.emit(EngineEvent::CreateFailed { view });
    }
}

fn command_view(command: &EngineCommand) -> Option<ViewId> {
    match command {
        EngineCommand::CreateView { view, .. }
        | EngineCommand::Show { view }
        | EngineCommand::Load { view, .. }
        | EngineCommand::Reload { view }
        | EngineCommand::Back { view }
        | EngineCommand::Forward { view }
        | EngineCommand::Close { view } => Some(*view),
        EngineCommand::CreateWindow { .. } => None,
    }
}

/// `WindowDelegate::on_window_destroyed`. Views of this window whose browser never existed are
/// dropped (nothing will report them). Views with a browser are still closing: the loop ends after
/// the last of them has reported `on_before_close`.
pub(crate) fn window_destroyed(window: WindowId) {
    let dropped = with(|s| {
        s.windows.remove(&window);
        s.closing.remove(&window);
        s.committed.remove(&window);
        s.attempts.remove(&window);
        let gone: Vec<ViewId> = s
            .views
            .iter()
            .filter(|(_, v)| v.window == window && v.browser.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            s.views.remove(id);
        }
        gone
    });
    if let Some(shared) = shared() {
        for view in dropped {
            shared.registry.remove_view(view);
        }
        shared.emit(EngineEvent::WindowClosed { window });
    }
    elog!("window closed window={window}");
    maybe_quit();
}

/// Ends the message loop once no window and no view is left. Called after the last window closes
/// and after the last `on_before_close`.
fn maybe_quit() {
    if with(|s| s.windows.is_empty() && s.views.is_empty()) {
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

/// Sends `script` to the view's main frame. UI thread.
pub(crate) fn push_script(view: ViewId, script: String) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    let Some(browser) = browser else {
        elog!("dropped push view={view} reason=no-browser");
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

/// A trusted left click at (x, y) in the view. CEF treats it as a user gesture, which a page needs
/// before `window.open` is allowed. Test builds only. UI thread.
#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub(crate) fn click(view: ViewId, x: i32, y: i32) {
    let browser = with(|s| s.views.get(&view).and_then(|entry| entry.browser.clone()));
    let Some(host) = browser.and_then(|b| b.host()) else {
        elog!("dropped click view={view} reason=no-browser");
        return;
    };
    let event = MouseEvent { x, y, modifiers: 0 };
    host.send_mouse_move_event(Some(&event), 0);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 0, 1);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 1, 1);
}
