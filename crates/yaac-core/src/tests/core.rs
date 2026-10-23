//! Behavior through `CoreHandle`, against `FakeEngine` and `FakeStore`. Every test here needs
//! `Position` and `TreeState` from `yaac-model`, so each is ignored until `m1/model` lands.

use super::harness::*;
use crate::testing::FakeStore;
use crate::{Principal, StoreOp};
use std::time::Duration;
use yaac_api::{
    API_VERSION, ApiResult, Body, Command, ErrorCode, Placement, Query, Request, ResponseBody,
    Snapshot, Topic,
};
use yaac_engine::{EngineCommand, EngineEvent, ViewKind};
use yaac_model::{
    Container, ContainerId, ContainerKind, Node, NodeId, NodeKind, NodeState, Position, Profile,
    ProfileId, ProfileKind, SessionState, Space, SpaceId, ViewId, WindowId, Workspace,
};

fn rev(body: &ResponseBody) -> u64 {
    match body {
        ResponseBody::Ack { rev }
        | ResponseBody::Opened { rev, .. }
        | ResponseBody::SpaceCreated { rev, .. } => *rev,
        ResponseBody::Snapshot(snapshot) => snapshot.rev,
    }
}

fn space_id(snapshot: &Snapshot) -> SpaceId {
    snapshot.active_space
}

fn stored_profile(ws: &mut Workspace, kind: ProfileKind, name: &str) -> ProfileId {
    let id = ProfileId::new();
    ws.profiles.insert(
        id,
        Profile {
            id,
            kind,
            name: name.to_owned(),
            cache_dir: format!("profiles/{id}"),
            created_at: 0,
        },
    );
    id
}

fn stored_container(
    ws: &mut Workspace,
    profile: ProfileId,
    space: Option<SpaceId>,
    kind: ContainerKind,
) -> ContainerId {
    let id = ContainerId::new();
    ws.containers.insert(
        id,
        Container {
            id,
            profile_id: profile,
            space_id: space,
            kind,
        },
    );
    id
}

fn stored_space(ws: &mut Workspace, profile: ProfileId, name: &str, position: Position) -> SpaceId {
    let id = SpaceId::new();
    ws.spaces.insert(
        id,
        Space {
            id,
            profile_id: profile,
            name: name.to_owned(),
            theme: None,
            position,
            agent_enabled: false,
        },
    );
    for kind in [
        ContainerKind::Pinned,
        ContainerKind::Today,
        ContainerKind::Archive,
    ] {
        stored_container(ws, profile, Some(id), kind);
    }
    id
}

fn container_of(ws: &Workspace, space: SpaceId, kind: ContainerKind) -> ContainerId {
    ws.containers
        .values()
        .find(|container| container.space_id == Some(space) && container.kind == kind)
        .expect("the space has the container")
        .id
}

fn stored_tab(ws: &mut Workspace, container: ContainerId, position: Position, url: &str) -> NodeId {
    let node = Node {
        id: NodeId::new(),
        container_id: container,
        parent_id: None,
        position,
        kind: NodeKind::Tab,
        title: "stored".to_owned(),
        url: Some(url.to_owned()),
        state: NodeState::Suspended,
        is_unread: false,
        created_at: 0,
        last_active_at: 0,
        updated_at: 0,
        origin_device: None,
        rev: 0,
    };
    let id = node.id;
    ws.tree.insert(node).expect("the tree accepts the node");
    id
}

