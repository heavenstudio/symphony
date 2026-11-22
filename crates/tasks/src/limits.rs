//! The project's work-in-progress limits, `[limits]` in the main checkout's `symphony.toml`, and
//! beside them the pipeline alarm's thresholds (`[board]`, [`crate::alarm`]): what a store holds
//! its starts and its PULL digests to, and what `just tasks limits` prints, each with where it came
//! from. Every work-in-progress limit is a setting, as is the model each role is delegated to, so a
//! change of conditions or of decision is one simple edit to the file. (The models are
//! `[agents.models]`, [`crate::launch::Defaults`].)
//!
//! - `orchestrator_slots`: the tasks in progress an orchestrator holds (work in review and red work
//!   not counted) before the board refuses it new work ([`crate::slots`]); none caps nothing.
//!   `[board] orchestrator_slots` is read where `[limits]` names none, so a file an older build
//!   reads keeps its cap; written in both with two values, `[limits]`'s holds and the other is said.
//! - `orchestrator_tasks`: the open tasks an orchestrator holds the orchestrator role on itself
//!   ([`crate::orchestration::Load`]) past which the board gives it no more and its take of the role
//!   and a hand-over to it are refused; [`ORCHESTRATOR_TASKS`] unwritten.
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
//!
//! The board's store ([`crate::settings`]) is read after the file: a `[limits]` entry it holds
//! stands over the committed one, named as the store's with what it stands over ([`Read::under`]).
//! The local file beside the project file (`symphony.local.toml`, [`crate::local`]) is read last:
//! a `[limits]` key it writes holds in the place of both, named with that file and its line.

use crate::alarm::{self, Thresholds};
use crate::cli::Env;
use crate::on_review::MANIFEST;
use crate::settings::Stored;
use crate::sweep::trigger::main_checkout;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;
use toml::Value;

/// The table of the project file the limits are written in.
pub const TABLE: &str = "limits";
/// Its keys, in the order `tasks limits` prints them.
pub const KEYS: [&str; 4] = [SLOTS, TASKS, LOW, MACHINE_AGENTS];
/// The cap on an orchestrator's open tasks, in `[limits]` or `[board]` ([`crate::slots::KEY`]).
pub const SLOTS: &str = crate::slots::KEY;
/// The most open tasks an orchestrator holds the role on, in `[limits]` alone: no build that reads
/// `[board]`'s cap and not `[limits]` knows it.
pub const TASKS: &str = "orchestrator_tasks";
/// The open tasks an orchestrator holds the role on unless the project names another number: 16, as
/// many again as 8 slots, filed ahead of the work it builds.
pub const ORCHESTRATOR_TASKS: usize = 16;
/// The open slots below which an orchestrator hears PULL's `low_volume` line.
pub const LOW: &str = "low_volume";
/// The agents running on this machine at which a launch is refused.
pub const MACHINE_AGENTS: &str = "machine_agents";
/// The low-volume threshold unless the project names another: 4 open slots.
pub const LOW_VOLUME: usize = 4;
/// Each key of `[limits]`, the least it may be, and what one below it is told.
pub(crate) const LEAST: [(&str, i64, &str); 4] = [
    (SLOTS, 1, "an orchestrator needs at least one slot"),
    (TASKS, 1, "an orchestrator holds at least one task"),
    (LOW, 0, "it is at least 0"),
    (MACHINE_AGENTS, 1, "a machine runs at least one agent"),
];

/// The least `key` of [`LEAST`] may be, and what one below it is told.
fn least(key: &str) -> (i64, &'static str) {
    let (_, least, below) = LEAST
        .iter()
        .find(|(known, _, _)| *known == key)
        .expect("a key of [limits]");
    (*least, below)
}

/// What a store holds its starts and its PULL digests to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The tasks in progress an orchestrator may hold ([`crate::slots`]); none caps nothing.
    pub orchestrator_slots: Option<usize>,
    /// The open tasks an orchestrator holds the orchestrator role on itself at which the board gives
    /// it no more ([`crate::orchestration::Load`]).
    pub orchestrator_tasks: usize,
    /// The open slots below which an orchestrator hears PULL's `low_volume` line.
    pub low_volume: usize,
    /// The agents running on this machine at which a launch is refused; none limits nothing.
    pub machine_agents: Option<usize>,
    /// When the pipeline alarm raises and clears.
    pub alarm: Thresholds,
}

impl Limits {
    /// The limit `key` names set to `most`: one of the four whole numbers, any other key the
    /// machine's agents, as the settings that hold only these four name them.
    fn set(&mut self, key: &str, most: usize) {
        match key {
            SLOTS => self.orchestrator_slots = Some(most),
            TASKS => self.orchestrator_tasks = most,
            LOW => self.low_volume = most,
            _ => self.machine_agents = Some(most),
        }
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            orchestrator_slots: None,
            orchestrator_tasks: ORCHESTRATOR_TASKS,
            low_volume: LOW_VOLUME,
            machine_agents: None,
            alarm: Thresholds::default(),
        }
    }
}

