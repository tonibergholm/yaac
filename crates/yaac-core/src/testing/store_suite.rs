//! A conformance suite for `Store` implementations. `yaac-store` runs it against SQLite. `FakeStore`
//! runs only `run_common`: it records batches and does not fold them into `load`, so the checks that
//! read back state do not apply to it.
//!
//! Each check is public so a caller can run and ignore them one at a time. Checks marked "needs
//! the model" call `TreeState::from_nodes` or `Position::from_raw` through `load`.

use crate::{Store, StoreOp};
use serde::Deserialize;
use serde::de::{IntoDeserializer, value::Error as DeError};
use yaac_model::{
    Container, ContainerId, ContainerKind, Node, NodeId, NodeKind, NodeState, Position, Profile,
    ProfileId, ProfileKind, SessionState, Space, SpaceId, WindowId, Workspace,
};

/// Runs every check against stores produced by `make`. Each call to `make` must return an empty,
/// independent store. Panics on the first failed check.
pub fn run_all(make: &dyn Fn() -> Box<dyn Store>) {
    run_common(make);
    check_profiles_and_containers_round_trip(make);
    check_large_batch_round_trips(make);
    check_upsert_overwrites(make);
    check_failed_batch_leaves_nothing(make);
    check_child_in_other_container_is_rejected(make);
    check_container_with_nodes_cannot_be_deleted(make);
    check_full_workspace_round_trips(make);
    check_delete_subtree_in_either_order(make);
    check_session_falls_back_when_selected_node_deleted(make);
}

/// The checks that every `Store` must pass, including ones that only record batches.
pub fn run_common(make: &dyn Fn() -> Box<dyn Store>) {
    check_empty_store_loads_empty(make);
    check_large_batch_applies(make);
}

pub fn check_empty_store_loads_empty(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let ws = store.load().expect("an empty store loads");
    assert!(ws.profiles.is_empty(), "profiles in an empty store");
    assert!(ws.spaces.is_empty(), "spaces in an empty store");
    assert!(ws.containers.is_empty(), "containers in an empty store");
    assert!(ws.tree.is_empty(), "nodes in an empty store");
    assert!(ws.sessions.is_empty(), "sessions in an empty store");
}

/// Applies 2 000 profiles and their favorites containers in one batch. FakeStore passes this too.
pub fn check_large_batch_applies(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    store
        .apply(large_batch(2_000))
        .expect("a large batch applies");
}

pub fn check_large_batch_round_trips(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    store
        .apply(large_batch(2_000))
        .expect("a large batch applies");
    let ws = store.load().expect("load after a large batch");
    assert_eq!(ws.profiles.len(), 2_000);
    assert_eq!(ws.containers.len(), 2_000);
}

pub fn check_profiles_and_containers_round_trip(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let standard = profile("standard", ProfileKind::Standard);
    let default = profile("default", ProfileKind::Default);
    let favorites = favorites_of(&standard);
    let favorites_default = favorites_of(&default);
    store
        .apply(vec![
            StoreOp::UpsertProfile(standard.clone()),
            StoreOp::UpsertProfile(default.clone()),
            StoreOp::UpsertContainer(favorites.clone()),
            StoreOp::UpsertContainer(favorites_default.clone()),
        ])
        .expect("apply");

    let ws = store.load().expect("load");
    assert_eq!(ws.profiles.get(&standard.id), Some(&standard));
    assert_eq!(ws.profiles.get(&default.id), Some(&default));
    assert_eq!(ws.containers.get(&favorites.id), Some(&favorites));
    assert_eq!(
        ws.containers.get(&favorites_default.id),
        Some(&favorites_default)
    );
    assert_eq!(ws.profiles.len(), 2);
    assert_eq!(ws.containers.len(), 2);
}

pub fn check_upsert_overwrites(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let mut p = profile("first", ProfileKind::Standard);
    store
        .apply(vec![StoreOp::UpsertProfile(p.clone())])
        .expect("first upsert");
    p.name = "second".into();
    store
        .apply(vec![StoreOp::UpsertProfile(p.clone())])
        .expect("second upsert");

    let ws = store.load().expect("load");
    assert_eq!(ws.profiles.len(), 1, "an upsert must not add a row");
    assert_eq!(ws.profiles.get(&p.id), Some(&p));
}

/// A batch with a valid profile, container and an orphan child (its parent id is unknown) fails at
/// commit. The profile and container in the same batch must not survive.
pub fn check_failed_batch_leaves_nothing(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let p = profile("p", ProfileKind::Standard);
    let c = favorites_of(&p);
    let orphan = node(c.id, Some(NodeId::new()), "a1", NodeKind::Tab, "orphan");
    let result = store.apply(vec![
        StoreOp::UpsertProfile(p.clone()),
        StoreOp::UpsertContainer(c.clone()),
        StoreOp::UpsertNode(orphan),
    ]);
    assert!(result.is_err(), "a batch with a missing parent must fail");

    let ws = store.load().expect("load after a failed batch");
    assert!(ws.profiles.is_empty(), "a failed batch left a profile");
    assert!(ws.containers.is_empty(), "a failed batch left a container");
    assert!(ws.tree.is_empty(), "a failed batch left a node");

    // The store still works after the failure.
    store
        .apply(vec![StoreOp::UpsertProfile(p.clone())])
        .expect("apply after a failed batch");
    assert_eq!(store.load().expect("load").profiles.len(), 1);
}

