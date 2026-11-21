//! The replay of a thread, each case's expected times written out by hand.

use super::*;

/// When every case's task was made.
pub(super) const T0: f64 = 1_000_000.0;
pub(super) const H: f64 = 3600.0;

fn line(id: i64, kind: &str, hours: f64, text: &str) -> Line {
    Line {
        id,
        kind: kind.into(),
        text: text.into(),
        ts: T0 + hours * H,
        sender: String::new(),
    }
}

fn sys(id: i64, hours: f64, text: &str) -> Line {
    line(id, "system", hours, text)
}

fn verdict(id: i64, hours: f64, text: &str) -> Line {
    line(id, "verdict", hours, text)
}

/// A verdict `by` a reviewer.
fn verdict_by(id: i64, hours: f64, text: &str, by: &str) -> Line {
    Line {
        sender: by.into(),
        ..verdict(id, hours, text)
    }
}

/// A stretch in `status` from `from` hours in to `to`, `by` whom.
fn segment(status: &str, from: f64, to: Option<f64>, by: Option<&str>) -> Segment {
    Segment {
        status: status.into(),
        from: at(from),
        to: to.map(at),
        by: by.map(str::to_string),
    }
}

/// A task made at `T0`, last written `updated` hours later.
fn facts(status: &str, updated: f64) -> Facts<'_> {
    Facts {
        status,
        created: T0,
        updated: T0 + updated * H,
    }
}

/// A stage first entered `entered` hours in, held `held` hours over `visits` visits.
pub(super) fn stage(status: &str, entered: f64, held: f64, visits: u32) -> Stage {
    Stage {
        status: status.into(),
        first_entered: T0 + entered * H,
        seconds: held * H,
        visits,
    }
}

pub(super) fn at(hours: f64) -> f64 {
    T0 + hours * H
}

/// Made ready, claimed after half an hour, in review at two hours, approved at three.
fn straight_through() -> Vec<Line> {
    vec![
        sys(1, 0.0, "created by owner"),
        sys(2, 0.0, "amber joined as orchestrator"),
        sys(3, 0.5, "teal claimed it"),
        line(4, "msg", 1.0, "teal moved it in_progress → blocked"),
        sys(5, 2.0, "teal moved it in_progress → review"),
        sys(6, 2.5, "plum joined as reviewer"),
        verdict_by(7, 3.0, "approve: fine", "plum"),
    ]
}

pub(super) fn straight_through_stages() -> Stages {
    Stages {
        stages: vec![
            stage("ready", 0.0, 0.5, 1),
            stage("in_progress", 0.5, 1.5, 1),
            stage("review", 2.0, 1.0, 1),
            stage("done", 3.0, 0.0, 1),
        ],
        segments: vec![
            segment("ready", 0.0, Some(0.5), Some("owner")),
            segment("in_progress", 0.5, Some(2.0), Some("teal")),
            segment("review", 2.0, Some(3.0), Some("teal")),
            segment("done", 3.0, None, Some("plum")),
        ],
        unclaimed_seconds: 1800.0,
        first_claimed: Some(at(0.5)),
        bounces: 0,
        current: "done".into(),
        current_since: at(3.0),
        closed: Some(at(3.0)),
    }
}

#[test]
fn a_task_claimed_reviewed_and_approved_stops_at_its_verdict() {
    let found = stages(facts("done", 3.0), &straight_through(), at(10.0));
    assert_eq!(found, straight_through_stages());
    assert_eq!(found.lead_seconds(T0), Some(10_800.0));
}

/// The one rule of a task done, which `tasks stats` and the roll-up share: its last close, while
/// it holds done.
#[test]
fn a_task_is_done_at_its_last_close_while_it_holds_done() {
    let done = stages(facts("done", 3.0), &straight_through(), at(10.0));
    assert_eq!(done.done_at(), Some(at(3.0)));
    // Sent back to review by its landing, approved again: done once, at the second approve.
    let mut again = straight_through();
    again.push(sys(8, 4.0, "teal moved it done → review"));
    again.push(verdict_by(9, 5.0, "approve: the merge", "plum"));
    again.push(verdict(10, 6.0, "landed on main"));
    let found = stages(facts("done", 5.0), &again, at(10.0));
    assert_eq!(found.done_at(), Some(at(5.0)));
    let entries = found.segments.iter().filter(|s| s.status == "done").count();
    assert_eq!(entries, 2, "it entered done twice");
    // Sent back and still in review: done no more.
    let reopened = stages(facts("review", 4.0), &again[..8], at(10.0));
    assert_eq!((reopened.closed, reopened.done_at()), (None, None));
    // Cancelled after its approve, by a hand edit no line records: closed, but not done.
    let cancelled = stages(facts("cancelled", 4.0), &straight_through(), at(10.0));
    assert_eq!(cancelled.closed, Some(at(4.0)));
    assert_eq!(cancelled.done_at(), None);
    // Open: no close.
    let open = stages(facts("review", 2.0), &straight_through()[..5], at(10.0));
    assert_eq!(open.done_at(), None);
}

