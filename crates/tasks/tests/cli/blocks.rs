//! `tasks move SLUG blocked --on THING [--why TEXT]`: a block names what it waits on, and one that
//! names nothing is refused, saying what to pass; `tasks show` prints what a task waits on.

use crate::Case;
use symphony_board::NewTask;
use symphony_board::waits_on::Blocked;

fn create(c: &Case, slug: &str) {
    c.store()
        .create(&NewTask::new(slug, "A task", "owner"))
        .unwrap();
}

#[test]
fn a_block_without_what_it_waits_on_is_refused_saying_what_to_pass() {
    let c = Case::new();
    create(&c, "sim/jump");
    let worker = c.register("worker");
    let ran = c.run_as(&worker, &["move", "sim/jump", "blocked"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert_eq!(
        ran.err.trim(),
        format!(
            "✖ a block names what it waits on: {} move sim/jump blocked --on THING [--why \
             TEXT], THING one of decision:NAME, check:NAME, task:SLUG or parked; nothing was \
             moved",
            c.board()
        )
    );
    assert_eq!(c.store().get("sim/jump").unwrap().status, "ready");
    // A why alone names nothing either.
    let ran = c.run_as(
        &worker,
        &["move", "sim/jump", "blocked", "--why", "later"],
        "",
    );
    assert_eq!(ran.code, 1, "{}", ran.err);
    // What a block waits on is a block's: a move anywhere else refuses it.
    let ran = c.run_as(
        &worker,
        &["move", "sim/jump", "design", "--on", "parked"],
        "",
    );
    assert_eq!(
        (ran.code, ran.err.trim()),
        (
            1,
            "✖ --on and --why say what a block waits on: sim/jump moves to design; nothing was \
             moved"
        )
    );
    assert_eq!(c.store().get("sim/jump").unwrap().status, "ready");
}

#[test]
fn a_block_names_a_thing_and_a_few_words_and_a_task_blocked_already_takes_one() {
    let c = Case::new();
    create(&c, "sim/jump");
    let worker = c.register("worker");
    c.ok(&worker, &["claim", "sim/jump"]);
    let long = "a".repeat(121);
    let ran = c.run_as(
        &worker,
        &[
            "move", "sim/jump", "blocked", "--on", "parked", "--why", &long,
        ],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("at most 120 characters"), "{}", ran.err);
    let ran = c.run_as(
        &worker,
        &["move", "sim/jump", "blocked", "--on", "amber-heron"],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err
            .contains("decision:NAME, check:NAME, task:SLUG or parked"),
        "{}",
        ran.err
    );
    let said = c.ok(
        &worker,
        &[
            "move",
            "sim/jump",
            "blocked",
            "--on",
            "decision:a-card",
            "--why",
            "which layout",
        ],
    );
    assert_eq!(said.trim(), "sim/jump is blocked");
    let record = Blocked::of(&c.store().get("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (
            record.on.as_str(),
            record.why.as_deref(),
            record.from.as_deref()
        ),
        ("decision:a-card", Some("which layout"), Some("in_progress"))
    );
    // Blocked already: what it waits on is named again in place, and it stays blocked.
    c.ok(
        &worker,
        &["move", "sim/jump", "blocked", "--on", "check:nightly"],
    );
    let record = Blocked::of(&c.store().get("sim/jump").unwrap()).unwrap();
    assert_eq!((record.on.as_str(), record.why), ("check:nightly", None));
}

#[test]
fn show_says_what_a_task_waits_on_and_for_how_long() {
    let c = Case::new();
    create(&c, "sim/jump");
    create(&c, "sim/after");
    let store = c.store();
    store
        .change_depends("sim/after", &["sim/jump".into()], &[], "owner")
        .unwrap();
    let worker = c.register("worker");
    let shown = |slug: &str| {
        let said = c.ok("owner", &["show", slug]);
        let line = said.lines().find(|l| l.trim_start().starts_with("Waits: "));
        line.map(|l| l.trim().to_string())
    };
    assert_eq!(
        shown("sim/jump").as_deref(),
        Some("Waits: on queued · waits to be staffed · 0 s")
    );
    assert_eq!(
        shown("sim/after").as_deref(),
        Some("Waits: on task: sim/jump · its dependency · 0 s")
    );
    c.ok(&worker, &["claim", "sim/jump"]);
    assert_eq!(
        shown("sim/jump").unwrap(),
        format!("Waits: on working: {worker} · executor · 0 s")
    );
    c.ok(
        &worker,
        &[
            "move",
            "sim/jump",
            "blocked",
            "--on",
            "task:sim/after",
            "--why",
            "it goes first",
        ],
    );
    assert_eq!(
        shown("sim/jump").as_deref(),
        Some("Waits: on task: sim/after · it goes first · 0 s")
    );
    // A finished task waits on nothing, and shows no line.
    store.move_to("sim/after", "cancelled", "owner").unwrap();
    store.move_to("sim/jump", "cancelled", "owner").unwrap();
    assert_eq!(shown("sim/jump"), None);
}
