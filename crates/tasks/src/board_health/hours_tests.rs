//! The hours: a line for each hour a writer recorded, its counts and percentiles from the minutes
//! the recorder keeps; a restart's partial hours and the gap between them; a reload's hand-off,
//! whose two generations never count the same minute; a machine asleep, short or long, told from a
//! board slow to answer; a project with no board; the day files' retention, their writes and
//! reads; and no board request for any of it.

use super::*;
use crate::board_health::{KEPT_MINUTES, Query, QueryLog, Recorder};
use std::panic::Location;
use std::sync::{Arc, Mutex};
use symphony_api::run_day;

/// The start of an hour, and so of a minute, in seconds since the epoch.
const T0: f64 = 1_800_000_000.0;

/// A folder of its own under the temporary one, removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("symphony-hours-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What a recorder said went wrong.
type Told = Arc<Mutex<Vec<String>>>;

/// How long the machine of a test has slept, in seconds: its clock that stops while the machine
/// sleeps reads the server's clock less this.
type Slept = Arc<Mutex<f64>>;

/// A recorder reading `log` that keeps its hours in `folder` as `gate` lets it, on a machine that
/// never sleeps.
fn keeping(folder: &Path, gate: &Arc<Gate>, log: &Arc<QueryLog>) -> (Recorder, Told) {
    let (recorder, told, _) = sleeping(folder, gate, log);
    (recorder, told)
}

/// [`keeping`], on a machine the test puts to sleep: each second added to its [`Slept`] is one the
/// server's clock runs ahead of the one that stops while it sleeps.
fn sleeping(folder: &Path, gate: &Arc<Gate>, log: &Arc<QueryLog>) -> (Recorder, Told, Slept) {
    let recorder = Recorder::default();
    recorder.reading(Arc::clone(log));
    recorder.gated(Arc::clone(gate));
    let told = Told::default();
    let said = Arc::clone(&told);
    let slept = Slept::default();
    let asleep = Arc::clone(&slept);
    recorder.keep_hours(
        folder.to_path_buf(),
        move |now| now - *asleep.lock().unwrap(),
        move |line| said.lock().unwrap().push(line.to_string()),
    );
    (recorder, told, slept)
}

/// A request that ended at `at` and took `ms`.
fn request(log: &QueryLog, at: f64, ms: f64, write: bool, failed: bool) {
    log.record(Query {
        at,
        ms,
        source: Location::caller(),
        write,
        failed,
    });
}

/// A dispatch pass every `every` seconds from `from` up to, not including, `to`; those at the
/// times in `failing` failed.
fn dispatching(recorder: &Recorder, from: f64, to: f64, every: f64, failing: &[f64]) {
    let mut at = from;
    while at < to {
        let outcome = if failing.contains(&at) {
            Err("the board did not answer")
        } else {
            Ok(())
        };
        recorder.dispatched(at, outcome);
        at += every;
    }
}

/// Each line's span: its hour, from, to and whether it is partial.
fn spans(lines: &[HealthHour]) -> Vec<(f64, f64, f64, bool)> {
    lines
        .iter()
        .map(|line| (line.start, line.from, line.to, line.partial))
        .collect()
}

