//! The replay of a thread, each case's expected times written out by hand.

use super::stats::{Row, Sitting, Spread, Waiting, spread, summarize};
use super::*;

/// When every case's task was made.
const T0: f64 = 1_000_000.0;
const H: f64 = 3600.0;
const D: f64 = 86_400.0;

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
fn stage(status: &str, entered: f64, held: f64, visits: u32) -> Stage {
    Stage {
        status: status.into(),
        first_entered: T0 + entered * H,
        seconds: held * H,
        visits,
    }
}

fn at(hours: f64) -> f64 {
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

fn straight_through_stages() -> Stages {
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
        "Stages: unclaimed 1.0 h · ready 1.0 h · in_progress 4.0 h (3 visits) · review 4.0 h (3 visits) · 2 bounces"
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
        "Stages: unclaimed 1.0 h · ready 1.0 h · in_progress 2.0 h (2 visits) · review 1.0 h · 1 bounce"
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
        "Stages: unclaimed 0.5 h · ready 0.5 h · in_progress 1.5 h · review 1.0 h"
    );
    assert_eq!(hours(4680.0), "1.3 h");
}

// The pipeline over a window.

fn near(found: f64, wanted: f64) {
    assert!((found - wanted).abs() < 1e-6, "{found} is not {wanted}");
}

fn spread_is(found: Option<Spread>, n: usize, median: f64, p75: f64, p90: f64) {
    let found = found.expect("a spread");
    assert_eq!(found.n, n);
    near(found.median, median * H);
    near(found.p75, p75 * H);
    near(found.p90, p90 * H);
}

/// A task made `made` hours in, holding `status` since `since` hours in, its stages given as
/// (status, hours held).
fn row(slug: &str, status: &str, made: f64, since: f64, held: &[(&str, f64)]) -> Row {
    Row {
        slug: slug.into(),
        status: status.into(),
        created: at(made),
        executing: false,
        stages: Stages {
            stages: held.iter().map(|(s, h)| stage(s, made, *h, 1)).collect(),
            unclaimed_seconds: held.first().map_or(0.0, |(_, h)| h * H),
            first_claimed: None,
            bounces: 0,
            current: status.into(),
            current_since: at(since),
            closed: CLOSED.contains(&status).then_some(at(since)),
        },
    }
}

fn day(time: f64) -> String {
    format!("d{}", ((time - T0) / D).floor())
}

fn window() -> Vec<Row> {
    let done = |slug: &str, made: f64, took: f64, held: &[(&str, f64)]| {
        row(slug, "done", made, made + took, held)
    };
    let mut b = done(
        "sim/b",
        24.0,
        8.0,
        &[("ready", 2.0), ("in_progress", 4.0), ("review", 2.0)],
    );
    b.stages.bounces = 2;
    let mut f = row("sim/f", "ready", 20.0, 24.0, &[("ready", 48.0)]);
    f.executing = true;
    let mut g = row("sim/g", "in_progress", 60.0, 66.0, &[("in_progress", 6.0)]);
    (g.stages.bounces, g.executing) = (1, true);
    vec![
        done(
            "sim/a",
            0.0,
            4.0,
            &[("ready", 1.0), ("in_progress", 2.0), ("review", 1.0)],
        ),
        b,
        done("sim/c", 24.0, 12.0, &[("ready", 3.0), ("in_progress", 9.0)]),
        row("sim/d", "blocked", 40.0, 48.0, &[("blocked", 24.0)]),
        row("sim/e", "ready", 60.0, 60.0, &[("ready", 12.0)]),
        f,
        g,
        row("sim/h", "cancelled", 10.0, 11.0, &[("ready", 1.0)]),
        row("sim/i", "ready", 48.0, 48.0, &[("ready", 24.0)]),
    ]
}

