//! Rework on hand-written records and lines: what a record of what a landing waits on holds and
//! judges, when a record of changes stands from and until, and review so judged counted in
//! progress.

use super::*;
use crate::alarm::tests::{line, waits_line};

#[test]
fn a_waits_record_is_the_tip_then_the_names_and_none_without_a_tip() {
    assert_eq!(
        Waits::parse("0123abc: certificate, review"),
        Some(Waits {
            tip: "0123abc",
            on: vec!["certificate", "review"],
        })
    );
    assert_eq!(
        Waits::parse(" 0123abc :"),
        Some(Waits {
            tip: "0123abc",
            on: vec![],
        }),
        "waits on nothing"
    );
    assert_eq!(
        Waits::parse("0123abc: review,, changes ").map(|w| w.on),
        Some(vec!["review", "changes"])
    );
    for text in ["", "certificate, review", ": review", "two words: review"] {
        assert_eq!(Waits::parse(text), None, "{text:?}");
    }
}

#[test]
fn a_waits_record_judges_the_tip_it_names_and_any_when_none_is_known() {
    let waits = Waits::parse("0123abc: changes").unwrap();
    let tips = [Some("0123abc"), Some("4567def"), None];
    assert_eq!(tips.map(|tip| waits.judged(tip)), [true, false, true]);
    let records = ["0123abc: changes", "0123abc: review"];
    let changes = records.map(|text| Waits::parse(text).is_some_and(|w| w.changes()));
    assert_eq!(changes, [true, false]);
}

/// The rework flags over a window from 0, opened with `opened` standing (when it was written, and
/// its text), the branch first moved past each tip of `fixed` after the record of it written then
/// when it says.
fn flags(lines: &[Line], opened: Option<(f64, &str)>, fixed: &[Ended]) -> Vec<(f64, bool)> {
    let lines: Vec<&Line> = lines.iter().collect();
    let opening_line = opened.map(|(written, text)| waits_line(99, written, text));
    let opened = opening_line.as_ref().and_then(Recorded::of);
    let fixed =
        |asked: Written<'_>| (fixed.iter()).find_map(|end| (end.record == asked).then_some(end.at));
    reworked(&lines, opened.as_ref(), &fixed, 0.0)
}

/// When the branch first moved past the tip of a record of changes after it was written.
struct Ended {
    record: Written<'static>,
    at: f64,
}

/// The branch first moved past `tip` at `at`, after the record of it written at `written`.
fn ended(sid: i64, tip: &'static str, written: f64, at: f64) -> Ended {
    let record = Written {
        tip,
        at: written,
        sid,
    };
    Ended { record, at }
}

fn moved(sid: i64, ts: f64, text: &str) -> Line {
    line("sim/a", sid, ts, "system", text)
}

#[test]
fn rework_runs_from_a_record_of_changes_to_the_next_record_through_changes_of_status() {
    let lines = [
        waits_line(1, 10.0, "aaa: review"),
        waits_line(2, 20.0, "aaa: changes"),
        moved(3, 30.0, "w moved it review → in_progress"),
        moved(4, 40.0, "w moved it in_progress → review"),
        waits_line(5, 50.0, "bbb: changes, certificate"),
        waits_line(6, 60.0, "ccc: certificate"),
        line(
            "sim/a",
            7,
            70.0,
            "msg",
            "w set _board.waits to ddd: changes",
        ),
    ];
    assert_eq!(
        flags(&lines, None, &[]),
        [
            (0.0, false),
            (10.0, false),
            (20.0, true),
            (50.0, true),
            (60.0, false),
        ],
        "only a system line records the field, and a move into progress and back keeps the tip"
    );
    // The branch went past aaa at 45 and past bbb at 55: each record of changes ends then.
    assert_eq!(
        flags(
            &lines,
            None,
            &[ended(2, "aaa", 20.0, 45.0), ended(5, "bbb", 50.0, 55.0)]
        ),
        [
            (0.0, false),
            (10.0, false),
            (20.0, true),
            (45.0, false),
            (50.0, true),
            (55.0, false),
            (60.0, false),
        ]
    );
}

#[test]
fn a_record_of_changes_stands_until_the_branch_first_moved_past_the_tip_it_judged() {
    let bounced = [
        waits_line(1, 10.0, "aaa: changes"),
        moved(2, 20.0, "w moved it review → in_progress"),
        moved(3, 30.0, "w moved it in_progress → review"),
    ];
    assert_eq!(
        flags(&bounced, None, &[]),
        [(0.0, false), (10.0, true)],
        "the tip it judged is the tip now"
    );
    assert_eq!(
        flags(&bounced, None, &[ended(1, "aaa", 10.0, 100.0)]),
        [(0.0, false), (10.0, true), (100.0, false)]
    );
    assert_eq!(
        flags(&bounced, None, &[ended(1, "aaa", 10.0, 5.0)]),
        [(0.0, false), (10.0, true), (10.0, false)],
        "a commit made before the record: it never stood"
    );
}