/// A child's container must equal its parent's. The batch fails and leaves nothing behind.
pub fn check_child_in_other_container_is_rejected(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let p1 = profile("p1", ProfileKind::Standard);
    let p2 = profile("p2", ProfileKind::Standard);
    let c1 = favorites_of(&p1);
    let c2 = favorites_of(&p2);
    let parent = node(c1.id, None, "a1", NodeKind::Folder, "parent");
    let child = node(c2.id, Some(parent.id), "a1", NodeKind::Tab, "child");
    let result = store.apply(vec![
        StoreOp::UpsertProfile(p1),
        StoreOp::UpsertProfile(p2),
        StoreOp::UpsertContainer(c1),
        StoreOp::UpsertContainer(c2),
        StoreOp::UpsertNode(parent),
        StoreOp::UpsertNode(child),
    ]);
    assert!(
        result.is_err(),
        "a child in another container must be rejected"
    );
    let ws = store.load().expect("load after a rejected batch");
    assert!(ws.tree.is_empty(), "a rejected batch left a node");
}

/// Needs the model: the container still holds a node, so `load` builds a tree.
pub fn check_container_with_nodes_cannot_be_deleted(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let p = profile("p", ProfileKind::Standard);
    let c = favorites_of(&p);
    let root = node(c.id, None, "a1", NodeKind::Tab, "root");
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertContainer(c.clone()),
            StoreOp::UpsertNode(root.clone()),
        ])
        .expect("setup");
    let result = store.apply(vec![StoreOp::DeleteContainer(c.id)]);
    assert!(
        result.is_err(),
        "a container that still holds nodes must not be deleted"
    );
    let ws = store.load().expect("load");
    assert!(ws.containers.contains_key(&c.id));
    assert_eq!(ws.tree.get(root.id), Some(&root));
}

/// Needs the model: `load` builds a tree and rebuilds the sidebar order.
pub fn check_full_workspace_round_trips(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let f = Fixture::new();
    store.apply(f.batch()).expect("apply fixture");
    assert_fixture_loaded(&store.load().expect("load"), &f);
}

/// Needs the model. `open` must return a handle to the same durable store each time it is called
/// (for SQLite, the same file). The fixture is written, the handle is dropped, and a new handle
/// must load the same workspace.
pub fn check_workspace_survives_reopen(open: &dyn Fn() -> Box<dyn Store>) {
    let f = Fixture::new();
    {
        let mut store = open();
        store.apply(f.batch()).expect("apply fixture");
    }
    let mut reopened = open();
    assert_fixture_loaded(&reopened.load().expect("load after reopen"), &f);
}

fn assert_fixture_loaded(ws: &Workspace, f: &Fixture) {
    assert_eq!(ws.profiles.get(&f.profile.id), Some(&f.profile));
    assert_eq!(ws.spaces.get(&f.space.id), Some(&f.space));
    assert_eq!(ws.containers.get(&f.today.id), Some(&f.today));
    assert_eq!(ws.containers.get(&f.favorites.id), Some(&f.favorites));
    assert_eq!(ws.sessions.get(&f.window), Some(&f.session));
    assert_eq!(ws.tree.len(), f.nodes.len());
    for node in &f.nodes {
        assert_eq!(ws.tree.get(node.id), Some(node), "node {} differs", node.id);
    }
    let top: Vec<NodeId> = ws
        .tree
        .children(f.today.id, None)
        .iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(
        top,
        vec![f.folder.id, f.split.id],
        "top-level sibling order"
    );
}

/// Needs the model. Deleting a folder with its children, listed children-first and parent-first,
/// removes exactly that subtree.
pub fn check_delete_subtree_in_either_order(make: &dyn Fn() -> Box<dyn Store>) {
    let f = Fixture::new();
    let removed = [f.folder.id, f.tab_one.id, f.tab_two.id];
    let children_first = [f.tab_one.id, f.tab_two.id, f.folder.id];
    let parent_first = [f.folder.id, f.tab_one.id, f.tab_two.id];
    for order in [children_first, parent_first] {
        let mut store = make();
        store.apply(f.batch()).expect("apply fixture");
        store
            .apply(order.iter().map(|id| StoreOp::DeleteNode(*id)).collect())
            .expect("delete subtree");
        let ws = store.load().expect("load");
        assert_eq!(ws.tree.len(), f.nodes.len() - removed.len());
        for id in removed {
            assert!(ws.tree.get(id).is_none(), "node {id} survived the delete");
        }
        assert!(ws.tree.get(f.split.id).is_some());
        assert!(ws.tree.get(f.split_one.id).is_some());
    }
}

