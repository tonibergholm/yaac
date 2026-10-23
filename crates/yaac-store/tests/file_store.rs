//! File-level behavior of `SqliteStore`: file mode, migrations and `user_version`, restart
//! persistence, corruption detection, and the SQL rules that the trait-level suite cannot see.
//! Raw SQL goes through a second `rusqlite` connection to the same file.
//!
//! Tests marked `needs m1/model` load nodes or spaces, which calls the model.

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde::de::{IntoDeserializer, value::Error as DeError};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use yaac_core::testing::store_suite;
use yaac_core::{Store, StoreError, StoreOp};
use yaac_model::{
    Container, ContainerId, ContainerKind, Node, NodeId, NodeKind, NodeState, Position, Profile,
    ProfileId, ProfileKind, SessionState, Space, SpaceId, WindowId,
};
use yaac_store::SqliteStore;

/// A scratch directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("yaac-store-{name}-{}", ProfileId::new())))
    }

    fn file(&self) -> PathBuf {
        self.0.join("nested").join("workspace.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(path: &Path) -> SqliteStore {
    SqliteStore::open(path).expect("open store")
}

fn raw(path: &Path) -> Connection {
    Connection::open(path).expect("raw connection")
}

fn open_err(path: &Path) -> StoreError {
    match SqliteStore::open(path) {
        Ok(_) => panic!("open unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn pos(key: &str) -> Position {
    Position::deserialize(IntoDeserializer::<DeError>::into_deserializer(key))
        .expect("test position keys are valid")
}

fn profile(name: &str) -> Profile {
    Profile {
        id: ProfileId::new(),
        kind: ProfileKind::Standard,
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

fn node(container: ContainerId, parent: Option<NodeId>, key: &str, kind: NodeKind) -> Node {
    Node {
        id: NodeId::new(),
        container_id: container,
        parent_id: parent,
        position: pos(key),
        kind,
        title: format!("{kind:?}"),
        url: None,
        state: NodeState::Suspended,
        is_unread: false,
        created_at: 1_000,
        last_active_at: 2_000,
        updated_at: 3_000,
        origin_device: None,
        rev: 0,
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

#[test]
fn open_creates_private_file_and_parent_directory() {
    let dir = TempDir::new("mode");
    let path = dir.file();
    assert!(!path.parent().unwrap().exists());

    let _store = open(&path);

    assert!(
        path.parent().unwrap().is_dir(),
        "parent directory was not created"
    );
    let mode = std::fs::metadata(&path).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(mode.mode() & 0o777, 0o600, "database file mode");
}

#[test]
fn fresh_file_is_wal_at_version_one_with_only_m1_tables() {
    let dir = TempDir::new("schema");
    let path = dir.file();
    drop(open(&path));

    let conn = raw(&path);
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal_mode");
    assert_eq!(journal, "wal");
    assert_eq!(user_version(&conn), 1);

    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .expect("prepare");
    let tables: BTreeSet<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|t| t.expect("row"))
        .collect();
    let expected: BTreeSet<String> = ["meta", "profile", "space", "container", "node", "session"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        tables, expected,
        "M1 must not create later milestones' tables"
    );
}

#[test]
fn profile_and_favorites_survive_reopen_with_their_kinds() {
    let dir = TempDir::new("restart-small");
    let path = dir.file();
    let p = profile("work");
    let c = favorites_of(&p);
    {
        let mut store = open(&path);
        store
            .apply(vec![
                StoreOp::UpsertProfile(p.clone()),
                StoreOp::UpsertContainer(c.clone()),
            ])
            .expect("apply");
    }
    let mut store = open(&path);
    let ws = store.load().expect("load");
    assert_eq!(ws.profiles.get(&p.id), Some(&p));
    assert_eq!(ws.containers.get(&c.id), Some(&c));

    // Encodings are the serde snake_case names.
    let conn = raw(&path);
    let kind: String = conn
        .query_row("SELECT kind FROM profile", [], |row| row.get(0))
        .expect("profile kind");
    assert_eq!(kind, "standard");
    let kind: String = conn
        .query_row("SELECT kind FROM container", [], |row| row.get(0))
        .expect("container kind");
    assert_eq!(kind, "favorites");
}

#[test]
fn restart_persists_full_workspace() {
    let dir = TempDir::new("restart-full");
    let path = dir.file();
    let reopen_path = path.clone();
    let reopen = move || -> Box<dyn Store> { Box::new(open(&reopen_path)) };
    store_suite::check_workspace_survives_reopen(&reopen);
}

#[test]
fn rev_is_read_from_meta() {
    let dir = TempDir::new("rev-meta");
    let path = dir.file();
    drop(open(&path));
    raw(&path)
        .execute("INSERT INTO meta (key, value) VALUES ('rev', '7')", [])
        .expect("insert rev");

    let mut store = open(&path);
    assert_eq!(store.load().expect("load").rev, 7);
}

#[test]
fn rev_is_largest_of_meta_and_node_rows() {
    let dir = TempDir::new("rev-max");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let c = favorites_of(&p);
    let mut root = node(c.id, None, "a1", NodeKind::Tab);
    root.rev = 9;
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertContainer(c),
            StoreOp::UpsertNode(root),
        ])
        .expect("apply");
    raw(&path)
        .execute("INSERT INTO meta (key, value) VALUES ('rev', '3')", [])
        .expect("insert rev");
    assert_eq!(store.load().expect("load").rev, 9);
}

#[test]
fn newer_schema_version_is_refused_and_left_alone() {
    let dir = TempDir::new("newer");
    let path = dir.file();
    drop(open(&path));
    raw(&path)
        .pragma_update(None, "user_version", 2)
        .expect("set user_version");

    let error = open_err(&path);
    match &error {
        StoreError::Backend(message) => {
            assert!(message.contains("newer"), "unclear message: {message}");
        }
        other => panic!("expected Backend, got {other:?}"),
    }
    assert_eq!(
        user_version(&raw(&path)),
        2,
        "the refused file was modified"
    );
}

#[test]
fn unknown_enum_text_is_corrupt() {
    let dir = TempDir::new("bad-enum");
    let path = dir.file();
    drop(open(&path));
    let conn = raw(&path);
    conn.pragma_update(None, "ignore_check_constraints", true)
        .expect("disable checks");
    conn.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at)
         VALUES (?1, 'Standard', 'p', 'c', 1)",
        params![ProfileId::new().to_string()],
    )
    .expect("insert bad kind");
    drop(conn);

    let mut store = open(&path);
    let error = store.load().expect_err("bad enum text must fail the load");
    assert!(matches!(error, StoreError::Corrupt(_)), "{error:?}");
}

#[test]
fn non_uuid_id_is_corrupt() {
    let dir = TempDir::new("bad-id");
    let path = dir.file();
    drop(open(&path));
    raw(&path)
        .execute(
            "INSERT INTO profile (id, kind, name, cache_dir, created_at)
             VALUES ('not-a-uuid', 'default', 'p', 'c', 1)",
            [],
        )
        .expect("insert");

    let mut store = open(&path);
    let error = store.load().expect_err("bad id must fail the load");
    assert!(matches!(error, StoreError::Corrupt(_)), "{error:?}");
}

#[test]
fn dangling_parent_is_corrupt() {
    let dir = TempDir::new("orphan");
    let path = dir.file();
    drop(open(&path));
    let p = profile("p");
    let c = favorites_of(&p);
    let conn = raw(&path);
    conn.pragma_update(None, "foreign_keys", false)
        .expect("disable foreign keys");
    conn.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at)
         VALUES (?1, 'standard', 'p', 'c', 1)",
        params![p.id.to_string()],
    )
    .expect("profile");
    conn.execute(
        "INSERT INTO container (id, profile_id, space_id, kind) VALUES (?1, ?2, NULL, 'favorites')",
        params![c.id.to_string(), p.id.to_string()],
    )
    .expect("container");
    conn.execute(
        "INSERT INTO node (id, container_id, parent_id, position, kind, title, state,
                           created_at, last_active_at, updated_at, rev)
         VALUES (?1, ?2, ?3, 'a0', 'tab', 't', 'suspended', 1, 1, 1, 0)",
        params![
            NodeId::new().to_string(),
            c.id.to_string(),
            NodeId::new().to_string()
        ],
    )
    .expect("orphan node");
    drop(conn);

    let mut store = open(&path);
    let error = store
        .load()
        .expect_err("a dangling parent must fail the load");
    assert!(matches!(error, StoreError::Corrupt(_)), "{error:?}");
}

