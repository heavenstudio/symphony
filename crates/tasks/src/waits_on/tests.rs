//! Each line a task's card ends in, from a board in memory: who is there, the block's record, the
//! landing's, and what a block that names nothing reads as.

use super::*;
use serde_json::json;

fn task(slug: &str, status: &str) -> Task {
    Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: format!("Task {slug}"),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: String::new(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: Default::default(),
    }
}

fn held(mut t: Task, role: &str, handle: &str, health: Health) -> Task {
    t.roles.push(crate::Role {
        handle: handle.into(),
        role: role.into(),
        since: 0.0,
        expires: None,
        health,
    });
    t
}

fn depending(mut t: Task, on: &[&str]) -> Task {
    t.depends = on.iter().map(|s| (*s).to_string()).collect();
    t
}

fn under(mut t: Task, parent: &str, id: i64) -> Task {
    t.parent = Some(parent.into());
    t.id = id;
    t
}

fn board_field(mut t: Task, name: &str, value: Value) -> Task {
    (t.fields.entry(crate::red::FIELDS.into()).or_default()).insert(name.into(), value);
    t
}

fn blocked_on(t: Task, on: &str, why: Option<&str>) -> Task {
    let record = Blocked {
        on: on.into(),
        why: why.map(Into::into),
        by: "amber-heron".into(),
        at: 5.0,
        from: Some("in_progress".into()),
    };
    board_field(t, BLOCKED, record.value())
}

/// The line of `slug` on the board `tasks`, its landing read as the server reads it.
fn line_of(tasks: &[Task], slug: &str) -> Option<Line> {
    let look = Look::new(tasks, &[]);
    let task = look.task(slug).unwrap();
    line(task, &look, Landing::of(task, None).as_ref())
}

/// The kind, who and why of `slug`'s line.
fn read(tasks: &[Task], slug: &str) -> (&'static str, String, String) {
    let line = line_of(tasks, slug).expect("an open task waits on something");
    (line.kind, line.who, line.why)
}

fn says(kind: &'static str, who: &str, why: &str) -> (&'static str, String, String) {
    (kind, who.into(), why.into())
}

#[test]
fn a_task_at_work_waits_on_its_holder_a_stale_one_marked_and_a_gone_one_on_nobody() {
    let board = [
        held(
            task("live", "in_progress"),
            "executor",
            "amber-heron",
            Health::Live,
        ),
        held(
            task("stale", "in_progress"),
            "executor",
            "amber-heron",
            Health::Stale,
        ),
        held(
            task("unknown", "design"),
            "executor",
            "jade-owl",
            Health::Unknown,
        ),
        held(
            task("lost", "in_progress"),
            "executor",
            "plum-wren",
            Health::Lost,
        ),
        held(task("left", "ready"), "executor", "plum-wren", Health::Left),
        task("nobody", "in_progress"),
    ];
    assert_eq!(
        read(&board, "live"),
        says(WORKING, "amber-heron", "executor")
    );
    assert_eq!(
        read(&board, "stale"),
        says(WORKING, "amber-heron (stale)", "executor")
    );
    // Liveness unknown reads as live, as PULL reads it.
    assert_eq!(
        read(&board, "unknown"),
        says(WORKING, "jade-owl", "executor")
    );
    assert_eq!(
        read(&board, "lost"),
        says(NOBODY, "plum-wren", "executor plum-wren lost")
    );
    assert_eq!(
        read(&board, "left"),
        says(NOBODY, "plum-wren", "executor plum-wren left")
    );
    assert_eq!(read(&board, "nobody"), says(NOBODY, "", "no executor"));
}

#[test]
fn a_session_running_a_test_is_there_whatever_its_beat_says() {
    let board = [held(
        task("t", "in_progress"),
        "executor",
        "teal",
        Health::Lost,
    )];
    let busy = ["teal".to_string()];
    let look = Look::new(&board, &busy);
    let line = line(&board[0], &look, None).unwrap();
    assert_eq!((line.kind, line.who.as_str()), (WORKING, "teal"));
    assert!(look.staffed(&board[0]));
}

#[test]
fn work_under_a_task_is_its_work_the_oldest_subtask_named_first() {
    let board = [
        task("parent", "in_progress"),
        under(
            held(
                task("parent/b", "in_progress"),
                "executor",
                "teal",
                Health::Live,
            ),
            "parent",
            2,
        ),
        under(
            held(task("parent/a", "ready"), "executor", "jade", Health::Stale),
            "parent",
            1,
        ),
        // Done, it is no work going on.
        under(
            held(task("parent/c", "done"), "executor", "plum", Health::Live),
            "parent",
            0,
        ),
    ];
    assert_eq!(
        read(&board, "parent"),
        says(WORKING, "jade (stale)", "executor of parent/a")
    );
}

