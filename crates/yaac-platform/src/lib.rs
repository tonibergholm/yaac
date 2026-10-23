//! Platform traits (docs/architecture.md section 5). Operating-system implementations live in
//! `macos` behind `cfg`. M1 needs only paths and the main-thread hook.

use std::fs::{DirBuilder, Permissions};
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
    /// Creates (or validates) every directory with `ensure_private_dir`. Browsing data lives here, so
    /// each one is private to the current user.
    pub fn create_all(&self) -> io::Result<()> {
        for dir in [
            &self.config,
            &self.data,
            &self.cache,
            &self.logs,
            &self.runtime,
        ] {
            ensure_private_dir(dir)?;
        }
        Ok(())
    }
}

/// Makes `path` a private directory of the current user, for data that must not be shared.
///
/// - A missing directory is created with mode 0700 (its missing parents with the default mode).
/// - An existing path must be a real directory: a symlink is refused, not followed.
/// - It must be owned by the current user.
/// - A looser mode (any group or other bits) is tightened to 0700. A stricter mode is kept.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            DirBuilder::new().mode(0o700).create(path)?;
        }
        Err(error) => return Err(error),
    }
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!("{} is a symlink", path.display())));
    }
    if !meta.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    if meta.uid() != current_uid() {
        return Err(io::Error::other(format!(
            "{} is not owned by the current user",
            path.display()
        )));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(path, Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn current_uid() -> u32 {
    // SAFETY: getuid takes no arguments, has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Runs work on the process's main thread. The host injects an implementation backed by the
/// engine's UI-thread task queue, so this crate never depends on the engine.
pub trait MainThread: Send + Sync {
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

pub trait Platform: Send + Sync {
    fn paths(&self) -> AppPaths;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A fresh directory under the system temp dir, unique to this test run.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("yaac-platform-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn creates_missing_directories_private() {
        let root = scratch("create");
        let leaf = root.join("a").join("b");
        ensure_private_dir(&leaf).expect("create");
        assert!(leaf.is_dir());
        assert_eq!(mode(&leaf), 0o700);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tightens_a_looser_mode_and_keeps_a_stricter_one() {
        let root = scratch("modes");
        let loose = root.join("loose");
        std::fs::create_dir(&loose).expect("dir");
        std::fs::set_permissions(&loose, Permissions::from_mode(0o755)).expect("chmod");
        ensure_private_dir(&loose).expect("validate");
        assert_eq!(mode(&loose), 0o700);

        let strict = root.join("strict");
        std::fs::create_dir(&strict).expect("dir");
        std::fs::set_permissions(&strict, Permissions::from_mode(0o500)).expect("chmod");
        ensure_private_dir(&strict).expect("validate");
        assert_eq!(mode(&strict), 0o500, "a stricter mode is left alone");
        std::fs::set_permissions(&strict, Permissions::from_mode(0o700)).expect("restore");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_symlink_and_does_not_touch_its_target() {
        let root = scratch("symlink");
        let target = root.join("target");
        std::fs::create_dir(&target).expect("dir");
        std::fs::set_permissions(&target, Permissions::from_mode(0o755)).expect("chmod");
        let link = root.join("link");
        symlink(&target, &link).expect("symlink");
        assert!(ensure_private_dir(&link).is_err());
        assert_eq!(
            mode(&target),
            0o755,
            "the target of a refused link is not changed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_regular_file() {
        let root = scratch("file");
        let file = root.join("file");
        std::fs::write(&file, b"x").expect("file");
        assert!(ensure_private_dir(&file).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_all_makes_every_directory_private() {
        let root = scratch("all");
        let paths = AppPaths {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            runtime: root.join("runtime"),
            logs: root.join("logs"),
        };
        paths.create_all().expect("create_all");
        for dir in [
            &paths.config,
            &paths.data,
            &paths.cache,
            &paths.runtime,
            &paths.logs,
        ] {
            assert_eq!(mode(dir), 0o700, "{}", dir.display());
        }
        // Idempotent: a second run validates and keeps the directories.
        paths.create_all().expect("second run");
        let _ = std::fs::remove_dir_all(&root);
    }
}
