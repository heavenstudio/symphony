//! The board's health an hour at a time ([`symphony_api::HealthHour`]): as each hour ends, the
//! machine's one server appends what its [`Recorder`](super::Recorder) held over it, one line to
//! the day file `symphony/health/YYYY-MM-DD.jsonl` in the git common dir ([`folder`]). A line is
//! read out of the minutes the recorder keeps ([`super::KEPT_MINUTES`]), so the append asks the board
//! nothing. Only the shared server keeps them (`apps/symphony-server`, `live/startup.rs`), as its
//! generation's gate lets it ([`crate::recording`]); a private server writes none.
//!
//! A writer's lines cover whole minutes, from the first whole minute after it began recording to
//! the end of each hour, and then to where it stopped: the moment it stopped, the start of the
//! minute its gate closed in, or, when the machine slept, the end of the last minute it watched
//! before. An hour it did not record has no line, never a line of zeros, and no two lines of one
//! project cover the same minute. A span whose minutes the recorder let go before it was written
//! (it went uncalled for two hours of requests) has no line either: its counts would fall short.

use super::{Recording, minute};
use crate::cli::Env;
use crate::recording::{Gate, State};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Instant;
use symphony_api::{HEALTH_SCHEMA, HealthHour, HourPasses, HourRequests, HourSlowest, HourWrites};

/// The day files' folder in the git common dir, in the one folder of Symphony's there, beside the
/// runs' ([`crate::runs::FOLDER`]).
pub const FOLDER: &str = "symphony/health";
/// Names the day files' folder in place of the common dir's: a test's, or a scratch board's.
pub const FOLDER_VAR: &str = "SYMPHONY_HEALTH";
/// How many days a day file is kept after its day, as the runs' are.
pub const KEPT_DAYS: u32 = 90;
/// How far, in seconds, the server's clock may run ahead of [`awake`]'s between two calls before
/// the machine counts as having slept between them. However far apart the calls are (a board slow
/// to answer holds a tick for minutes), both clocks run while the process does; only a sleep, or a
/// clock set forward, puts the server's ahead.
pub const SLEPT: f64 = 60.0;
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

