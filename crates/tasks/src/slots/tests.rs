use super::*;

fn entry<'a>(slug: &'a str, parent: Option<&'a str>, status: &'a str) -> Entry<'a> {
    Entry {
        slug,
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
fn each_open_task_holds_a_slot_of_its_own_and_only_a_mirror_or_an_instruction_shares_one() {
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
        ("a/b", vec!["o"], "a/b"),
        ("a/b/c", vec!["o"], "a/b/c"),
        ("a/build", vec!["o"], "a"),
        ("a/build/again", vec!["o"], "a"),
        ("a/b/fix", vec!["o"], "a/b"),
        ("a/d", vec!["p"], "a/d"),
        ("a/d/e", vec!["p"], "a/d/e"),
        ("review/a/d/e", vec!["p"], "a/d/e"),
    ] {
        let place = ledger.place(slug);
        assert_eq!(
            (place.owners, place.key, place.by_parent),
            (owners, key, false),
            "{slug}"
        );
    }
    assert_eq!(ledger.work("orphan"), "orphan", "its parent is not read");
    assert_eq!(keys(&ledger, "o"), ["a", "a/b", "a/b/c"]);
    assert_eq!(keys(&ledger, "p"), ["a/d", "a/d/e"]);
    assert_eq!(
        named(&ledger.held("o")),
        "a (in_progress) with a/build (in_progress) with a/build/again (in_progress), a/b (in_progress) with a/b/fix (in_progress), a/b/c (in_progress)"
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
    assert_eq!((place.owners, place.key), (vec!["p"], "loose/sub"));
    assert!(
        ledger.place("idle").owners.is_empty(),
        "no executor, no one to count against"
    );
    let place = ledger.place_by("idle", &["w3", "nobody"]);
    assert_eq!(place.owners, vec!["p"]);
    assert_eq!(keys(&ledger, "o"), ["loose"]);
    assert_eq!(keys(&ledger, "p"), ["loose/sub"]);
}

/// `slug`, blocked under a hold that keeps its slot, of `by`'s.
fn held<'a>(slug: &'a str, by: &'a str) -> Entry<'a> {
    Entry {
        held: true,
        ..led(slug, None, "blocked", by)
    }
}

#[test]
fn only_in_progress_review_and_held_blocked_work_hold_a_slot() {
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
        "a (in_progress) with a/review (review) with review/a (review), b (review), bb (blocked, held)",
        "a hold keeps blocked work open, and only blocked work"
    );
    let counts = ledger.counts();
    assert_eq!((counts["o"], counts["other"], counts.len()), (3, 1, 2));
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
        let freed = before.changed("freed", Some(status), None, &[]);
        assert_eq!(
            started(&before, &freed, 2).unwrap_err().owner,
            "o",
            "{status}"
        );
    }
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
            entry("loose/one", Some("loose"), "review"),
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
        "a slot freed"
    );
    let back = detached.reparented("a/part", Some("a"));
    assert_eq!(started(&detached, &back, 2).unwrap_err().owner, "o");
    assert_eq!(started(&detached, &back, 3), Ok(vec!["o"]));
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
        started(&before, &start("s1/sub"), 3).map_err(|f| f.owner),
        Err("o"),
        "a subtask that is work of its own takes a slot of its own"
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
        "open to open: no new slot"
    );
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
            led("o2", None, "review", "o"),
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
    let after = p_full.changed("both", Some("review"), None, &[]);
    assert_eq!(started(&p_full, &after, 2).unwrap_err().owner, "p");
}

#[test]
fn taking_the_role_moves_the_slots_of_the_work_below_to_the_taker() {
    let before = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            entry("b", None, "review"),
            entry("c", None, "ready"),
            entry("c/one", Some("c"), "in_progress"),
            entry("c/two", Some("c"), "review"),
        ],
        [],
    );
    let taken = |slug| before.changed(slug, None, Some("o"), &[]);
    assert_eq!(taken("b").place("b").owners, vec!["o"]);
    assert_eq!(started(&before, &taken("b"), 1).unwrap_err().owner, "o");
    assert_eq!(started(&before, &taken("a"), 1), Ok(vec![]), "its own");
    assert_eq!(
        started(&before, &taken("c"), 2).unwrap_err().owner,
        "o",
        "c is not open, the two below it are"
    );
    assert_eq!(started(&before, &taken("c"), 3), Ok(vec!["o"]));
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
    };
    let said = refusal("c cannot start", &full, &[], 2);
    assert_eq!(
        said,
        "c cannot start: o holds 2 open tasks, the cap of 2 an orchestrator holds: a (in_progress) with a/build (in_progress) with review/a (review), b (review); a slot frees when one of them is done or cancelled"
    );
    let listed = said.split_once("holds: ").unwrap().1;
    let listed = listed.split_once("; ").unwrap().0;
    assert_eq!(listed.split(", ").count(), full.held.len(), "{said}");
    assert!(
        refusal("c cannot start", &full, &["w"], 2).starts_with(
            "c cannot start: o (no orchestrator holds it: o is the parent of w) holds 2"
        )
    );
}

/// `slug`, filed for a red.
fn red<'a>(entry: Entry<'a>) -> Entry<'a> {
    Entry { red: true, ..entry }
}

