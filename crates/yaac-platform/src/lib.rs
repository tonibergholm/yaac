//! Platform traits (docs/architecture.md section 5). Operating-system implementations live in
//! `macos` behind `cfg`. M1 needs only paths and the main-thread hook.

use std::fs::DirBuilder;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{MacPlatform, activate_app, install_app_menu, paths_from};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    /// Per-user runtime directory (socket, lock), mode 0700, owned by the current user.
    pub runtime: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    /// Creates every directory. The runtime directory must be private: it is created 0700 and an
    /// existing directory is rejected unless it is owned by the current user with mode 0700.
    pub fn create_all(&self) -> io::Result<()> {
        // Browsing data lives here, so every directory is private to the user.
        for dir in [&self.config, &self.data, &self.cache, &self.logs] {
            create_dir(dir, 0o700)?;
        }
        create_dir(&self.runtime, 0o700)?;
        // `symlink_metadata`: a symlink planted at this path must not be followed.
        let meta = std::fs::symlink_metadata(&self.runtime)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(io::Error::other(format!(
                "runtime path {} is not a plain directory",
                self.runtime.display()
            )));
        }
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let mode = meta.permissions().mode() & 0o777;
        if meta.uid() != uid || mode != 0o700 {
            return Err(io::Error::other(format!(
                "runtime directory {} is not private (uid {}, mode {mode:o})",
                self.runtime.display(),
                meta.uid()
            )));
        }
        Ok(())
    }
}

fn create_dir(path: &Path, mode: u32) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(mode).create(path)
}

/// Runs work on the process's main thread. The host injects an implementation backed by the
/// engine's UI-thread task queue, so this crate never depends on the engine.
pub trait MainThread: Send + Sync {
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

pub trait Platform: Send + Sync {
    fn paths(&self) -> AppPaths;
}
