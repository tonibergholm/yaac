//! macOS implementation. Paths follow Apple's conventions; the runtime directory lives under
//! `$TMPDIR` as `yaac-<uid>`, which macOS clears on reboot.

use crate::{AppPaths, Platform};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{NSString, ns_string};
use std::path::{Path, PathBuf};

pub struct MacPlatform;

impl Platform for MacPlatform {
    fn paths(&self) -> AppPaths {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let tmp = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        paths_from(&home, &tmp, uid)
    }
}

/// The path layout for a given home, temp dir and uid. Pure, so it can be tested.
pub fn paths_from(home: &Path, tmp: &Path, uid: u32) -> AppPaths {
    let support = home
        .join("Library")
        .join("Application Support")
        .join("yaac");
    AppPaths {
        config: support.join("config"),
        data: support.join("data"),
        cache: support.join("cache"),
        runtime: tmp.join(format!("yaac-{uid}")),
        logs: home.join("Library").join("Logs").join("yaac"),
    }
}

/// Installs the standard app menu with Quit (Cmd-Q, which sends `terminate:`) and makes the app a
/// regular, foreground app. Call on the main thread after CEF has initialized.
pub fn install_app_menu(app_name: &str) {
    let mtm = MainThreadMarker::new().expect("install_app_menu must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // NSApplicationActivationPolicyRegular = 0: shows in the Dock and takes focus.
    let _: bool = unsafe { msg_send![&*app, setActivationPolicy: 0_isize] };

    let menu_bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    menu_bar.addItem(&app_item);
    let app_menu = NSMenu::new(mtm);
    let title = NSString::from_str(&format!("Quit {app_name}"));
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &title,
            Some(sel!(terminate:)),
            ns_string!("q"),
        )
    };
    app_menu.addItem(&quit);
    app_item.setSubmenu(Some(&app_menu));
    app.setMainMenu(Some(&menu_bar));
}

/// Brings the app to the front, so a window opened by `open` is visible.
pub fn activate_app() {
    let mtm = MainThreadMarker::new().expect("activate_app must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let _: () = unsafe { msg_send![&*app, activateIgnoringOtherApps: true] };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_apple_conventions() {
        let paths = paths_from(Path::new("/Users/x"), Path::new("/var/folders/T"), 501);
        assert_eq!(
            paths.data,
            PathBuf::from("/Users/x/Library/Application Support/yaac/data")
        );
        assert_eq!(paths.logs, PathBuf::from("/Users/x/Library/Logs/yaac"));
        assert_eq!(paths.runtime, PathBuf::from("/var/folders/T/yaac-501"));
    }
}
