//! What a task's landing waits on, the board's own field `fields._board.waits`, on the real store:
//! what a record keeps beside the task's other fields (the assessment it states), the line it
//! writes for the pipeline alarm, a record retried and one of a fresh assessment, and what it
//! refuses; where the rework a record of changes asked for ended, kept once a record; and the
//! alarm's reads of a thread back from a moment, which no number of field values reading like the
//! board's lines misleads.

use super::common::{Fixture, Seed, refused};
use serde_json::json;
use symphony_board::alarm::{Assessed, Tip, Written, kept_fix};

mod checked;

#[test]
fn a_record_is_kept_among_the_board_s_fields_and_said_on_the_thread_as_the_alarm_reads_it() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f.store
        .set_field("sim/jump", "game", "effort", &json!(3), "brisk-finch")
        .unwrap();
    let record = "0123abc: certificate, review";
    let task = (f.store)
        .record_waits("sim/jump", record, "look-1", "brisk-finch")
        .unwrap();
    let sid = f.store.status_lines(f.now()).unwrap().last().unwrap().sid;
    let assessed = json!({"id": "look-1", "at": f.now(), "sid": sid});
    assert_eq!(
        task.fields,
        [
            (
                "_board".into(),
                [
                    ("assessed".into(), assessed),
                    (
                        "assessments".into(),
                        json!({"look-1": {record: {"at": f.now(), "sid": sid}}})
                    ),
                    ("waits".into(), json!(record)),
                ]
                .into()
            ),
            ("game".into(), [("effort".into(), json!(3))].into()),
        ]
        .into()
    );
    assert_eq!(
        f.said("sim/jump").last().map(String::as_str),
        Some("brisk-finch set _board.waits to 0123abc: certificate, review")
    );
    f.advance(1.0);
    let next = (f.store)
        .record_waits("sim/jump", "4567def:", "look-2", "owner")
        .unwrap();
    assert_eq!(next.fields["_board"]["waits"], json!("4567def:"));
    let assessed = Assessed {
        id: "look-2".into(),
        at: f.now(),
        sid: Some(f.store.status_lines(f.now()).unwrap().last().unwrap().sid),
    };
    assert_eq!(Assessed::of(&next), Some(assessed));
    assert_eq!(
        f.said("sim/jump").last().map(String::as_str),
        Some("owner set _board.waits to 4567def:"),
        "a record of nothing missing"
    );
}

#[test]
fn a_retry_of_a_record_writes_nothing_and_a_fresh_assessment_of_the_same_text_writes_it_again() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let write = |assessment: &str| {
        (f.store)
            .record_waits("sim/jump", "0123abc: changes", assessment, "brisk-finch")
            .unwrap()
    };
    let first = write("verdict-1");
    let (lines, written) = (f.said("sim/jump").len(), f.now());
    f.advance(1.0);
    let again = write("verdict-1");
    assert_eq!(
        (again.updated, f.said("sim/jump").len()),
        (first.updated, lines),
        "a retry"
    );
    assert_eq!(Assessed::of(&again).map(|a| a.at), Some(written));
    f.advance(1.0);
    let fresh = write("verdict-2");
    let said = f.said("sim/jump");
    assert_eq!(said.len(), lines + 1, "a record of its own: {said:?}");
    assert_eq!(
        said.last().map(String::as_str),
        Some("brisk-finch set _board.waits to 0123abc: changes")
    );
    let assessed = Assessed {
        id: "verdict-2".into(),
        at: f.now(),
        sid: Some(f.store.status_lines(f.now()).unwrap().last().unwrap().sid),
    };
    assert_eq!(Assessed::of(&fresh), Some(assessed));
}

