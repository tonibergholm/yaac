//! Shared helpers for the `CoreHandle` tests.

use crate::testing::FakeStore;
use crate::{CoreHandle, EngineEventSender, Principal, spawn};
use std::sync::Arc;
use yaac_api::{
    API_VERSION, ApiError, ApiResult, Body, Command, NodeView, Placement, Query, Request,
    ResponseBody, Snapshot,
};
use yaac_engine::{EngineCommand, EngineEvent, FakeEngine, ViewKind};
use yaac_model::{ContainerKind, NodeId, SpaceId, ViewId};

pub(super) struct Rig {
    pub(super) handle: CoreHandle,
    /// Engine ingress. Events sent through it are applied in arrival order with requests.
    pub(super) events_in: EngineEventSender,
    pub(super) engine: FakeEngine,
    pub(super) store: FakeStore,
    pub(super) sidebar: ViewId,
    /// The engine commands issued while starting.
    pub(super) startup: Vec<EngineCommand>,
}

impl Rig {
    pub(super) fn start(store: FakeStore) -> Self {
        let engine = FakeEngine::new();
        let (handle, events_in) =
            spawn(Box::new(store.clone()), Arc::new(engine.clone())).expect("the core starts");
        let startup = engine.take_commands();
        let sidebar = startup
            .iter()
            .find_map(|command| match command {
                EngineCommand::CreateView { view, spec, .. } if spec.kind == ViewKind::Sidebar => {
                    Some(*view)
                }
                _ => None,
            })
            .expect("startup creates the sidebar view");
        Self {
            handle,
            events_in,
            engine,
            store,
            sidebar,
            startup,
        }
    }

    pub(super) fn empty() -> Self {
        Self::start(FakeStore::new())
    }

    pub(super) fn ui(&self) -> Principal {
        Principal::Ui { view: self.sidebar }
    }

    pub(super) async fn send(&self, command: Command) -> Result<ResponseBody, ApiError> {
        self.send_as(self.ui(), command).await
    }

    pub(super) async fn send_as(
        &self,
        principal: Principal,
        command: Command,
    ) -> Result<ResponseBody, ApiError> {
        let request = Request {
            v: API_VERSION,
            id: 1,
            body: Body::Command(command),
        };
        match self.handle.request(principal, request).await.result {
            ApiResult::Ok { body } => Ok(body),
            ApiResult::Err { error } => Err(error),
        }
    }

    pub(super) async fn ok(&self, command: Command) -> ResponseBody {
        self.send(command).await.expect("the command succeeds")
    }

    pub(super) async fn snapshot(&self) -> Snapshot {
        let request = Request {
            v: API_VERSION,
            id: 2,
            body: Body::Query(Query::Snapshot),
        };
        match self.handle.request(self.ui(), request).await.result {
            ApiResult::Ok {
                body: ResponseBody::Snapshot(snapshot),
            } => snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    /// Sends an engine event and returns the snapshot after it. The inbox is FIFO, so the snapshot
    /// is answered only after the event has been handled.
    pub(super) async fn emit(&self, event: EngineEvent) -> Snapshot {
        self.events_in.send(event);
        self.snapshot().await
    }

    /// Store batches recorded so far.
    pub(super) fn batches(&self) -> Vec<Vec<crate::StoreOp>> {
        self.store.applied()
    }
}

/// The content view most recently created in `commands`.
pub(super) fn last_content_view(commands: &[EngineCommand]) -> Option<ViewId> {
    commands.iter().rev().find_map(|command| match command {
        EngineCommand::CreateView { view, spec, .. } if spec.kind == ViewKind::Content => {
            Some(*view)
        }
        _ => None,
    })
}

pub(super) fn tab_in_today(space: SpaceId) -> Placement {
    Placement {
        space,
        container: ContainerKind::Today,
        parent: None,
        before: None,
    }
}

/// Every node of every container, nested children included.
pub(super) fn all_nodes(snapshot: &Snapshot) -> Vec<NodeView> {
    fn walk(nodes: &[NodeView], out: &mut Vec<NodeView>) {
        for node in nodes {
            out.push(node.clone());
            walk(&node.children, out);
        }
    }
    let mut out = Vec::new();
    for space in &snapshot.spaces {
        for container in &space.containers {
            walk(&container.nodes, &mut out);
        }
    }
    out
}

pub(super) fn node(snapshot: &Snapshot, id: NodeId) -> NodeView {
    all_nodes(snapshot)
        .into_iter()
        .find(|candidate| candidate.id == id)
        .unwrap_or_else(|| panic!("node {id} is in the snapshot"))
}

pub(super) fn opened(body: ResponseBody) -> NodeId {
    match body {
        ResponseBody::Opened { node, .. } => node,
        other => panic!("expected Opened, got {other:?}"),
    }
}

pub(super) fn rev_of(body: &ResponseBody) -> u64 {
    match body {
        ResponseBody::Ack { rev }
        | ResponseBody::Opened { rev, .. }
        | ResponseBody::SpaceCreated { rev, .. } => *rev,
        ResponseBody::Snapshot(snapshot) => snapshot.rev,
    }
}

pub(super) fn space_of(snapshot: &Snapshot) -> SpaceId {
    snapshot.active_space
}