#[test]
fn red_work_takes_no_slot_with_its_mirrors_instructions_and_the_work_under_it() {
    let ledger = Ledger::new(
        [
            red(led("tests/red", None, "in_progress", "o")),
            told("tests/red/do", "tests/red", "in_progress"),
            entry("tests/red/fix", Some("tests/red"), "review"),
            entry("review/tests/red/fix", None, "review"),
            entry("tests/red/fix/part", Some("tests/red/fix"), "in_progress"),
            red(led("tests/other", None, "ready", "o")),
            led("a", None, "in_progress", "o"),
            entry("review/a", None, "review"),
            led("tests/red-alike", None, "in_progress", "o"),
        ],
        [],
    );
    for slug in [
        "tests/red",
        "tests/red/do",
        "tests/red/fix",
        "review/tests/red/fix",
        "tests/red/fix/part",
    ] {
        assert!(ledger.red(slug), "{slug}");
    }
    for slug in ["a", "review/a", "tests/red-alike", "unknown"] {
        assert!(!ledger.red(slug), "{slug}: no mark on it or above it");
    }
    assert_eq!(
        named(&ledger.held("o")),
        "a (in_progress) with review/a (review), tests/red-alike (in_progress)",
        "a slug that looks red is counted: only the mark tells"
    );
    assert_eq!(ledger.counts()["o"], 2);
    assert_eq!(
        named(&ledger.reds()["o"]),
        "tests/red (in_progress) with tests/red/do (in_progress), tests/red/fix (review) with review/tests/red/fix (review), tests/red/fix/part (in_progress)",
        "the red work is listed by slot, and only open work"
    );
}

#[test]
fn red_work_starts_at_any_count_and_a_feature_start_at_the_cap_is_still_refused() {
    let mut tasks: Vec<Entry> = ["s1", "s2", "s3"]
        .into_iter()
        .map(|s| led(s, None, "in_progress", "o"))
        .collect();
    tasks.push(red(led("red", None, "ready", "o")));
    tasks.push(entry("red/fix", Some("red"), "ready"));
    tasks.push(led("next", None, "ready", "o"));
    let before = Ledger::new(tasks, []);
    let start =
        |ledger: &Ledger<'static>, slug: &str| ledger.changed(slug, Some("in_progress"), None, &[]);
    assert_eq!(
        started(&before, &start(&before, "red"), 3),
        Ok(vec![]),
        "no counted slot changes, so the write holds no counter"
    );
    let reds = start(&start(&before, "red"), "red/fix");
    assert_eq!(started(&before, &reds, 3), Ok(vec![]));
    assert_eq!(started(&before, &reds, 1), Ok(vec![]), "above the cap too");
    let full = started(&reds, &start(&reds, "next"), 3).expect_err("a feature at the cap");
    assert_eq!((full.owner, full.held.len()), ("o", 3));
    assert_eq!(
        full.red.keys().copied().collect::<Vec<_>>(),
        ["red", "red/fix"]
    );
    assert_eq!(
        Some(full.red),
        reds.reds().remove("o"),
        "what it held before the start"
    );
    // Work moved under a red frees its slot, and moved back out takes one again.
    let under = before.reparented("s1", Some("red"));
    assert_eq!(started(&before, &under, 3), Ok(vec!["o"]), "a slot freed");
    assert_eq!(started(&under, &start(&under, "next"), 3), Ok(vec!["o"]));
    assert_eq!(
        started(&under, &before, 2).unwrap_err().owner,
        "o",
        "back out of the red, s1 counts again"
    );
}

#[test]
fn a_refusal_names_the_red_work_apart_as_not_counted() {
    let ledger = Ledger::new(
        [
            led("a", None, "in_progress", "o"),
            red(led("tests/red", None, "in_progress", "o")),
            entry("review/tests/red", None, "review"),
            entry("fix", Some("tests/red"), "in_progress"),
        ],
        [],
    );
    let full = Full {
        owner: "o",
        held: ledger.held("o"),
        red: ledger.reds().remove("o").unwrap(),
    };
    assert_eq!(
        refusal("c cannot start", &full, &[], 1),
        "c cannot start: o holds 1 open tasks, the cap of 1 an orchestrator holds: a (in_progress); a slot frees when one of them is done or cancelled; not counted, as red work takes no slot: fix (in_progress), tests/red (in_progress) with review/tests/red (review)"
    );
}

#[test]
fn the_manifest_names_the_cap_or_none_and_a_broken_one_is_an_error() {
    let dir = std::env::temp_dir().join(format!("slots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(configured(&dir), Ok(None), "no manifest");
    for (text, expected) in [
        ("[project]\nname = \"x\"\n", Ok(None)),
        ("[board]\non_review = [\"x\"]\n", Ok(None)),
        ("[board]\norchestrator_slots = 8\n", Ok(Some(8))),
        ("[board]\norchestrator_slots = 1\n", Ok(Some(1))),
    ] {
        std::fs::write(dir.join(MANIFEST), text).unwrap();
        assert_eq!(configured(&dir), expected, "{text}");
    }
    for (text, said) in [
        (
            "[board]\norchestrator_slots = 0\n",
            "an orchestrator needs at least one slot",
        ),
        (
            "[board]\norchestrator_slots = -2\n",
            "an orchestrator needs at least one slot",
        ),
        (
            "[board]\norchestrator_slots = \"eight\"\n",
            "[board] orchestrator_slots",
        ),
    ] {
        std::fs::write(dir.join(MANIFEST), text).unwrap();
        let error = configured(&dir).unwrap_err();
        assert!(error.contains(said), "{text}: {error}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