#[test]
fn review_waits_on_its_reviewer_an_executor_under_it_or_nobody() {
    let board = [
        held(task("reviewed", "review"), "reviewer", "moss", Health::Live),
        held(task("alone", "review"), "executor", "teal", Health::Live),
        held(task("gone", "review"), "reviewer", "moss", Health::Lost),
        task("slices", "review"),
        under(
            held(
                task("slices/1", "in_progress"),
                "executor",
                "jade",
                Health::Live,
            ),
            "slices",
            1,
        ),
    ];
    assert_eq!(read(&board, "reviewed"), says(WORKING, "moss", "reviewer"));
    // Its own executor waits on review: no one reviews it.
    assert_eq!(read(&board, "alone"), says(NOBODY, "", "no reviewer"));
    assert_eq!(
        read(&board, "gone"),
        says(NOBODY, "moss", "reviewer moss lost")
    );
    assert_eq!(
        read(&board, "slices"),
        says(WORKING, "jade", "executor of slices/1")
    );
}

#[test]
fn work_not_started_waits_on_the_tasks_before_it_else_on_being_staffed() {
    let board = [
        depending(task("after", "ready"), &["first", "second", "landed"]),
        task("first", "in_progress"),
        task("second", "design"),
        task("landed", "done"),
        // Its dependency is a subtask of another, open.
        depending(task("on-a-slice", "design"), &["other/slice"]),
        task("other", "in_progress"),
        under(task("other/slice", "ready"), "other", 1),
        depending(task("free", "ready"), &["landed", "not-on-the-board"]),
        task("concept", "concept"),
        task("later", "backlog"),
        held(task("kept", "ready"), "reserved", "keen", Health::Live),
    ];
    assert_eq!(
        read(&board, "after"),
        says(TASK, "first, second", "its dependencies")
    );
    assert_eq!(
        read(&board, "on-a-slice"),
        says(TASK, "other/slice", "its dependency")
    );
    assert_eq!(
        read(&board, "free"),
        says(QUEUED, "", "waits to be staffed")
    );
    assert_eq!(
        read(&board, "concept"),
        says(QUEUED, "", "waits to be staffed")
    );
    assert_eq!(read(&board, "later"), says(QUEUED, "", "in the backlog"));
    assert_eq!(
        read(&board, "kept"),
        says(QUEUED, "keen", "reserved for keen")
    );
}

#[test]
fn a_cancelled_dependency_is_named_on_any_open_task_and_a_finished_one_waits_on_nothing() {
    let board = [
        depending(task("one", "blocked"), &["gone"]),
        depending(task("two", "ready"), &["gone", "also"]),
        task("gone", "cancelled"),
        task("also", "cancelled"),
        task("closed", "done"),
        task("dropped", "cancelled"),
    ];
    assert_eq!(
        read(&board, "one"),
        says(NOBODY, "gone", "its dependency gone was cancelled")
    );
    assert_eq!(
        read(&board, "two"),
        says(
            NOBODY,
            "gone, also",
            "its dependencies gone, also were cancelled"
        )
    );
    // The board's own block names its thing: it stays blocked on the wire.
    assert!(!line_of(&board, "one").unwrap().unnamed);
    assert_eq!(line_of(&board, "closed"), None);
    assert_eq!(line_of(&board, "dropped"), None);
}

