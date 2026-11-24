//! Consumer acceptance on the delivered held-aware parent-feature observation.

use super::*;
use crate::common::{HOST, NOW};
use std::collections::BTreeSet;
use symphony_board::{Error, Health, NewLaunch};

fn held(f: &Fixture, orch: &str, worker: &str, slug: &str) {
    start(f, orch, worker, slug);
    let store = f.elsewhere();
    store.move_to(slug, "review", worker).unwrap();
    store.hold_blocked(slug, worker).unwrap();
}

#[test]
fn a_held_only_count_change_reaches_the_account_without_a_board_marker_or_ready_change() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    for n in 0..4 {
        held(&f, &orch, &worker, &format!("held/{n}"));
    }
    let listener = listen(&f, &orch);
    assert!(!listener.text().contains("low_volume "));
    let marker = || vec![json!(f.store.marker().unwrap())];
    let before = marker();
    let board = f.store.board().unwrap();
    // End this hold's validity without changing any ordinary PULL or ownership input.
    (f.store.raw().row("counter", json!(["hold", "held/3"])))
        .add("ts", 1)
        .unwrap();
    assert_eq!(marker(), before);
    assert_eq!(f.store.board().unwrap(), board);
    f.store.dispatch_callbacks().unwrap();
    let told = listener.drain(&f, &worker, &orch);
    assert_eq!(
        warnings(&told),
        [format!(
            "low_volume {orch} open=3 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
        )]
    );
    f.store.dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &worker, &orch)).is_empty());
    (f.store.raw().row("counter", json!(["hold", "held/3"])))
        .add("ts", -1)
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &worker, &orch)).is_empty());
    (f.store.raw().row("counter", json!(["hold", "held/3"])))
        .add("ts", 1)
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(warnings(&listener.drain(&f, &worker, &orch)).len(), 1);
}

#[test]
fn an_observation_pass_cannot_hide_a_hold_only_crossing_behind_its_sequence_gate() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    for n in 0..4 {
        held(&f, &orch, &worker, &format!("held/{n}"));
    }
    let deferred = f.other().dispatching_by(Box::new(|| true));
    deferred
        .owner_callback("volume-sentinel", "Retained")
        .unwrap();
    let listener = listen(&f, &orch);
    assert!(!listener.text().contains("low_volume "));
    // Bookkeeping writes moved the marker without changing the observed Board or count.
    // Establish the equivalent current-marker checkpoint a concurrent observer can leave.
    let marker = || vec![json!(f.store.marker().unwrap())];
    let before = marker();
    let sequence = before[0]
        .as_u64()
        .expect("the canonical marker is unsigned");
    (f.store.raw().row("callback_state", orch.as_str()))
        .set(json!({"snapshot_sequence": sequence}))
        .unwrap();
    assert_eq!(
        (f.store.raw().row("callback_state", orch.as_str()))
            .field("snapshot_sequence")
            .unwrap(),
        before[0]
    );
    (f.store.raw().row("counter", json!(["hold", "held/3"])))
        .add("ts", 1)
        .unwrap();
    deferred
        .owner_callback("volume-sentinel", "Retained")
        .unwrap();
    assert_eq!(
        marker(),
        before,
        "an existing callback key changes no board input"
    );
    let pending = deferred.callback_inbox(&orch).unwrap();
    assert!(
        pending
            .iter()
            .any(|(_, text)| text.contains("open=3 threshold=4")),
        "{pending:?}"
    );
    // A fresh observer must take the unchanged-count sequence shortcut without replacing it.
    let repeat = f.other().dispatching_by(Box::new(|| true));
    repeat
        .owner_callback("volume-sentinel", "Retained")
        .unwrap();
    assert_eq!(repeat.callback_inbox(&orch).unwrap(), pending);
    // Deliver through its idle cache, including the pending event left by the first observer.
    repeat.dispatch_callbacks().unwrap();
    assert_eq!(warnings(&listener.drain(&f, &worker, &orch)).len(), 1);
    repeat.dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &worker, &orch)).is_empty());
}