#[test]
fn lines_out_of_order_or_twice_read_the_same() {
    let mut lines = straight_through();
    lines.reverse();
    // The same message twice, and the same move said again by a later message.
    lines.push(sys(5, 2.0, "teal moved it in_progress → review"));
    lines.push(sys(8, 2.2, "teal moved it in_progress → review"));
    lines.swap(0, 3);
    assert_eq!(
        stages(facts("done", 3.0), &lines, at(10.0)),
        straight_through_stages()
    );
}

#[test]
fn a_task_nobody_claimed_waits_up_to_now() {
    let lines = [sys(1, 0.0, "created by owner")];
    assert_eq!(
        stages(facts("ready", 0.0), &lines, at(4.0)),
        Stages {
            stages: vec![stage("ready", 0.0, 4.0, 1)],
            segments: vec![segment("ready", 0.0, None, Some("owner"))],
            unclaimed_seconds: 14_400.0,
            first_claimed: None,
            bounces: 0,
            current: "ready".into(),
            current_since: T0,
            closed: None,
        }
    );
}

#[test]
fn a_thread_with_no_lines_holds_the_status_it_was_made_in() {
    let found = stages(facts("backlog", 0.0), &[], at(2.0));
    assert_eq!(found.stages, vec![stage("backlog", 0.0, 2.0, 1)]);
    assert_eq!(found.current_since, T0);
    assert_eq!(found.unclaimed_seconds, 7200.0);
    assert_eq!(found.lead_seconds(T0), None);
}

#[test]
fn review_sending_it_back_twice_is_two_bounces_and_three_visits() {
    let lines = [
        sys(1, 0.0, "created by amber"),
        sys(2, 1.0, "teal claimed it"),
        sys(3, 2.0, "teal moved it in_progress → review"),
        verdict_by(4, 3.0, "changes: the test is missing", "plum"),
        sys(5, 5.0, "teal moved it in_progress → review"),
        sys(6, 6.0, "teal moved it review → in_progress"),
        sys(7, 7.0, "teal moved it in_progress → review"),
    ];
    let found = stages(facts("review", 7.0), &lines, at(9.0));
    assert_eq!(
        found,
        Stages {
            stages: vec![
                stage("ready", 0.0, 1.0, 1),
                stage("in_progress", 1.0, 4.0, 3),
                stage("review", 2.0, 4.0, 3),
            ],
            segments: vec![
                segment("ready", 0.0, Some(1.0), Some("amber")),
                segment("in_progress", 1.0, Some(2.0), Some("teal")),
                segment("review", 2.0, Some(3.0), Some("teal")),
                segment("in_progress", 3.0, Some(5.0), Some("plum")),
                segment("review", 5.0, Some(6.0), Some("teal")),
                segment("in_progress", 6.0, Some(7.0), Some("teal")),
                segment("review", 7.0, None, Some("teal")),
            ],
            unclaimed_seconds: 3600.0,
            first_claimed: Some(at(1.0)),
            bounces: 2,
            current: "review".into(),
            current_since: at(7.0),
            closed: None,
        }
    );
    assert_eq!(
        summary_line(&found),
        "Stages: ready 1.0 h · in_progress 4.0 h (3 visits) · review 4.0 h (3 visits) · 2 bounces; first claimed after 1.0 h"
    );
}

#[test]
fn a_changes_verdict_said_twice_is_one_bounce() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 2.0, "teal moved it in_progress → review"),
        verdict(3, 3.0, "changes"),
        verdict(4, 3.5, "changes: and this"),
    ];
    let found = stages(facts("in_progress", 3.0), &lines, at(4.0));
    assert_eq!(found.bounces, 1);
    assert_eq!(
        found.stage("in_progress"),
        Some(&stage("in_progress", 1.0, 2.0, 2))
    );
    assert_eq!(
        summary_line(&found),
        "Stages: ready 1.0 h · in_progress 2.0 h (2 visits) · review 1.0 h · 1 bounce; first claimed after 1.0 h"
    );
}

#[test]
fn blocked_by_hand_and_back_counts_the_block_apart() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(2, 1.0, "amber moved it backlog → ready"),
        sys(3, 2.0, "teal claimed it"),
        sys(4, 3.0, "teal moved it in_progress → blocked"),
        sys(5, 5.0, "teal moved it blocked → in_progress"),
    ];
    let found = stages(facts("in_progress", 5.0), &lines, at(6.0));
    assert_eq!(
        found.stages,
        vec![
            stage("backlog", 0.0, 1.0, 1),
            stage("ready", 1.0, 1.0, 1),
            stage("in_progress", 2.0, 2.0, 2),
            stage("blocked", 3.0, 2.0, 1),
        ]
    );
    assert_eq!(found.bounces, 0);
    assert_eq!(found.unclaimed_seconds, 7200.0);
    assert_eq!(found.current_since, at(5.0));
}