#[test]
fn the_window_is_counted_spread_and_ranked() {
    let found = summarize(&window(), at(72.0), 3, &day);
    assert_eq!((found.days, found.tasks), (3, 9));
    let counts: Vec<(&str, usize)> = (found.by_status.iter())
        .map(|(status, count)| (status.as_str(), *count))
        .collect();
    assert_eq!(
        counts,
        [
            ("backlog", 0),
            ("concept", 0),
            ("design", 0),
            ("ready", 3),
            ("in_progress", 1),
            ("review", 0),
            ("done", 3),
            ("blocked", 1),
            ("cancelled", 1),
        ]
    );
    // Lead times of 4, 8 and 12 hours; waits of 1, 2 and 3.
    spread_is(found.lead, 3, 8.0, 10.0, 11.2);
    spread_is(found.unclaimed, 3, 2.0, 2.5, 2.8);
    let staged: Vec<&str> = found.per_stage.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(staged, ["ready", "in_progress", "review"]);
    spread_is(Some(found.per_stage[0].1), 3, 2.0, 2.5, 2.8);
    // 2, 4 and 9 hours in progress; only two of the three were ever in review, 1 and 2 hours.
    spread_is(Some(found.per_stage[1].1), 3, 4.0, 6.5, 8.0);
    spread_is(Some(found.per_stage[2].1), 2, 1.5, 1.75, 1.9);
    assert_eq!(found.bounces, [(0, 7), (1, 1), (2, 1)]);
    let per_day: Vec<(&str, usize)> = (found.done_per_day.iter())
        .map(|(day, done)| (day.as_str(), *done))
        .collect();
    assert_eq!(per_day, [("d0", 1), ("d1", 2), ("d2", 0), ("d3", 0)]);
    let sitting = |slug: &str, status: &str, hours: f64| Sitting {
        slug: slug.into(),
        status: status.into(),
        seconds: hours * H,
    };
    assert_eq!(
        found.slowest,
        [
            sitting("sim/f", "ready", 48.0),
            sitting("sim/d", "blocked", 24.0),
            sitting("sim/i", "ready", 24.0),
            sitting("sim/e", "ready", 12.0),
            sitting("sim/g", "in_progress", 6.0),
        ]
    );
    assert_eq!(
        found.blocked,
        Waiting {
            count: 1,
            median_seconds: Some(86_400.0)
        }
    );
    // sim/e and sim/i: sim/f is ready too, but held.
    assert_eq!(
        found.unstaffed,
        Waiting {
            count: 2,
            median_seconds: Some(64_800.0)
        }
    );
}

#[test]
fn an_empty_window_has_its_days_and_nothing_else() {
    let found = summarize(&[], at(30.0), 1, &day);
    assert_eq!(found.tasks, 0);
    assert_eq!((found.lead, found.unclaimed), (None, None));
    assert!(found.per_stage.is_empty() && found.bounces.is_empty() && found.slowest.is_empty());
    assert_eq!(
        found.done_per_day,
        [("d0".to_string(), 0), ("d1".to_string(), 0)]
    );
    let none = Waiting {
        count: 0,
        median_seconds: None,
    };
    assert_eq!((found.blocked, found.unstaffed), (none, none));
    assert!(found.by_status.iter().all(|(_, count)| *count == 0));
}

#[test]
fn only_the_ten_longest_waits_are_named() {
    let rows: Vec<Row> = (0..12u8)
        .map(|n| {
            let since = f64::from(n);
            row(&format!("sim/t{n:02}"), "review", 0.0, since, &[])
        })
        .collect();
    let found = summarize(&rows, at(20.0), 1, &day);
    let slugs: Vec<&str> = found.slowest.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "sim/t00", "sim/t01", "sim/t02", "sim/t03", "sim/t04", "sim/t05", "sim/t06", "sim/t07",
            "sim/t08", "sim/t09"
        ]
    );
    near(found.slowest[9].seconds, 11.0 * H);
}

#[test]
fn a_spread_falls_between_its_neighbours() {
    assert_eq!(spread(Vec::new()), None);
    let one = spread(vec![5.0]).unwrap();
    assert_eq!((one.n, one.median, one.p75, one.p90), (1, 5.0, 5.0, 5.0));
    let four = spread(vec![4.0, 1.0, 3.0, 2.0]).unwrap();
    assert_eq!(four.n, 4);
    near(four.median, 2.5);
    near(four.p75, 3.25);
    near(four.p90, 3.7);
}