#[test]
fn two_hours_of_passes_failures_and_slow_requests_give_two_lines_with_their_counts_and_percentiles()
{
    let dir = Scratch::new("two-hours");
    let log = Arc::new(QueryLog::default());
    let (recorder, told) = keeping(&dir.0, &Arc::new(Gate::default()), &log);
    // Its first call, just before the hour: it records from the first whole minute after.
    recorder.dispatched(T0 - 0.5, Ok(()));

    // The first hour: a pass every 10 s, three failing; a sweep every five minutes, one failing;
    // ten writes, a slow read and a read that got no answer.
    for (minute, ms) in (0..10).zip([5.0, 1.0, 9.0, 3.0, 7.0, 2.0, 10.0, 4.0, 8.0, 6.0]) {
        request(&log, T0 + 60.0 * f64::from(minute), ms, true, false);
    }
    request(&log, T0 + 1000.0, 1500.0, false, false);
    request(&log, T0 + 2000.0, 3000.5, false, true);
    dispatching(
        &recorder,
        T0,
        T0 + 3600.0,
        10.0,
        &[T0 + 600.0, T0 + 610.0, T0 + 1800.0],
    );
    for k in 0..12 {
        let at = T0 + 150.0 + 300.0 * f64::from(k);
        recorder.swept(at, if k == 3 { Err("reap failed") } else { Ok(()) });
    }
    assert!(
        read(&dir.0, &run_day(T0)).is_empty(),
        "nothing is written before the hour ends"
    );

    // The second: every pass ok, no sweep, eleven writes, one of them slow.
    for ms in 20..30 {
        request(
            &log,
            T0 + 3600.0 + 60.0 * f64::from(ms - 20),
            f64::from(ms),
            true,
            false,
        );
    }
    request(&log, T0 + 6600.0, 1200.0, true, false);
    dispatching(&recorder, T0 + 3600.0, T0 + 7200.0, 10.0, &[]);
    recorder.dispatched(T0 + 7200.5, Ok(()));

    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0, T0 + 3600.0, false),
            (T0 + 3600.0, T0 + 3600.0, T0 + 7200.0, false)
        ]
    );
    let source = |line: &HealthHour| line.requests.slowest.as_ref().unwrap().source.clone();
    assert!(
        source(&lines[0]).starts_with("crates/tasks/src/board_health/hours_tests.rs:"),
        "{}",
        source(&lines[0])
    );
    let first = HealthHour {
        schema: HEALTH_SCHEMA,
        at: T0 + 3600.0,
        pid: std::process::id(),
        generation: T0 - 0.5,
        start: T0,
        from: T0,
        to: T0 + 3600.0,
        partial: false,
        dispatch: HourPasses { ok: 357, failed: 3 },
        sweep: HourPasses { ok: 11, failed: 1 },
        requests: HourRequests {
            count: 12,
            slow: 2,
            unanswered: 1,
            slowest: Some(HourSlowest {
                ms: 3000.5,
                source: source(&lines[0]),
                at: T0 + 2000.0,
                unanswered: true,
            }),
        },
        writes: HourWrites {
            count: 10,
            p50_ms: Some(5.0),
            p90_ms: Some(9.0),
        },
    };
    assert_eq!(
        lines[0], first,
        "written by the first pass after the hour, before it is counted"
    );
    let second = HealthHour {
        at: T0 + 7200.5,
        start: T0 + 3600.0,
        from: T0 + 3600.0,
        to: T0 + 7200.0,
        dispatch: HourPasses { ok: 360, failed: 0 },
        sweep: HourPasses::default(),
        requests: HourRequests {
            count: 11,
            slow: 1,
            unanswered: 0,
            slowest: Some(HourSlowest {
                ms: 1200.0,
                source: source(&lines[1]),
                at: T0 + 6600.0,
                unanswered: false,
            }),
        },
        writes: HourWrites {
            count: 11,
            p50_ms: Some(25.0),
            p90_ms: Some(29.0),
        },
        ..first
    };
    assert_eq!(lines[1], second);
    assert!(
        told.lock().unwrap().is_empty(),
        "{:?}",
        told.lock().unwrap()
    );
    // `/api/health` still reads the last sixty minutes alone.
    let report = recorder.report(T0 + 7200.5);
    assert_eq!((report.dispatch.ok, report.sweep.ok), (355, 0));
}