#[test]
fn a_block_reads_what_its_record_names() {
    let board = [
        blocked_on(
            task("card", "blocked"),
            "decision:board-at-a-glance",
            Some("which layout"),
        ),
        blocked_on(
            task("card-bare", "blocked"),
            "decision:board-at-a-glance",
            None,
        ),
        blocked_on(task("nightly", "blocked"), "check:nightly", None),
        blocked_on(task("parked", "blocked"), "parked", Some("runs each month")),
        blocked_on(
            task("on-open", "blocked"),
            "task:open",
            Some("after its schema"),
        ),
        blocked_on(task("on-done", "blocked"), "task:landed", None),
        blocked_on(task("on-cancelled", "blocked"), "task:dropped", None),
        blocked_on(task("on-none", "blocked"), "task:nowhere", None),
        task("open", "in_progress"),
        task("landed", "done"),
        task("dropped", "cancelled"),
    ];
    assert_eq!(
        read(&board, "card"),
        says(DECISION, "board-at-a-glance", "which layout")
    );
    assert_eq!(
        read(&board, "card-bare"),
        says(DECISION, "board-at-a-glance", "the owner's decision")
    );
    assert_eq!(
        read(&board, "nightly"),
        says(CHECK, "nightly", "a check to give")
    );
    assert_eq!(read(&board, "parked"), says(PARKED, "", "runs each month"));
    assert_eq!(
        read(&board, "on-open"),
        says(TASK, "open", "after its schema")
    );
    assert_eq!(
        read(&board, "on-done"),
        says(NOBODY, "landed", "its dependency landed is done")
    );
    assert_eq!(
        read(&board, "on-cancelled"),
        says(NOBODY, "dropped", "its dependency dropped was cancelled")
    );
    assert_eq!(
        read(&board, "on-none"),
        says(
            NOBODY,
            "nowhere",
            "its dependency nowhere is not on the board"
        )
    );
    for slug in ["card", "nightly", "parked", "on-open", "on-done"] {
        assert!(!line_of(&board, slug).unwrap().unnamed, "{slug}");
    }
}

#[test]
fn a_block_on_the_owners_stamp_waits_on_his_decision_named_by_his_approve_never_by_him() {
    let board = [
        // The default landing's reason still names its tip.
        blocked_on(
            task("held", "blocked"),
            "check:owner",
            Some("the owner's approve of 0123abc"),
        ),
        blocked_on(task("held-bare", "blocked"), "check:owner", None),
        blocked_on(
            task("manual", "blocked"),
            "check:owner",
            Some("review the sketch"),
        ),
        // Another stamp is a check still.
        blocked_on(task("owners", "blocked"), "check:owners", None),
    ];
    assert_eq!(
        read(&board, "held"),
        says(DECISION, OWNER_APPROVE, "the owner's approve of 0123abc")
    );
    assert_eq!(
        read(&board, "held-bare"),
        says(DECISION, OWNER_APPROVE, "the owner's approval")
    );
    assert_eq!(
        read(&board, "manual"),
        says(DECISION, OWNER_APPROVE, "review the sketch")
    );
    assert_eq!(
        read(&board, "owners"),
        says(CHECK, "owners", "a check to give")
    );
    assert_eq!(
        said(&line_of(&board, "held").unwrap(), 300.0),
        "Waits: on decision: the owner's approve of its tip · the owner's approve of 0123abc · 5 min"
    );
}

#[test]
fn a_block_that_names_nothing_reads_no_reason_and_is_not_blocked_on_the_wire() {
    let board = [
        task("bare", "blocked"),
        blocked_on(task("unknown-thing", "blocked"), "someone", None),
        // A record left on a task no longer blocked is no block's.
        blocked_on(task("moved-on", "ready"), "parked", None),
    ];
    let bare = line_of(&board, "bare").unwrap();
    assert_eq!(
        (bare.kind, bare.why.as_str(), bare.unnamed),
        (NOBODY, NO_REASON, true)
    );
    assert!(line_of(&board, "unknown-thing").unwrap().unnamed);
    assert_eq!(
        read(&board, "moved-on"),
        says(QUEUED, "", "waits to be staffed")
    );
    assert_eq!(Blocked::of(&board[2]), None);
}

/// The landing's record: `TIP: names`, as the tool that lands it writes it.
fn landing(t: Task, waits: &str) -> Task {
    board_field(t, alarm::WAITS, json!(waits))
}

#[test]
fn a_landing_record_reads_as_the_checks_it_lacks_changes_as_its_executors_work() {
    let board = [
        landing(task("lacks", "review"), "0123abc: certificate, pictures"),
        landing(task("review-too", "review"), "0123abc: certificate, review"),
        landing(task("none-missing", "review"), "0123abc:"),
        landing(
            held(task("changes", "review"), "executor", "teal", Health::Live),
            "0123abc: changes",
        ),
        landing(task("changes-alone", "review"), "0123abc: changes"),
        // A consumer's landing held it blocked for what it lacks.
        landing(task("held", "blocked"), "0123abc: certificate"),
        landing(task("held-for-nothing", "blocked"), "0123abc: changes"),
    ];
    assert_eq!(
        read(&board, "lacks"),
        says(CHECK, "certificate, pictures", "its landing lacks them")
    );
    assert_eq!(read(&board, "review-too"), says(NOBODY, "", "no reviewer"));
    assert_eq!(
        read(&board, "none-missing"),
        says(QUEUED, "", "nothing missing: it waits to land")
    );
    assert_eq!(
        read(&board, "changes"),
        says(WORKING, "teal", "executor, making the changes asked")
    );
    assert_eq!(
        read(&board, "changes-alone"),
        says(NOBODY, "", "no executor to make the changes asked")
    );
    assert_eq!(
        read(&board, "held"),
        says(CHECK, "certificate", "its landing lacks them")
    );
    let bare = line_of(&board, "held-for-nothing").unwrap();
    assert!(bare.unnamed, "{bare:?}");
}

