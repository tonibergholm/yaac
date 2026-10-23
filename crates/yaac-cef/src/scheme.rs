//! The `app://ui` scheme. Each request is attributed to a browser first, and only then decided:
//! - a sidebar (UI) browser gets the bundled files, and may call `POST /api/v1/rpc`;
//! - a content browser, or a request nobody can attribute, gets 403 (failing closed);
//! - the engine is shutting down: 503 for new requests.
//!
//! Attribution is by browser id of a view the core created, never by URL (docs/architecture.md
//! section 8). No response carries an `Access-Control-*` header.

use crate::assets::Asset;
use crate::log::elog;
use crate::engine::Shared;
use crate::rpc::{RPC_PATH, UiCaller};
use cef::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use yaac_api::{API_VERSION, ApiError, ApiResult, ErrorCode, Request as ApiRequest, Response as ApiResponse};
use yaac_engine::ViewKind;

pub(crate) const SCHEME: &str = "app";
pub(crate) const DOMAIN: &str = "ui";

/// Largest RPC body accepted, in bytes.
const MAX_BODY: usize = 1 << 20;
/// Most post-data elements accepted. A form or fetch body is one element.
const MAX_ELEMENTS: usize = 16;
/// RPCs accepted and not yet answered. Beyond this a request gets 503.
const RPC_INFLIGHT_LIMIT: usize = 64;
/// The only origin a UI page sends.
const UI_ORIGIN: &str = "app://ui";

/// The Content-Security-Policy sent with every UI HTML page. Same string as ui/csp.mjs, which the
/// UI build also writes into the page as a meta tag.
pub(crate) const UI_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src app://ui; frame-src 'none'; base-uri 'none'; form-action 'none'";

/// The scheme flags. The only place they are set.
///
/// - STANDARD: parsed like http, with a real origin, so `'self'` and module scripts work.
/// - SECURE: counts as a secure context.
/// - FETCH_ENABLED: the UI's `fetch()` to `app://ui/api/v1/rpc` needs it. Without it the page never
///   finishes loading (observed in the M1 spike, see crates/yaac-app/README.md).
///
/// Not set, on purpose:
/// - CORS_ENABLED: puts `app` on Chromium's list of schemes that accept CORS requests, so remote
///   pages could send requests to `app://ui/api`. Remote pages are refused without it (E-IPC-1).
fn scheme_option_bits() -> i32 {
    (SchemeOptions::STANDARD.get_raw() | SchemeOptions::SECURE.get_raw() | SchemeOptions::FETCH_ENABLED.get_raw()) as i32
}

pub(crate) fn register(registrar: Option<&mut SchemeRegistrar>) {
    let Some(registrar) = registrar else { return };
    registrar.add_custom_scheme(Some(&CefString::from(SCHEME)), scheme_option_bits());
}

/// The body of a POST, as far as the engine will read it.
pub(crate) enum PostBody {
    Bytes(Vec<u8>),
    TooLarge,
    Unsupported,
}

/// What a request resolves to. Decided on the IO thread, before any body is read.
pub(crate) enum Plan {
    Static {
        status: i32,
        mime: &'static str,
        body: Vec<u8>,
        csp: bool,
    },
    Rpc {
        caller: UiCaller,
        body: Vec<u8>,
    },
}

/// The request headers the decision uses. Empty when absent.
pub(crate) struct Headers {
    pub(crate) content_type: String,
    pub(crate) origin: String,
}

