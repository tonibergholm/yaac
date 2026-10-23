//! The actor task and the store thread. The actor is the only writer: it takes requests and engine
//! events from one FIFO inbox, so requests and callbacks are handled in the order they were sent.
//! It awaits store commits and never awaits the engine.
//!
//! Title and URL changes are applied in memory at once but persisted on a coalescing flush: at most
//! one batch per `FLUSH_INTERVAL`, and always before the next command's commit and at shutdown.

use crate::auth;
use crate::commands;
use crate::error::{conflict, internal};
use crate::events::{self, Step};
use crate::state::{Plan, State, snapshot};
use crate::{Principal, Store, StoreError, StoreOp};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use yaac_api::{ApiError, ApiResult, Body, Event, Request, Response, ResponseBody};
use yaac_engine::{EngineEvent, EngineHandle};
use yaac_model::{Node, NodeId, Workspace};

pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) type Reply<T> = Box<dyn FnOnce(Result<T, StoreError>) + Send>;

pub(crate) enum StoreJob {
    Load(Reply<Workspace>),
    Apply(Vec<StoreOp>, Reply<()>),
}

/// Runs store calls in order until every sender is dropped. Each `apply` is atomic and synchronous,
/// so the actor's last commit is durable once it returns.
pub(crate) fn run_store(mut store: Box<dyn Store>, jobs: std_mpsc::Receiver<StoreJob>) {
    for job in jobs {
        match job {
            StoreJob::Load(reply) => reply(store.load()),
            StoreJob::Apply(ops, reply) => reply(store.apply(ops)),
        }
    }
}

fn stopped() -> StoreError {
    StoreError::Backend("the store thread has stopped".to_owned())
}

/// Blocking calls, used only while the core starts (before any request can arrive).
pub(crate) fn load_blocking(jobs: &std_mpsc::Sender<StoreJob>) -> Result<Workspace, StoreError> {
    let (reply, answer) = std_mpsc::sync_channel(1);
    jobs.send(StoreJob::Load(Box::new(move |result| {
        let _ = reply.send(result);
    })))
    .map_err(|_| stopped())?;
    answer.recv().unwrap_or_else(|_| Err(stopped()))
}

pub(crate) fn commit_blocking(
    jobs: &std_mpsc::Sender<StoreJob>,
    ops: Vec<StoreOp>,
) -> Result<(), StoreError> {
    let (reply, answer) = std_mpsc::sync_channel(1);
    jobs.send(StoreJob::Apply(
        ops,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ))
    .map_err(|_| stopped())?;
    answer.recv().unwrap_or_else(|_| Err(stopped()))
}

async fn commit_async(
    jobs: &std_mpsc::Sender<StoreJob>,
    ops: Vec<StoreOp>,
) -> Result<(), StoreError> {
    let (reply, answer) = oneshot::channel();
    jobs.send(StoreJob::Apply(
        ops,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ))
    .map_err(|_| stopped())?;
    answer.await.unwrap_or_else(|_| Err(stopped()))
}

pub(crate) struct Message {
    pub(crate) principal: Principal,
    pub(crate) request: Request,
    pub(crate) reply: oneshot::Sender<Response>,
}

/// Everything the actor receives, in arrival order.
pub(crate) enum Inbound {
    Request(Message),
    Engine(EngineEvent),
    /// Flush and stop. Sent by `CoreHandle::shutdown`, and when the last handle is dropped.
    Shutdown,
}

pub(crate) struct Actor {
    state: State,
    store: std_mpsc::Sender<StoreJob>,
    engine: Arc<dyn EngineHandle>,
    events: broadcast::Sender<Event>,
    /// Coalesced node writes not yet persisted. The last value per node wins.
    pending: BTreeMap<NodeId, Node>,
}

impl Actor {
    pub(crate) fn new(
        state: State,
        store: std_mpsc::Sender<StoreJob>,
        engine: Arc<dyn EngineHandle>,
        events: broadcast::Sender<Event>,
    ) -> Self {
        Self {
            state,
            store,
            engine,
            events,
            pending: BTreeMap::new(),
        }
    }