#[test]
fn stored_position_that_fails_validation_is_corrupt() {
    let dir = TempDir::new("bad-position");
    let path = dir.file();
    drop(open(&path));
    let p = profile("p");
    let c = favorites_of(&p);
    let conn = raw(&path);
    conn.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at)
         VALUES (?1, 'standard', 'p', 'c', 1)",
        params![p.id.to_string()],
    )
    .expect("profile");
    conn.execute(
        "INSERT INTO container (id, profile_id, space_id, kind) VALUES (?1, ?2, NULL, 'favorites')",
        params![c.id.to_string(), p.id.to_string()],
    )
    .expect("container");
    conn.execute(
        "INSERT INTO node (id, container_id, parent_id, position, kind, title, state,
                           created_at, last_active_at, updated_at, rev)
         VALUES (?1, ?2, NULL, '!!', 'tab', 't', 'suspended', 1, 1, 1, 0)",
        params![NodeId::new().to_string(), c.id.to_string()],
    )
    .expect("node");
    drop(conn);

    let mut store = open(&path);
    let error = store
        .load()
        .expect_err("an invalid stored position must fail the load");
    assert!(matches!(error, StoreError::Corrupt(_)), "{error:?}");
}

#[test]
fn schema_rejects_values_outside_the_checks() {
    let dir = TempDir::new("checks");
    let path = dir.file();
    drop(open(&path));
    let conn = raw(&path);
    let p = profile("p");
    conn.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at)
         VALUES (?1, 'standard', 'p', 'c', 1)",
        params![p.id.to_string()],
    )
    .expect("valid profile");

    let bad_profile_kind = conn.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at)
         VALUES (?1, 'bogus', 'p', 'c', 1)",
        params![ProfileId::new().to_string()],
    );
    assert!(bad_profile_kind.is_err(), "profile kind CHECK");

    let favorites_with_space = conn.execute(
        "INSERT INTO container (id, profile_id, space_id, kind)
         VALUES (?1, ?2, ?3, 'favorites')",
        params![
            ContainerId::new().to_string(),
            p.id.to_string(),
            SpaceId::new().to_string()
        ],
    );
    assert!(
        favorites_with_space.is_err(),
        "favorites must not have a Space"
    );

    let favorites = ContainerId::new();
    conn.execute(
        "INSERT INTO container (id, profile_id, space_id, kind) VALUES (?1, ?2, NULL, 'favorites')",
        params![favorites.to_string(), p.id.to_string()],
    )
    .expect("valid favorites");
    let negative_rev = conn.execute(
        "INSERT INTO node (id, container_id, parent_id, position, kind, title, state,
                           created_at, last_active_at, updated_at, rev)
         VALUES (?1, ?2, NULL, 'a0', 'tab', 't', 'suspended', 1, 1, 1, -1)",
        params![NodeId::new().to_string(), favorites.to_string()],
    );
    assert!(negative_rev.is_err(), "negative rev CHECK");
}

