//! The board's health an hour at a time ([`symphony_api::HealthHour`]): as each hour ends, the
//! machine's one server appends what its [`Recorder`](super::Recorder) held over it, one line to
//! the day file `symphony/health/YYYY-MM-DD.jsonl` in the git common dir ([`folder`]). A line is
//! read out of the minutes the recorder keeps ([`super::KEPT_MINUTES`]), so the append asks the
//! board nothing. Only the shared server keeps them (`apps/symphony-server`, `live/startup.rs`); a
//! private server writes none.
//!
//! A writer's lines cover whole minutes, from the first whole minute after it began recording to
//! the end of each hour, and then to where it stopped: the moment it stopped, the start of the
//! minute its [`Gate`] closed in, or the end of the minute it was last called in before a [`GAP`]
//! in which it was not. An hour it did not record has no line, never a line of zeros, and no two lines of one
//! project cover the same minute.

use super::{KEPT_MINUTES, Recording, minute};
use crate::cli::Env;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use symphony_api::{HEALTH_SCHEMA, HealthHour, HourPasses, HourRequests, HourSlowest, HourWrites};

/// The day files' folder in the git common dir, in the one folder of Symphony's there, beside the
/// runs' ([`crate::runs::FOLDER`]).
pub const FOLDER: &str = "symphony/health";
/// Names the day files' folder in place of the common dir's: a test's, or a scratch board's.
pub const FOLDER_VAR: &str = "SYMPHONY_HEALTH";
/// How many days a day file is kept after its day, as the runs' are.
pub const KEPT_DAYS: u32 = 90;
/// How long a writer may go uncalled, in seconds, before it counts as having watched nothing
/// since its last call. The server's clock calls it every second, through a board that does not
/// answer too, so a gap this long is a process that was suspended (a machine asleep) or a clock
/// that stood still.
pub const GAP: f64 = 900.0;
const HOUR: f64 = 3_600.0;
const MINUTE: f64 = 60.0;
const DAY: f64 = 86_400.0;

/// The first second of the hour `at` falls in.
fn hour_of(at: f64) -> f64 {
    (at / HOUR).floor() * HOUR
}

/// The first second of the minute `at` falls in.
fn floor_minute(at: f64) -> f64 {
    (at / MINUTE).floor() * MINUTE
}

/// The first whole minute at or after `at`.
fn ceil_minute(at: f64) -> f64 {
    (at / MINUTE).ceil() * MINUTE
}

/// The minute of the last moment before `to`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a time in seconds is far inside i64"
)]
fn last_minute(to: f64) -> i64 {
    (to / MINUTE).ceil() as i64 - 1
}

/// Whether the recorder still holds every minute from `from` at `now`: it keeps a minute
/// [`KEPT_MINUTES`] after it, and a request answered meanwhile on another thread may drop it a
/// little sooner.
fn held(from: f64, now: f64) -> bool {
    minute(now + MINUTE) < minute(from) + KEPT_MINUTES
}

/// Where the board `env` reaches keeps its hours: the folder [`FOLDER_VAR`] names, else [`FOLDER`]
/// in the git common dir of its working directory, `symphony/health.<database>/` for a database
/// other than the live one. A board `TASKS_SERVER` names keeps none unless [`FOLDER_VAR`] names it.
pub fn folder(env: &Env) -> Result<PathBuf, String> {
    if let Some(path) = env.var(FOLDER_VAR) {
        return Ok(PathBuf::from(path));
    }
    if env.var("TASKS_SERVER").is_some() {
        return Err(format!(
            "a board TASKS_SERVER names keeps no hours unless {FOLDER_VAR} names their folder"
        ));
    }
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    Ok(common.join(match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("{FOLDER}.{database}"),
        _ => FOLDER.to_string(),
    }))
}

/// When a server generation's recorders keep their hours: one gate for every project it serves,
/// the process's own ([`Gate::process`]). A reload's hand-off has two generations alive at once,
/// both counting their own passes and requests, so the replacement's gate is held until the
/// hand-off commits and the generation it replaces closes its own just before it commits: no two
/// lines cover the same minute. A hand-off that fails after that close opens the gate again
/// ([`Gate::reopen`]).
///
/// A recorder reads its gate at each call, so a close ends its hours where the minute the gate
/// closed in begins, however late the next call comes, and an open begins them at the first whole
/// minute after.
#[derive(Debug)]
pub struct Gate(Mutex<Opening>);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Opening {
    /// Recording; since this time when it was opened after a hold or a close.
    Open { since: Option<f64> },
    /// A replacement's, until its hand-off commits: nothing is recorded.
    Held,
    /// Closed at this time, by the server's clock, as the hand-off commits: nothing is recorded from
    /// the minute it fell in.
    Closed(f64),
}

impl Default for Gate {
    fn default() -> Gate {
        Gate(Mutex::new(Opening::Open { since: None }))
    }
}

