//! Linux implementation (Omarchy and other XDG desktops). Paths follow the XDG Base Directory
//! specification. Nothing here touches D-Bus or a desktop service yet; those come with the
//! features that need them.

use crate::{AppPaths, Platform};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn paths(&self) -> AppPaths {
        paths_from(&LinuxEnv::from_process())
    }
}

/// The inputs of the path layout, read from the environment once. Pure, so the layout can be
/// tested without touching the real environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxEnv {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    /// The system temp directory (`$TMPDIR`, else `/tmp`). Only used for the runtime directory
    /// when `XDG_RUNTIME_DIR` is unset, and for a home directory that cannot be found.
    pub tmp: PathBuf,
    pub uid: u32,
}

impl LinuxEnv {
    pub fn from_process() -> Self {
        // SAFETY: getuid takes no arguments, has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let var = |key: &str| std::env::var_os(key).map(PathBuf::from);
        // A relative HOME is not a home directory: the password database is asked instead.
        let home = var("HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| passwd_home(uid));
        // A relative TMPDIR is ignored, as for the XDG variables.
        let tmp = var("TMPDIR")
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        LinuxEnv {
            home,
            xdg_config_home: var("XDG_CONFIG_HOME"),
            xdg_data_home: var("XDG_DATA_HOME"),
            xdg_cache_home: var("XDG_CACHE_HOME"),
            xdg_state_home: var("XDG_STATE_HOME"),
            xdg_runtime_dir: var("XDG_RUNTIME_DIR"),
            tmp,
            uid,
        }
    }
}

/// The home directory of `uid` from the password database (`getpwuid_r`), if it is absolute.
fn passwd_home(uid: u32) -> Option<PathBuf> {
    // The size the C library asks for; fall back to a generous buffer when it does not say.
    // SAFETY: sysconf takes an integer constant and has no preconditions.
    let size = match unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) } {
        n if n > 0 => n as usize,
        _ => 16 * 1024,
    };
    let mut buffer = vec![0u8; size];
    // SAFETY: a zeroed passwd is a valid out-parameter; getpwuid_r writes into `buffer` and sets
    // `result` to point at `pwd` on success, or to null when there is no entry.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buffer.as_mut_ptr().cast::<libc::c_char>(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: on success pw_dir is a NUL-terminated string inside `buffer`, which is alive here.
    let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
    path.is_absolute().then_some(path)
}

/// The XDG base directory spec: a relative or empty value is invalid and must be ignored.
fn absolute(value: &Option<PathBuf>) -> Option<&Path> {
    value.as_deref().filter(|p| p.is_absolute())
}

/// The path layout for a given environment. Pure, so it can be tested.
///
/// - config: `$XDG_CONFIG_HOME/yaac`, else `~/.config/yaac`
/// - data: `$XDG_DATA_HOME/yaac`, else `~/.local/share/yaac`
/// - cache: `$XDG_CACHE_HOME/yaac`, else `~/.cache/yaac`
/// - logs: `$XDG_STATE_HOME/yaac`, else `~/.local/state/yaac`
/// - runtime: `$XDG_RUNTIME_DIR/yaac`. Without `XDG_RUNTIME_DIR`, `<tmp>/yaac-<uid>`. The private
///   directory check refuses a directory another user created at that path.
///
/// Without a usable `HOME`, the home directory is `<tmp>/yaac-home-<uid>`. The private directory
/// check then applies to it as well.
pub fn paths_from(env: &LinuxEnv) -> AppPaths {
    let home = absolute(&env.home)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env.tmp.join(format!("yaac-home-{}", env.uid)));
    let under = |xdg: &Option<PathBuf>, fallback: &[&str]| {
        let base = absolute(xdg).map(Path::to_path_buf).unwrap_or_else(|| {
            fallback
                .iter()
                .fold(home.clone(), |acc, part| acc.join(part))
        });
        base.join("yaac")
    };
    let runtime = match absolute(&env.xdg_runtime_dir) {
        Some(dir) => dir.join("yaac"),
        None => env.tmp.join(format!("yaac-{}", env.uid)),
    };
    AppPaths {
        config: under(&env.xdg_config_home, &[".config"]),
        data: under(&env.xdg_data_home, &[".local", "share"]),
        cache: under(&env.xdg_cache_home, &[".cache"]),
        logs: under(&env.xdg_state_home, &[".local", "state"]),
        runtime,
    }
}

