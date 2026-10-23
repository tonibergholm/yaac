//! Directories the engine writes to: CEF's cache root and one per profile. Same rules as
//! `yaac_platform::ensure_private_dir`, kept here so the engine needs no platform dependency.

use std::fs::{DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::Path;

/// Creates `path` as a private directory, or validates an existing one: a real directory, not a
/// symlink, owned by the current user, mode 0700 (looser modes are tightened, stricter ones kept).
pub(crate) fn private_dir(path: &Path) -> io::Result<()> {
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
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(io::Error::other(format!("{} is not a plain directory", path.display())));
    }
    // SAFETY: getuid takes no arguments, has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if meta.uid() != uid {
        return Err(io::Error::other(format!("{} is not owned by the current user", path.display())));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(path, Permissions::from_mode(0o700))?;
    }
    Ok(())
}
