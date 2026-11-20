//! The pipeline alarm's pieces on hand-written lines and counts: what each line the board writes says
//! of a status, a task's timeline walked back through them to what its thread said before the window,
//! the counts added up and the alarm replayed from either state, and the thresholds the manifest
//! names. Rework's are its module's (`rework/tests.rs`), with the line helpers here.

use super::*;

const HOUR: f64 = 3600.0;

/// A line on `slug`'s thread.
pub(super) fn line(slug: &str, sid: i64, ts: f64, kind: &str, text: &str) -> Line {
    Line {
        slug: slug.into(),
        sid,
        ts,
        kind: kind.into(),
        text: text.into(),
    }
}

fn system(text: &str) -> Line {
    line("sim/a", 1, 0.0, "system", text)
}

fn verdict(text: &str) -> Line {
    line("sim/a", 1, 0.0, "verdict", text)
}

#[test]
fn every_line_the_board_writes_on_a_change_of_status_says_which() {
    let cases = [
        (
            system("amber-heron moved it in_progress → review"),
            Step::Between("in_progress", "review"),
        ),
        (
            system("owner moved it blocked → review"),
            Step::Between("blocked", "review"),
        ),
        (system("amber-heron claimed it"), Step::Claimed),
        (
            system("amber-heron claimed it, handed over from calm-vole (lost)"),
            Step::Claimed,
        ),
        (
            system("blocked: cancelled dependencies: sim/b, sim/c (was review)"),
            Step::Between("review", "blocked"),
        ),
        (
            system("unblocked: no cancelled dependency left; back to in_progress"),
            Step::Between("blocked", "in_progress"),
        ),
        (
            system(
                "unblocked: no cancelled dependency left; back to ready (its block line recorded no earlier status)",
            ),
            Step::Between("blocked", "ready"),
        ),
        (
            system("unblocked: the owner decided layout (reject); back to in_progress"),
            Step::Between("blocked", "in_progress"),
        ),
        (
            system("unblocked: the owner approved 1a2b3c4; back to review"),
            Step::Between("blocked", "review"),
        ),
        (
            system("unblocked: the owner decided layout (approve); back to nowhere"),
            Step::Unchanged,
        ),
        (system("unblocked: no status named"), Step::Unchanged),
        (verdict("approve: ok"), Step::Between("review", "done")),
        (verdict("approve"), Step::Between("review", "done")),
        (
            verdict("changes: the test cannot fail"),
            Step::Between("review", "in_progress"),
        ),
        (
            verdict("done: it is done"),
            Step::Between("in_progress", "done"),
        ),
        (
            verdict("landed sim/a on main at abc1234"),
            Step::Landed { taken_back: false },
        ),
        (
            verdict(
                "landed sim/a on main; sim/a was taken back to in_progress, so the landing closes it: further changes go in a new task",
            ),
            Step::Landed { taken_back: true },
        ),
        (
            verdict(
                "covered by sim/b, whose landing at 75b4516a0123 holds its work; closed by x: its fix",
            ),
            Step::Covered,
        ),
    ];
    for (line, expected) in cases {
        assert_eq!(step(&line), expected, "{}", line.text);
    }
}

/// A covered close closes whatever open status the line before it left, not review as a landing
/// does, so a task closed so from progress spent no time in review.
#[test]
fn a_covered_close_closes_the_status_the_line_before_it_left() {
    let lines = [
        line("sim/a", 1, 1.0 * HOUR, "system", "x claimed it"),
        line(
            "sim/a",
            2,
            3.0 * HOUR,
            "verdict",
            "covered by sim/b, whose landing at 75b4516a0123 holds its work; closed by x: y",
        ),
    ];
    let refs: Vec<&Line> = lines.iter().collect();
    assert_eq!(
        timeline("done", 0.0, &refs, 0.0, None),
        vec![
            (0.0, Some("ready")),
            (1.0 * HOUR, Some("in_progress")),
            (3.0 * HOUR, Some("done")),
        ]
    );
    // Closed with no change on record before it, it closes the status work is filed in.
    let alone = [&lines[1]];
    assert_eq!(
        timeline("done", 0.0, &alone, 0.0, None),
        vec![(0.0, Some("ready")), (3.0 * HOUR, Some("done"))]
    );
    // Or the one its thread said before the window opened.
    assert_eq!(
        timeline("done", 0.0, &alone, 0.0, Some("blocked")),
        vec![(0.0, Some("blocked")), (3.0 * HOUR, Some("done"))]
    );
    assert!(rests_on_before("done", &alone));
    assert_eq!(opening(&alone).status, Some("done"));
}

