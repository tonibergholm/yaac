//! Host wiring: the main-thread hook, the UI assets, and the start and shutdown of the core.
//!
//! `start` runs on the CEF UI thread and returns at once. The store is opened and the core spawned
//! on a runtime blocking thread. RPCs that arrive before the core is ready get a retriable error.

use crate::Mode;
#[cfg(yaac_dev)]
use crate::harness;
#[cfg(yaac_dev)]
use crate::selftest;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::UnboundedReceiver;
use yaac_api::{ApiError, ApiResult, ErrorCode, Event, Request, Response};
use yaac_cef::{CefEngine, UiAssets, UiCaller};
use yaac_core::{CoreHandle, EngineEventSender, Principal, spawn};
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind};
use yaac_model::ViewId;
use yaac_platform::{MainThread, activate_app, install_app_menu};
use yaac_store::SqliteStore;

/// How long the shutdown waits for in-flight RPCs, and for the runtime's tasks.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Engine-side main-thread hook, backed by CEF's UI-thread task queue.
pub struct CefMainThread;

impl MainThread for CefMainThread {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        yaac_cef::post_to_ui(job);
    }
}

/// Where the UI files come from.
///
/// Release builds serve only the copy compiled into the binary. Development builds use `YAAC_UI_DIR`
/// or the workspace `ui/dist`, and fall back to the harness placeholder when there is no build.
pub fn ui_assets() -> Arc<dyn UiAssets> {
    #[cfg(not(yaac_dev))]
    {
        match yaac_cef::embedded_ui() {
            Some(assets) => assets,
            None => panic!("this build has no embedded UI"),
        }
    }
    #[cfg(yaac_dev)]
    {
        let dir = std::env::var_os("YAAC_UI_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/dist"))
            });
        if dir.join("sidebar.html").is_file() {
            println!("HOST ui-assets source=dir");
            Arc::new(yaac_cef::DirAssets::new(dir))
        } else {
            println!("HOST ui-assets source=placeholder");
            Arc::new(yaac_cef::EmbeddedAssets::new(harness::HARNESS_FILES))
        }
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
                // Retriable: the UI retries its snapshot pull with backoff.
                None => refuse(ErrorCode::Internal, "the core is starting; retry"),
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

/// Runs once CEF is initialized, on the UI thread. Must not block.
pub fn start(
    mode: Mode,
    engine: Arc<CefEngine>,
    runtime: Handle,
    events: UnboundedReceiver<EngineEvent>,
    #[cfg(yaac_dev)] calls: tokio::sync::mpsc::UnboundedReceiver<(UiCaller, Request)>,
    data_dir: PathBuf,
    core_cell: Arc<OnceLock<CoreHandle>>,
) {
    CefMainThread.run(Box::new(|| {
        install_app_menu("yaac");
        activate_app();
    }));
    #[cfg(yaac_dev)]
    if let Some(stages) = mode.stages() {
        runtime.spawn(harness::run(stages, engine, events, calls));
        return;
    }
    start_core(mode, engine, runtime, events, data_dir, core_cell);
}

fn start_core(
    mode: Mode,
    engine: Arc<CefEngine>,
    runtime: Handle,
    events: UnboundedReceiver<EngineEvent>,
    data_dir: PathBuf,
    core_cell: Arc<OnceLock<CoreHandle>>,
) {
    #[cfg(not(yaac_dev))]
    let _ = mode;
    let blocking = runtime.clone();
    // The store and the core start off the UI thread: opening SQLite must not stall the UI.
    runtime.spawn_blocking(move || {
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
        if engine.is_quitting() {
            // The window closed while the core was starting. Stop it again at once.
            core.shutdown();
            return;
        }
        blocking.spawn(forward_engine_events(events, sender));
        blocking.spawn(push_core_events(
            core.subscribe(),
            engine.clone(),
            sidebar.clone(),
        ));
        #[cfg(yaac_dev)]
        match mode {
            Mode::PersistWrite => {
                blocking.spawn(selftest::persist_write(core, sidebar, engine));
            }
            Mode::PersistVerify => {
                blocking.spawn(selftest::persist_verify(core, sidebar, engine));
            }
            _ => {}
        }
        #[cfg(not(yaac_dev))]
        let _ = (mode, sidebar);
    });
}

/// The shutdown sequence after the message loop has ended, on the UI thread: stop the core (which
/// flushes its writes and joins its threads), wait a bounded time for in-flight RPCs, then stop the
/// runtime. CEF's own shutdown follows in `run_browser`.
pub fn shutdown(engine: Arc<CefEngine>, core_cell: Arc<OnceLock<CoreHandle>>, runtime: Runtime) {
    if let Some(core) = core_cell.get() {
        core.shutdown();
    }
    let drained = engine.wait_rpc_idle(SHUTDOWN_WAIT);
    println!("HOST shutdown rpc_drained={drained}");
    runtime.shutdown_timeout(SHUTDOWN_WAIT);
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
