use super::*;
use crate::rpc::Client;
use serde_json::json;
use std::fs;

/// The start of a minute, in seconds since the epoch.
const T0: f64 = 1_800_000_000.0;

const SLOW: &str = "RPC query at crates/tasks/src/store/callbacks.rs:590, elapsed 3.001s, configured wait 3.000s: no answer";

/// A folder of its own under the temporary one, for test `name`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("symphony-health-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn env(vars: &[(&str, &str)], cwd: &Path) -> Env {
    Env {
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        cwd: cwd.to_path_buf(),
    }
}

#[test]
fn passes_are_counted_and_warned_of_while_the_newest_failed_within_ten_minutes() {
    let mut passes = Passes::default();
    passes.record(T0, Ok(()));
    passes.record(T0 + 1.0, Err("the first"));
    passes.record(T0 + 2.0, Err(SLOW));
    let report = passes.report(T0 + 3.0);
    assert_eq!(
        report,
        PassReport {
            ok: 1,
            failed: 2,
            last_at: Some(T0 + 2.0),
            last_failed: true,
            failed_at: Some(T0 + 2.0),
            error: Some(SLOW.into()),
            failing_since: Some(T0 + 1.0),
            ok_since: None,
        }
    );
    assert!(report.failing(T0 + 3.0));
    assert!(report.failing(T0 + 2.0 + RECENT));
    assert!(
        !report.failing(T0 + 2.0 + RECENT + 1.0),
        "a failure ten minutes old is no longer warned of"
    );
    assert_eq!(
        report.warning("callback dispatch", T0 + 61.0),
        Some(format!(
            "callback dispatch failing for 60 s (2 of 3 passes in the last hour failed): {SLOW}"
        ))
    );

    passes.record(T0 + 4.0, Ok(()));
    let report = passes.report(T0 + 5.0);
    assert!(report.failing(T0 + 5.0), "one good pass does not clear it");
    assert_eq!(
        report.warning("callback dispatch", T0 + 5.0),
        Some(format!(
            "callback dispatch failing for 4 s (2 of 4 passes in the last hour failed), ok again \
             for 1 s: {SLOW}"
        ))
    );
    assert!(report.failing(T0 + 4.0 + CLEARS_AFTER - 0.5));
    assert!(
        !report.failing(T0 + 4.0 + CLEARS_AFTER),
        "a minute of good passes clears it, with no pass at the minute"
    );
    let cleared = passes.report(T0 + 4.0 + CLEARS_AFTER);
    assert_eq!(cleared.warning("callback dispatch", T0 + 64.0), None);
    assert_eq!(
        (cleared.ok, cleared.failed, cleared.failing_since),
        (2, 2, None),
        "the hour still counts the failures"
    );
    assert_eq!(cleared.error.as_deref(), Some(SLOW));
    passes.record(T0 + 70.0, Err("again"));
    assert_eq!(
        passes.report(T0 + 70.0).failing_since,
        Some(T0 + 70.0),
        "a failure after a minute of good passes starts a new spell"
    );
}

#[test]
fn a_board_failing_every_other_tick_is_warned_of_throughout_until_a_minute_of_good_passes() {
    // The clock is the test's: each pass names the time it ended, a tick a second.
    let recorder = Recorder::default();
    let mut turned = Vec::new();
    let mut was = [false, false];
    let mut tick = |at: f64, outcome: Result<(), &str>| {
        recorder.dispatched(at, outcome);
        let failing = recorder.failing(at);
        if failing != was {
            turned.push((at, failing[0]));
            was = failing;
        }
    };
    for second in 0..180 {
        let outcome = if second % 2 == 0 { Err(SLOW) } else { Ok(()) };
        tick(T0 + f64::from(second), outcome);
    }
    for second in 180..=200 {
        tick(T0 + f64::from(second), Ok(()));
    }
    assert_eq!(
        recorder.report(T0 + 200.0).warnings,
        [format!(
            "callback dispatch failing for 3 min (90 of 201 passes in the last hour failed), ok \
             again for 21 s: {SLOW}"
        )],
        "the spell is said from its first failure"
    );
    for second in 201..300 {
        tick(T0 + f64::from(second), Ok(()));
    }
    assert_eq!(
        turned,
        [(T0, true), (T0 + 179.0 + CLEARS_AFTER, false)],
        "on at the first failure, off a minute after the good passes began at 179 s, never between"
    );
}

