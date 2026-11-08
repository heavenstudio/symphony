//! Red work in the ledger: what takes no slot, until when, and what a start or a red's close holds
//! for it.

use super::*;

/// `slug`, filed for a red.
fn red<'a>(entry: Entry<'a>) -> Entry<'a> {
    Entry { red: true, ..entry }
}

#[test]
fn a_reds_close_holds_the_counts_it_raises_and_the_red_that_work_started_under_it_holds() {
    let open = Ledger::new(
        [
            red(led("red", None, "review", "o")),
            entry("red/fix", Some("red"), "in_progress"),
            entry("red/new", Some("red"), "ready"),
            led("a", None, "in_progress", "o"),
            led("b", None, "ready", "o"),
        ],
        [],
    );
    let closed = open.changed("red", Some("cancelled"), None, &[]);
    assert_eq!(
        (changed(&open, &closed), reds(&open, &closed)),
        (vec!["o"], vec!["red"]),
        "red/fix counts from the close"
    );
    let past = started(&open, &closed, 1).map_err(|full| full.owner);
    assert_eq!(
        past,
        Err("o"),
        "past the cap, which a close is never refused at"
    );
    let under = open.changed("red/new", Some("in_progress"), None, &[]);
    assert_eq!(
        (changed(&open, &under), reds(&open, &under)),
        (vec![], vec!["red"]),
        "work started under the open red takes no slot and holds the red"
    );
    let feature = open.changed("b", Some("in_progress"), None, &[]);
    assert_eq!(
        (changed(&open, &feature), reds(&open, &feature)),
        (vec!["o"], vec![]),
        "a feature's start relies on no red"
    );
    let out = open.reparented("red/fix", None);
    assert_eq!(
        reds(&open, &out),
        ["red"],
        "open work moved out from under it"
    );
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
fn a_red_done_or_cancelled_exempts_nothing_under_it() {
    for closed in ["done", "cancelled"] {
        let ledger = Ledger::new(
            [
                red(led("tests/red", None, closed, "o")),
                entry("tests/red/fix", Some("tests/red"), "in_progress"),
                told("tests/red/fix/do", "tests/red/fix", "in_progress"),
            ],
            [],
        );
        for slug in ["tests/red", "tests/red/fix", "tests/red/fix/do"] {
            assert!(!ledger.red(slug), "{slug} under a {closed} red");
        }
        assert_eq!(
            (named(&ledger.held("o")), ledger.reds().contains_key("o")),
            (
                "tests/red/fix (in_progress) with tests/red/fix/do (in_progress)".to_string(),
                false
            ),
            "the work under a {closed} red is counted"
        );
    }
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
