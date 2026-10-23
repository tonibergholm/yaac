//! Runs the `yaac-core` `Store` conformance suite against `SqliteStore` (in memory) and, for the
//! checks that apply to it, against `FakeStore`. Tests marked `needs m1/model` call into
//! `TreeState::from_nodes` or `Position::from_raw` and panic until the model lands.

use yaac_core::Store;
use yaac_core::testing::{FakeStore, store_suite};
use yaac_store::SqliteStore;

fn sqlite() -> Box<dyn Store> {
    Box::new(SqliteStore::open_in_memory().expect("open in-memory store"))
}

fn fake() -> Box<dyn Store> {
    Box::new(FakeStore::new())
}

#[test]
fn fake_store_passes_common_checks() {
    store_suite::run_common(&fake);
}

#[test]
fn sqlite_empty_store_loads_empty() {
    store_suite::check_empty_store_loads_empty(&sqlite);
}

#[test]
fn sqlite_large_batch_applies() {
    store_suite::check_large_batch_applies(&sqlite);
}

#[test]
fn sqlite_large_batch_round_trips() {
    store_suite::check_large_batch_round_trips(&sqlite);
}

#[test]
fn sqlite_profiles_and_containers_round_trip() {
    store_suite::check_profiles_and_containers_round_trip(&sqlite);
}

#[test]
fn sqlite_upsert_overwrites() {
    store_suite::check_upsert_overwrites(&sqlite);
}

#[test]
fn sqlite_failed_batch_leaves_nothing() {
    store_suite::check_failed_batch_leaves_nothing(&sqlite);
}

#[test]
fn sqlite_child_in_other_container_is_rejected() {
    store_suite::check_child_in_other_container_is_rejected(&sqlite);
}

#[test]
#[ignore = "needs m1/model"]
fn sqlite_container_with_nodes_cannot_be_deleted() {
    store_suite::check_container_with_nodes_cannot_be_deleted(&sqlite);
}

#[test]
#[ignore = "needs m1/model"]
fn sqlite_full_workspace_round_trips() {
    store_suite::check_full_workspace_round_trips(&sqlite);
}

#[test]
#[ignore = "needs m1/model"]
fn sqlite_delete_subtree_in_either_order() {
    store_suite::check_delete_subtree_in_either_order(&sqlite);
}

#[test]
#[ignore = "needs m1/model"]
fn sqlite_session_falls_back_when_selected_node_deleted() {
    store_suite::check_session_falls_back_when_selected_node_deleted(&sqlite);
}

#[test]
#[ignore = "needs m1/model"]
fn sqlite_run_all() {
    store_suite::run_all(&sqlite);
}