#[test]
fn a_cancelled_prerequisite_blocks_and_gives_the_status_back() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(
            2,
            1.0,
            "blocked: cancelled dependencies: sim/a, sim/b (was ready)",
        ),
        sys(
            3,
            4.0,
            "unblocked: no cancelled dependency left; back to ready",
        ),
    ];
    let found = stages(facts("ready", 4.0), &lines, at(5.0));
    assert_eq!(
        found.stages,
        vec![stage("ready", 0.0, 2.0, 2), stage("blocked", 1.0, 3.0, 1)]
    );
    assert_eq!(found.current_since, at(4.0));
}

#[test]
fn the_owners_word_ending_a_block_gives_the_status_back() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(2, 1.0, "orch moved it ready → blocked"),
        sys(
            3,
            4.0,
            "unblocked: the owner decided layout (approve); back to ready",
        ),
    ];
    let found = stages(facts("ready", 4.0), &lines, at(5.0));
    assert_eq!(
        found.stages,
        vec![stage("ready", 0.0, 2.0, 2), stage("blocked", 1.0, 3.0, 1)]
    );
    assert_eq!(found.current_since, at(4.0));
}

#[test]
fn a_block_ending_line_is_read_by_its_last_back_to() {
    use crate::stages::{unblocked_line, unblocked_to};
    for (text, to) in [
        (UNBLOCK_LINE.to_string() + "ready", Some("ready")),
        (
            UNBLOCK_LINE.to_string() + "in_progress (its block line recorded no earlier status)",
            Some("in_progress"),
        ),
        (
            unblocked_line("the owner decided layout (dismiss)", "review"),
            Some("review"),
        ),
        (
            unblocked_line("the owner approved 1a2b3c4", "review"),
            Some("review"),
        ),
        ("unblocked: nothing named".to_string(), None),
        ("blocked: back to ready".to_string(), None),
        ("orch said unblocked: x; back to ready".to_string(), None),
    ] {
        assert_eq!(unblocked_to(&text), to, "{text}");
    }
    assert_eq!(
        unblocked_line("the owner decided layout (approve)", "ready"),
        "unblocked: the owner decided layout (approve); back to ready"
    );
}

#[test]
fn an_unblock_line_with_its_reason_still_names_its_status() {
    let lines = [
        sys(1, 1.0, "blocked: cancelled dependencies: sim/a"),
        sys(
            2,
            3.0,
            "unblocked: no cancelled dependency left; back to in_progress (its block line recorded no earlier status)",
        ),
    ];
    let found = stages(facts("in_progress", 3.0), &lines, at(4.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 1.0, 1),
            stage("blocked", 1.0, 2.0, 1),
            stage("in_progress", 3.0, 1.0, 1),
        ]
    );
}

#[test]
fn a_cancelled_task_stops_at_its_cancellation() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(2, 2.0, "owner moved it ready → cancelled"),
        sys(3, 3.0, "teal claimed it"),
        sys(4, 4.0, "teal moved it in_progress → review"),
    ];
    assert_eq!(
        stages(facts("cancelled", 2.0), &lines, at(50.0)),
        Stages {
            stages: vec![stage("ready", 0.0, 2.0, 1), stage("cancelled", 2.0, 0.0, 1)],
            segments: vec![
                segment("ready", 0.0, Some(2.0), Some("owner")),
                segment("cancelled", 2.0, None, Some("owner")),
            ],
            unclaimed_seconds: 7200.0,
            first_claimed: None,
            bounces: 0,
            current: "cancelled".into(),
            current_since: at(2.0),
            closed: Some(at(2.0)),
        }
    );
}

#[test]
fn work_its_last_executor_released_is_ready_again() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 1.5, "plum released reviewer (released)"),
        sys(3, 1.6, "plum released every role (left)"),
        sys(4, 2.0, "teal released executor (released)"),
        sys(5, 4.0, "jade claimed it"),
    ];
    let found = stages(facts("in_progress", 4.0), &lines, at(5.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 3.0, 2),
            stage("in_progress", 1.0, 2.0, 2)
        ]
    );
    assert_eq!(found.first_claimed, Some(at(1.0)));
    assert_eq!(found.unclaimed_seconds, 3600.0);
}

#[test]
fn a_claim_that_takes_over_keeps_the_work_in_progress() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 2.0, "jade claimed it, handed over from teal (lost)"),
        // teal no longer executes it: its leaving changes nothing.
        sys(3, 3.0, "teal released every role (left)"),
        sys(4, 6.0, "jade released every role (left)"),
    ];
    let found = stages(facts("ready", 6.0), &lines, at(7.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 2.0, 2),
            stage("in_progress", 1.0, 5.0, 1)
        ]
    );
    assert_eq!(found.current_since, at(6.0));
}

#[test]
fn a_claim_of_work_not_ready_changes_no_status() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 3.0, "amber moved it design → ready"),
    ];
    let found = stages(facts("ready", 3.0), &lines, at(4.0));
    assert_eq!(
        found.stages,
        vec![stage("design", 0.0, 3.0, 1), stage("ready", 3.0, 1.0, 1)]
    );
    assert_eq!(found.first_claimed, Some(at(1.0)));
}