#[test]
fn a_landing_that_lacks_the_owners_stamp_alone_waits_on_his_decision() {
    let board = [
        landing(task("in-review", "review"), "0123abc: owner"),
        // A consumer's landing held it blocked for the owner's stamp alone.
        landing(task("held", "blocked"), "0123abc: owner"),
        // With another stamp still to give, a check comes first.
        landing(task("and-tests", "review"), "0123abc: owner, tests"),
    ];
    for slug in ["in-review", "held"] {
        assert_eq!(
            read(&board, slug),
            says(DECISION, OWNER_APPROVE, "its landing lacks it"),
            "{slug}"
        );
    }
    assert_eq!(
        read(&board, "and-tests"),
        says(CHECK, "owner, tests", "its landing lacks them")
    );
}

#[test]
fn a_landing_record_of_an_older_tip_says_so_and_answered_changes_ask_nothing() {
    let older = landing(task("older", "review"), "0123abc: certificate");
    let look_at = |t: &Task, tip: Option<&str>| {
        let board = std::slice::from_ref(t);
        let look = Look::new(board, &[]);
        line(t, &look, Landing::of(t, tip).as_ref()).unwrap()
    };
    let line = look_at(&older, Some("4567def"));
    assert_eq!(
        (line.kind, line.who.as_str(), line.why.as_str()),
        (
            CHECK,
            "certificate",
            "its tip is newer than its landing's last look"
        )
    );
    assert_eq!(
        look_at(&older, Some("0123abc")).why,
        "its landing lacks them"
    );
    // Changes the branch answered, as the board keeps the answer: it waits on review again.
    let answered = board_field(
        board_field(
            landing(task("answered", "review"), "0123abc: changes"),
            alarm::ASSESSED,
            json!({"id": "a-1", "at": 4.0, "sid": 9}),
        ),
        alarm::FIXED,
        json!({"0123abc": {"line:9": {"commit": "4567def", "at": 6.0}}}),
    );
    let read = Landing::of(&answered, None).unwrap();
    assert!(!read.reworking && read.names.is_empty(), "{read:?}");
    assert_eq!(
        look_at(&answered, None).why,
        "nothing missing: it waits to land"
    );
}

#[test]
fn a_thing_is_one_of_four_each_named_by_one_word() {
    assert_eq!(
        Thing::parse("decision:a-card"),
        Ok(Thing::Decision("a-card"))
    );
    assert_eq!(Thing::parse(" check:nightly "), Ok(Thing::Check("nightly")));
    assert_eq!(Thing::parse("task:tools/x"), Ok(Thing::Task("tools/x")));
    assert_eq!(Thing::parse("parked"), Ok(Thing::Parked));
    for thing in ["decision:a-card", "check:nightly", "task:tools/x", "parked"] {
        assert_eq!(Thing::parse(thing).unwrap().to_string(), thing);
    }
    assert_eq!(Thing::parse("task:tools/x").unwrap().name(), "tools/x");
    assert_eq!(Thing::Parked.name(), "");
    for bad in [
        "",
        "owner",
        "amber-heron",
        "decision",
        "decision:",
        "decision:two words",
        "check:a,b",
        "person:amber",
        "parked:long",
    ] {
        let refused = Thing::parse(bad).unwrap_err();
        assert!(refused.contains(THINGS), "{bad}: {refused}");
    }
}

#[test]
fn a_why_is_a_few_words() {
    assert_eq!(why("  the owner's call "), Ok("the owner's call"));
    assert_eq!(
        why(&"é".repeat(WHY_LIMIT)),
        Ok("é".repeat(WHY_LIMIT).as_str())
    );
    let long = why(&"a".repeat(WHY_LIMIT + 1)).unwrap_err();
    assert!(long.contains("120") && long.contains("121"), "{long}");
    assert!(why("   ").unwrap_err().contains("words"));
}