#[test]
fn a_board_not_made_or_with_no_store_nor_surrealdb_is_absent_and_any_other_failure_is_not() {
    use crate::schema::{Access, Opened, VERSION, decide};
    use crate::server::ServerError;
    let program = crate::Program::named("tasks", "/repo/.target/bin/tasks");
    let refused = |found| match decide(found, VERSION, Access::Read, Some("demo"), &program) {
        Opened::Refused(why) => why,
        other => panic!("{other:?}"),
    };
    let pinned = std::path::Path::new("/repo/.git")
        .join(crate::folders::SURREAL)
        .join(crate::server::SURREAL_VERSION)
        .join("surreal");
    for error in [
        refused(None),
        ServerError::NoSurreal(pinned.clone()).to_string(),
        ServerError::NoSurreal(PathBuf::from("/opt/surreal")).to_string(),
    ] {
        assert!(absent(&error), "{error}");
        let wrapped = format!("no pass, the board could not be reached: {error}");
        assert!(absent(&wrapped), "its wrapper keeps its words: {wrapped}");
    }
    // The same missing SurrealDB with the board's store there is a board that cannot be served.
    let data = PathBuf::from("/repo/.git")
        .join(crate::folders::TASKS)
        .join("data");
    for error in [
        ServerError::Unserved {
            surreal: pinned,
            data: data.clone(),
        }
        .to_string(),
        ServerError::Unserved {
            surreal: PathBuf::from("/opt/surreal"),
            data,
        }
        .to_string(),
        refused(Some(VERSION - 1)),
        ServerError::NotUp("the server did not come up".into()).to_string(),
        "connection refused".to_string(),
        SLOW.to_string(),
        String::new(),
    ] {
        assert!(
            !absent(&error),
            "a board that is there and failing: {error}"
        );
    }
}

#[test]
fn a_reader_refused_a_board_behind_it_or_two_ahead_is_refused_by_schema_and_nothing_else_is() {
    use crate::schema::{Access, Opened, VERSION, decide};
    let program = crate::Program::named("tasks", "/repo/.target/bin/tasks");
    let opened = |found, access| decide(found, VERSION, access, Some("demo"), &program);
    let refused = |found| match opened(found, Access::Read) {
        Opened::Refused(why) => why,
        other => panic!("{other:?}"),
    };
    for error in [refused(Some(VERSION - 1)), refused(Some(VERSION + 2))] {
        assert!(refused_by_schema(&error), "{error}");
        let wrapped = format!("no pass, the board could not be reached: {error}");
        assert!(refused_by_schema(&wrapped), "its wrapper keeps its words");
    }
    assert_eq!(opened(Some(VERSION + 1), Access::Read), Opened::Ahead);
    let Opened::Refused(writer) = opened(Some(VERSION + 1), Access::Write) else {
        panic!("a writer refuses a board ahead of it");
    };
    for error in [
        refused(None),
        writer,
        "connection refused".to_string(),
        SLOW.to_string(),
        String::new(),
    ] {
        assert!(!refused_by_schema(&error), "{error}");
    }
}

#[test]
fn an_error_is_kept_on_one_line_and_a_long_one_cut_to_its_first_and_last_characters() {
    assert_eq!(
        shortened("  the board did not answer\n"),
        "the board did not answer"
    );
    assert_eq!(
        shortened("exit status: 3: Compiling tasks\r\nerror: no answer"),
        "exit status: 3: Compiling tasks ⏎ error: no answer"
    );
    let line = |n: usize| format!("é{n:04} build output line\n");
    let stderr: String = (0..400).map(line).collect::<String>() + "error: the board did not answer";
    let error = format!("exit status: 3: {stderr}");
    let kept = shortened(&error);
    let whole = error.trim().replace('\n', " ⏎ ");
    let left = whole.chars().count() - ERROR_CHARS;
    let marker = format!(" … [{left} characters left out] … ");
    assert_eq!(kept.chars().count(), ERROR_CHARS + marker.chars().count());
    assert!(!kept.contains('\n'), "{kept}");
    let (head, end) = kept.split_once(&marker).unwrap();
    assert_eq!(
        (head.chars().count(), end.chars().count()),
        (ERROR_HEAD, ERROR_CHARS - ERROR_HEAD)
    );
    assert!(
        head.starts_with("exit status: 3: é0000 build output line ⏎ "),
        "{head}"
    );
    assert!(end.ends_with("⏎ error: the board did not answer"), "{end}");
    assert!(whole.starts_with(head) && whole.ends_with(end));

    // The record keeps it so, for the status line and the pages alike.
    let mut passes = Passes::default();
    passes.record(T0, Err(&error));
    assert_eq!(passes.report(T0).error, Some(kept));
}

