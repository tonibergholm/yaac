//! Host wiring: the UI-thread main-thread hook, the UI assets and the start-up path for each mode.
//! Phase 2 replaces `start`'s shell and harness arms with the real core (see README).

use crate::harness::{self, Stages};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;
use yaac_api::{Request, Response};
use yaac_cef::{CefEngine, DirAssets, EmbeddedAssets, UiAssets, UiCaller};
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind, ViewSpec, WindowSpec};
use yaac_model::{ProfileId, ViewId, WindowId};
use yaac_platform::{MainThread, activate_app, install_app_menu};

/// The sidebar placeholder and nothing else. Used by the harness, and by the shell when the UI
/// directory has no `sidebar.html`.
static SIDEBAR_HTML: &[u8] = include_bytes!("../harness/sidebar.html");
static PROBE_HTML: &[u8] = include_bytes!("../harness/probe.html");
pub static HARNESS_FILES: &[(&str, &str, &[u8])] = &[
    ("sidebar.html", "text/html", SIDEBAR_HTML),
    ("probe.html", "text/html", PROBE_HTML),
];

/// Engine-side main-thread hook, backed by CEF's UI-thread task queue.
pub struct CefMainThread;

impl MainThread for CefMainThread {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        yaac_cef::post_to_ui(job);
    }
}

/// Where the UI files come from. `YAAC_UI_DIR` wins; otherwise the workspace `ui/dist`. The harness
/// always uses its own placeholder, so it does not depend on the UI build.
pub fn ui_assets(mode: Mode) -> Arc<dyn UiAssets> {
    if mode != Mode::Shell {
        return Arc::new(EmbeddedAssets::new(HARNESS_FILES));
    }
    let dir = std::env::var_os("YAAC_UI_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/dist"))
        });
    if dir.join("sidebar.html").is_file() {
        println!("HOST ui-assets source=dir path={}", dir.display());
        Arc::new(DirAssets::new(dir))
    } else {
        println!(
            "HOST ui-assets source=placeholder reason=no-sidebar-in-{}",
            dir.display()
        );
        Arc::new(EmbeddedAssets::new(HARNESS_FILES))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No flag: open one window and keep it until it closes.
    Shell,
    /// `--engine-harness`: every stage.
    EngineHarness,
    /// `--rpc-harness`: the RPC stage only.
    RpcHarness,
}

impl Mode {
    pub fn from_args(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut mode = Mode::Shell;
        for arg in args {
            match arg.as_str() {
                "--engine-harness" => mode = Mode::EngineHarness,
                "--rpc-harness" => mode = Mode::RpcHarness,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(mode)
    }

    pub fn stages(self) -> Option<Stages> {
        match self {
            Mode::Shell => None,
            Mode::EngineHarness => Some(Stages::All),
            Mode::RpcHarness => Some(Stages::RpcOnly),
        }
    }
}

/// Stub RPC handler for the harness. Records each call and answers `ok`.
pub struct StubRpc {
    pub calls: tokio::sync::mpsc::UnboundedSender<(UiCaller, Request)>,
}

impl yaac_cef::RpcHandler for StubRpc {
    fn call(
        &self,
        caller: UiCaller,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        let id = request.id;
        let _ = self.calls.send((caller, request));
        Box::pin(async move {
            Response {
                id,
                result: yaac_api::ApiResult::Ok {
                    body: yaac_api::ResponseBody::Ack { rev: 0 },
                },
            }
        })
    }
}

/// Runs once CEF is initialized, on the UI thread. Must not block.
pub fn start(
    mode: Mode,
    engine: Arc<CefEngine>,
    runtime: Handle,
    events: UnboundedReceiver<EngineEvent>,
    calls: UnboundedReceiver<(UiCaller, Request)>,
) {
    CefMainThread.run(Box::new(|| {
        install_app_menu("yaac");
        activate_app();
    }));
    match mode.stages() {
        None => open_shell_window(&engine),
        Some(stages) => {
            runtime.spawn(harness::run(stages, engine, events, calls));
        }
    }
}

/// The shell window: sidebar and one content view, the content view shown. Closing the window
/// quits the app.
fn open_shell_window(engine: &CefEngine) {
    let window = WindowId::new();
    let sidebar = ViewId(1);
    let content = ViewId(2);
    engine.send(EngineCommand::CreateWindow {
        window,
        spec: WindowSpec {
            title: "yaac".into(),
            width: 1000,
            height: 640,
        },
    });
    engine.send(EngineCommand::CreateView {
        view: sidebar,
        window,
        spec: ViewSpec {
            kind: ViewKind::Sidebar,
            profile: ProfileId::new(),
            url: harness::SIDEBAR_URL.into(),
        },
    });
    engine.send(EngineCommand::CreateView {
        view: content,
        window,
        spec: ViewSpec {
            kind: ViewKind::Content,
            profile: ProfileId::new(),
            url: harness::data_url("<title>New tab</title><p>New tab</p>"),
        },
    });
    engine.send(EngineCommand::Show { view: content });
}

/// RPC handler for the shell until the core is wired. Answers every request with an error.
pub struct NoCore;

impl yaac_cef::RpcHandler for NoCore {
    fn call(
        &self,
        _caller: UiCaller,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        let id = request.id;
        Box::pin(async move {
            Response {
                id,
                result: yaac_api::ApiResult::Err {
                    error: yaac_api::ApiError {
                        code: yaac_api::ErrorCode::Internal,
                        message: "the core is not wired in phase 1".into(),
                    },
                },
            }
        })
    }
}
