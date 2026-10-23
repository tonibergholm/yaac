//! CEF client and handlers for one view. Each handler reports to the host as an `EngineEvent` (see
//! `docs/architecture.md` section 5a). Every method runs on the UI thread and returns without
//! waiting for the core.

use crate::engine::Shared;
use crate::log::elog;
use crate::ui;
use cef::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use yaac_engine::{EngineEvent, ViewKind};
use yaac_model::ViewId;

/// Navigation generation for one view. Bumped on each main-frame load start and address change.
#[derive(Default)]
pub(crate) struct NavState {
    generation: AtomicU64,
}

impl NavState {
    fn bump(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn current(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

/// Whether a navigation may proceed. UI browsers stay on `app://ui/`. Content browsers never reach
/// `app://` (docs/architecture.md section 8).
pub(crate) fn navigation_allowed(kind: ViewKind, url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    match kind {
        ViewKind::Sidebar => lower == "app://ui" || lower.starts_with("app://ui/"),
        ViewKind::Content => !lower.starts_with("app:"),
    }
}

fn is_main(frame: Option<&mut Frame>) -> bool {
    frame.map(|f| f.is_main() != 0).unwrap_or(false)
}

wrap_client! {
    pub(crate) struct EngineClient {
        view: ViewId,
        kind: ViewKind,
        shared: Arc<Shared>,
        nav: Arc<NavState>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(EngineLifeSpan::new(self.view, self.shared.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(EngineDisplay::new(self.view, self.shared.clone(), self.nav.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(EngineLoad::new(self.view, self.shared.clone(), self.nav.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(EngineRequest::new(self.view, self.kind, self.shared.clone()))
        }
    }
}

wrap_life_span_handler! {
    pub(crate) struct EngineLifeSpan {
        view: ViewId,
        shared: Arc<Shared>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                ui::browser_created(self.view, browser.clone());
            }
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            ui::browser_closed(self.view);
        }

        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            // Deny synchronously. The core decides, through a command, whether to open a tab.
            let url = target_url.map(CefString::to_string).unwrap_or_default();
            elog!("popup denied opener={} url={url}", self.view);
            self.shared.emit(EngineEvent::PopupRequested {
                opener: self.view,
                url,
                user_gesture: user_gesture != 0,
            });
            1
        }
    }
}

wrap_display_handler! {
    pub(crate) struct EngineDisplay {
        view: ViewId,
        shared: Arc<Shared>,
        nav: Arc<NavState>,
    }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            self.shared.emit(EngineEvent::TitleChanged {
                view: self.view,
                nav_gen: self.nav.current(),
                title,
            });
        }

        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if !is_main(frame) {
                return;
            }
            let nav_gen = self.nav.bump();
            let url = url.map(CefString::to_string).unwrap_or_default();
            self.shared.emit(EngineEvent::Navigated {
                view: self.view,
                nav_gen,
                url,
            });
        }
    }
}

wrap_load_handler! {
    pub(crate) struct EngineLoad {
        view: ViewId,
        shared: Arc<Shared>,
        nav: Arc<NavState>,
    }

    impl LoadHandler {
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if is_main(frame) {
                let generation = self.nav.bump();
                elog!("load start view={} nav_gen={generation}", self.view);
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if is_main(frame) {
                let text = error_text.map(CefString::to_string).unwrap_or_default();
                let url = failed_url.map(CefString::to_string).unwrap_or_default();
                elog!("load error view={} code={:?} text={text} url={url}", self.view, error_code);
            }
        }

        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: i32,
            can_go_back: i32,
            can_go_forward: i32,
        ) {
            self.shared.emit(EngineEvent::LoadState {
                view: self.view,
                loading: is_loading != 0,
                can_back: can_go_back != 0,
                can_forward: can_go_forward != 0,
            });
        }
    }
}

wrap_request_handler! {
    pub(crate) struct EngineRequest {
        view: ViewId,
        kind: ViewKind,
        shared: Arc<Shared>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let url = request
                .map(|r| CefString::from(&r.url()).to_string())
                .unwrap_or_default();
            if navigation_allowed(self.kind, &url) {
                return 0;
            }
            elog!("refused navigation view={} kind={:?} url={url}", self.view, self.kind);
            1
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: i32,
            _error_string: Option<&CefString>,
        ) {
            elog!("renderer crashed view={}", self.view);
            self.shared.emit(EngineEvent::Crashed { view: self.view });
        }
    }
}