#[test]
fn a_landing_closes_work_whatever_its_verdict_says() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 2.0, "teal moved it in_progress → review"),
        verdict(3, 6.0, "landed sim/a on main at 0e97e33"),
    ];
    let found = stages(facts("done", 6.0), &lines, at(9.0));
    assert_eq!(found.closed, Some(at(6.0)));
    assert_eq!(found.stage("review"), Some(&stage("review", 2.0, 4.0, 1)));
    assert_eq!(found.stage("done"), Some(&stage("done", 6.0, 0.0, 1)));
}

#[test]
fn a_subtask_closed_with_done_was_in_progress_since_its_claim() {
    let lines = [
        sys(1, 0.25, "teal claimed it"),
        verdict(2, 1.25, "done: 4 ms"),
    ];
    let found = stages(facts("done", 1.25), &lines, at(2.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 0.25, 1),
            stage("in_progress", 0.25, 1.0, 1),
            stage("done", 1.25, 0.0, 1),
        ]
    );
}

#[test]
fn the_first_line_naming_a_status_says_what_it_was_made_in() {
    let made = |status: &str, lines: &[Line]| {
        stages(facts(status, 1.0), lines, at(2.0)).stages[0]
            .status
            .clone()
    };
    assert_eq!(
        made("ready", &[sys(1, 1.0, "amber moved it concept → ready")]),
        "concept"
    );
    assert_eq!(made("in_progress", &[verdict(1, 1.0, "changes")]), "review");
    assert_eq!(made("done", &[verdict(1, 1.0, "approve")]), "review");
    assert_eq!(
        made(
            "blocked",
            &[sys(
                1,
                1.0,
                "blocked: cancelled dependencies: a (was design)"
            )]
        ),
        "design"
    );
    // Nothing says: a status work is made in is its own, any other was reached from ready.
    assert_eq!(made("design", &[]), "design");
    assert_eq!(made("review", &[]), "ready");
    assert_eq!(
        made("in_progress", &[sys(1, 1.0, "teal claimed it")]),
        "ready"
    );
}

#[test]
fn a_change_the_thread_does_not_record_is_dated_by_the_tasks_last_write() {
    let found = stages(
        facts("done", 3.0),
        &[sys(1, 0.0, "created by owner")],
        at(8.0),
    );
    assert_eq!(
        found.stages,
        vec![stage("ready", 0.0, 3.0, 1), stage("done", 3.0, 0.0, 1)]
    );
    assert_eq!(found.closed, Some(at(3.0)));
    assert_eq!(found.unclaimed_seconds, 10_800.0);
    // An open status reached off the record is held from that write on.
    let found = stages(
        facts("review", 2.0),
        &[sys(1, 1.0, "teal claimed it")],
        at(5.0),
    );
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 1.0, 1),
            stage("in_progress", 1.0, 1.0, 1),
            stage("review", 2.0, 3.0, 1),
        ]
    );
}

#[test]
fn a_move_from_another_status_than_the_replay_holds_is_taken_at_its_word() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(2, 1.0, "amber moved it ready → backlog"),
        sys(3, 3.0, "teal moved it in_progress → review"),
    ];
    let found = stages(facts("review", 3.0), &lines, at(4.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 1.0, 1),
            stage("backlog", 1.0, 2.0, 1),
            stage("review", 3.0, 1.0, 1),
        ]
    );
}

#[test]
fn only_the_boards_own_lines_are_read() {
    let lines = [
        line(1, "msg", 1.0, "teal claimed it"),
        line(2, "msg", 2.0, "teal moved it ready → review"),
        sys(3, 2.0, "teal moved it ready → shipped"),
        sys(4, 2.0, "teal moved it ready"),
        sys(5, 2.0, "teal claimed it twice"),
        sys(
            6,
            2.0,
            "unblocked: no cancelled dependency left; back to nowhere",
        ),
        sys(7, 2.0, "teal reserved it"),
    ];
    let found = stages(facts("ready", 0.0), &lines, at(3.0));
    assert_eq!(found.stages, vec![stage("ready", 0.0, 3.0, 1)]);
    assert_eq!(found.first_claimed, None);
}

#[test]
fn a_time_before_the_task_or_after_now_counts_nothing_negative() {
    let lines = [sys(1, -2.0, "teal claimed it")];
    let found = stages(facts("in_progress", 0.0), &lines, at(-1.0));
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 0.0, 1),
            stage("in_progress", 0.0, 0.0, 1)
        ]
    );
    assert_eq!(found.unclaimed_seconds, 0.0);
    assert_eq!(found.first_claimed, Some(T0));
}

