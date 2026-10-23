//! The actor task (one writer, one total order of commands) and the store thread (synchronous
//! persistence). The actor awaits store commits. It never awaits the engine.

use crate::auth;
use crate::commands;
use crate::error::internal;
use crate::events::{self, Step};
use crate::state::{Plan, State, snapshot};
use crate::{Principal, Store, StoreError, StoreOp};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use tokio::sync::{broadcast, mpsc, oneshot};
use yaac_api::{ApiError, ApiResult, Body, Event, Request, Response, ResponseBody};
use yaac_engine::{EngineEvent, EngineHandle};
use yaac_model::Workspace;

pub(crate) type Reply<T> = Box<dyn FnOnce(Result<T, StoreError>) + Send>;

pub(crate) enum StoreJob {
    Load(Reply<Workspace>),
    Apply(Vec<StoreOp>, Reply<()>),
}

/// Runs store calls in order until every sender is dropped. Each `apply` is atomic and synchronous,
/// so dropping the senders is the flush.
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

pub(crate) struct Actor {
    state: State,
    store: std_mpsc::Sender<StoreJob>,
    engine: Arc<dyn EngineHandle>,
    events: broadcast::Sender<Event>,
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
        }
    }

    /// Engine events are polled first, so a callback sent before a request is applied before it.
    /// The loop ends when every request sender is gone, or when the core's window closes.
    pub(crate) async fn run(
        mut self,
        mut requests: mpsc::Receiver<Message>,
        mut engine_events: mpsc::UnboundedReceiver<EngineEvent>,
    ) {
        loop {
            tokio::select! {
                biased;
                Some(event) = engine_events.recv() => {
                    if self.on_engine_event(event).await {
                        break;
                    }
                }
                message = requests.recv() => {
                    let Some(Message { principal, request, reply }) = message else {
                        break;
                    };
                    let response = self.dispatch(&principal, request).await;
                    let _ = reply.send(response);
                }
            }
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
        let kind = auth::admit(&self.state, principal, &request)?;
        match request.body {
            Body::Query(_) => snapshot(&self.state).map(ResponseBody::Snapshot),
            Body::Command(command) => {
                let (plan, answer) = commands::plan(&self.state, kind, command)?;
                self.commit(plan).await?;
                Ok(answer.into_body(self.state.ws.rev))
            }
        }
    }

    /// Returns true when the actor should stop.
    async fn on_engine_event(&mut self, event: EngineEvent) -> bool {
        match events::on_engine_event(&self.state, event) {
            Step::Ignore => false,
            Step::Stop => true,
            Step::Plan(plan) => {
                if let Err(error) = self.commit(*plan).await {
                    tracing::warn!("engine event not applied: {}", error.message);
                }
                false
            }
        }
    }

    /// Commit before effect: the store commits the ops, then the state is swapped in, then the
    /// engine commands go out, then the change is published.
    async fn commit(&mut self, plan: Plan) -> Result<(), ApiError> {
        let Plan {
            state,
            ops,
            effects,
            topic,
        } = plan;
        if !ops.is_empty() {
            commit_async(&self.store, ops)
                .await
                .map_err(|error| internal(error.to_string()))?;
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
}
