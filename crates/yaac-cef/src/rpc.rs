//! The RPC contract between the `app://ui` scheme and the host. The engine decodes the request and
//! attributes the caller; the host decides what the request may do.

use std::future::Future;
use std::pin::Pin;
use yaac_api::{Request, Response};
use yaac_model::ViewId;

/// `POST app://ui/api/v1/rpc` with a JSON `yaac_api::Request` body.
pub const RPC_PATH: &str = "/api/v1/rpc";

/// The authenticated caller. Built by the engine from the browser id of a view the core created
/// with `ViewKind::Sidebar`. Never built from the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiCaller {
    pub view: ViewId,
    /// False for requests from a subframe of the UI browser. Policy-editing commands require true.
    pub main_frame: bool,
}

/// Answers UI requests. Called on the RPC runtime, never on the UI thread. The returned future is
/// driven to completion there; the scheme response is sent when it resolves.
pub trait RpcHandler: Send + Sync {
    fn call(
        &self,
        caller: UiCaller,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Response> + Send>>;
}
