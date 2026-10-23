//! The Ozone platform for a Chromium run. Pure, so the rule is tested on every OS (CI included);
//! `yaac-cef` applies the result.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `wayland` when the Wayland socket is reachable, `x11` otherwise (XWayland under a Wayland
/// session, or a real X server). `WAYLAND_DISPLAY` is either a socket name, relative to
/// `XDG_RUNTIME_DIR`, or an absolute path. `exists` is injected so the rule can be tested.
pub fn ozone_platform(
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
