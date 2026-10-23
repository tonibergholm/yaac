//! `CefEngine`: the `Send + Sync` command proxy and the engine state shared between threads.
//!
//! Shutdown protocol (in order):
//! 1. A window starts closing (user close, Cmd-Q, Apple Event). `WindowClosing` is sent first and
//!    `quitting` is set. From then on `send` drops commands, new RPCs get 503, no `Closed` is sent
//!    for views, and views still being created are cancelled.
//! 2. The message loop ends. The host's `before_shutdown` hook runs: the core shuts down, in-flight
//!    RPCs drain (bounded), the runtime stops.
//! 3. Thread-local CEF objects are released on the UI thread, pending events are flushed, and the
//!    gate closes: `post_to_ui` returns false from then on and never calls CEF.
//! 4. `CefShutdown`.

use crate::assets::UiAssets;
use crate::coalesce::{Coalescer, PopupLimiter};
use crate::registry::Registry;
use crate::rpc::RpcHandler;
use crate::ui;
use cef::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use yaac_api::Event;
use yaac_engine::{EngineCommand, EngineEvent, EngineHandle, ViewKind};
use yaac_model::ViewId;

pub type Job = Box<dyn FnOnce() + Send>;

/// Coalescing window for navigation, title and load-state events.
const FLUSH_MS: i64 = 50;
/// Popup reports allowed per opener in one second.
const POPUPS_PER_SECOND: usize = 4;

/// What the host supplies before the engine starts.
pub struct EngineSettings {
    /// Root for CEF's caches. Each profile gets `<data_dir>/cef/<profile id>`.
    pub data_dir: PathBuf,
    /// CEF's own log file.
    pub log_file: PathBuf,
    /// Files behind `app://ui/`.
    pub ui_assets: Arc<dyn UiAssets>,
    /// Answers `POST app://ui/api/v1/rpc` from UI browsers.
    pub rpc: Arc<dyn RpcHandler>,
    /// Runs the RPC futures. Must stay alive while the engine runs.
    pub rpc_runtime: tokio::runtime::Handle,
    /// Engine events, sent without blocking the UI thread.
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
    /// `app://ui` requests refused or failed (status 4xx). Read by tests.
    pub(crate) rejections: AtomicU64,
    /// RPC replies sent to a sidebar page.
    pub(crate) rpc_answered: AtomicU64,
    /// RPCs accepted and not yet answered.
    pub(crate) rpc_inflight: AtomicUsize,
    /// Set when a window starts closing. Engine-wide: M1 has one window.
    pub(crate) quitting: AtomicBool,
    /// Live views per kind: `[sidebar, content]`.
    pub(crate) live: [AtomicUsize; 2],
    pub(crate) coalescer: Mutex<Coalescer>,
    pub(crate) flush_scheduled: AtomicBool,
    pub(crate) popups: Mutex<PopupLimiter>,
}

static GATE: RwLock<bool> = RwLock::new(false);
static GLOBAL: OnceLock<Arc<Shared>> = OnceLock::new();

impl Shared {
    pub(crate) fn live_index(kind: ViewKind) -> usize {
        match kind {
            ViewKind::Sidebar => 0,
            ViewKind::Content => 1,
        }
    }

    /// Queues an event for the next flush. Callers cap page-supplied strings first.
    pub(crate) fn emit(&self, event: EngineEvent) {
        if let Ok(mut coalescer) = self.coalescer.lock() {
            coalescer.push(event);
        }
        self.schedule_flush();
    }

    /// Sends the queued batch, in order, to the core.
    pub(crate) fn flush(&self) {
        self.flush_scheduled.store(false, Ordering::SeqCst);
        let batch = self
            .coalescer
            .lock()
            .map(|mut c| c.take())
            .unwrap_or_default();
        for event in batch {
            // The receiver lives as long as the host. A send error means the host is shutting down.
            let _ = self.events.send(event);
        }
    }

    fn schedule_flush(&self) {
        if self.flush_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        if !post_after(Box::new(|| flush_global()), FLUSH_MS) {
            // CEF is not running (or is shutting down): deliver now, do not wait for a timer.
            self.flush();
        }
    }