wrap_scheme_handler_factory! {
    pub(crate) struct AppSchemeFactory {
        shared: Arc<Shared>,
    }

    impl SchemeHandlerFactory {
        fn create(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut cef::Request>,
        ) -> Option<ResourceHandler> {
            let request = request?;
            let browser_id = browser.map(|b| b.identifier());
            let main_frame = frame.map(|f| f.is_main() != 0).unwrap_or(false);
            let method = CefString::from(&request.method()).to_string();
            let url = CefString::from(&request.url()).to_string();
            let headers = Headers {
                content_type: header(request, "Content-Type"),
                origin: header(request, "Origin"),
            };
            let plan = plan(&self.shared, browser_id, main_frame, &method, &url, &headers, || {
                read_post_body(request)
            });
            if matches!(plan, Plan::Static { status, .. } if status >= 400) {
                self.shared.rejections.fetch_add(1, Ordering::SeqCst);
            }
            Some(AppResource::new(
                self.shared.clone(),
                Arc::new(Mutex::new(Some(plan))),
                Arc::new(Mutex::new(Reply::default())),
            ))
        }
    }
}

fn header(request: &mut cef::Request, name: &str) -> String {
    CefString::from(&request.header_by_name(Some(&CefString::from(name)))).to_string()
}

/// The path of an `app://ui` URL, without query or fragment. Empty for any other URL.
fn request_path(url: &str) -> &str {
    let Some(rest) = url.strip_prefix("app://ui") else {
        return "";
    };
    rest.split(['?', '#']).next().unwrap_or("")
}

/// True for `application/json`, with or without parameters.
pub(crate) fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

/// Decides one request. `body` is called only for a sidebar RPC that passed every other check, so
/// an unattributed or refused request never costs a read of its body.
pub(crate) fn plan(
    shared: &Shared,
    browser: Option<i32>,
    main_frame: bool,
    method: &str,
    url: &str,
    headers: &Headers,
    body: impl FnOnce() -> PostBody,
) -> Plan {
    // Only GET (assets) and POST (RPC) exist. Everything else, OPTIONS included, gets 405.
    if method != "GET" && method != "POST" {
        return refuse(405, "method not allowed");
    }
    if shared.quitting.load(Ordering::SeqCst) {
        return refuse(503, "shutting down");
    }
    let path = request_path(url);
    let Some((view, kind)) = browser.and_then(|id| shared.registry.browser(id)) else {
        elog!("refused request caller=unattributed method={method} path={path}");
        return refuse(403, "forbidden");
    };
    if kind != ViewKind::Sidebar {
        elog!("refused request caller=content view={view} method={method} path={path}");
        return refuse(403, "forbidden");
    }

    if path == RPC_PATH {
        if method != "POST" {
            return refuse(405, "method not allowed");
        }
        if !is_json(&headers.content_type) {
            return refuse(415, "unsupported media type");
        }
        if !headers.origin.is_empty() && headers.origin != UI_ORIGIN {
            return refuse(403, "forbidden");
        }
        if shared.rpc_inflight.load(Ordering::SeqCst) >= RPC_INFLIGHT_LIMIT {
            return refuse(503, "busy");
        }
        return match body() {
            PostBody::Bytes(bytes) => Plan::Rpc {
                caller: UiCaller { view, main_frame },
                body: bytes,
            },
            PostBody::TooLarge => refuse(413, "request too large"),
            PostBody::Unsupported => refuse(400, "unsupported request body"),
        };
    }

    if method != "GET" {
        return refuse(405, "method not allowed");
    }
    match shared.assets.asset(path.trim_start_matches('/')) {
        Some(Asset { mime, body }) => Plan::Static {
            status: 200,
            mime,
            body,
            csp: shared.assets.enforces_csp() && mime == "text/html",
        },
        None => Plan::Static {
            status: 404,
            mime: "text/plain",
            body: b"not found".to_vec(),
            csp: false,
        },
    }
}

