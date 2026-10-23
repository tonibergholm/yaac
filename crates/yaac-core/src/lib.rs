//! The state actor: one writer, one total order of commands. Depends on traits only; it never names
//! CEF, objc2 or SQLite. It defines the `Store` port that `yaac-store` implements.

mod actor;
mod auth;
mod clock;
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

use actor::{Actor, Inbound, Message};
use clock::{Clock, SystemClock};
use state::Plan;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use tokio::sync::{broadcast, mpsc, oneshot};
use yaac_api::{ApiError, ApiResult, ErrorCode, Event, Request, Response};
use yaac_engine::{EngineEvent, EngineHandle};
use yaac_model::ViewId;

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
    /// A principal with no authority. Exists only to test refusals through the real dispatch path.
    #[cfg(test)]
    Restricted,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("core stopped")]
    Stopped,
}

/// Request side of the core. Cheap to clone. Holds no engine ingress: engine callbacks go through
/// `EngineEventSender`, so a holder of this handle cannot inject them.
#[derive(Clone)]
pub struct CoreHandle {
    lifecycle: Arc<Lifecycle>,
    events: broadcast::Sender<Event>,
}

/// Engine ingress. Cloneable. Callbacks are queued in arrival order with the requests, and dropped
/// once the core has stopped. The engine adapter must buffer its callbacks until it holds this
/// sender: `spawn` returns it only after the startup engine commands are sent.
#[derive(Clone)]
pub struct EngineEventSender {
    inbox: mpsc::UnboundedSender<Inbound>,
}

impl EngineEventSender {
    pub fn send(&self, event: EngineEvent) {
        let _ = self.inbox.send(Inbound::Engine(event));
    }
}

/// Owns the inbox and the threads. When the last `CoreHandle` goes, the last `Arc` drops and the
/// actor is asked to flush and stop. The threads end on their own; they are not joined.
struct Lifecycle {
    inbox: mpsc::UnboundedSender<Inbound>,
    threads: Mutex<Option<Threads>>,
}

struct Threads {
    core: JoinHandle<()>,
    store: JoinHandle<()>,
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        let _ = self.inbox.send(Inbound::Shutdown);
    }
}

impl CoreHandle {
    /// Authorizes and runs one request. Once the core has stopped, every request answers
    /// `Unavailable`. The inbox is unbounded: a client's requests are bounded by its own waiting.
    pub async fn request(&self, principal: Principal, request: Request) -> Response {
        let id = request.id;
        let (reply, answer) = oneshot::channel();
        let message = Message {
            principal,
            request,
            reply,
        };
        if self
            .lifecycle
            .inbox
            .send(Inbound::Request(message))
            .is_err()
        {
            return stopped(id);
        }
        answer.await.unwrap_or_else(|_| stopped(id))
    }

    /// Published after every committed or runtime change, with the new revision.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Flushes the coalesced writes, stops the actor, and joins both threads. Blocks until they have
    /// stopped. Calling it again, or after the core has stopped, does nothing.
    pub fn shutdown(&self) {
        let _ = self.lifecycle.inbox.send(Inbound::Shutdown);
        let threads = self
            .lifecycle
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(Threads { core, store }) = threads {
            let _ = core.join();
            let _ = store.join();
        }
    }
}

fn stopped(id: u64) -> Response {
    Response {
        id,
        result: ApiResult::Err {
            error: ApiError {
                code: ErrorCode::Unavailable,
                message: "the core has stopped".to_owned(),
            },
        },
    }
}

/// Loads the workspace from `store` (creating the first profile, Space and window on an empty
/// store), reconciles it with the engine (every restored tab comes up suspended; the selected tab
/// is loaded) and starts the actor. Returns the request handle and the engine ingress.
///
/// Needs no Tokio runtime: the store and the actor each run on their own thread. The startup engine
/// commands have been sent when this returns.
pub fn spawn(
    store: Box<dyn Store>,
    engine: Arc<dyn EngineHandle>,
) -> Result<(CoreHandle, EngineEventSender), CoreError> {
    spawn_with_clock(store, engine, Arc::new(SystemClock))
}

/// `spawn` with an injected time source. Tests move the clock to exercise time-based rules.
pub(crate) fn spawn_with_clock(
    store: Box<dyn Store>,
    engine: Arc<dyn EngineHandle>,
    clock: Arc<dyn Clock>,
) -> Result<(CoreHandle, EngineEventSender), CoreError> {
    let (jobs, job_queue) = std::sync::mpsc::channel();
    let store_thread = std::thread::Builder::new()
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

    let (inbox, inbox_queue) = mpsc::unbounded_channel();
    let (events, _) = broadcast::channel(EVENT_QUEUE);
    let actor = Actor::new(state, jobs, engine, events.clone(), clock);
    let core_thread = std::thread::Builder::new()
        .name("yaac-core".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("the core runtime can start");
            runtime.block_on(actor.run(inbox_queue));
        })
        .expect("the core thread can start");

    let lifecycle = Arc::new(Lifecycle {
        inbox: inbox.clone(),
        threads: Mutex::new(Some(Threads {
            core: core_thread,
            store: store_thread,
        })),
    });
    Ok((
        CoreHandle { lifecycle, events },
        EngineEventSender { inbox },
    ))
}