    /// Handles inbox items one at a time. The flush tick runs between them, so a stream of callbacks
    /// cannot hold back a pending flush, and a flush cannot hold back a request.
    pub(crate) async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Inbound>) {
        let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                message = inbox.recv() => match message {
                    Some(Inbound::Request(Message { principal, request, reply })) => {
                        let response = self.dispatch(&principal, request).await;
                        let _ = reply.send(response);
                    }
                    Some(Inbound::Engine(event)) => {
                        if self.on_engine_event(event).await {
                            break;
                        }
                    }
                    Some(Inbound::Shutdown) | None => break,
                },
                _ = ticker.tick() => {
                    if let Err(error) = self.flush().await {
                        tracing::warn!("coalesced writes not flushed: {error}");
                    }
                }
            }
        }
        if let Err(error) = self.flush().await {
            tracing::warn!("coalesced writes not flushed at shutdown: {error}");
        }
    }

    async fn dispatch(&mut self, principal: &Principal, request: Request) -> Response {
        let id = request.id;
        let result = match self.handle(principal, request).await {
            Ok(body) => ApiResult::Ok { body },
            Err(error) => ApiResult::Err { error },
        };
        Response { id, result }
    }

    async fn handle(
        &mut self,
        principal: &Principal,
        request: Request,
    ) -> Result<ResponseBody, ApiError> {
        auth::admit(&self.state, principal, &request)?;
        match request.body {
            Body::Query(_) => snapshot(&self.state).map(ResponseBody::Snapshot),
            Body::Command(command) => {
                let (plan, answer) = commands::plan(&self.state, command)?;
                // A closing window takes no command that creates, selects or navigates a view.
                if self.state.closing && !plan.effects.is_empty() {
                    return Err(conflict("window is closing"));
                }
                self.commit(plan).await?;
                Ok(answer.into_body(self.state.ws.rev))
            }
        }
    }

    /// Returns true when the actor should stop.
    async fn on_engine_event(&mut self, event: EngineEvent) -> bool {
        let closing =
            matches!(&event, EngineEvent::WindowClosing { window } if *window == self.state.window);
        let stop = match events::on_engine_event(&self.state, event) {
            Step::Ignore => false,
            Step::Stop => true,
            Step::Plan(plan) => {
                if let Err(error) = self.commit(*plan).await {
                    tracing::warn!("engine event not applied: {}", error.message);
                }
                false
            }
        };
        if closing && let Err(error) = self.flush().await {
            tracing::warn!("coalesced writes not flushed on close: {error}");
        }
        stop
    }

    /// Commit before effect. A plan that is not coalesced first persists the pending writes with its
    /// own ops in one batch. Then the state is swapped in, the engine commands go out, and the change
    /// is published. A failed store commit changes nothing and the pending writes stay queued.
    async fn commit(&mut self, plan: Plan) -> Result<(), ApiError> {
        let Plan {
            state,
            ops,
            mut effects,
            topic,
            coalesce,
        } = plan;
        if state.closing {
            // Nothing reaches the engine for a closing window.
            effects.clear();
        }
        if coalesce {
            for op in ops {
                if let StoreOp::UpsertNode(node) = op {
                    self.pending.insert(node.id, node);
                }
            }
        } else if !ops.is_empty() || !self.pending.is_empty() {
            let drained = std::mem::take(&mut self.pending);
            let mut batch = pending_ops(&drained);
            batch.extend(ops);
            if !batch.iter().any(|op| matches!(op, StoreOp::SetRev(_))) {
                batch.push(StoreOp::SetRev(state.ws.rev));
            }
            if let Err(error) = commit_async(&self.store, batch).await {
                self.pending = drained;
                return Err(internal(error.to_string()));
            }
        }
        self.state = state;
        for command in effects {
            self.engine.send(command);
        }
        if let Some(topic) = topic {
            let _ = self.events.send(Event {
                topic,
                rev: self.state.ws.rev,
            });
        }
        Ok(())
    }

    /// Persists the coalesced writes as one batch. A failed batch is kept for the next flush.
    async fn flush(&mut self) -> Result<(), StoreError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let drained = std::mem::take(&mut self.pending);
        let mut batch = pending_ops(&drained);
        batch.push(StoreOp::SetRev(self.state.ws.rev));
        match commit_async(&self.store, batch).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.pending = drained;
                Err(error)
            }
        }
    }
}

fn pending_ops(pending: &BTreeMap<NodeId, Node>) -> Vec<StoreOp> {
    pending.values().cloned().map(StoreOp::UpsertNode).collect()
}
