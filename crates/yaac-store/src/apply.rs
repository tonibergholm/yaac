//! Writes one batch in one transaction. Every upsert is `INSERT ... ON CONFLICT DO UPDATE`, never
//! `INSERT OR REPLACE`: REPLACE deletes the old row first, which would fire `ON DELETE SET NULL` on
//! `session.selected_node_id` for every update of the selected node.
//!
//! Rules enforced by the schema. The foreign-key rules are checked at commit, so their order within
//! a batch does not matter:
//! - a node's parent must exist and sit in the same container (a missing parent or a container
//!   mismatch fails the batch);
//! - a container's profile must equal its Space's profile;
//! - a container that still holds nodes cannot be deleted, so a removed subtree must be listed
//!   completely.
//!
//! `ON DELETE SET NULL` on `session.selected_node_id` is not deferred. It runs when the node row is
//! deleted, so a batch that deletes a node and later upserts a session that selects it fails at
//! commit, while the reverse order commits. A batch that deletes and re-creates the same node id
//! clears the selection. The core must not do that.
//!
//! Tree shape (parent kind, split arity, cycles) is not checked here. `yaac-model` enforces it, and
//! `load` rejects a stored tree that breaks it.

use crate::backend;
use crate::codec::{container_kind_text, node_kind_text, node_state_text, profile_kind_text};
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use yaac_core::{StoreError, StoreOp};
use yaac_model::{Container, Node, Profile, SessionState, Space};

pub(crate) fn apply(conn: &mut Connection, batch: &[StoreOp]) -> Result<(), StoreError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    for op in batch {
        apply_op(&tx, op)?;
    }
    // Deferred foreign keys are checked here. A violation rolls back the whole transaction when
    // `tx` is dropped. `StoreError` has no batch-rejected variant, so this is `Backend`.
    tx.commit()
        .map_err(|e| backend(format!("batch rejected: {e}")))
}

fn apply_op(tx: &Transaction<'_>, op: &StoreOp) -> Result<(), StoreError> {
    let result = match op {
        StoreOp::UpsertProfile(profile) => upsert_profile(tx, profile),
        StoreOp::UpsertSpace(space) => upsert_space(tx, space),
        StoreOp::UpsertContainer(container) => upsert_container(tx, container),
        StoreOp::UpsertNode(node) => upsert_node(tx, node),
        StoreOp::DeleteNode(id) => tx.execute("DELETE FROM node WHERE id = ?1", [id.to_string()]),
        StoreOp::DeleteContainer(id) => {
            tx.execute("DELETE FROM container WHERE id = ?1", [id.to_string()])
        }
        StoreOp::UpsertSession(session) => upsert_session(tx, session),
        StoreOp::DeleteSession(id) => {
            tx.execute("DELETE FROM session WHERE window_id = ?1", [id.to_string()])
        }
    };
    result.map(drop).map_err(backend)
}

fn upsert_profile(tx: &Transaction<'_>, profile: &Profile) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO profile (id, kind, name, cache_dir, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, name = excluded.name,
             cache_dir = excluded.cache_dir, created_at = excluded.created_at",
        params![
            profile.id.to_string(),
            profile_kind_text(profile.kind),
            profile.name,
            profile.cache_dir,
            profile.created_at,
        ],
    )
}

fn upsert_space(tx: &Transaction<'_>, space: &Space) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO space (id, profile_id, name, theme, position, agent_enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET profile_id = excluded.profile_id, name = excluded.name,
             theme = excluded.theme, position = excluded.position,
             agent_enabled = excluded.agent_enabled",
        params![
            space.id.to_string(),
            space.profile_id.to_string(),
            space.name,
            space.theme,
            space.position.as_str(),
            i64::from(space.agent_enabled),
        ],
    )
}

fn upsert_container(tx: &Transaction<'_>, container: &Container) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO container (id, profile_id, space_id, kind) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET profile_id = excluded.profile_id,
             space_id = excluded.space_id, kind = excluded.kind",
        params![
            container.id.to_string(),
            container.profile_id.to_string(),
            container.space_id.map(|id| id.to_string()),
            container_kind_text(container.kind),
        ],
    )
}

fn upsert_node(tx: &Transaction<'_>, node: &Node) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO node (id, container_id, parent_id, position, kind, title, url, state,
                           is_unread, created_at, last_active_at, updated_at, origin_device, rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (id) DO UPDATE SET container_id = excluded.container_id,
             parent_id = excluded.parent_id, position = excluded.position,
             kind = excluded.kind, title = excluded.title, url = excluded.url,
             state = excluded.state, is_unread = excluded.is_unread,
             created_at = excluded.created_at, last_active_at = excluded.last_active_at,
             updated_at = excluded.updated_at, origin_device = excluded.origin_device,
             rev = excluded.rev",
        params![
            node.id.to_string(),
            node.container_id.to_string(),
            node.parent_id.map(|id| id.to_string()),
            node.position.as_str(),
            node_kind_text(node.kind),
            node.title,
            node.url,
            node_state_text(node.state),
            i64::from(node.is_unread),
            node.created_at,
            node.last_active_at,
            node.updated_at,
            node.origin_device,
            node.rev,
        ],
    )
}

fn upsert_session(tx: &Transaction<'_>, session: &SessionState) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO session (window_id, space_id, selected_node_id, sidebar_width)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (window_id) DO UPDATE SET space_id = excluded.space_id,
             selected_node_id = excluded.selected_node_id,
             sidebar_width = excluded.sidebar_width",
        params![
            session.window_id.to_string(),
            session.space_id.to_string(),
            session.selected_node_id.map(|id| id.to_string()),
            i64::from(session.sidebar_width),
        ],
    )
}