/// Where a setting in force was written: its table and key as the file spells them
/// (`[limits] low_volume`), the local file's name before them when that file wrote it
/// (`symphony.local.toml [limits] low_volume`, [`crate::local`]), and its line there when that can
/// be found.
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
    pub(crate) fn named(&self) -> String {
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
    /// By key, for each limit the board's store holds in force: what it stands over, the
    /// committed value and where it is written, or the default ([`over_stored`]).
    pub under: BTreeMap<&'static str, String>,
}

/// The file at `path`, as [`parse`] reads it, with the local file beside it over it
/// ([`over`]): no file is every default, and one that cannot be read every default too, said.
pub fn read_at(path: &Path) -> Read {
    read_with(path, &Stored::default())
}

/// The file at `path` as [`parse`] reads it, then what the board's store holds over it
/// ([`over_stored`]), then the local file beside it over both ([`over`]): the order every limit
/// is read in.
pub fn read_with(path: &Path, stored: &Stored) -> Read {
    let read = match std::fs::read_to_string(path) {
        Ok(text) => parse(&path.display().to_string(), &text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Read::default(),
        Err(error) => Read {
            problems: vec![format!(
                "cannot read {}: {error}; every limit takes its default",
                path.display()
            )],
            ..Read::default()
        },
    };
    over(over_stored(read, stored), crate::local::beside(path))
}

/// `value`, a limit in force, as it is said: its number, or `none`.
fn said(value: Option<usize>) -> String {
    value.map_or_else(|| "none".to_string(), |n| n.to_string())
}

/// `read`, the committed limits, with those the board's store holds in their place, each named as
/// the store's, and what each stands over kept ([`Read::under`]): the committed value with where
/// it is written, or the default. What the store could not read of itself is said.
pub fn over_stored(mut read: Read, stored: &Stored) -> Read {
    read.problems.extend(stored.problems.iter().cloned());
    for (key, most) in &stored.held.limits {
        let was = match *key {
            SLOTS => said(read.limits.orchestrator_slots.replace(*most)),
            TASKS => said(Some(std::mem::replace(
                &mut read.limits.orchestrator_tasks,
                *most,
            ))),
            LOW => said(Some(std::mem::replace(&mut read.limits.low_volume, *most))),
            _ => said(read.limits.machine_agents.replace(*most)),
        };
        let under = match read.written.get(key) {
            Some(written) => format!("{was} ({})", written.said()),
            None => format!("its default, {was}"),
        };
        read.under.insert(key, under);
        let at = Stored::written(&format!("{TABLE}.{key}"));
        read.written.insert(key, at);
    }
    read
}

/// `read`, the committed limits, with those `local` writes in their place, each named by the
/// local file and its line ([`crate::local`]); a local file that is refused is said and none of it
/// read.
pub fn over(mut read: Read, local: Result<Option<crate::local::Local>, Vec<String>>) -> Read {
    let local = match local {
        Ok(Some(local)) => local,
        Ok(None) => return read,
        Err(problems) => {
            read.problems
                .extend(problems.iter().map(|p| crate::local::left_out(p)));
            return read;
        }
    };
    for (key, most) in &local.limits {
        read.limits.set(key, *most);
        let at = local.written(&format!("[{TABLE}] {key}"), &[TABLE], key);
        read.written.insert(key, at);
        read.under.remove(key);
    }
    read
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
    // The cap: `[limits]`'s, else `[board]`'s, where an older build reads it. A value the rule
    // refuses is said in its table's words, with what holds instead.
    let no_cap = "no cap on an orchestrator's open tasks applies";
    let slots = |table: &str, value: &Value| {
        let at = setting(table, SLOTS);
        judged(SLOTS, value)
            .map(|cap| (cap, at.clone()))
            .map_err(|why| format!("{path}: {} is {}; {why}", at.named(), shown(value)))
    };
    // What is said of `[board]`'s cap written beside `[limits]`', after what is said of that one.
    let mut beside = None;
    let cap = match (limits.get(SLOTS), board.get(SLOTS)) {
        (Some(ours), Some(older)) => {
            let held = slots(TABLE, ours);
            beside = match (&held, slots("board", older)) {
                (Ok((cap, at)), Ok((other, _))) if cap != &other => Some(format!(
                    "{path}: {} is {other}, left out: {} is {cap}, which holds",
                    setting("board", SLOTS).named(),
                    at.named()
                )),
                (Ok((cap, at)), Err(refused)) => Some(format!(
                    "{refused}; left out: {} is {cap}, which holds",
                    at.named()
                )),
                (Err(_), Err(refused)) => Some(format!("{refused}; {no_cap}")),
                _ => None,
            };
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
        Some(Err(problem)) => read.problems.push(format!("{problem}; {no_cap}")),
        None => {}
    }
    read.problems.extend(beside);
    // The three limits that are one whole number each, and what a value that is none leaves.
    let whole_limits = [
        (TASKS, format!("it takes its default, {ORCHESTRATOR_TASKS}")),
        (LOW, format!("it takes its default, {LOW_VOLUME}")),
        (
            MACHINE_AGENTS,
            "no limit on the agents this machine runs applies".to_string(),
        ),
    ];
    for (key, left) in whole_limits {
        let Some(value) = limits.get(key) else {
            continue;
        };
        let at = setting(TABLE, key);
        match judged(key, value) {
            Ok(most) => {
                read.limits.set(key, most);
                read.written.insert(key, at);
            }
            Err(why) => read.problems.push(format!(
                "{path}: {} is {}; {why}; {left}",
                at.named(),
                shown(value)
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
        match whole(value, 1).and_then(|n| u32::try_from(n).map_err(|_| Some(PAST))) {
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

/// `value`, written for the limit `key` of [`LEAST`], as every reader of a limit takes it, wherever
/// it is written: the project file's `[limits]`, or `[board]` for the cap, as the board reads it
/// ([`parse`]) and the Symphony's load refuses what it cannot read; the local file and the board's
/// store ([`crate::local`]). A whole number from its least, and the cap ([`SLOTS`]) at most
/// [`u32::MAX`], the most the Symphony's project holds it in (`project::Board`); else why not, a
/// value below its least told what its least says.
///
/// # Errors
///
/// Why it is not one: no whole number, below its least, or past what it can be.
pub fn judged(key: &str, value: &Value) -> Result<usize, &'static str> {
    let (least, below) = least(key);
    let most = whole(value, least).map_err(|why| why.unwrap_or(below))?;
    if key == SLOTS && u32::try_from(most).is_err() {
        return Err(PAST);
    }
    Ok(most)
}

/// Why a whole number is refused that is greater than its setting can hold.
const PAST: &str = "past what it can be";

/// `value` as a whole number of at least `least`; else why not: `None` for one below it (its
/// setting says what it is at least), else what it is.
pub(crate) fn whole(value: &Value, least: i64) -> Result<usize, Option<&'static str>> {
    match value {
        Value::Integer(n) if *n >= least => usize::try_from(*n).map_err(|_| Some(PAST)),
        Value::Integer(_) => Err(None),
        _ => Err(Some("not a whole number")),
    }
}

/// `value` as the file writes it: a number or a boolean as it is, text in quotes, else its kind.
pub(crate) fn shown(value: &Value) -> String {
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

/// The limits of a project file and the local file beside it ([`crate::local`]), read again
/// whenever the length or time of either changes: a store that
/// holds them ([`crate::Store::watching`]) is held to the file as it is now, so a change reaches a
/// store kept open (the Symphony server's) at its next read, with no restart. What is wrong with a
/// version of the file read again is said on stderr, once.
#[derive(Debug)]
pub struct Watched {
    path: PathBuf,
    last: Mutex<([Stamp; 2], Stored, Read)>,
}

impl Watched {
    /// The file at `path`, read now: the watch and what it read, whose problems are the caller's to
    /// say.
    pub fn new(path: PathBuf) -> (Arc<Watched>, Read) {
        let stamp = Self::stamps(&path);
        let read = read_at(&path);
        let watched = Watched {
            path,
            last: Mutex::new((stamp, Stored::default(), read.clone())),
        };
        (Arc::new(watched), read)
    }

    /// The project file and the local file beside it, as they are now.
    fn stamps(path: &Path) -> [Stamp; 2] {
        [Stamp::of(path), Stamp::of(&crate::local::path_beside(path))]
    }

    /// The project file it watches, with the local file beside it.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The files as they are now, read again when either changed since the last look.
    pub fn read(&self) -> Read {
        self.read_with(&Stored::default())
    }

    /// The files as they are now with `stored`, what the board's store holds, between them
    /// ([`read_with`]): read again when either file or the store's entries changed since the last
    /// look, and what is newly wrong said on stderr, once.
    pub fn read_with(&self, stored: &Stored) -> Read {
        let stamp = Self::stamps(&self.path);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last.0 != stamp || &last.1 != stored {
            let read = read_with(&self.path, stored);
            // A change of the store alone says only what is new: the files' problems were said.
            let files = last.0 != stamp;
            for problem in &read.problems {
                if files || !last.2.problems.contains(problem) {
                    eprintln!("tasks: {problem}");
                }
            }
            *last = (stamp, stored.clone(), read);
        }
        last.2.clone()
    }

    /// The limits in force now.
    pub fn now(&self) -> Limits {
        self.read().limits
    }

    /// The limits in force now, over `stored` ([`Watched::read_with`]).
    pub fn now_with(&self, stored: &Stored) -> Limits {
        self.read_with(stored).limits
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
    /// The limits in force now; `stored` reads what the board's store holds, asked only where a
    /// project file is watched: limits set here hold whatever a board keeps.
    pub(crate) fn now(&self, stored: impl FnOnce() -> Stored) -> Limits {
        match self {
            Held::Set(limits) => *limits,
            Held::Watched(watched) => watched.now_with(&stored()),
        }
    }
}

#[cfg(test)]
mod tests;