#[test]
fn two_threads_recording_at_once_write_whole_records_and_the_newest_lands_last() {
    use std::sync::atomic::AtomicBool;
    const PASSES: u32 = 400;
    let dir = scratch("threads");
    let path = dir.join(FILE);
    let recorder = Arc::new(Recorder::writing(path.clone()));
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (path, done) = (path.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut whole = 0_u64;
            while !done.load(Ordering::SeqCst) {
                match Report::read(&path) {
                    Ok(Some(_)) => whole += 1,
                    Ok(None) => {}
                    Err(torn) => panic!("a reader met a record not whole: {torn}"),
                }
            }
            whole
        })
    };
    // The clock's thread and the sweep's: a sweep writes the record every time.
    let writers: Vec<_> = [0.0, 0.5]
        .into_iter()
        .map(|offset| {
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || {
                for pass in 0..PASSES {
                    recorder.swept(T0 + f64::from(pass) + offset, Ok(()));
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    done.store(true, Ordering::SeqCst);
    let whole = reader.join().unwrap();
    assert!(whole > 0, "the reader read the record while it was written");
    assert_eq!(recorder.writer().unwritten, None, "no write failed");
    assert_eq!(
        Report::read(&path).unwrap().unwrap().sweep.ok,
        u64::from(2 * PASSES),
        "the record that landed last holds every pass"
    );
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [FILE], "no partial file stays");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_minute_leaves_the_counts_an_hour_after_it_and_the_memory_once_a_hundred_and_twenty_follow() {
    let mut passes = Passes::default();
    passes.record(T0, Err("old"));
    passes.record(T0 + 59.0, Ok(()));
    let held = passes.report(T0 + 3599.0);
    assert_eq!(
        (held.ok, held.failed),
        (1, 1),
        "the 60th minute still holds it"
    );
    passes.record(T0 + 3600.0, Ok(()));
    let report = passes.report(T0 + 3600.0);
    assert_eq!(
        (report.ok, report.failed, report.failed_at, report.error),
        (1, 0, None, None)
    );
    // A day on, the minute out of the window is still kept, for the hour's line.
    passes.record(T0 + 86_400.0, Ok(()));
    assert_eq!(passes.minutes.len(), 3, "however long ago it was");
    assert_eq!(passes.within(&(minute(T0)..=minute(T0))), (1, 1));
    assert!(passes.holds(minute(T0)));
    for k in 1..=117 {
        passes.record(T0 + 86_400.0 + 60.0 * f64::from(k), Ok(()));
    }
    assert_eq!(passes.minutes.len(), KEPT_MINUTES);
    assert!(passes.holds(minute(T0)), "a hundred and twenty are kept");
    passes.record(T0 + 86_400.0 + 60.0 * 118.0, Ok(()));
    assert_eq!(passes.minutes.len(), KEPT_MINUTES);
    assert_eq!(passes.within(&(minute(T0)..=minute(T0))), (0, 0));
    assert!(
        !passes.holds(minute(T0)) && passes.holds(minute(T0) + 1),
        "the one let go, and none after it"
    );
    assert_eq!(
        passes.within(&(minute(T0 + 3600.0)..=minute(T0 + 3600.0))),
        (1, 0)
    );
}

#[test]
fn the_requests_keep_their_newest_minutes_and_a_span_from_one_let_go_reads_as_none() {
    let log = QueryLog::default();
    let here = Location::caller();
    let kept = u32::try_from(KEPT_MINUTES).unwrap();
    let first = minute(T0);
    assert!(
        log.whole_within(&(first..=first + 59))
            .is_some_and(|(requests, _)| requests.count == 0),
        "none let go yet"
    );
    for k in 0..=kept {
        log.record(Query {
            at: T0 + 60.0 * f64::from(k),
            ms: 1.0,
            source: here,
            write: k % 2 == 0,
            failed: false,
        });
    }
    assert_eq!(
        log.whole_within(&(first..=first + 59)),
        None,
        "its first minute went"
    );
    assert_eq!(log.within(&(first..=first)).0.count, 0);
    let (requests, writes) = log.whole_within(&(first + 1..=first + 60)).unwrap();
    assert_eq!((requests.count, writes.count), (60, 30));
}

#[test]
fn a_clock_set_back_counts_its_pass_in_the_newest_minute() {
    let mut passes = Passes::default();
    passes.record(T0 + 120.0, Ok(()));
    passes.record(T0, Err("set back"));
    let report = passes.report(T0 + 120.0);
    assert_eq!((report.ok, report.failed), (1, 1));
    assert_eq!(passes.minutes.len(), 1);
    assert_eq!(report.last_at, Some(T0));
}

#[test]
fn requests_give_the_slowest_with_its_line_and_writes_their_percentiles() {
    let mut queries = Queries::default();
    let read = Location::caller();
    let write = Location::caller();
    let latencies = [5.0, 1.0, 9.0, 3.0, 7.0, 2.0, 10.0, 4.0, 8.0, 6.0];
    for (i, ms) in latencies.into_iter().enumerate() {
        queries.record(Query {
            at: T0 + i as f64,
            ms,
            source: write,
            write: true,
            failed: false,
        });
    }
    queries.record(Query {
        at: T0 + 61.0,
        ms: 3000.5,
        source: read,
        write: false,
        failed: true,
    });
    queries.record(Query {
        at: T0 + 62.0,
        ms: SLOW_MS,
        source: write,
        write: false,
        failed: false,
    });
    let (requests, writes) = queries.report(T0 + 62.0);
    let slowest = SlowQuery {
        ms: 3000.5,
        source: format!("{}:{}", read.file(), read.line()),
        at: T0 + 61.0,
        failed: true,
    };
    assert_eq!(
        requests,
        QueryReport {
            count: 12,
            slow: 2,
            failed: 1,
            slowest: Some(slowest.clone()),
        }
    );
    assert_eq!(
        writes,
        WriteReport {
            count: 10,
            p50_ms: Some(5.0),
            p90_ms: Some(9.0),
        }
    );
    // An hour after the first minute, its writes and their latencies are out of the window.
    let (requests, writes) = queries.report(T0 + 3600.0);
    assert_eq!(
        (requests.count, requests.slowest, writes),
        (2, Some(slowest), WriteReport::default())
    );
}

#[test]
fn the_percentiles_read_the_newest_writes_only() {
    let mut queries = Queries::default();
    let here = Location::caller();
    for i in 0..WRITE_SAMPLES + 10 {
        queries.record(Query {
            at: T0,
            ms: if i < 10 { 900.0 } else { 1.0 },
            source: here,
            write: true,
            failed: false,
        });
    }
    let (_, writes) = queries.report(T0);
    assert_eq!(writes.count, WRITE_SAMPLES as u64 + 10);
    assert_eq!(
        (writes.p50_ms, writes.p90_ms),
        (Some(1.0), Some(1.0)),
        "the oldest, slow, writes gave way"
    );
    assert_eq!(queries.writes.len(), WRITE_SAMPLES);
}

#[test]
fn a_percentile_is_the_nearest_rank() {
    assert_eq!(percentile(&[], 50), None);
    assert_eq!(percentile(&[4.0], 50), Some(4.0));
    assert_eq!(percentile(&[4.0], 90), Some(4.0));
    assert_eq!(percentile(&[1.0, 2.0], 50), Some(1.0));
    assert_eq!(percentile(&[1.0, 2.0], 90), Some(2.0));
}

#[test]
fn an_age_reads_in_seconds_minutes_or_hours() {
    let said: Vec<String> = [-3.0, 0.0, 89.4, 96.0, 600.0, 5340.0, 7200.0]
        .into_iter()
        .map(ago)
        .collect();
    assert_eq!(
        said,
        ["0 s", "0 s", "89 s", "2 min", "10 min", "89 min", "2 h"]
    );
}

#[test]
fn every_client_of_a_board_counts_its_requests_in_the_boards_one_log() {
    let client = crate::store_fixture::client();
    let other = Client::new(client.addr(), "sf", client.database());
    let elsewhere = crate::store_fixture::client();
    assert!(Arc::ptr_eq(&client.queries(), &other.queries()));
    assert!(!Arc::ptr_eq(&client.queries(), &elsewhere.queries()));

    let before = crate::sweep::trigger::now();
    client
        .define_then("CREATE thing SET n = 1 RETURN NONE;", json!({}))
        .unwrap();
    other.query("SELECT * FROM thing;", json!({})).unwrap();
    let (requests, writes) = client.queries().report(crate::sweep::trigger::now());
    assert_eq!(
        (requests.count, requests.failed, writes.count),
        (2, 0, 1),
        "one read, one write"
    );
    let slowest = requests.slowest.unwrap();
    assert!(
        slowest
            .source
            .starts_with("crates/tasks/src/board_health/tests.rs:"),
        "the line that sent it, through the client's tracked callers: {}",
        slowest.source
    );
    assert!(slowest.at >= before && slowest.ms >= 0.0);
    assert!(writes.p50_ms.is_some());
    assert_eq!(elsewhere.queries().report(slowest.at).0.count, 0);
}

#[test]
fn a_request_that_got_no_answer_is_counted_failed() {
    let client = Client::new("127.0.0.1:1", "sf", "health_refused");
    assert!(client.query("SELECT * FROM thing;", json!({})).is_err());
    let (requests, writes) = client.queries().report(crate::sweep::trigger::now());
    assert_eq!((requests.count, requests.failed, writes.count), (1, 1, 0));
    assert!(requests.slowest.unwrap().failed);
}

/// A recorder whose dispatch failed twice, its sweep ok, its requests one slow unanswered read.
fn failing_recorder() -> (Recorder, Arc<QueryLog>) {
    let recorder = Recorder::default();
    let log = Arc::new(QueryLog::default());
    log.record(Query {
        at: T0 + 2.0,
        ms: 3001.0,
        source: Location::caller(),
        write: false,
        failed: true,
    });
    log.record(Query {
        at: T0 + 3.0,
        ms: 4.0,
        source: Location::caller(),
        write: true,
        failed: false,
    });
    recorder.reading(Arc::clone(&log));
    recorder.dispatched(T0, Ok(()));
    recorder.dispatched(T0 + 1.0, Err("the first"));
    recorder.dispatched(T0 + 2.0, Err(SLOW));
    recorder.swept(T0, Ok(()));
    (recorder, log)
}

#[test]
fn a_report_warns_of_a_failing_dispatch_and_says_it_line_by_line() {
    let (recorder, log) = failing_recorder();
    let report = recorder.report(T0 + 62.0);
    let warning = format!(
        "callback dispatch failing for 61 s (2 of 3 passes in the last hour failed): {SLOW}"
    );
    assert_eq!(report.warnings, std::slice::from_ref(&warning));
    assert_eq!(
        (report.at, report.pid, report.window_seconds),
        (T0 + 62.0, std::process::id(), WINDOW)
    );
    assert_eq!((report.sweep.ok, report.sweep.failed), (1, 0));
    assert_eq!(report.queries, log.report(T0 + 62.0).0);
    let slowest = report.queries.slowest.clone().unwrap().source;
    assert_eq!(
        report.lines(T0 + 82.0, &report.warnings),
        [
            format!(
                "board health, as the Symphony server (pid {}) recorded it 20 s ago, over the last hour:",
                std::process::id()
            ),
            format!("  callback dispatch: 1 ok, 2 failed; the last failed 80 s ago: {SLOW}"),
            "  idle sweep: 1 ok, 0 failed".to_string(),
            format!(
                "  board requests: 2, 1 slow (1 s or more), 1 failed; the slowest 3001 ms at {slowest}, 80 s ago, unanswered"
            ),
            "  writes: 1, p50 4.0 ms, p90 4.0 ms".to_string(),
            format!("⚠ {warning}"),
        ]
    );
    assert_eq!(recorder.failing(T0 + 62.0), [true, false]);
    recorder.dispatched(T0 + 63.0, Ok(()));
    assert_eq!(
        recorder.failing(T0 + 63.0),
        [true, false],
        "one good pass does not clear it"
    );
    assert_eq!(
        recorder.report(T0 + 64.0).warnings,
        [format!(
            "callback dispatch failing for 63 s (2 of 4 passes in the last hour failed), ok again \
             for 1 s: {SLOW}"
        )]
    );
    recorder.dispatched(T0 + 63.0 + CLEARS_AFTER, Ok(()));
    assert_eq!(
        recorder.report(T0 + 63.0 + CLEARS_AFTER).warnings,
        Vec::<String>::new(),
        "a minute of good passes clears it"
    );
    assert_eq!(recorder.failing(T0 + 63.0 + CLEARS_AFTER), [false, false]);
}

#[test]
fn a_failing_sweep_is_warned_of_and_a_quiet_one_said_unrecorded() {
    let recorder = Recorder::default();
    recorder.swept(T0, Err("tasks reap: timed out after 240 s"));
    let report = recorder.report(T0 + 1.0);
    assert_eq!(recorder.failing(T0 + 1.0), [false, true]);
    assert_eq!(
        recorder.failing(T0 + RECENT + 1.0),
        [false, false],
        "ten minutes on, with no sweep since"
    );
    assert_eq!(
        report.warnings,
        [
            "idle sweep failing for 1 s (1 of 1 passes in the last hour failed): tasks reap: timed out after 240 s"
        ]
    );
    assert_eq!(
        report.lines(T0 + 1.0, &[])[1..3],
        [
            "  callback dispatch: 0 ok, 0 failed; none recorded".to_string(),
            "  idle sweep: 0 ok, 1 failed; the last failed 1 s ago: tasks reap: timed out after 240 s"
                .to_string(),
        ]
    );
}

#[test]
fn a_reader_warns_when_the_record_stopped_and_whether_its_server_runs() {
    let report = Report {
        at: T0,
        pid: 4242,
        ..Report::default()
    };
    assert_eq!(
        report.read_warnings(T0 + STALE, |_| false),
        Vec::<String>::new()
    );
    let stopped = "the Symphony server (pid 4242) last recorded the board's health 61 s ago";
    let since = ": no callback dispatch pass is recorded since";
    assert_eq!(
        report.read_warnings(T0 + STALE + 1.0, |pid| pid == 4242),
        [format!("{stopped}{since}")]
    );
    assert_eq!(
        report.read_warnings(T0 + STALE + 1.0, |_| false),
        [format!("{stopped}, and is not running{since}")]
    );
}

#[test]
fn a_record_is_written_whole_and_read_back() {
    let dir = scratch("record");
    let path = dir.join("symphony/tasks").join(FILE);
    assert_eq!(Report::read(&path), Ok(None));
    let (recorder, _) = failing_recorder();
    let report = recorder.report(T0 + 5.0);
    report.write(&path).unwrap();
    assert_eq!(Report::read(&path), Ok(Some(report)));
    let names: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [FILE], "no partial file stays");
    fs::write(&path, "{not json").unwrap();
    let refused = Report::read(&path).unwrap_err();
    assert!(
        refused.starts_with(&path.display().to_string()),
        "{refused}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_writing_recorder_writes_when_due_or_when_failing_turns() {
    let dir = scratch("persist");
    let path = dir.join(FILE);
    let recorder = Recorder::writing(path.clone());
    assert_eq!(recorder.file(), Some(path.clone()));
    let written_at = || Report::read(&path).unwrap().map(|report| report.at);
    let taken = || {
        let at = Report::read(&path).unwrap().map(|report| report.at);
        let _ = fs::remove_file(&path);
        at
    };

    recorder.dispatched(T0, Ok(()));
    assert_eq!(taken(), Some(T0), "the first pass writes it");
    recorder.dispatched(T0 + 1.0, Ok(()));
    assert_eq!(written_at(), None, "nothing changed and it is not due");
    recorder.dispatched(T0 + 2.0, Err(SLOW));
    assert_eq!(taken(), Some(T0 + 2.0), "the dispatch started failing");
    recorder.dispatched(T0 + 3.0, Err(SLOW));
    assert_eq!(written_at(), None, "still failing, and not due");
    recorder.dispatched(T0 + 12.0, Err(SLOW));
    assert_eq!(taken(), Some(T0 + 12.0), "due ten seconds after the last");
    recorder.dispatched(T0 + 13.0, Ok(()));
    assert_eq!(
        written_at(),
        None,
        "one good pass: still failing, and not due"
    );
    recorder.dispatched(T0 + 70.0, Ok(()));
    let due = Report::read(&path).unwrap().unwrap();
    assert_eq!(
        (due.at, due.warnings.len()),
        (T0 + 70.0, 1),
        "due, and failing until a minute of good passes"
    );
    let _ = fs::remove_file(&path);
    recorder.dispatched(T0 + 13.0 + CLEARS_AFTER, Ok(()));
    let cleared = Report::read(&path).unwrap().unwrap();
    assert_eq!(
        (cleared.at, cleared.warnings),
        (T0 + 13.0 + CLEARS_AFTER, Vec::new()),
        "the dispatch stopped failing: written though not due"
    );
    let _ = fs::remove_file(&path);
    recorder.swept(T0 + 74.0, Ok(()));
    assert_eq!(taken(), Some(T0 + 74.0), "a sweep always writes it");

    let unwritable = Recorder::writing(dir.join(FILE).join("under-a-file"));
    fs::write(dir.join(FILE), "a file, not a folder").unwrap();
    unwritable.dispatched(T0, Ok(()));
    assert!(
        unwritable.writer().unwritten.is_some(),
        "a write that failed is kept to be said once"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_board_that_went_back_is_warned_of_first_and_written_at_once_until_it_opens_again() {
    let dir = scratch("went-back");
    let path = dir.join(FILE);
    let recorder = Recorder::writing(path.clone());
    recorder.dispatched(T0, Err(SLOW));
    let _ = fs::remove_file(&path);
    let line = "it went back from schema 16, so what was written since is lost: nothing was \
                written; do not run `just tasks migrate`; the Symphony's \
                docs/workflow/TASKS.md#a-board-that-went-back says how to recover it";

    assert!(!recorder.went_back(T0 + 1.0, None), "nothing changed");
    assert_eq!(Report::read(&path), Ok(None), "and nothing was written");
    assert!(recorder.went_back(T0 + 1.0, Some(line.into())));
    assert_eq!(recorder.rollback().as_deref(), Some(line));
    let written = Report::read(&path).unwrap().unwrap();
    assert_eq!(written.at, T0 + 1.0, "written though not due");
    assert_eq!(written.rollback.as_deref(), Some(line));
    assert_eq!(
        written.read_warnings(T0 + 2.0, |_| true)[0],
        format!("the task board is unwell: {line}"),
        "first, as `tasks status` reads it"
    );
    assert_eq!(written.warnings.len(), 2, "{:?}", written.warnings);
    let _ = fs::remove_file(&path);
    assert!(!recorder.went_back(T0 + 2.0, Some(line.into())));
    assert_eq!(Report::read(&path), Ok(None), "still the same: not written");

    assert!(
        recorder.went_back(T0 + 3.0, None),
        "a store opened it again"
    );
    let cleared = Report::read(&path).unwrap().unwrap();
    assert_eq!((cleared.at, cleared.rollback), (T0 + 3.0, None));
    assert!(
        !fs::read_to_string(&path).unwrap().contains("rollback"),
        "a board that did not go back says nothing of it"
    );
    assert_eq!(recorder.report(T0 + 3.0).warnings.len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_recorder_that_writes_nothing_still_reports() {
    let recorder = Recorder::default();
    recorder.dispatched(T0, Ok(()));
    assert_eq!(recorder.file(), None);
    assert_eq!(recorder.report(T0).dispatch.ok, 1);
}

#[test]
fn the_record_of_a_board_is_in_its_common_dir_named_by_its_database() {
    let dir = scratch("file");
    fs::create_dir_all(dir.join(".git")).unwrap();
    let common = crate::git::common_dir(&dir).unwrap();
    let named = dir.join("named.json");
    let named = named.to_str().unwrap();
    assert_eq!(
        file(&env(
            &[(FILE_VAR, named), ("TASKS_SERVER", "127.0.0.1:9")],
            &dir
        )),
        Ok(PathBuf::from(named))
    );
    assert_eq!(
        file(&env(&[("TASKS_SERVER", "127.0.0.1:9")], &dir)),
        Err(
            "a board TASKS_SERVER names keeps no health record unless TASKS_HEALTH names one"
                .into()
        )
    );
    let live = common.join("symphony/tasks").join("health.json");
    assert_eq!(file(&env(&[], &dir)), Ok(live.clone()));
    assert_eq!(file(&env(&[("TASKS_DATABASE", "tasks")], &dir)), Ok(live));
    assert_eq!(
        file(&env(&[("TASKS_DATABASE", "scratch_1")], &dir)),
        Ok(common.join("symphony/tasks").join("health.scratch_1.json"))
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// A task of `status` at `slug` under `parent`, held by the orchestrators `held` of `health`.
fn task(
    slug: &str,
    parent: Option<&str>,
    status: &str,
    held: &[(&str, crate::Health)],
) -> crate::Task {
    crate::Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: String::new(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: parent.map(str::to_string),
        created_by: String::new(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: (held.iter())
            .map(|(handle, health)| crate::Role {
                handle: handle.to_string(),
                role: "orchestrator".into(),
                since: 0.0,
                expires: None,
                health: *health,
            })
            .collect(),
        instruction: false,
        fields: Default::default(),
    }
}

#[test]
fn a_report_counts_the_open_tasks_nobody_orchestrates_in_the_newest_board_read() {
    use crate::Health::{Live, Lost};
    let recorder = Recorder::default();
    let report = recorder.report(T0);
    assert_eq!(report.unowned, None, "no board read yet");
    assert!(
        !(report.lines(T0, &[]).iter()).any(|line| line.contains("no orchestrator")),
        "and none said"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("unowned").is_none(), "{json}");
    let board = crate::Board {
        tasks: vec![
            task("held", None, "in_progress", &[("amber", Live)]),
            task("held/below", Some("held"), "ready", &[]),
            task("left", None, "review", &[("ghost", Lost)]),
            task("free", None, "ready", &[]),
            task("finished", None, "done", &[]),
            task("dropped", None, "cancelled", &[]),
        ],
        sessions: Vec::new(),
        latest_message: 0,
    };
    recorder.unowned(&board);
    let report = recorder.report(T0 + 1.0);
    assert_eq!(report.unowned, Some(2), "left and free");
    let lines = report.lines(T0 + 1.0, &["it is unwell".into()]);
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "  open tasks no orchestrator holds: 2, as of the newest board read".to_string(),
            "⚠ it is unwell".to_string(),
        ]
    );
    // No defect, as `tasks status` reads the record: each waits for an orchestrator under the limit
    // on its open tasks that stays, a well state, counted and never warned of.
    assert!(report.read_warnings(T0 + 1.0, |_| true).is_empty());
    assert!(
        report.warnings(T0 + 1.0).is_empty(),
        "the record's own warnings, which the server's pages show, stay its passes'"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["unowned"], 2);
    let read: Report = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(read.unowned, Some(2));
    let mut older = json;
    older.as_object_mut().unwrap().remove("unowned");
    let read: Report = serde_json::from_value(older).unwrap();
    assert_eq!(
        read.unowned, None,
        "a record written before the count reads without it"
    );
    recorder.unowned(&crate::Board {
        tasks: Vec::new(),
        sessions: Vec::new(),
        latest_message: 0,
    });
    let settled = recorder.report(T0 + 2.0);
    assert_eq!(settled.unowned, Some(0), "the newest board decides");
    assert!(settled.read_warnings(T0 + 2.0, |_| true).is_empty());
}
