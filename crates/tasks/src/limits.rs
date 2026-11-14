//! The project's work-in-progress limits, `[limits]` in the main checkout's `symphony.toml`, and
//! beside them the pipeline alarm's thresholds (`[board]`, [`crate::alarm`]): what a store holds
//! its starts and its PULL digests to, and what `just tasks limits` prints, each with where it came
//! from. The owner, 2026-11-12: "all WIP limits should be settings anyway, as well as preferred
//! model to delegate to. Should be very easy and simple to change the file whenever conditions or
//! deicions happen." (The models are `[agents.models]`, [`crate::launch::Defaults`].)
//!
//! - `orchestrator_slots`: the tasks in progress an orchestrator holds (work in review and red work
//!   not counted) before the board refuses it new work ([`crate::slots`]); none caps nothing.
//!   `[board] orchestrator_slots` is read where `[limits]` names none, so a file an older build
//!   reads keeps its cap; written in both with two values, `[limits]`'s holds and the other is said.
//! - `low_volume`: an orchestrator holding fewer open slots hears PULL's `low_volume` line
//!   ([`crate::cli::pull`]); [`LOW_VOLUME`] unwritten, and 0 says it to none.
//! - `machine_agents`: the agents running on this machine ([`crate::launch::running`]) at which
//!   `tasks launch` refuses to start another and past which `tasks next` warns, and the staffing
//!   pass's session cap when it is lower than its own; none limits nothing.
//!
//! The board reads the file itself, as it reads `[board] on_review` (it cannot link the Symphony's
//! reading of it): every command anew, and a store kept open, as the Symphony's are, again whenever
//! the file's length or time changes ([`Watched`]), so a change takes effect without a restart. A
//! value that is no whole number, or out of its range, is said with its line and takes its
//! default (the alarm's thresholds all take theirs, as one): a broken project file must not stop
//! the board, nor the landing that mends it. A key the table does not know is left out here and
//! named by `tasks limits`; the Symphony's reading of the file refuses it by name and line.

use crate::alarm::{self, Thresholds};
use crate::cli::Env;
use crate::on_review::MANIFEST;
use crate::sweep::trigger::main_checkout;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;
use toml::Value;

/// The table of the project file the limits are written in.
pub const TABLE: &str = "limits";
/// Its keys, in the order `tasks limits` prints them.
pub const KEYS: [&str; 3] = [SLOTS, LOW, MACHINE_AGENTS];
/// The cap on an orchestrator's open tasks, in `[limits]` or `[board]` ([`crate::slots::KEY`]).
pub const SLOTS: &str = crate::slots::KEY;
/// The open slots below which an orchestrator hears PULL's `low_volume` line.
pub const LOW: &str = "low_volume";
/// The agents running on this machine at which a launch is refused.
pub const MACHINE_AGENTS: &str = "machine_agents";
/// The low-volume threshold unless the project names another: 4 open slots.
pub const LOW_VOLUME: usize = 4;

/// What a store holds its starts and its PULL digests to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The tasks in progress an orchestrator may hold ([`crate::slots`]); none caps nothing.
    pub orchestrator_slots: Option<usize>,
    /// The open slots below which an orchestrator hears PULL's `low_volume` line.
    pub low_volume: usize,
    /// The agents running on this machine at which a launch is refused; none limits nothing.
    pub machine_agents: Option<usize>,
    /// When the pipeline alarm raises and clears.
    pub alarm: Thresholds,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            orchestrator_slots: None,
            low_volume: LOW_VOLUME,
            machine_agents: None,
            alarm: Thresholds::default(),
        }
    }
}

/// Where a setting in force was written: its table and key as the file spells them
/// (`[limits] low_volume`), and its line when that can be found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub at: String,
    pub line: Option<usize>,
}

impl Written {
    /// `[limits] low_volume, line 3`.
    pub fn said(&self) -> String {
        match self.line {
            Some(line) => format!("{}, line {line}", self.at),
            None => self.at.clone(),
        }
    }

    /// `[limits] low_volume (line 3)`: the setting as a problem with it names it.
    fn named(&self) -> String {
        match self.line {
            Some(line) => format!("{} (line {line})", self.at),
            None => self.at.clone(),
        }
    }
}

/// The project file read: the limits in force, where each was written (a key not there is its
/// default's), what was wrong with it, and the keys of `[limits]` this build does not know.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub limits: Limits,
    /// By key: [`KEYS`] and the alarm's [`alarm::KEYS`].
    pub written: BTreeMap<&'static str, Written>,
    /// Each said whole, with what applies instead.
    pub problems: Vec<String>,
    /// `[limits] KEY (line N)` for each key of the table this build does not know, left out.
    pub unknown: Vec<String>,
}

