use super::*;

mod features;
mod red;

fn entry<'a>(slug: &'a str, parent: Option<&'a str>, status: &'a str) -> Entry<'a> {
    Entry {
        slug,
        branch: None,
        parent,
        status,
        instruction: false,
        held: false,
        orchestrators: Vec::new(),
        executors: Vec::new(),
        red: false,
    }
}

fn led<'a>(slug: &'a str, parent: Option<&'a str>, status: &'a str, by: &'a str) -> Entry<'a> {
    Entry {
        orchestrators: vec![by],
        ..entry(slug, parent, status)
    }
}

/// An instruction subtask of `parent`.
fn told<'a>(slug: &'a str, parent: &'a str, status: &'a str) -> Entry<'a> {
    Entry {
        instruction: true,
        ..entry(slug, Some(parent), status)
    }
}

/// The open slots `handle` holds, by the task whose work each is.
fn keys<'a>(ledger: &Ledger<'a>, handle: &str) -> Vec<&'a str> {
    ledger.held(handle).into_keys().collect()
}

#[test]
fn a_mirror_counts_with_the_task_it_reviews_when_the_board_holds_it() {
    let ledger = Ledger::new(
        [
            entry("sim/jump", None, "review"),
            entry("review/sim/jump", None, "review"),
            entry("review-2/sim/jump", None, "review"),
            entry("sim/jump/review", Some("sim/jump"), "review"),
            entry("review/gone", None, "review"),
            entry("docs/x/review", None, "review"),
        ],
        [],
    );
    for (slug, reviewed) in [
        ("sim/jump", "sim/jump"),
        ("review/sim/jump", "sim/jump"),
        ("review-2/sim/jump", "sim/jump"),
        ("sim/jump/review", "sim/jump"),
        ("review/gone", "review/gone"),
        ("docs/x/review", "docs/x/review"),
    ] {
        assert_eq!(ledger.reviewed(slug), reviewed, "{slug}");
        assert_eq!(ledger.work(slug), reviewed, "{slug}");
    }
}

#[test]
fn every_subtask_and_mirror_shares_the_parent_features_slot_and_owner() {
    let ledger = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("a/b", Some("a"), "in_progress"),
            led("a/b/c", Some("a/b"), "in_progress", "o"),
            told("a/build", "a", "in_progress"),
            told("a/build/again", "a/build", "in_progress"),
            told("a/b/fix", "a/b", "in_progress"),
            led("a/d", Some("a"), "in_progress", "p"),
            entry("a/d/e", Some("a/d"), "review"),
            entry("review/a/d/e", None, "review"),
            told("orphan", "gone", "in_progress"),
        ],
        [],
    );
    for (slug, owners, key) in [
        ("a", vec!["o"], "a"),
        ("a/b", vec!["o"], "a"),
        ("a/b/c", vec!["o"], "a"),
        ("a/build", vec!["o"], "a"),
        ("a/build/again", vec!["o"], "a"),
        ("a/b/fix", vec!["o"], "a"),
        ("a/d", vec!["o"], "a"),
        ("a/d/e", vec!["o"], "a"),
        ("review/a/d/e", vec!["o"], "a"),
    ] {
        let place = ledger.place(slug);
        assert_eq!(
            (place.owners, place.key, place.by_parent),
            (owners, key, false),
            "{slug}"
        );
    }
    assert_eq!(ledger.work("orphan"), "orphan", "its parent is not read");
    assert_eq!(keys(&ledger, "o"), ["a"]);
    assert!(keys(&ledger, "p").is_empty());
    assert_eq!(
        named(&ledger.held("o")),
        "a (in_progress) with a/b (in_progress) with a/b/c (in_progress) with a/b/fix (in_progress) with a/build (in_progress) with a/build/again (in_progress) with a/d (in_progress)"
    );
    assert_eq!(
        named(&ledger.reviews()["o"]),
        "a/d/e (review) with review/a/d/e (review)",
        "the work in review of the same slot, apart"
    );
}

#[test]
fn a_task_nobody_orchestrates_counts_against_its_executors_parents() {
    let mut loose = entry("loose", None, "in_progress");
    loose.executors = vec!["w1", "w2"];
    let mut orphan = entry("loose/sub", Some("loose"), "in_progress");
    orphan.executors = vec!["w3"];
    let ledger = Ledger::new(
        [loose, orphan, entry("idle", None, "in_progress")],
        [("w1", "o"), ("w2", "o"), ("w3", "p")],
    );
    let place = ledger.place("loose");
    assert_eq!(
        (place.owners, place.key, place.by_parent),
        (vec!["o"], "loose", true)
    );
    let place = ledger.place("loose/sub");
    assert_eq!((place.owners, place.key), (vec!["o"], "loose"));
    assert!(
        ledger.place("idle").owners.is_empty(),
        "no executor, no one to count against"
    );
    let place = ledger.place_by("idle", &["w3", "nobody"]);
    assert_eq!(place.owners, vec!["p"]);
    assert_eq!(keys(&ledger, "o"), ["loose"]);
    assert!(keys(&ledger, "p").is_empty());
}

