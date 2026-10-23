//! Behavior through `CoreHandle`, against `FakeEngine` and `FakeStore`.

use super::harness::*;
use crate::events::CLOSE_STALL;
use crate::testing::FakeStore;
use crate::{Principal, StoreOp};
use std::time::{Duration, Instant};
use yaac_api::{
    API_VERSION, ApiResult, Body, Command, ErrorCode, Placement, Query, Request, ResponseBody,
    Topic,
};
use yaac_engine::{EngineCommand, EngineEvent, ViewKind};
use yaac_model::{
    Container, ContainerId, ContainerKind, Node, NodeId, NodeKind, NodeState, Position, Profile,
    ProfileId, ProfileKind, SessionState, Space, SpaceId, ViewId, WindowId, Workspace,
};

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
fn stored_workspace(middle_url: &str) -> (Workspace, NodeId, NodeId, NodeId, SpaceId) {
    let mut ws = Workspace::default();
    let profile = stored_profile(&mut ws, ProfileKind::Default, "Default");
    stored_container(&mut ws, profile, None, ContainerKind::Favorites);
    let space = stored_space(&mut ws, profile, "Work", Position::first());
    let today = container_of(&ws, space, ContainerKind::Today);
    let first = Position::first();
    let second = Position::between(Some(&first), None).expect("a key fits");
    let third = Position::between(Some(&second), None).expect("a key fits");
    let one = stored_tab(&mut ws, today, first, "https://one.example");
    let two = stored_tab(&mut ws, today, second, middle_url);
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

/// The engine commands and store batches issued by one request.
struct Step {
    commands: Vec<EngineCommand>,
    batch: Vec<StoreOp>,
}

/// Runs one command, asserts it produced exactly one store batch that ends with `SetRev` at the
/// response's revision, and returns the commands it issued.
async fn run(rig: &Rig, command: Command) -> (ResponseBody, Step) {
    rig.engine.take_commands();
    let before = rig.batches().len();
    let body = rig.ok(command).await;
    let batches = rig.batches();
    let issued = &batches[before..];
    let batch = if issued.is_empty() {
        Vec::new()
    } else {
        assert_eq!(issued.len(), 1, "one store batch per command");
        issued[0].clone()
    };
    if let Some(last) = batch.last() {
        assert_eq!(
            last,
            &StoreOp::SetRev(rev_of(&body)),
            "the batch records the revision of the response"
        );
    }
    let step = Step {
        commands: rig.engine.take_commands(),
        batch,
    };
    (body, step)
}

fn every_command(node: NodeId, space: SpaceId) -> Vec<Command> {
    let placement = tab_in_today(space);
    vec![
        Command::OpenTab {
            placement: placement.clone(),
            url: "https://a.example".to_owned(),
            activate: true,
        },
        Command::CloseTab { node },
        Command::SelectTab { node },
        Command::SelectSpace { space },
        Command::MoveNode {
            node,
            to: placement.clone(),
        },
        Command::CreateFolder {
            placement,
            title: "f".to_owned(),
        },
        Command::RenameNode {
            node,
            title: "t".to_owned(),
        },
        Command::CreateSpace {
            name: "s".to_owned(),
        },
        Command::Navigate {
            node,
            url: "https://b.example".to_owned(),
        },
        Command::Back { node },
        Command::Forward { node },
        Command::Reload { node },
    ]
}

#[tokio::test]
async fn bootstrap_sequence_and_persistence() {
    let rig = Rig::empty();
    let snapshot = rig.snapshot().await;
    let tab = all_nodes(&snapshot)[0].id;

    assert_eq!(rig.startup.len(), 4);
    let (window, sidebar, content) = match rig.startup.as_slice() {
        [
            EngineCommand::CreateWindow { window, .. },
            EngineCommand::CreateView {
                view: sidebar,
                spec: side,
                ..
            },
            EngineCommand::CreateView {
                view: content,
                spec: tab_spec,
                ..
            },
            EngineCommand::Show { view: shown },
        ] => {
            assert_eq!(side.kind, ViewKind::Sidebar);
            assert_eq!(side.url, "app://ui/sidebar.html");
            assert_eq!(tab_spec.kind, ViewKind::Content);
            assert_eq!(tab_spec.url, "about:blank");
            assert_eq!(shown, content);
            (*window, *sidebar, *content)
        }
        other => panic!("unexpected startup sequence: {other:?}"),
    };
    assert_eq!(window, snapshot.window);
    assert_eq!(sidebar, rig.sidebar);

    let ops: Vec<StoreOp> = rig.batches().into_iter().flatten().collect();
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
    assert_eq!(ops.last(), Some(&StoreOp::SetRev(1)));

    assert!(content != sidebar);
    let shown = all_nodes(&snapshot);
    assert_eq!(shown.len(), 1);
    assert!(shown[0].live);
    assert_eq!(shown[0].url.as_deref(), Some("about:blank"));
    assert_eq!(snapshot.selected, Some(tab));
    assert_eq!(snapshot.rev, 1);
}

#[tokio::test]
async fn restore_suspends_every_tab_and_loads_only_the_selected_one() {
    let (ws, one, two, three, space) = stored_workspace("https://two.example");
    let rig = Rig::start(FakeStore::with_workspace(ws));

    let window = match rig.startup.as_slice() {
        [
            EngineCommand::CreateWindow { window, .. },
            EngineCommand::CreateView { spec: sidebar, .. },
            EngineCommand::CreateView {
                view: content,
                spec,
                ..
            },
            EngineCommand::Show { view: shown },
        ] => {
            assert_eq!(sidebar.kind, ViewKind::Sidebar);
            assert_eq!(spec.kind, ViewKind::Content);
            assert_eq!(spec.url, "https://two.example");
            assert_eq!(shown, content);
            *window
        }
        other => panic!("unexpected restore sequence: {other:?}"),
    };

    // Only the revision changed: the selected tab gained a view.
    let batches = rig.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0], vec![StoreOp::SetRev(4)]);

    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.window, window);
    assert_eq!(snapshot.active_space, space);
    assert_eq!(snapshot.selected, Some(two));
    assert!(!node(&snapshot, one).live);
    assert!(node(&snapshot, two).live);
    assert!(!node(&snapshot, three).live);
}

