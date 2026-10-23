//! The real core on the real SQLite store: state written in one run is restored in the next.

use std::sync::Arc;
use yaac_api::{
    API_VERSION, ApiResult, Body, Command, ContainerView, NodeView, Placement, Query, Request,
    ResponseBody, Snapshot,
};
use yaac_core::{CoreHandle, Principal, spawn};
use yaac_engine::{EngineCommand, FakeEngine, ViewKind};
use yaac_model::{ContainerKind, ViewId};
use yaac_store::SqliteStore;

struct Run {
    core: CoreHandle,
    sidebar: ViewId,
    startup: Vec<EngineCommand>,
    next_id: u64,
}

impl Run {
    fn start(path: &std::path::Path) -> Run {
        let engine = FakeEngine::new();
        let store = SqliteStore::open(path).expect("open the database");
        let (core, _events) = spawn(Box::new(store), Arc::new(engine.clone())).expect("spawn");
        let startup = engine.take_commands();
        let sidebar = startup
            .iter()
            .find_map(|command| match command {
                EngineCommand::CreateView { view, spec, .. } if spec.kind == ViewKind::Sidebar => {
                    Some(*view)
                }
                _ => None,
            })
            .expect("the core creates the sidebar view at startup");
        Run {
            core,
            sidebar,
            startup,
            next_id: 1,
        }
    }

    async fn call(&mut self, body: Body) -> ResponseBody {
        self.next_id += 1;
        let request = Request {
            v: API_VERSION,
            id: self.next_id,
            body,
        };
        let principal = Principal::Ui { view: self.sidebar };
        match self.core.request(principal, request).await.result {
            ApiResult::Ok { body } => body,
            ApiResult::Err { error } => panic!("request failed: {error}"),
        }
    }

    async fn snapshot(&mut self) -> Snapshot {
        match self.call(Body::Query(Query::Snapshot)).await {
            ResponseBody::Snapshot(snapshot) => snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    fn stop(self) {
        self.core.shutdown();
    }
}

fn container(snapshot: &Snapshot, kind: ContainerKind) -> &ContainerView {
    let space = snapshot
        .spaces
        .iter()
        .find(|space| space.id == snapshot.active_space)
        .expect("active space");
    space
        .containers
        .iter()
        .find(|c| c.kind == kind)
        .expect("container")
}

fn titles(nodes: &[NodeView]) -> Vec<String> {
    nodes.iter().map(|node| node.title.clone()).collect()
}

#[tokio::test]
async fn tabs_survive_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("yaac.db");

    let mut first = Run::start(&path);
    let boot = first.snapshot().await;
    let space = boot.active_space;
    let today = || Placement {
        space,
        container: ContainerKind::Today,
        parent: None,
        before: None,
    };

    let a = match first
        .call(Body::Command(Command::OpenTab {
            placement: today(),
            url: "https://example.com/a".into(),
            activate: true,
        }))
        .await
    {
        ResponseBody::Opened { node, .. } => node,
        other => panic!("{other:?}"),
    };
    first
        .call(Body::Command(Command::OpenTab {
            placement: today(),
            url: "https://example.com/b".into(),
            activate: false,
        }))
        .await;
    let folder = match first
        .call(Body::Command(Command::CreateFolder {
            placement: today(),
            title: "Work".into(),
        }))
        .await
    {
        ResponseBody::Opened { node, .. } => node,
        other => panic!("{other:?}"),
    };
    first
        .call(Body::Command(Command::MoveNode {
            node: a,
            to: Placement {
                space,
                container: ContainerKind::Today,
                parent: Some(folder),
                before: None,
            },
        }))
        .await;
    let before = first.snapshot().await;
    let rev_before = before.rev;
    first.stop();

    let mut second = Run::start(&path);
    let after = second.snapshot().await;

    assert_eq!(after.active_space, before.active_space);
    assert_eq!(
        after.selected, before.selected,
        "the selected tab is restored"
    );
    assert!(
        after.rev >= rev_before,
        "the revision never goes backwards across a restart"
    );
    let today_before = container(&before, ContainerKind::Today);
    let today_after = container(&after, ContainerKind::Today);
    assert_eq!(titles(&today_after.nodes), titles(&today_before.nodes));
    let restored_folder = today_after
        .nodes
        .iter()
        .find(|n| n.id == folder)
        .expect("folder");
    assert_eq!(
        restored_folder.children.len(),
        1,
        "the tab moved into the folder is restored"
    );
    assert!(
        today_after
            .nodes
            .iter()
            .flat_map(|n| std::iter::once(n).chain(n.children.iter()))
            .all(|n| !n.live || Some(n.id) == after.selected),
        "only the selected tab has a browser after a restart"
    );
    // Exactly one content view is created at startup: the selected tab's, loaded with its url.
    let content: Vec<_> = second
        .startup
        .iter()
        .filter_map(|c| match c {
            EngineCommand::CreateView { spec, .. } if spec.kind == ViewKind::Content => {
                Some(spec.url.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        content.len(),
        1,
        "only the selected tab gets a view after a restart"
    );
    let selected = after.selected.expect("a selected tab");
    let selected_url = today_after
        .nodes
        .iter()
        .flat_map(|n| std::iter::once(n).chain(n.children.iter()))
        .find(|n| n.id == selected)
        .and_then(|n| n.url.clone());
    assert_eq!(Some(content[0].clone()), selected_url);
    second.stop();
}
