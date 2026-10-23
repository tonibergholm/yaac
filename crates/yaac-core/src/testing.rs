//! Test support, behind the `testing` feature: `FakeStore` and (written by the store work) a
//! `Store` conformance suite that `yaac-store` runs against its SQLite implementation.

pub mod store_suite;

use crate::{Store, StoreError, StoreOp};
use std::sync::{Arc, Mutex};
use yaac_model::Workspace;

#[derive(Debug, Default)]
struct Inner {
    initial: Workspace,
    applied: Vec<Vec<StoreOp>>,
    fail_next_apply: bool,
}

/// An in-memory `Store` that records the batches it receives. Clone it to inspect it from a test
/// after handing a clone to the core.
#[derive(Debug, Default, Clone)]
pub struct FakeStore {
    inner: Arc<Mutex<Inner>>,
}

impl FakeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_workspace(workspace: Workspace) -> Self {
        let store = Self::default();
        store.inner.lock().unwrap().initial = workspace;
        store
    }

    pub fn applied(&self) -> Vec<Vec<StoreOp>> {
        self.inner.lock().unwrap().applied.clone()
    }

    pub fn fail_next_apply(&self) {
        self.inner.lock().unwrap().fail_next_apply = true;
    }
}

impl Store for FakeStore {
    fn load(&mut self) -> Result<Workspace, StoreError> {
        Ok(self.inner.lock().unwrap().initial.clone())
    }

    fn apply(&mut self, batch: Vec<StoreOp>) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().unwrap();
        if std::mem::take(&mut inner.fail_next_apply) {
            return Err(StoreError::Backend("injected failure".into()));
        }
        inner.applied.push(batch);
        Ok(())
    }
}