#[test]
fn a_line_that_changes_no_status_is_unchanged() {
    for line in [
        system("amber-heron reserved it"),
        system("amber-heron joined as reviewer"),
        system("created by owner"),
        system("land set _board.red to main"),
        system("amber-heron changed priority p2 → p1"),
        system("amber-heron moved it nowhere → review"),
        system("someone said they claimed it yesterday"),
        // A field's text that reads like a move is no move: a handle holds no space.
        system("amber-heron set notes to w moved it ready → review"),
        line(
            "sim/a",
            1,
            0.0,
            "msg",
            "amber-heron moved it ready → review",
        ),
        line("sim/a", 1, 0.0, "msg", "approve: looks fine"),
        // A move's reason is a post beside its line.
        line(
            "sim/a",
            1,
            0.0,
            "msg",
            "cancelled: its work landed with sim/b",
        ),
        line(
            "sim/a",
            1,
            0.0,
            "msg",
            "covered by sim/b, whose landing at 75b4516",
        ),
    ] {
        assert_eq!(step(&line), Step::Unchanged, "{}", line.text);
    }
}

/// What `lines`, a thread's newest before the window, say held when it opened.
fn opened(lines: &[Line]) -> Opening<'_> {
    opening(&lines.iter().collect::<Vec<_>>())
}

/// `slug`'s timeline over a window from 0, its lines at the given times.
fn walked<'a>(status: &'a str, created: f64, lines: &'a [Line]) -> Vec<(f64, Option<&'a str>)> {
    let lines: Vec<&Line> = lines.iter().collect();
    timeline(status, created, &lines, 0.0, None)
}

#[test]
fn a_timeline_walks_back_from_the_status_now_through_each_change() {
    let lines = [
        line("sim/a", 1, 10.0, "system", "w claimed it"),
        line("sim/a", 2, 20.0, "system", "w reserved it"),
        line(
            "sim/a",
            3,
            30.0,
            "system",
            "w moved it in_progress → review",
        ),
        line("sim/a", 4, 40.0, "verdict", "changes: no"),
        line(
            "sim/a",
            5,
            50.0,
            "system",
            "w moved it in_progress → review",
        ),
    ];
    assert_eq!(
        walked("review", -5.0, &lines),
        [
            (0.0, Some("ready")),
            (10.0, Some("in_progress")),
            (30.0, Some("review")),
            (40.0, Some("in_progress")),
            (50.0, Some("review")),
        ]
    );
}

#[test]
fn a_task_made_in_the_window_held_no_status_before() {
    let lines = [line("sim/a", 1, 10.0, "system", "w claimed it")];
    assert_eq!(
        walked("in_progress", 5.0, &lines),
        [
            (0.0, None),
            (5.0, Some("ready")),
            (10.0, Some("in_progress"))
        ]
    );
    assert_eq!(
        walked("ready", 5.0, &[]),
        [(0.0, None), (5.0, Some("ready"))]
    );
}

#[test]
fn a_task_with_no_line_in_the_window_held_its_status_throughout() {
    assert_eq!(walked("review", -100.0, &[]), [(0.0, Some("review"))]);
}

#[test]
fn a_claim_after_a_release_finds_the_work_in_progress_from_the_line_before() {
    let lines = [
        line("sim/a", 1, 10.0, "system", "w claimed it"),
        line("sim/a", 2, 20.0, "system", "v claimed it"),
    ];
    assert_eq!(
        walked("in_progress", -5.0, &lines),
        [
            (0.0, Some("ready")),
            (10.0, Some("in_progress")),
            (20.0, Some("in_progress")),
        ]
    );
    // A claim of work not ready changes nothing.
    let lines = [line("sim/a", 1, 10.0, "system", "w claimed it")];
    assert_eq!(
        walked("design", -5.0, &lines),
        [(0.0, Some("design")), (10.0, Some("design"))]
    );
}

#[test]
fn a_landing_closes_what_the_line_before_left_blocked_or_else_review() {
    let held = [
        line("sim/a", 1, 10.0, "system", "land moved it review → blocked"),
        line("sim/a", 2, 20.0, "verdict", "landed sim/a on main"),
    ];
    assert_eq!(
        walked("done", -5.0, &held),
        [
            (0.0, Some("review")),
            (10.0, Some("blocked")),
            (20.0, Some("done"))
        ]
    );
    let plain = [line("sim/a", 1, 20.0, "verdict", "landed sim/a on main")];
    assert_eq!(
        walked("done", -5.0, &plain),
        [(0.0, Some("review")), (20.0, Some("done"))]
    );
}