/// The file at `path`, as [`parse`] reads it: no file is every default, and one that cannot be read
/// every default too, said.
pub fn read_at(path: &Path) -> Read {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&path.display().to_string(), &text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Read::default(),
        Err(error) => Read {
            problems: vec![format!(
                "cannot read {}: {error}; every limit takes its default",
                path.display()
            )],
            ..Read::default()
        },
    }
}

/// `text`, the project file at `path`: the limits it writes over their defaults. Only `[limits]`
/// and the `[board]` keys the board reads are looked at; the rest of the file is the Symphony's.
pub fn parse(path: &str, text: &str) -> Read {
    let mut read = Read::default();
    let file: toml::Table = match toml::from_str(text) {
        Ok(file) => file,
        Err(error) => {
            read.problems.push(format!(
                "{path}: {}; every limit takes its default",
                error.to_string().trim_end()
            ));
            return read;
        }
    };
    let limits = match file.get(TABLE) {
        None => toml::Table::new(),
        Some(Value::Table(limits)) => limits.clone(),
        Some(other) => {
            read.problems.push(format!(
                "{path}: {TABLE} (line {}) is {}, not a table; every limit takes its default",
                line(text, &[], TABLE).unwrap_or(0),
                other.type_str()
            ));
            toml::Table::new()
        }
    };
    let board = match file.get("board") {
        Some(Value::Table(board)) => board.clone(),
        _ => toml::Table::new(),
    };
    let setting = |table: &str, key: &str| Written {
        at: format!("[{table}] {key}"),
        line: line(text, &[table], key),
    };
    read.unknown = limits
        .keys()
        .filter(|key| !KEYS.contains(&key.as_str()))
        .map(|key| setting(TABLE, key).named())
        .collect();
    // The cap: `[limits]`'s, else `[board]`'s, where an older build reads it.
    let slots = |table: &str, value: &Value| {
        let at = setting(table, SLOTS);
        whole(value, 1).map(|cap| (cap, at.clone())).map_err(|why| {
            let why = why.unwrap_or("an orchestrator needs at least one slot");
            format!(
                "{path}: {} is {}; {why}; no cap on an orchestrator's open tasks applies",
                at.named(),
                shown(value)
            )
        })
    };
    let cap = match (limits.get(SLOTS), board.get(SLOTS)) {
        (Some(ours), Some(older)) => {
            let held = slots(TABLE, ours);
            if let (Ok((cap, at)), Ok((other, _))) = (&held, slots("board", older))
                && cap != &other
            {
                read.problems.push(format!(
                    "{path}: {} is {other}, left out: {} is {cap}, which holds",
                    setting("board", SLOTS).named(),
                    at.named()
                ));
            }
            Some(held)
        }
        (Some(ours), None) => Some(slots(TABLE, ours)),
        (None, Some(older)) => Some(slots("board", older)),
        (None, None) => None,
    };
    match cap {
        Some(Ok((cap, at))) => {
            read.limits.orchestrator_slots = Some(cap);
            read.written.insert(SLOTS, at);
        }
        Some(Err(problem)) => read.problems.push(problem),
        None => {}
    }
    if let Some(value) = limits.get(LOW) {
        let at = setting(TABLE, LOW);
        match whole(value, 0) {
            Ok(low) => {
                read.limits.low_volume = low;
                read.written.insert(LOW, at);
            }
            Err(why) => read.problems.push(format!(
                "{path}: {} is {}; {}; it takes its default, {LOW_VOLUME}",
                at.named(),
                shown(value),
                why.unwrap_or("it is at least 0")
            )),
        }
    }
    if let Some(value) = limits.get(MACHINE_AGENTS) {
        let at = setting(TABLE, MACHINE_AGENTS);
        match whole(value, 1) {
            Ok(most) => {
                read.limits.machine_agents = Some(most);
                read.written.insert(MACHINE_AGENTS, at);
            }
            Err(why) => read.problems.push(format!(
                "{path}: {} is {}; {}; no limit on the agents this machine runs applies",
                at.named(),
                shown(value),
                why.unwrap_or("a machine runs at least one agent")
            )),
        }
    }
    // The alarm's thresholds, all three or none: one that cannot be read leaves the defaults.
    let mut thresholds = Thresholds::default();
    let mut alarm_written = Vec::new();
    let mut alarm_problem = None;
    for (key, into) in [
        (alarm::KEYS[0], &mut thresholds.ratio),
        (alarm::KEYS[1], &mut thresholds.raise_hours),
        (alarm::KEYS[2], &mut thresholds.clear_hours),
    ] {
        let Some(value) = board.get(key) else {
            continue;
        };
        let at = setting("board", key);
        match whole(value, 1)
            .and_then(|n| u32::try_from(n).map_err(|_| Some("past what it can be")))
        {
            Ok(n) => {
                *into = n;
                alarm_written.push((key, at));
            }
            Err(why) => {
                alarm_problem.get_or_insert_with(|| {
                    format!(
                        "{path}: {} is {}; {}; the pipeline alarm takes its default thresholds",
                        at.named(),
                        shown(value),
                        why.unwrap_or("it is at least 1")
                    )
                });
            }
        }
    }
    match alarm_problem {
        Some(problem) => read.problems.push(problem),
        None => {
            read.limits.alarm = thresholds;
            read.written.extend(alarm_written);
        }
    }
    read
}

