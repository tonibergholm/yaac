//! The CEF implementation of `yaac-engine`, on cef-rs 154.5.0 (CEF 154.0.34). The only crate that
//! names CEF types. Everything CEF-specific stays on the UI (main) thread; `CefEngine` carries ids
//! and plain data only.
//!
//! Layout:
//! - `lifecycle`: browser and helper process entry points (`run_browser`, `run_helper`).
//! - `engine`: the `Send + Sync` handle (`CefEngine`) and the settings it is built from.
//! - `ui`: UI-thread state (windows, views, request contexts) and command execution.
//! - `client`, `window`: CEF handler implementations that report back as `EngineEvent`s.
//! - `scheme`: the `app://ui` scheme, its authorization and the RPC bridge.
//! - `assets`, `rpc`, `registry`: plain-data helpers shared by the above.

#[cfg(not(target_os = "macos"))]
compile_error!("yaac-cef implements only macOS in M1 (see docs/m1-plan.md)");

mod assets;
mod client;
mod engine;
mod lifecycle;
mod log;
mod registry;
mod rpc;
mod scheme;
mod ui;
mod window;

#[cfg(target_os = "macos")]
mod macos;

pub use assets::{Asset, DirAssets, EmbeddedAssets, UiAssets};
pub use engine::{CefEngine, EngineSettings, PushError, post_to_ui};
pub use lifecycle::{run_browser, run_helper};
pub use rpc::{RPC_PATH, RpcHandler, UiCaller};