/// The seconds this process has been awake: the monotonic clock std's `Instant` reads, which does
/// not run while the machine sleeps (`CLOCK_UPTIME_RAW` on macOS, `CLOCK_MONOTONIC` on Linux). It
/// is read at the call, whatever the server's clock says (`_now`, which a test's clock follows).
pub fn awake(_now: f64) -> f64 {
    static START: LazyLock<Instant> = LazyLock::new(Instant::now);
    START.elapsed().as_secs_f64()
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

/// What a recorder keeps of its hours: where the next line begins, and its writer's names.
pub(super) struct Hours {
    folder: PathBuf,
    /// The seconds on a clock that stops while the machine sleeps, at a time on the server's
    /// ([`awake`]).
    awake: Box<dyn Fn(f64) -> f64 + Send + Sync>,
    /// Where the next line begins: none until the writer begins recording, and again from where
    /// its gate closed, it stopped or the machine slept.
    from: Option<f64>,
    /// Where the last line it ended runs to: no line begins before it.
    ended: Option<f64>,
    /// When it was first called.
    first: Option<f64>,
    /// When it was last called, on the server's clock and on [`Hours::awake`]'s.
    last: Option<(f64, f64)>,
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
        awake: Box<dyn Fn(f64) -> f64 + Send + Sync>,
        tell: Box<dyn Fn(&str) + Send + Sync>,
    ) -> Hours {
        Hours {
            folder,
            awake,
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

    /// The spans `(from, to)` whose lines are due at `now`, oldest first, as `gate` stands (none
    /// given: held): each hour that ended, and the span under way once the machine slept, the gate
    /// closed or, `stopping`, its server stops.
    pub(super) fn due(&mut self, now: f64, stopping: bool, gate: Option<&Gate>) -> Vec<(f64, f64)> {
        let mut spans = Vec::new();
        if self.stopped {
            return spans;
        }
        let awake = (self.awake)(now);
        let first = *self.first.get_or_insert(now);
        // Asleep since its last call: the server's clock ran ahead of the one that stops meanwhile.
        // It watched for at most `up` after that call, and again for at most `up` before this one.
        let woke = self.last.and_then(|(last, then)| {
            let up = (awake - then).max(0.0);
            (now - last - up > SLEPT).then(|| {
                let fell = (floor_minute(last + up) + MINUTE).min(floor_minute(now));
                (fell, fell.max(floor_minute(now - up)))
            })
        });
        if let Some((fell, _)) = woke {
            self.end(fell, &mut spans);
        }
        match gate.map_or(State::Held, Gate::stands) {
            State::Held => {}
            State::Closed(at) => self.end(floor_minute(at), &mut spans),
            State::Open { since } => {
                if self.from.is_none() {
                    let begin = match woke {
                        // The same process watching again: no other writer counted these minutes.
                        Some((_, rose)) => rose,
                        // Another generation may have counted up to then: the next whole minute.
                        None => ceil_minute(since.unwrap_or(now).max(first)),
                    };
                    // Never before its gate opened, nor where its last line ended.
                    let begin = since.map_or(begin, |since| begin.max(ceil_minute(since)));
                    self.from = Some(self.ended.map_or(begin, |ended| begin.max(ended)));
                    self.generation
                        .get_or_insert(since.unwrap_or(now).max(first));
                }
                self.hours_ended(now, &mut spans);
                if stopping {
                    self.end(now, &mut spans);
                }
            }
        }
        self.last = Some((now, awake));
        self.stopped = stopping;
        spans
    }

    /// The span under way ended at `to`: each hour of it that ended by then, then the rest. Nothing
    /// is recorded until it begins again.
    fn end(&mut self, to: f64, spans: &mut Vec<(f64, f64)>) {
        if self.from.is_none() {
            return;
        }
        self.hours_ended(to, spans);
        if let Some(from) = self.from.take()
            && to > from
        {
            spans.push((from, to));
        }
        self.ended = Some(self.ended.map_or(to, |ended| ended.max(to)));
    }

    /// Each hour of the span under way that ended by `until`, the span moving on to the next.
    fn hours_ended(&mut self, until: f64, spans: &mut Vec<(f64, f64)>) {
        while let Some(from) = self.from {
            let end = hour_of(from) + HOUR;
            if end > until {
                break;
            }
            spans.push((from, end));
            self.from = Some(end);
        }
    }

    /// The line of the span `(from, to)`, written at `now`, from what `held` keeps; none when a
    /// minute of it was let go, so its counts would fall short, or when it holds nothing: a project
    /// with no board counts no pass, and an hour of it is no hour of a board.
    pub(super) fn line(
        &self,
        held: &Recording,
        (from, to): (f64, f64),
        now: f64,
    ) -> Option<HealthHour> {
        let minutes = minute(from)..=last_minute(to);
        if !held.dispatch.holds(*minutes.start()) || !held.sweep.holds(*minutes.start()) {
            return None;
        }
        let (requests, writes) = match &held.queries {
            Some(log) => log.whole_within(&minutes)?,
            None => Default::default(),
        };
        let passes = |passes: &super::Passes| {
            let (ok, failed) = passes.within(&minutes);
            HourPasses { ok, failed }
        };
        let (dispatch, sweep) = (passes(&held.dispatch), passes(&held.sweep));
        if dispatch == HourPasses::default()
            && sweep == HourPasses::default()
            && requests.count == 0
        {
            return None;
        }
        let start = hour_of(from);
        Some(HealthHour {
            schema: HEALTH_SCHEMA,
            at: now,
            pid: std::process::id(),
            generation: self.generation.unwrap_or(from),
            start,
            from,
            to,
            partial: from > start || to < start + HOUR,
            dispatch,
            sweep,
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
        })
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
/// a time ([`crate::recording`]).
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
    text.lines().filter_map(crate::objects::parse).collect()
}

#[cfg(test)]
#[path = "hours_tests.rs"]
mod tests;