#[test]
fn delete_subtree_removes_exactly_the_listed_rows_in_either_order() {
    let p = profile("p");
    let c = favorites_of(&p);
    let folder = node(c.id, None, "a1", NodeKind::Folder);
    let t1 = node(c.id, Some(folder.id), "a1", NodeKind::Tab);
    let t2 = node(c.id, Some(folder.id), "a2", NodeKind::Tab);
    let other = node(c.id, None, "a2", NodeKind::Tab);
    let setup = vec![
        StoreOp::UpsertProfile(p.clone()),
        StoreOp::UpsertContainer(c.clone()),
        // Children first: the batch must not depend on order.
        StoreOp::UpsertNode(t2.clone()),
        StoreOp::UpsertNode(t1.clone()),
        StoreOp::UpsertNode(folder.clone()),
        StoreOp::UpsertNode(other.clone()),
    ];
    let orders = [vec![t1.id, t2.id, folder.id], vec![folder.id, t1.id, t2.id]];
    for order in orders {
        let dir = TempDir::new("subtree");
        let path = dir.file();
        let mut store = open(&path);
        store.apply(setup.clone()).expect("setup");
        store
            .apply(order.iter().map(|id| StoreOp::DeleteNode(*id)).collect())
            .expect("delete subtree");
        let conn = raw(&path);
        assert_eq!(count(&conn, "node"), 1);
        let remaining: String = conn
            .query_row("SELECT id FROM node", [], |row| row.get(0))
            .expect("remaining");
        assert_eq!(remaining, other.id.to_string());
    }
}