#[test]
fn a_restart_mid_hour_writes_two_partial_lines_and_the_time_between_and_an_hour_down_have_none() {
    let dir = Scratch::new("restart");
    // The first server, from 5 s into the hour: it counts from the next whole minute, so its pass
    // and request before that are in no line.
    let before = Arc::new(QueryLog::default());
    let (first, _) = keeping(&dir.0, &Arc::new(Gate::default()), &before);
    request(&before, T0 + 30.0, 4.0, false, false);
    request(&before, T0 + 100.0, 4.0, false, false);
    dispatching(&first, T0 + 5.0, T0 + 1200.0, 10.0, &[]);
    first.stop_hours(T0 + 1200.5);
    first.dispatched(T0 + 1201.0, Ok(()));
    let stopped = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&stopped),
        [(T0, T0 + 60.0, T0 + 1200.5, true)],
        "its stop writes the hour so far, and it records nothing after"
    );
    assert_eq!(
        (stopped[0].dispatch.ok, stopped[0].requests.count),
        (114, 1)
    );

    // The next, started five minutes later: from the next whole minute again, to the hour's end.
    let after = Arc::new(QueryLog::default());
    let (next, _) = keeping(&dir.0, &Arc::new(Gate::default()), &after);
    dispatching(&next, T0 + 1500.5, T0 + 3600.0, 10.0, &[]);
    assert_eq!(
        read(&dir.0, &run_day(T0)).len(),
        1,
        "not before its hour ends"
    );
    dispatching(&next, T0 + 3600.5, T0 + 3700.0, 10.0, &[]);
    next.stop_hours(T0 + 3700.0);
    // A third, after an hour no server ran in.
    let (third, _) = keeping(&dir.0, &Arc::new(Gate::default()), &after);
    dispatching(
        &third,
        T0 + 3.0 * 3600.0 + 10.0,
        T0 + 4.0 * 3600.0 + 1.0,
        10.0,
        &[],
    );

    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0 + 60.0, T0 + 1200.5, true),
            (T0, T0 + 1560.0, T0 + 3600.0, true),
            (T0 + 3600.0, T0 + 3600.0, T0 + 3700.0, true),
            (
                T0 + 3.0 * 3600.0,
                T0 + 3.0 * 3600.0 + 60.0,
                T0 + 4.0 * 3600.0,
                true
            ),
        ],
        "the minutes no server counted have no line, and the hour none ran in none at all"
    );
    assert_eq!(lines[1].dispatch.ok, 204, "its passes from 26 minutes in");
    assert_eq!(lines[2].dispatch.ok, 10);
    assert!(
        lines[0].generation < lines[1].generation && lines[1].generation < lines[3].generation,
        "each writer names when it began"
    );
}

#[test]
fn a_reload_counts_the_old_generation_to_its_close_and_the_replacement_from_its_commit() {
    let dir = Scratch::new("hand-off");
    let (old_gate, new_gate) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
    let (old_log, new_log) = (Arc::new(QueryLog::default()), Arc::new(QueryLog::default()));
    let (old, _) = keeping(&dir.0, &old_gate, &old_log);
    // The replacement's gate is held before its hubs start.
    new_gate.hold();
    let (new, _) = keeping(&dir.0, &new_gate, &new_log);

    dispatching(&old, T0, T0 + 1790.0, 1.0, &[]);
    // Both are alive while the replacement reads the board and says it is ready.
    for second in 0..10 {
        let at = T0 + 1790.0 + f64::from(second);
        old.dispatched(at, Ok(()));
        new.dispatched(at + 0.1, Ok(()));
        request(&new_log, at + 0.05, 2.0, false, false);
    }
    old_gate.close(T0 + 1800.3);
    new_gate.open(T0 + 1800.4);
    // The old one drains a few seconds more, then stops; the replacement goes on.
    dispatching(&old, T0 + 1800.5, T0 + 1805.0, 1.0, &[]);
    old.stop_hours(T0 + 1805.0);
    for second in [0.5, 70.0] {
        request(&new_log, T0 + 1800.0 + second, 3.0, false, false);
    }
    dispatching(&new, T0 + 1800.5, T0 + 3601.0, 1.0, &[]);

    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0, T0 + 1800.0, true),
            (T0, T0 + 1860.0, T0 + 3600.0, true)
        ],
        "the old to the minute it closed in, the replacement from the first whole minute after its \
         commit: no minute in both"
    );
    assert_eq!(
        (lines[0].dispatch.ok, lines[0].requests.count),
        (1800, 0),
        "the old one's passes up to its close"
    );
    assert_eq!(
        (lines[1].dispatch.ok, lines[1].requests.count),
        (1740, 1),
        "the replacement's own passes and requests before then are in no line"
    );
}

