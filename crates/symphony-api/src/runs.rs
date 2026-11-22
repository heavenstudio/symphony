//! A project's runs: one [`RunLine`] per run and per step of one, as the board's recorder (`tasks
//! run`) appends them to the day files `symphony/runs/YYYY-MM-DD.jsonl` in the git common dir, and
//! as the Runs page, the roll-up and the send to another machine's server read them
//! (docs/workflow/RUNS.md).
//!
//! A run or a step writes a start line once its command started ([`RunLine::end`] none), then an
//! end line with the same [`RunLine::run`] and its end. A reader takes each run once ([`fold`]): its
//! first end line, else its newest start line. A field added later is optional, so a reader built
//! before it reads a newer line, and one after it reads an older line.

use serde::{Deserialize, Serialize};

/// [`RunLine::schema`] as written now.
pub const RUN_SCHEMA: u32 = 1;

/// The pipelines Symphony's own recipes and recorder name: a verify, a lint, a test, a client
/// build, a Cargo build no recipe names, the command a task moving to review starts, a landing, a
/// release run. A project may name others ([`run_name`]).
pub const PIPELINES: [&str; 8] = [
    "verify",
    "lint",
    "test",
    "client",
    "build",
    "review-callback",
    "landing",
    "release",
];

/// What started a run when no session did: the review callback, a release run, a landing, the owner.
pub const TRIGGERS: [&str; 4] = ["on_review", "release", "land", "owner"];

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// Exit 0.
    Green,
    /// Any other exit, or a crash by a signal of its own (`SIGSEGV`, `SIGABRT` and the like).
    Red,
    /// Stopped for its time: exit 124 (what `timeout` exits with), or `SIGALRM`, `SIGVTALRM` or
    /// `SIGXCPU`.
    TimedOut,
    /// Stopped from outside: `SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGKILL`, `SIGPIPE` or `SIGTERM`.
    Interrupted,
    /// Its command never started, or how it ended is not known (its recorder ended first).
    Incomplete,
}

/// `SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGKILL`, `SIGPIPE`, `SIGTERM`: their numbers on Linux and the
/// BSDs alike.
const STOPPED: [i32; 6] = [1, 2, 3, 9, 13, 15];
/// `SIGALRM`, `SIGXCPU`, `SIGVTALRM`, the same on both.
const TIMERS: [i32; 3] = [14, 24, 26];

impl End {
    /// How a command ended, by its exit code or the signal that ended it: `started` when it ran at
    /// all. An exit of 128 + N is a shell's report of a command that signal N ended, read as N.
    pub fn of(started: bool, exit: Option<i32>, signal: Option<i32>) -> End {
        if !started {
            return End::Incomplete;
        }
        match (exit, signal) {
            (_, Some(signal)) => End::by_signal(signal),
            (Some(0), None) => End::Green,
            (Some(124), None) => End::TimedOut,
            (Some(code @ 129..=159), None) => End::by_signal(code - 128),
            (Some(_), None) => End::Red,
            (None, None) => End::Incomplete,
        }
    }

    fn by_signal(signal: i32) -> End {
        if STOPPED.contains(&signal) {
            End::Interrupted
        } else if TIMERS.contains(&signal) {
            End::TimedOut
        } else {
            End::Red
        }
    }
}

/// One test binary's results, as its run's log says them (docs/workflow/RUNS.md, *Tests*).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Suite {
    /// Cargo's words for it, the executable's hash cut: `unittests src/lib.rs (symphony_board)`,
    /// `tests/cli.rs (cli)`, `Doc-tests symphony_board`.
    pub name: String,
    pub passed: u32,
    pub failed: u32,
    pub ignored: u32,
    /// Its time, in seconds.
    pub s: f64,
    /// Whether it ended with no result line (it crashed, or its run was stopped first): never a
    /// pass, its counts and time those of the results it said, none.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
}

/// A test that failed, as its run's log says it (docs/workflow/RUNS.md, *Tests*).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct FailedTest {
    /// The binary it is in, when known.
    #[serde(default)]
    pub suite: Option<String>,
    pub name: String,
    /// Its failure's first line.
    pub line: String,
}