fn refuse(status: i32, message: &str) -> Plan {
    let body = format!(r#"{{"ok":false,"error":"{message}"}}"#).into_bytes();
    Plan::Static {
        status,
        mime: "application/json",
        body,
        csp: false,
    }
}

/// Reads a POST body within `MAX_BODY`. A file upload, or more elements than `MAX_ELEMENTS`, is
/// refused: the RPC body is always inline bytes.
fn read_post_body(request: &mut cef::Request) -> PostBody {
    let Some(post) = request.post_data() else {
        return PostBody::Bytes(Vec::new());
    };
    let count = post.element_count();
    if count > MAX_ELEMENTS {
        return PostBody::TooLarge;
    }
    // cef-rs fills a vector sized to the element count; an empty vector gets nothing.
    let mut elements: Vec<Option<PostDataElement>> = (0..count).map(|_| None).collect();
    post.elements(Some(&mut elements));
    let mut out = Vec::new();
    for element in elements.into_iter().flatten() {
        if element.get_type() != PostdataelementType::BYTES {
            return PostBody::Unsupported;
        }
        // Read at most one byte past the limit, so the size check is exact without an allocation
        // sized by the sender.
        let budget = (MAX_BODY + 1).saturating_sub(out.len());
        let size = element.bytes_count();
        let want = size.min(budget);
        let mut chunk = vec![0u8; want];
        let read = element.bytes(want, chunk.as_mut_ptr());
        chunk.truncate(read);
        out.extend_from_slice(&chunk);
        if size > budget || out.len() > MAX_BODY {
            return PostBody::TooLarge;
        }
    }
    PostBody::Bytes(out)
}

/// The response state. Filled either synchronously or from the RPC runtime.
#[derive(Default)]
struct Reply {
    status: i32,
    mime: &'static str,
    body: Vec<u8>,
    offset: usize,
    csp: bool,
}

/// `Callback` is a CEF object. CEF documents `Continue` as callable from any thread, which is what
/// lets the RPC runtime resume a request the IO thread started.
struct SendCallback(Callback);

// SAFETY: CefCallback::Continue is documented by CEF as callable from any thread. It is the only
// method called from another thread, and it is called exactly once per request.
unsafe impl Send for SendCallback {}

/// Counts one RPC in flight until it is dropped, including when its task is dropped at shutdown.
struct InflightGuard(Arc<Shared>);

impl InflightGuard {
    fn new(shared: Arc<Shared>) -> Self {
        shared.rpc_inflight.fetch_add(1, Ordering::SeqCst);
        Self(shared)
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.rpc_inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

wrap_resource_handler! {
    pub(crate) struct AppResource {
        shared: Arc<Shared>,
        plan: Arc<Mutex<Option<Plan>>>,
        reply: Arc<Mutex<Reply>>,
    }

    impl ResourceHandler {
        fn process_request(
            &self,
            _request: Option<&mut cef::Request>,
            callback: Option<&mut Callback>,
        ) -> i32 {
            let Some(callback) = callback.cloned() else { return 0 };
            let plan = self.plan.lock().ok().and_then(|mut p| p.take());
            match plan {
                Some(Plan::Static { status, mime, body, csp }) => {
                    set_reply(&self.reply, status, mime, body, csp);
                    callback.cont();
                    1
                }
                Some(Plan::Rpc { caller, body }) => {
                    let guard = InflightGuard::new(self.shared.clone());
                    let shared = self.shared.clone();
                    let reply = self.reply.clone();
                    let callback = SendCallback(callback);
                    self.shared.rpc_runtime.spawn(async move {
                        let _guard = guard;
                        let (status, out) = answer_rpc(&shared, caller, &body).await;
                        set_reply(&reply, status, "application/json", out, false);
                        callback.0.cont();
                    });
                    1
                }
                None => 0,
            }
        }

        fn response_headers(
            &self,
            response: Option<&mut cef::Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let Ok(reply) = self.reply.lock() else { return };
            if let Some(response) = response {
                response.set_status(reply.status);
                response.set_mime_type(Some(&CefString::from(reply.mime)));
                response.set_header_by_name(
                    Some(&CefString::from("X-Content-Type-Options")),
                    Some(&CefString::from("nosniff")),
                    1,
                );
                response.set_header_by_name(
                    Some(&CefString::from("Cache-Control")),
                    Some(&CefString::from("no-store")),
                    1,
                );
                if reply.csp {
                    response.set_header_by_name(
                        Some(&CefString::from("Content-Security-Policy")),
                        Some(&CefString::from(UI_CSP)),
                        1,
                    );
                }
            }
            if let Some(response_length) = response_length {
                *response_length = reply.body.len() as i64;
            }
        }

        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: i32,
            bytes_read: Option<&mut i32>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> i32 {
            copy_body(&self.reply, data_out, bytes_to_read, bytes_read)
        }

        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read_response(
            &self,
            data_out: *mut u8,
            bytes_to_read: i32,
            bytes_read: Option<&mut i32>,
            _callback: Option<&mut Callback>,
        ) -> i32 {
            copy_body(&self.reply, data_out, bytes_to_read, bytes_read)
        }
    }
}

/// Copies the next part of the body into CEF's buffer. Returns 0 at the end of the body.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
fn copy_body(reply: &Mutex<Reply>, data_out: *mut u8, bytes_to_read: i32, bytes_read: Option<&mut i32>) -> i32 {
    let Some(bytes_read) = bytes_read else { return 0 };
    *bytes_read = 0;
    let Ok(mut reply) = reply.lock() else { return 0 };
    let remaining = reply.body.len() - reply.offset;
    let count = remaining.min(bytes_to_read.max(0) as usize);
    if count == 0 || data_out.is_null() {
        return 0;
    }
    // SAFETY: `data_out` is CEF's buffer for this read, with room for `bytes_to_read` bytes, and
    // `count` never exceeds that. The source range lies inside `reply.body` because
    // `offset + count <= body.len()`.
    unsafe {
        std::ptr::copy_nonoverlapping(reply.body.as_ptr().add(reply.offset), data_out, count);
    }
    reply.offset += count;
    *bytes_read = count as i32;
    1
}

fn set_reply(reply: &Mutex<Reply>, status: i32, mime: &'static str, body: Vec<u8>, csp: bool) {
    if let Ok(mut reply) = reply.lock() {
        reply.status = status;
        reply.mime = mime;
        reply.body = body;
        reply.offset = 0;
        reply.csp = csp;
    }
}

/// Decodes the request, asks the host, and encodes the response. Runs on the RPC runtime.
async fn answer_rpc(shared: &Shared, caller: UiCaller, body: &[u8]) -> (i32, Vec<u8>) {
    let request: ApiRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => {
            elog!("rpc rejected view={} reason=decode", caller.view);
            let response = error_response(0, ErrorCode::BadRequest, "malformed request");
            return (400, serde_json::to_vec(&response).unwrap_or_default());
        }
    };
    let id = request.id;
    if request.v != API_VERSION {
        let response = error_response(id, ErrorCode::UnsupportedVersion, "unsupported api version");
        return (200, serde_json::to_vec(&response).unwrap_or_default());
    }
    let response: ApiResponse = shared.rpc.call(caller, request).await;
    shared.rpc_answered.fetch_add(1, Ordering::SeqCst);
    elog!("rpc answered view={} main_frame={}", caller.view, caller.main_frame);
    (200, serde_json::to_vec(&response).unwrap_or_default())
}

fn error_response(id: u64, code: ErrorCode, message: &str) -> ApiResponse {
    ApiResponse {
        id,
        result: ApiResult::Err {
            error: ApiError {
                code,
                message: message.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{EmbeddedAssets, UiAssets};
    use crate::coalesce::{Coalescer, PopupLimiter};
    use crate::registry::Registry;
    use crate::rpc::RpcHandler;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize};
    use std::time::Duration;
    use yaac_model::ViewId;

    struct NoRpc;
    impl RpcHandler for NoRpc {
        fn call(&self, _caller: UiCaller, _request: ApiRequest) -> Pin<Box<dyn Future<Output = ApiResponse> + Send>> {
            unreachable!("the planner never calls the handler")
        }
    }

    /// Serves one html page that asks for CSP, and one plain asset that does not.
    struct CspAssets;
    impl UiAssets for CspAssets {
        fn asset(&self, path: &str) -> Option<Asset> {
            match path {
                "index.html" | "sidebar.html" => Some(Asset { mime: "text/html", body: b"<p>".to_vec() }),
                "app.js" => Some(Asset { mime: "text/javascript", body: b"1".to_vec() }),
                _ => None,
            }
        }
    }

    fn shared_with(assets: Arc<dyn UiAssets>) -> (Shared, tokio::runtime::Runtime) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
        let shared = Shared {
            events,
            assets,
            rpc: Arc::new(NoRpc),
            rpc_runtime: runtime.handle().clone(),
            registry: Registry::default(),
            data_dir: std::env::temp_dir(),
            exit_code: AtomicI32::new(0),
            rejections: AtomicU64::new(0),
            rpc_answered: AtomicU64::new(0),
            rpc_inflight: AtomicUsize::new(0),
            quitting: AtomicBool::new(false),
            live: [AtomicUsize::new(0), AtomicUsize::new(0)],
            coalescer: Mutex::new(Coalescer::default()),
            flush_scheduled: AtomicBool::new(false),
            popups: Mutex::new(PopupLimiter::new(4, Duration::from_secs(1))),
        };
        (shared, runtime)
    }

    fn json_headers() -> Headers {
        Headers {
            content_type: "application/json".into(),
            origin: "app://ui".into(),
        }
    }

    const SIDEBAR: i32 = 1;
    const CONTENT: i32 = 2;

    fn bound(shared: &Shared) {
        shared.registry.bind_browser(SIDEBAR, ViewId(1), ViewKind::Sidebar);
        shared.registry.bind_browser(CONTENT, ViewId(2), ViewKind::Content);
    }

    fn status_of(plan: &Plan) -> Option<i32> {
        match plan {
            Plan::Static { status, .. } => Some(*status),
            Plan::Rpc { .. } => None,
        }
    }

    #[test]
    fn unattributed_and_content_requests_are_refused_without_reading_the_body() {
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[])));
        bound(&shared);
        let never = || -> PostBody { panic!("the body must not be read for a refused caller") };
        let unknown = plan(&shared, None, true, "POST", "app://ui/api/v1/rpc", &json_headers(), never);
        assert_eq!(status_of(&unknown), Some(403));
        let content = plan(&shared, Some(CONTENT), true, "POST", "app://ui/api/v1/rpc", &json_headers(), never);
        assert_eq!(status_of(&content), Some(403));
        let asset = plan(&shared, Some(CONTENT), true, "GET", "app://ui/sidebar.html", &json_headers(), never);
        assert_eq!(status_of(&asset), Some(403));
    }

    #[test]
    fn only_get_and_post_exist_and_options_is_405_for_everyone() {
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[])));
        bound(&shared);
        for caller in [None, Some(SIDEBAR), Some(CONTENT)] {
            for method in ["OPTIONS", "PUT", "DELETE", "TRACE"] {
                let p = plan(&shared, caller, true, method, "app://ui/api/v1/rpc", &json_headers(), || PostBody::Bytes(vec![]));
                assert_eq!(status_of(&p), Some(405), "{method} from {caller:?}");
            }
        }
    }

    #[test]
    fn sidebar_rpc_needs_json_and_the_ui_origin() {
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[])));
        bound(&shared);
        let body = || PostBody::Bytes(b"{}".to_vec());
        let ok = plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &json_headers(), body);
        assert!(matches!(ok, Plan::Rpc { .. }));
        let text = Headers { content_type: "text/plain".into(), origin: "app://ui".into() };
        assert_eq!(status_of(&plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &text, body)), Some(415));
        let foreign = Headers { content_type: "application/json".into(), origin: "https://evil.test".into() };
        assert_eq!(status_of(&plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &foreign, body)), Some(403));
        let no_origin = Headers { content_type: "application/json; charset=utf-8".into(), origin: String::new() };
        assert!(matches!(plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &no_origin, body), Plan::Rpc { .. }));
        assert_eq!(status_of(&plan(&shared, Some(SIDEBAR), true, "GET", "app://ui/api/v1/rpc", &json_headers(), body)), Some(405));
    }

    #[test]
    fn oversized_and_unsupported_bodies_are_refused() {
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[])));
        bound(&shared);
        let big = plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &json_headers(), || PostBody::TooLarge);
        assert_eq!(status_of(&big), Some(413));
        let file = plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &json_headers(), || PostBody::Unsupported);
        assert_eq!(status_of(&file), Some(400));
    }

    #[test]
    fn quitting_answers_503_to_everyone() {
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[])));
        bound(&shared);
        shared.quitting.store(true, Ordering::SeqCst);
        let p = plan(&shared, Some(SIDEBAR), true, "POST", "app://ui/api/v1/rpc", &json_headers(), || PostBody::Bytes(vec![]));
        assert_eq!(status_of(&p), Some(503));
    }

    #[test]
    fn sidebar_assets_are_served_and_html_gets_the_csp() {
        let (shared, _rt) = shared_with(Arc::new(CspAssets));
        bound(&shared);
        let no_body = || PostBody::Bytes(vec![]);
        match plan(&shared, Some(SIDEBAR), true, "GET", "app://ui/sidebar.html?x=1#f", &json_headers(), no_body) {
            Plan::Static { status, csp, mime, .. } => {
                assert_eq!((status, mime, csp), (200, "text/html", true));
            }
            Plan::Rpc { .. } => panic!("an asset is not an RPC"),
        }
        match plan(&shared, Some(SIDEBAR), true, "GET", "app://ui/app.js", &json_headers(), no_body) {
            Plan::Static { status, csp, mime, .. } => {
                assert_eq!((status, mime, csp), (200, "text/javascript", false));
            }
            Plan::Rpc { .. } => panic!("an asset is not an RPC"),
        }
        assert_eq!(status_of(&plan(&shared, Some(SIDEBAR), true, "GET", "app://ui/missing.html", &json_headers(), no_body)), Some(404));
    }

    #[test]
    fn harness_assets_get_no_csp() {
        // The placeholder pages keep their inline scripts; they are never served as the real UI.
        let (shared, _rt) = shared_with(Arc::new(EmbeddedAssets::new(&[("sidebar.html", "text/html", b"<p>")])));
        bound(&shared);
        match plan(&shared, Some(SIDEBAR), true, "GET", "app://ui/sidebar.html", &json_headers(), || PostBody::Bytes(vec![])) {
            Plan::Static { csp, .. } => assert!(!csp),
            Plan::Rpc { .. } => panic!("an asset is not an RPC"),
        }
    }

    #[test]
    fn json_media_type_is_recognised() {
        assert!(is_json("application/json"));
        assert!(is_json("Application/JSON; charset=utf-8"));
        assert!(is_json(" application/json "));
        assert!(!is_json("text/plain"));
        assert!(!is_json("application/json-seq"));
        assert!(!is_json(""));
    }

    #[test]
    fn request_paths_drop_query_and_fragment() {
        assert_eq!(request_path("app://ui/api/v1/rpc?x=1"), "/api/v1/rpc");
        assert_eq!(request_path("app://ui/a#b"), "/a");
        assert_eq!(request_path("app://ui"), "");
        assert_eq!(request_path("https://ui/a"), "");
    }

    #[test]
    fn the_csp_matches_the_ui_build() {
        assert!(UI_CSP.contains("connect-src app://ui"));
        assert!(UI_CSP.contains("frame-src 'none'"));
        assert!(!UI_CSP.contains("unsafe"));
    }
}
