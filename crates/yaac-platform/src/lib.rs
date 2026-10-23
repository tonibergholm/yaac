//! Platform traits (docs/architecture.md section 5). Operating-system implementations live in
//! `macos` behind `cfg`. M1 needs only paths and the main-thread hook.

use std::fs::{DirBuilder, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
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
/// The check uses the open file, not the path, so nothing can be swapped between the check and the
/// change:
/// - the leaf is opened with `O_DIRECTORY | O_NOFOLLOW`: a symlink or a file is refused;
/// - `fstat` on that descriptor: the owner must be the current user;
/// - `fchmod` on that descriptor: the mode becomes exactly 0700 (a stricter mode is changed too).
///
/// Path components below `$HOME`, `$TMPDIR` or the system temp directory must not be symlinks (`O_NOFOLLOW` only covers the
/// last component). The parent must be owned by the current user or by root. A missing directory is
/// created with mode 0700; its missing parents are created with the default mode.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    let uid = current_uid();
    let base = private_base(path);
    // Checked before anything is created: a link in the path must not be followed for creation.
    reject_symlinks_below(path, base.as_deref())?;
    if std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        DirBuilder::new().mode(0o700).create(path)?;
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    let meta = dir.metadata()?;
    if meta.uid() != uid {
        return Err(io::Error::other(format!(
            "{} is not owned by the current user",
            path.display()
        )));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let parent_meta = std::fs::metadata(parent)?;
        if parent_meta.uid() != uid && parent_meta.uid() != 0 {
            return Err(io::Error::other(format!(
                "the parent of {} is not owned by the user",
                path.display()
            )));
        }
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        dir.set_permissions(Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// `$HOME`, `$TMPDIR` or the system temp directory, whichever the path lies below. The temp
/// directory is read from `std::env::temp_dir`, which falls back to `/tmp` when `$TMPDIR` is unset
/// (the usual case on Linux). Symlinks are checked below that base only: the system's own links
/// above it (on macOS `/var` is a link) are not the user's to control.
fn private_base(path: &Path) -> Option<PathBuf> {
    let from_env = ["HOME", "TMPDIR"]
        .iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from));
    from_env
        .chain(std::iter::once(std::env::temp_dir()))
        .filter(|base| path.starts_with(base))
        .max_by_key(|base| base.as_os_str().len())
}

/// Refuses a symlink in any component of `path` below `base`, the leaf included.
fn reject_symlinks_below(path: &Path, base: Option<&Path>) -> io::Result<()> {
    let Some(base) = base else { return Ok(()) };
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if !current.starts_with(base) || current.as_os_str().len() <= base.as_os_str().len() {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::other(format!(
                    "{} is a symbolic link",
                    current.display()
                )));
            }
            Ok(_) => {}
            // Not created yet: nothing below it exists either.
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
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
    fn normalises_the_mode_to_0700_in_both_directions() {
        let root = scratch("modes");
        for (name, start) in [("loose", 0o755), ("strict", 0o500), ("private", 0o700)] {
            let dir = root.join(name);
            std::fs::create_dir(&dir).expect("dir");
            std::fs::set_permissions(&dir, Permissions::from_mode(start)).expect("chmod");
            ensure_private_dir(&dir).expect("validate");
            assert_eq!(mode(&dir), 0o700, "{name} starts at {start:o}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn trailing_slash_is_the_same_directory() {
        let root = scratch("slash");
        let dir = root.join("d");
        ensure_private_dir(&dir).expect("create");
        let with_slash = PathBuf::from(format!("{}/", dir.display()));
        ensure_private_dir(&with_slash).expect("validate with slash");
        assert_eq!(mode(&dir), 0o700);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_symlinked_parent_below_the_temp_dir_is_refused() {
        let root = scratch("parent-link");
        let real = root.join("real");
        std::fs::create_dir(&real).expect("dir");
        let link = root.join("link");
        symlink(&real, &link).expect("symlink");
        let through = link.join("child");
        assert!(
            ensure_private_dir(&through).is_err(),
            "a link in the path must not be followed"
        );
        assert!(
            !real.join("child").exists(),
            "nothing is created through the link"
        );
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