#[tokio::test]
async fn restore_replaces_an_unloadable_url_with_about_blank_and_keeps_the_stored_value() {
    let (ws, _, two, _, _) = stored_workspace("javascript:alert(1)");
    let rig = Rig::start(FakeStore::with_workspace(ws));

    assert!(matches!(
        rig.startup.iter().find(|command| matches!(command, EngineCommand::CreateView { spec, .. } if spec.kind == ViewKind::Content)),
        Some(EngineCommand::CreateView { spec, .. }) if spec.url == "about:blank"
    ));
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.selected, Some(two));
    assert_eq!(
        node(&snapshot, two).url.as_deref(),
        Some("javascript:alert(1)")
    );
}

#[tokio::test]
async fn open_select_and_close_issue_the_full_engine_sequence() {
    let rig = Rig::empty();
    let first = all_nodes(&rig.snapshot().await)[0].id;
    let first_view = last_content_view(&rig.startup).expect("the first tab has a view");
    let space = space_of(&rig.snapshot().await);

    let (body, step) = run(
        &rig,
        Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://b.example".to_owned(),
            activate: true,
        },
    )
    .await;
    let second = opened(body);
    let second_view = match step.commands.as_slice() {
        [
            EngineCommand::CreateView { view, spec, .. },
            EngineCommand::Show { view: shown },
        ] if spec.kind == ViewKind::Content && spec.url == "https://b.example" => {
            assert_eq!(shown, view);
            *view
        }
        other => panic!("unexpected open sequence: {other:?}"),
    };
    assert_eq!(step.batch.len(), 3, "node, session, rev: {:?}", step.batch);

    let (_, step) = run(&rig, Command::SelectTab { node: first }).await;
    assert_eq!(
        step.commands,
        vec![EngineCommand::Show { view: first_view }]
    );

    let (_, step) = run(&rig, Command::CloseTab { node: first }).await;
    assert_eq!(
        step.commands,
        vec![
            EngineCommand::Close { view: first_view },
            EngineCommand::Show { view: second_view },
        ]
    );

    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.selected, Some(second));
    assert_eq!(all_nodes(&snapshot).len(), 1);
}

#[tokio::test]
async fn closing_the_last_tab_opens_a_new_blank_tab() {
    let rig = Rig::empty();
    let only = all_nodes(&rig.snapshot().await)[0].id;
    let only_view = last_content_view(&rig.startup).expect("the tab has a view");

    let (_, step) = run(&rig, Command::CloseTab { node: only }).await;
    match step.commands.as_slice() {
        [
            EngineCommand::Close { view },
            EngineCommand::CreateView {
                view: created,
                spec,
                ..
            },
            EngineCommand::Show { view: shown },
        ] if spec.url == "about:blank" => {
            assert_eq!(*view, only_view);
            assert_eq!(shown, created);
        }
        other => panic!("unexpected close sequence: {other:?}"),
    }

    let snapshot = rig.snapshot().await;
    let tabs = all_nodes(&snapshot);
    assert_eq!(tabs.len(), 1);
    assert_ne!(tabs[0].id, only);
    assert_eq!(snapshot.selected, Some(tabs[0].id));
    assert!(node(&snapshot, tabs[0].id).live);
}