#[test]
fn a_record_without_a_tip_or_of_no_task_is_refused() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    for record in ["certificate, review", ": review", "two words: review", ""] {
        let why = refused(
            f.store
                .record_waits("sim/jump", record, "look", "brisk-finch"),
        );
        assert!(
            why.contains("a landing's record is its tip"),
            "{record:?}: {why}"
        );
    }
    let why = refused(
        f.store
            .record_waits("sim/none", "0123abc: owner", "look", "brisk-finch"),
    );
    assert!(why.contains("sim/none"), "{why}");
    // A plugin's field write leaves the board's own to this.
    let why = refused(f.store.set_field(
        "sim/jump",
        "_board",
        "waits",
        &json!("0123abc: owner"),
        "brisk-finch",
    ));
    assert!(why.contains("a landing's record by record_waits"), "{why}");
    assert!(f.store.get("sim/jump").unwrap().fields.is_empty());
    let why = refused(
        f.store
            .record_waits("sim/jump", "aaa: review", " ", "brisk-finch"),
    );
    assert!(why.contains("assessment must have an identity"), "{why}");
}

#[test]
fn where_a_record_of_changes_ended_is_kept_once_a_record_and_nothing_later_moves_it() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f.store
        .record_waits("sim/jump", "aaa: changes", "look", "brisk-finch")
        .unwrap();
    let (lines, updated) = (
        f.said("sim/jump").len(),
        f.store.get("sim/jump").unwrap().updated,
    );
    f.advance(1.0);
    let tip = |commit: &str, at: f64| Tip {
        commit: commit.into(),
        at,
    };
    let keep = |judged: &str, written: f64, first: &Tip| {
        let record = Written {
            tip: judged,
            at: written,
            sid: (written * 100.0) as i64,
        };
        f.store.record_fixed("sim/jump", record, first).unwrap()
    };
    assert_eq!(keep("aaa", 5.25, &tip("bbb", 10.0)), Some(tip("bbb", 10.0)));
    assert_eq!(
        keep("aaa", 5.25, &tip("ccc", 20.0)),
        Some(tip("bbb", 10.0)),
        "the first kept stands"
    );
    assert_eq!(
        keep("aaa", 40.5, &tip("ddd", 50.0)),
        Some(tip("ddd", 50.0)),
        "a later record of the same tip keeps its own"
    );
    assert_eq!(
        keep("ccc", 5.25, &tip("eee", 30.0)),
        Some(tip("eee", 30.0)),
        "another tip's own"
    );
    let task = f.store.get("sim/jump").unwrap();
    let records = [
        ("aaa", 5.25),
        ("aaa", 40.5),
        ("ccc", 5.25),
        ("aaa", 5.0),
        ("fff", 5.25),
    ];
    let kept = records.map(|(tip, at)| {
        kept_fix(
            &task,
            Written {
                tip,
                at,
                sid: (at * 100.0) as i64,
            },
        )
    });
    let (bbb, ddd, eee) = (tip("bbb", 10.0), tip("ddd", 50.0), tip("eee", 30.0));
    assert_eq!(kept, [Some(bbb), Some(ddd), Some(eee), None, None]);
    assert_eq!(
        task.fields["_board"]["waits"],
        json!("aaa: changes"),
        "beside the record"
    );
    assert_eq!(
        (f.said("sim/jump").len(), task.updated),
        (lines, updated),
        "no line, and the task not updated"
    );
    let none = Written {
        tip: "aaa",
        at: 5.25,
        sid: 525,
    };
    let kept = f.store.record_fixed("sim/none", none, &tip("bbb", 10.0));
    assert_eq!(kept.unwrap(), None);
}