/// Needs the model. The session selected a node that was deleted: the session stays, with no
/// selection. The core then falls back (architecture section 7).
pub fn check_session_falls_back_when_selected_node_deleted(make: &dyn Fn() -> Box<dyn Store>) {
    let mut store = make();
    let f = Fixture::new();
    store.apply(f.batch()).expect("apply fixture");
    store
        .apply(vec![StoreOp::DeleteNode(f.tab_one.id)])
        .expect("delete the selected node");
    let ws = store.load().expect("load");
    let session = ws.sessions.get(&f.window).expect("session survives");
    assert_eq!(session.selected_node_id, None);
    assert_eq!(session.space_id, f.space.id);
}

// Builders. Positions are plain base-62 keys, valid for the model's `Position::from_raw`.

fn pos(key: &str) -> Position {
    Position::deserialize(IntoDeserializer::<DeError>::into_deserializer(key))
        .expect("test position keys are valid")
}

fn profile(name: &str, kind: ProfileKind) -> Profile {
    Profile {
        id: ProfileId::new(),
        kind,
        name: name.into(),
        cache_dir: format!("profiles/{name}"),
        created_at: 1_000,
    }
}

fn favorites_of(profile: &Profile) -> Container {
    Container {
        id: ContainerId::new(),
        profile_id: profile.id,
        space_id: None,
        kind: ContainerKind::Favorites,
    }
}

fn node(
    container: ContainerId,
    parent: Option<NodeId>,
    key: &str,
    kind: NodeKind,
    title: &str,
) -> Node {
    Node {
        id: NodeId::new(),
        container_id: container,
        parent_id: parent,
        position: pos(key),
        kind,
        title: title.into(),
        url: (kind == NodeKind::Tab).then(|| format!("https://example.com/{title}")),
        state: NodeState::Suspended,
        is_unread: false,
        created_at: 1_000,
        last_active_at: 2_000,
        updated_at: 3_000,
        origin_device: None,
        rev: 0,
    }
}

fn large_batch(count: usize) -> Vec<StoreOp> {
    (0..count)
        .flat_map(|i| {
            let p = profile(&format!("bulk-{i}"), ProfileKind::Standard);
            let c = favorites_of(&p);
            [StoreOp::UpsertProfile(p), StoreOp::UpsertContainer(c)]
        })
        .collect()
}

/// A profile, a Space with a `today` container and a favorites container, a small tree, and one
/// session that selects a tab.
struct Fixture {
    profile: Profile,
    space: Space,
    favorites: Container,
    today: Container,
    folder: Node,
    tab_one: Node,
    tab_two: Node,
    split: Node,
    split_one: Node,
    nodes: Vec<Node>,
    window: WindowId,
    session: SessionState,
}

impl Fixture {
    fn new() -> Self {
        let profile = profile("work", ProfileKind::Standard);
        let space = Space {
            id: SpaceId::new(),
            profile_id: profile.id,
            name: "Work".into(),
            theme: Some("dark".into()),
            position: pos("a1"),
            agent_enabled: false,
        };
        let favorites = favorites_of(&profile);
        let today = Container {
            id: ContainerId::new(),
            profile_id: profile.id,
            space_id: Some(space.id),
            kind: ContainerKind::Today,
        };
        let folder = node(today.id, None, "a1", NodeKind::Folder, "folder");
        let tab_one = node(today.id, Some(folder.id), "a1", NodeKind::Tab, "one");
        let mut tab_two = node(today.id, Some(folder.id), "a2", NodeKind::Tab, "two");
        tab_two.is_unread = true;
        let split = node(today.id, None, "a2", NodeKind::Split, "split");
        let split_one = node(today.id, Some(split.id), "a1", NodeKind::Tab, "s1");
        let split_two = node(today.id, Some(split.id), "a2", NodeKind::Tab, "s2");
        let nodes = vec![
            folder.clone(),
            tab_one.clone(),
            tab_two.clone(),
            split.clone(),
            split_one.clone(),
            split_two.clone(),
        ];
        let window = WindowId::new();
        let session = SessionState {
            window_id: window,
            space_id: space.id,
            selected_node_id: Some(tab_one.id),
            sidebar_width: 240,
        };
        Self {
            profile,
            space,
            favorites,
            today,
            folder,
            tab_one,
            tab_two,
            split,
            split_one,
            nodes,
            window,
            session,
        }
    }

    fn batch(&self) -> Vec<StoreOp> {
        let mut ops = vec![
            StoreOp::UpsertProfile(self.profile.clone()),
            StoreOp::UpsertSpace(self.space.clone()),
            StoreOp::UpsertContainer(self.favorites.clone()),
            StoreOp::UpsertContainer(self.today.clone()),
        ];
        // Children before parents, on purpose: the batch must not depend on order.
        ops.extend(self.nodes.iter().rev().cloned().map(StoreOp::UpsertNode));
        ops.push(StoreOp::UpsertSession(self.session.clone()));
        ops
    }
}