impl Gate {
    /// This process's: every project the server serves records by it.
    pub fn process() -> Arc<Gate> {
        static PROCESS: LazyLock<Arc<Gate>> = LazyLock::new(Arc::default);
        Arc::clone(&PROCESS)
    }

    fn state(&self) -> MutexGuard<'_, Opening> {
        // Each holder only reads or replaces a whole value.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A replacement's, before its hubs start: nothing is recorded until [`Gate::open`].
    pub fn hold(&self) {
        *self.state() = Opening::Held;
    }

    /// A held gate's hand-off committed at `at`: recording from the first whole minute after. Any
    /// other gate stays as it is, so a generation's own reload closing it is never undone here.
    pub fn open(&self, at: f64) {
        self.opened(|state| state == Opening::Held, at);
    }

    /// Nothing recorded from the minute `at` falls in, by the server's clock, until it opens again:
    /// its generation's hand-off commits.
    pub fn close(&self, at: f64) {
        *self.state() = Opening::Closed(at);
    }

    /// A closed gate's hand-off failed at `at`: its generation goes on, recording again from the
    /// first whole minute after. Any other gate stays as it is.
    pub fn reopen(&self, at: f64) {
        self.opened(|state| matches!(state, Opening::Closed(_)), at);
    }

    /// Open since `at`, when it stands as `from` says.
    fn opened(&self, from: impl Fn(Opening) -> bool, at: f64) {
        let mut state = self.state();
        if from(*state) {
            *state = Opening::Open { since: Some(at) };
        }
    }

    fn opening(&self) -> Opening {
        *self.state()
    }
}

/// What a recorder keeps of its hours: where the next line begins, and its writer's names.
pub(super) struct Hours {
    folder: PathBuf,
    gate: Arc<Gate>,
    /// Where the next line begins: none until the writer begins recording, and again from where
    /// its gate closed, it stopped or it went a [`GAP`] uncalled.
    from: Option<f64>,
    /// Where the last line it ended runs to: no line begins before it.
    ended: Option<f64>,
    /// When it was first called, and last.
    first: Option<f64>,
    last: Option<f64>,
    /// When it began recording: the generation its lines name.
    generation: Option<f64>,
    /// Whether its server stopped: nothing begins after.
    stopped: bool,
    /// The day (since the epoch) whose old files were last pruned.
    pruned: Option<i64>,
    /// Why the last append failed, while it fails, so it is said once.
    unwritten: Option<String>,
    /// Where what goes wrong is said: the server's log.
    tell: Box<dyn Fn(&str) + Send + Sync>,
}

impl fmt::Debug for Hours {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hours")
            .field("folder", &self.folder)
            .field("from", &self.from)
            .field("ended", &self.ended)
            .field("last", &self.last)
            .field("generation", &self.generation)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}

impl Hours {
    pub(super) fn new(
        folder: PathBuf,
        gate: Arc<Gate>,
        tell: Box<dyn Fn(&str) + Send + Sync>,
    ) -> Hours {
        Hours {
            folder,
            gate,
            from: None,
            ended: None,
            first: None,
            last: None,
            generation: None,
            stopped: false,
            pruned: None,
            unwritten: None,
            tell,
        }
    }

    pub(super) fn folder(&self) -> &Path {
        &self.folder
    }

    /// The spans `(from, to)` whose lines are due at `now`, oldest first, as the gate stands: each
    /// hour that ended, and the span under way once the gate closed, the writer went a [`GAP`]
    /// uncalled or, `stopping`, its server stops. A span whose first minutes the recorder may no
    /// longer hold is left out ([`held`]).
    pub(super) fn due(&mut self, now: f64, stopping: bool) -> Vec<(f64, f64)> {
        let mut spans = Vec::new();
        if self.stopped {
            return spans;
        }
        let first = *self.first.get_or_insert(now);
        // Uncalled for longer than a gap: it watched nothing after the minute of its last call.
        let gap = self.last.filter(|last| now - last > GAP);
        if let Some(last) = gap {
            self.end(floor_minute(last) + MINUTE, now, &mut spans);
        }
        match self.gate.opening() {
            Opening::Held => {}
            Opening::Closed(at) => self.end(floor_minute(at), now, &mut spans),
            Opening::Open { since } => {
                if self.from.is_none() {
                    let begin = match gap {
                        // The same process watching again: no other writer counted this minute.
                        Some(_) => floor_minute(now),
                        // Another generation may have counted up to then: the next whole minute.
                        None => ceil_minute(since.unwrap_or(now).max(first)),
                    };
                    self.from = Some(self.ended.map_or(begin, |ended| begin.max(ended)));
                    self.generation
                        .get_or_insert(since.unwrap_or(now).max(first));
                }
                self.hours_ended(now, now, &mut spans);
                if stopping {
                    self.end(now, now, &mut spans);
                }
            }
        }
        self.last = Some(now);
        self.stopped = stopping;
        spans
    }