/// One Default profile with favorites and one Space holding three tabs. The session selects the
/// middle tab.
fn stored_workspace() -> (Workspace, NodeId, NodeId, NodeId, SpaceId) {
    let mut ws = Workspace::default();
    let profile = stored_profile(&mut ws, ProfileKind::Default, "Default");
    stored_container(&mut ws, profile, None, ContainerKind::Favorites);
    let space = stored_space(&mut ws, profile, "Work", Position::first());
    let today = container_of(&ws, space, ContainerKind::Today);
    let first = Position::first();
    let second = Position::between(Some(&first), None).expect("a key fits");
    let third = Position::between(Some(&second), None).expect("a key fits");
    let one = stored_tab(&mut ws, today, first, "https://one.example");
    let two = stored_tab(&mut ws, today, second, "https://two.example");
    let three = stored_tab(&mut ws, today, third, "https://three.example");
    let window = WindowId::new();
    ws.sessions.insert(
        window,
        SessionState {
            window_id: window,
            space_id: space,
            selected_node_id: Some(two),
            sidebar_width: 280,
        },
    );
    ws.rev = 3;
    (ws, one, two, three, space)
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn bootstrap_creates_one_profile_space_tab_and_window_and_persists_them() {
    let rig = Rig::empty();
    let ops: Vec<StoreOp> = rig.store.applied().into_iter().flatten().collect();
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, StoreOp::UpsertProfile(_)))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, StoreOp::UpsertSpace(_)))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, StoreOp::UpsertContainer(_)))
            .count(),
        4
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, StoreOp::UpsertNode(_)))
            .count(),
        1
    );
    assert!(ops.iter().any(|op| matches!(op, StoreOp::UpsertSession(_))));

    assert!(matches!(rig.startup[0], EngineCommand::CreateWindow { .. }));
    assert!(matches!(
        &rig.startup[1],
        EngineCommand::CreateView { spec, .. }
            if spec.kind == ViewKind::Sidebar && spec.url == "app://ui/sidebar.html"
    ));
    let content = last_content_view(&rig.startup).expect("the selected tab has a view");
    assert!(matches!(
        rig.startup.last(),
        Some(EngineCommand::Show { view }) if *view == content
    ));

    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.rev, 1);
    let tabs = all_nodes(&snapshot);
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].live);
    assert_eq!(tabs[0].url.as_deref(), Some("about:blank"));
    assert_eq!(snapshot.selected, Some(tabs[0].id));
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn restore_suspends_every_tab_and_loads_only_the_selected_one() {
    let (ws, one, two, three, space) = stored_workspace();
    let rig = Rig::start(FakeStore::with_workspace(ws));

    assert!(rig.store.applied().is_empty(), "nothing needed repair");
    let content_creates = rig
        .startup
        .iter()
        .filter(|command| matches!(command, EngineCommand::CreateView { spec, .. } if spec.kind == ViewKind::Content))
        .count();
    assert_eq!(content_creates, 1);
    assert!(rig.startup.iter().any(|command| matches!(
        command,
        EngineCommand::CreateView { spec, .. } if spec.url == "https://two.example"
    )));

    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.active_space, space);
    assert_eq!(snapshot.selected, Some(two));
    assert!(!node(&snapshot, one).live);
    assert!(node(&snapshot, two).live);
    assert!(!node(&snapshot, three).live);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn open_select_and_close_issue_the_right_engine_sequence() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let first = {
        let snapshot = rig.snapshot().await;
        all_nodes(&snapshot)[0].id
    };
    let space = space_id(&rig.snapshot().await);

    let second = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://b.example".to_owned(),
            activate: true,
        })
        .await,
    );
    let commands = rig.engine.take_commands();
    let second_view = last_content_view(&commands).expect("the new tab gets a view");
    assert!(matches!(commands.as_slice(), [
        EngineCommand::CreateView { spec, .. },
        EngineCommand::Show { view },
    ] if spec.url == "https://b.example" && *view == second_view));

    rig.ok(Command::SelectTab { node: first }).await;
    let commands = rig.engine.take_commands();
    assert!(matches!(commands.as_slice(), [EngineCommand::Show { .. }]));

    rig.ok(Command::CloseTab { node: first }).await;
    let commands = rig.engine.take_commands();
    assert!(matches!(
        commands.first(),
        Some(EngineCommand::Close { .. })
    ));
    assert!(matches!(commands.last(), Some(EngineCommand::Show { view }) if *view == second_view));

    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.selected, Some(second));
    assert_eq!(all_nodes(&snapshot).len(), 1);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn closing_the_last_tab_opens_a_new_blank_tab() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let snapshot = rig.snapshot().await;
    let only = all_nodes(&snapshot)[0].id;

    rig.ok(Command::CloseTab { node: only }).await;
    let commands = rig.engine.take_commands();
    assert!(matches!(
        commands.first(),
        Some(EngineCommand::Close { .. })
    ));
    assert!(matches!(
        commands.iter().find(|command| matches!(command, EngineCommand::CreateView { .. })),
        Some(EngineCommand::CreateView { spec, .. }) if spec.url == "about:blank"
    ));

    let snapshot = rig.snapshot().await;
    let tabs = all_nodes(&snapshot);
    assert_eq!(tabs.len(), 1);
    assert_ne!(tabs[0].id, only);
    assert_eq!(snapshot.selected, Some(tabs[0].id));
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn moving_a_folder_across_containers_rewrites_its_children() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let space = space_id(&rig.snapshot().await);
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(space),
        title: "Work".to_owned(),
    })
    .await;
    let folder = folder_titled(&rig, "Work").await;
    let child = opened(
        rig.ok(Command::OpenTab {
            placement: Placement {
                space,
                container: ContainerKind::Today,
                parent: Some(folder),
                before: None,
            },
            url: "https://child.example".to_owned(),
            activate: false,
        })
        .await,
    );

    rig.ok(Command::MoveNode {
        node: folder,
        to: Placement {
            space,
            container: ContainerKind::Pinned,
            parent: None,
            before: None,
        },
    })
    .await;

    let last = rig.store.applied().pop().expect("the move persisted");
    assert!(
        last.iter()
            .any(|op| matches!(op, StoreOp::UpsertNode(n) if n.id == child))
    );
    let snapshot = rig.snapshot().await;
    let pinned = snapshot.spaces[0]
        .containers
        .iter()
        .find(|container| container.kind == ContainerKind::Pinned)
        .expect("the pinned container");
    assert_eq!(pinned.nodes.len(), 1);
    assert_eq!(pinned.nodes[0].children[0].id, child);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn moving_a_tab_to_another_profile_is_a_conflict() {
    let mut ws = Workspace::default();
    let default = stored_profile(&mut ws, ProfileKind::Default, "Default");
    stored_container(&mut ws, default, None, ContainerKind::Favorites);
    let first_space = stored_space(&mut ws, default, "Home", Position::first());
    let today = container_of(&ws, first_space, ContainerKind::Today);
    let tab = stored_tab(&mut ws, today, Position::first(), "https://a.example");
    let other = stored_profile(&mut ws, ProfileKind::Standard, "Work");
    let other_space = stored_space(&mut ws, other, "Work", Position::first());
    let window = WindowId::new();
    ws.sessions.insert(
        window,
        SessionState {
            window_id: window,
            space_id: first_space,
            selected_node_id: Some(tab),
            sidebar_width: 280,
        },
    );
    let rig = Rig::start(FakeStore::with_workspace(ws));

    let error = rig
        .send(Command::MoveNode {
            node: tab,
            to: tab_in_today(other_space),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn moving_a_folder_into_its_own_subtree_is_a_conflict() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let space = space_id(&rig.snapshot().await);
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(space),
        title: "Outer".to_owned(),
    })
    .await;
    let folder = folder_titled(&rig, "Outer").await;
    rig.ok(Command::CreateFolder {
        placement: Placement {
            space,
            container: ContainerKind::Today,
            parent: Some(folder),
            before: None,
        },
        title: "Inner".to_owned(),
    })
    .await;
    let inner = folder_titled(&rig, "Inner").await;

    let error = rig
        .send(Command::MoveNode {
            node: folder,
            to: Placement {
                space,
                container: ContainerKind::Today,
                parent: Some(inner),
                before: None,
            },
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn a_stale_navigation_callback_is_dropped() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the first tab has a view");
    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 2,
        url: "https://x.example".to_owned(),
    })
    .await;
    let snapshot = rig
        .emit(EngineEvent::TitleChanged {
            view,
            nav_gen: 1,
            title: "Old".to_owned(),
        })
        .await;

    let tab = all_nodes(&snapshot)[0].clone();
    assert_eq!(tab.url.as_deref(), Some("https://x.example"));
    assert_eq!(tab.title, "", "the stale title never lands");
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn title_and_url_move_together_across_navigations() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the first tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;

    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 1,
        url: "https://a.example".to_owned(),
    })
    .await;
    rig.emit(EngineEvent::TitleChanged {
        view,
        nav_gen: 1,
        title: "A".to_owned(),
    })
    .await;
    let snapshot = rig
        .emit(EngineEvent::Navigated {
            view,
            nav_gen: 2,
            url: "https://b.example".to_owned(),
        })
        .await;
    let shown = node(&snapshot, tab);
    assert_eq!(shown.url.as_deref(), Some("https://b.example"));
    assert_eq!(shown.title, "", "the old title is cleared with the old URL");

    // A title that arrives before its own navigation waits for it.
    let early = rig
        .emit(EngineEvent::TitleChanged {
            view,
            nav_gen: 3,
            title: "C".to_owned(),
        })
        .await;
    assert_eq!(node(&early, tab).title, "");
    let snapshot = rig
        .emit(EngineEvent::Navigated {
            view,
            nav_gen: 3,
            url: "https://c.example".to_owned(),
        })
        .await;
    let shown = node(&snapshot, tab);
    assert_eq!(shown.url.as_deref(), Some("https://c.example"));
    assert_eq!(shown.title, "C");
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn a_store_failure_leaves_the_state_and_the_engine_untouched() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let before = rig.snapshot().await;

    rig.store.fail_next_apply();
    let error = rig
        .send(Command::OpenTab {
            placement: tab_in_today(space_id(&before)),
            url: "https://x.example".to_owned(),
            activate: true,
        })
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal);
    assert!(
        rig.engine.take_commands().is_empty(),
        "no engine command was sent"
    );
    let after = rig.snapshot().await;
    assert_eq!(after.rev, before.rev);
    assert_eq!(after.selected, before.selected);
    assert_eq!(all_nodes(&after).len(), all_nodes(&before).len());
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn a_ui_principal_for_an_unregistered_view_is_refused() {
    let rig = Rig::empty();
    let request = Request {
        v: API_VERSION,
        id: 9,
        body: Body::Query(Query::Snapshot),
    };
    let response = rig
        .handle
        .request(
            Principal::Ui {
                view: ViewId(u64::MAX),
            },
            request,
        )
        .await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Forbidden
    ));
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn a_version_mismatch_is_refused() {
    let rig = Rig::empty();
    let request = Request {
        v: API_VERSION + 1,
        id: 9,
        body: Body::Query(Query::Snapshot),
    };
    let response = rig.handle.request(Principal::System, request).await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::UnsupportedVersion
    ));
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn published_events_carry_increasing_revisions() {
    let rig = Rig::empty();
    let space = space_id(&rig.snapshot().await);
    let mut events = rig.handle.subscribe();

    let mut last = 0;
    for title in ["one", "two", "three"] {
        let body = rig
            .ok(Command::CreateFolder {
                placement: tab_in_today(space),
                title: title.to_owned(),
            })
            .await;
        let event = events.try_recv().expect("the commit published an event");
        assert_eq!(event.topic, Topic::Tree);
        assert_eq!(event.rev, rev(&body));
        assert!(event.rev > last);
        last = event.rev;
    }
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn the_snapshot_nests_children_and_flags_only_live_nodes() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let space = space_id(&rig.snapshot().await);
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(space),
        title: "Folder".to_owned(),
    })
    .await;
    let folder = folder_titled(&rig, "Folder").await;
    let child = opened(
        rig.ok(Command::OpenTab {
            placement: Placement {
                space,
                container: ContainerKind::Today,
                parent: Some(folder),
                before: None,
            },
            url: "https://child.example".to_owned(),
            activate: false,
        })
        .await,
    );

    let snapshot = rig.snapshot().await;
    let top = &snapshot.spaces[0]
        .containers
        .iter()
        .find(|container| container.kind == ContainerKind::Today)
        .expect("the today container")
        .nodes;
    let folder_view = top
        .iter()
        .find(|view| view.id == folder)
        .expect("folder at the top");
    assert_eq!(folder_view.children[0].id, child);
    assert!(!folder_view.live, "a folder never has a view");
    assert!(
        !folder_view.children[0].live,
        "a suspended child is not live"
    );
    let selected = snapshot.selected.expect("a tab is selected");
    assert!(node(&snapshot, selected).live);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn navigating_a_suspended_tab_creates_a_view_without_showing_it() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let space = space_id(&rig.snapshot().await);
    let background = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://background.example".to_owned(),
            activate: false,
        })
        .await,
    );
    rig.engine.take_commands();

    rig.ok(Command::Navigate {
        node: background,
        url: "https://next.example".to_owned(),
    })
    .await;

    let commands = rig.engine.take_commands();
    assert!(
        matches!(commands.as_slice(), [EngineCommand::CreateView { spec, .. }] if spec.url == "https://next.example")
    );
    assert!(node(&rig.snapshot().await, background).live);
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn a_popup_opens_a_tab_only_with_a_user_gesture() {
    let rig = Rig::empty();
    let opener = last_content_view(&rig.startup).expect("the first tab has a view");

    let snapshot = rig
        .emit(EngineEvent::PopupRequested {
            opener,
            url: "https://dropped.example".to_owned(),
            user_gesture: false,
        })
        .await;
    assert_eq!(all_nodes(&snapshot).len(), 1);

    let snapshot = rig
        .emit(EngineEvent::PopupRequested {
            opener,
            url: "https://popup.example".to_owned(),
            user_gesture: true,
        })
        .await;
    let popup = all_nodes(&snapshot)
        .into_iter()
        .find(|tab| tab.url.as_deref() == Some("https://popup.example"))
        .expect("the popup became a tab");
    assert_eq!(snapshot.selected, Some(popup.id));
}