/// Linux has no application menu bar: commands live in the command bar. Kept so the host code
/// is the same on every platform.
pub fn install_app_menu(_app_name: &str) {}

/// Linux windows are raised by the window manager when they are created; nothing to do here.
pub fn activate_app() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: Option<&str>, uid: u32) -> LinuxEnv {
        LinuxEnv {
            home: home.map(PathBuf::from),
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_cache_home: None,
            xdg_state_home: None,
            xdg_runtime_dir: None,
            tmp: PathBuf::from("/tmp"),
            uid,
        }
    }

    #[test]
    fn defaults_follow_the_xdg_fallbacks() {
        let paths = paths_from(&env(Some("/home/toni"), 1000));
        assert_eq!(paths.config, PathBuf::from("/home/toni/.config/yaac"));
        assert_eq!(paths.data, PathBuf::from("/home/toni/.local/share/yaac"));
        assert_eq!(paths.cache, PathBuf::from("/home/toni/.cache/yaac"));
        assert_eq!(paths.logs, PathBuf::from("/home/toni/.local/state/yaac"));
        assert_eq!(paths.runtime, PathBuf::from("/tmp/yaac-1000"));
    }

    #[test]
    fn xdg_variables_are_honoured() {
        let mut e = env(Some("/home/toni"), 1000);
        e.xdg_config_home = Some("/x/config".into());
        e.xdg_data_home = Some("/x/data".into());
        e.xdg_cache_home = Some("/x/cache".into());
        e.xdg_state_home = Some("/x/state".into());
        e.xdg_runtime_dir = Some("/run/user/1000".into());
        let paths = paths_from(&e);
        assert_eq!(paths.config, PathBuf::from("/x/config/yaac"));
        assert_eq!(paths.data, PathBuf::from("/x/data/yaac"));
        assert_eq!(paths.cache, PathBuf::from("/x/cache/yaac"));
        assert_eq!(paths.logs, PathBuf::from("/x/state/yaac"));
        assert_eq!(paths.runtime, PathBuf::from("/run/user/1000/yaac"));
    }

    #[test]
    fn the_password_database_gives_an_absolute_home_when_it_has_an_entry() {
        // Containers may have no entry for the current uid; then there is nothing to check.
        // SAFETY: getuid takes no arguments and cannot fail.
        let uid = unsafe { libc::getuid() };
        if let Some(home) = passwd_home(uid) {
            assert!(home.is_absolute(), "{}", home.display());
        }
    }

    #[test]
    fn relative_xdg_values_are_ignored() {
        let mut e = env(Some("/home/toni"), 1000);
        e.xdg_config_home = Some("relative/config".into());
        e.xdg_runtime_dir = Some(PathBuf::new());
        let paths = paths_from(&e);
        assert_eq!(paths.config, PathBuf::from("/home/toni/.config/yaac"));
        assert_eq!(paths.runtime, PathBuf::from("/tmp/yaac-1000"));
    }

    #[test]
    fn runtime_falls_back_per_user_when_xdg_runtime_dir_is_unset() {
        let mut e = env(Some("/home/toni"), 4242);
        e.tmp = PathBuf::from("/var/folders/T");
        assert_eq!(
            paths_from(&e).runtime,
            PathBuf::from("/var/folders/T/yaac-4242")
        );
    }

    #[test]
    fn a_missing_home_falls_back_under_the_temp_dir() {
        let paths = paths_from(&env(None, 1000));
        assert_eq!(
            paths.config,
            PathBuf::from("/tmp/yaac-home-1000/.config/yaac")
        );
        assert!(
            paths.data.starts_with("/tmp/yaac-home-1000"),
            "{}",
            paths.data.display()
        );
    }

    #[test]
    fn a_relative_home_is_not_used() {
        let paths = paths_from(&env(Some("home/toni"), 1000));
        assert!(paths.config.starts_with("/tmp/yaac-home-1000"));
    }

    #[test]
    fn every_directory_is_distinct() {
        let paths = paths_from(&env(Some("/home/toni"), 1000));
        let all = [
            &paths.config,
            &paths.data,
            &paths.cache,
            &paths.logs,
            &paths.runtime,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
