//! The engine harness (dev builds only). It drives `EngineHandle` directly with a stub core, checks
//! the `EngineEvent`s, the RPC calls and the local test site's reports, prints one `HARNESS` line
//! per step, and quits the engine with 0 only when every expectation held.
//!
//! Expectations for a step are checked from the last `mark`. Within one step, matches may arrive in
//! any order, and the returned matches are in arrival order.

use crate::site::Site;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{Instant, sleep, timeout_at};
use yaac_api::Request;
use yaac_cef::{CefEngine, UiCaller};
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind, ViewSpec, WindowSpec};
use yaac_model::{ProfileId, ViewId, WindowId};

pub const SIDEBAR_URL: &str = "app://ui/sidebar.html";
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// Titles a page may set while the title storm runs. Coalescing keeps this far below 100k.
const STORM_EVENT_LIMIT: usize = 2_000;
/// RSS growth allowed across the storm, in KiB.
const STORM_RSS_LIMIT_KIB: i64 = 200_000;

/// The harness's own pages, compiled in. Served only to the sidebar in harness runs.
static SIDEBAR_HTML: &[u8] = include_bytes!("../harness/sidebar.html");
static PROBE_HTML: &[u8] = include_bytes!("../harness/probe.html");
static SIDEBAR_PROBE_HTML: &[u8] = include_bytes!("../harness/sidebar-probe.html");
pub static HARNESS_FILES: &[(&str, &str, &[u8])] = &[
    ("sidebar.html", "text/html", SIDEBAR_HTML),
    ("probe.html", "text/html", PROBE_HTML),
    ("sidebar-probe.html", "text/html", SIDEBAR_PROBE_HTML),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stages {
    /// Views, popups, title storm, cookie isolation, remote-page probes, sidebar probes, RPC, close.
    All,
    /// Sidebar RPC, sidebar refusals, remote-page probes, close.
    RpcOnly,
    /// Creates a window and a view whose profile is still initializing, then quits at once. The
    /// process must exit promptly with the pending creation cancelled.
    QuitDuringCreate,
    /// A window with a sidebar and three content views: closing one tab (a middle one, one with an
    /// unload prompt) and then the last content view must not close the window.
    TabClose,
}

/// Every probe the remote page must report, by name. A missing name fails the E-IPC-1 stage, so a
/// probe that silently stops running cannot hide a regression.
const REMOTE_PROBES: &[&str] = &[
    "fetch-get-rpc",
    "fetch-post-text-rpc",
    "fetch-post-json-rpc",
    "fetch-post-nocors-rpc",
    "fetch-credentials-rpc",
    "fetch-sidebar-page",
    "xhr-post-text-rpc",
    "xhr-post-json-rpc",
    "xhr-get-sidebar-page",
    "form-post-rpc",
    "anchor-click-app",
    "window-open-app",
    "iframe-app-sidebar",
    "srcdoc-fetch-rpc",
    "data-iframe-fetch-rpc",
    "javascript-iframe-rpc",
    "blob-iframe-rpc",
    "iframe-redirect-app",
    "iframe-meta-refresh-app",
    "sw-register-fetch-rpc",
    "window-open-named-app",
    "anchor-blank-noopener-app",
    "link-prerender-app",
    "anchor-ping-app",
    "link-preload-header-app",
    "websocket-app",
    "request-navigate-app",
    "postmessage-sidebar",
];

/// Sidebar probes whose script is known to stall after a refused navigation: the attempt is still
/// required to be refused (no escape), and the stall is printed as a finding. Any other stall fails.
const KNOWN_SIDEBAR_STALLS: &[(&str, &str)] = &[
    (
        "iframe-http",
        "a refused subframe navigation leaves the probe script waiting",
    ),
    (
        "anchor-blank",
        "a target=_blank click from the UI page leaves the probe script waiting",
    ),
];

/// Percent-encodes an HTML page into a `data:` URL. Nothing leaves the machine.
pub fn data_url(html: &str) -> String {
    let mut out = String::from("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Runs the stages and quits the engine: exit 0 on pass, 1 on fail.
pub async fn run(
    stages: Stages,
    engine: Arc<CefEngine>,
    events: UnboundedReceiver<EngineEvent>,
    calls: UnboundedReceiver<(UiCaller, Request)>,
) {
    // Until the stages pass, a window that closes early is a failed run.
    engine.set_exit_code(1);
    if stages == Stages::TabClose {
        let outcome = match Site::start() {
            Ok(site) => {
                let mut harness = Harness {
                    engine: engine.clone(),
                    events,
                    history: Vec::new(),
                    cursor: 0,
                    calls,
                    calls_seen: Vec::new(),
                    site,
                };
                tab_close(&mut harness).await
            }
            Err(error) => Err(format!("site: {error}")),
        };
        match outcome {
            Ok(()) => {
                engine.set_exit_code(0);
                println!("HARNESS result=pass stages=TabClose");
                engine.quit(0);
            }
            Err(reason) => {
                println!("HARNESS result=fail reason={reason}");
                engine.quit(1);
            }
        }
        return;
    }
    if stages == Stages::QuitDuringCreate {
        let outcome = match Site::start() {
            Ok(site) => {
                let mut harness = Harness {
                    engine: engine.clone(),
                    events,
                    history: Vec::new(),
                    cursor: 0,
                    calls,
                    calls_seen: Vec::new(),
                    site,
                };
                quit_during_create(&mut harness).await
            }
            Err(error) => Err(format!("site: {error}")),
        };
        match outcome {
            Ok(()) => {
                engine.set_exit_code(0);
                println!("HARNESS result=pass stages={stages:?}");
            }
            Err(reason) => {
                println!("HARNESS result=fail reason={reason}");
                engine.quit(1);
            }
        }
        return;
    }
    let site = match Site::start() {
        Ok(site) => site,
        Err(error) => {
            println!("HARNESS result=fail reason=site: {error}");
            engine.quit(1);
            return;
        }
    };
    let mut harness = Harness {
        engine: engine.clone(),
        events,
        history: Vec::new(),
        cursor: 0,
        calls,
        calls_seen: Vec::new(),
        site,
    };
    match harness.stages(stages).await {
        Ok(()) => {
            println!("HARNESS result=pass stages={stages:?}");
            engine.quit(0);
        }
        Err(reason) => {
            println!("HARNESS result=fail reason={reason}");
            engine.quit(1);
        }
    }
}

/// Closing tabs must not close the window. The window has a sidebar and three content views; the
/// middle one has an unload prompt. Closing it reports `Closed` only; the window stays, the other
/// views and the sidebar keep working, and no close attempt is entered. Then the last content view
/// is closed: the sidebar remains, so the window stays open too.
async fn tab_close(harness: &mut Harness) -> Result<(), String> {
    let window = WindowId::new();
    let sidebar = ViewId(1);
    let first = ViewId(2);
    let middle = ViewId(3);
    let last = ViewId(4);
    let page_first = data_url("<title>tab-first</title><p>first</p>");
    let page_last = data_url("<title>tab-last</title><p>last</p>");
    let page_middle = data_url(
        "<title>tab-middle</title><body style=\"margin:0\"><script>window.onbeforeunload = function () { return 'stay?'; };</script><div style=\"width:100vw;height:100vh\"></div></body>",
    );
    harness.mark();
    harness.send(EngineCommand::CreateWindow {
        window,
        spec: WindowSpec {
            title: "tabs".into(),
            width: 1000,
            height: 640,
        },
    });
    harness.send(EngineCommand::CreateView {
        view: sidebar,
        window,
        spec: ViewSpec {
            kind: ViewKind::Sidebar,
            profile: ProfileId::new(),
            url: SIDEBAR_URL.into(),
        },
    });
    let profile = ProfileId::new();
    for (view, url) in [
        (first, page_first.clone()),
        (middle, page_middle.clone()),
        (last, page_last.clone()),
    ] {
        harness.send(EngineCommand::CreateView {
            view,
            window,
            spec: ViewSpec {
                kind: ViewKind::Content,
                profile,
                url,
            },
        });
    }
    harness.send(EngineCommand::Show { view: first });
    harness
        .expect(
            "tabs-created",
            vec![
                ("created sidebar", created_pred(sidebar)),
                ("created first", created_pred(first)),
                ("created middle", created_pred(middle)),
                ("created last", created_pred(last)),
                ("title middle", title_pred(middle, "tab-middle")),
            ],
        )
        .await?;
    harness.engine.click(middle, 100, 100);
    sleep(Duration::from_millis(300)).await;
    println!("HARNESS step=tabs-created result=ok");

    // Close the middle tab (it has an unload prompt, answered by the policy: allow).
    let attempts = yaac_cef::test_hooks::window_close_attempts();
    harness.mark();
    harness.send(EngineCommand::Close { view: middle });
    harness
        .expect_one("close-middle", "middle closed", closed_pred(middle))
        .await?;
    sleep(Duration::from_millis(1000)).await;
    harness.drain_events();
    let since = harness
        .history
        .iter()
        .skip(harness.cursor)
        .filter(|e| {
            matches!(
                e,
                EngineEvent::WindowClosing { .. } | EngineEvent::WindowClosed { .. }
            )
        })
        .count();
    let entered = yaac_cef::test_hooks::window_close_attempts() - attempts;
    println!("HARNESS step=close-middle window_events={since} close_attempts={entered}");
    if since > 0 {
        return Err(format!(
            "closing a middle tab closed the window ({since} window events)"
        ));
    }
    if entered > 0 {
        return Err(format!(
            "closing a tab entered {entered} window close attempt(s)"
        ));
    }

    // The other views are untouched and the sidebar still answers.
    harness.mark();
    harness.send(EngineCommand::Load {
        view: first,
        url: data_url("<title>first again</title>"),
    });
    harness
        .expect_one(
            "after-close-load",
            "first still loads",
            title_pred(first, "first again"),
        )
        .await?;
    harness
        .reload_sidebar_and_wait_for_rpc(sidebar, "after-close-rpc")
        .await?;
    println!("HARNESS step=tab-middle-after result=ok");

    // Close the last content view. The sidebar remains, so the window stays open.
    let attempts = yaac_cef::test_hooks::window_close_attempts();
    harness.mark();
    harness.send(EngineCommand::Close { view: last });
    harness
        .expect_one("close-last", "last closed", closed_pred(last))
        .await?;
    sleep(Duration::from_millis(1000)).await;
    harness.drain_events();
    let since = harness
        .history
        .iter()
        .skip(harness.cursor)
        .filter(|e| {
            matches!(
                e,
                EngineEvent::WindowClosing { .. } | EngineEvent::WindowClosed { .. }
            )
        })
        .count();
    let entered = yaac_cef::test_hooks::window_close_attempts() - attempts;
    println!("HARNESS step=close-last window_events={since} close_attempts={entered}");
    if since > 0 || entered > 0 {
        return Err(format!(
            "closing the last content view closed the window (events {since}, attempts {entered})"
        ));
    }
    harness
        .reload_sidebar_and_wait_for_rpc(sidebar, "sidebar-alive")
        .await?;
    println!("HARNESS step=tab-close result=ok");
    Ok(())
}

/// Quit while a view is still being created. The profile is held not ready (a test hook), so the
/// `CreateView`s stay queued when the window is asked to close. The commit must cancel them: each
/// reports `CreateFailed`, `WindowClosing` goes out first, `WindowClosed` last, and the process
/// exits. A failed expectation fails the run.
async fn quit_during_create(harness: &mut Harness) -> Result<(), String> {
    let window = WindowId::new();
    let sidebar = ViewId(1);
    let content = ViewId(2);
    yaac_cef::test_hooks::delay_profile_readiness(Duration::from_secs(3));
    harness.mark();
    harness.send(EngineCommand::CreateWindow {
        window,
        spec: WindowSpec {
            title: "quit harness".into(),
            width: 800,
            height: 500,
        },
    });
    harness.send(EngineCommand::CreateView {
        view: sidebar,
        window,
        spec: ViewSpec {
            kind: ViewKind::Sidebar,
            profile: ProfileId::new(),
            url: SIDEBAR_URL.into(),
        },
    });
    harness.send(EngineCommand::CreateView {
        view: content,
        window,
        spec: ViewSpec {
            kind: ViewKind::Content,
            profile: ProfileId::new(),
            url: data_url("<title>pending</title>"),
        },
    });
    // The pass code is set before the close: the loop can end as soon as WindowClosed is sent.
    harness.engine.set_exit_code(0);
    harness.engine.close_windows();
    println!("HARNESS step=quit-during-create requested");
    let matched = harness
        .expect(
            "quit-during-create",
            vec![
                ("window closing", window_closing_pred(window)),
                ("create failed sidebar", create_failed_pred(sidebar)),
                ("create failed content", create_failed_pred(content)),
                ("window closed", window_closed_pred(window)),
            ],
        )
        .await?;
    let first = matched.first();
    let last = matched.last();
    if !matches!(first, Some(EngineEvent::WindowClosing { .. })) {
        return Err(format!("WindowClosing was not first: {matched:?}"));
    }
    if !matches!(last, Some(EngineEvent::WindowClosed { .. })) {
        return Err(format!("WindowClosed was not last: {matched:?}"));
    }
    println!("HARNESS step=quit-during-create result=ok cancelled=2");
    Ok(())
}

struct Harness {
    engine: Arc<CefEngine>,
    events: UnboundedReceiver<EngineEvent>,
    history: Vec<EngineEvent>,
    cursor: usize,
    calls: UnboundedReceiver<(UiCaller, Request)>,
    calls_seen: Vec<UiCaller>,
    site: Site,
}

type Pred = Box<dyn Fn(&EngineEvent) -> bool + Send>;

impl Harness {
    fn send(&self, command: EngineCommand) {
        self.engine.send(command);
    }

    fn record(&mut self, event: EngineEvent) {
        // Titles can be long; the harness log shows a prefix only.
        match &event {
            EngineEvent::TitleChanged {
                view,
                nav_gen,
                title,
            } => {
                let shown: String = title.chars().take(40).collect();
                println!(
                    "HARNESS event TitleChanged view={view} nav_gen={nav_gen} title={shown:?}"
                );
            }
            other => println!("HARNESS event {other:?}"),
        }
        self.history.push(event);
    }

    /// Everything after this point is what the next expectations look at.
    fn mark(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.record(event);
        }
        self.cursor = self.history.len();
    }

    /// Waits until each predicate has matched an event since the mark, within `limit`. Returns the
    /// matched events in arrival order.
    async fn expect_within(
        &mut self,
        step: &str,
        limit: Duration,
        preds: Vec<(&str, Pred)>,
    ) -> Result<Vec<EngineEvent>, String> {
        let deadline = Instant::now() + limit;
        loop {
            let mut found: Vec<Option<usize>> = vec![None; preds.len()];
            for index in self.cursor..self.history.len() {
                for (slot, (_, pred)) in found.iter_mut().zip(preds.iter()) {
                    if slot.is_none() && pred(&self.history[index]) {
                        *slot = Some(index);
                    }
                }
            }
            if found.iter().all(Option::is_some) {
                let mut matched: Vec<usize> = found.iter().flatten().copied().collect();
                matched.sort_unstable();
                self.cursor = self.history.len();
                let events: Vec<EngineEvent> =
                    matched.iter().map(|i| self.history[*i].clone()).collect();
                for event in &events {
                    println!("HARNESS match step={step} event={event:?}");
                }
                return Ok(events);
            }
            match timeout_at(deadline, self.events.recv()).await {
                Ok(Some(event)) => self.record(event),
                Ok(None) => return Err(format!("{step}: event channel closed")),
                Err(_) => {
                    let missing: Vec<&str> = found
                        .iter()
                        .zip(preds.iter())
                        .filter(|(slot, _)| slot.is_none())
                        .map(|(_, (name, _))| *name)
                        .collect();
                    return Err(format!("{step}: timed out waiting for {missing:?}"));
                }
            }
        }
    }

    async fn expect(
        &mut self,
        step: &str,
        preds: Vec<(&str, Pred)>,
    ) -> Result<Vec<EngineEvent>, String> {
        self.expect_within(step, STEP_TIMEOUT, preds).await
    }

    /// Reloads the sidebar and waits until the engine has answered one more RPC from it. The title is
    /// not a reliable signal here: the coalescer drops a title that ends where the last one did.
    async fn reload_sidebar_and_wait_for_rpc(
        &mut self,
        sidebar: ViewId,
        step: &str,
    ) -> Result<(), String> {
        let before = self.engine.rpc_answered();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: SIDEBAR_URL.into(),
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if self.engine.rpc_answered() > before {
                return Ok(());
            }
            sleep(Duration::from_millis(50)).await;
        }
        Err(format!(
            "{step}: the sidebar's RPC was not answered after a reload"
        ))
    }

    async fn expect_one(
        &mut self,
        step: &str,
        name: &str,
        pred: Pred,
    ) -> Result<EngineEvent, String> {
        let mut matched = self.expect(step, vec![(name, pred)]).await?;
        Ok(matched.remove(0))
    }

    /// Waits until a site report named `name` has arrived.
    async fn report(&mut self, step: &str, name: &str, limit: Duration) -> Result<String, String> {
        let deadline = Instant::now() + limit;
        loop {
            self.drain_events();
            if let Some(value) = self.site.report(name) {
                println!("HARNESS report step={step} name={name} result={value}");
                return Ok(value);
            }
            if Instant::now() >= deadline {
                return Err(format!("{step}: no report {name}"));
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    /// Waits for an RPC call from `view`. Calls are recorded so later checks can see them.
    async fn rpc_from(&mut self, step: &str, view: ViewId) -> Result<UiCaller, String> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            if let Some(caller) = self.calls_seen.iter().copied().find(|c| c.view == view) {
                println!("HARNESS rpc step={step} caller={caller:?}");
                return Ok(caller);
            }
            match timeout_at(deadline, self.calls.recv()).await {
                Ok(Some((caller, request))) => {
                    println!("HARNESS rpc received caller={caller:?} id={}", request.id);
                    self.calls_seen.push(caller);
                }
                Ok(None) => return Err(format!("{step}: rpc channel closed")),
                Err(_) => return Err(format!("{step}: no rpc call from {view}")),
            }
        }
    }

    /// Records every call that has arrived.
    fn drain_calls(&mut self) {
        while let Ok((caller, request)) = self.calls.try_recv() {
            println!("HARNESS rpc received caller={caller:?} id={}", request.id);
            self.calls_seen.push(caller);
        }
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.record(event);
        }
    }

    async fn stages(&mut self, stages: Stages) -> Result<(), String> {
        let all = stages == Stages::All;
        let window = WindowId::new();
        let sidebar = ViewId(1);
        let first = ViewId(2);
        let second = ViewId(3);
        let profile_ui = ProfileId::new();
        let profile_a = ProfileId::new();
        let page_a = data_url("<title>Page A</title><p>page a</p>");
        let page_b = data_url("<title>Page B</title><p>page b</p>");
        let page_c = data_url("<title>Page C</title><p>page c</p>");

        // Views.
        self.mark();
        self.send(EngineCommand::CreateWindow {
            window,
            spec: WindowSpec {
                title: "yaac harness".into(),
                width: 1000,
                height: 640,
            },
        });
        self.send(EngineCommand::CreateView {
            view: sidebar,
            window,
            spec: ViewSpec {
                kind: ViewKind::Sidebar,
                profile: profile_ui,
                url: SIDEBAR_URL.into(),
            },
        });
        if all {
            self.send(EngineCommand::CreateView {
                view: first,
                window,
                spec: ViewSpec {
                    kind: ViewKind::Content,
                    profile: profile_a,
                    url: page_a,
                },
            });
        }
        self.send(EngineCommand::CreateView {
            view: second,
            window,
            spec: ViewSpec {
                kind: ViewKind::Content,
                profile: profile_a,
                url: page_b.clone(),
            },
        });
        let shown_first = if all { first } else { second };
        self.send(EngineCommand::Show { view: shown_first });
        let mut created = vec![
            ("created sidebar", created_pred(sidebar)),
            ("created second", created_pred(second)),
        ];
        if all {
            created.push(("created first", created_pred(first)));
            created.push(("title first", title_pred(first, "Page A")));
        }
        created.push(("title second", title_pred(second, "Page B")));
        self.expect("views-created", created).await?;
        println!("HARNESS step=views-created result=ok");

        if all {
            self.swap_and_navigate(window, first, second, &page_b, &page_c)
                .await?;
            self.popup(second).await?;
            self.title_storm(second).await?;
            self.isolation(window, first, second, profile_a).await?;
        }

        // RPC: the sidebar's RPC ran on load and its reply reached the page.
        let caller = self.rpc_from("sidebar-rpc", sidebar).await?;
        if caller.view != sidebar || !caller.main_frame {
            return Err(format!("sidebar rpc caller mismatch: {caller:?}"));
        }
        self.seen_title(sidebar, "rpc-ok").await?;
        println!("HARNESS step=sidebar-rpc result=ok");

        self.sidebar_refusals(sidebar).await?;
        self.remote_probes(second).await?;
        self.sidebar_probes(sidebar).await?;
        if all {
            self.cancelled_close(sidebar, second).await?;
        }

        // Close: closing the window's views commits the window close. The engine reports
        // `WindowClosing` first and `WindowClosed` last; the views' own `Closed` events are held
        // back once the close is committed. The process can exit as soon as `WindowClosed` is sent,
        // so the pass code is set before the close: lifecycle events reach this task through the
        // engine's 50 ms flush, and the message loop can end before they arrive. A failure after
        // this point is reported by the `HARNESS result=fail` line (and fails any run that
        // requires `result=pass`), not by the exit code.
        self.mark();
        self.engine.set_exit_code(0);
        // A window close, as the red button or Cmd-Q makes it. Closing views one by one is a tab
        // close and leaves the window open.
        self.engine.close_windows();
        let first = self
            .expect(
                "close",
                vec![("window closing", window_closing_pred(window))],
            )
            .await?;
        if !matches!(first.last(), Some(EngineEvent::WindowClosing { .. })) {
            return Err(format!("WindowClosing was not seen: {first:?}"));
        }
        let last = self
            .expect(
                "window closed",
                vec![("window closed", window_closed_pred(window))],
            )
            .await?;
        if !matches!(last.last(), Some(EngineEvent::WindowClosed { .. })) {
            return Err(format!("WindowClosed was not last: {last:?}"));
        }
        println!("HARNESS step=close result=ok");
        Ok(())
    }

    /// A window close that the page cancels. The content page holds a beforeunload confirmation (set
    /// up after a trusted click, so Chromium asks). The harness answers the confirmation with "stay"
    /// once. The close must stop at the commit point: no `WindowClosing`, no `Closed`, the sidebar
    /// keeps answering RPCs, and the content view keeps taking commands. A later close, with the
    /// confirmation allowed, goes through.
    ///
    /// The first content view has no confirmation. It is closed beforehand, so the cancelled attempt
    /// has nothing to report. (A tab without a confirmation that closes before a cancelling one is
    /// reported as closed; see the README.)
    async fn cancelled_close(&mut self, sidebar: ViewId, second: ViewId) -> Result<(), String> {
        let page = data_url(
            "<title>unload</title><body style=\"margin:0\"><script>window.onbeforeunload = function () { return 'stay?'; };</script><div style=\"width:100vw;height:100vh\"></div></body>",
        );
        self.mark();
        self.send(EngineCommand::Load {
            view: second,
            url: page,
        });
        self.expect_one(
            "unload-page",
            "unload page loaded",
            title_pred(second, "unload"),
        )
        .await?;
        sleep(Duration::from_millis(300)).await;
        self.engine.click(second, 100, 100);
        sleep(Duration::from_millis(300)).await;

        self.drain_events();
        let start = self.history.len();
        yaac_cef::test_hooks::cancel_next_unloads(1);
        self.engine.close_windows();
        sleep(Duration::from_millis(2000)).await;
        self.drain_events();
        let leaked: Vec<String> = self.history[start..]
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EngineEvent::WindowClosing { .. }
                        | EngineEvent::Closed { .. }
                        | EngineEvent::WindowClosed { .. }
                        | EngineEvent::CreateFailed { .. }
                        | EngineEvent::Crashed { .. }
                )
            })
            .map(|e| format!("{e:?}"))
            .collect();
        if !leaked.is_empty() {
            return Err(format!(
                "cancelled close still emitted lifecycle events: {leaked:?}"
            ));
        }

        self.mark();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: SIDEBAR_URL.into(),
        });
        self.expect_within(
            "cancelled-close-rpc",
            Duration::from_secs(5),
            vec![("sidebar rpc after cancel", title_pred(sidebar, "rpc-ok"))],
        )
        .await?;
        self.mark();
        self.send(EngineCommand::Load {
            view: second,
            url: data_url("<title>after cancel</title>"),
        });
        self.expect_one(
            "cancelled-close-content",
            "content view still takes commands",
            title_pred(second, "after cancel"),
        )
        .await?;
        println!(
            "HARNESS step=cancelled-close result=ok window_closing=none closed_events=none rpc=ok content=ok"
        );
        Ok(())
    }

    async fn seen_title(&mut self, view: ViewId, title: &'static str) -> Result<(), String> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            self.drain_events();
            if self.history.iter().any(|e| title_pred(view, title)(e)) {
                println!("HARNESS seen view={view} title={title}");
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("no title {title} from {view}"));
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    async fn swap_and_navigate(
        &mut self,
        _window: WindowId,
        first: ViewId,
        second: ViewId,
        page_b: &str,
        page_c: &str,
    ) -> Result<(), String> {
        // Swap: the first view hides and the second shows. The first must stay alive.
        self.mark();
        self.send(EngineCommand::Show { view: second });
        sleep(Duration::from_millis(400)).await;
        println!("HARNESS step=show-second result=ok");

        // Navigate the visible view. The title of the load carries the load's generation.
        self.mark();
        self.send(EngineCommand::Load {
            view: second,
            url: page_c.to_string(),
        });
        let nav = self
            .expect_one(
                "navigate",
                "navigated to C",
                navigated_exact(second, page_c.to_string()),
            )
            .await?;
        let EngineEvent::Navigated { nav_gen, .. } = nav else {
            return Err("not a navigation".into());
        };
        self.expect_one("navigate", "title C", title_at(second, "Page C", nav_gen))
            .await?;
        println!("HARNESS step=navigate result=ok nav_gen={nav_gen}");

        // Back to B.
        self.mark();
        self.send(EngineCommand::Back { view: second });
        let back = self
            .expect_one(
                "back",
                "navigated back to B",
                navigated_exact(second, page_b.to_string()),
            )
            .await?;
        let EngineEvent::Navigated { nav_gen, .. } = back else {
            return Err("not a navigation".into());
        };
        self.expect_one("back", "title B", title_at(second, "Page B", nav_gen))
            .await?;
        println!("HARNESS step=back result=ok");

        // The hidden first view is still alive: reload it.
        self.mark();
        self.send(EngineCommand::Reload { view: first });
        self.expect_one("reload-hidden", "reloaded first", navigated_for(first))
            .await?;
        println!("HARNESS step=reload-hidden result=ok");
        Ok(())
    }

    async fn popup(&mut self, second: ViewId) -> Result<(), String> {
        // A script-only window.open has no user gesture, so Chromium's popup blocker stops it before
        // the engine sees it. The link is clicked with a trusted mouse event instead.
        self.mark();
        let target = data_url("<title>popup-target</title>");
        let host = data_url(&format!(
            "<title>popup-host</title><a href=\"{target}\" target=_blank \
             style=\"position:fixed;left:0;top:0;width:300px;height:200px;display:block;background:yellow\">open</a>"
        ));
        self.send(EngineCommand::Load {
            view: second,
            url: host,
        });
        self.expect_one(
            "popup-page",
            "popup host loaded",
            title_pred(second, "popup-host"),
        )
        .await?;
        sleep(Duration::from_millis(300)).await;
        self.mark();
        click_for_popup(&self.engine, second);
        let popup = self
            .expect_one("popup", "popup requested by second", popup_from(second))
            .await?;
        println!("HARNESS step=popup result=ok event={popup:?}");
        Ok(())
    }

    /// 100k title changes in one page. Coalescing keeps the events the core sees small, memory does
    /// not grow with the count, and a later RPC still answers promptly.
    async fn title_storm(&mut self, second: ViewId) -> Result<(), String> {
        let rss_before = rss_kib();
        self.mark();
        let storm = data_url(
            "<title>begin</title><script>for (let i = 0; i < 100000; i++) { document.title = 't' + i; }\
             document.title = 'storm-' + 'x'.repeat(2000);</script>",
        );
        self.send(EngineCommand::Load {
            view: second,
            url: storm,
        });
        let mark = self.history.len();
        self.expect_one("storm", "storm finished", title_starts(second, "storm-"))
            .await?;
        let titles = self.history[mark..]
            .iter()
            .filter(|e| matches!(e, EngineEvent::TitleChanged { view, .. } if *view == second))
            .count();
        let final_title = self
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                EngineEvent::TitleChanged { view, title, .. } if *view == second => {
                    Some(title.clone())
                }
                _ => None,
            })
            .unwrap_or_default();
        if titles > STORM_EVENT_LIMIT {
            return Err(format!(
                "title storm: {titles} title events reached the core"
            ));
        }
        if final_title.chars().count() > 512 {
            return Err(format!(
                "title storm: title not capped ({} chars)",
                final_title.chars().count()
            ));
        }
        let rss_after = rss_kib();
        if let (Some(before), Some(after)) = (rss_before, rss_after)
            && after - before > STORM_RSS_LIMIT_KIB
        {
            return Err(format!("title storm: RSS grew by {} KiB", after - before));
        }
        // A later RPC answers promptly.
        let started = Instant::now();
        self.mark();
        self.send(EngineCommand::Load {
            view: ViewId(1),
            url: SIDEBAR_URL.to_string(),
        });
        self.expect_within(
            "storm-rpc",
            Duration::from_secs(2),
            vec![("sidebar rpc after storm", title_pred(ViewId(1), "rpc-ok"))],
        )
        .await?;
        println!(
            "HARNESS step=title-storm result=ok title_events={titles} title_chars={} rss_kib={:?}->{:?} rpc_after_ms={}",
            final_title.chars().count(),
            rss_before,
            rss_after,
            started.elapsed().as_millis()
        );
        Ok(())
    }

    /// A7: a cookie set in profile A is sent by another view of A, not by a view of B, and
    /// localStorage written in A is not visible in B.
    async fn isolation(
        &mut self,
        window: WindowId,
        first: ViewId,
        second: ViewId,
        profile_a: ProfileId,
    ) -> Result<(), String> {
        let iso_b = ViewId(4);
        let profile_b = ProfileId::new();
        let base = format!("http://127.0.0.1:{}", self.site.port);
        self.mark();
        self.send(EngineCommand::Load {
            view: second,
            url: format!("{base}/set-cookie"),
        });
        self.expect_one(
            "a7-set",
            "A stores cookie and localStorage",
            title_pred(second, "stored"),
        )
        .await?;
        self.mark();
        self.send(EngineCommand::CreateView {
            view: iso_b,
            window,
            spec: ViewSpec {
                kind: ViewKind::Content,
                profile: profile_b,
                url: format!("{base}/echo?who=B"),
            },
        });
        self.expect_one("a7-b", "B view created", created_pred(iso_b))
            .await?;
        self.report("a7-b", "ls-B", STEP_TIMEOUT).await?;
        self.mark();
        self.send(EngineCommand::Load {
            view: first,
            url: format!("{base}/echo?who=A2"),
        });
        self.expect_one("a7-a2", "second A view loaded", title_pred(first, "echo"))
            .await?;
        self.report("a7-a2", "ls-A2", STEP_TIMEOUT).await?;
        let _ = profile_a;

        let b_cookie = self.site.cookie_for("B").unwrap_or_default();
        let a_cookie = self.site.cookie_for("A2").unwrap_or_default();
        let b_storage = self.site.report("ls-B").unwrap_or_default();
        let a_storage = self.site.report("ls-A2").unwrap_or_default();
        let ok = !b_cookie.contains("yaac_probe=alpha")
            && a_cookie.contains("yaac_probe=alpha")
            && b_storage == "null"
            && a_storage == "alpha";
        println!(
            "HARNESS step=a7-isolation result={} cookie_b={:?} cookie_a2={:?} localstorage_b={b_storage} localstorage_a2={a_storage}",
            if ok { "ok" } else { "fail" },
            b_cookie,
            a_cookie
        );
        if !ok {
            return Err("A7: profile isolation failed".into());
        }
        Ok(())
    }

    /// E-IPC-1, from a remote page in a content view. Every attempt reports its outcome; the harness
    /// checks that the page ran, that the RPC handler never saw a content caller, that the engine
    /// refused requests, and that no content view navigated into app://.
    async fn remote_probes(&mut self, second: ViewId) -> Result<(), String> {
        let before = self.engine.rejections();
        self.drain_calls();
        self.mark();
        let url = format!("http://127.0.0.1:{}/remote", self.site.port);
        self.send(EngineCommand::Load { view: second, url });
        self.report("e-ipc-1", "e-ipc-done", Duration::from_secs(60))
            .await?;
        let missing: Vec<&str> = REMOTE_PROBES
            .iter()
            .copied()
            .filter(|name| self.site.report(name).is_none())
            .collect();
        for name in REMOTE_PROBES {
            if let Some(result) = self.site.report(name) {
                println!("HARNESS probe name={name} result={result}");
            }
        }
        if !missing.is_empty() {
            return Err(format!("E-IPC-1: probes did not report: {missing:?}"));
        }
        sleep(Duration::from_millis(1500)).await;
        self.drain_calls();
        let content_calls: Vec<&UiCaller> = self
            .calls_seen
            .iter()
            .filter(|c| c.view != ViewId(1))
            .collect();
        let refused = self.engine.rejections() - before;
        let app_nav = self.history[self.cursor..].iter().any(|e| {
            matches!(e, EngineEvent::Navigated { view, url, .. } if *view == second && url.starts_with("app:"))
        });
        let attempts = self.site.reports().len();
        println!(
            "HARNESS step=e-ipc-1 attempts={attempts} content_rpc_calls={} rejections={refused} app_navigation={app_nav}",
            content_calls.len()
        );
        if !content_calls.is_empty() {
            return Err(format!(
                "E-IPC-1: the RPC handler saw {} content calls",
                content_calls.len()
            ));
        }
        if refused == 0 {
            return Err("E-IPC-1: the engine refused nothing".into());
        }
        if app_nav {
            return Err("E-IPC-1: a content view navigated into app://".into());
        }
        Ok(())
    }

    /// Refusals on the sidebar side: a text/plain POST and an OPTIONS request from the sidebar's
    /// own origin. Expect 415 and 405, and two counted rejections.
    async fn sidebar_refusals(&mut self, sidebar: ViewId) -> Result<(), String> {
        let before = self.engine.rejections();
        self.mark();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: "app://ui/probe.html".into(),
        });
        self.expect_one(
            "sidebar-probe",
            "probe result",
            title_pred(sidebar, "probe 415 405"),
        )
        .await?;
        let counted = self.engine.rejections() - before;
        if counted < 2 {
            return Err(format!(
                "sidebar probe: expected 2 rejections, engine counted {counted}"
            ));
        }
        println!("HARNESS step=sidebar-refusals result=ok status=415,405 rejections={counted}");
        self.mark();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: SIDEBAR_URL.into(),
        });
        self.expect_one(
            "sidebar-reload",
            "sidebar rpc again",
            title_pred(sidebar, "rpc-ok"),
        )
        .await?;
        Ok(())
    }

    /// The sidebar page tries to navigate itself to forbidden targets, one attempt per load. Each
    /// attempt must be refused: the sidebar never leaves app://ui/, and every attempt counts as a
    /// rejection.
    async fn sidebar_probes(&mut self, sidebar: ViewId) -> Result<(), String> {
        const KINDS: [&str; 6] = [
            "location-http",
            "window-open-http",
            "iframe-http",
            "anchor-blank",
            "location-data",
            "location-javascript",
        ];
        let before = self.engine.rejections();
        self.drain_calls();
        let mut finished = 0usize;
        for kind in KINDS {
            self.mark();
            let url = format!("app://ui/sidebar-probe.html?kind={kind}");
            self.send(EngineCommand::Load { view: sidebar, url });
            let done = format!("sb-done {kind}");
            let outcome = self
                .expect_within(
                    "sidebar-probes",
                    Duration::from_secs(10),
                    vec![("attempt finished", title_pred_owned(sidebar, done))],
                )
                .await;
            // A subframe navigation that is refused can leave the probe page without finishing its
            // script (observed with an iframe). That is recorded as a finding, not a pass. The page
            // is reloaded before the next attempt.
            let completed = outcome.is_ok();
            if completed {
                finished += 1;
            } else if !KNOWN_SIDEBAR_STALLS.iter().any(|(name, _)| *name == kind) {
                return Err(format!(
                    "sidebar probe {kind} stalled and is not a known stall"
                ));
            }
            let escaped = self.history[self.cursor..].iter().any(|e| {
                matches!(e, EngineEvent::Navigated { view, url, .. } if *view == sidebar && !url.starts_with("app://ui/"))
            });
            println!(
                "HARNESS step=sidebar-probe kind={kind} completed={completed} escaped={escaped}"
            );
            if escaped {
                return Err(format!("sidebar navigated away from app://ui/ ({kind})"));
            }
        }
        let refused = self.engine.rejections() - before;
        println!(
            "HARNESS step=sidebar-probes completed={finished}/{} rejections={refused}",
            KINDS.len()
        );
        // Stalled probe scripts are findings, reported above. The requirements are: no attempt left
        // app://ui/ (checked per attempt), and the engine refused at least one of them.
        if refused == 0 {
            return Err("sidebar probes: the engine refused nothing".into());
        }
        self.mark();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: SIDEBAR_URL.into(),
        });
        self.expect_one(
            "sidebar-back",
            "sidebar rpc after probes",
            title_pred(sidebar, "rpc-ok"),
        )
        .await?;
        Ok(())
    }
}

