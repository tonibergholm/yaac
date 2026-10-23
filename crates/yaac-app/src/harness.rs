//! The engine harness. Drives `EngineHandle` directly (no core), checks the `EngineEvent`s and the
//! RPC calls, prints one `HARNESS` line per step, and quits the engine with 0 only when every
//! expectation held.
//!
//! Expectations are checked from the last `mark`. Within one step, matches may arrive in any
//! order, and the returned matches are in arrival order. Facts that are not tied to a step (the
//! sidebar's RPC reply) are looked up in the whole history.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{Instant, sleep, timeout_at};
use yaac_api::Request;
use yaac_cef::{CefEngine, UiCaller};
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind, ViewSpec, WindowSpec};
use yaac_model::{ProfileId, ViewId, WindowId};

pub const SIDEBAR_URL: &str = "app://ui/sidebar.html";
const RPC_URL: &str = "app://ui/api/v1/rpc";
const UI_ORIGIN: &str = "app://ui";
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stages {
    /// Views, swap, navigation, popup, RPC and close.
    All,
    /// Sidebar and one content view: RPC and close only.
    RpcOnly,
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
    let mut harness = Harness {
        engine: engine.clone(),
        events,
        history: Vec::new(),
        cursor: 0,
        calls,
        calls_seen: Vec::new(),
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

struct Harness {
    engine: Arc<CefEngine>,
    events: UnboundedReceiver<EngineEvent>,
    history: Vec<EngineEvent>,
    cursor: usize,
    calls: UnboundedReceiver<(UiCaller, Request)>,
    calls_seen: Vec<UiCaller>,
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

    /// Waits until each predicate has matched an event since the mark. Returns the matched events
    /// in arrival order.
    async fn expect(
        &mut self,
        step: &str,
        preds: Vec<(&str, Pred)>,
    ) -> Result<Vec<EngineEvent>, String> {
        let deadline = Instant::now() + STEP_TIMEOUT;
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

    async fn expect_one(
        &mut self,
        step: &str,
        name: &str,
        pred: Pred,
    ) -> Result<EngineEvent, String> {
        let mut matched = self.expect(step, vec![(name, pred)]).await?;
        Ok(matched.remove(0))
    }

    /// Waits until `pred` matches anything in the whole history, including before the mark.
    async fn seen(&mut self, step: &str, pred: Pred) -> Result<(), String> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            if let Some(event) = self.history.iter().find(|e| pred(e)) {
                println!("HARNESS seen step={step} event={event:?}");
                return Ok(());
            }
            match timeout_at(deadline, self.events.recv()).await {
                Ok(Some(event)) => self.record(event),
                Ok(None) => return Err(format!("{step}: event channel closed")),
                Err(_) => return Err(format!("{step}: timed out")),
            }
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

    /// Records any calls that have arrived.
    fn drain_calls(&mut self) {
        while let Ok((caller, request)) = self.calls.try_recv() {
            println!("HARNESS rpc received caller={caller:?} id={}", request.id);
            self.calls_seen.push(caller);
        }
    }

    async fn stages(&mut self, stages: Stages) -> Result<(), String> {
        let all = stages == Stages::All;
        let window = WindowId::new();
        let sidebar = ViewId(1);
        let first = ViewId(2);
        let second = ViewId(3);
        let profile_ui = ProfileId::new();
        let profile_web = ProfileId::new();
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
                    profile: profile_web,
                    url: page_a,
                },
            });
        }
        self.send(EngineCommand::CreateView {
            view: second,
            window,
            spec: ViewSpec {
                kind: ViewKind::Content,
                profile: profile_web,
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
            // Swap: the first view hides and the second shows. The first must stay alive.
            self.mark();
            self.send(EngineCommand::Show { view: second });
            sleep(Duration::from_millis(400)).await;
            println!("HARNESS step=show-second result=ok");

            // Navigate the visible view. The title of the load carries the load's generation.
            self.mark();
            self.send(EngineCommand::Load {
                view: second,
                url: page_c.clone(),
            });
            let nav = self
                .expect_one(
                    "navigate",
                    "navigated to C",
                    navigated_exact(second, page_c),
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
                    navigated_exact(second, page_b),
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

            // Popup: the page opens a target=_blank link and calls window.open. The engine denies
            // both and reports the request.
            // A script-only window.open has no user gesture, so Chromium's popup blocker stops it
            // before the engine sees it. The link is clicked with a trusted mouse event instead.
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
            self.engine.click(second, 100, 100);
            let popup = self
                .expect_one("popup", "popup requested by second", popup_from(second))
                .await?;
            println!("HARNESS step=popup result=ok event={popup:?}");
        }

        // RPC. The sidebar's RPC ran on load. Its reply reached the page, which set the title.
        let caller = self.rpc_from("sidebar-rpc", sidebar).await?;
        if caller.view != sidebar || !caller.main_frame {
            return Err(format!("sidebar rpc caller mismatch: {caller:?}"));
        }
        self.seen("sidebar-rpc-reply", title_any(sidebar, "rpc-ok"))
            .await?;
        println!("HARNESS step=sidebar-rpc result=ok");
        // For a screenshot: keep the window as it is now (sidebar shows the RPC reply).
        if let Some(ms) = std::env::var("YAAC_HARNESS_HOLD_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            println!("HARNESS hold ms={ms}");
            sleep(Duration::from_millis(ms)).await;
        }

        // Refusals on the sidebar side: the probe page (same origin) sends a text/plain POST and
        // an OPTIONS request. Expect 415 and 405, and two rejections counted by the engine.
        let before = self.engine.rejections();
        self.mark();
        self.send(EngineCommand::Load {
            view: sidebar,
            url: format!("{UI_ORIGIN}/probe.html"),
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
        // Back to the sidebar page for the rest of the run.
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

        // A content view POSTs to the UI endpoint (text/plain, so there is no preflight). The
        // engine answers 403 with no CORS headers, so the fetch rejects. The page then tries to
        // navigate to app://ui; that navigation is refused.
        self.drain_calls();
        let before = self.engine.rejections();
        self.mark();
        // no-cors: the request reaches the engine (a CORS fetch from an opaque origin would be
        // stopped by the browser before the scheme handler). The response is opaque, so the page
        // only learns that the request was sent.
        let probe = data_url(&format!(
            "<title>probe</title><script>fetch('{RPC_URL}',{{method:'POST',mode:'no-cors',body:'{{}}'}})\
             .then(function(){{document.title='rpc-sent'}})\
             .catch(function(){{document.title='rpc-failed'}});\
             setTimeout(function(){{location.href='app://ui/sidebar.html'}},800);</script>"
        ));
        self.send(EngineCommand::Load {
            view: second,
            url: probe,
        });
        self.expect_one(
            "content-rpc",
            "content rpc sent",
            title_pred(second, "rpc-sent"),
        )
        .await?;
        sleep(Duration::from_millis(1500)).await;
        self.drain_calls();
        if self.calls_seen.iter().any(|c| c.view == second) {
            return Err("a content view reached the rpc handler".into());
        }
        let app_navigation = self.history[self.cursor..].iter().any(|e| {
            matches!(e, EngineEvent::Navigated { view, url, .. } if *view == second && url.starts_with("app:"))
        });
        if app_navigation {
            return Err("a content view navigated to app://".into());
        }
        let counted = self.engine.rejections() - before;
        if counted < 1 {
            return Err("content probe: engine counted no rejection".into());
        }
        println!("HARNESS step=content-rpc-refused result=ok rejections={counted}");
        println!("HARNESS step=content-app-navigation-refused result=ok");

        // Close is asynchronous. Each view reports Closed; the window reports WindowClosed last.
        self.mark();
        if all {
            self.send(EngineCommand::Close { view: first });
        }
        self.send(EngineCommand::Close { view: second });
        self.send(EngineCommand::Close { view: sidebar });
        let mut closes = vec![
            ("closed second", closed_pred(second)),
            ("closed sidebar", closed_pred(sidebar)),
        ];
        if all {
            closes.push(("closed first", closed_pred(first)));
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

fn title_any(view: ViewId, expected: &'static str) -> Pred {
    title_pred(view, expected)
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
    Box::new(move |e| {
        matches!(e, EngineEvent::PopupRequested { opener: o, url, .. }
            if *o == opener && url.contains("popup-target") || (*o == opener && url.starts_with("data:")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_are_percent_encoded() {
        assert_eq!(data_url("<a>"), "data:text/html;charset=utf-8,%3Ca%3E");
    }
}