#[test]
fn a_record_of_changes_standing_when_the_window_opened_stands_until_what_ends_it() {
    let asked = Some((-10.0, "aaa: changes"));
    assert_eq!(flags(&[], asked, &[]), [(0.0, true)]);
    assert_eq!(flags(&[], None, &[]), [(0.0, false)]);
    // The branch went past aaa at 20, after the window opened, or at -5, before it; the record
    // on the newer tip written at 50 ends nothing then.
    let refreshed = [waits_line(1, 50.0, "bbb: certificate, review")];
    assert_eq!(
        flags(&refreshed, asked, &[ended(99, "aaa", -10.0, 20.0)]),
        [(0.0, true), (20.0, false), (50.0, false)]
    );
    assert_eq!(
        flags(&refreshed, asked, &[ended(99, "aaa", -10.0, -5.0)]),
        [(0.0, true), (0.0, false), (50.0, false)]
    );
    assert_eq!(
        flags(&refreshed, asked, &[]),
        [(0.0, true), (50.0, false)],
        "with no tip known, the next record ends it"
    );
    assert_eq!(
        flags(&[], asked, &[ended(99, "aaa", 0.0, 5.0)]),
        [(0.0, true)],
        "the record is the one written at -10, not one at the window's start"
    );
}

#[test]
fn two_records_of_changes_on_one_tip_each_end_when_the_branch_moved_past_it_after_that_one() {
    let returned = [
        waits_line(1, 10.0, "aaa: changes"),
        waits_line(2, 30.0, "aaa: changes, review"),
    ];
    // The branch went past aaa at 20, after the first, then came back to it: the second waits.
    assert_eq!(
        flags(&returned, None, &[ended(1, "aaa", 10.0, 20.0)]),
        [(0.0, false), (10.0, true), (20.0, false), (30.0, true)]
    );
    assert_eq!(
        flags(
            &returned,
            None,
            &[ended(1, "aaa", 10.0, 20.0), ended(2, "aaa", 30.0, 40.0)]
        ),
        [
            (0.0, false),
            (10.0, true),
            (20.0, false),
            (30.0, true),
            (40.0, false),
        ]
    );
}

#[test]
fn review_under_a_record_of_changes_counts_in_progress_and_nothing_else_moves() {
    let status = [
        (0.0, Some("ready")),
        (10.0, Some("in_progress")),
        (20.0, Some("review")),
        (50.0, Some("done")),
    ];
    let reworked = [
        (0.0, true),
        (30.0, true),
        (40.0, false),
        (40.0, true),
        (45.0, false),
    ];
    assert_eq!(
        as_worked(&status, &reworked),
        [
            (0.0, Some("ready")),
            (10.0, Some("in_progress")),
            (45.0, Some("review")),
            (50.0, Some("done")),
        ],
        "rework before review changes nothing; the later of two flags at 40 stands"
    );
    let none = [(0.0, false)];
    assert_eq!(as_worked(&status, &none), status.to_vec());
}

#[test]
fn equal_time_records_keep_their_sequence_and_a_malformed_record_ends_the_previous_one() {
    let returned = [
        waits_line(1, 10.0, "aaa: changes"),
        waits_line(2, 10.0, "aaa: changes"),
    ];
    let mut asked = Vec::new();
    let fixed = |record: Written<'_>| {
        // The real cache uses the occurrence identity; only the first request was answered.
        (record.sid == 1).then_some(10.0)
    };
    let refs: Vec<_> = returned.iter().collect();
    assert_eq!(
        reworked(&refs, None, &fixed, 0.0),
        [(0.0, false), (10.0, true), (10.0, true)]
    );
    for line in &returned {
        asked.push(Recorded::of(line).unwrap().written.sid);
    }
    assert_eq!(asked, [1, 2]);
    let malformed = [
        waits_line(1, 10.0, "aaa: changes"),
        waits_line(2, 20.0, "no tip"),
    ];
    assert_eq!(
        flags(&malformed, None, &[]),
        [(0.0, false), (10.0, true), (20.0, false)]
    );
    for line in [
        waits_line(0, 1.0, "aaa: changes"),
        waits_line(1, f64::NAN, "aaa: changes"),
    ] {
        assert_eq!(Recorded::of(&line), None, "{line:?}");
    }
}
