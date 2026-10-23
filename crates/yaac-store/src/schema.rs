//! Numbered migrations. Each file runs in its own transaction, and `PRAGMA user_version` is set in
//! that same transaction, so a failed migration leaves the version unchanged.

use crate::{backend, corrupt};
use rusqlite::{Connection, TransactionBehavior};
use yaac_core::StoreError;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_m1_core.sql")];

/// Refuses a file written by a newer build. Runs before anything is written to the file, including
/// the switch to WAL mode.
pub(crate) fn refuse_newer(conn: &Connection) -> Result<(), StoreError> {
    let version = user_version(conn)?;
    check_version(version).map(drop)
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    loop {
        // IMMEDIATE takes the write lock before the version is read, so two processes cannot both
        // run the same migration.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let version = user_version(&tx)?;
        let applied = check_version(version)?;
        if applied == MIGRATIONS.len() {
            return Ok(());
        }
        tx.execute_batch(MIGRATIONS[applied]).map_err(backend)?;
        tx.pragma_update(None, "user_version", version + 1)
            .map_err(backend)?;
        tx.commit().map_err(backend)?;
    }
}

fn user_version(conn: &Connection) -> Result<i64, StoreError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(backend)
}

/// Returns the number of migrations already applied, or an error when the file is not one this
/// build can open.
fn check_version(version: i64) -> Result<usize, StoreError> {
    let latest = MIGRATIONS.len();
    let applied = usize::try_from(version)
        .map_err(|_| corrupt(format!("negative schema version {version}")))?;
    if applied > latest {
        return Err(backend(format!(
            "database schema version {version} is newer than this build supports \
             (version {latest}); open it with a newer yaac"
        )));
    }
    Ok(applied)
}
