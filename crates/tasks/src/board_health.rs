//! The board's health as a process serving it sees it over the last hour: its callback dispatch
//! passes and idle sweeps, ok and failed, with the newest failure's error; the slowest request any
//! client in the process sent the board, with the line of code that sent it; and how long its
//! writes took. None of it is on the board, so a board too slow to answer still has it said. The
//! Symphony server keeps it in memory ([`Recorder`]) and serves it at `/api/health`; the machine's
//! one server also writes it to `sf-tasks/health.json` in the git common dir ([`file`]), which
//! `tasks status` reads ([`Report::read`]). Every request a [`crate::rpc::Client`] sends is counted
//! in the [`QueryLog`] of its board ([`queries_of`]), on the client's side, with no request of its
//! own.

use crate::cli::Env;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

/// How far back the counters reach, in seconds: the last sixty minutes, this one included.
pub const WINDOW: f64 = 3600.0;
/// How recent a failed pass must be to be warned of, in seconds.
pub const RECENT: f64 = 600.0;
/// How long, in seconds, a kind of pass must have been ok since its newest failure before its
/// warning clears: a board failing every other tick is warned of throughout, not on and off.
pub const CLEARS_AFTER: f64 = 60.0;
/// The most characters of an error the record keeps: its first [`ERROR_HEAD`] and its last ones.
pub const ERROR_CHARS: usize = 600;
/// How many of the characters kept of a longer error are its first.
const ERROR_HEAD: usize = 200;
/// A request that took this long, in milliseconds, is slow.
pub const SLOW_MS: f64 = 1000.0;
/// A record older than this, in seconds, says its server stopped recording: the one server writes
/// it every [`PERSIST_EVERY`] while its clock ticks, and its clock ticks every second.
pub const STALE: f64 = 60.0;
/// How often the one server writes the record while what it warns of stays the same, in seconds.
pub const PERSIST_EVERY: f64 = 10.0;
/// The record's name in `sf-tasks/` for the live database; another database's is
/// `health.<database>.json`.
pub const FILE: &str = "health.json";
/// The setting naming the record's file outright: a test's, or a scratch board's.
pub const FILE_VAR: &str = "TASKS_HEALTH";
/// How many of the hour's write latencies the percentiles are read from: the newest.
const WRITE_SAMPLES: usize = 4096;
/// The minutes the window holds.
const MINUTES: i64 = 60;

/// The minute `at`, in seconds since the epoch, falls in.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a time in seconds is far inside i64"
)]
fn minute(at: f64) -> i64 {
    (at / 60.0).floor() as i64
}

/// Whether the minute `bucket` is in the window ending in the minute of `now`.
fn in_window(bucket: i64, now: f64) -> bool {
    bucket > minute(now) - MINUTES
}

/// How long `seconds` is, said the way a person reads it: `42 s`, `13 min`, `3 h`.
pub fn ago(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds < 90.0 {
        format!("{seconds:.0} s")
    } else if seconds < 90.0 * 60.0 {
        format!("{:.0} min", seconds / 60.0)
    } else {
        format!("{:.0} h", seconds / 3600.0)
    }
}

/// `error` as the record keeps it, on one line: each line break said as ` ⏎ `, and an error longer
/// than [`ERROR_CHARS`] cut to its first and last characters, saying how many it leaves out. A
/// failed sweep's error carries the whole stderr of `tasks reap`, build output included; the
/// server's log keeps it whole.
pub fn shortened(error: &str) -> String {
    let error = error.trim().replace("\r\n", "\n").replace('\n', " ⏎ ");
    let count = error.chars().count();
    if count <= ERROR_CHARS {
        return error;
    }
    let tail = ERROR_CHARS - ERROR_HEAD;
    let head: String = error.chars().take(ERROR_HEAD).collect();
    let end: String = error.chars().skip(count - tail).collect();
    format!(
        "{head} … [{} characters left out] … {end}",
        count - ERROR_CHARS
    )
}