fn title_pred_owned(view: ViewId, expected: String) -> Pred {
    Box::new(
        move |e| matches!(e, EngineEvent::TitleChanged { view: v, title, .. } if *v == view && *title == expected),
    )
}

/// Resident memory of this process in KiB, from `ps`. None if it cannot be read.
fn rss_kib() -> Option<i64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// A trusted click at the popup link. Compiled only with the test entry point.
fn click_for_popup(engine: &CefEngine, view: ViewId) {
    engine.click(view, 100, 100);
}

fn created_pred(view: ViewId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::Created { view: v } if *v == view))
}

fn closed_pred(view: ViewId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::Closed { view: v } if *v == view))
}

fn window_closed_pred(window: WindowId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::WindowClosed { window: w } if *w == window))
}

fn title_pred(view: ViewId, expected: &'static str) -> Pred {
    Box::new(
        move |e| matches!(e, EngineEvent::TitleChanged { view: v, title, .. } if *v == view && title == expected),
    )
}

fn title_starts(view: ViewId, prefix: &'static str) -> Pred {
    Box::new(
        move |e| matches!(e, EngineEvent::TitleChanged { view: v, title, .. } if *v == view && title.starts_with(prefix)),
    )
}

fn title_at(view: ViewId, expected: &'static str, generation: u64) -> Pred {
    Box::new(move |e| {
        matches!(e, EngineEvent::TitleChanged { view: v, title, nav_gen }
            if *v == view && title == expected && *nav_gen == generation)
    })
}

fn navigated_exact(view: ViewId, url: String) -> Pred {
    Box::new(
        move |e| matches!(e, EngineEvent::Navigated { view: v, url: u, .. } if *v == view && *u == url),
    )
}

fn navigated_for(view: ViewId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::Navigated { view: v, .. } if *v == view))
}

fn popup_from(opener: ViewId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::PopupRequested { opener: o, .. } if *o == opener))
}

/// Stub RPC handler for the harness. Records each call and answers `ok`. The core is not involved.
pub struct StubRpc {
    pub calls: tokio::sync::mpsc::UnboundedSender<(UiCaller, Request)>,
}

impl yaac_cef::RpcHandler for StubRpc {
    fn call(
        &self,
        caller: UiCaller,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = yaac_api::Response> + Send>> {
        let id = request.id;
        let _ = self.calls.send((caller, request));
        Box::pin(async move {
            yaac_api::Response {
                id,
                result: yaac_api::ApiResult::Ok {
                    body: yaac_api::ResponseBody::Ack { rev: 0 },
                },
            }
        })
    }
}

fn window_closing_pred(window: WindowId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::WindowClosing { window: w } if *w == window))
}

fn create_failed_pred(view: ViewId) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::CreateFailed { view: v } if *v == view))
}
