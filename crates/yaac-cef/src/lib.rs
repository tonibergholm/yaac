//! The CEF implementation of `yaac-engine`, on cef-rs 154.5.0 (CEF 154.0.34). The only crate that
//! names CEF types. CEF objects stay on the UI (main) thread; `CefEngine` carries ids and plain data.
//!
//! Layout:
//! - `lifecycle`: browser and helper process entry points (`run_browser`, `run_helper`).
//! - `engine`: the `Send + Sync` handle (`CefEngine`), the shutdown gate and the event flush.
//! - `ui`: UI-thread state (windows, views, request contexts), the ordered command queue and the
//!   window-close protocol.
//! - `client`, `window`: CEF handlers that report back as `EngineEvent`s.
//! - `scheme`: the `app://ui` scheme, its authorization and the RPC bridge.
//! - `assets`, `rpc`, `registry`: files, RPC contract and browser attribution.
//! - `coalesce`, `nav`, `text`, `paths`: bounds on page-driven events, navigation generations,
//!   caps and log redaction, private directories. Pure, and unit-tested.

#[cfg(not(target_os = "macos"))]
compile_error!("yaac-cef implements only macOS in M1 (see docs/m1-plan.md)");

mod assets;
mod client;
mod coalesce;
mod engine;
mod lifecycle;
mod log;
mod nav;
mod registry;
mod rpc;
mod scheme;
mod text;
mod ui;
mod window;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub use assets::DirAssets;
#[cfg(all(feature = "embed-ui", not(debug_assertions)))]
pub use assets::embedded_ui;
pub use assets::{Asset, EmbeddedAssets, UiAssets};
#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub use engine::test_hooks;
pub use engine::{CefEngine, EngineSettings, PushError, post_to_ui};
pub use lifecycle::{run_browser, run_helper};
pub use rpc::{RPC_PATH, RpcHandler, UiCaller};