/// `slug`, blocked under a hold that keeps its slot, of `by`'s.
fn held<'a>(slug: &'a str, by: &'a str) -> Entry<'a> {
    Entry {
        held: true,
        ..led(slug, None, "blocked", by)
    }
}

#[test]
fn only_in_progress_and_held_blocked_work_hold_a_slot_and_review_is_named_apart() {
    let ledger = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("a/review", Some("a"), "review"),
            entry("review/a", None, "review"),
            led("b", None, "review", "o"),
            held("bb", "o"),
            Entry {
                held: true,
                ..led("held-ready", None, "ready", "o")
            },
            led("c", None, "blocked", "o"),
            led("d", None, "ready", "o"),
            led("e", None, "backlog", "o"),
            led("f", None, "concept", "o"),
            led("g", None, "design", "o"),
            led("h", None, "done", "o"),
            led("i", None, "cancelled", "o"),
            led("j", None, "in_progress", "other"),
        ],
        [],
    );
    assert_eq!(
        named(&ledger.held("o")),
        "a (in_progress), bb (blocked, held)",
        "a hold keeps blocked work open, and only blocked work"
    );
    assert_eq!(
        named(&ledger.reviews()["o"]),
        "a/review (review) with review/a (review), b (review)",
        "work in review is open, and not counted"
    );
    let counts = ledger.counts();
    assert_eq!((counts["o"], counts["other"], counts.len()), (2, 1, 2));
    assert!(!ledger.reviews().contains_key("other"));
}

#[test]
fn a_reviewers_instruction_and_mirror_under_a_task_in_review_start_at_the_cap_named_apart() {
    let before = Ledger::new(
        [
            led("a", None, "review", "o"),
            led("b", None, "in_progress", "o"),
            led("c", None, "ready", "o"),
            told("a/review-by-x", "a", "ready"),
            entry("review/a", None, "ready"),
            entry("a/fix", Some("a"), "in_progress"),
        ],
        [],
    );
    let start =
        |ledger: &Ledger<'static>, slug: &str| ledger.changed(slug, Some("in_progress"), None, &[]);
    for slug in ["a/review-by-x", "review/a"] {
        let after = start(&before, slug);
        assert!(after.reviewing(slug), "{slug}: judged by a, in review");
        assert_eq!(started(&before, &after, 1), Ok(vec![]), "{slug}");
    }
    assert!(before.reviewing("a/fix"), "the work under a task in review");
    assert!(!before.reviewing("b") && !before.reviewing("c"));
    let both = start(&start(&before, "a/review-by-x"), "review/a");
    let full = started(&both, &start(&both, "c"), 1).unwrap_err();
    assert_eq!(
        refusal("c cannot start", &full, &[], 1),
        "c cannot start: o holds 1 open tasks, the cap of 1 an orchestrator holds: b (in_progress); a slot frees when one of them goes to review or is done or cancelled; not counted, as work in review takes no slot: a (review) with a/fix (in_progress) with a/review-by-x (in_progress) with review/a (in_progress)"
    );
    // A changes verdict sends a back to work: its slot counts again, the reviewer's work in it.
    let back = both.changed("a", Some("in_progress"), None, &[]);
    assert_eq!(back.counts()["o"], 2);
    assert!(back.reviews().is_empty());
}

#[test]
fn work_a_hold_keeps_open_returns_without_a_new_slot_and_other_blocked_work_takes_one() {
    let before = Ledger::new(
        [
            held("kept", "o"),
            led("freed", None, "blocked", "o"),
            led("busy", None, "in_progress", "o"),
        ],
        [],
    );
    for status in ["review", "in_progress"] {
        let kept = before.changed("kept", Some(status), None, &[]);
        assert_eq!(started(&before, &kept, 2), Ok(vec!["o"]), "{status}");
        assert!(!kept.tasks["kept"].held, "a move ends the hold");
    }
    let freed = before.changed("freed", Some("in_progress"), None, &[]);
    assert_eq!(started(&before, &freed, 2).unwrap_err().owner, "o");
    let reviewed = before.changed("freed", Some("review"), None, &[]);
    assert_eq!(
        started(&before, &reviewed, 2),
        Ok(vec![]),
        "back to review takes no slot, at the cap too"
    );
    let unheld = before.changed("kept", Some("blocked"), None, &[]);
    assert_eq!(
        unheld.counts()["o"],
        1,
        "blocked again by a move, it holds no slot"
    );
}

