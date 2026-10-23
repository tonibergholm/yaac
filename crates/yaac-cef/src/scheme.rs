//! The `app://ui` scheme. The handler attributes each request to a browser, then:
//! - sidebar (UI) browsers get the bundled files and may call `POST /api/v1/rpc`;
//! - content browsers and unattributed requests get 403, with no CORS headers anywhere.
//!
//! Attribution is by browser id of a view the core created, never by URL (docs/architecture.md
//! section 8). Failing closed: no attribution means refusal.

use crate::assets::Asset;
use crate::engine::Shared;
use crate::log::elog;
use crate::rpc::{RPC_PATH, UiCaller};
use cef::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use yaac_api::{
    API_VERSION, ApiError, ApiResult, ErrorCode, Request as ApiRequest, Response as ApiResponse,
};
use yaac_engine::ViewKind;

pub(crate) const SCHEME: &str = "app";
pub(crate) const DOMAIN: &str = "ui";

/// Largest RPC body accepted.
const MAX_BODY: usize = 1 << 20;

/// The scheme flags. The only place they are set. Each flag is listed with what it does, so the
/// set that ships for E-IPC-1 is readable here.
///
/// - STANDARD: parsed like http, with a real origin, so `'self'` and module scripts work.
/// - SECURE: counts as a secure context.
///
/// Not set, on purpose:
/// - CORS_ENABLED: puts `app` on Chromium's list of schemes that accept CORS requests, so remote
///   pages could send requests to `app://ui/api`. Add only if same-origin requests fail without it.
/// - FETCH_ENABLED: only if `fetch()` from a UI page fails without it (see the result in
///   crates/yaac-app/README.md).
fn scheme_option_bits() -> i32 {
    (SchemeOptions::STANDARD.get_raw()
        | SchemeOptions::SECURE.get_raw()
        | SchemeOptions::FETCH_ENABLED.get_raw()) as i32
}

pub(crate) fn register(registrar: Option<&mut SchemeRegistrar>) {
    let Some(registrar) = registrar else { return };
    registrar.add_custom_scheme(Some(&CefString::from(SCHEME)), scheme_option_bits());
}