#[test]
fn nested_branch_mirrors_reds_and_finished_work_use_the_canonical_feature_keys() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    let filer = f.session(symphony_board::red::FILER);
    for slug in ["feature/a", "feature/b", "feature/c"] {
        start(&f, &orch, &worker, slug);
    }
    let store = f.elsewhere();
    for (slug, parent) in [("part", "feature/a"), ("nested", "part")] {
        store
            .create(&NewTask {
                parent: Some(parent.into()),
                ..NewTask::new(slug, "Part", &worker)
            })
            .unwrap();
        store.claim(slug, &worker).unwrap();
    }
    store
        .record_launch(&NewLaunch {
            branch: "alias".into(),
            ..NewLaunch::new("feature/a", &worker, "executor")
        })
        .unwrap();
    for slug in ["review/alias", "review-2/alias"] {
        start(&f, &orch, &worker, slug);
        store.move_to(slug, "review", &worker).unwrap();
    }
    start(&f, &orch, &worker, "finished");
    store
        .landed("finished", &orch, "Complete", false, &["in_progress"])
        .unwrap();
    start(&f, &orch, &worker, "cancelled");
    store.move_to("cancelled", "cancelled", &worker).unwrap();
    for (slug, parent) in [
        ("tests/main-red-volume", None),
        ("tests/main-red-nested", Some("feature/a")),
    ] {
        store
            .create(&NewTask {
                red: Some("main".into()),
                parent: parent.map(str::to_owned),
                ..NewTask::new(slug, "Red", &filer)
            })
            .unwrap();
        store.take_role(slug, &orch, "orchestrator").unwrap();
        store.claim(slug, &worker).unwrap();
    }
    let expected = BTreeSet::from(["feature/a".into(), "feature/b".into(), "feature/c".into()]);
    assert_eq!(f.store.slot_holdings().unwrap()[&orch], expected);
    let capped = f.other().capping(Some(8));
    let board = f.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&capped, &[], None, f.now(), &[])
        .unwrap()
        .only_for(&board, &orch);
    assert_eq!(digest.slots, Some(3));
    assert!(
        digest
            .lines()
            .iter()
            .any(|line| line.contains(&format!("low_volume {orch} open=3 threshold=4 cap=8")))
    );
    held(&f, &orch, &worker, "fourth");
    let board = f.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&capped, &[], None, f.now(), &[])
        .unwrap()
        .only_for(&board, &orch);
    assert_eq!(digest.slots, Some(4));
    assert!(!digest.entries().contains(&("low_volume".into(), orch)));
}

#[test]
fn unknown_owners_keep_valid_blocked_holds_in_consumer_counts() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    for n in 0..4 {
        held(&f, &orch, &worker, &format!("held/{n}"));
    }
    (f.store.raw().row("callback_state", orch.as_str()))
        .put(json!({
            "handle": orch,
            "address": "codex:abcdef01",
            "host": HOST,
            "confirmed_until": NOW - 1.0,
            "lease": NOW + 1000.0,
        }))
        .unwrap();
    (f.store.raw().row("session", orch.as_str()))
        .set(json!({"last_seen": NOW - 500.0}))
        .unwrap();
    assert_eq!(f.health(&orch), Health::Unknown);
    let capped = f.other().capping(Some(8));
    let board = f.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&capped, &[], None, f.now(), &[])
        .unwrap()
        .only_for(&board, &orch);
    assert_eq!(digest.slots, Some(4));
    assert!(
        !digest
            .entries()
            .contains(&("low_volume".into(), orch.clone()))
    );
    let observed = symphony_board::cli::attach::observe(&f.store, &orch, "orchestrator").unwrap();
    (f.store.raw().row("counter", json!(["hold", "held/3"])))
        .add("ts", 1)
        .unwrap();
    let later = symphony_board::cli::attach::observe(&f.store, &orch, "orchestrator").unwrap();
    assert_ne!(observed.staffing, later.staffing);
    assert_eq!(
        (observed.roles, observed.statuses, observed.ready),
        (later.roles, later.statuses, later.ready)
    );
    let digest = symphony_board::cli::pull::digest(&capped, &[], None, f.now(), &[])
        .unwrap()
        .only_for(&board, &orch);
    assert_eq!(digest.slots, Some(3));
    assert!(
        digest
            .lines()
            .iter()
            .any(|line| line.contains(&format!("low_volume {orch} open=3 threshold=4 cap=8")))
    );
}

#[test]
fn failed_holdings_are_errors_and_do_not_clear_the_delivered_account() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    for n in 0..4 {
        held(&f, &orch, &worker, &format!("held/{n}"));
    }
    let listener = listen(&f, &orch);
    assert!(!listener.text().contains("low_volume "));
    let receipt = || {
        (f.store.raw().row("callback_state", orch.as_str()))
            .fields(&["pull_entries", "pull_withheld", "pull_awaiting"])
            .unwrap()
    };
    let before = receipt();
    assert_eq!(before["pull_entries"], json!([]));
    // A malformed private counter key fails only the canonical slot read, not Board.
    (f.store.raw().row("counter", json!(["slots", 7])))
        .create(json!({"last": 0}))
        .unwrap();
    assert!(f.store.board().is_ok());
    assert!(matches!(f.store.slot_holdings(), Err(Error::Db(_))));
    assert!(symphony_board::cli::pull::digest(&f.store, &[], None, f.now(), &[]).is_err());
    assert!(symphony_board::cli::attach::observe(&f.store, &orch, "orchestrator").is_err());
    assert!(symphony_board::cli::attach::observe(&f.store, &worker, "worker").is_ok());
    assert!(f.store.dispatch_callbacks().is_err());
    assert_eq!(receipt(), before);
    assert!(f.store.callback_inbox(&orch).unwrap().is_empty());
    (f.store.raw().row("counter", json!(["slots", 7])))
        .delete()
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &worker, &orch)).is_empty());
}
