//! Parent-feature accounting and recorded-branch pairing through the real store boundary.

use super::*;
use symphony_board::NewLaunch;
use symphony_board::slots::Ledger;

#[test]
fn held_children_keep_the_slot_of_an_inactive_parent_until_the_last_child_finishes() {
    let seed = with(sessions(), "umbrella", "done", None, Some(O));
    let seed = with(seed, "child", "review", Some("umbrella"), Some(P));
    let seed = with(seed, "next", "ready", None, Some(O));
    let f = Fixture::new(seed.role("child", K, "executor"));
    let store = capped(&f, 1);
    store.hold_blocked("child", P).unwrap();
    let said = refused(store.claim("next", K));
    assert!(said.contains("child (blocked, held)"), "{said}");
    assert!(said.contains("olive-orch holds 1"), "{said}");
    assert_eq!(store.get("umbrella").unwrap().status, "done");
    store.move_to("child", "in_progress", P).unwrap();
    store.done("child", K, "integrated into umbrella").unwrap();
    store.claim("next", K).unwrap();
}

#[test]
fn a_red_nested_in_an_ordinary_feature_does_not_exempt_its_parent_or_siblings() {
    let f = eight_and(&[("next", "ready")]);
    let store = capped(&f, 8);
    let red_slug = "tests/main-red-nested";
    store
        .create(&NewTask {
            parent: Some("work/a".into()),
            red: Some("main".into()),
            ..NewTask::new(red_slug, "Red", F)
        })
        .unwrap();
    store
        .create(&NewTask {
            parent: Some(red_slug.into()),
            ..NewTask::new("fix", "Fix", "owner")
        })
        .unwrap();
    store.claim("fix", W).unwrap();
    let said = refused(store.claim("next", K));
    assert!(said.contains("holds 8 open tasks"), "{said}");
    assert!(said.contains("work/a (in_progress)"), "{said}");
    assert!(
        said.contains("not counted, as red work takes no slot: fix (in_progress)"),
        "{said}"
    );
    store.move_to(red_slug, "cancelled", "owner").unwrap();
    let said = refused(store.claim("next", K));
    assert!(
        said.contains("work/a (in_progress) with fix (in_progress)"),
        "{said}"
    );
    assert!(!said.contains("not counted"), "{said}");
}

#[test]
fn a_branch_mirror_and_regular_nested_children_share_the_parent_slot_at_eight() {
    let mut seed = eight_and_seed(&[("next", "ready")]);
    seed = with(seed, "part", "in_progress", Some("work/a"), Some(P));
    seed = with(seed, "nested", "review", Some("part"), Some(P));
    seed = with(seed, "review/alias", "review", None, None);
    seed = with(seed, "review-2/alias", "review", None, None);
    let f = Fixture::new(seed.role("part", K, "executor"));
    f.store
        .record_launch(&NewLaunch {
            branch: "alias".into(),
            ..NewLaunch::new("work/a", K, "executor")
        })
        .unwrap();
    f.store
        .record_launch(&NewLaunch {
            branch: "alias".into(),
            ..NewLaunch::new("review/alias", W, "executor")
        })
        .unwrap();
    let store = capped(&f, 8);
    store.take_role("review/alias", O, "orchestrator").unwrap();
    let board = store.board().unwrap();
    let ledger = Ledger::of_board(&board);
    assert_eq!(ledger.counts().get(O), Some(&8));
    assert!(
        !ledger.counts().contains_key(P),
        "local child roles do not take another owner's slot"
    );
    assert_eq!(ledger.place("review-2/alias").key, "work/a");
    assert_eq!(ledger.held(O)["work/a"].len(), 2, "work/a and part");
    assert_eq!(ledger.reviews()[O]["work/a"].len(), 3, "in review, apart");
    assert_eq!(
        store
            .tasks_for_branch("alias")
            .unwrap()
            .iter()
            .map(|t| t.slug.as_str())
            .collect::<Vec<_>>(),
        ["work/a", "review/alias", "review-2/alias"]
    );
    let said = refused(store.claim("next", K));
    assert!(said.contains("holds 8 open tasks"), "{said}");
    assert!(
        said.contains("work/a (in_progress) with part (in_progress)"),
        "{said}"
    );
    assert!(
        said.contains("; not counted, as work in review takes no slot: nested (review) with review-2/alias (review) with review/alias (review)"),
        "{said}"
    );
}

#[test]
fn a_mirror_finds_its_inactive_recorded_task_and_latest_executor_branch_only() {
    let seed = eight_and_seed(&[])
        .task("legacy", "done")
        .role("legacy", O, "orchestrator")
        .task("review/new-branch", "ready");
    let f = Fixture::new(seed);
    for (handle, branch) in [(K, "old-branch"), (W, "new-branch")] {
        f.store
            .record_launch(&NewLaunch {
                branch: branch.into(),
                ..NewLaunch::new("legacy", handle, "executor")
            })
            .unwrap();
    }
    assert!(f.store.tasks_for_branch("old-branch").unwrap().is_empty());
    assert_eq!(
        f.store.tasks_for_branch("new-branch").unwrap()[0].slug,
        "legacy"
    );
    let store = capped(&f, 8);
    let said = refused(store.claim("review/new-branch", K));
    assert!(said.contains("olive-orch holds 8"), "{said}");
    assert_eq!(store.get("legacy").unwrap().status, "done");
}

#[test]
fn discovering_a_held_tasks_branch_does_not_erase_its_hold_above_the_cap() {
    let seed = with(sessions(), "kept", "review", None, Some(O));
    let seed = with(seed, "busy", "in_progress", None, Some(O));
    let seed = with(seed, "review/alias", "ready", None, None);
    let f = Fixture::new(seed);
    f.store
        .record_launch(&NewLaunch {
            branch: "alias".into(),
            ..NewLaunch::new("kept", K, "executor")
        })
        .unwrap();
    f.store.hold_blocked("kept", O).unwrap();
    let store = capped(&f, 1);
    assert_eq!(
        store.claim("review/alias", K).unwrap().status,
        "in_progress",
        "joining the held slot is allowed even above the cap"
    );
    assert_eq!(store.get("kept").unwrap().status, "blocked");
}