#[test]
fn a_change_of_parent_moves_the_open_work_below_to_the_new_tree() {
    let before = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("a/part", Some("a"), "in_progress"),
            entry("loose", None, "ready"),
            entry("loose/one", Some("loose"), "in_progress"),
            led("other", None, "ready", "p"),
            held("kept", "o"),
        ],
        [],
    );
    let detached = before.reparented("a/part", None);
    assert_eq!(detached.place("a/part").owners, Vec::<&str>::new());
    assert_eq!(
        started(&before, &detached, 1),
        Ok(vec!["o"]),
        "the owner's slot membership changes"
    );
    let back = detached.reparented("a/part", Some("a"));
    assert_eq!(
        started(&detached, &back, 2),
        Ok(vec!["o"]),
        "joins the existing feature slot"
    );
    let under = before.reparented("loose", Some("other"));
    assert_eq!(
        under.place("loose/one").owners,
        ["p"],
        "the work below moves"
    );
    assert_eq!(started(&before, &under, 0).unwrap_err().owner, "p");
    assert_eq!(started(&before, &under, 1), Ok(vec!["p"]));
    let moved = before.reparented("kept", Some("a"));
    assert!(
        !moved.tasks["kept"].held,
        "a hold lasts no change of parent"
    );
}

#[test]
fn a_start_is_refused_only_when_it_raises_an_owner_past_the_cap() {
    let mut tasks: Vec<Entry> = ["s1", "s2", "s3"]
        .into_iter()
        .map(|s| led(s, None, "in_progress", "o"))
        .collect();
    tasks.push(led("next", None, "ready", "o"));
    tasks.push(told("s1/more", "s1", "ready"));
    tasks.push(entry("s1/sub", Some("s1"), "ready"));
    tasks.push(entry("free", None, "ready"));
    let before = Ledger::new(tasks, []);
    let start = |slug: &str| before.changed(slug, Some("in_progress"), None, &[]);
    let full = started(&before, &start("next"), 3).expect_err("at the cap");
    assert_eq!((full.owner, full.held.len()), ("o", 3));
    assert_eq!(started(&before, &start("next"), 4), Ok(vec!["o"]));
    assert_eq!(
        started(&before, &start("s1/more"), 3),
        Ok(vec!["o"]),
        "an instruction joins s1's slot, and the write still holds o's counter"
    );
    assert_eq!(
        started(&before, &start("s1/sub"), 3),
        Ok(vec!["o"]),
        "a regular subtask joins the feature slot too"
    );
    assert_eq!(
        started(&before, &start("next"), 2).unwrap_err().held.len(),
        3,
        "above it"
    );
    assert_eq!(
        started(&before, &start("free"), 1),
        Ok(vec![]),
        "no one to count it against"
    );
    let stays = before.changed("s1", Some("review"), None, &[]);
    assert_eq!(
        started(&before, &stays, 1),
        Ok(vec!["o"]),
        "into review: no new slot, s1's freed"
    );
    assert_eq!(stays.counts()["o"], 2);
    let ends = before.changed("s2", Some("done"), None, &[]);
    assert_eq!(started(&before, &ends, 1), Ok(vec!["o"]), "a slot freed");
}

#[test]
fn a_task_two_orchestrators_hold_counts_against_both_and_is_refused_if_either_is_full() {
    let mut both = entry("both", None, "ready");
    both.orchestrators = vec!["p", "o"];
    let before = Ledger::new(
        [
            both,
            led("o1", None, "in_progress", "o"),
            led("o2", None, "in_progress", "o"),
            led("p1", None, "in_progress", "p"),
        ],
        [],
    );
    let after = before.changed("both", Some("in_progress"), None, &[]);
    assert_eq!(after.place("both").owners, ["o", "p"]);
    assert_eq!(started(&before, &after, 3), Ok(vec!["o", "p"]));
    let full = started(&before, &after, 2).unwrap_err();
    assert_eq!((full.owner, full.held.len()), ("o", 2), "o is full");
    let p_full = Ledger::new(
        [
            led("both", None, "ready", "o"),
            led("p1", None, "in_progress", "p"),
            led("p2", None, "in_progress", "p"),
        ]
        .map(|mut e| {
            if e.slug == "both" {
                e.orchestrators.push("p");
            }
            e
        }),
        [],
    );
    let after = p_full.changed("both", Some("in_progress"), None, &[]);
    assert_eq!(started(&p_full, &after, 2).unwrap_err().owner, "p");
    let reviewed = p_full.changed("both", Some("review"), None, &[]);
    assert_eq!(
        started(&p_full, &reviewed, 2),
        Ok(vec![]),
        "review counts for neither"
    );
}