#[tokio::test]
async fn closing_the_selected_tab_selects_the_previous_sibling_when_there_is_no_next() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let first = all_nodes(&rig.snapshot().await)[0].id;
    let kept = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://b.example".to_owned(),
            activate: false,
        })
        .await,
    );
    let (body, _) = run(
        &rig,
        Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://c.example".to_owned(),
            activate: true,
        },
    )
    .await;
    let last = opened(body);

    let (_, step) = run(&rig, Command::CloseTab { node: last }).await;
    assert!(matches!(
        step.commands.as_slice(),
        [
            EngineCommand::Close { .. },
            EngineCommand::CreateView { spec, .. },
            EngineCommand::Show { .. },
        ] if spec.url == "https://b.example"
    ));
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.selected, Some(kept));
    assert_ne!(snapshot.selected, Some(first));
}

#[tokio::test]
async fn closing_a_selected_tab_falls_back_to_another_tab_of_the_space() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let home = all_nodes(&rig.snapshot().await)[0].id;
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(space),
        title: "Group".to_owned(),
    })
    .await;
    let group = folder_titled(&rig, "Group").await;
    let inside = opened(
        rig.ok(Command::OpenTab {
            placement: Placement {
                space,
                container: ContainerKind::Today,
                parent: Some(group),
                before: None,
            },
            url: "https://inside.example".to_owned(),
            activate: true,
        })
        .await,
    );

    let (_, step) = run(&rig, Command::CloseTab { node: inside }).await;
    assert!(matches!(
        step.commands.first(),
        Some(EngineCommand::Close { .. })
    ));
    assert!(matches!(
        step.commands.last(),
        Some(EngineCommand::Show { .. })
    ));
    assert_eq!(
        step.commands.len(),
        2,
        "no new tab: another tab of the space is selected"
    );
    assert_eq!(rig.snapshot().await.selected, Some(home));
}

#[tokio::test]
async fn closing_a_folder_with_children_is_a_conflict_and_an_empty_one_is_deleted() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(space),
        title: "Full".to_owned(),
    })
    .await;
    let folder = folder_titled(&rig, "Full").await;
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

    let error = rig
        .send(Command::CloseTab { node: folder })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "folder not empty");

    rig.ok(Command::CloseTab { node: child }).await;
    rig.ok(Command::CloseTab { node: folder }).await;
    assert!(
        !all_nodes(&rig.snapshot().await)
            .iter()
            .any(|candidate| candidate.id == folder)
    );
}

#[tokio::test]
async fn moving_a_folder_across_containers_rewrites_its_children() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
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

    let (_, step) = run(
        &rig,
        Command::MoveNode {
            node: folder,
            to: Placement {
                space,
                container: ContainerKind::Pinned,
                parent: None,
                before: None,
            },
        },
    )
    .await;
    assert!(
        step.batch
            .iter()
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
async fn moving_a_folder_into_its_own_subtree_is_a_conflict() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
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
async fn moving_the_selected_tab_into_another_space_switches_the_active_space() {
    let rig = Rig::empty();
    let home = space_of(&rig.snapshot().await);
    let other = match run(
        &rig,
        Command::CreateSpace {
            name: "Work".to_owned(),
        },
    )
    .await
    .0
    {
        ResponseBody::SpaceCreated { space, .. } => space,
        other => panic!("expected SpaceCreated, got {other:?}"),
    };
    rig.ok(Command::CreateFolder {
        placement: tab_in_today(home),
        title: "Carry".to_owned(),
    })
    .await;
    let folder = folder_titled(&rig, "Carry").await;
    let child = opened(
        rig.ok(Command::OpenTab {
            placement: Placement {
                space: home,
                container: ContainerKind::Today,
                parent: Some(folder),
                before: None,
            },
            url: "https://carried.example".to_owned(),
            activate: true,
        })
        .await,
    );

    rig.ok(Command::MoveNode {
        node: folder,
        to: Placement {
            space: other,
            container: ContainerKind::Pinned,
            parent: None,
            before: None,
        },
    })
    .await;
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.active_space, other);
    assert_eq!(snapshot.selected, Some(child));
    assert!(node(&snapshot, child).live);
}