#[test]
fn a_hand_off_that_fails_after_the_close_records_again_from_the_next_whole_minute() {
    let dir = Scratch::new("failed-hand-off");
    let gate = Arc::new(Gate::default());
    let log = Arc::new(QueryLog::default());
    let (recorder, _) = keeping(&dir.0, &gate, &log);
    dispatching(&recorder, T0, T0 + 600.0, 1.0, &[]);
    gate.close(T0 + 600.2);
    // Opening is a replacement's, when its hand-off commits: it leaves a close as it was.
    gate.open(T0 + 600.3);
    dispatching(&recorder, T0 + 600.5, T0 + 630.0, 1.0, &[]);
    gate.reopen(T0 + 630.2);
    gate.reopen(T0 + 660.5);
    gate.open(T0 + 660.5);
    dispatching(&recorder, T0 + 630.5, T0 + 3601.0, 1.0, &[]);
    assert_eq!(
        spans(&read(&dir.0, &run_day(T0))),
        [
            (T0, T0, T0 + 600.0, true),
            (T0, T0 + 660.0, T0 + 3600.0, true)
        ],
        "the time it was closed is in no line; a gate open already stays as it was"
    );

    let missed = Scratch::new("missed-close");
    let gate = Arc::new(Gate::default());
    let (recorder, _) = keeping(&missed.0, &gate, &log);
    dispatching(&recorder, T0, T0 + 600.0, 1.0, &[]);
    // Closed and opened again between two of its calls: its process recorded the whole time,
    // and no replacement did, so its line goes on.
    gate.close(T0 + 600.1);
    gate.reopen(T0 + 600.2);
    dispatching(&recorder, T0 + 600.5, T0 + 3601.0, 1.0, &[]);
    assert_eq!(
        spans(&read(&missed.0, &run_day(T0))),
        [(T0, T0, T0 + 3600.0, false)]
    );
}

#[test]
fn a_replacement_that_never_commits_writes_nothing() {
    let dir = Scratch::new("never-commits");
    let gate = Arc::new(Gate::default());
    gate.hold();
    // Reopening is a closed generation's, whose hand-off failed: it leaves a hold as it was.
    gate.reopen(T0);
    let (recorder, _) = keeping(&dir.0, &gate, &Arc::new(QueryLog::default()));
    dispatching(&recorder, T0, T0 + 4000.0, 10.0, &[]);
    recorder.stop_hours(T0 + 4000.0);
    assert!(!dir.path(&format!("{}.jsonl", run_day(T0))).exists());
}

#[test]
fn a_machine_asleep_ends_its_line_with_the_minute_it_fell_asleep_in_and_goes_on_from_the_one_it_woke_in()
 {
    let dir = Scratch::new("sleep");
    let log = Arc::new(QueryLog::default());
    let (recorder, _, slept) = sleeping(&dir.0, &Arc::new(Gate::default()), &log);
    dispatching(&recorder, T0, T0 + 620.0, 10.0, &[]);
    // Asleep from 5 s after its last call, at T0 + 610, until T0 + 1530: the clock that stops
    // while it sleeps ran 5 s between the two calls, the server's 920 s.
    let woke = T0 + 1530.0;
    *slept.lock().unwrap() = woke - (T0 + 615.0);
    // A request it sent before it slept ends as it wakes, before the waking tick calls the recorder.
    request(&log, woke, 300_000.0, false, true);
    recorder.dispatched(woke, Ok(()));
    dispatching(&recorder, woke + 10.0, T0 + 3610.0, 10.0, &[]);
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0, T0 + 660.0, true),
            (T0, T0 + 1500.0, T0 + 3600.0, true)
        ],
        "no line holds the minutes it slept through"
    );
    assert_eq!(lines[0].at, woke, "written once it is called again");
    assert_eq!(
        (
            lines[0].dispatch.ok,
            lines[1].dispatch.ok,
            lines[1].requests.unanswered
        ),
        (62, 207, 1)
    );
    assert_eq!(
        lines[0].generation, lines[1].generation,
        "one writer, asleep a while"
    );
}

