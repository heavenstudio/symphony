//! What a block waits on (`Store::move_on`, `Store::hold_blocked_on`): kept on the task in the
//! move's own write, given to a task blocked already, removed by every write that takes the task
//! out of blocked, refused when it names no thing the board knows; and a block through the calls
//! that name none, which records nothing, as before.

use crate::common::{Fixture, NOW, Seed, refused};
use symphony_board::waits_on::{BLOCKED, Blocked};
use symphony_board::{On, Task};

const SLUG: &str = "sim/jump";

fn on<'a>(thing: &'a str, why: Option<&'a str>) -> Option<On<'a>> {
    Some(On { thing, why })
}

/// `SLUG` in progress under a session's executor role: the fixture and the executor's handle.
fn at_work() -> (Fixture, String) {
    let f = Fixture::new(Seed::default());
    let exe = f.session("worker");
    f.task(SLUG);
    f.store.claim(SLUG, &exe).unwrap();
    (f, exe)
}

fn record(task: &Task) -> Option<Blocked> {
    Blocked::of(task)
}

/// Whether the task's fields hold a block's record, whatever its status.
fn kept(task: &Task) -> bool {
    (task.fields.get(symphony_board::red::FIELDS)).is_some_and(|board| board.contains_key(BLOCKED))
}

#[test]
fn a_block_keeps_what_it_waits_on_and_says_it_on_the_thread() {
    let (f, exe) = at_work();
    let t = f
        .store
        .move_on(
            SLUG,
            "blocked",
            &exe,
            None,
            on("decision:a-card", Some("which layout")),
        )
        .unwrap();
    assert_eq!(t.status, "blocked");
    assert_eq!(
        record(&t),
        Some(Blocked {
            on: "decision:a-card".into(),
            why: Some("which layout".into()),
            by: exe.clone(),
            at: NOW,
            from: Some("in_progress".into()),
        })
    );
    let said = f.said(SLUG);
    assert_eq!(
        said[said.len() - 2..],
        [
            format!("{exe} moved it in_progress → blocked"),
            "blocked: on decision:a-card; which layout".to_string(),
        ]
    );
    // A reason given says the mover's own words; the record keeps the thing.
    f.store.move_to(SLUG, "in_progress", &exe).unwrap();
    let t = f
        .store
        .move_on(
            SLUG,
            "blocked",
            &exe,
            Some("the nightly run is red"),
            on("check:nightly", None),
        )
        .unwrap();
    assert_eq!(record(&t).unwrap().on, "check:nightly");
    assert_eq!(
        f.said(SLUG).last().unwrap(),
        "blocked: the nightly run is red"
    );
}

#[test]
fn every_way_out_of_blocked_removes_the_record() {
    let (f, exe) = at_work();
    let block = |f: &Fixture| {
        let t = f
            .store
            .move_on(SLUG, "blocked", &exe, None, on("parked", None))
            .unwrap();
        assert!(kept(&t));
    };
    block(&f);
    let back = f.store.move_to(SLUG, "in_progress", &exe).unwrap();
    assert!(!kept(&back), "{:?}", back.fields);
    // The owner's move back to review, and its cancel.
    block(&f);
    let review = f.store.move_to(SLUG, "review", "owner").unwrap();
    assert_eq!(review.status, "review");
    assert!(!kept(&review), "{:?}", review.fields);
    f.store.move_to(SLUG, "in_progress", &exe).unwrap();
    block(&f);
    let cancelled = f.store.move_to(SLUG, "cancelled", "owner").unwrap();
    assert!(!kept(&cancelled), "{:?}", cancelled.fields);
}

#[test]
fn a_task_blocked_already_is_given_what_it_waits_on_and_stays_blocked() {
    let (f, exe) = at_work();
    f.store.move_to(SLUG, "review", &exe).unwrap();
    // Blocked as before, naming nothing: no record.
    let bare = f.store.move_to(SLUG, "blocked", &exe).unwrap();
    assert_eq!(record(&bare), None);
    let lines = f.said(SLUG).len();
    f.advance(60.0);
    let named = f
        .store
        .move_on(SLUG, "blocked", &exe, None, on("decision:a-card", None))
        .unwrap();
    assert_eq!(named.status, "blocked");
    assert_eq!(
        named.updated, bare.updated,
        "no move: its status and time stand"
    );
    let first = record(&named).unwrap();
    // The status it was blocked from, read from its thread.
    assert_eq!(
        (first.on.as_str(), first.from.as_deref(), first.at),
        ("decision:a-card", Some("review"), NOW + 60.0)
    );
    assert_eq!(
        f.said(SLUG)[lines..],
        ["blocked: on decision:a-card".to_string()]
    );
    // Named again: the new thing in its place, the status it was blocked from kept.
    let again = f
        .store
        .move_on(
            SLUG,
            "blocked",
            &exe,
            None,
            on("parked", Some("until spring")),
        )
        .unwrap();
    let second = record(&again).unwrap();
    assert_eq!(
        (
            second.on.as_str(),
            second.why.as_deref(),
            second.from.as_deref()
        ),
        ("parked", Some("until spring"), Some("review"))
    );
    // Only a holder of a role names it.
    let other = f.session("worker");
    assert_eq!(
        refused(
            f.store
                .move_on(SLUG, "blocked", &other, None, on("parked", None))
        ),
        format!("{other} holds no role on {SLUG}, which is blocked")
    );
}