/// One kind of pass over the hour: each minute's ok and failed passes, and the newest pass and
/// failure.
#[derive(Debug, Default, Clone)]
pub struct Passes {
    /// `(minute, ok, failed)`, oldest first, none out of the window of the newest pass.
    minutes: VecDeque<(i64, u64, u64)>,
    /// When the newest pass ended, and whether it was ok.
    last: Option<(f64, bool)>,
    /// When the newest failure ended, and why it failed ([`shortened`]).
    failure: Option<(f64, String)>,
    /// When the first failure of this spell ended: the spell ends once the passes have been ok for
    /// [`CLEARS_AFTER`].
    failing_since: Option<f64>,
    /// When the first of the ok passes since the newest failure ended; none while the newest pass
    /// failed.
    ok_since: Option<f64>,
}

impl Passes {
    /// A pass that ended at `at`: ok, or failed for the reason given.
    pub fn record(&mut self, at: f64, outcome: Result<(), &str>) {
        let now = minute(at);
        while self
            .minutes
            .front()
            .is_some_and(|&(bucket, _, _)| bucket <= now - MINUTES)
        {
            self.minutes.pop_front();
        }
        // A clock set back counts its passes in the newest minute, never before it.
        if self.minutes.back().is_none_or(|back| back.0 < now) {
            self.minutes.push_back((now, 0, 0));
        }
        let Some(bucket) = self.minutes.back_mut() else {
            return;
        };
        // Passes ok for long enough ended the spell, whatever this one is.
        if self.ok_since.is_some_and(|ok| at - ok >= CLEARS_AFTER) {
            self.failing_since = None;
        }
        match outcome {
            Ok(()) => {
                bucket.1 += 1;
                self.ok_since.get_or_insert(at);
            }
            Err(why) => {
                bucket.2 += 1;
                self.failure = Some((at, shortened(why)));
                self.failing_since.get_or_insert(at);
                self.ok_since = None;
            }
        }
        self.last = Some((at, outcome.is_ok()));
    }

    /// What the window ending at `now` holds.
    pub fn report(&self, now: f64) -> PassReport {
        let (ok, failed) = self
            .minutes
            .iter()
            .filter(|(bucket, _, _)| in_window(*bucket, now))
            .fold((0, 0), |(ok, failed), (_, o, f)| (ok + o, failed + f));
        let failure = self
            .failure
            .as_ref()
            .filter(|(at, _)| in_window(minute(*at), now));
        PassReport {
            ok,
            failed,
            last_at: self.last.map(|(at, _)| at),
            last_failed: self.last.is_some_and(|(_, ok)| !ok),
            failed_at: failure.map(|(at, _)| *at),
            error: failure.map(|(_, why)| why.clone()),
            // The spell is over once the passes have been ok long enough, recorded or not.
            failing_since: self
                .failing_since
                .filter(|_| self.ok_since.is_none_or(|ok| now - ok < CLEARS_AFTER)),
            ok_since: self.ok_since,
        }
    }
}

/// One kind of pass, as the window has it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PassReport {
    /// Passes that were ok in the window.
    pub ok: u64,
    /// Passes that failed in the window.
    pub failed: u64,
    /// When the newest pass ended, in seconds since the epoch; none before the first.
    pub last_at: Option<f64>,
    /// Whether the newest pass failed.
    pub last_failed: bool,
    /// When the newest failure in the window ended.
    pub failed_at: Option<f64>,
    /// Why it failed.
    pub error: Option<String>,
    /// When the first failure of this spell ended: how long it has been failing. None once the
    /// passes have been ok for [`CLEARS_AFTER`].
    pub failing_since: Option<f64>,
    /// When the first of the ok passes since the newest failure ended; none while the newest pass
    /// failed.
    pub ok_since: Option<f64>,
}

impl PassReport {
    /// Whether it is failing at `now`: from a failed pass until the passes have been ok for
    /// [`CLEARS_AFTER`], so a board failing every other tick is failing throughout. A newest pass
    /// that failed more than [`RECENT`] before `now` is warned of no more.
    pub fn failing(&self, now: f64) -> bool {
        let Some(last_at) = self.last_at else {
            return false;
        };
        if self.last_failed {
            return now - last_at <= RECENT;
        }
        self.failing_since.is_some() && self.ok_since.is_some_and(|ok| now - ok < CLEARS_AFTER)
    }