    /// The span under way ended at `to`: each hour of it that ended by then, then the rest. Nothing
    /// is recorded until it begins again.
    fn end(&mut self, to: f64, now: f64, spans: &mut Vec<(f64, f64)>) {
        if self.from.is_none() {
            return;
        }
        self.hours_ended(to, now, spans);
        if let Some(from) = self.from.take()
            && to > from
            && held(from, now)
        {
            spans.push((from, to));
        }
        self.ended = Some(self.ended.map_or(to, |ended| ended.max(to)));
    }

    /// Each hour of the span under way that ended by `until`, the span moving on to the next.
    fn hours_ended(&mut self, until: f64, now: f64, spans: &mut Vec<(f64, f64)>) {
        while let Some(from) = self.from {
            let end = hour_of(from) + HOUR;
            if end > until {
                break;
            }
            if held(from, now) {
                spans.push((from, end));
            }
            self.from = Some(end);
        }
    }

    /// The line of the span `(from, to)`, written at `now`, from what `held` keeps.
    pub(super) fn line(&self, held: &Recording, (from, to): (f64, f64), now: f64) -> HealthHour {
        let minutes = minute(from)..=last_minute(to);
        let passes = |passes: &super::Passes| {
            let (ok, failed) = passes.within(&minutes);
            HourPasses { ok, failed }
        };
        let (requests, writes) = held
            .queries
            .as_ref()
            .map(|log| log.within(&minutes))
            .unwrap_or_default();
        let hour = hour_of(from);
        HealthHour {
            schema: HEALTH_SCHEMA,
            at: now,
            pid: std::process::id(),
            generation: self.generation.unwrap_or(from),
            hour,
            from,
            to,
            partial: from > hour || to < hour + HOUR,
            dispatch: passes(&held.dispatch),
            sweep: passes(&held.sweep),
            requests: HourRequests {
                count: requests.count,
                slow: requests.slow,
                unanswered: requests.failed,
                slowest: requests.slowest.map(|slowest| HourSlowest {
                    ms: slowest.ms,
                    source: slowest.source,
                    at: slowest.at,
                    unanswered: slowest.failed,
                }),
            },
            writes: HourWrites {
                count: writes.count,
                p50_ms: writes.p50_ms,
                p90_ms: writes.p90_ms,
            },
        }
    }

    /// `lines` appended at `now`, and the old day files pruned at its first call of each UTC day. A
    /// write that fails is said once for each change of what it is, and its lines are not kept.
    pub(super) fn write(&mut self, lines: &[HealthHour], now: f64) {
        if !lines.is_empty() {
            match append(&self.folder, lines) {
                Ok(()) => self.unwritten = None,
                Err(e) => {
                    let why = format!("{}: {e}", self.folder.display());
                    if self.unwritten.as_ref() != Some(&why) {
                        (self.tell)(&format!("· board health hours: {why}"));
                    }
                    self.unwritten = Some(why);
                }
            }
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a day count since 1970 of any time a clock says fits an i64"
        )]
        let today = (now / DAY).floor() as i64;
        if self.pruned != Some(today) {
            self.pruned = Some(today);
            crate::runs::prune(&self.folder, now, KEPT_DAYS);
        }
    }
}

/// `lines` appended to the files of their days in `folder`, made as needed: each day's in one
/// write, after a newline when a writer before was cut short in its line, so each line stays its
/// own; never through a link or into a FIFO. No lock is taken: one server generation records at
/// a time ([`Gate`]).
pub fn append(folder: &Path, lines: &[HealthHour]) -> io::Result<()> {
    let mut by_day: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for line in lines {
        let text = by_day.entry(line.day()).or_default();
        serde_json::to_writer(&mut *text, line).map_err(io::Error::other)?;
        text.push(b'\n');
    }
    if by_day.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(folder)?;
    for (day, mut text) in by_day {
        let mut file = crate::runs::regular(
            &folder.join(format!("{day}.jsonl")),
            OpenOptions::new().read(true).append(true).create(true),
        )?;
        if !crate::runs::ends_a_line(&file)? {
            text.insert(0, b'\n');
        }
        file.write_all(&text)?;
    }
    Ok(())
}

/// The lines of `day`'s file (`YYYY-MM-DD`) in `folder`, in order; none when there is none. A line
/// that does not read (cut short, empty, another shape) is passed over, and nothing is read through
/// a link or from a FIFO.
pub fn read(folder: &Path, day: &str) -> Vec<HealthHour> {
    let Ok(mut file) = crate::runs::regular(
        &folder.join(format!("{day}.jsonl")),
        OpenOptions::new().read(true),
    ) else {
        return Vec::new();
    };
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
#[path = "hours_tests.rs"]
mod tests;