#[test]
fn the_oldest_change_in_the_window_takes_its_status_before_from_the_thread_before_the_window() {
    // A hand-over of work the thread before the window left in progress: in progress all through.
    let handed = [line(
        "sim/a",
        9,
        10.0,
        "system",
        "v claimed it, handed over from w",
    )];
    let lines: Vec<&Line> = handed.iter().collect();
    assert_eq!(
        timeline("in_progress", -5.0, &lines, 0.0, Some("in_progress")),
        [(0.0, Some("in_progress")), (10.0, Some("in_progress"))]
    );
    // A landing of work held blocked before the window.
    let landed = [line("sim/a", 9, 10.0, "verdict", "landed sim/a on main")];
    let lines: Vec<&Line> = landed.iter().collect();
    assert_eq!(
        timeline("done", -5.0, &lines, 0.0, Some("blocked")),
        [(0.0, Some("blocked")), (10.0, Some("done"))]
    );
}

#[test]
fn what_held_when_the_window_opened_is_the_newest_change_and_the_newest_record() {
    let moved = |sid: i64, text: &str| line("sim/a", sid, 1.0, "system", text);
    let lines = [
        waits_line(1, 1.0, "aaa: review"),
        moved(2, "w moved it in_progress → review"),
        moved(3, "w set notes to x moved it review → in_progress"),
        waits_line(4, 1.0, "bbb: certificate"),
        waits_line(5, 5.0, "bbb: changes"),
    ];
    assert_eq!(
        opened(&lines),
        Opening {
            status: Some("review"),
            record: Waits::parse("bbb: changes").map(|waits| Recorded {
                written: Written {
                    tip: "bbb",
                    at: 5.0,
                    sid: 5
                },
                waits,
            }),
        },
        "a field's text is no move, and the newest record stands, with when it was written"
    );
    // A change of status after the record ends nothing: the record judged a tip.
    let moved_after = [
        waits_line(1, 1.0, "aaa: changes"),
        moved(2, "w moved it review → in_progress"),
    ];
    assert_eq!(
        opened(&moved_after),
        Opening {
            status: Some("in_progress"),
            record: Waits::parse("aaa: changes").map(|waits| Recorded {
                written: Written {
                    tip: "aaa",
                    at: 1.0,
                    sid: 1
                },
                waits,
            }),
        }
    );
    // The newest record is the one, though it names no tip: none older stands behind it.
    let unnamed = [
        waits_line(1, 1.0, "aaa: changes"),
        waits_line(2, 1.0, "no tip"),
    ];
    assert_eq!(opened(&unnamed).record, None);
    // A record a field's text forges is none.
    let forged = [line(
        "sim/a",
        1,
        1.0,
        "system",
        "w set notes to x set _board.waits to aaa: changes",
    )];
    assert_eq!(opened(&forged), Opening::default());
    assert_eq!(opened(&[]), Opening::default(), "a thread that never said");
}

#[test]
fn only_a_claim_or_landing_first_or_review_in_the_window_rests_on_the_thread_before() {
    let taken_back = format!("landed sim/a on main; it{TAKEN_BACK}");
    let cases = [
        ("review", None, true),
        ("in_progress", None, false),
        ("done", Some(("verdict", "approve")), true),
        // Back in review on a tip a record from before the window may judge.
        (
            "review",
            Some(("system", "w moved it in_progress → review")),
            true,
        ),
        ("in_progress", Some(("system", "w claimed it")), true),
        ("done", Some(("verdict", "landed sim/a on main")), true),
        ("done", Some(("verdict", taken_back.as_str())), false),
        ("done", Some(("verdict", "done: it is done")), false),
        (
            "blocked",
            Some(("system", "w moved it ready → blocked")),
            false,
        ),
        ("ready", Some(("system", "w reserved it")), false),
    ];
    for (status, said, rests) in cases {
        let lines: Vec<Line> = (said.iter())
            .map(|(kind, text)| line("sim/a", 1, 10.0, kind, text))
            .collect();
        let lines: Vec<&Line> = lines.iter().collect();
        assert_eq!(
            rests_on_before(status, &lines),
            rests,
            "{status} after {said:?}"
        );
    }
}