/// `value` as a whole number of at least `least`; else why not: `None` for one below it (its
/// setting says what it is at least), else what it is.
fn whole(value: &Value, least: i64) -> Result<usize, Option<&'static str>> {
    match value {
        Value::Integer(n) if *n >= least => {
            usize::try_from(*n).map_err(|_| Some("past what it can be"))
        }
        Value::Integer(_) => Err(None),
        _ => Err(Some("not a whole number")),
    }
}

/// `value` as the file writes it: a number or a boolean as it is, text in quotes, else its kind.
fn shown(value: &Value) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::String(text) => format!("{text:?}"),
        other => other.type_str().to_string(),
    }
}

/// The line `key` is written on in the table `path` leads to in `text`, when every table on the
/// path is there: how a problem with a setting, or `tasks limits`, names where it stands.
pub fn line(text: &str, path: &[&str], key: &str) -> Option<usize> {
    let parsed = toml::de::DeTable::parse(text).ok()?;
    let mut table = parsed.get_ref();
    for name in path {
        let (_, value) = table.iter().find(|(k, _)| **k.get_ref() == **name)?;
        table = value.get_ref().as_table()?;
    }
    let start = table
        .iter()
        .find(|(k, _)| **k.get_ref() == *key)?
        .0
        .span()
        .start;
    Some(text[..start].matches('\n').count() + 1)
}

/// The project file the board `env` runs against reads its limits from: the `symphony.toml` of the
/// main checkout of the repository holding its working directory, found from its `.git` files
/// alone, with no `git` process (every command reads it); none outside a repository, or in one with
/// no main checkout, where every limit is its default.
pub fn file_of(env: &Env) -> Option<PathBuf> {
    let common = crate::git::read(&env.cwd)?;
    main_checkout(&common).map(|root| root.join(MANIFEST))
}

/// A path's length and modification time, what it leads to; nothing when it is absent: what tells
/// a project file from the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp(Option<(u64, SystemTime)>);

impl Stamp {
    fn of(path: &Path) -> Stamp {
        Stamp(
            std::fs::metadata(path)
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH))),
        )
    }
}

/// The limits of a project file, read again whenever its length or time changes: a store that
/// holds them ([`crate::Store::watching`]) is held to the file as it is now, so a change reaches a
/// store kept open (the Symphony server's) at its next read, with no restart. What is wrong with a
/// version of the file read again is said on stderr, once.
#[derive(Debug)]
pub struct Watched {
    path: PathBuf,
    last: Mutex<(Stamp, Read)>,
}

impl Watched {
    /// The file at `path`, read now: the watch and what it read, whose problems are the caller's to
    /// say.
    pub fn new(path: PathBuf) -> (Arc<Watched>, Read) {
        let stamp = Stamp::of(&path);
        let read = read_at(&path);
        let watched = Watched {
            path,
            last: Mutex::new((stamp, read.clone())),
        };
        (Arc::new(watched), read)
    }

    /// The file it watches.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file as it is now, read again when it changed since the last look.
    pub fn read(&self) -> Read {
        let stamp = Stamp::of(&self.path);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last.0 != stamp {
            let read = read_at(&self.path);
            for problem in &read.problems {
                eprintln!("tasks: {problem}");
            }
            *last = (stamp, read);
        }
        last.1.clone()
    }

    /// The limits in force now.
    pub fn now(&self) -> Limits {
        self.read().limits
    }
}

/// What a store holds its limits from: set once, or a project file it watches.
#[derive(Clone, Debug)]
pub(crate) enum Held {
    Set(Limits),
    Watched(Arc<Watched>),
}

impl Default for Held {
    fn default() -> Held {
        Held::Set(Limits::default())
    }
}

impl Held {
    pub(crate) fn now(&self) -> Limits {
        match self {
            Held::Set(limits) => *limits,
            Held::Watched(watched) => watched.now(),
        }
    }
}

#[cfg(test)]
mod tests;
