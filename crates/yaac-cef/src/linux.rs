//! Linux specifics that CEF needs. There is no framework loader and no application subclass:
//! `libcef.so` is linked at build time and found through the executable's RUNPATH (set in
//! `crates/yaac-app/build.rs`), and the helper processes are the same executable (see
//! `lifecycle::run_helper`). What is left is the windowing platform and the password store.

use cef::*;

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

/// Gives the top-level window its requested client size before it is shown. On the Linux headless
/// Ozone platform the window stayed 1x1 while its root panel was laid out at the requested size, so
/// the browser views got no height (found by the engine harness, see docs/spikes/m2-linux-omarchy.md).
pub(crate) fn fit_window(window: &Window, width: i32, height: i32) {
    let size = Size { width, height };
    window.set_size(Some(&size));
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
    // An explicit --ozone-platform on the command line (a development run) wins.
    if command_line.has_switch(Some(&CefString::from("ozone-platform"))) != 0 {
        return;
    }
    let display = std::env::var_os("WAYLAND_DISPLAY");
    let runtime = std::env::var_os("XDG_RUNTIME_DIR");
    let platform =
        yaac_platform::ozone::ozone_platform(display.as_deref(), runtime.as_deref(), |path| {
            path.exists()
        });
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

/// The store that encrypts Chromium's profile data (cookies, saved logins). On a desktop Chromium
/// does not recognise (Hyprland), the default falls back to the basic store, whose fixed key leaves
/// the data effectively unencrypted at rest. So the secret service is requested explicitly, as the
/// Omarchy browsers do. If no secret service is running, Chromium may still fall back; the cookie
/// prefix check (`scripts/cookie-store-check.sh`: `v11` is the keyring, `v10` the fixed key) shows
/// which one a build actually uses.
pub(crate) fn add_secure_store(command_line: &mut CommandLine) {
    eprintln!("yaac: password store gnome-libsecret");
    command_line.append_switch_with_value(
        Some(&CefString::from("password-store")),
        Some(&CefString::from("gnome-libsecret")),
    );
}