#[tokio::test]
async fn moving_the_selected_tab_to_the_archive_selects_another_tab() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let selected = all_nodes(&rig.snapshot().await)[0].id;
    let selected_view = last_content_view(&rig.startup).expect("the tab has a view");
    let other = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://other.example".to_owned(),
            activate: false,
        })
        .await,
    );

    let (_, step) = run(
        &rig,
        Command::MoveNode {
            node: selected,
            to: Placement {
                space,
                container: ContainerKind::Archive,
                parent: None,
                before: None,
            },
        },
    )
    .await;
    assert!(matches!(
        step.commands.as_slice(),
        [
            EngineCommand::Close { view },
            EngineCommand::CreateView { spec, .. },
            EngineCommand::Show { .. },
        ] if *view == selected_view && spec.url == "https://other.example"
    ));
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.selected, Some(other));
    assert!(!node(&snapshot, selected).live);
}

#[tokio::test]
async fn select_space_restores_the_last_selected_tab_or_opens_a_blank_one() {
    let rig = Rig::empty();
    let home = space_of(&rig.snapshot().await);
    let home_tab = all_nodes(&rig.snapshot().await)[0].id;
    let other = match rig
        .ok(Command::CreateSpace {
            name: "Work".to_owned(),
        })
        .await
    {
        ResponseBody::SpaceCreated { space, .. } => space,
        other => panic!("expected SpaceCreated, got {other:?}"),
    };

    let (_, step) = run(&rig, Command::SelectSpace { space: other }).await;
    assert!(matches!(
        step.commands.as_slice(),
        [EngineCommand::CreateView { spec, .. }, EngineCommand::Show { .. }] if spec.url == "about:blank"
    ));
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.active_space, other);
    let blank = snapshot.selected.expect("a blank tab is selected");
    assert_ne!(blank, home_tab);

    let (_, step) = run(&rig, Command::SelectSpace { space: home }).await;
    assert!(matches!(
        step.commands.as_slice(),
        [EngineCommand::Show { .. }]
    ));
    let snapshot = rig.snapshot().await;
    assert_eq!(snapshot.active_space, home);
    assert_eq!(snapshot.selected, Some(home_tab));

    let error = rig
        .send(Command::SelectSpace {
            space: SpaceId::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn stale_navigation_callback_is_dropped() {
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
async fn crashed_view_turns_the_node_not_live_and_bumps_the_revision() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    let before = rig.snapshot().await.rev;

    let snapshot = rig.emit(EngineEvent::Crashed { view }).await;
    assert!(!node(&snapshot, tab).live);
    assert_eq!(snapshot.rev, before + 1);
    assert_eq!(
        rig.batches().last().and_then(|batch| batch.last()),
        Some(&StoreOp::SetRev(before + 1))
    );
}

#[tokio::test]
async fn a_store_failure_while_handling_an_engine_event_leaves_the_state_consistent() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let before = rig.snapshot().await;

    rig.store.fail_next_apply();
    let snapshot = rig.emit(EngineEvent::Crashed { view }).await;

    assert_eq!(
        snapshot.rev, before.rev,
        "the failed change did not bump the revision"
    );
    assert!(
        node(&snapshot, all_nodes(&before)[0].id).live,
        "the view mapping is kept"
    );
}

#[tokio::test]
async fn a_store_failure_leaves_the_state_and_the_engine_untouched() {
    let rig = Rig::empty();
    rig.engine.take_commands();
    let before = rig.snapshot().await;

    rig.store.fail_next_apply();
    let error = rig
        .send(Command::OpenTab {
            placement: tab_in_today(space_of(&before)),
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
async fn a_restricted_principal_is_refused_every_command_through_the_handle() {
    let rig = Rig::empty();
    let node_id = all_nodes(&rig.snapshot().await)[0].id;
    let space = space_of(&rig.snapshot().await);
    rig.engine.take_commands();
    let before = rig.batches().len();

    for command in every_command(node_id, space) {
        let error = rig
            .send_as(Principal::Restricted, command)
            .await
            .expect_err("a restricted principal is refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
    }
    let response = rig
        .handle
        .request(
            Principal::Restricted,
            Request {
                v: API_VERSION,
                id: 4,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Forbidden
    ));
    assert!(rig.engine.take_commands().is_empty());
    assert_eq!(
        rig.batches().len(),
        before,
        "a refused command writes nothing"
    );
}

#[tokio::test]
async fn app_ui_urls_are_refused_for_every_principal() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let node_id = all_nodes(&rig.snapshot().await)[0].id;
    let app = "app://ui/sidebar.html".to_owned();

    let error = rig
        .send(Command::OpenTab {
            placement: tab_in_today(space),
            url: app.clone(),
            activate: true,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BadRequest);

    let error = rig
        .send(Command::Navigate {
            node: node_id,
            url: app.clone(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BadRequest);

    let request = Request {
        v: API_VERSION,
        id: 5,
        body: Body::Command(Command::OpenTab {
            placement: tab_in_today(space),
            url: app,
            activate: true,
        }),
    };
    let response = rig.handle.request(Principal::System, request).await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::BadRequest
    ));
}

#[tokio::test]
async fn a_recreated_sidebar_replaces_the_old_ui_principal() {
    let rig = Rig::empty();
    let old = rig.sidebar;

    rig.events_in.send(EngineEvent::Closed { view: old });
    let refused = rig
        .handle
        .request(
            Principal::Ui { view: old },
            Request {
                v: API_VERSION,
                id: 6,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(
        refused.result,
        ApiResult::Err { error } if error.code == ErrorCode::Forbidden
    ));

    let created = rig.engine.take_commands();
    let fresh = match created.as_slice() {
        [EngineCommand::CreateView { view, spec, .. }] if spec.kind == ViewKind::Sidebar => *view,
        other => panic!("expected one sidebar CreateView, got {other:?}"),
    };
    assert_ne!(fresh, old);
    let accepted = rig
        .handle
        .request(
            Principal::Ui { view: fresh },
            Request {
                v: API_VERSION,
                id: 7,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(accepted.result, ApiResult::Ok { .. }));
}

#[tokio::test]
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
async fn an_unregistered_ui_view_is_refused() {
    let rig = Rig::empty();
    let response = rig
        .handle
        .request(
            Principal::Ui {
                view: ViewId(u64::MAX),
            },
            Request {
                v: API_VERSION,
                id: 9,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Forbidden
    ));
}

#[tokio::test]
async fn published_events_carry_increasing_revisions() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
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
        assert_eq!(event.rev, rev_of(&body));
        assert!(event.rev > last);
        last = event.rev;
    }
}

#[tokio::test]
async fn the_snapshot_nests_children_and_flags_only_live_nodes() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let folder = opened(
        rig.ok(Command::CreateFolder {
            placement: tab_in_today(space),
            title: "Folder".to_owned(),
        })
        .await,
    );
    assert_eq!(
        folder_titled(&rig, "Folder").await,
        folder,
        "CreateFolder answers with the new node"
    );
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
async fn navigating_a_suspended_tab_creates_a_view_without_showing_it() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let background = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://background.example".to_owned(),
            activate: false,
        })
        .await,
    );

    let (_, step) = run(
        &rig,
        Command::Navigate {
            node: background,
            url: "https://next.example".to_owned(),
        },
    )
    .await;
    assert!(matches!(
        step.commands.as_slice(),
        [EngineCommand::CreateView { spec, .. }] if spec.url == "https://next.example"
    ));
    assert!(node(&rig.snapshot().await, background).live);
}

#[tokio::test]
async fn a_popup_from_a_favorites_tab_opens_in_the_active_space() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let favorite = opened(
        rig.ok(Command::OpenTab {
            placement: Placement {
                space,
                container: ContainerKind::Favorites,
                parent: None,
                before: None,
            },
            url: "https://favorite.example".to_owned(),
            activate: true,
        })
        .await,
    );
    let view = last_content_view(&rig.engine.take_commands()).expect("the favorite has a view");

    let snapshot = rig
        .emit(EngineEvent::PopupRequested {
            opener: view,
            url: "https://popup.example".to_owned(),
            user_gesture: true,
        })
        .await;
    let popup = all_nodes(&snapshot)
        .into_iter()
        .find(|tab| tab.url.as_deref() == Some("https://popup.example"))
        .expect("the popup became a tab");
    let today = snapshot.spaces[0]
        .containers
        .iter()
        .find(|container| container.kind == ContainerKind::Today)
        .expect("the today container");
    assert!(today.nodes.iter().any(|candidate| candidate.id == popup.id));
    assert_eq!(snapshot.selected, Some(popup.id));
    assert_ne!(popup.id, favorite);
}