#[test]
fn the_lines_the_store_writes_are_the_lines_read() {
    assert_eq!(
        moved_line("teal", "ready", "in_progress"),
        "teal moved it ready → in_progress"
    );
    assert_eq!(claimed_line("teal", &[]), "teal claimed it");
    assert_eq!(
        claimed_line("jade", &["teal (lost)".into(), "plum (stale)".into()]),
        "jade claimed it, handed over from teal (lost), plum (stale)"
    );
    let read = |text: String| event(&sys(1, 0.0, &text)).map(|e| format!("{e:?}"));
    assert_eq!(
        read(moved_line("teal", "review", "in_progress")).as_deref(),
        Some(r#"Move { from: "review", to: "in_progress" }"#)
    );
    assert_eq!(
        read(claimed_line(
            "jade",
            &["teal (lost)".into(), "plum (stale)".into()]
        ))
        .as_deref(),
        Some(r#"Claim { by: "jade", handed: ["teal", "plum"] }"#)
    );
    assert_eq!(
        read(format!("teal{RELEASED}{EVERY_ROLE} (left)")).as_deref(),
        Some(r#"Release { by: "teal" }"#)
    );
    assert_eq!(read(format!("{CREATED}owner")), None);
}

#[test]
fn a_task_with_no_bounce_and_one_visit_each_says_only_its_hours() {
    assert_eq!(
        summary_line(&straight_through_stages()),
        "Stages: ready 0.5 h · in_progress 1.5 h · review 1.0 h; first claimed after 0.5 h"
    );
    assert_eq!(hours(4680.0), "1.3 h");
}

#[test]
fn a_task_nobody_claimed_says_how_long_it_has_waited() {
    let found = stages(facts("ready", 0.0), &[], at(4.0));
    assert_eq!(
        summary_line(&found),
        "Stages: ready 4.0 h; never claimed in 4.0 h"
    );
}

#[test]
fn the_statuses_times_add_up_to_the_lead_time_and_the_wait_is_a_part_of_them() {
    // Ready half an hour, in progress an hour and a half, in review an hour: three hours.
    let found = stages(facts("done", 3.0), &straight_through(), at(10.0));
    let held: f64 = found.stages.iter().map(|stage| stage.seconds).sum();
    assert_eq!(held, 10_800.0);
    assert_eq!(found.lead_seconds(T0), Some(10_800.0));
    // The wait before the claim is the ready half hour, counted once.
    assert_eq!(found.unclaimed_seconds, 1800.0);
    assert_eq!(found.stage("ready").map(|s| s.seconds), Some(1800.0));
}

#[test]
fn lines_are_replayed_in_the_order_they_were_written_whatever_their_clocks_said() {
    // The move was written after the claim by a clock ten seconds behind the claimer's.
    let claim = Line {
        id: 5,
        kind: "system".into(),
        text: "teal claimed it".into(),
        ts: T0 + 100.0,
        sender: "teal".into(),
    };
    let moved = Line {
        id: 6,
        kind: "system".into(),
        text: "teal moved it in_progress → review".into(),
        ts: T0 + 90.0,
        sender: "teal".into(),
    };
    let found = stages(facts("review", 0.0), &[moved, claim], T0 + 400.0);
    assert_eq!(
        found.stages,
        vec![
            Stage {
                status: "ready".into(),
                first_entered: T0,
                seconds: 100.0,
                visits: 1
            },
            Stage {
                status: "in_progress".into(),
                first_entered: T0 + 100.0,
                seconds: 0.0,
                visits: 1
            },
            Stage {
                status: "review".into(),
                first_entered: T0 + 100.0,
                seconds: 300.0,
                visits: 1
            },
        ]
    );
    assert_eq!(found.first_claimed, Some(T0 + 100.0));
    assert_eq!(found.current_since, T0 + 100.0);
}

/// Each status's segments add up to its stage's time and are as many as its visits; they follow
/// one another from the task's creation, and only the last is open.
fn segments_hold_the_stages_time(found: &Stages, created: f64, now: f64) {
    let first = found
        .segments
        .first()
        .expect("a task holds a status from its creation");
    assert_eq!(first.from, created);
    for pair in found.segments.windows(2) {
        assert_eq!(pair[0].to, Some(pair[1].from), "{:?}", found.segments);
    }
    assert_eq!(found.segments.last().and_then(|s| s.to), None);
    for stage in &found.stages {
        let mine: Vec<&Segment> = found
            .segments
            .iter()
            .filter(|segment| segment.status == stage.status)
            .collect();
        let held: f64 = mine.iter().map(|segment| segment.seconds(now)).sum();
        assert!(
            (held - stage.seconds).abs() < 1e-6,
            "{}: {held} against {}",
            stage.status,
            stage.seconds
        );
        assert_eq!(
            u32::try_from(mine.len()).unwrap(),
            stage.visits,
            "{}",
            stage.status
        );
        assert_eq!(mine[0].from, stage.first_entered, "{}", stage.status);
    }
    let last = found.segments.last().unwrap();
    assert_eq!(
        (last.status.as_str(), last.from),
        (found.current.as_str(), found.current_since)
    );
}

#[test]
fn a_task_bounced_twice_and_unclaimed_for_an_hour_keeps_each_stretch_and_the_same_totals() {
    let lines = [
        sys(1, 0.0, "created by amber"),
        sys(2, 0.0, "amber joined as orchestrator"),
        sys(3, 1.0, "teal claimed it"),
        sys(4, 2.0, "teal moved it in_progress → review"),
        sys(5, 2.5, "plum joined as reviewer"),
        verdict_by(6, 3.0, "changes: the test is missing", "plum"),
        sys(7, 4.0, "teal moved it in_progress → review"),
        verdict_by(8, 4.5, "changes: and its edge case", "plum"),
        sys(9, 6.0, "teal moved it in_progress → review"),
        verdict_by(10, 7.0, "approve: it holds", "plum"),
    ];
    let found = stages(facts("done", 7.0), &lines, at(9.0));
    assert_eq!(
        found.segments,
        vec![
            segment("ready", 0.0, Some(1.0), Some("amber")),
            segment("in_progress", 1.0, Some(2.0), Some("teal")),
            segment("review", 2.0, Some(3.0), Some("teal")),
            segment("in_progress", 3.0, Some(4.0), Some("plum")),
            segment("review", 4.0, Some(4.5), Some("teal")),
            segment("in_progress", 4.5, Some(6.0), Some("plum")),
            segment("review", 6.0, Some(7.0), Some("teal")),
            segment("done", 7.0, None, Some("plum")),
        ]
    );
    // The totals are the replay's as they were: an hour unclaimed, two bounces.
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 1.0, 1),
            stage("in_progress", 1.0, 3.5, 3),
            stage("review", 2.0, 2.5, 3),
            stage("done", 7.0, 0.0, 1),
        ]
    );
    assert_eq!((found.unclaimed_seconds, found.bounces), (3600.0, 2));
    assert_eq!(found.lead_seconds(T0), Some(7.0 * H));
    segments_hold_the_stages_time(&found, T0, at(9.0));
}

