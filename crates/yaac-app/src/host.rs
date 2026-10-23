//! Host wiring: the UI-thread main-thread hook, the UI assets and the start-up path for each mode.
//! Phase 2 replaces `start`'s shell and harness arms with the real core (see README).

use crate::harness::{self, Stages};
use crate::selftest;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::Handle;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::UnboundedReceiver;
use yaac_api::{ApiError, ApiResult, ErrorCode, Event, Request, Response};
use yaac_cef::{CefEngine, DirAssets, EmbeddedAssets, UiAssets, UiCaller};
use yaac_core::{CoreHandle, EngineEventSender, Principal, spawn};
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind};
use yaac_model::ViewId;
use yaac_platform::{MainThread, activate_app, install_app_menu};
use yaac_store::SqliteStore;

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
    if mode.is_harness() {
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
    /// `--persist-write`: real core and store; opens tabs through the RPC path, then quits.
    PersistWrite,
    /// `--persist-verify`: real core and store on the same data directory; checks the tabs came back.
    PersistVerify,
}

impl Mode {
    pub fn from_args(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut mode = Mode::Shell;
        for arg in args {
            match arg.as_str() {
                "--engine-harness" => mode = Mode::EngineHarness,
                "--rpc-harness" => mode = Mode::RpcHarness,
                "--persist-write" => mode = Mode::PersistWrite,
                "--persist-verify" => mode = Mode::PersistVerify,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(mode)
    }

    pub fn stages(self) -> Option<Stages> {
        match self {
            Mode::Shell | Mode::PersistWrite | Mode::PersistVerify => None,
            Mode::EngineHarness => Some(Stages::All),
            Mode::RpcHarness => Some(Stages::RpcOnly),
        }
    }

    /// The modes that run the engine against a stub, without the core.
    pub fn is_harness(self) -> bool {
        self.stages().is_some()
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
    data_dir: PathBuf,
    core_cell: Arc<OnceLock<CoreHandle>>,
) {
    CefMainThread.run(Box::new(|| {
        install_app_menu("yaac");
        activate_app();
    }));
    match mode.stages() {
        Some(stages) => {
            runtime.spawn(harness::run(stages, engine, events, calls));
        }
        None => start_core(mode, engine, runtime, events, data_dir, core_cell),
    }
}

/// Wraps the CEF engine for the core. It remembers which view the core created as the sidebar, so
/// events can be pushed to the right view, even after the core recreates it.
struct TrackingEngine {
    inner: Arc<CefEngine>,
    sidebar: Arc<Mutex<Option<ViewId>>>,
}

impl EngineHandle for TrackingEngine {
    fn send(&self, command: EngineCommand) {
        if let EngineCommand::CreateView { view, spec, .. } = &command
            && spec.kind == ViewKind::Sidebar
        {
            *self.sidebar.lock().expect("sidebar lock") = Some(*view);
        }
        self.inner.send(command);
    }
}

/// The RPC handler backed by the core. Requests arrive from the `app://ui` scheme with a caller the
/// engine authenticated by browser id. Subframe requests are refused: the UI hosts no subframes.
pub struct CoreRpc {
    pub core: Arc<OnceLock<CoreHandle>>,
}

impl yaac_cef::RpcHandler for CoreRpc {
    fn call(
        &self,
        caller: UiCaller,
        request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        let id = request.id;
        let core = self.core.get().cloned();
        Box::pin(async move {
            let refuse = |code, message: &str| Response {
                id,
                result: ApiResult::Err {
                    error: ApiError {
                        code,
                        message: message.into(),
                    },
                },
            };
            match core {
                None => refuse(ErrorCode::Internal, "the core is not running"),
                Some(_) if !caller.main_frame => refuse(
                    ErrorCode::Forbidden,
                    "only the main frame may call the core",
                ),
                Some(core) => {
                    core.request(Principal::Ui { view: caller.view }, request)
                        .await
                }
            }
        })
    }
}

fn start_core(
    mode: Mode,
    engine: Arc<CefEngine>,
    runtime: Handle,
    events: UnboundedReceiver<EngineEvent>,
    data_dir: PathBuf,
    core_cell: Arc<OnceLock<CoreHandle>>,
) {
    let store = match SqliteStore::open(&data_dir.join("yaac.db")) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("yaac-app: cannot open the database: {error}");
            engine.quit(1);
            return;
        }
    };
    let sidebar = Arc::new(Mutex::new(None));
    let tracking = Arc::new(TrackingEngine {
        inner: engine.clone(),
        sidebar: sidebar.clone(),
    });
    let (core, sender) = match spawn(Box::new(store), tracking) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("yaac-app: cannot start the core: {error}");
            engine.quit(1);
            return;
        }
    };
    let _ = core_cell.set(core.clone());
    println!("HOST core started data_dir={}", data_dir.display());
    runtime.spawn(forward_engine_events(events, sender));
    runtime.spawn(push_core_events(
        core.subscribe(),
        engine.clone(),
        sidebar.clone(),
    ));
    match mode {
        Mode::PersistWrite => {
            runtime.spawn(selftest::persist_write(core, sidebar, engine));
        }
        Mode::PersistVerify => {
            runtime.spawn(selftest::persist_verify(core, sidebar, engine));
        }
        _ => {}
    }
}

/// Engine callbacks reach the core here. Events produced before this task starts wait in the
/// channel, so none are lost between `spawn` returning and the forwarder running.
async fn forward_engine_events(
    mut events: UnboundedReceiver<EngineEvent>,
    sender: EngineEventSender,
) {
    while let Some(event) = events.recv().await {
        sender.send(event);
    }
}

/// Core events go to the sidebar as `window.yaacEvent(...)`. A lagging receiver skips ahead: the
/// next event makes the UI pull the whole snapshot anyway.
async fn push_core_events(
    mut events: tokio::sync::broadcast::Receiver<Event>,
    engine: Arc<CefEngine>,
    sidebar: Arc<Mutex<Option<ViewId>>>,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                let view = *sidebar.lock().expect("sidebar lock");
                if let Some(view) = view {
                    let _ = engine.push_ui_event(view, &event);
                }
            }
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => break,
        }
    }
}
