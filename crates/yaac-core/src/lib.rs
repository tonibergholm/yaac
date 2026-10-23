//! The state actor: one writer, one total order of commands. Depends on traits only; it never names
//! CEF, objc2 or SQLite. It defines the `Store` port that `yaac-store` implements.

mod store;

pub use store::{Store, StoreError, StoreOp};

#[cfg(feature = "testing")]
pub mod testing;

use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use yaac_api::{Event, Request, Response};
use yaac_engine::{EngineEvent, EngineHandle};
use yaac_model::ViewId;

/// Who is calling. Built by the ingress after it authenticates the caller; never read from the
/// request. M1 has only the UI and the system; agents and importers arrive later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// A request from the main frame of a registered UI browser.
    Ui {
        view: ViewId,
    },
    System,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("core stopped")]
    Stopped,
}

/// Cheap to clone. All state lives in the actor behind it.
#[derive(Clone)]
pub struct CoreHandle {
    _private: (),
}

impl CoreHandle {
    pub async fn request(&self, principal: Principal, request: Request) -> Response {
        let _ = (principal, request);
        todo!("m1/core")
    }

    /// Engine callbacks go in here; the actor drops stale ones by navigation generation.
    pub fn engine_events(&self) -> mpsc::UnboundedSender<EngineEvent> {
        todo!("m1/core")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        todo!("m1/core")
    }
}

/// Loads the workspace from `store` (creating the first profile, Space and window on an empty
/// store), reconciles it with the engine (every restored tab comes up suspended; the selected tab
/// is loaded) and starts the actor.
pub fn spawn(
    store: Box<dyn Store>,
    engine: Arc<dyn EngineHandle>,
) -> Result<CoreHandle, CoreError> {
    let _ = (store, engine);
    todo!("m1/core")
}