#[test]
fn a_task_reopened_after_its_approval_keeps_its_history_and_the_bounce_after_it() {
    let lines = [
        sys(1, 0.0, "created by amber"),
        sys(2, 0.5, "teal claimed it"),
        sys(3, 2.0, "teal moved it in_progress → review"),
        verdict_by(4, 3.0, "approve: it holds", "plum"),
        // Reopened from done; a stray verdict while it was closed changes nothing.
        verdict_by(5, 3.5, "changes: too late", "plum"),
        sys(6, 4.0, "owner moved it done → review"),
        verdict_by(7, 5.0, "changes: one more case", "plum"),
        sys(8, 6.0, "teal moved it in_progress → review"),
    ];
    let found = stages(facts("review", 6.0), &lines, at(8.0));
    assert_eq!(
        found.segments,
        vec![
            segment("ready", 0.0, Some(0.5), Some("amber")),
            segment("in_progress", 0.5, Some(2.0), Some("teal")),
            segment("review", 2.0, Some(3.0), Some("teal")),
            segment("done", 3.0, Some(4.0), Some("plum")),
            segment("review", 4.0, Some(5.0), Some("owner")),
            segment("in_progress", 5.0, Some(6.0), Some("plum")),
            segment("review", 6.0, None, Some("teal")),
        ]
    );
    // The time closed counts for no status; the changes after the reopening is a bounce.
    assert_eq!(
        found.stages,
        vec![
            stage("ready", 0.0, 0.5, 1),
            stage("in_progress", 0.5, 2.5, 2),
            stage("review", 2.0, 4.0, 3),
            stage("done", 3.0, 0.0, 1),
        ]
    );
    assert_eq!((found.bounces, found.closed), (1, None));
    assert_eq!(found.current, "review");
    // `tasks show` says the same.
    assert_eq!(
        summary_line(&found),
        "Stages: ready 0.5 h · in_progress 2.5 h (2 visits) · review 4.0 h (3 visits) · 1 bounce; \
         first claimed after 0.5 h"
    );
    segments_hold_the_stages_time(&found, T0, at(8.0));
    // Closed again, it is closed from its last close.
    let mut closed = lines.to_vec();
    closed.push(verdict_by(9, 7.0, "approve: now it holds", "plum"));
    let found = stages(facts("done", 7.0), &closed, at(8.0));
    assert_eq!(found.closed, Some(at(7.0)));
    assert_eq!(found.bounces, 1);
    assert_eq!(found.segments.len(), 8);
}

#[test]
fn every_replay_keeps_segments_that_add_up_to_its_stages() {
    let cases: Vec<(Facts, Vec<Line>, f64)> = vec![
        (facts("done", 3.0), straight_through(), at(10.0)),
        (facts("ready", 0.0), vec![], at(4.0)),
        // Blocked by a cancelled dependency and given its status back.
        (
            facts("in_progress", 4.0),
            vec![
                sys(1, 1.0, "teal claimed it"),
                sys(
                    2,
                    2.0,
                    "blocked: cancelled dependencies: tools/a (was in_progress)",
                ),
                sys(
                    3,
                    4.0,
                    "unblocked: no cancelled dependency left; back to in_progress",
                ),
            ],
            at(5.0),
        ),
        // Released by its last executor, claimed again, left.
        (
            facts("ready", 6.0),
            vec![
                sys(1, 1.0, "teal claimed it"),
                sys(2, 2.0, "teal released executor (released)"),
                sys(3, 4.0, "jade claimed it"),
                sys(4, 6.0, "jade released every role (left)"),
            ],
            at(8.0),
        ),
        // A clock that went back, and a status the thread never recorded taking.
        (
            facts("blocked", 5.0),
            vec![
                sys(1, 2.0, "teal claimed it"),
                sys(2, 1.0, "teal moved it in_progress → review"),
            ],
            at(6.0),
        ),
        // A task cancelled, then reopened by hand: the cancelled stretch closes.
        (
            facts("ready", 5.0),
            vec![sys(1, 2.0, "owner moved it ready → cancelled")],
            at(6.0),
        ),
    ];
    for (facts, lines, now) in cases {
        let found = stages(facts, &lines, now);
        segments_hold_the_stages_time(&found, facts.created, now);
    }
}