#[test]
fn deleting_a_parent_without_its_children_fails_the_batch() {
    let dir = TempDir::new("partial-subtree");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let c = favorites_of(&p);
    let folder = node(c.id, None, "a1", NodeKind::Folder);
    let child = node(c.id, Some(folder.id), "a1", NodeKind::Tab);
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertContainer(c),
            StoreOp::UpsertNode(folder.clone()),
            StoreOp::UpsertNode(child.clone()),
        ])
        .expect("setup");
    assert!(store.apply(vec![StoreOp::DeleteNode(folder.id)]).is_err());
    assert_eq!(
        count(&raw(&path), "node"),
        2,
        "the failed batch changed nothing"
    );
}

#[test]
fn deleting_the_selected_node_clears_the_session_selection() {
    let dir = TempDir::new("session-fallback");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let space = Space {
        id: SpaceId::new(),
        profile_id: p.id,
        name: "Work".into(),
        theme: None,
        position: pos("a1"),
        agent_enabled: false,
    };
    let today = Container {
        id: ContainerId::new(),
        profile_id: p.id,
        space_id: Some(space.id),
        kind: ContainerKind::Today,
    };
    let tab = node(today.id, None, "a1", NodeKind::Tab);
    let window = WindowId::new();
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertSpace(space.clone()),
            StoreOp::UpsertContainer(today.clone()),
            StoreOp::UpsertNode(tab.clone()),
            StoreOp::UpsertSession(SessionState {
                window_id: window,
                space_id: space.id,
                selected_node_id: Some(tab.id),
                sidebar_width: 240,
            }),
        ])
        .expect("setup");

    store
        .apply(vec![StoreOp::DeleteNode(tab.id)])
        .expect("delete selected node");

    let conn = raw(&path);
    let selected: Option<String> = conn
        .query_row(
            "SELECT selected_node_id FROM session WHERE window_id = ?1",
            params![window.to_string()],
            |row| row.get(0),
        )
        .expect("session row");
    assert_eq!(
        selected, None,
        "the selection must be cleared, not dangling"
    );
    assert_eq!(count(&conn, "session"), 1, "the session row must survive");
}