#[test]
fn counts_add_up_every_task_and_take_one_moment_s_changes_together() {
    let a = vec![(0.0, Some("in_progress")), (10.0, Some("review"))];
    let b = vec![(0.0, Some("review")), (10.0, Some("done"))];
    let c = vec![
        (0.0, None),
        (5.0, Some("ready")),
        (20.0, Some("in_progress")),
    ];
    let (start, changes) = counted(&[a.as_slice(), b.as_slice(), c.as_slice()]);
    assert_eq!(start, counts(1, 1));
    // At 5 nothing counted moves, so no step; at 10 one enters review as another leaves it, and
    // only the work leaving progress shows.
    assert_eq!(changes, [(10.0, counts(1, 0)), (20.0, counts(1, 1))]);
}

fn counts(review: usize, in_progress: usize) -> Counts {
    Counts {
        review,
        in_progress,
    }
}

#[test]
fn review_over_twice_in_progress_raises_the_alarm_after_four_hours_and_not_before() {
    let t = Thresholds::default();
    let start = counts(10, 4);
    let at = |now: f64| replay(&t, Standing::Lowered, start, &[], 0.0, now);
    let times = [3.0 * HOUR, 4.0 * HOUR - 1.0];
    assert_eq!(
        times.map(|now| at(now).raised),
        [false, false],
        "three hours, a second short"
    );
    let raised = at(4.0 * HOUR);
    assert_eq!(
        raised,
        Alarm {
            raised: true,
            counts: start,
            over: true,
            since: 0.0
        }
    );
}

#[test]
fn exactly_twice_is_not_over_and_review_with_nothing_in_progress_is() {
    let cases = [
        (2, counts(8, 4), false),
        (2, counts(9, 4), true),
        (2, counts(1, 0), true),
        (2, counts(0, 0), false),
        (3, counts(12, 4), false),
        (3, counts(13, 4), true),
    ];
    for (ratio, counts, over) in cases {
        let t = Thresholds {
            ratio,
            ..Thresholds::default()
        };
        assert_eq!(t.over(counts), over, "{counts:?} at {ratio}×");
    }
}

#[test]
fn the_alarm_clears_only_after_an_hour_at_or_below_the_ratio() {
    let t = Thresholds::default();
    let fell = 5.0 * HOUR;
    let changes = [(fell, counts(7, 4))];
    let at = |now: f64| replay(&t, Standing::Lowered, counts(10, 4), &changes, 0.0, now);
    let dip = at(fell + HOUR - 1.0);
    assert_eq!(
        (dip.raised, dip.over, dip.since, dip.counts),
        (true, false, fell, counts(7, 4)),
        "under for less than an hour"
    );
    let cleared = at(fell + HOUR);
    assert_eq!(
        (cleared.raised, cleared.since),
        (false, fell),
        "an hour under clears it"
    );
}

#[test]
fn a_dip_shorter_than_an_hour_keeps_the_alarm_and_a_new_rise_raises_it_only_after_four_hours() {
    let t = Thresholds::default();
    let changes = [
        (5.0 * HOUR, counts(7, 4)),
        (5.5 * HOUR, counts(10, 4)),
        (7.0 * HOUR, counts(7, 4)),
        (8.0 * HOUR, counts(10, 4)),
    ];
    let at = |now: f64| replay(&t, Standing::Lowered, counts(10, 4), &changes, 0.0, now);
    // The half-hour dip did not clear it; under, not yet an hour; an hour under at 8:00; over
    // again, not yet four hours; four hours over again.
    let times = [5.9, 7.9, 8.0, 11.9, 12.0].map(|hours| hours * HOUR);
    assert_eq!(
        times.map(|now| at(now).raised),
        [true, true, false, false, true]
    );
}

#[test]
fn the_replay_ignores_changes_after_now() {
    let t = Thresholds::default();
    let changes = [(2.0 * HOUR, counts(1, 1)), (10.0 * HOUR, counts(1, 1))];
    let alarm = replay(
        &t,
        Standing::Lowered,
        counts(10, 1),
        &changes,
        0.0,
        3.0 * HOUR,
    );
    assert_eq!(
        (alarm.raised, alarm.over, alarm.since, alarm.counts),
        (false, false, 2.0 * HOUR, counts(1, 1))
    );
}