#[test]
fn a_block_and_an_unblock_name_nobody_and_a_change_the_thread_missed_is_dated_at_updated() {
    let lines = [
        sys(1, 1.0, "teal claimed it"),
        sys(
            2,
            2.0,
            "blocked: cancelled dependencies: tools/a (was in_progress)",
        ),
        sys(
            3,
            3.0,
            "unblocked: no cancelled dependency left; back to in_progress",
        ),
    ];
    let found = stages(facts("review", 5.0), &lines, at(6.0));
    assert_eq!(
        found.segments,
        vec![
            segment("ready", 0.0, Some(1.0), None),
            segment("in_progress", 1.0, Some(2.0), Some("teal")),
            segment("blocked", 2.0, Some(3.0), None),
            segment("in_progress", 3.0, Some(5.0), None),
            segment("review", 5.0, None, None),
        ]
    );
    assert_eq!(found.segments[4].seconds(at(6.0)), H);
    assert_eq!(segment("done", 1.0, None, None).seconds(at(9.0)), 0.0);
}

fn hold(handle: &str, role: &str, from: f64, to: Option<f64>) -> Hold {
    Hold {
        handle: handle.into(),
        role: role.into(),
        from: at(from),
        to: to.map(at),
    }
}

#[test]
fn holds_follow_claims_joins_hand_overs_and_releases() {
    let lines = [
        sys(1, 0.0, "created by amber"),
        sys(2, 0.0, "amber joined as orchestrator"),
        sys(3, 1.0, "teal claimed it"),
        // A second claim by the holder is no new hold.
        sys(4, 1.5, "teal claimed it"),
        sys(5, 2.0, "teal moved it in_progress → review"),
        sys(6, 2.5, "plum joined as reviewer"),
        verdict_by(7, 3.0, "changes: the test is missing", "plum"),
        sys(8, 4.0, "jade claimed it, handed over from teal (lost)"),
        // teal no longer holds it: its leaving ends nothing more.
        sys(9, 4.5, "teal released every role (left)"),
        sys(10, 5.0, "plum released reviewer (released)"),
        sys(11, 6.0, "moss joined as reviewer"),
        // An orchestrator's release is no executor's or reviewer's.
        sys(12, 6.5, "amber released orchestrator (released)"),
        sys(13, 7.0, "jade released every role (left)"),
        line(14, "msg", 7.5, "plum joined as reviewer"),
    ];
    assert_eq!(
        holds(&lines, T0),
        vec![
            hold("teal", "executor", 1.0, Some(4.0)),
            hold("plum", "reviewer", 2.5, Some(5.0)),
            hold("jade", "executor", 4.0, Some(7.0)),
            hold("moss", "reviewer", 6.0, None),
        ]
    );
    // A line dated before the one ahead of it is taken at that one's time, as the replay takes it.
    let skewed = [
        sys(1, 2.0, "teal claimed it"),
        sys(2, 1.0, "plum joined as reviewer"),
    ];
    assert_eq!(
        holds(&skewed, T0),
        vec![
            hold("teal", "executor", 2.0, None),
            hold("plum", "reviewer", 2.0, None),
        ]
    );
    assert_eq!(holds(&[], T0), Vec::new());
}