#[test]
fn a_thread_s_newest_change_of_status_and_newest_record_before_a_moment_are_read_in_order() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    for slug in ["sim/jump", "sim/quiet"] {
        f.task(slug);
    }
    f.store.claim("sim/jump", "brisk-finch").unwrap();
    for n in 0..5 {
        f.advance(1.0);
        let record = format!("tip{n}: review");
        f.store
            .record_waits("sim/jump", &record, "look", "brisk-finch")
            .unwrap();
    }
    f.store
        .set_field("sim/jump", "game", "note", &json!("x"), "brisk-finch")
        .unwrap();
    f.advance(1.0);
    let since = f.now();
    f.store
        .move_to("sim/jump", "review", "brisk-finch")
        .unwrap();
    let before = f
        .store
        .lines_before(&["sim/jump", "sim/quiet"], since)
        .unwrap();
    let said: Vec<&str> = before["sim/jump"].iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        said,
        [
            "brisk-finch claimed it",
            "brisk-finch set _board.waits to tip4: review",
        ],
        "the claim, the newest record, nothing from the moment on and no other field"
    );
    assert!(!before.contains_key("sim/quiet"), "a thread that says none");
    assert!(f.store.lines_before(&[], since).unwrap().is_empty());
}

#[test]
fn field_values_that_read_like_the_board_s_lines_hide_none_older_however_many() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f.store.claim("sim/jump", "brisk-finch").unwrap();
    f.store
        .record_waits("sim/jump", "tip0: changes", "look", "brisk-finch")
        .unwrap();
    f.advance(1.0);
    let entered = f.now();
    f.store
        .move_to("sim/jump", "review", "brisk-finch")
        .unwrap();
    // More than two reads' worth of field values, each reading like a record and a move into review.
    for n in 0..9 {
        f.advance(1.0);
        let note = format!("w set _board.waits to tip{n}: review; w moved it in_progress → review");
        f.store
            .set_field("sim/jump", "game", "note", &json!(note), "brisk-finch")
            .unwrap();
    }
    f.advance(1.0);
    let before = f.store.lines_before(&["sim/jump"], f.now()).unwrap();
    let said: Vec<&str> = before["sim/jump"].iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        said,
        [
            "brisk-finch set _board.waits to tip0: changes",
            "brisk-finch moved it in_progress → review",
        ]
    );
    let into = f.store.entered_review(&["sim/jump"]).unwrap();
    assert_eq!(into.get("sim/jump"), Some(&entered));
}

#[test]
fn overlapping_duplicate_preserves_the_committed_occurrence_and_its_time() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let first_at = f.now();
    let first = symphony_board::Store::unchecked_on(
        f.other_database(),
        Box::new(move || first_at),
        Box::new(|_| false),
        crate::common::HOST,
        &symphony_board::Opening::write(),
    );
    f.advance(1.0);
    // The competing write commits after this caller starts, before it can mutate the board.
    let late = f.late(move || {
        first
            .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
            .unwrap();
    });
    let before = f.said("sim/jump").len();
    let task = late
        .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
        .unwrap();
    assert_eq!(f.said("sim/jump").len(), before + 1);
    let assessed = Assessed::of(&task).unwrap();
    assert_eq!(assessed.at, first_at);
    assert_eq!(
        assessed.sid,
        Some(f.store.status_lines(first_at).unwrap().last().unwrap().sid)
    );
}

