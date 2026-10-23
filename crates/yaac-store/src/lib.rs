//! SQLite persistence: schema, numbered migrations, and the `Store` implementation.

use std::path::Path;
use yaac_core::{Store, StoreError, StoreOp};
use yaac_model::Workspace;

pub struct SqliteStore {
    _private: (),
}

impl SqliteStore {
    /// Opens (creating and migrating if needed) the database at `path`. WAL mode, one writer.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let _ = path;
        todo!("m1/store")
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        todo!("m1/store")
    }
}

impl Store for SqliteStore {
    fn load(&mut self) -> Result<Workspace, StoreError> {
        todo!("m1/store")
    }

    fn apply(&mut self, batch: Vec<StoreOp>) -> Result<(), StoreError> {
        let _ = batch;
        todo!("m1/store")
    }
}