#[test]
fn taking_the_role_moves_the_slots_of_the_work_below_to_the_taker() {
    let before = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("b", None, "in_progress"),
            entry("c", None, "ready"),
            entry("c/one", Some("c"), "in_progress"),
            entry("c/two", Some("c"), "review"),
            entry("d", None, "review"),
        ],
        [],
    );
    let taken = |slug| before.changed(slug, None, Some("o"), &[]);
    assert_eq!(
        started(&before, &taken("d"), 1),
        Ok(vec![]),
        "work in review takes no slot"
    );
    assert_eq!(taken("b").place("b").owners, vec!["o"]);
    assert_eq!(started(&before, &taken("b"), 1).unwrap_err().owner, "o");
    assert_eq!(started(&before, &taken("a"), 1), Ok(vec![]), "its own");
    assert_eq!(
        started(&before, &taken("c"), 1).unwrap_err().owner,
        "o",
        "c is not open, its children together hold one slot"
    );
    assert_eq!(started(&before, &taken("c"), 2), Ok(vec!["o"]));
}

#[test]
fn a_loop_the_board_should_never_hold_ends_the_walk() {
    let ledger = Ledger::new(
        [
            entry("x", Some("y"), "in_progress"),
            entry("y", Some("x"), "in_progress"),
            told("i", "j", "in_progress"),
            told("j", "i", "in_progress"),
        ],
        [],
    );
    assert_eq!(ledger.place("x").key, "x");
    assert_eq!(ledger.place("unknown").key, "unknown");
    assert!(["i", "j"].contains(&ledger.work("i")));
}

#[test]
fn the_tasks_a_ledger_needs_are_the_parents_and_the_reviewed() {
    let needed = wanted([
        ("a/b", Some("a")),
        ("review/x", None),
        ("review-3/y/z", Some("p")),
        ("s/review", Some("s")),
        ("t/review", None),
        ("root", None),
    ]);
    assert_eq!(
        needed.into_iter().collect::<Vec<_>>(),
        ["a", "p", "s", "t", "x", "y/z"]
    );
}

#[test]
fn the_refusal_names_as_many_open_tasks_as_it_counts_and_how_a_slot_frees() {
    let ledger = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("review/a", None, "review"),
            told("a/build", "a", "in_progress"),
            led("b", None, "review", "o"),
        ],
        [],
    );
    let full = Full {
        owner: "o",
        held: ledger.held("o"),
        red: Held::new(),
        review: ledger.reviews().remove("o").unwrap(),
    };
    let said = refusal("c cannot start", &full, &[], 1);
    assert_eq!(
        said,
        "c cannot start: o holds 1 open tasks, the cap of 1 an orchestrator holds: a (in_progress) with a/build (in_progress); a slot frees when one of them goes to review or is done or cancelled; not counted, as work in review takes no slot: review/a (review), b (review)"
    );
    let listed = said.split_once("holds: ").unwrap().1;
    let listed = listed.split_once("; ").unwrap().0;
    assert_eq!(listed.split(", ").count(), full.held.len(), "{said}");
    assert!(
        refusal("c cannot start", &full, &["w"], 1).starts_with(
            "c cannot start: o (no orchestrator holds it: o is the parent of w) holds 1"
        )
    );
    let nothing_in_review = Full {
        review: Held::new(),
        ..full
    };
    assert!(
        !refusal("c cannot start", &nothing_in_review, &[], 1).contains("not counted"),
        "no work in review, nothing named apart"
    );
}

#[test]
fn only_a_claim_refused_at_the_cap_points_to_the_reservation() {
    let said = claim_hint("work/a", "work/a cannot start");
    assert!(
        said.starts_with("; if the task is reserved for you, start work under the reservation now"),
        "{said}"
    );
    assert!(said.contains("`just tasks reserve work/a`"), "{said}");
    assert!(said.contains("retry the claim"), "{said}");
    assert_eq!(
        claim_hint("work/a", "work/a cannot go from blocked to review"),
        ""
    );
    assert_eq!(
        claim_hint(
            "work/a",
            "o cannot take orchestrator on work/a, which is ready"
        ),
        ""
    );
}