#[test]
fn the_record_reads_back_as_written_and_a_stray_one_reads_as_none() {
    let record = Blocked {
        on: "check:nightly".into(),
        why: None,
        by: "amber-heron".into(),
        at: 5.0,
        from: None,
    };
    assert_eq!(
        record.value(),
        json!({"on": "check:nightly", "by": "amber-heron", "at": 5.0})
    );
    let t = board_field(task("t", "blocked"), BLOCKED, record.value());
    assert_eq!(Blocked::of(&t), Some(record));
    let stray = board_field(task("t", "blocked"), BLOCKED, json!("on nothing"));
    assert_eq!(Blocked::of(&stray), None);
}

#[test]
fn the_board_orchestrators_are_each_once_with_their_health_and_none_of_closed_work() {
    let board = [
        held(
            task("a", "ready"),
            "orchestrator",
            "sable-finch",
            Health::Stale,
        ),
        held(
            task("b", "in_progress"),
            "orchestrator",
            "sable-finch",
            Health::Stale,
        ),
        held(
            task("c", "blocked"),
            "orchestrator",
            "glacial-heron",
            Health::Left,
        ),
        held(
            task("d", "done"),
            "orchestrator",
            "quiet-plover",
            Health::Live,
        ),
        held(task("e", "review"), "executor", "teal", Health::Live),
    ];
    assert_eq!(
        orchestrators(&board),
        [
            ("glacial-heron".to_string(), Health::Left),
            ("sable-finch".to_string(), Health::Stale)
        ]
    );
    assert_eq!(orchestrators(&[]), []);
}

#[test]
fn the_ready_tasks_waiting_to_start_are_pulls_unstaffed_ones() {
    let board = [
        task("waits", "ready"),
        held(task("staffed", "ready"), "executor", "teal", Health::Live),
        held(task("lost", "ready"), "executor", "plum", Health::Lost),
        depending(task("after", "ready"), &["open"]),
        depending(task("after-cancel", "ready"), &["dropped"]),
        task("open", "in_progress"),
        task("dropped", "cancelled"),
        task("design", "design"),
    ];
    assert_eq!(Look::new(&board, &[]).unstarted(&board), 2);
}

#[test]
fn the_show_line_says_kind_who_why_and_age() {
    let at_work = Line::new(WORKING, "amber-heron", "executor");
    assert_eq!(
        said(&at_work, 300.0),
        "Waits: on working: amber-heron · executor · 5 min"
    );
    let gone = Line::new(NOBODY, "amber-heron", "executor amber-heron lost");
    assert_eq!(
        said(&gone, 2.0 * 86_400.0),
        "Waits: on nobody · executor amber-heron lost · 2 d"
    );
    assert_eq!(
        said(&Line::new(QUEUED, "", "waits to be staffed"), 42.0),
        "Waits: on queued · waits to be staffed · 42 s"
    );
    assert_eq!(age(-3.0), "0 s");
    assert_eq!(age(5.0 * 3600.0), "5 h");
    assert_eq!(age(36.0 * 3600.0), "2 d");
}

#[test]
fn show_omits_who_only_when_the_reason_names_it_in_full() {
    for (who, why) in [
        ("art", "start after review"),
        ("build", "rebuild the client"),
        ("card", "card-longer was answered"),
        ("card", "parent/card is pending"),
        ("card", "card.md is pending"),
        ("card", "card_name is pending"),
        ("café", "décaféiné"),
    ] {
        assert_eq!(
            said(&Line::new(CHECK, who, why), 0.0),
            format!("Waits: on check: {who} · {why} · 0 s")
        );
    }
    for why in [
        "card",
        "waiting on card",
        "card: pending",
        "(card) is pending",
        "card-longer then card",
    ] {
        assert_eq!(
            said(&Line::new(DECISION, "card", why), 0.0),
            format!("Waits: on decision · {why} · 0 s")
        );
    }
}

#[test]
fn the_kinds_are_the_wires() {
    let mut kinds = [WORKING, DECISION, CHECK, TASK, QUEUED, PARKED, NOBODY];
    crate::order::sort_by(&mut kinds, &Ord::cmp);
    let mut wire = symphony_api::WAITS_KINDS;
    crate::order::sort_by(&mut wire, &Ord::cmp);
    assert_eq!(kinds, wire);
}
