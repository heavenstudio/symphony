//! The hours: a line for each hour a writer recorded, its counts and percentiles from the minutes
//! the recorder keeps; a restart's partial hours and the gap between them; a reload's hand-off,
//! whose two generations never count the same minute; a writer that went unwatched; the day files'
//! retention, their writes and reads; and no board request for any of it.

use super::*;
use crate::board_health::{Query, QueryLog, Recorder};
use std::panic::Location;
use symphony_api::run_day;

/// The start of an hour, and so of a minute, in seconds since the epoch.
const T0: f64 = 1_800_000_000.0;

/// A folder of its own under the temporary one, removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("sf-hours-{name}-{}", std::process::id()));
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

/// A recorder reading `log` that keeps its hours in `folder` as `gate` lets it.
fn keeping(folder: &Path, gate: &Arc<Gate>, log: &Arc<QueryLog>) -> (Recorder, Told) {
    let recorder = Recorder::default();
    recorder.reading(Arc::clone(log));
    let told = Told::default();
    let said = Arc::clone(&told);
    recorder.keep_hours(folder.to_path_buf(), Arc::clone(gate), move |line| {
        said.lock().unwrap().push(line.to_string());
    });
    (recorder, told)
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
        .map(|line| (line.hour, line.from, line.to, line.partial))
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
        hour: T0,
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
        hour: T0 + 3600.0,
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
fn a_writer_uncalled_past_the_gap_ends_its_line_at_its_last_minute_and_goes_on_from_its_next() {
    let dir = Scratch::new("gap");
    let log = Arc::new(QueryLog::default());
    let (recorder, _) = keeping(&dir.0, &Arc::new(Gate::default()), &log);
    dispatching(&recorder, T0, T0 + 620.0, 10.0, &[]);
    // Asleep: no call for longer than the gap. A request that ended as it woke is counted.
    let woke = T0 + 610.0 + GAP + 75.0;
    request(&log, woke - 1.0, 4000.0, false, true);
    recorder.dispatched(woke, Ok(()));
    dispatching(&recorder, woke + 10.0, T0 + 3610.0, 10.0, &[]);
    let lines = read(&dir.0, &run_day(T0));
    assert_eq!(
        spans(&lines),
        [
            (T0, T0, T0 + 660.0, true),
            (T0, T0 + 1560.0, T0 + 3600.0, true)
        ],
        "no line holds the minutes it did not watch"
    );
    assert_eq!(lines[0].at, woke, "written once it is called again");
    assert_eq!(lines[1].requests.unanswered, 1);

    // Asleep past the minutes the recorder keeps: the hour it fell asleep in is gone, and no line
    // of zeros stands for it.
    let long = Scratch::new("long-gap");
    let (recorder, _) = keeping(&long.0, &Arc::new(Gate::default()), &log);
    dispatching(&recorder, T0, T0 + 1800.0, 10.0, &[]);
    dispatching(
        &recorder,
        T0 + 3.0 * 3600.0,
        T0 + 4.0 * 3600.0 + 2.0,
        10.0,
        &[],
    );
    assert_eq!(
        spans(&read(&long.0, &run_day(T0))),
        [(
            T0 + 3.0 * 3600.0,
            T0 + 3.0 * 3600.0,
            T0 + 4.0 * 3600.0,
            false
        )]
    );
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
    let client = crate::store_fixture::client();
    let log = client.queries();
    let (recorder, _) = keeping(&dir.0, &Arc::new(Gate::default()), &log);
    client.query("RETURN 1;", serde_json::json!({})).unwrap();
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