#[test]
fn a_board_slow_to_answer_holds_its_ticks_for_minutes_and_the_line_goes_on_whole() {
    let dir = Scratch::new("slow-board");
    let log = Arc::new(QueryLog::default());
    let (recorder, _, slept) = sleeping(&dir.0, &Arc::new(Gate::default()), &log);
    // Each tick waits on three requests the board never answers, each the client's whole 300 s:
    // its calls are 901 s apart, and both clocks run all the while, but for a doze of 50 s.
    let mut at = T0;
    while at < T0 + 3600.0 {
        if at > T0 + 1000.0 {
            *slept.lock().unwrap() = 50.0;
        }
        recorder.dispatched(at, Err("the board did not answer"));
        for k in 1..=3 {
            request(&log, at + 300.0 * f64::from(k), 300_000.0, false, true);
        }
        at += 901.0;
    }
    recorder.dispatched(at, Ok(()));
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(spans(&lines), [(T0, T0, T0 + 3600.0, false)]);
    assert_eq!(
        (
            lines[0].dispatch.failed,
            lines[0].requests.count,
            lines[0].requests.unanswered
        ),
        (4, 11, 11),
        "every request of the hour, the last of the fourth tick's in the next"
    );
}

#[test]
fn a_long_sleep_keeps_the_line_of_the_hour_it_fell_asleep_in() {
    let dir = Scratch::new("long-sleep");
    let log = Arc::new(QueryLog::default());
    let (recorder, _, slept) = sleeping(&dir.0, &Arc::new(Gate::default()), &log);
    for minute in 0..41 {
        request(&log, T0 + 60.0 * f64::from(minute) + 5.0, 2.0, false, false);
    }
    dispatching(&recorder, T0, T0 + 2460.0, 10.0, &[]);
    // Eight hours asleep, from 5 s after its last call; the waking tick's own requests are
    // counted before it calls the recorder, and the hour it slept in has its line all the same.
    let wake = T0 + 8.0 * 3600.0;
    *slept.lock().unwrap() = wake - (T0 + 2455.0);
    request(&log, wake + 0.2, 2.0, false, false);
    request(&log, wake + 0.4, 2.0, true, false);
    recorder.dispatched(wake + 0.5, Ok(()));
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(spans(&lines), [(T0, T0, T0 + 2460.0, true)]);
    assert_eq!((lines[0].dispatch.ok, lines[0].requests.count), (246, 41));
    // The minute it woke in, the last of the hour before, holds nothing: no line of zeros. The
    // hour after is whole.
    dispatching(&recorder, wake + 10.5, wake + 3601.0, 10.0, &[]);
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0, T0 + 2460.0, true),
            (wake, wake, wake + 3600.0, false)
        ]
    );
    assert_eq!((lines[1].dispatch.ok, lines[1].requests.count), (360, 2));
}

#[test]
fn a_span_whose_minutes_were_let_go_before_its_line_has_none_and_the_next_whole_one_its_own() {
    let dir = Scratch::new("let-go");
    let log = Arc::new(QueryLog::default());
    let (recorder, _) = keeping(&dir.0, &Arc::new(Gate::default()), &log);
    dispatching(&recorder, T0, T0 + 1800.0, 60.0, &[]);
    // Its ticks stop while the process's other clients ask the board every minute, for one minute
    // more than the recorder keeps: the first of them goes, and the hour it was in is short.
    let kept = u32::try_from(KEPT_MINUTES).unwrap();
    for k in 30..=30 + kept {
        request(&log, T0 + 60.0 * f64::from(k), 2.0, false, false);
    }
    recorder.dispatched(T0 + 60.0 * f64::from(31 + kept), Ok(()));
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [(T0 + 3600.0, T0 + 3600.0, T0 + 7200.0, false)],
        "no line counts less than its span held"
    );
    assert_eq!((lines[0].dispatch.ok, lines[0].requests.count), (0, 60));
}

#[test]
fn a_project_with_no_board_writes_no_line_of_zeros() {
    let dir = Scratch::new("no-board");
    let (recorder, _) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    // Its reconnects find no board to go back over; it passes nothing and asks nothing.
    let mut at = T0;
    while at < T0 + 3.0 * 3600.0 + 1.0 {
        recorder.went_back(at, None);
        at += 30.0;
    }
    recorder.stop_hours(at);
    assert!(!dir.path(&format!("{}.jsonl", run_day(T0))).exists());
}

