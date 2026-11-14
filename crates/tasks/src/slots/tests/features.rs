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
        assert_eq!(ledger.held("o")["umbrella"], [("child", "in_progress")]);
        assert_eq!(ledger.reviews()["o"]["umbrella"], [("nested", "review")]);
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
                ..led("review/a", None, "in_progress", "o")
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

#[test]
fn normalizing_an_independent_feature_keeps_its_instruction_in_the_feature_slot() {
    let umbrella = "docs/process-evaluation";
    let feature = "tools/pipeline-review-alarm";
    let child = "tools/pipeline-review-alarm/codex-publisher-boundary-fixes";
    let before = Ledger::new(
        [
            entry(umbrella, None, "done"),
            led(feature, Some(umbrella), "in_progress", "sable-finch-625"),
            Entry {
                executors: vec!["nimble-lark-274"],
                ..told(child, feature, "in_progress")
            },
        ],
        [("nimble-lark-274", "another-session-parent")],
    );
    assert_eq!(before.work(child), umbrella);

    let normalized = before.reparented(feature, None);
    assert_eq!(normalized.counts(), HashMap::from([("sable-finch-625", 1)]));
    assert_eq!(normalized.work(child), feature);
    assert_eq!(normalized.place(child).owners, ["sable-finch-625"]);
    assert_eq!(
        normalized.held("sable-finch-625")[feature],
        [(feature, "in_progress"), (child, "in_progress")]
    );
    assert_eq!(
        started(&before, &normalized, 1),
        Ok(vec!["sable-finch-625"])
    );
    assert_eq!(normalized.tasks[umbrella], before.tasks[umbrella]);
    assert_eq!(normalized.tasks[child], before.tasks[child]);
    assert!(normalized.tasks[feature].parent.is_none());
    assert!(!normalized.tasks[feature].instruction);
    assert_eq!(
        normalized.tasks[feature].orchestrators,
        before.tasks[feature].orchestrators
    );
    assert_eq!(normalized.tasks[feature].status, "in_progress");
}
