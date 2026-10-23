use crate::StoreOp;
use crate::actor::{StoreJob, commit_blocking, run_store};
use crate::testing::FakeStore;
use yaac_model::{Profile, ProfileId, ProfileKind};

fn profile() -> Profile {
    Profile {
        id: ProfileId::new(),
        kind: ProfileKind::Default,
        name: "Default".to_owned(),
        cache_dir: "profiles/test".to_owned(),
        created_at: 0,
    }
}

#[test]
fn the_store_thread_commits_batches_in_order_and_reports_failures() {
    let store = FakeStore::new();
    let (jobs, queue) = std::sync::mpsc::channel::<StoreJob>();
    let worker_store = store.clone();
    std::thread::spawn(move || run_store(Box::new(worker_store), queue));

    commit_blocking(&jobs, vec![StoreOp::UpsertProfile(profile())]).expect("commit succeeds");
    store.fail_next_apply();
    let failed = commit_blocking(&jobs, vec![StoreOp::UpsertProfile(profile())]);

    assert!(failed.is_err(), "the injected failure reaches the caller");
    assert_eq!(store.applied().len(), 1, "a failed batch is not recorded");
}