#[tokio::test]
#[ignore = "needs m1/model"]
async fn closing_the_window_stops_the_actor() {
    let rig = Rig::empty();
    let window = rig.snapshot().await.window;
    rig.handle
        .engine_events()
        .send(EngineEvent::WindowClosed { window })
        .expect("the actor is running");

    let mut stopped = false;
    for _ in 0..200 {
        let response = rig
            .handle
            .request(
                rig.ui(),
                Request {
                    v: API_VERSION,
                    id: 3,
                    body: Body::Query(Query::Snapshot),
                },
            )
            .await;
        if matches!(response.result, ApiResult::Err { error } if error.code == ErrorCode::Internal)
        {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(stopped, "the actor stops after the window closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs m1/model"]
async fn concurrent_requests_are_totally_ordered() {
    let rig = Rig::empty();
    let space = space_id(&rig.snapshot().await);
    let mut tasks = Vec::new();
    for index in 0..40 {
        let handle = rig.handle.clone();
        let principal = rig.ui();
        tasks.push(tokio::spawn(async move {
            let request = Request {
                v: API_VERSION,
                id: index,
                body: Body::Command(Command::CreateFolder {
                    placement: tab_in_today(space),
                    title: format!("folder {index}"),
                }),
            };
            match handle.request(principal, request).await.result {
                ApiResult::Ok { body } => rev(&body),
                ApiResult::Err { error } => panic!("{error}"),
            }
        }));
    }
    let mut revisions = Vec::new();
    for task in tasks {
        revisions.push(task.await.expect("the task completes"));
    }
    revisions.sort_unstable();
    assert_eq!(
        revisions,
        (2..=41).collect::<Vec<u64>>(),
        "each commit gets its own revision"
    );
    assert_eq!(rig.snapshot().await.rev, 41);
}

/// Creating a folder answers with an acknowledgement, so the test finds the folder by its title.
async fn folder_titled(rig: &Rig, title: &str) -> NodeId {
    all_nodes(&rig.snapshot().await)
        .into_iter()
        .find(|candidate| candidate.kind == NodeKind::Folder && candidate.title == title)
        .unwrap_or_else(|| panic!("a folder titled {title}"))
        .id
}