#[tokio::test]
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
async fn a_burst_of_title_changes_does_not_hold_back_commands_or_flood_the_store() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let space = space_of(&rig.snapshot().await);
    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 1,
        url: "https://busy.example".to_owned(),
    })
    .await;
    let batches_before = rig.batches().len();

    let mut worst = Duration::ZERO;
    for index in 0..1000 {
        rig.events_in.send(EngineEvent::TitleChanged {
            view,
            nav_gen: 1,
            title: format!("title {index}"),
        });
        if index % 100 == 50 {
            let started = Instant::now();
            rig.ok(Command::CreateFolder {
                placement: tab_in_today(space),
                title: format!("folder {index}"),
            })
            .await;
            worst = worst.max(started.elapsed());
        }
    }
    let snapshot = rig.snapshot().await;
    assert_eq!(
        node(&snapshot, all_nodes(&snapshot)[0].id).title,
        "title 999"
    );
    assert!(
        worst < Duration::from_millis(500),
        "a command waited {worst:?} behind the burst"
    );
    // Ten folder commands write one batch each; the burst itself adds at most two flushes.
    let issued = rig.batches().len() - batches_before;
    assert!(
        issued <= 10 + 2,
        "the burst produced {issued} store batches"
    );
}

#[tokio::test]
async fn title_changes_reach_the_store_within_one_flush_interval() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 1,
        url: "https://persist.example".to_owned(),
    })
    .await;
    rig.emit(EngineEvent::TitleChanged {
        view,
        nav_gen: 1,
        title: "Persisted".to_owned(),
    })
    .await;

    tokio::time::sleep(Duration::from_millis(1200)).await;
    let persisted = rig.batches().into_iter().flatten().any(
        |op| matches!(op, StoreOp::UpsertNode(node) if node.id == tab && node.title == "Persisted"),
    );
    assert!(persisted, "the coalesced title was flushed");
}