    pub(crate) fn allow_popup(&self, opener: ViewId) -> (bool, u64) {
        match self.popups.lock() {
            Ok(mut limiter) => (limiter.allow(opener, Instant::now()), limiter.suppressed()),
            Err(_) => (false, 0),
        }
    }
}

fn flush_global() {
    if let Some(shared) = GLOBAL.get() {
        shared.flush();
    }
}

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
            rpc_answered: AtomicU64::new(0),
            rpc_inflight: AtomicUsize::new(0),
            quitting: AtomicBool::new(false),
            live: [AtomicUsize::new(0), AtomicUsize::new(0)],
            coalescer: Mutex::new(Coalescer::default()),
            flush_scheduled: AtomicBool::new(false),
            popups: Mutex::new(PopupLimiter::new(POPUPS_PER_SECOND, Duration::from_secs(1))),
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
        self.shared.quitting.store(true, Ordering::SeqCst);
        request_quit();
    }

    /// True once a window has started closing.
    pub fn is_quitting(&self) -> bool {
        self.shared.quitting.load(Ordering::SeqCst)
    }

    /// Requests refused or failed so far.
    pub fn rejections(&self) -> u64 {
        self.shared.rejections.load(Ordering::SeqCst)
    }

    /// RPC replies sent to sidebar pages so far.
    pub fn rpc_answered(&self) -> u64 {
        self.shared.rpc_answered.load(Ordering::SeqCst)
    }

    /// Live browser views of `kind`, counted from `Created` to `Closed`.
    pub fn view_count(&self, kind: ViewKind) -> usize {
        self.shared.live[Shared::live_index(kind)].load(Ordering::SeqCst)
    }

    /// Waits up to `timeout` for accepted RPCs to be answered. True if none is left.
    pub fn wait_rpc_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.shared.rpc_inflight.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    /// Sends queued events now, without waiting for the flush timer.
    pub fn flush_events(&self) {
        self.shared.flush();
    }

    /// Clicks at (x, y) in a view with a trusted mouse event: a user gesture. Test builds only.
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
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
        if self.shared.quitting.load(Ordering::SeqCst) {
            crate::log::elog!("dropped command during quit");
            return;
        }
        post_to_ui(Box::new(move || ui::execute(command)));
    }
}

/// Opens the gate: `post_to_ui` may post from now on. Called before CEF starts.
pub(crate) fn open_gate() {
    if let Ok(mut open) = GATE.write() {
        *open = true;
    }
}

/// Closes the gate. Waits for posts in progress, then `post_to_ui` returns false and never calls
/// CEF. Called right before `CefShutdown`.
pub(crate) fn close_gate() {
    if let Ok(mut open) = GATE.write() {
        *open = false;
    }
}

/// Runs `job` on the CEF UI thread. Returns false if CEF is not running or is shutting down; the
/// job is then dropped without touching CEF.
pub fn post_to_ui(job: Job) -> bool {
    post_after(job, 0)
}

/// Runs `job` on the CEF UI thread after `delay_ms`. Same gate as `post_to_ui`.
pub(crate) fn post_after(job: Job, delay_ms: i64) -> bool {
    let Ok(open) = GATE.read() else { return false };
    if !*open {
        return false;
    }
    let mut task = JobTask::new(Arc::new(Mutex::new(Some(job))));
    if delay_ms <= 0 {
        post_task(ThreadId::UI, Some(&mut task)) != 0
    } else {
        post_delayed_task(ThreadId::UI, Some(&mut task), delay_ms) != 0
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_posted_while_the_gate_is_closed() {
        // Closed until CEF starts. The check happens before any call into CEF, so this runs without
        // a CEF instance: a refused post returns false and never reaches post_task.
        assert!(!post_to_ui(Box::new(|| {})));
        open_gate();
        close_gate();
        assert!(!post_to_ui(Box::new(|| {})));
        assert!(!post_after(Box::new(|| {}), 50));
    }
}
