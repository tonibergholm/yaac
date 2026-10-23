//! macOS specifics that CEF requires: the `NSApplication` subclass that conforms to
//! `CefAppProtocol` (CEF checks for it before `initialize`), the framework loader and the helper
//! sandbox. No Objective-C type leaves this module.

use crate::engine;
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use cef::{args::Args, library_loader::LibraryLoader, sandbox::Sandbox};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, extern_methods,
    runtime::{AnyObject, Bool, NSObjectProtocol},
};
use objc2_app_kit::{NSApp, NSApplication, NSEvent, NSView};
use std::cell::Cell;
use std::path::Path;

/// Loads the CEF framework. Helpers resolve it relative to their own bundle.
pub(crate) fn load_framework(exe: &Path, is_helper: bool) -> LibraryLoader {
    let loader = LibraryLoader::new(exe, is_helper);
    assert!(loader.load(), "failed to load Chromium Embedded Framework");
    loader
}

/// Must run before `initialize()`. CEF needs `NSApp` to be our subclass.
pub(crate) fn setup_application() {
    let _ = YaacApplication::shared_application();
    let mtm = MainThreadMarker::new().expect("main thread");
    assert!(NSApp(mtm).isKindOfClass(YaacApplication::class()));
}

/// The NSWindow number of a browser, for `screencapture -l`. Logged once per browser.
pub(crate) fn window_number(browser: &cef::Browser) -> Option<isize> {
    use cef::ImplBrowser as _;
    use cef::ImplBrowserHost as _;
    let host = browser.host()?;
    let view = host.window_handle().cast::<NSView>();
    // SAFETY: the handle is the browser's NSView, alive while the browser is.
    let view = unsafe { view.as_ref()? };
    Some(view.window()?.windowNumber())
}

/// The helper process sandbox. Keep the returned value alive for the life of the process.
pub(crate) fn helper_sandbox(args: &Args) -> Sandbox {
    let mut sandbox = Sandbox::new();
    sandbox.initialize(args.as_main_args());
    sandbox
}

define_class!(
    /// `NSApplication` subclass for CEF. Same shape as cef-rs `examples/cefsimple`.
    #[unsafe(super(NSApplication))]
    #[ivars = YaacApplicationIvars]
    pub struct YaacApplication;

    impl YaacApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was_sending_event = self.is_handling_send_event();
            if !was_sending_event {
                self.set_handling_send_event(true);
            }
            let _: () = unsafe { objc2::msg_send![super(self), sendEvent: event] };
            if !was_sending_event {
                self.set_handling_send_event(false);
            }
        }

        /// Cmd-Q and the app menu's Quit land here. Close the windows through the normal close
        /// path so CEF can finish its browsers first. Calling `exit()` would skip that.
        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: Option<&AnyObject>) {
            engine::request_quit();
        }
    }

    unsafe impl CrAppControlProtocol for YaacApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn _set_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for YaacApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn _is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for YaacApplication {}
);

#[derive(Default)]
pub struct YaacApplicationIvars {
    handling_send_event: Cell<Bool>,
}

impl YaacApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> objc2::rc::Retained<Self>;

        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling_send_event(&self, handling_send_event: bool);

        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling_send_event(&self) -> bool;
    }
}