/// What a request resolves to, decided on the IO thread.
enum Plan {
    Static {
        status: i32,
        mime: &'static str,
        body: Vec<u8>,
    },
    Rpc {
        caller: UiCaller,
        body: Vec<u8>,
    },
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
            let body = if method == "POST" { post_body(request) } else { Vec::new() };
            let plan = plan(&self.shared, browser_id, main_frame, &method, &url, &headers, body);
            if matches!(&plan, Plan::Static { status, .. } if *status >= 400) {
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

/// Request headers the handler decides on. Empty when absent.
struct Headers {
    content_type: String,
    origin: String,
}

fn header(request: &mut cef::Request, name: &str) -> String {
    CefString::from(&request.header_by_name(Some(&CefString::from(name)))).to_string()
}

fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .map(|media| media.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

fn refuse(status: i32, message: &str) -> Plan {
    let body = format!(r#"{{"ok":false,"error":"{message}"}}"#).into_bytes();
    Plan::Static {
        status,
        mime: "application/json",
        body,
    }
}

fn plan(
    shared: &Shared,
    browser: Option<i32>,
    main_frame: bool,
    method: &str,
    url: &str,
    headers: &Headers,
    body: Vec<u8>,
) -> Plan {
    // Only GET (assets) and POST (RPC) exist. Everything else, OPTIONS included, gets 405.
    if method != "GET" && method != "POST" {
        return refuse(405, "method not allowed");
    }
    let path = url
        .strip_prefix("app://ui")
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_string();

    let attributed = browser.and_then(|id| shared.registry.browser(id));
    let Some((view, kind)) = attributed else {
        elog!("refused scheme caller=unattributed method={method} path={path}");
        return refuse(403, "forbidden");
    };
    if kind != ViewKind::Sidebar {
        elog!("refused scheme caller=content view={view} method={method} path={path}");
        return refuse(403, "forbidden");
    }

    if path == RPC_PATH {
        if method != "POST" {
            return refuse(405, "method not allowed");
        }
        if !is_json(&headers.content_type) {
            return refuse(415, "unsupported media type");
        }
        // The page sends its own origin. Any other origin is refused.
        if !headers.origin.is_empty() && headers.origin != "app://ui" {
            return refuse(403, "forbidden");
        }
        if body.len() > MAX_BODY {
            return refuse(413, "request too large");
        }
        return Plan::Rpc {
            caller: UiCaller { view, main_frame },
            body,
        };
    }

    if method != "GET" {
        return refuse(405, "method not allowed");
    }
    let rest = path.trim_start_matches('/');
    match shared.assets.asset(rest) {
        Some(Asset { mime, body }) => Plan::Static {
            status: 200,
            mime,
            body,
        },
        None => Plan::Static {
            status: 404,
            mime: "text/plain",
            body: b"not found".to_vec(),
        },
    }
}

fn post_body(request: &mut cef::Request) -> Vec<u8> {
    let Some(post) = request.post_data() else {
        return Vec::new();
    };
    // cef-rs fills a vector sized to the element count; an empty vector gets nothing.
    let mut elements: Vec<Option<PostDataElement>> =
        (0..post.element_count()).map(|_| None).collect();
    post.elements(Some(&mut elements));
    let mut out = Vec::new();
    for element in elements.into_iter().flatten() {
        if element.get_type() != PostdataelementType::BYTES {
            continue;
        }
        let count = element.bytes_count();
        if out.len() + count > MAX_BODY {
            break;
        }
        let mut chunk = vec![0u8; count];
        let read = element.bytes(count, chunk.as_mut_ptr());
        chunk.truncate(read);
        out.extend_from_slice(&chunk);
    }
    out
}

/// The response state. Filled either synchronously or from the RPC runtime.
#[derive(Default)]
struct Reply {
    status: i32,
    mime: &'static str,
    body: Vec<u8>,
    offset: usize,
}

/// `Callback` is a CEF object. CEF documents `Continue` as callable from any thread, which is what
/// lets the RPC runtime resume a request the IO thread started.
struct SendCallback(Callback);

// SAFETY: CefCallback::Continue is thread-safe in CEF; it is the only method called from another
// thread, and it is called once.
unsafe impl Send for SendCallback {}

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
                Some(Plan::Static { status, mime, body }) => {
                    set_reply(&self.reply, status, mime, body);
                    callback.cont();
                    1
                }
                Some(Plan::Rpc { caller, body }) => {
                    let shared = self.shared.clone();
                    let reply = self.reply.clone();
                    let callback = SendCallback(callback);
                    shared.clone().rpc_runtime.spawn(async move {
                        let (status, out) = answer_rpc(&shared, caller, &body).await;
                        set_reply(&reply, status, "application/json", out);
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

/// Copies the next part of the body. Returns 0 at the end of the body.
fn copy_body(
    reply: &Mutex<Reply>,
    data_out: *mut u8,
    bytes_to_read: i32,
    bytes_read: Option<&mut i32>,
) -> i32 {
    let Some(bytes_read) = bytes_read else {
        return 0;
    };
    *bytes_read = 0;
    let Ok(mut reply) = reply.lock() else {
        return 0;
    };
    let remaining = reply.body.len() - reply.offset;
    let count = remaining.min(bytes_to_read.max(0) as usize);
    if count == 0 {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(reply.body.as_ptr().add(reply.offset), data_out, count);
    }
    reply.offset += count;
    *bytes_read = count as i32;
    1
}

fn set_reply(reply: &Mutex<Reply>, status: i32, mime: &'static str, body: Vec<u8>) {
    if let Ok(mut reply) = reply.lock() {
        reply.status = status;
        reply.mime = mime;
        reply.body = body;
        reply.offset = 0;
    }
}

/// Decodes the request, asks the host, and encodes the response. Runs on the RPC runtime.
async fn answer_rpc(shared: &Shared, caller: UiCaller, body: &[u8]) -> (i32, Vec<u8>) {
    let request: ApiRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(error) => {
            elog!(
                "rpc rejected view={} reason=decode error={error}",
                caller.view
            );
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
    elog!(
        "rpc answered view={} main_frame={} id={id}",
        caller.view,
        caller.main_frame
    );
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