#[tokio::test]
async fn shutdown_flushes_pending_writes_stops_the_core_and_is_idempotent() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 1,
        url: "https://last.example".to_owned(),
    })
    .await;
    rig.emit(EngineEvent::TitleChanged {
        view,
        nav_gen: 1,
        title: "Last".to_owned(),
    })
    .await;

    rig.handle.shutdown();
    let flushed = rig.batches().into_iter().flatten().any(
        |op| matches!(op, StoreOp::UpsertNode(node) if node.id == tab && node.title == "Last"),
    );
    assert!(flushed, "shutdown flushed the pending title");

    rig.handle.shutdown();
    let response = rig
        .handle
        .request(
            rig.ui(),
            Request {
                v: API_VERSION,
                id: 8,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Unavailable
    ));
}

#[tokio::test]
async fn closing_the_window_stops_the_actor() {
    let rig = Rig::empty();
    let window = rig.snapshot().await.window;
    rig.events_in.send(EngineEvent::WindowClosed { window });

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
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Unavailable
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_are_totally_ordered() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
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
                ApiResult::Ok { body } => rev_of(&body),
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

/// The id of the folder titled `title`.
async fn folder_titled(rig: &Rig, title: &str) -> NodeId {
    all_nodes(&rig.snapshot().await)
        .into_iter()
        .find(|candidate| candidate.kind == NodeKind::Folder && candidate.title == title)
        .unwrap_or_else(|| panic!("a folder titled {title}"))
        .id
}

/// Waits until the actor has handled everything queued before this call. Uses the System principal,
/// so it works after the UI principal has been refused.
async fn settle(rig: &Rig) {
    let _ = rig
        .handle
        .request(
            Principal::System,
            Request {
                v: API_VERSION,
                id: 10,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
}

fn sidebar_created(commands: &[EngineCommand]) -> ViewId {
    match commands {
        [EngineCommand::CreateView { view, spec, .. }] if spec.kind == ViewKind::Sidebar => *view,
        other => panic!("expected one sidebar CreateView, got {other:?}"),
    }
}

async fn ui_refused(rig: &Rig, view: ViewId) -> bool {
    let response = rig
        .handle
        .request(
            Principal::Ui { view },
            Request {
                v: API_VERSION,
                id: 11,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    matches!(response.result, ApiResult::Err { error } if error.code == ErrorCode::Forbidden)
}

#[tokio::test]
async fn commands_that_create_or_select_views_conflict_while_the_window_closes() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    let window = rig.snapshot().await.window;
    rig.events_in.send(EngineEvent::WindowClosing { window });
    settle(&rig).await;
    rig.engine.take_commands();

    for command in [
        Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://late.example".to_owned(),
            activate: true,
        },
        Command::SelectTab { node: tab },
        Command::Navigate {
            node: tab,
            url: "https://late.example".to_owned(),
        },
    ] {
        let error = rig.send(command).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.message, "window is closing");
    }
    assert!(rig.engine.take_commands().is_empty());
    assert_eq!(
        all_nodes(&rig.snapshot().await).len(),
        1,
        "no tab was added"
    );
}

#[tokio::test]
async fn create_failed_for_a_selected_content_view_keeps_the_selection_and_unlives_the_node() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let opened_node = opened(
        rig.ok(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://b.example".to_owned(),
            activate: true,
        })
        .await,
    );
    let view = last_content_view(&rig.engine.take_commands()).expect("the new tab has a view");
    let before = rig.snapshot().await;
    assert_eq!(before.selected, Some(opened_node));

    rig.store.fail_next_apply();
    let snapshot = rig.emit(EngineEvent::CreateFailed { view }).await;
    assert_eq!(
        snapshot.rev, before.rev,
        "a failed store write leaves the revision alone"
    );
    assert!(
        node(&snapshot, opened_node).live,
        "the store write failed, so nothing changed"
    );

    let snapshot = rig.emit(EngineEvent::CreateFailed { view }).await;
    assert!(!node(&snapshot, opened_node).live);
    assert_eq!(snapshot.selected, Some(opened_node), "the selection stays");
    assert_eq!(snapshot.rev, before.rev + 1);
    assert_eq!(
        rig.batches().last().and_then(|batch| batch.last()),
        Some(&StoreOp::SetRev(before.rev + 1))
    );
    assert!(rig.engine.take_commands().is_empty());
}