    /// `what` failing at `now`, said with its counts and the newest error; none while it is not.
    fn warning(&self, what: &str, now: f64) -> Option<String> {
        if !self.failing(now) {
            return None;
        }
        let since = self
            .failing_since
            .map_or_else(String::new, |at| format!(" for {}", ago(now - at)));
        let recovering = match self.ok_since {
            Some(ok) if !self.last_failed => format!(", ok again for {}", ago(now - ok)),
            _ => String::new(),
        };
        let error = self.error.as_deref().unwrap_or("no error was said");
        Some(format!(
            "{what} failing{since} ({} of {} passes in the last hour failed){recovering}: {error}",
            self.failed,
            self.ok + self.failed
        ))
    }

    /// One line of `tasks status`: `what`, its counts, and the newest failure's age and error.
    fn line(&self, what: &str, now: f64) -> String {
        let mut line = format!("{what}: {} ok, {} failed", self.ok, self.failed);
        match (self.failed_at, &self.error) {
            (Some(at), Some(error)) => {
                line.push_str(&format!("; the last failed {} ago: {error}", ago(now - at)));
            }
            _ if self.last_at.is_none() => line.push_str("; none recorded"),
            _ => {}
        }
        line
    }
}

/// One request a client sent, as its [`QueryLog`] counts it.
#[derive(Debug, Clone, Copy)]
pub struct Query {
    /// When its answer, or its failure, came, in seconds since the epoch.
    pub at: f64,
    /// How long it took, in milliseconds, its one more sending included.
    pub ms: f64,
    /// The line that asked for it ([`crate::rpc::Client::query`] tracks its caller).
    pub source: &'static Location<'static>,
    /// Whether it may write ([`crate::rpc::Kind::Write`]).
    pub write: bool,
    /// Whether it got no answer.
    pub failed: bool,
}

/// The slowest request of the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlowQuery {
    /// How long it took, in milliseconds.
    pub ms: f64,
    /// The file and line that asked for it.
    pub source: String,
    /// When it ended, in seconds since the epoch.
    pub at: f64,
    /// Whether it got no answer: it took the client's whole wait.
    pub failed: bool,
}

/// The board's requests in a window.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryReport {
    /// Requests sent.
    pub count: u64,
    /// Of them, those that took [`SLOW_MS`] or longer.
    pub slow: u64,
    /// Of them, those that got no answer.
    pub failed: u64,
    /// The slowest.
    pub slowest: Option<SlowQuery>,
}

/// The board's writes in a window.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteReport {
    /// Requests that may write.
    pub count: u64,
    /// The median of their latencies in milliseconds, over the newest few thousand.
    pub p50_ms: Option<f64>,
    /// Their 90th percentile, over the same.
    pub p90_ms: Option<f64>,
}

#[derive(Debug, Default, Clone)]
struct QueryMinute {
    minute: i64,
    count: u64,
    slow: u64,
    failed: u64,
    writes: u64,
    slowest: Option<SlowQuery>,
}

/// The requests of one board over the hour.
#[derive(Debug, Default)]
pub struct Queries {
    /// Oldest first, none out of the window of the newest request.
    minutes: VecDeque<QueryMinute>,
    /// `(at, ms)` of the newest writes, oldest first.
    writes: VecDeque<(f64, f64)>,
}

impl Queries {
    pub fn record(&mut self, query: Query) {
        let now = minute(query.at);
        while self
            .minutes
            .front()
            .is_some_and(|bucket| bucket.minute <= now - MINUTES)
        {
            self.minutes.pop_front();
        }
        if self.minutes.back().is_none_or(|back| back.minute < now) {
            self.minutes.push_back(QueryMinute {
                minute: now,
                ..QueryMinute::default()
            });
        }
        let Some(bucket) = self.minutes.back_mut() else {
            return;
        };
        bucket.count += 1;
        bucket.slow += u64::from(query.ms >= SLOW_MS);
        bucket.failed += u64::from(query.failed);
        if bucket
            .slowest
            .as_ref()
            .is_none_or(|slowest| query.ms > slowest.ms)
        {
            bucket.slowest = Some(SlowQuery {
                ms: query.ms,
                source: format!("{}:{}", query.source.file(), query.source.line()),
                at: query.at,
                failed: query.failed,
            });
        }
        if query.write {
            bucket.writes += 1;
            if self.writes.len() == WRITE_SAMPLES {
                self.writes.pop_front();
            }
            self.writes.push_back((query.at, query.ms));
        }
    }

