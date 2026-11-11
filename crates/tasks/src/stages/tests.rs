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
    }
}

fn sys(id: i64, hours: f64, text: &str) -> Line {
    line(id, "system", hours, text)
}

fn verdict(id: i64, hours: f64, text: &str) -> Line {
    line(id, "verdict", hours, text)
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
        verdict(7, 3.0, "approve: fine"),
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
        verdict(4, 3.0, "changes: the test is missing"),
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
    };
    let moved = Line {
        id: 6,
        kind: "system".into(),
        text: "teal moved it in_progress → review".into(),
        ts: T0 + 90.0,
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