/// One line of a day file: a run, or a step of one, as it started or as it ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RunLine {
    /// [`RUN_SCHEMA`].
    pub schema: u32,
    /// 32 hex digits: the id of the operation that recorded it, which its live record in the work
    /// folder carries too.
    pub run: String,
    /// The run or step this is a step of; none for a run.
    pub parent: Option<String>,
    /// The project's id.
    pub project: Option<String>,
    /// The machine's installation id, when it has one.
    pub machine: Option<String>,
    /// Its run's pipeline ([`PIPELINES`]), a step's too.
    pub pipeline: String,
    /// The step's name; none for a run.
    pub step: Option<String>,
    /// The recipe or the command line.
    pub label: String,
    pub task: Option<String>,
    /// The session it ran for.
    pub handle: Option<String>,
    /// Who or what started it: a session's handle, or one of [`TRIGGERS`].
    pub trigger: String,
    pub branch: Option<String>,
    /// The commit it checked.
    pub commit: Option<String>,
    /// That commit's tree.
    pub tree: Option<String>,
    /// Whether the checkout differed from its commit (tracked changes, or files git does not
    /// ignore) when it began: its tree then says only what it started from.
    pub dirty: Option<bool>,
    /// When it was asked for, started and ended, in seconds since the epoch on the recording
    /// machine's clock.
    pub queued: f64,
    pub started: Option<f64>,
    pub ended: Option<f64>,
    /// None on a start line.
    pub end: Option<End>,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    /// The CPU seconds, user and system, of its command and every child it reaped; a run's
    /// include its steps'.
    pub cpu_s: Option<f64>,
    /// Beside `cpu_s`, whether its recorder knows it leaves out CPU the command's processes took
    /// (`true`), saw no such loss (`false`), or could not watch for one (absent): docs/workflow/RUNS.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_partial: Option<bool>,
    /// The newest earlier run of the same pipeline, step and label on the same tree, both clean.
    pub rerun_of: Option<String>,
    /// Its log, relative to the day files' folder: `logs/YYYY-MM-DD/RUN.log`, the day it was asked
    /// for. Named on its start line too, so a run that still runs can be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// The bytes its log held as its end line was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_bytes: Option<u64>,
    /// Per test binary its log names, on an end line; absent when it names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suites: Option<Vec<Suite>>,
    /// The tests that failed, beside `suites`: empty when none did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<Vec<FailedTest>>,
}

impl RunLine {
    /// Whether it says the run started, not how it ended.
    pub fn is_start(&self) -> bool {
        self.end.is_none()
    }

    /// The day file it goes in: the UTC day it ended, else the one it started (or was asked for).
    pub fn day(&self) -> String {
        run_day(self.ended.or(self.started).unwrap_or(self.queued))
    }
}

/// The read of one run's log: `GET` [`RUN_LOG_PATH`]`?run=ID` answers its last [`RUN_LOG_TAIL`]
/// bytes, `&tail=N` its last N, `&from=A` (and `&to=B`) the bytes from A (to B); never more than
/// [`RUN_LOG_MOST`] at once (docs/workflow/RUNS.md, *Logs*).
pub const RUN_LOG_PATH: &str = "/api/run-log";
/// What a read of a log that names no range answers: its tail, this many bytes.
pub const RUN_LOG_TAIL: u64 = 64 * 1024;
/// The most bytes of a log one read answers.
pub const RUN_LOG_MOST: u64 = 1024 * 1024;

/// Part of one run's log, as [`RUN_LOG_PATH`] answers it: the bytes `from..to` of the `size` its
/// file held when read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RunLog {
    /// The run's id.
    pub run: String,
    /// The log, relative to the day files' folder, as [`RunLine::log`] names it.
    pub log: String,
    /// Its whole length when read, in bytes: a run that still runs adds to it.
    pub size: u64,
    /// Where the bytes answered start in it.
    pub from: u64,
    /// Where they end, never past `size`.
    pub to: u64,
    /// Those bytes as text; a character a range cuts through reads as U+FFFD.
    pub text: String,
}

/// The Runs page's list: `GET` [`RUNS_PATH`] answers a [`RunsPage`], the project's runs (never
/// their steps) newest first by when each was asked for, [`RUNS_PAGE`] at a time, read from its
/// newest day files back [`RUNS_DAYS`] days (docs/workflow/RUNS.md, *The Runs page*). It takes `pipeline`,
/// `end` (an [`End`], or `started` for a run with no end line yet), `task` and `branch`, each
/// matched whole, and `before`, a time: the runs asked for before it, for the page after one whose
/// last run was asked for then.
pub const RUNS_PATH: &str = "/api/runs";
/// One run and its steps: `GET` [`RUN_PATH`]`?run=ID` answers a [`RunPage`].
pub const RUN_PATH: &str = "/api/run";
/// How many runs a page of the list holds.
pub const RUNS_PAGE: usize = 50;
/// How many days back, today among them, the list and a run's read reach.
pub const RUNS_DAYS: u32 = 14;
/// The most bytes of one day file read; a larger file is not read, and named.
pub const RUNS_FILE_BYTES: u64 = 16 << 20;
/// The most bytes read for one answer, newest days first; the files past it are not read, and named.
pub const RUNS_READ_BYTES: u64 = 64 << 20;