#[test]
fn a_block_naming_what_the_board_does_not_know_is_refused_and_nothing_moves() {
    let (f, exe) = at_work();
    f.task("sim/other");
    let before = f.said(SLUG).len();
    let refusal = |thing: &str, why: Option<&str>, status: &str| {
        refused(f.store.move_on(SLUG, status, &exe, None, on(thing, why)))
    };
    assert!(refusal("amber-heron", None, "blocked").contains("decision:NAME"));
    assert!(refusal("decision:two words", None, "blocked").contains("task:SLUG"));
    assert_eq!(
        refusal("task:sim/nowhere", None, "blocked"),
        "no task 'sim/nowhere': a block waits on a task the board holds"
    );
    assert_eq!(
        refusal("task:sim/jump", None, "blocked"),
        "sim/jump cannot wait on itself"
    );
    let long = refusal("parked", Some(&"a".repeat(121)), "blocked");
    assert!(
        long.contains("at most 120 characters: this one is 121"),
        "{long}"
    );
    assert_eq!(
        refusal("parked", None, "review"),
        "what a block waits on is said by a move to blocked, not to review"
    );
    let t = f.store.get(SLUG).unwrap();
    assert_eq!((t.status.as_str(), kept(&t)), ("in_progress", false));
    assert_eq!(
        f.said(SLUG).len(),
        before,
        "a refusal says nothing on the thread"
    );
    // A task the board holds, open: taken.
    let t = f
        .store
        .move_on(SLUG, "blocked", &exe, None, on("task:sim/other", None))
        .unwrap();
    assert_eq!(record(&t).unwrap().on, "task:sim/other");
}

#[test]
fn a_hold_names_what_it_waits_on_and_keeps_its_slot_as_before() {
    let (f, exe) = at_work();
    f.store.move_to(SLUG, "review", &exe).unwrap();
    let held = f
        .store
        .hold_blocked_on(
            SLUG,
            &exe,
            on("check:owner", Some("the owner's approve of 0123abc")),
        )
        .unwrap();
    let kept_record = record(&held).unwrap();
    assert_eq!(
        (kept_record.on.as_str(), kept_record.from.as_deref()),
        ("check:owner", Some("review"))
    );
    // A hold writes no reason line: the land that held it says why.
    assert_eq!(
        f.said(SLUG).last().unwrap(),
        &format!("{exe} moved it review → blocked")
    );
    // The library's and the api's calls that name nothing block as before, and record nothing.
    f.store.move_to(SLUG, "review", &exe).unwrap();
    let bare = f.store.hold_blocked(SLUG, &exe).unwrap();
    assert_eq!((bare.status.as_str(), kept(&bare)), ("blocked", false));
}

#[test]
fn when_each_task_took_its_status_and_the_one_before_are_read_at_once() {
    let (f, exe) = at_work();
    f.task("sim/ready");
    f.advance(30.0);
    f.store
        .move_on(SLUG, "blocked", &exe, None, on("parked", None))
        .unwrap();
    let slugs = ["sim/jump", "sim/ready", "sim/nowhere"].map(String::from);
    let read = f.store.in_status(&slugs).unwrap();
    assert_eq!(read.len(), 2, "a slug no task has is left out: {read:?}");
    assert_eq!(read["sim/jump"].since, NOW + 30.0);
    assert_eq!(read["sim/jump"].from.as_deref(), Some("in_progress"));
    assert_eq!(read["sim/ready"].from, None);
    // The read of times alone still refuses it.
    assert_eq!(
        refused(f.store.in_status_since(&slugs)),
        "no task 'sim/nowhere'"
    );
}