#[tokio::test]
async fn a_crashed_sidebar_is_recreated_when_the_window_is_not_closing() {
    let rig = Rig::empty();
    rig.events_in
        .send(EngineEvent::Crashed { view: rig.sidebar });
    settle(&rig).await;
    let fresh = sidebar_created(&rig.engine.take_commands());
    assert_ne!(fresh, rig.sidebar);
}

/// Whether the store holds node `tab` with `title`.
fn title_stored(rig: &Rig, tab: NodeId, title: &str) -> bool {
    rig.batches()
        .into_iter()
        .flatten()
        .any(|op| matches!(op, StoreOp::UpsertNode(node) if node.id == tab && node.title == title))
}

#[tokio::test]
async fn quitting_a_window_recreates_nothing_and_flushes_the_store() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    rig.emit(EngineEvent::Navigated {
        view,
        nav_gen: 1,
        url: "https://quit.example".to_owned(),
    })
    .await;
    rig.emit(EngineEvent::TitleChanged {
        view,
        nav_gen: 1,
        title: "Quitting".to_owned(),
    })
    .await;
    rig.engine.take_commands();
    let window = rig.snapshot().await.window;

    // The engine's quit order: closing, then each view's close, then the window. The title is still
    // pending when the window starts closing, so the flush must happen on WindowClosing itself.
    rig.events_in.send(EngineEvent::WindowClosing { window });
    rig.snapshot().await;
    assert!(
        title_stored(&rig, tab, "Quitting"),
        "the pending title reached the store before WindowClosed"
    );

    rig.events_in
        .send(EngineEvent::Closed { view: rig.sidebar });
    rig.events_in.send(EngineEvent::Closed { view });
    rig.events_in.send(EngineEvent::WindowClosed { window });
    let response = rig
        .handle
        .request(
            rig.ui(),
            Request {
                v: API_VERSION,
                id: 12,
                body: Body::Query(Query::Snapshot),
            },
        )
        .await;
    assert!(matches!(
        response.result,
        ApiResult::Err { error } if error.code == ErrorCode::Unavailable
    ));
    assert!(
        rig.engine.take_commands().is_empty(),
        "no CreateView or Close during quit"
    );
}

#[tokio::test]
async fn a_closing_window_ignores_crashes_create_failures_and_popups() {
    let rig = Rig::empty();
    let view = last_content_view(&rig.startup).expect("the tab has a view");
    let tab = all_nodes(&rig.snapshot().await)[0].id;
    let window = rig.snapshot().await.window;
    rig.events_in.send(EngineEvent::WindowClosing { window });
    let before = rig.snapshot().await;
    rig.engine.take_commands();

    rig.events_in.send(EngineEvent::Crashed { view });
    rig.events_in.send(EngineEvent::CreateFailed { view });
    rig.events_in
        .send(EngineEvent::Crashed { view: rig.sidebar });
    rig.events_in.send(EngineEvent::PopupRequested {
        opener: view,
        url: "https://popup.example".to_owned(),
        user_gesture: true,
    });
    let after = rig.snapshot().await;

    assert!(node(&after, tab).live, "the view exit is not handled");
    assert_eq!(after.rev, before.rev, "nothing changed");
    assert_eq!(all_nodes(&after).len(), 1, "no popup tab opened");
    assert!(
        rig.engine.take_commands().is_empty(),
        "no engine command for a closing window"
    );
}

