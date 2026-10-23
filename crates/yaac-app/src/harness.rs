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
}

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
    if stages == Stages::QuitDuringCreate {
        quit_during_create(&engine);
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

/// A view created on a profile that is still initializing, then a quit. The pending creation must be
/// cancelled and the window closed without waiting for it.
fn quit_during_create(engine: &CefEngine) {
    let window = WindowId::new();
    engine.send(EngineCommand::CreateWindow {
        window,
        spec: WindowSpec {
            title: "quit harness".into(),
            width: 800,
            height: 500,
        },
    });
    engine.send(EngineCommand::CreateView {
        view: ViewId(1),
        window,
        spec: ViewSpec {
            kind: ViewKind::Sidebar,
            profile: ProfileId::new(),
            url: SIDEBAR_URL.into(),
        },
    });
    engine.send(EngineCommand::CreateView {
        view: ViewId(2),
        window,
        spec: ViewSpec {
            kind: ViewKind::Content,
            profile: ProfileId::new(),
            url: data_url("<title>pending</title>"),
        },
    });
    println!("HARNESS step=quit-during-create requested");
    engine.quit(0);
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
        println!("HARNESS event {event:?}");
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
    async fn expect_within(&mut self, step: &str, limit: Duration, preds: Vec<(&str, Pred)>) -> Result<Vec<EngineEvent>, String> {
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
                let events: Vec<EngineEvent> = matched.iter().map(|i| self.history[*i].clone()).collect();
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

    async fn expect(&mut self, step: &str, preds: Vec<(&str, Pred)>) -> Result<Vec<EngineEvent>, String> {
        self.expect_within(step, STEP_TIMEOUT, preds).await
    }

    async fn expect_one(&mut self, step: &str, name: &str, pred: Pred) -> Result<EngineEvent, String> {
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
            spec: WindowSpec { title: "yaac harness".into(), width: 1000, height: 640 },
        });
        self.send(EngineCommand::CreateView {
            view: sidebar,
            window,
            spec: ViewSpec { kind: ViewKind::Sidebar, profile: profile_ui, url: SIDEBAR_URL.into() },
        });
        if all {
            self.send(EngineCommand::CreateView {
                view: first,
                window,
                spec: ViewSpec { kind: ViewKind::Content, profile: profile_a, url: page_a },
            });
        }
        self.send(EngineCommand::CreateView {
            view: second,
            window,
            spec: ViewSpec { kind: ViewKind::Content, profile: profile_a, url: page_b.clone() },
        });
        let shown_first = if all { first } else { second };
        self.send(EngineCommand::Show { view: shown_first });
        let mut created = vec![("created sidebar", created_pred(sidebar)), ("created second", created_pred(second))];
        if all {
            created.push(("created first", created_pred(first)));
            created.push(("title first", title_pred(first, "Page A")));
        }
        created.push(("title second", title_pred(second, "Page B")));
        self.expect("views-created", created).await?;
        println!("HARNESS step=views-created result=ok");

        if all {
            self.swap_and_navigate(window, first, second, &page_b, &page_c).await?;
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

        // Close: asynchronous. Each view reports Closed; the window reports WindowClosed last.
        self.mark();
        let mut closes = Vec::new();
        if all {
            self.send(EngineCommand::Close { view: first });
            closes.push(("closed first", closed_pred(first)));
        }
        self.send(EngineCommand::Close { view: second });
        self.send(EngineCommand::Close { view: sidebar });
        closes.push(("closed second", closed_pred(second)));
        closes.push(("closed sidebar", closed_pred(sidebar)));
        if all {
            // The isolation views are closed with the rest.
            let iso_b = ViewId(4);
            self.send(EngineCommand::Close { view: iso_b });
            closes.push(("closed iso_b", closed_pred(iso_b)));
        }
        closes.push(("window closed", window_closed_pred(window)));
        let matched = self.expect("close", closes).await?;
        match matched.last() {
            Some(EngineEvent::WindowClosed { .. }) => {}
            other => return Err(format!("WindowClosed was not last: {other:?}")),
        }
        println!("HARNESS step=close result=ok");
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

    async fn swap_and_navigate(&mut self, _window: WindowId, first: ViewId, second: ViewId, page_b: &str, page_c: &str) -> Result<(), String> {
        // Swap: the first view hides and the second shows. The first must stay alive.
        self.mark();
        self.send(EngineCommand::Show { view: second });
        sleep(Duration::from_millis(400)).await;
        println!("HARNESS step=show-second result=ok");

        // Navigate the visible view. The title of the load carries the load's generation.
        self.mark();
        self.send(EngineCommand::Load { view: second, url: page_c.to_string() });
        let nav = self
            .expect_one("navigate", "navigated to C", navigated_exact(second, page_c.to_string()))
            .await?;
        let EngineEvent::Navigated { nav_gen, .. } = nav else { return Err("not a navigation".into()) };
        self.expect_one("navigate", "title C", title_at(second, "Page C", nav_gen)).await?;
        println!("HARNESS step=navigate result=ok nav_gen={nav_gen}");

        // Back to B.
        self.mark();
        self.send(EngineCommand::Back { view: second });
        let back = self
            .expect_one("back", "navigated back to B", navigated_exact(second, page_b.to_string()))
            .await?;
        let EngineEvent::Navigated { nav_gen, .. } = back else { return Err("not a navigation".into()) };
        self.expect_one("back", "title B", title_at(second, "Page B", nav_gen)).await?;
        println!("HARNESS step=back result=ok");

        // The hidden first view is still alive: reload it.
        self.mark();
        self.send(EngineCommand::Reload { view: first });
        self.expect_one("reload-hidden", "reloaded first", navigated_for(first)).await?;
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
        self.send(EngineCommand::Load { view: second, url: host });
        self.expect_one("popup-page", "popup host loaded", title_pred(second, "popup-host")).await?;
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
        let storm = data_url(&format!(
            "<title>begin</title><script>for (let i = 0; i < 100000; i++) {{ document.title = 't' + i; }}\
             document.title = 'storm-' + 'x'.repeat(2000);</script>"
        ));
        self.send(EngineCommand::Load { view: second, url: storm });
        self.expect_one("storm", "storm finished", title_starts(second, "storm-")).await?;
        let titles = self
            .history
            .iter()
            .skip(self.cursor)
            .filter(|e| matches!(e, EngineEvent::TitleChanged { view, .. } if *view == second))
            .count();
        let final_title = self
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                EngineEvent::TitleChanged { view, title, .. } if *view == second => Some(title.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if titles > STORM_EVENT_LIMIT {
            return Err(format!("title storm: {titles} title events reached the core"));
        }
        if final_title.chars().count() > 512 {
            return Err(format!("title storm: title not capped ({} chars)", final_title.chars().count()));
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
        self.send(EngineCommand::Load { view: ViewId(1), url: SIDEBAR_URL.to_string() });
        self.expect_within("storm-rpc", Duration::from_secs(2), vec![("sidebar rpc after storm", title_pred(ViewId(1), "rpc-ok"))])
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
    async fn isolation(&mut self, window: WindowId, first: ViewId, second: ViewId, profile_a: ProfileId) -> Result<(), String> {
        let iso_b = ViewId(4);
        let profile_b = ProfileId::new();
        let base = format!("http://127.0.0.1:{}", self.site.port);
        self.mark();
        self.send(EngineCommand::Load { view: second, url: format!("{base}/set-cookie") });
        self.expect_one("a7-set", "A stores cookie and localStorage", title_pred(second, "stored")).await?;
        self.mark();
        self.send(EngineCommand::CreateView {
            view: iso_b,
            window,
            spec: ViewSpec { kind: ViewKind::Content, profile: profile_b, url: format!("{base}/echo?who=B") },
        });
        self.expect_one("a7-b", "B view created", created_pred(iso_b)).await?;
        self.report("a7-b", "ls-B", STEP_TIMEOUT).await?;
        self.mark();
        self.send(EngineCommand::Load { view: first, url: format!("{base}/echo?who=A2") });
        self.expect_one("a7-a2", "second A view loaded", title_pred(first, "echo")).await?;
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
        self.report("e-ipc-1", "e-ipc-done", Duration::from_secs(60)).await?;
        sleep(Duration::from_millis(1500)).await;
        self.drain_calls();
        let content_calls: Vec<&UiCaller> = self.calls_seen.iter().filter(|c| c.view != ViewId(1)).collect();
        let refused = self.engine.rejections() - before;
        let app_nav = self.history[self.cursor..].iter().any(|e| {
            matches!(e, EngineEvent::Navigated { view, url, .. } if *view == second && url.starts_with("app:"))
        });
        let attempts = self.site.reports().len();
        println!("HARNESS step=e-ipc-1 attempts={attempts} content_rpc_calls={} rejections={refused} app_navigation={app_nav}", content_calls.len());
        if !content_calls.is_empty() {
            return Err(format!("E-IPC-1: the RPC handler saw {} content calls", content_calls.len()));
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
        self.send(EngineCommand::Load { view: sidebar, url: "app://ui/probe.html".into() });
        self.expect_one("sidebar-probe", "probe result", title_pred(sidebar, "probe 415 405")).await?;
        let counted = self.engine.rejections() - before;
        if counted < 2 {
            return Err(format!("sidebar probe: expected 2 rejections, engine counted {counted}"));
        }
        println!("HARNESS step=sidebar-refusals result=ok status=415,405 rejections={counted}");
        self.mark();
        self.send(EngineCommand::Load { view: sidebar, url: SIDEBAR_URL.into() });
        self.expect_one("sidebar-reload", "sidebar rpc again", title_pred(sidebar, "rpc-ok")).await?;
        Ok(())
    }

    /// The sidebar page tries to navigate itself to forbidden targets. Each attempt must be refused,
    /// so the sidebar never leaves app://ui/.
    async fn sidebar_probes(&mut self, sidebar: ViewId) -> Result<(), String> {
        let before = self.engine.rejections();
        self.drain_calls();
        self.mark();
        let url = format!("app://ui/sidebar-probe.html?port={}", self.site.port);
        self.send(EngineCommand::Load { view: sidebar, url });
        self.report("sidebar-probes", "sb-done", Duration::from_secs(30)).await?;
        sleep(Duration::from_millis(1000)).await;
        let escaped = self.history[self.cursor..].iter().any(|e| {
            matches!(e, EngineEvent::Navigated { view, url, .. } if *view == sidebar && !url.starts_with("app://ui/"))
        });
        let refused = self.engine.rejections() - before;
        println!("HARNESS step=sidebar-probes escaped={escaped} rejections={refused}");
        if escaped {
            return Err("sidebar navigated away from app://ui/".into());
        }
        if refused == 0 {
            return Err("sidebar probes: the engine refused nothing".into());
        }
        self.mark();
        self.send(EngineCommand::Load { view: sidebar, url: SIDEBAR_URL.into() });
        self.expect_one("sidebar-back", "sidebar rpc after probes", title_pred(sidebar, "rpc-ok")).await?;
        Ok(())
    }
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
    Box::new(move |e| matches!(e, EngineEvent::TitleChanged { view: v, title, .. } if *v == view && title == expected))
}

fn title_starts(view: ViewId, prefix: &'static str) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::TitleChanged { view: v, title, .. } if *v == view && title.starts_with(prefix)))
}

fn title_at(view: ViewId, expected: &'static str, generation: u64) -> Pred {
    Box::new(move |e| {
        matches!(e, EngineEvent::TitleChanged { view: v, title, nav_gen }
            if *v == view && title == expected && *nav_gen == generation)
    })
}

fn navigated_exact(view: ViewId, url: String) -> Pred {
    Box::new(move |e| matches!(e, EngineEvent::Navigated { view: v, url: u, .. } if *v == view && *u == url))
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