#[test]
fn in_status_since_is_when_the_task_took_the_status_it_holds() {
    // Open: the move that put it in review, whatever the clock says now.
    let bounced = [
        sys(1, 0.0, "created by amber"),
        sys(2, 1.0, "teal claimed it"),
        sys(3, 2.0, "teal moved it in_progress → review"),
        verdict_by(4, 3.0, "changes: the test is missing", "plum"),
        sys(5, 5.0, "teal moved it in_progress → review"),
    ];
    assert_eq!(in_status_since(facts("review", 5.0), &bounced), at(5.0));
    // Sent back: the verdict that did.
    assert_eq!(
        in_status_since(facts("in_progress", 3.0), &bounced[..4]),
        at(3.0)
    );
    // Never claimed: its creation.
    let made = [sys(1, 0.0, "created by owner")];
    assert_eq!(in_status_since(facts("ready", 0.0), &made), T0);
    assert_eq!(in_status_since(facts("backlog", 0.0), &[]), T0);
    // Closed: its close, and a stray line after it moves nothing.
    let mut approved = straight_through();
    approved.push(verdict_by(8, 4.0, "changes: too late", "plum"));
    assert_eq!(in_status_since(facts("done", 3.0), &approved), at(3.0));
    // Released by its last executor: ready again from the release.
    let released = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 2.0, "teal released executor (released)"),
    ];
    assert_eq!(in_status_since(facts("ready", 2.0), &released), at(2.0));
    // Blocked by a cancelled dependency and given its status back.
    let unblocked = [
        sys(1, 1.0, "blocked: cancelled dependencies: sim/a (was ready)"),
        sys(
            2,
            4.0,
            "unblocked: no cancelled dependency left; back to ready",
        ),
    ];
    assert_eq!(in_status_since(facts("ready", 4.0), &unblocked), at(4.0));
    assert_eq!(
        in_status_since(facts("blocked", 1.0), &unblocked[..1]),
        at(1.0)
    );
    // A change the thread does not record: the task's last write.
    let off_record = [sys(1, 1.0, "teal claimed it")];
    assert_eq!(in_status_since(facts("review", 2.5), &off_record), at(2.5));
    // Reopened from done: the reopening.
    let reopened = [
        sys(1, 0.5, "teal claimed it"),
        sys(2, 2.0, "teal moved it in_progress → review"),
        verdict_by(3, 3.0, "approve", "plum"),
        sys(4, 4.0, "owner moved it done → review"),
    ];
    assert_eq!(in_status_since(facts("review", 4.0), &reopened), at(4.0));
}

#[test]
fn in_status_says_the_status_a_task_held_before_the_one_it_holds() {
    let from = |status: &str, updated: f64, lines: &[Line]| {
        let read = in_status(facts(status, updated), lines);
        (read.since, read.from)
    };
    // Blocked by hand from review: review, from the move that blocked it.
    let held = [
        sys(1, 1.0, "teal claimed it"),
        sys(2, 2.0, "teal moved it in_progress → review"),
        sys(3, 3.0, "teal moved it review → blocked"),
    ];
    assert_eq!(
        from("blocked", 3.0, &held),
        (at(3.0), Some("review".into()))
    );
    // Blocked by a cancelled dependency: the status its line names.
    let cancelled = [sys(
        1,
        1.0,
        "blocked: cancelled dependencies: sim/a (was design)",
    )];
    assert_eq!(
        from("blocked", 1.0, &cancelled),
        (at(1.0), Some("design".into()))
    );
    // A change no line records: the status the replay last knew, from the task's last write.
    let off_record = [sys(1, 1.0, "teal claimed it")];
    assert_eq!(
        from("blocked", 2.5, &off_record),
        (at(2.5), Some("in_progress".into()))
    );
    // Still in the status it was made in: none before it.
    let made = [sys(1, 0.0, "created by owner")];
    assert_eq!(from("ready", 0.0, &made), (T0, None));
}

#[test]
fn in_status_since_is_the_replays_current_since_at_any_now() {
    let cases: Vec<(Facts, Vec<Line>)> = vec![
        (facts("done", 3.0), straight_through()),
        (facts("ready", 0.0), vec![]),
        (
            facts("in_progress", 3.0),
            vec![
                sys(1, 1.0, "teal claimed it"),
                sys(2, 2.0, "teal moved it in_progress → review"),
                verdict(3, 3.0, "changes"),
            ],
        ),
    ];
    for (facts, lines) in cases {
        let since = in_status_since(facts, &lines);
        for now in [T0, at(3.0), at(100.0)] {
            assert_eq!(stages(facts, &lines, now).current_since, since);
        }
    }
}

#[test]
fn review_counts_follow_review_segments_through_changes_blocks_and_reopening() {
    let lines = [
        sys(1, 0.0, "created by owner"),
        sys(2, 1.0, "teal claimed it"),
        sys(3, 2.0, "teal moved it in_progress → review"),
        sys(4, 3.0, "plum joined as reviewer"),
        verdict_by(5, 4.0, "changes: needs a test", "plum"),
        sys(6, 5.0, "teal moved it in_progress → review"),
        sys(7, 6.0, "teal moved it review → blocked"),
        sys(8, 7.0, "teal moved it blocked → review"),
        verdict_by(9, 8.0, "approve", "plum"),
        sys(10, 9.0, "owner moved it done → review"),
        sys(11, 10.0, "owner moved it review → cancelled"),
    ];
    for (length, status, count) in [
        (1, "ready", 0),
        (2, "in_progress", 0),
        (3, "review", 1),
        (4, "review", 1),
        (5, "in_progress", 1),
        (6, "review", 2),
        (7, "blocked", 2),
        (8, "review", 3),
        (9, "done", 3),
        (10, "review", 4),
        (11, "cancelled", 4),
    ] {
        assert_eq!(
            in_status(facts(status, (length - 1) as f64), &lines[..length]).review_rounds,
            count,
            "{length} lines, {status}"
        );
    }
    // Legacy history is interpreted by the same replay: a current review with no move is
    // one known stretch, while a closed task with no history proves no review stretch.
    for (status, count) in [("review", 1), ("ready", 0), ("done", 0), ("cancelled", 0)] {
        assert_eq!(in_status(facts(status, 3.0), &[]).review_rounds, count);
    }
}
