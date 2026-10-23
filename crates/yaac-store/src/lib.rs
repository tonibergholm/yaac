//! SQLite persistence: schema, numbered migrations, and the `Store` implementation.
//!
//! One connection, one writer. Each batch runs in one `BEGIN IMMEDIATE` transaction. Foreign keys
//! are deferred to commit, so statement order inside a batch does not matter.

mod apply;
mod codec;
mod load;
mod schema;

use rusqlite::Connection;
use std::{fmt::Display, fs::OpenOptions, path::Path, time::Duration};
use yaac_core::{Store, StoreError, StoreOp};
use yaac_model::Workspace;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens (creating, and migrating if needed) the database at `path`. The file is created with
    /// mode 0600 and its parent directory if missing. An existing file keeps its mode.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(backend)?;
        }
        create_private_file(path)?;
        let conn = Connection::open(path).map_err(backend)?;
        Self::setup(conn, true)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::setup(conn, false)
    }

    fn setup(mut conn: Connection, on_disk: bool) -> Result<Self, StoreError> {
        conn.busy_timeout(BUSY_TIMEOUT).map_err(backend)?;
        // Per connection, and a no-op inside a transaction, so it must come first.
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(backend)?;
        if on_disk {
            let mode: String = conn
                .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
                .map_err(backend)?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(backend(format!("could not enable WAL mode (got {mode})")));
            }
            // FULL, not NORMAL: a commit must survive power loss before its engine effect runs
            // (architecture section 4). Commands are user-paced, so the extra fsync is cheap.
            conn.pragma_update(None, "synchronous", "FULL")
                .map_err(backend)?;
        }
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }
}

impl Store for SqliteStore {
    fn load(&mut self) -> Result<Workspace, StoreError> {
        load::load(&self.conn)
    }

    fn apply(&mut self, batch: Vec<StoreOp>) -> Result<(), StoreError> {
        apply::apply(&mut self.conn, &batch)
    }
}

/// Creates the file with mode 0600 if it does not exist. `append` never truncates an existing file.
fn create_private_file(path: &Path) -> Result<(), StoreError> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map(drop).map_err(backend)
}

pub(crate) fn backend(error: impl Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

pub(crate) fn corrupt(error: impl Display) -> StoreError {
    StoreError::Corrupt(error.to_string())
}
