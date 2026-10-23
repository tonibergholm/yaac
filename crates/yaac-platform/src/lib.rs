//! Platform traits (docs/architecture.md section 5). Operating-system implementations live in the
//! `macos` and `linux` modules behind `cfg`. M1 needs only paths and the main-thread hook.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    /// Per-user runtime directory (socket, lock), mode 0700.
    pub runtime: PathBuf,
    pub logs: PathBuf,
}

/// Runs work on the process's main thread. The host injects an implementation backed by the
/// engine's UI-thread task queue, so this crate never depends on the engine.
pub trait MainThread: Send + Sync {
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

pub trait Platform: Send + Sync {
    fn paths(&self) -> AppPaths;
}
