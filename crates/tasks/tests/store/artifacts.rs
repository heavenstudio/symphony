use crate::common::{Fixture, HOST, NOW, Seed, database};
use serde_json::json;
use std::sync::Barrier;
use symphony_board::{Error, NewTask, Opening, Schema, Store};

#[test]
fn artifact_schema_keeps_old_reads_and_fences_older_writers() {
    let database = database();
    let previous = Schema::this_build().before();
    let open = |how| {
        Store::opening_on(
            database.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            &how,
        )
        .unwrap()
    };
    let older = open(Opening::write().at(previous.clone()));
    older
        .create(&NewTask::new("legacy/task", "Legacy task", "owner"))
        .unwrap();
    let current = open(Opening::write());
    assert!(
        current
            .artifact_ledger_cas("tenant", "project", 0, &json!({"format": 1}))
            .unwrap()
    );
    let reader = open(Opening::read().at(previous));
    assert_eq!(older.get("legacy/task").unwrap().title, "Legacy task");
    assert_eq!(reader.get("legacy/task").unwrap().title, "Legacy task");
    for store in [&older, &reader] {
        assert!(matches!(
            store.artifact_ledger_cas("tenant", "project", 1, &json!({"changed": true})),
            Err(Error::Refused(_))
        ));
    }
    assert_eq!(
        current.artifact_ledger("tenant", "project").unwrap(),
        Some(json!({"revision": 1, "body": {"format": 1}}))
    );
}

#[test]
fn artifact_ledger_is_scoped_durable_and_revisions_never_overwrite() {
    let f = Fixture::new(Seed::default());
    assert_eq!(f.store.artifact_ledger("tenant", "project").unwrap(), None);
    let body = json!({"format": 1, "intent": "pending", "roots": [], "nested": {"optional": null}, "array": [null, 1]});
    assert!(
        f.store
            .artifact_ledger_cas("tenant", "project", 0, &body)
            .unwrap()
    );
    assert!(
        !f.store
            .artifact_ledger_cas("tenant", "project", 0, &json!({"lost": true}))
            .unwrap()
    );
    assert_eq!(
        f.other().artifact_ledger("tenant", "project").unwrap(),
        Some(json!({"revision": 1, "body": body}))
    );
    assert_eq!(
        f.store.artifact_ledger("tenant-other", "project").unwrap(),
        None
    );
    assert_eq!(
        f.store.artifact_ledger("tenant", "project-other").unwrap(),
        None
    );
    assert!(
        !f.store
            .artifact_ledger_cas("tenant", "project", 2, &body)
            .unwrap()
    );
    let next = json!({"format": 1, "intent": "available", "roots": ["task", "backup"]});
    assert!(
        f.other()
            .artifact_ledger_cas("tenant", "project", 1, &next)
            .unwrap()
    );
    assert!(
        !f.store
            .artifact_ledger_cas("tenant", "project", 1, &body)
            .unwrap()
    );
    assert_eq!(
        f.store.artifact_ledger("tenant", "project").unwrap(),
        Some(json!({"revision": 2, "body": next}))
    );
}

#[test]
fn concurrent_artifact_cas_has_one_winner_for_creation_and_replacement() {
    let f = Fixture::new(Seed::default());
    for expected in 0..2 {
        let first = f.other();
        let second = f.other();
        let barrier = Barrier::new(2);
        let (a, b) = std::thread::scope(|threads| {
            let a = threads.spawn(|| {
                barrier.wait();
                first
                    .artifact_ledger_cas("tenant", "project", expected, &json!({"winner": "a"}))
                    .unwrap()
            });
            let b = threads.spawn(|| {
                barrier.wait();
                second
                    .artifact_ledger_cas("tenant", "project", expected, &json!({"winner": "b"}))
                    .unwrap()
            });
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_ne!(a, b);
        assert_eq!(
            f.store
                .artifact_ledger("tenant", "project")
                .unwrap()
                .unwrap()["revision"],
            expected + 1
        );
    }
}

#[test]
fn artifact_scope_components_and_untrusted_parameters_are_not_query_text() {
    let f = Fixture::new(Seed::default());
    let hostile = "quote'; RETURN true; --";
    assert!(
        f.store
            .artifact_ledger_cas(hostile, "a/b", 0, &json!({"safe": true}))
            .unwrap()
    );
    assert_eq!(
        f.store.artifact_ledger(hostile, "a/b").unwrap().unwrap()["body"],
        json!({"safe": true})
    );
    assert_eq!(f.store.artifact_ledger("quote", "a/b").unwrap(), None);
    assert!(
        f.store
            .artifact_ledger_cas("a/b", "c", 0, &json!({"one": true}))
            .unwrap()
    );
    assert_eq!(f.store.artifact_ledger("a", "b/c").unwrap(), None);
    assert!(f.store.artifact_ledger("", "project").is_err());
    assert!(f.store.artifact_ledger("tenant", "\n").is_err());
    assert!(
        f.store
            .artifact_ledger_cas("tenant", "project", u64::MAX, &json!({}))
            .is_err()
    );
    assert!(
        f.store
            .artifact_ledger_cas("tenant", "project", 0, &json!([]))
            .is_err()
    );
}