/// A page of the project's runs, as [`RUNS_PATH`] answers it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RunsPage {
    /// The runs that match, newest first by [`RunLine::queued`], each once ([`fold`]): its end
    /// line, else its start line.
    pub runs: Vec<RunLine>,
    /// Whether more runs match, asked for before the last of these.
    pub more: bool,
    /// The pipelines the runs read name, by name: what the page offers to filter by.
    pub pipelines: Vec<String>,
    /// The day files that were not read, each a sentence naming it: their runs are missing.
    pub unread: Vec<String>,
}

/// One run, or one step, and the steps it ran, as [`RUN_PATH`] answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RunPage {
    pub run: RunLine,
    /// Its steps and theirs, each after the one it ran inside, in the order they were asked for.
    pub steps: Vec<RunLine>,
    /// The day files that were not read, each a sentence naming it.
    pub unread: Vec<String>,
}

/// A pipeline's or a step's name: 1 to 64 ASCII letters, digits, `-`, `_` or `.`.
pub fn run_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The UTC day of `at`, seconds since the epoch, as a day file names it: `YYYY-MM-DD`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a day count since 1970 of any time a clock says fits an i64"
)]
pub fn run_day(at: f64) -> String {
    // Days since 1970-01-01, then the civil date of that day (Howard Hinnant's algorithm).
    let days = (at / 86_400.0).floor() as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Each run of `lines` once, in the order first seen: its first end line, else its newest start
/// line. A second end line is only ever a sweep's of a recorder that ended between writing its own
/// and finishing its record, so the first is how it ended.
pub fn fold(lines: impl IntoIterator<Item = RunLine>) -> Vec<RunLine> {
    let mut order: Vec<String> = Vec::new();
    let mut runs: std::collections::HashMap<String, RunLine> = std::collections::HashMap::new();
    for line in lines {
        match runs.get_mut(&line.run) {
            None => {
                order.push(line.run.clone());
                runs.insert(line.run.clone(), line);
            }
            Some(kept) if kept.is_start() => *kept = line,
            Some(_) => {}
        }
    }
    order
        .into_iter()
        .filter_map(|run| runs.remove(&run))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(run: &str, end: Option<End>) -> RunLine {
        RunLine {
            schema: RUN_SCHEMA,
            run: run.into(),
            parent: None,
            project: Some("symphony".into()),
            machine: None,
            pipeline: "verify".into(),
            step: None,
            label: "just verify".into(),
            task: None,
            handle: None,
            trigger: "owner".into(),
            branch: None,
            commit: None,
            tree: None,
            dirty: None,
            queued: 1_794_400_000.0,
            started: Some(1_794_400_001.0),
            ended: end.map(|_| 1_794_400_100.0),
            end,
            exit: end.map(|_| 0),
            signal: None,
            cpu_s: None,
            cpu_partial: None,
            rerun_of: None,
            log: None,
            log_bytes: None,
            suites: None,
            failed: None,
        }
    }

    #[test]
    fn an_end_is_green_red_timed_out_interrupted_or_incomplete() {
        for (started, exit, signal, end) in [
            (true, Some(0), None, End::Green),
            (true, Some(1), None, End::Red),
            (true, Some(101), None, End::Red),
            (true, Some(128), None, End::Red),
            (true, Some(255), None, End::Red),
            (true, Some(124), None, End::TimedOut),
            (true, Some(130), None, End::Interrupted),
            (true, Some(143), None, End::Interrupted),
            (true, Some(137), None, End::Interrupted),
            (true, Some(142), None, End::TimedOut),
            (true, Some(139), None, End::Red),
            (true, Some(160), None, End::Red),
            (true, None, Some(2), End::Interrupted),
            (true, None, Some(1), End::Interrupted),
            (true, None, Some(3), End::Interrupted),
            (true, None, Some(9), End::Interrupted),
            (true, None, Some(13), End::Interrupted),
            (true, None, Some(15), End::Interrupted),
            (true, None, Some(14), End::TimedOut),
            (true, None, Some(24), End::TimedOut),
            (true, None, Some(26), End::TimedOut),
            (true, None, Some(11), End::Red),
            (true, None, Some(6), End::Red),
            (true, None, None, End::Incomplete),
            (false, Some(127), None, End::Incomplete),
            (false, Some(0), None, End::Incomplete),
        ] {
            assert_eq!(End::of(started, exit, signal), end, "{exit:?} {signal:?}");
        }
    }

    #[test]
    fn a_day_is_the_utc_date_of_a_time() {
        for (at, day) in [
            (0.0, "1970-01-01"),
            (86_399.9, "1970-01-01"),
            (86_400.0, "1970-01-02"),
            (951_782_400.0, "2000-02-29"),
            (1_709_164_800.0, "2024-02-29"),
            (1_794_441_599.0, "2026-11-11"),
            (1_794_441_600.0, "2026-11-12"),
            (1_798_761_600.0, "2027-01-01"),
            (-1.0, "1969-12-31"),
        ] {
            assert_eq!(run_day(at), day, "{at}");
        }
        let mut ended = line("a", Some(End::Green));
        assert_eq!(ended.day(), "2026-11-11");
        ended.ended = Some(1_794_441_600.0);
        assert_eq!(
            ended.day(),
            "2026-11-12",
            "an end line goes in the day it ended"
        );
        let mut asked = line("b", None);
        asked.started = None;
        asked.queued = 1_794_441_600.0;
        assert_eq!(asked.day(), "2026-11-12");
    }

    #[test]
    fn a_name_is_short_plain_ascii() {
        for good in ["verify", "review-callback", "client_check", "a.b", "x"] {
            assert!(run_name(good), "{good}");
        }
        let long = "a".repeat(65);
        for bad in ["", "two words", "a/b", "é", "line\nbreak", long.as_str()] {
            assert!(!run_name(bad), "{bad:?}");
        }
        assert!(run_name(&"a".repeat(64)));
        assert!(PIPELINES.iter().all(|name| run_name(name)));
    }

    #[test]
    fn a_line_reads_back_as_written_and_leaves_out_what_no_one_filled() {
        let mut ended = line("4f1c", Some(End::TimedOut));
        ended.step = Some("tests".into());
        ended.cpu_s = Some(12.5);
        let text = serde_json::to_string(&ended).unwrap();
        assert!(text.contains("\"end\":\"timed_out\""), "{text}");
        for later in [
            "\"cpu_partial\"",
            "\"log\"",
            "\"log_bytes\"",
            "\"suites\"",
            "\"failed\"",
        ] {
            assert!(!text.contains(later), "{later} in {text}");
        }
        assert_eq!(serde_json::from_str::<RunLine>(&text).unwrap(), ended);
        // Said, the flag sits beside the CPU it qualifies.
        ended.cpu_partial = Some(true);
        let text = serde_json::to_string(&ended).unwrap();
        assert!(
            text.contains("\"cpu_s\":12.5,\"cpu_partial\":true,\"rerun_of\""),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<RunLine>(&text).unwrap(), ended);
        // A field a later writer adds is ignored, and an optional one it leaves out is none.
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["added_later"] = serde_json::json!(true);
        value.as_object_mut().unwrap().remove("rerun_of");
        value["suites"] = serde_json::json!([
            {"name": "symphony-board::cli", "passed": 3, "failed": 1, "ignored": 0, "s": 4.5}
        ]);
        let read: RunLine = serde_json::from_value(value).unwrap();
        assert_eq!(read.rerun_of, None);
        let suites = read.suites.unwrap();
        assert_eq!((suites[0].failed, suites[0].error), (1, false));
        // A suite's error is said only when it had one.
        let mut suite = suites[0].clone();
        let text = serde_json::to_string(&suite).unwrap();
        assert!(!text.contains("error"), "{text}");
        suite.error = true;
        let text = serde_json::to_string(&suite).unwrap();
        assert!(text.ends_with(",\"error\":true}"), "{text}");
        assert_eq!(serde_json::from_str::<Suite>(&text).unwrap(), suite);
    }

    #[test]
    fn a_fold_takes_each_run_once_by_its_first_end_line_else_its_newest_start() {
        let mut restarted = line("b", None);
        restarted.started = Some(1_794_400_050.0);
        let mut swept = line("a", Some(End::Incomplete));
        swept.exit = None;
        let folded = fold([
            line("a", None),
            line("b", None),
            line("a", Some(End::Green)),
            swept,
            restarted.clone(),
            line("c", Some(End::Red)),
        ]);
        assert_eq!(
            folded,
            vec![
                line("a", Some(End::Green)),
                restarted,
                line("c", Some(End::Red))
            ]
        );
    }
}
