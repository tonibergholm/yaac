//! Shared helpers for the `CoreHandle` tests.

use crate::testing::FakeStore;
use crate::{CoreHandle, Principal, spawn};
use std::sync::Arc;
use yaac_api::{
    API_VERSION, ApiError, ApiResult, Body, Command, NodeView, Placement, Query, Request,
    ResponseBody, Snapshot,
};
use yaac_engine::{EngineCommand, EngineEvent, FakeEngine, ViewKind};
use yaac_model::{ContainerKind, SpaceId, ViewId};

pub(super) struct Rig {
    pub(super) handle: CoreHandle,
    pub(super) engine: FakeEngine,
    pub(super) store: FakeStore,
    sidebar: ViewId,
    /// The engine commands issued while starting.
    pub(super) startup: Vec<EngineCommand>,
}

impl Rig {
    pub(super) fn start(store: FakeStore) -> Self {
        let engine = FakeEngine::new();
        let handle =
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
        let request = Request {
            v: API_VERSION,
            id: 1,
            body: Body::Command(command),
        };
        match self.handle.request(self.ui(), request).await.result {
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

    /// Sends an engine event. A snapshot taken afterwards is a barrier: the actor polls engine
    /// events first, so the event is applied before the snapshot is answered.
    pub(super) async fn emit(&self, event: EngineEvent) -> Snapshot {
        self.handle
            .engine_events()
            .send(event)
            .expect("the actor is running");
        self.snapshot().await
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

pub(super) fn node(snapshot: &Snapshot, id: yaac_model::NodeId) -> NodeView {
    all_nodes(snapshot)
        .into_iter()
        .find(|candidate| candidate.id == id)
        .unwrap_or_else(|| panic!("node {id} is in the snapshot"))
}

pub(super) fn opened(body: ResponseBody) -> yaac_model::NodeId {
    match body {
        ResponseBody::Opened { node, .. } => node,
        other => panic!("expected Opened, got {other:?}"),
    }
}