    /// What the window ending at `now` holds.
    pub fn report(&self, now: f64) -> (QueryReport, WriteReport) {
        let mut queries = QueryReport::default();
        let mut writes = WriteReport::default();
        for bucket in self.minutes.iter().filter(|b| in_window(b.minute, now)) {
            queries.count += bucket.count;
            queries.slow += bucket.slow;
            queries.failed += bucket.failed;
            writes.count += bucket.writes;
            if let Some(slowest) = &bucket.slowest
                && queries
                    .slowest
                    .as_ref()
                    .is_none_or(|before| slowest.ms > before.ms)
            {
                queries.slowest = Some(slowest.clone());
            }
        }
        let mut latencies: Vec<f64> = self
            .writes
            .iter()
            .filter(|(at, _)| in_window(minute(*at), now))
            .map(|(_, ms)| *ms)
            .collect();
        latencies.sort_by(f64::total_cmp);
        writes.p50_ms = percentile(&latencies, 50);
        writes.p90_ms = percentile(&latencies, 90);
        (queries, writes)
    }
}

/// The nearest-rank `p`th percentile of `sorted`, ascending; none of nothing.
fn percentile(sorted: &[f64], p: usize) -> Option<f64> {
    let rank = (sorted.len() * p).div_ceil(100);
    sorted.get(rank.max(1) - 1).copied()
}

/// What every client of one board in this process sent it.
#[derive(Debug, Default)]
pub struct QueryLog(Mutex<Queries>);

impl QueryLog {
    pub fn record(&self, query: Query) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .record(query);
    }

    pub fn report(&self, now: f64) -> (QueryReport, WriteReport) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .report(now)
    }
}

/// The boards this process has sent a request, by server, namespace and database.
static BOARDS: LazyLock<Mutex<HashMap<String, Arc<QueryLog>>>> = LazyLock::new(Mutex::default);

/// The log of the board at `addr`, in namespace `ns` and database `db`, which every client of it
/// in this process counts its requests in.
pub fn queries_of(addr: &str, ns: &str, db: &str) -> Arc<QueryLog> {
    let mut boards = BOARDS.lock().unwrap_or_else(PoisonError::into_inner);
    Arc::clone(boards.entry(format!("{addr}/{ns}/{db}")).or_default())
}

/// The board's health over the window ending at `at`: what `/api/health` answers, the record
/// holds and `tasks status` prints.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// When it was made, in seconds since the epoch.
    pub at: f64,
    /// The process that made it.
    pub pid: u32,
    /// How far back its counts reach, in seconds.
    pub window_seconds: f64,
    /// The callback dispatch passes of the server's clock.
    pub dispatch: PassReport,
    /// The idle sweeps: the machine's one server makes them, for its starter's project.
    pub sweep: PassReport,
    /// The requests every client of the board in the process sent.
    pub queries: QueryReport,
    /// Those of them that may write.
    pub writes: WriteReport,
    /// What is wrong, at `at`: a dispatch or a sweep failing ([`Report::warnings`]).
    pub warnings: Vec<String>,
}

