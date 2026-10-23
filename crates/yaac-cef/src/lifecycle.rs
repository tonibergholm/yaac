//! Process entry points. The browser process calls `run_browser` and blocks in CEF's message loop
//! until the last window closes. Each helper process calls `run_helper` and exits with its code.

use crate::engine::{CefEngine, EngineSettings};
use crate::scheme;
use crate::ui;
use cef::*;
use std::sync::{Arc, Mutex};

type OnReady = Box<dyn FnOnce(Arc<CefEngine>) + Send>;

/// The browser-process hook. `on_ready` runs on the UI thread once CEF is initialized. It must not
/// block: spawn work on a runtime instead.
struct ReadyHook {
    engine: Arc<CefEngine>,
    on_ready: Mutex<Option<OnReady>>,
}

wrap_app! {
    struct YaacApp {
        ready: Option<Arc<ReadyHook>>,
    }

    impl App {
        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            scheme::register(registrar);
        }

        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Browser process only.
            let is_browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            if !is_browser {
                return;
            }
            let Some(command_line) = command_line else { return };
            for switch in [
                "disable-extensions",
                "disable-background-networking",
                "disable-component-update",
                "disable-sync",
                "disable-domain-reliability",
                "disable-client-side-phishing-detection",
                "safebrowsing-disable-auto-update",
                "metrics-recording-only",
                "no-pings",
            ] {
                command_line.append_switch(Some(&CefString::from(switch)));
            }
            // The mock keychain stores Chromium's profile-encryption key as a fixed constant, so
            // cookies and saved logins are effectively unencrypted at rest. It keeps the Keychain
            // prompt away from ad-hoc signed development builds, so it is for debug builds and an
            // explicit opt-in only. Release builds use the real Keychain.
            if cfg!(debug_assertions) || std::env::var_os("YAAC_MOCK_KEYCHAIN").is_some() {
                eprintln!("yaac: WARNING mock keychain in use; profile data is not protected");
                command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            }
            command_line.append_switch_with_value(
                Some(&CefString::from("disable-features")),
                Some(&CefString::from(
                    "OptimizationHints,OptimizationGuideModelDownloading,MediaRouter,Translate,SafeBrowsing,CertificateTransparencyComponentUpdater",
                )),
            );
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            self.ready.clone().map(BrowserHook::new)
        }
    }
}

wrap_browser_process_handler! {
    struct BrowserHook {
        ready: Arc<ReadyHook>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            ui::install(self.ready.engine.shared());
            let hook = self.ready.on_ready.lock().ok().and_then(|mut slot| slot.take());
            if let Some(hook) = hook {
                hook(self.ready.engine.clone());
            }
        }
    }
}

/// Runs the browser process. Returns the exit code once the message loop has ended.
pub fn run_browser(
    settings: EngineSettings,
    on_ready: impl FnOnce(Arc<CefEngine>) + Send + 'static,
) -> i32 {
    let exe = std::env::current_exe().expect("current_exe");
    let _library = crate::macos::load_framework(&exe, false);
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    crate::macos::setup_application();

    let cef_root = settings.data_dir.join("cef");
    if let Err(error) = std::fs::create_dir_all(&cef_root) {
        crate::log::elog!("error cef-root path={} reason={error}", cef_root.display());
        return 1;
    }
    let log_file = settings.log_file.clone();
    let engine = Arc::new(CefEngine::new(settings));
    let hook = Arc::new(ReadyHook {
        engine: engine.clone(),
        on_ready: Mutex::new(Some(Box::new(on_ready))),
    });
    let mut app = YaacApp::new(Some(hook));

    let args = cef::args::Args::new();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        // Not the browser process. Nothing to run here.
        return code;
    }

    let cef_settings = Settings {
        no_sandbox: 0,
        root_cache_path: CefString::from(cef_root.to_string_lossy().as_ref()),
        // Equal to the root: the default profile lives in the root, and named profiles are its
        // children (docs: CefSettings.cache_path). An empty value gives incognito-only profiles.
        cache_path: CefString::from(cef_root.to_string_lossy().as_ref()),
        log_file: CefString::from(log_file.to_string_lossy().as_ref()),
        ..Default::default()
    };
    let initialized = initialize(
        Some(args.as_main_args()),
        Some(&cef_settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if initialized != 1 {
        crate::log::elog!("error initialize failed");
        return 1;
    }
    run_message_loop();
    shutdown();
    crate::log::line(format_args!("exit code={}", crate::engine::exit_code()));
    crate::engine::exit_code()
}

/// Runs a helper (renderer, GPU, utility) process. Returns the process exit code.
pub fn run_helper() -> i32 {
    let exe = std::env::current_exe().expect("current_exe");
    let args = cef::args::Args::new();
    let _sandbox = crate::macos::helper_sandbox(&args);
    let _library = crate::macos::load_framework(&exe, true);
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let mut app = YaacApp::new(None);
    execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    )
}