#[test]
fn replays_from_raised_and_from_lowered_agree_once_a_span_decides_and_not_before() {
    let t = Thresholds::default();
    let from = [Standing::Raised, Standing::Lowered];
    // Three hours over, half an hour at the ratio, three over: no span decides, until four hours
    // over since the dip raise it from either.
    let changes = [(3.0 * HOUR, counts(8, 4)), (3.5 * HOUR, counts(10, 4))];
    let at =
        |now: f64| from.map(|opened| replay(&t, opened, counts(10, 4), &changes, 0.0, now).raised);
    assert_eq!(at(6.5 * HOUR - 1.0), [true, false], "each stands as it was");
    assert_eq!(at(7.5 * HOUR), [true, true]);
    // An hour at the ratio clears it from either.
    let cleared = [(HOUR, counts(8, 4))];
    let clear = from.map(|opened| replay(&t, opened, counts(10, 4), &cleared, 0.0, 2.0 * HOUR));
    assert_eq!(clear.map(|alarm| alarm.raised), [false, false]);
}

#[test]
fn the_lookback_is_a_day_or_twice_the_spans() {
    assert_eq!(Thresholds::default().lookback(), LOOKBACK_SECONDS);
    let long = Thresholds {
        ratio: 2,
        raise_hours: 20,
        clear_hours: 4,
    };
    assert_eq!(long.lookback(), 48.0 * HOUR);
}

#[test]
fn held_since_is_the_start_of_the_last_unbroken_run_of_the_status() {
    let timelines = HashMap::from([
        (
            "a",
            vec![
                (0.0, Some("review")),
                (10.0, Some("in_progress")),
                (20.0, Some("review")),
            ],
        ),
        ("b", vec![(0.0, Some("review")), (15.0, Some("review"))]),
        ("c", vec![(0.0, Some("review")), (15.0, Some("done"))]),
        ("d", vec![(0.0, None), (5.0, Some("review"))]),
    ]);
    let since = held_since(&timelines, "review");
    assert_eq!(since, HashMap::from([("a", 20.0), ("b", 0.0), ("d", 5.0)]));
}

/// The line the landing tool writes recording `text` as what a landing waits on.
pub(super) fn waits_line(sid: i64, ts: f64, text: &str) -> Line {
    line(
        "sim/a",
        sid,
        ts,
        "system",
        &format!("land{WAITS_SET}{text}"),
    )
}

/// Revision `revision` of `slug`, made at `at`, putting it where it projects to `status`.
fn transition(slug: &str, revision: u64, status: &str, at: f64) -> crate::workflow::Moved {
    crate::workflow::Moved {
        slug: slug.into(),
        revision,
        status: status.into(),
        at,
    }
}

#[test]
fn a_held_tasks_transitions_say_each_change_of_the_status_it_projects_to_as_a_move() {
    let transitions = [
        // Placed, claimed, blocked at a position that keeps its status, submitted: the first
        // places it, and the block changes nothing the alarm counts.
        transition("sim/a", 1, "ready", 0.0),
        transition("sim/a", 2, "in_progress", HOUR),
        transition("sim/a", 3, "in_progress", 2.0 * HOUR),
        transition("sim/a", 4, "review", 3.0 * HOUR),
        // Another task's first transition is no move from sim/a's last.
        transition("sim/b", 1, "ready", 3.5 * HOUR),
        transition("sim/b", 2, "in_progress", 4.0 * HOUR),
    ];
    let said = transition_lines(&transitions, 0.0);
    let read: Vec<(&str, f64, Step<'_>)> = (said.iter())
        .map(|l| (l.slug.as_str(), l.ts, step(l)))
        .collect();
    assert_eq!(
        read,
        [
            ("sim/a", HOUR, Step::Between("ready", "in_progress")),
            ("sim/a", 3.0 * HOUR, Step::Between("in_progress", "review")),
            ("sim/b", 4.0 * HOUR, Step::Between("ready", "in_progress")),
        ]
    );
    assert_eq!(
        said[0].text,
        "workflow moved it ready → in_progress (sim/a@2)"
    );
    // Only those made at the window's start or later; the one before still says where it was.
    let since = transition_lines(&transitions, 3.0 * HOUR);
    let steps: Vec<Step<'_>> = since.iter().map(step).collect();
    assert_eq!(
        steps,
        [
            Step::Between("in_progress", "review"),
            Step::Between("ready", "in_progress")
        ]
    );
    // Its timeline walks back through them as through the board's own moves.
    let lines: Vec<&Line> = said.iter().filter(|l| l.slug == "sim/a").collect();
    assert_eq!(
        timeline("review", 0.0, &lines, 0.0, None),
        [
            (0.0, Some("ready")),
            (HOUR, Some("in_progress")),
            (3.0 * HOUR, Some("review"))
        ]
    );
}