#[tokio::test]
async fn a_repeated_window_closing_changes_nothing() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    let window = rig.snapshot().await.window;
    rig.events_in.send(EngineEvent::WindowClosing { window });
    let before = rig.snapshot().await;
    let batches = rig.batches().len();

    rig.events_in.send(EngineEvent::WindowClosing { window });
    let after = rig.snapshot().await;
    assert_eq!(after.rev, before.rev);
    assert_eq!(rig.batches().len(), batches, "no second flush");
    let error = rig
        .send(Command::OpenTab {
            placement: tab_in_today(space),
            url: "https://late.example".to_owned(),
            activate: true,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
}

#[tokio::test]
async fn window_closing_for_another_window_is_ignored() {
    let rig = Rig::empty();
    let space = space_of(&rig.snapshot().await);
    rig.events_in.send(EngineEvent::WindowClosing {
        window: WindowId::new(),
    });

    rig.ok(Command::OpenTab {
        placement: tab_in_today(space),
        url: "https://still-open.example".to_owned(),
        activate: true,
    })
    .await;
    assert!(matches!(
        rig.engine.take_commands().as_slice(),
        [EngineCommand::CreateView { .. }, EngineCommand::Show { .. }]
    ));
}

#[tokio::test]
async fn window_closed_for_another_window_is_ignored() {
    let rig = Rig::empty();
    rig.events_in.send(EngineEvent::WindowClosed {
        window: WindowId::new(),
    });
    assert_eq!(
        all_nodes(&rig.snapshot().await).len(),
        1,
        "the core still answers"
    );
}

#[tokio::test]
async fn a_close_that_never_completes_keeps_refusing_view_commands_and_answers_queries() {
    let clock = ManualClock::new();
    let rig = Rig::start_on(FakeStore::new(), clock.clone());
    let space = space_of(&rig.snapshot().await);
    let window = rig.snapshot().await.window;
    rig.events_in.send(EngineEvent::WindowClosing { window });
    settle(&rig).await;

    clock.advance(CLOSE_STALL + Duration::from_secs(1));
    settle(&rig).await;

    for _ in 0..2 {
        let error = rig
            .send(Command::OpenTab {
                placement: tab_in_today(space),
                url: "https://late.example".to_owned(),
                activate: true,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        rig.snapshot().await;
        clock.advance(CLOSE_STALL);
        settle(&rig).await;
    }
}

/// One sidebar death, then the sidebar the core created in response, if any.
async fn recreated_after(rig: &Rig, event: EngineEvent) -> Option<ViewId> {
    rig.events_in.send(event);
    settle(rig).await;
    let commands = rig.engine.take_commands();
    if commands.is_empty() {
        None
    } else {
        Some(sidebar_created(&commands))
    }
}

#[tokio::test]
async fn a_crash_loop_stops_after_three_recreations() {
    let clock = ManualClock::new();
    let rig = Rig::start_on(FakeStore::new(), clock.clone());
    let mut current = rig.sidebar;
    for attempt in 1..=3 {
        rig.events_in.send(EngineEvent::Created { view: current });
        current = recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .unwrap_or_else(|| panic!("recreation {attempt} is allowed"));
    }
    rig.events_in.send(EngineEvent::Created { view: current });
    assert!(
        recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .is_none(),
        "the fourth death stops the recreations"
    );
    assert!(ui_refused(&rig, current).await);
}

#[tokio::test]
async fn a_sidebar_that_lives_ten_seconds_resets_the_budget() {
    let clock = ManualClock::new();
    let rig = Rig::start_on(FakeStore::new(), clock.clone());
    let mut current = rig.sidebar;
    for _ in 0..2 {
        current = recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .expect("allowed");
    }

    rig.events_in.send(EngineEvent::Created { view: current });
    // The actor reads the clock when it handles `Created`, so settle before moving time.
    settle(&rig).await;
    clock.advance(Duration::from_secs(10));
    current = recreated_after(&rig, EngineEvent::Crashed { view: current })
        .await
        .expect("the healthy sidebar is recreated");

    for _ in 0..2 {
        current = recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .expect("allowed after the reset");
    }
    assert!(
        recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .is_none(),
        "the budget is spent again"
    );
}

#[tokio::test]
async fn the_budget_is_counted_over_a_rolling_thirty_seconds() {
    let clock = ManualClock::new();
    let rig = Rig::start_on(FakeStore::new(), clock.clone());
    let mut current = rig.sidebar;
    for _ in 0..2 {
        current = recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .expect("allowed");
    }

    clock.advance(Duration::from_secs(31));
    for _ in 0..3 {
        current = recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .expect("the earlier deaths left the window");
    }
    assert!(
        recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .is_none()
    );
}

#[tokio::test]
async fn create_failures_and_crashes_share_the_budget() {
    let clock = ManualClock::new();
    let rig = Rig::start_on(FakeStore::new(), clock.clone());
    let mut current = rig.sidebar;
    current = recreated_after(&rig, EngineEvent::CreateFailed { view: current })
        .await
        .expect("allowed");
    current = recreated_after(&rig, EngineEvent::Crashed { view: current })
        .await
        .expect("allowed");
    current = recreated_after(&rig, EngineEvent::CreateFailed { view: current })
        .await
        .expect("allowed");
    assert!(
        recreated_after(&rig, EngineEvent::Crashed { view: current })
            .await
            .is_none(),
        "the fourth failure of either kind stops the recreations"
    );
}
