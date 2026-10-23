//! Numbered migrations. Each file runs in its own transaction, and `PRAGMA user_version` is set in
//! that same transaction, so a failed migration leaves the version unchanged.

use crate::backend;
use rusqlite::{Connection, TransactionBehavior};
use yaac_core::StoreError;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_m1_core.sql")];

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    loop {
        // IMMEDIATE takes the write lock before the version is read, so two processes cannot both
        // run the same migration.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let version: i64 = tx
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(backend)?;
        let latest = MIGRATIONS.len();
        match usize::try_from(version) {
            Ok(applied) if applied == latest => return Ok(()),
            Ok(applied) if applied < latest => {
                tx.execute_batch(MIGRATIONS[applied]).map_err(backend)?;
                tx.pragma_update(None, "user_version", version + 1)
                    .map_err(backend)?;
                tx.commit().map_err(backend)?;
            }
            _ => {
                return Err(backend(format!(
                    "database schema version {version} is newer than this build supports \
                     (version {latest}); open it with a newer yaac"
                )));
            }
        }
    }
}
