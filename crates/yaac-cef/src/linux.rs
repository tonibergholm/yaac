//! Linux specifics that CEF needs. There is no framework loader and no application subclass:
//! `libcef.so` is linked at build time and found through the executable's RUNPATH (set in
//! `crates/yaac-app/build.rs`), and the helper processes are the same executable (see
//! `lifecycle::run_helper`). What is left is the windowing platform and the password store.

use cef::*;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Nothing to hold: the library is already mapped through RUNPATH.
pub(crate) struct Library;

/// No process sandbox object: CEF's user-namespace sandbox needs no setup from the host.
pub(crate) struct HelperSandbox;

pub(crate) fn load_framework(_exe: &Path, _is_helper: bool) -> Library {
    Library
}

pub(crate) fn helper_sandbox(_args: &cef::args::Args) -> HelperSandbox {
    HelperSandbox
}

/// Linux has no application subclass to install.
pub(crate) fn setup_application() {}

/// The browser has no native NSWindow. Views windows are logged without a native number.
pub(crate) fn window_number(_browser: &Browser) -> Option<isize> {
    None
}

/// Chooses the Ozone platform for this run and sets it explicitly, in lowercase. Uppercase values
/// crash CEF (CEF issue #4020), and an explicit value does not depend on the session variables a
/// desktop entry or a service may lack.
pub(crate) fn add_platform_switches(command_line: &mut CommandLine) {
    let display = std::env::var_os("WAYLAND_DISPLAY");
    let runtime = std::env::var_os("XDG_RUNTIME_DIR");
    let platform = ozone_platform(display.as_deref(), runtime.as_deref(), |path| path.exists());
    eprintln!("yaac: ozone platform {platform}");
    command_line.append_switch_with_value(
        Some(&CefString::from("ozone-platform")),
        Some(&CefString::from(platform)),
    );
}

/// Debug builds with `YAAC_MOCK_KEYCHAIN=1` only. The basic store keeps Chromium's data
/// unencrypted at rest, which is the same warning as the macOS mock keychain.
pub(crate) fn add_mock_keychain(command_line: &mut CommandLine) {
    command_line.append_switch_with_value(
        Some(&CefString::from("password-store")),
        Some(&CefString::from("basic")),
    );
}

/// `wayland` when the Wayland socket is reachable, `x11` otherwise (XWayland under a Wayland
/// session, or a real X server). `WAYLAND_DISPLAY` is either a socket name, relative to
/// `XDG_RUNTIME_DIR`, or an absolute path. `exists` is injected so the rule can be tested.
pub(crate) fn ozone_platform(
    wayland_display: Option<&OsStr>,
    xdg_runtime_dir: Option<&OsStr>,
    exists: impl Fn(&Path) -> bool,
) -> &'static str {
    let Some(display) = wayland_display.filter(|d| !d.is_empty()) else {
        return "x11";
    };
    let display = Path::new(display);
    let socket: PathBuf = if display.is_absolute() {
        display.to_path_buf()
    } else {
        match xdg_runtime_dir.filter(|d| !d.is_empty()) {
            Some(dir) => Path::new(dir).join(display),
            None => return "x11",
        }
    };
    if exists(&socket) { "wayland" } else { "x11" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn reachable(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|allowed| p == Path::new(allowed))
    }

    #[test]
    fn wayland_when_the_socket_is_reachable() {
        let display = OsString::from("wayland-1");
        let runtime = OsString::from("/run/user/1000");
        assert_eq!(
            ozone_platform(
                Some(&display),
                Some(&runtime),
                reachable(&["/run/user/1000/wayland-1"])
            ),
            "wayland"
        );
    }

    #[test]
    fn x11_when_the_socket_is_missing() {
        let display = OsString::from("wayland-1");
        let runtime = OsString::from("/run/user/1000");
        assert_eq!(
            ozone_platform(Some(&display), Some(&runtime), reachable(&[])),
            "x11"
        );
    }

    #[test]
    fn x11_without_a_wayland_display() {
        let runtime = OsString::from("/run/user/1000");
        assert_eq!(ozone_platform(None, Some(&runtime), |_| true), "x11");
        let empty = OsString::new();
        assert_eq!(
            ozone_platform(Some(&empty), Some(&runtime), |_| true),
            "x11"
        );
    }

    #[test]
    fn an_absolute_display_is_used_as_is() {
        let display = OsString::from("/tmp/nested/wayland-9");
        assert_eq!(
            ozone_platform(Some(&display), None, reachable(&["/tmp/nested/wayland-9"])),
            "wayland"
        );
    }

    #[test]
    fn a_relative_display_without_a_runtime_dir_is_not_reachable() {
        let display = OsString::from("wayland-1");
        assert_eq!(ozone_platform(Some(&display), None, |_| true), "x11");
    }
}