impl Report {
    /// What is wrong at `now`: the dispatch or the sweep failing ([`PassReport::failing`]).
    pub fn warnings(&self, now: f64) -> Vec<String> {
        [
            self.dispatch.warning("callback dispatch", now),
            self.sweep.warning("idle sweep", now),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// What is wrong at `now` with the board whose record this is, as its reader sees it: the
    /// record's own warnings, and that its server stopped recording, when it did ([`STALE`]).
    /// `alive` says whether a process is running.
    pub fn read_warnings(&self, now: f64, alive: impl Fn(i64) -> bool) -> Vec<String> {
        let mut warnings = self.warnings(now);
        if now - self.at > STALE {
            let gone = if alive(i64::from(self.pid)) {
                ""
            } else {
                ", and is not running"
            };
            warnings.push(format!(
                "the Symphony server (pid {}) last recorded the board's health {} ago{gone}: no callback dispatch pass is recorded since",
                self.pid,
                ago(now - self.at)
            ));
        }
        warnings
    }

    /// The lines `tasks status` prints of it at `now`, its warnings `warnings`.
    pub fn lines(&self, now: f64, warnings: &[String]) -> Vec<String> {
        let mut lines = vec![format!(
            "board health, as the Symphony server (pid {}) recorded it {} ago, over the last hour:",
            self.pid,
            ago(now - self.at)
        )];
        lines.push(format!(
            "  {}",
            self.dispatch.line("callback dispatch", now)
        ));
        lines.push(format!("  {}", self.sweep.line("idle sweep", now)));
        let q = &self.queries;
        let mut queries = format!(
            "  board requests: {}, {} slow (1 s or more), {} failed",
            q.count, q.slow, q.failed
        );
        if let Some(slowest) = &q.slowest {
            queries.push_str(&format!(
                "; the slowest {:.0} ms at {}, {} ago{}",
                slowest.ms,
                slowest.source,
                ago(now - slowest.at),
                if slowest.failed { ", unanswered" } else { "" }
            ));
        }
        lines.push(queries);
        let w = &self.writes;
        lines.push(match (w.p50_ms, w.p90_ms) {
            (Some(p50), Some(p90)) => {
                format!("  writes: {}, p50 {p50:.1} ms, p90 {p90:.1} ms", w.count)
            }
            _ => format!("  writes: {}", w.count),
        });
        lines.extend(warnings.iter().map(|warning| format!("⚠ {warning}")));
        lines
    }

    /// The record at `path`; none when there is none.
    pub fn read(path: &Path) -> Result<Option<Report>, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Write it to `path` whole: to a file of this write's own beside it, then renamed over it, so
    /// a reader never meets half of it, and no other write, of this process or another, writes or
    /// renames that file meanwhile. Which of two writes at once lands last is the callers' to
    /// order ([`Recorder`] writes one at a time).
    pub fn write(&self, path: &Path) -> Result<(), String> {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let folder = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let name = path
            .file_name()
            .map_or_else(|| FILE.into(), |name| name.to_string_lossy().into_owned());
        let partial = folder.join(format!(
            ".{name}.{}.{}",
            std::process::id(),
            WRITES.fetch_add(1, Ordering::Relaxed)
        ));
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&partial, text + "\n")
            .and_then(|()| std::fs::rename(&partial, path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&partial);
                format!("{}: {e}", path.display())
            })
    }
}

/// Where the board `env` reaches keeps its record: the file `TASKS_HEALTH` names, else `FILE` in
/// `sf-tasks/` in the git common dir of its working directory, `health.<database>.json` for a
/// database other than the live one. A board `TASKS_SERVER` names keeps none unless
/// `TASKS_HEALTH` names it.
pub fn file(env: &Env) -> Result<PathBuf, String> {
    if let Some(path) = env.var(FILE_VAR) {
        return Ok(PathBuf::from(path));
    }
    if env.var("TASKS_SERVER").is_some() {
        return Err(format!(
            "a board TASKS_SERVER names keeps no health record unless {FILE_VAR} names one"
        ));
    }
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let name = match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("health.{database}.json"),
        _ => FILE.to_string(),
    };
    Ok(crate::sweep::trigger::folder(&common).join(name))
}

/// A server's record of one board: its dispatch passes and sweeps, the log of the board it reads
/// now, and the file it writes all of it to, if it writes one.
///
/// The server records from two threads, its clock's and its sweep's. Each holds the write lock
/// from its record to the file's rename, so the file is written one report at a time and each
/// report holds every pass recorded before it: the last to land is never older than another.
/// Reading the record takes only the record's own lock, held for no file.
#[derive(Debug, Default)]
pub struct Recorder {
    recording: Mutex<Recording>,
    /// Taken before `recording`, never while it is held.
    persisting: Mutex<Persisting>,
}

