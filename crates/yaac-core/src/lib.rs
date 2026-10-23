//! The state actor: one writer, one total order of commands. Depends on traits only; it never names
//! CEF, objc2 or SQLite. It defines the `Store` port that `yaac-store` implements.

mod actor;
mod auth;
mod commands;
mod error;
mod events;
mod navigation;
mod startup;
mod state;
mod store;
mod urls;

pub use store::{Store, StoreError, StoreOp};

#[cfg(feature = "testing")]
pub mod testing;

#[cfg(all(test, feature = "testing"))]
mod tests;

use actor::{Actor, Message};
use state::Plan;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};
use yaac_api::{ApiError, ApiResult, ErrorCode, Event, Request, Response};
use yaac_engine::{EngineEvent, EngineHandle};
use yaac_model::ViewId;

const REQUEST_QUEUE: usize = 256;
const EVENT_QUEUE: usize = 256;

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
    requests: mpsc::Sender<Message>,
    engine_events: mpsc::UnboundedSender<EngineEvent>,
    events: broadcast::Sender<Event>,
}

impl CoreHandle {
    /// Authorizes and runs one request. Once the core has stopped, every request answers
    /// `Internal`.
    pub async fn request(&self, principal: Principal, request: Request) -> Response {
        let id = request.id;
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(Message {
                principal,
                request,
                reply,
            })
            .await
            .is_err()
        {
            return stopped(id);
        }
        answer.await.unwrap_or_else(|_| stopped(id))
    }

    /// Engine callbacks go in here; the actor drops stale ones by navigation generation.
    pub fn engine_events(&self) -> mpsc::UnboundedSender<EngineEvent> {
        self.engine_events.clone()
    }

    /// Published after every committed or runtime change, with the new revision.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
}

fn stopped(id: u64) -> Response {
    Response {
        id,
        result: ApiResult::Err {
            error: ApiError {
                code: ErrorCode::Internal,
                message: "the core has stopped".to_owned(),
            },
        },
    }
}

/// Loads the workspace from `store` (creating the first profile, Space and window on an empty
/// store), reconciles it with the engine (every restored tab comes up suspended; the selected tab
/// is loaded) and starts the actor.
///
/// Needs no Tokio runtime: the store and the actor each run on their own thread. Startup commits
/// and engine commands are complete when this returns.
pub fn spawn(
    store: Box<dyn Store>,
    engine: Arc<dyn EngineHandle>,
) -> Result<CoreHandle, CoreError> {
    let (jobs, job_queue) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("yaac-store".into())
        .spawn(move || actor::run_store(store, job_queue))
        .expect("the store thread can start");

    let workspace = actor::load_blocking(&jobs)?;
    let Plan {
        state,
        ops,
        effects,
        ..
    } = startup::initialize(workspace)?;
    if !ops.is_empty() {
        actor::commit_blocking(&jobs, ops)?;
    }
    for command in effects {
        engine.send(command);
    }

    let (requests, request_queue) = mpsc::channel(REQUEST_QUEUE);
    let (engine_events, engine_queue) = mpsc::unbounded_channel();
    let (events, _) = broadcast::channel(EVENT_QUEUE);
    let actor = Actor::new(state, jobs, engine, events.clone());
    std::thread::Builder::new()
        .name("yaac-core".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("the core runtime can start");
            runtime.block_on(actor.run(request_queue, engine_queue));
        })
        .expect("the core thread can start");

    Ok(CoreHandle {
        requests,
        engine_events,
        events,
    })
}