#[test]
fn independent_occurrences_at_one_time_keep_separate_ends() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let first_at = f.now() + 1.0;
    let end = Tip {
        commit: "bbb".into(),
        at: first_at + 1.0,
    };
    let writer = symphony_board::Store::unchecked_on(
        f.other_database(),
        Box::new(move || first_at),
        Box::new(|_| false),
        crate::common::HOST,
        &symphony_board::Opening::write(),
    );
    // A slow caller reads its earlier time, then another writer commits at a later time. logged
    // permits that timestamp to tie; the later verdict must still be a distinct occurrence.
    let saved = end.clone();
    let slow = f.slow(move || {
        let task = writer
            .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
            .unwrap();
        let first = Assessed::of(&task).unwrap();
        let record = Written {
            tip: "aaa",
            at: first.at,
            sid: first.sid.unwrap(),
        };
        writer.record_fixed("sim/jump", record, &saved).unwrap();
    });
    let task = slow
        .record_waits("sim/jump", "aaa: changes", "verdict-2", "brisk-finch")
        .unwrap();
    let history = f.store.status_lines(f.now()).unwrap();
    let record = history
        .iter()
        .find_map(symphony_board::alarm::Recorded::of)
        .unwrap()
        .written;
    let second = Assessed::of(&task).unwrap();
    assert_eq!(
        (record.at, second.at),
        (first_at, first_at),
        "timestamps are permitted to tie"
    );
    assert_ne!(Some(record.sid), second.sid);
    let next = Written {
        tip: "aaa",
        at: second.at,
        sid: second.sid.unwrap(),
    };
    assert_eq!(kept_fix(&task, record), Some(end.clone()));
    assert_eq!(
        kept_fix(&task, next),
        None,
        "a fresh verdict inherits no end"
    );
    let later = Tip {
        commit: "ccc".into(),
        at: f.now() + 2.0,
    };
    f.store.record_fixed("sim/jump", next, &later).unwrap();
    let task = f.store.get("sim/jump").unwrap();
    assert_eq!(kept_fix(&task, record), Some(end));
    assert_eq!(kept_fix(&task, next), Some(later));
    let history = f.store.status_lines(f.now()).unwrap();
    let records: Vec<_> = history
        .iter()
        .filter_map(symphony_board::alarm::Recorded::of)
        .collect();
    assert_eq!(
        records.iter().map(|line| line.written).collect::<Vec<_>>(),
        [record, next]
    );
}

#[test]
fn malformed_occurrences_and_ends_are_refused_without_writing() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let record = Written {
        tip: "aaa",
        at: f.now(),
        sid: 1,
    };
    let before = f.store.get("sim/jump").unwrap().fields;
    let invalid = [
        (
            Written { sid: 0, ..record },
            Tip {
                commit: "bbb".into(),
                at: f.now(),
            },
        ),
        (
            Written {
                at: f64::NAN,
                ..record
            },
            Tip {
                commit: "bbb".into(),
                at: f.now(),
            },
        ),
        (
            record,
            Tip {
                commit: " ".into(),
                at: f.now(),
            },
        ),
        (
            record,
            Tip {
                commit: "bbb".into(),
                at: f64::INFINITY,
            },
        ),
    ];
    for (record, end) in invalid {
        assert_eq!(kept_fix(&f.store.get("sim/jump").unwrap(), record), None);
        let why = refused(f.store.record_fixed("sim/jump", record, &end));
        assert!(why.contains("invalid rework occurrence or end"), "{why}");
    }
    assert_eq!(f.store.get("sim/jump").unwrap().fields, before);
}

#[test]
fn simultaneous_duplicates_and_an_old_retry_after_a_fresh_verdict_write_once_each() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let before = f.said("sim/jump").len();
    let start = std::sync::Barrier::new(4);
    let stores: Vec<_> = (0..4).map(|_| f.other()).collect();
    std::thread::scope(|scope| {
        let calls: Vec<_> = stores
            .iter()
            .map(|store| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    store
                        .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = calls
            .into_iter()
            .map(|call| Assessed::of(&call.join().unwrap()))
            .collect();
        assert_eq!(results, vec![results[0].clone(); 4]);
    });
    assert_eq!(f.said("sim/jump").len(), before + 1);
    f.advance(1.0);
    let fresh = f
        .store
        .record_waits("sim/jump", "aaa: changes", "verdict-2", "brisk-finch")
        .unwrap();
    f.advance(1.0);
    let retry = f
        .store
        .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
        .unwrap();
    assert_eq!(
        retry.fields, fresh.fields,
        "an old receipt cannot replace a newer assessment"
    );
    assert_eq!(retry.updated, fresh.updated);
    assert_eq!(f.said("sim/jump").len(), before + 2);
    // An acknowledged receipt is still a no-write retry after its original writer leaves.
    f.store.leave("brisk-finch").unwrap();
    let after_leave = f.said("sim/jump").len();
    let retry = f
        .store
        .record_waits("sim/jump", "aaa: changes", "verdict-1", "brisk-finch")
        .unwrap();
    assert_eq!(retry.fields, fresh.fields);
    assert_eq!(f.said("sim/jump").len(), after_leave);
}