#[derive(Debug, Default)]
struct Recording {
    dispatch: Passes,
    sweep: Passes,
    queries: Option<Arc<QueryLog>>,
}

#[derive(Debug, Default)]
struct Persisting {
    file: Option<PathBuf>,
    /// When the file was last written, and whether the dispatch and the sweep were failing then.
    written: Option<(f64, [bool; 2])>,
    /// Why the last write failed, while it fails.
    unwritten: Option<String>,
}

impl Recorder {
    /// A recorder that writes its report to `file` too ([`Recorder::persist`]).
    pub fn writing(file: PathBuf) -> Recorder {
        Recorder {
            persisting: Mutex::new(Persisting {
                file: Some(file),
                ..Persisting::default()
            }),
            ..Recorder::default()
        }
    }

    fn held(&self) -> MutexGuard<'_, Recording> {
        self.recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn writer(&self) -> MutexGuard<'_, Persisting> {
        // A panic while writing leaves at worst a partial file of its own beside the record.
        self.persisting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The file it writes, if it writes one.
    pub fn file(&self) -> Option<PathBuf> {
        self.writer().file.clone()
    }

    /// The board it reads now: its requests are the report's.
    pub fn reading(&self, queries: Arc<QueryLog>) {
        self.held().queries = Some(queries);
    }

    /// A dispatch pass that ended at `at`, then the file written if it is due.
    pub fn dispatched(&self, at: f64, outcome: Result<(), &str>) {
        let mut writer = self.writer();
        self.held().dispatch.record(at, outcome);
        self.persist_held(&mut writer, at, false);
    }

    /// An idle sweep that ended at `at`, then the file written.
    pub fn swept(&self, at: f64, outcome: Result<(), &str>) {
        let mut writer = self.writer();
        self.held().sweep.record(at, outcome);
        self.persist_held(&mut writer, at, true);
    }

    /// Whether the dispatch, then the sweep, is failing at `now` ([`PassReport::failing`]).
    pub fn failing(&self, now: f64) -> [bool; 2] {
        let held = self.held();
        [
            held.dispatch.report(now).failing(now),
            held.sweep.report(now).failing(now),
        ]
    }

    /// The report at `now`.
    pub fn report(&self, now: f64) -> Report {
        let held = self.held();
        let (queries, writes) = held
            .queries
            .as_ref()
            .map(|log| log.report(now))
            .unwrap_or_default();
        let mut report = Report {
            at: now,
            pid: std::process::id(),
            window_seconds: WINDOW,
            dispatch: held.dispatch.report(now),
            sweep: held.sweep.report(now),
            queries,
            writes,
            warnings: Vec::new(),
        };
        report.warnings = report.warnings(now);
        report
    }

    /// Write the report at `now` to the file, if there is one, when the caller asks (`always`), when
    /// [`PERSIST_EVERY`] has passed since the last write, or when the dispatch or the sweep started
    /// or stopped failing since. What goes wrong is said on stderr, once for each change of what it
    /// is.
    pub fn persist(&self, now: f64, always: bool) {
        let mut writer = self.writer();
        self.persist_held(&mut writer, now, always);
    }

    /// [`Recorder::persist`], its write lock held from the due check to the rename.
    fn persist_held(&self, writer: &mut Persisting, now: f64, always: bool) {
        let Some(file) = writer.file.clone() else {
            return;
        };
        let failing = self.failing(now);
        let due = always
            || writer
                .written
                .is_none_or(|(at, was)| now - at >= PERSIST_EVERY || now < at || was != failing);
        if !due {
            return;
        }
        let unwritten = self.report(now).write(&file).err();
        if let Some(why) = &unwritten
            && writer.unwritten.as_ref() != Some(why)
        {
            eprintln!("· board health record: {why}");
        }
        writer.unwritten = unwritten;
        writer.written = Some((now, failing));
    }
}

#[cfg(test)]
mod tests;
