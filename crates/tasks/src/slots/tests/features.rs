use super::*;

#[test]
fn branch_named_mirrors_join_a_nested_features_owner_at_the_cap() {
    let before = Ledger::new(
        [
            led("feature", None, "in_progress", "o"),
            Entry {
                branch: Some("different-branch"),
                ..led("feature/part", Some("feature"), "review", "child-owner")
            },
            led("review/different-branch", None, "review", "mirror-owner"),
            entry("review-2/different-branch", None, "review"),
        ],
        [],
    );
    assert_eq!(before.reviewed("review/different-branch"), "feature/part");
    assert_eq!(before.place("review-2/different-branch").owners, ["o"]);
    assert_eq!(before.counts(), HashMap::from([("o", 1)]));
    let after = before.changed("review/different-branch", None, Some("o"), &[]);
    assert_eq!(started(&before, &after, 1), Ok(vec![]));
    assert_eq!(after.place("review/different-branch").key, "feature");
}

#[test]
fn active_children_of_inactive_umbrellas_remain_counted_without_changing_the_parent() {
    for status in ["ready", "blocked", "done", "cancelled"] {
        let ledger = Ledger::new(
            [
                led("umbrella", None, status, "o"),
                led("child", Some("umbrella"), "in_progress", "child-owner"),
                entry("nested", Some("child"), "review"),
            ],
            [],
        );
        assert_eq!(ledger.tasks["umbrella"].status, status);
        assert_eq!(ledger.counts(), HashMap::from([("o", 1)]), "{status}");
        assert_eq!(
            ledger.held("o")["umbrella"],
            [("child", "in_progress"), ("nested", "review")]
        );
    }
    let legacy = Ledger::new(
        [
            entry("unowned", None, "done"),
            led("child", Some("unowned"), "in_progress", "remaining-owner"),
            entry("nested", Some("child"), "review"),
        ],
        [],
    );
    assert_eq!(legacy.counts(), HashMap::from([("remaining-owner", 1)]));
    assert_eq!(legacy.place("nested").key, "unowned");
}

#[test]
fn missing_parents_and_cyclic_mirrors_do_not_drop_active_work_or_loop() {
    let ledger = Ledger::new(
        [
            led("missing", Some("gone"), "in_progress", "o"),
            Entry {
                branch: Some("b"),
                ..led("review/a", None, "review", "o")
            },
            Entry {
                branch: Some("a"),
                ..entry("review/b", None, "review")
            },
        ],
        [],
    );
    assert_eq!(ledger.work("missing"), "missing");
    assert_eq!(ledger.place("missing").owners, ["o"]);
    assert_eq!(ledger.reviewed("review/a"), "review/a");
    assert_eq!(ledger.reviewed("review/b"), "review/b");
    assert_eq!(
        ledger.counts()["o"],
        2,
        "invalid ancestry keeps active work counted"
    );
}

#[test]
fn branch_pairing_prefers_the_slug_and_is_stable_for_legacy_duplicate_records() {
    assert_eq!(
        branch_task("b", [("z", Some("b"), None), ("a", Some("b"), None)]),
        Some("a")
    );
    assert_eq!(
        branch_task("b", [("a", Some("b"), None), ("b", None, None)]),
        Some("b")
    );
    assert_eq!(branch_task("missing", [("a", None, None)]), None);
    assert_eq!(
        branch_task(
            "b",
            [
                ("a", Some("b"), Some("z")),
                ("z", Some("b"), None),
                ("review/b", Some("b"), None)
            ]
        ),
        Some("z")
    );
    assert_eq!(branch_task("b", [("review/b", Some("b"), None)]), None);
}

#[test]
fn a_red_child_exempts_only_its_repair_subtree_and_fences_its_close() {
    let before = Ledger::new(
        [
            led("feature", None, "in_progress", "o"),
            Entry {
                red: true,
                ..entry("red", Some("feature"), "review")
            },
            entry("fix", Some("red"), "ready"),
            entry("sibling", Some("feature"), "in_progress"),
            led("unrelated", None, "ready", "o"),
        ],
        [],
    );
    let after = before.changed("fix", Some("in_progress"), None, &[]);
    assert_eq!(started(&before, &after, 1), Ok(vec![]));
    assert_eq!(reds(&before, &after), ["red"]);
    assert!(after.red("fix"));
    assert!(!after.red("feature") && !after.red("sibling"));
    assert_eq!(after.counts()["o"], 1);
    assert_eq!(
        started(
            &after,
            &after.changed("unrelated", Some("in_progress"), None, &[]),
            1
        )
        .unwrap_err()
        .owner,
        "o"
    );
    let closed = after.changed("red", Some("done"), None, &[]);
    assert!(!closed.red("fix"));
    assert_eq!(reds(&after, &closed), ["red"]);
}