#[test]
fn updating_the_selected_node_keeps_the_session_selection() {
    // Guards against `INSERT OR REPLACE`, which deletes the row first and would fire ON DELETE SET
    // NULL on the session.
    let dir = TempDir::new("upsert-keeps-selection");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let space = Space {
        id: SpaceId::new(),
        profile_id: p.id,
        name: "Work".into(),
        theme: None,
        position: pos("a1"),
        agent_enabled: false,
    };
    let today = Container {
        id: ContainerId::new(),
        profile_id: p.id,
        space_id: Some(space.id),
        kind: ContainerKind::Today,
    };
    let mut tab = node(today.id, None, "a1", NodeKind::Tab);
    let window = WindowId::new();
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertSpace(space.clone()),
            StoreOp::UpsertContainer(today.clone()),
            StoreOp::UpsertNode(tab.clone()),
            StoreOp::UpsertSession(SessionState {
                window_id: window,
                space_id: space.id,
                selected_node_id: Some(tab.id),
                sidebar_width: 240,
            }),
        ])
        .expect("setup");

    tab.title = "renamed".into();
    store
        .apply(vec![StoreOp::UpsertNode(tab.clone())])
        .expect("update node");

    let selected: String = raw(&path)
        .query_row(
            "SELECT selected_node_id FROM session WHERE window_id = ?1",
            params![window.to_string()],
            |row| row.get(0),
        )
        .expect("selection");
    assert_eq!(selected, tab.id.to_string());
}

#[test]
fn rejected_batch_writes_nothing_by_row_count() {
    let dir = TempDir::new("reject-counts");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let c = favorites_of(&p);
    store
        .apply(vec![
            StoreOp::UpsertProfile(p.clone()),
            StoreOp::UpsertContainer(c),
        ])
        .expect("setup");
    let bad = node(ContainerId::new(), None, "a1", NodeKind::Tab);
    let later = profile("later");
    assert!(
        store
            .apply(vec![
                StoreOp::UpsertProfile(later),
                StoreOp::UpsertNode(bad),
            ])
            .is_err()
    );
    let conn = raw(&path);
    assert_eq!(count(&conn, "node"), 0);
    assert_eq!(count(&conn, "container"), 1);
    assert_eq!(
        count(&conn, "profile"),
        1,
        "the valid op in the failed batch must roll back"
    );
}

#[test]
fn container_space_profile_mismatch_is_rejected() {
    let dir = TempDir::new("profile-mismatch");
    let path = dir.file();
    let mut store = open(&path);
    let a = profile("a");
    let b = profile("b");
    let space = Space {
        id: SpaceId::new(),
        profile_id: a.id,
        name: "A".into(),
        theme: None,
        position: pos("a1"),
        agent_enabled: false,
    };
    // Container in profile B, pointing at a Space of profile A.
    let mismatched = Container {
        id: ContainerId::new(),
        profile_id: b.id,
        space_id: Some(space.id),
        kind: ContainerKind::Today,
    };
    let result = store.apply(vec![
        StoreOp::UpsertProfile(a),
        StoreOp::UpsertProfile(b),
        StoreOp::UpsertSpace(space),
        StoreOp::UpsertContainer(mismatched),
    ]);
    assert!(
        result.is_err(),
        "a container's profile must equal its Space's profile"
    );
    let conn = raw(&path);
    assert_eq!(count(&conn, "container"), 0);
    assert_eq!(count(&conn, "space"), 0);
}

#[test]
fn container_holding_nodes_is_not_deleted() {
    let dir = TempDir::new("container-delete");
    let path = dir.file();
    let mut store = open(&path);
    let p = profile("p");
    let c = favorites_of(&p);
    let root = node(c.id, None, "a1", NodeKind::Tab);
    store
        .apply(vec![
            StoreOp::UpsertProfile(p),
            StoreOp::UpsertContainer(c.clone()),
            StoreOp::UpsertNode(root),
        ])
        .expect("setup");
    assert!(store.apply(vec![StoreOp::DeleteContainer(c.id)]).is_err());
    let conn = raw(&path);
    assert_eq!(count(&conn, "container"), 1);
    assert_eq!(count(&conn, "node"), 1);
}