#[test]
fn a_clock_set_back_before_the_line_began_ends_it_with_none() {
    let dir = Scratch::new("set-back");
    let (recorder, _) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    dispatching(&recorder, T0, T0 + 1800.0, 10.0, &[]);
    recorder.dispatched(T0 - 600.0, Ok(()));
    recorder.stop_hours(T0 - 500.0);
    assert!(
        read(&dir.0, &run_day(T0)).is_empty(),
        "nothing ends before it began"
    );
}

#[test]
fn day_files_older_than_ninety_days_go_once_a_day_and_nothing_else_does() {
    let dir = Scratch::new("prune");
    let now = T0 + 12.0 * 3600.0;
    let old = |days: f64| format!("{}.jsonl", run_day(now - days * 86_400.0));
    for name in [old(91.0), old(90.0), old(1.0), "notes.txt".into()] {
        fs::write(dir.path(&name), "{}\n").unwrap();
    }
    std::os::unix::fs::symlink(dir.path(&old(1.0)), dir.path(&old(95.0))).unwrap();
    let (recorder, _) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    recorder.dispatched(now, Ok(()));
    let mut names: Vec<String> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let mut kept = vec![old(95.0), old(90.0), old(1.0), "notes.txt".into()];
    kept.sort();
    assert_eq!(names, kept, "a link named like an old day stays");
    // Pruned once a day: a file older than kept, put back the same day, stays until the next.
    fs::write(dir.path(&old(91.0)), "{}\n").unwrap();
    recorder.dispatched(now + 60.0, Ok(()));
    assert!(dir.path(&old(91.0)).exists());
    recorder.dispatched(now + 86_400.0, Ok(()));
    assert!(!dir.path(&old(91.0)).exists());
}

#[test]
fn a_failed_append_is_said_once_for_each_change_of_what_it_is_and_its_lines_are_not_kept() {
    let dir = Scratch::new("unwritten");
    let day = dir.path(&format!("{}.jsonl", run_day(T0)));
    // The day file is a link: never written through.
    let target = dir.path("elsewhere.jsonl");
    std::os::unix::fs::symlink(&target, &day).unwrap();
    let (recorder, told) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    dispatching(&recorder, T0, T0 + 3.0 * 3600.0 + 1.0, 600.0, &[]);
    let said = told.lock().unwrap().clone();
    assert_eq!(
        said.len(),
        1,
        "the same failure twice is said once: {said:?}"
    );
    assert!(
        said[0].starts_with("· board health hours: ")
            && said[0].contains(&dir.0.display().to_string()),
        "{said:?}"
    );
    assert!(!target.exists(), "nothing went through the link");
    fs::remove_file(&day).unwrap();
    dispatching(&recorder, T0 + 11_400.0, T0 + 14_401.0, 600.0, &[]);
    assert_eq!(
        spans(&read(&dir.0, &run_day(T0))),
        [(
            T0 + 3.0 * 3600.0,
            T0 + 3.0 * 3600.0,
            T0 + 4.0 * 3600.0,
            false
        )],
        "the lines it could not write are not kept for later"
    );
}

#[test]
fn a_line_after_one_cut_short_starts_on_its_own_and_a_reader_passes_over_the_cut() {
    let dir = Scratch::new("cut");
    let day = dir.path(&format!("{}.jsonl", run_day(T0)));
    fs::write(&day, "{\"schema\":1,\"at\":").unwrap();
    let (recorder, _) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    dispatching(&recorder, T0, T0 + 3601.0, 60.0, &[]);
    let text = fs::read_to_string(&day).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert_eq!(
        spans(&read(&dir.0, &run_day(T0))),
        [(T0, T0, T0 + 3600.0, false)]
    );
    assert!(
        read(&dir.0, "2001-01-01").is_empty(),
        "no file reads as no lines"
    );
}

