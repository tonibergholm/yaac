//! Reads every table into a `Workspace`. Anything that does not validate is `Corrupt`.

use crate::codec::{decode_enum, flag, parse_id, parse_position, to_u32};
use crate::{backend, corrupt};
use rusqlite::{Connection, OptionalExtension, Row};
use yaac_core::StoreError;
use yaac_model::{
    Container, ContainerKind, Node, Profile, SessionState, Space, SpaceId, TreeState, Workspace,
};

pub(crate) fn load(conn: &Connection) -> Result<Workspace, StoreError> {
    check_references(conn)?;

    let mut workspace = Workspace::default();
    for profile in read_all(conn, PROFILES, profile_row)? {
        workspace.profiles.insert(profile.id, profile);
    }
    for space in read_all(conn, SPACES, space_row)? {
        workspace.spaces.insert(space.id, space);
    }
    for container in read_all(conn, CONTAINERS, container_row)? {
        workspace.containers.insert(container.id, container);
    }
    let nodes = read_all(conn, NODES, node_row)?;
    workspace.tree = build_tree(nodes)?;
    for session in read_all(conn, SESSIONS, session_row)? {
        workspace.sessions.insert(session.window_id, session);
    }
    workspace.rev = revision(conn)?;
    Ok(workspace)
}

/// The one call into the model's tree constructor. An empty store loads without it.
fn build_tree(nodes: Vec<Node>) -> Result<TreeState, StoreError> {
    if nodes.is_empty() {
        return Ok(TreeState::new());
    }
    TreeState::from_nodes(nodes).map_err(corrupt)
}

/// `Workspace.rev` is the larger of the `meta` row (written by `StoreOp::SetRev`) and the largest
/// node `rev`.
fn revision(conn: &Connection) -> Result<u64, StoreError> {
    let node_max: Option<i64> = conn
        .query_row("SELECT MAX(rev) FROM node", [], |row| row.get(0))
        .map_err(backend)?;
    let meta: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'rev'", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(backend)?;

    let from_meta = match meta {
        Some(text) => text
            .parse::<u64>()
            .map_err(|e| corrupt(format!("meta rev {text:?}: {e}")))?,
        None => 0,
    };
    let from_nodes = match node_max {
        Some(rev) => {
            u64::try_from(rev).map_err(|_| corrupt(format!("node rev {rev} is negative")))?
        }
        None => 0,
    };
    Ok(from_meta.max(from_nodes))
}

/// Every foreign key must resolve. A dangling reference means the file was edited outside the
/// store, because the schema makes one impossible when foreign keys are on.
fn check_references(conn: &Connection) -> Result<(), StoreError> {
    let mut statement = conn.prepare("PRAGMA foreign_key_check").map_err(backend)?;
    let mut rows = statement.query([]).map_err(backend)?;
    if let Some(row) = rows.next().map_err(backend)? {
        let table: String = row.get(0).map_err(corrupt)?;
        let parent: String = row.get(2).map_err(corrupt)?;
        return Err(corrupt(format!(
            "table {table} has a reference to a missing row in {parent}"
        )));
    }
    Ok(())
}

fn read_all<T>(
    conn: &Connection,
    sql: &str,
    map: impl Fn(&Row<'_>) -> Result<T, StoreError>,
) -> Result<Vec<T>, StoreError> {
    let mut statement = conn.prepare(sql).map_err(backend)?;
    let mut rows = statement.query([]).map_err(backend)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        out.push(map(row)?);
    }
    Ok(out)
}

fn text(row: &Row<'_>, index: usize) -> Result<String, StoreError> {
    row.get(index).map_err(corrupt)
}

fn optional_text(row: &Row<'_>, index: usize) -> Result<Option<String>, StoreError> {
    row.get(index).map_err(corrupt)
}

fn integer(row: &Row<'_>, index: usize) -> Result<i64, StoreError> {
    row.get(index).map_err(corrupt)
}

const PROFILES: &str = "SELECT id, kind, name, cache_dir, created_at FROM profile";

fn profile_row(row: &Row<'_>) -> Result<Profile, StoreError> {
    Ok(Profile {
        id: parse_id(&text(row, 0)?)?,
        kind: decode_enum(&text(row, 1)?)?,
        name: text(row, 2)?,
        cache_dir: text(row, 3)?,
        created_at: integer(row, 4)?,
    })
}

const SPACES: &str = "SELECT id, profile_id, name, theme, position, agent_enabled FROM space";

fn space_row(row: &Row<'_>) -> Result<Space, StoreError> {
    Ok(Space {
        id: parse_id(&text(row, 0)?)?,
        profile_id: parse_id(&text(row, 1)?)?,
        name: text(row, 2)?,
        theme: optional_text(row, 3)?,
        position: parse_position(&text(row, 4)?)?,
        agent_enabled: flag(integer(row, 5)?)?,
    })
}

const CONTAINERS: &str = "SELECT id, profile_id, space_id, kind FROM container";

fn container_row(row: &Row<'_>) -> Result<Container, StoreError> {
    let space_id: Option<SpaceId> = optional_text(row, 2)?
        .map(|raw| parse_id(&raw))
        .transpose()?;
    let kind: ContainerKind = decode_enum(&text(row, 3)?)?;
    // The schema has the same CHECK; repeated here because a file edited outside the store can
    // bypass it.
    if (kind == ContainerKind::Favorites) != space_id.is_none() {
        return Err(corrupt(format!(
            "container of kind {kind:?} has space_id {space_id:?}"
        )));
    }
    Ok(Container {
        id: parse_id(&text(row, 0)?)?,
        profile_id: parse_id(&text(row, 1)?)?,
        space_id,
        kind,
    })
}

const NODES: &str = "SELECT id, container_id, parent_id, position, kind, title, url, state, \
     is_unread, created_at, last_active_at, updated_at, origin_device, rev FROM node";

fn node_row(row: &Row<'_>) -> Result<Node, StoreError> {
    Ok(Node {
        id: parse_id(&text(row, 0)?)?,
        container_id: parse_id(&text(row, 1)?)?,
        parent_id: optional_text(row, 2)?
            .map(|raw| parse_id(&raw))
            .transpose()?,
        position: parse_position(&text(row, 3)?)?,
        kind: decode_enum(&text(row, 4)?)?,
        title: text(row, 5)?,
        url: optional_text(row, 6)?,
        state: decode_enum(&text(row, 7)?)?,
        is_unread: flag(integer(row, 8)?)?,
        created_at: integer(row, 9)?,
        last_active_at: integer(row, 10)?,
        updated_at: integer(row, 11)?,
        origin_device: optional_text(row, 12)?,
        rev: integer(row, 13)?,
    })
}

const SESSIONS: &str = "SELECT window_id, space_id, selected_node_id, sidebar_width FROM session";

fn session_row(row: &Row<'_>) -> Result<SessionState, StoreError> {
    Ok(SessionState {
        window_id: parse_id(&text(row, 0)?)?,
        space_id: parse_id(&text(row, 1)?)?,
        selected_node_id: optional_text(row, 2)?
            .map(|raw| parse_id(&raw))
            .transpose()?,
        sidebar_width: to_u32(integer(row, 3)?)?,
    })
}
