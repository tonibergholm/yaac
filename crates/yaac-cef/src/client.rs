//! CEF client and handlers for one view. Each handler reports to the host as an `EngineEvent`
//! (docs/architecture.md section 5a). Every method runs on the UI thread and returns without
//! waiting for the core. Page-supplied strings are capped before they are queued, and logs show
//! URLs as scheme and host only.

use crate::engine::Shared;
use crate::log::dev_log;
use crate::log::elog;
use crate::nav::NavTracker;
use crate::text::{cap_title, cap_url, redact_url};
use crate::ui;
use cef::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use yaac_engine::{EngineEvent, ViewKind};
use yaac_model::ViewId;

/// Test hook: how many unload confirmations to answer "stay" (cancel the close). Zero in production:
/// the app accepts every unload prompt, so a close only cancels when a test asks it to.
pub(crate) static UNLOAD_CANCELS: AtomicU32 = AtomicU32::new(0);

fn unload_allowed() -> bool {
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    {
        if UNLOAD_CANCELS
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return false;
        }
    }
    true
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
        nav: Arc<Mutex<NavTracker>>,
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

        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(EngineJsDialog::new())
        }
    }
}

wrap_jsdialog_handler! {
    pub(crate) struct EngineJsDialog {}

    impl JsdialogHandler {
        fn on_before_unload_dialog(
            &self,
            _browser: Option<&mut Browser>,
            _message_text: Option<&CefString>,
            _is_reload: i32,
            callback: Option<&mut JsdialogCallback>,
        ) -> i32 {
            // The page's beforeunload confirmation is answered here, synchronously. "Stay" cancels
            // the close and leaves every browser as it was.
            let allow = unload_allowed();
            if let Some(callback) = callback {
                callback.cont(i32::from(allow), None);
            }
            dev_log!("unload confirmation answered allow={allow}");
            1
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
            // Always denied, synchronously. Only the first few per second per opener are reported;
            // the core decides, through a command, whether to open a tab.
            let url = target_url.map(CefString::to_string).unwrap_or_default();
            let gesture = user_gesture != 0;
            if self.shared.allow_popup(self.view, gesture) {
                self.shared.emit(EngineEvent::PopupRequested {
                    opener: self.view,
                    url: cap_url(&url),
                    user_gesture: gesture,
                });
            }
            // Denied in every case. Per-popup lines are not logged; suppression is summarised.
            self.shared.log_popup_summary();
            1
        }
    }
}

wrap_display_handler! {
    pub(crate) struct EngineDisplay {
        view: ViewId,
        shared: Arc<Shared>,
        nav: Arc<Mutex<NavTracker>>,
    }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            // Titles are page-controlled and never logged.
            let title = title.map(CefString::to_string).unwrap_or_default();
            let nav_gen = self.nav.lock().map(|nav| nav.current()).unwrap_or(0);
            self.shared.emit(EngineEvent::TitleChanged {
                view: self.view,
                nav_gen,
                title: cap_title(&title),
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
            let url = url.map(CefString::to_string).unwrap_or_default();
            let nav_gen = self.nav.lock().map(|mut nav| nav.address_change()).unwrap_or(0);
            dev_log!("navigated view={} nav_gen={nav_gen} url={}", self.view, redact_url(&url));
            self.shared.emit(EngineEvent::Navigated {
                view: self.view,
                nav_gen,
                url: cap_url(&url),
            });
        }
    }
}

wrap_load_handler! {
    pub(crate) struct EngineLoad {
        view: ViewId,
        shared: Arc<Shared>,
        nav: Arc<Mutex<NavTracker>>,
    }

    impl LoadHandler {
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if is_main(frame) {
                let generation = self.nav.lock().map(|mut nav| nav.load_start()).unwrap_or(0);
                dev_log!("load start view={} nav_gen={generation}", self.view);
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            _error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if is_main(frame) {
                let url = failed_url.map(CefString::to_string).unwrap_or_default();
                elog!("load error view={} code={:?} url={}", self.view, error_code, redact_url(&url));
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
            elog!("refused navigation view={} kind={:?} url={}", self.view, self.kind, redact_url(&url));
            self.shared.rejections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
            ui::browser_crashed(self.view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_browsers_stay_on_app_ui() {
        assert!(navigation_allowed(
            ViewKind::Sidebar,
            "app://ui/sidebar.html"
        ));
        assert!(navigation_allowed(ViewKind::Sidebar, "app://ui"));
        assert!(navigation_allowed(ViewKind::Sidebar, "APP://UI/x"));
        assert!(!navigation_allowed(
            ViewKind::Sidebar,
            "https://example.com/"
        ));
        assert!(!navigation_allowed(ViewKind::Sidebar, "about:blank"));
        assert!(!navigation_allowed(ViewKind::Sidebar, "app://uix/"));
        assert!(!navigation_allowed(ViewKind::Sidebar, "app://other/"));
    }

    #[test]
    fn content_browsers_never_reach_app_urls() {
        assert!(!navigation_allowed(
            ViewKind::Content,
            "app://ui/sidebar.html"
        ));
        assert!(!navigation_allowed(ViewKind::Content, "App://ui/"));
        assert!(!navigation_allowed(ViewKind::Content, "app:whatever"));
        assert!(navigation_allowed(
            ViewKind::Content,
            "https://example.com/"
        ));
        assert!(navigation_allowed(
            ViewKind::Content,
            "http://127.0.0.1:8080/"
        ));
        assert!(navigation_allowed(ViewKind::Content, "about:blank"));
    }
}