#[test]
fn the_hours_are_appended_from_memory_and_send_the_board_nothing() {
    let dir = Scratch::new("no-request");
    let client = crate::store_fixture::postgresql();
    client.make_database().unwrap();
    let log = client.queries();
    let (recorder, _) = keeping(&dir.0, &Arc::new(Gate::default()), &log);
    client
        .request(&[crate::pg::Step::bare("SELECT 1")])
        .unwrap();
    let every = i64::MIN..=i64::MAX;
    let sent = log.within(&every).0.count;
    assert!(sent > 0, "the request is counted");
    dispatching(&recorder, T0, T0 + 2.0 * 3600.0 + 1.0, 300.0, &[]);
    recorder.stop_hours(T0 + 2.0 * 3600.0 + 30.0);
    assert_eq!(read(&dir.0, &run_day(T0)).len(), 3);
    assert_eq!(
        log.within(&every).0.count,
        sent,
        "no client of the board sent it a request for the lines"
    );
}

#[test]
fn a_recorder_that_keeps_no_hours_writes_none() {
    let recorder = Recorder::default();
    recorder.dispatched(T0, Ok(()));
    recorder.dispatched(T0 + 3601.0, Ok(()));
    recorder.stop_hours(T0 + 3700.0);
    assert_eq!(recorder.hours_folder(), None);
}

#[test]
fn a_keeping_recorder_names_its_folder_nothing_appended_makes_none_and_bytes_not_text_read_as_none()
{
    let dir = Scratch::new("edges");
    let (recorder, _) = keeping(
        &dir.0,
        &Arc::new(Gate::default()),
        &Arc::new(QueryLog::default()),
    );
    assert_eq!(recorder.hours_folder(), Some(dir.0.clone()));
    assert!(format!("{recorder:?}").contains("Hours {"), "{recorder:?}");
    let unmade = dir.path("unmade");
    append(&unmade, &[]).unwrap();
    assert!(!unmade.exists(), "no lines, no folder");
    fs::write(dir.path(&format!("{}.jsonl", run_day(T0))), b"\xff\xfe\n").unwrap();
    assert!(read(&dir.0, &run_day(T0)).is_empty());
    let gate = Arc::new(Gate::default());
    recorder.gated(Arc::clone(&gate));
    assert!(
        recorder
            .gate()
            .is_some_and(|given| Arc::ptr_eq(&given, &gate)),
        "the gate it keeps to is the one it was given"
    );
}

#[test]
fn a_recorder_given_no_gate_keeps_no_hours() {
    let dir = Scratch::new("ungated");
    let recorder = Recorder::default();
    recorder.keep_hours(dir.0.clone(), |now| now, |_| {});
    assert_eq!(recorder.gate().map(|_| ()), None);
    dispatching(&recorder, T0, T0 + 2.0 * 3600.0 + 1.0, 60.0, &[]);
    recorder.stop_hours(T0 + 2.0 * 3600.0 + 30.0);
    assert!(
        read(&dir.0, &run_day(T0)).is_empty(),
        "held until its server's generation gives it its gate"
    );
}

#[test]
fn the_hours_of_a_board_are_in_its_common_dir_named_by_its_database() {
    let dir = Scratch::new("folder");
    fs::create_dir_all(dir.path(".git")).unwrap();
    let common = crate::git::common_dir(&dir.0).unwrap();
    let env = |vars: &[(&str, &str)]| Env {
        vars: vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        cwd: dir.0.clone(),
    };
    let named = dir.path("named");
    let named = named.to_str().unwrap();
    assert_eq!(
        folder(&env(&[
            (FOLDER_VAR, named),
            ("TASKS_SERVER", "127.0.0.1:9")
        ])),
        Ok(PathBuf::from(named))
    );
    assert_eq!(
        folder(&env(&[("TASKS_SERVER", "127.0.0.1:9")])),
        Err(
            "a board TASKS_SERVER names keeps no hours unless SYMPHONY_HEALTH names their folder"
                .into()
        )
    );
    assert_eq!(folder(&env(&[])), Ok(common.join("symphony/health")));
    assert_eq!(
        folder(&env(&[("TASKS_DATABASE", "tasks"), (FOLDER_VAR, "")])),
        Ok(common.join("symphony/health"))
    );
    assert_eq!(
        folder(&env(&[("TASKS_DATABASE", "scratch_1")])),
        Ok(common.join("symphony/health.scratch_1"))
    );
}
