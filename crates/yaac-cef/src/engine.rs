//! `CefEngine`: the `Send + Sync` command proxy. It holds ids and plain data only. Every command
//! becomes a task on the CEF UI thread, where `ui` executes it.

use crate::assets::UiAssets;
use crate::registry::Registry;
use crate::rpc::RpcHandler;
use crate::ui;
use cef::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;
use yaac_api::Event;
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind};
use yaac_model::ViewId;

pub type Job = Box<dyn FnOnce() + Send>;

/// What the host supplies before the engine starts.
pub struct EngineSettings {
    /// Root for CEF's caches. Each profile gets `<data_dir>/cef/profiles/<profile id>`.
    pub data_dir: PathBuf,
    /// CEF's own log file.
    pub log_file: PathBuf,
    /// Files behind `app://ui/`.
    pub ui_assets: Arc<dyn UiAssets>,
    /// Answers `POST app://ui/api/v1/rpc` from UI browsers.
    pub rpc: Arc<dyn RpcHandler>,
    /// Runs the RPC futures. Must be a multi-thread or current-thread runtime that stays alive
    /// while the engine runs.
    pub rpc_runtime: tokio::runtime::Handle,
    /// Engine callbacks, sent without blocking the UI thread.
    pub events: UnboundedSender<EngineEvent>,
}

/// State shared by the UI thread, the IO thread (scheme handler) and the RPC runtime.
pub(crate) struct Shared {
    pub(crate) events: UnboundedSender<EngineEvent>,
    pub(crate) assets: Arc<dyn UiAssets>,
    pub(crate) rpc: Arc<dyn RpcHandler>,
    pub(crate) rpc_runtime: tokio::runtime::Handle,
    pub(crate) registry: Registry,
    pub(crate) data_dir: PathBuf,
    pub(crate) exit_code: AtomicI32,
    /// Requests the scheme handler refused or failed (status 4xx). Read by tests.
    pub(crate) rejections: AtomicU64,
}

impl Shared {
    pub(crate) fn emit(&self, event: EngineEvent) {
        // The receiver lives as long as the host. A send error means the host is shutting down.
        let _ = self.events.send(event);
    }
}

static GLOBAL: OnceLock<Arc<Shared>> = OnceLock::new();

/// Cheap to clone. Commands run on the UI thread, in the order they were sent.
#[derive(Clone)]
pub struct CefEngine {
    shared: Arc<Shared>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PushError {
    #[error("view {0} is not a UI view")]
    NotUiView(ViewId),
}

impl CefEngine {
    pub(crate) fn new(settings: EngineSettings) -> Self {
        let shared = Arc::new(Shared {
            events: settings.events,
            assets: settings.ui_assets,
            rpc: settings.rpc,
            rpc_runtime: settings.rpc_runtime,
            registry: Registry::default(),
            data_dir: settings.data_dir,
            exit_code: AtomicI32::new(0),
            rejections: AtomicU64::new(0),
        });
        let _ = GLOBAL.set(shared.clone());
        Self { shared }
    }

    pub(crate) fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    /// Closes every window. The message loop ends when the last window has closed, and the
    /// process exits with `code`.
    pub fn quit(&self, code: i32) {
        self.shared.exit_code.store(code, Ordering::SeqCst);
        request_quit();
    }

    /// How many `app://ui` requests have been refused or failed so far.
    pub fn rejections(&self) -> u64 {
        self.shared.rejections.load(Ordering::SeqCst)
    }

    /// Clicks at (x, y) in a view with a trusted mouse event. A user gesture, for tests.
    pub fn click(&self, view: ViewId, x: i32, y: i32) {
        post_to_ui(Box::new(move || ui::click(view, x, y)));
    }

    /// Pushes an event to a UI view as `window.yaacEvent(<json>)`. Only `ViewKind::Sidebar` views
    /// receive pushes. Returns once the push is queued on the UI thread.
    pub fn push_ui_event(&self, view: ViewId, event: &Event) -> Result<(), PushError> {
        if self.shared.registry.view_kind(view) != Some(ViewKind::Sidebar) {
            return Err(PushError::NotUiView(view));
        }
        let json = serde_json::to_string(event).unwrap_or_else(|_| "null".to_string());
        let script = format!("window.yaacEvent({json});");
        post_to_ui(Box::new(move || ui::push_script(view, script)));
        Ok(())
    }
}

impl EngineHandle for CefEngine {
    fn send(&self, command: EngineCommand) {
        post_to_ui(Box::new(move || ui::execute(command)));
    }
}

/// Runs `job` on the CEF UI thread. Returns false if CEF is not running.
pub fn post_to_ui(job: Job) -> bool {
    let mut task = JobTask::new(Arc::new(Mutex::new(Some(job))));
    post_task(ThreadId::UI, Some(&mut task)) != 0
}

/// Quit requested from the platform (Cmd-Q, the app menu). Used by the NSApplication subclass.
pub(crate) fn request_quit() {
    post_to_ui(Box::new(ui::close_all_windows));
}

/// The engine's exit code, read after the message loop ends.
pub(crate) fn exit_code() -> i32 {
    GLOBAL
        .get()
        .map(|s| s.exit_code.load(Ordering::SeqCst))
        .unwrap_or(1)
}

wrap_task! {
    struct JobTask {
        job: Arc<Mutex<Option<Job>>>,
    }

    impl Task {
        fn execute(&self) {
            let job = self.job.lock().ok().and_then(|mut slot| slot.take());
            if let Some(job) = job {
                job();
            }
        }
    }
}
