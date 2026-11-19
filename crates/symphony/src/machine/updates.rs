//! The machine's schema updates (docs/RELEASES.md, "An update"): one entry for each time a
//! registered project's board reaches a schema that the machine's server must come to write, kept
//! in the machine folder's `updates.json` ([`FILE`]). A release's refresh writes it
//! (`crates/symphony-build/src/release/updates.rs`, under the release run's lock). The machine's
//! server reads it, so a page can say how long each update took, and the read-only notice can link
//! the task of a failed one.
//!
//! An update is named `ID@N`: the board of the project `ID` at schema `N`. It is complete once the
//! machine's server writes that board at `N` (its project list says it writes it, or, from a server
//! that says nothing of writes, it is at `N` itself). Each entry keeps when the board reached `N`,
//! when the first green release serving `N` was installed and when the server first wrote it, each
//! with where that time comes from ([`Stamp`]), so the latency the owner reads is the record's,
//! never a guess. An entry with no served time is still open.
//!
//! **Generations.** The same board at the same schema is one update, however many refreshes find it
//! and whichever release serves it. A board seen below `N` after it reached it left that update
//! ([`Update::left`]): it went back. A later arrival at `N` is then a new generation, `ID@N.2`, its
//! own entry beside the old one, which keeps its history. So does an arrival the board's own
//! record dates after the entry's (`schema-seen.log`, which starts again from an accepted rollback).
//!
//! **Its task.** A refresh that had a green release serving `N` and ended with the update
//! incomplete is a failure of it ([`Update::failures`]). The first files one task, on the board
//! that can act, and the entry keeps which board and slug ([`Task`]). Each later failure posts on
//! that same task, and the refresh that completes the update closes it. A first-time success files
//! nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The ledger, in the machine folder.
pub const FILE: &str = "updates.json";
/// The ledger's format: a later build that writes one it cannot read refuses it, never loses it.
pub const VERSION: u32 = 1;
/// The series an update's task is filed in: the release run's reds
/// (`symphony_board::red::SERIES`), so it is red work, staffed before other work of its priority
/// and given to an orchestrator at its limit.
pub const SERIES: &str = "tests/release-red-update-";

/// The machine's updates: every entry a refresh wrote, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    #[serde(default)]
    pub updates: Vec<Update>,
}

/// A time a record gives, and which record: the provenance the latency is read with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    /// Seconds since the epoch.
    pub at: f64,
    /// Where it comes from, in words: `schema-seen.log` (the first store that saw the board at the
    /// schema; the move was then or just before), `the refresh's move`, `the server's hand-off`
    /// (its reload's line in `server.log`), `the release's manifest`, or a refresh's own look,
    /// which bounds it from above.
    pub source: String,
}

/// One failure of an update: when, by which run, and what the refresh found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub at: f64,
    pub run: String,
    pub why: String,
}

/// An update's task: the project whose board holds it, and its slug there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// The id of the project whose board holds it, as registered; for the Symphony's own board,
    /// its id when this repository is registered, else `this repository`.
    pub project: String,
    /// Whether it is on the Symphony's own board, the release run's repository's, whatever id that
    /// is registered under, or none.
    #[serde(default)]
    pub ours: bool,
    pub slug: String,
    /// Whether the refresh that completed the update closed it.
    #[serde(default)]
    pub closed: bool,
}

/// One board's update to one schema, in one generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Update {
    /// `ID@N`, `ID@N.G` from the second generation ([`name`]).
    pub id: String,
    pub project: String,
    pub schema: i64,
    pub generation: u32,
    /// When its board reached `N`, as far as a record says; none when none does.
    pub reached: Option<Stamp>,
    /// The first green release serving `N` installed here, and when it was.
    pub release: Option<String>,
    pub available: Option<Stamp>,
    /// When the machine's server first wrote its board at `N`: the update complete.
    pub served: Option<Stamp>,
    /// The release whose server wrote it then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_by: Option<String>,
    /// When a refresh found its board below `N` before it was served: it went back, and a later
    /// arrival is a new generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Stamp>,
    /// Each refresh that ended with it incomplete though a green release serving `N` was here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<Failure>,
    /// Its task, filed at its first failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
}

/// The name of the update of `project`'s board to `schema` in its `generation`: `ID@N`, then
/// `ID@N.2`, `ID@N.3`.
pub fn name(project: &str, schema: i64, generation: u32) -> String {
    if generation <= 1 {
        format!("{project}@{schema}")
    } else {
        format!("{project}@{schema}.{generation}")
    }
}

/// The slug of that update's task: [`SERIES`], then the project's id as a slug keeps it (each run
/// of anything but an ASCII letter or digit one `-`, in lower case), the schema and, from the second
/// generation, `-gG`: `tests/release-red-update-alpha-23`, `tests/release-red-update-alpha-23-g2`.
pub fn slug(project: &str, schema: i64, generation: u32) -> String {
    let mut key = String::new();
    for c in project.chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c.to_ascii_lowercase());
        } else if !key.is_empty() && !key.ends_with('-') {
            key.push('-');
        }
    }
    let key = key.trim_end_matches('-');
    let key = if key.is_empty() { "project" } else { key };
    let generation = if generation <= 1 {
        String::new()
    } else {
        format!("-g{generation}")
    };
    format!("{SERIES}{key}-{schema}{generation}")
}

impl Update {
    /// A new update of `project`'s board to `schema`, in `generation`, reached at `reached`.
    pub fn new(project: &str, schema: i64, generation: u32, reached: Option<Stamp>) -> Update {
        Update {
            id: name(project, schema, generation),
            project: project.to_string(),
            schema,
            generation,
            reached,
            release: None,
            available: None,
            served: None,
            served_by: None,
            left: None,
            failures: Vec::new(),
            task: None,
        }
    }

    /// Whether it is still to be served: neither served nor left.
    pub fn open(&self) -> bool {
        self.served.is_none() && self.left.is_none()
    }

    /// How long the owner waited: from its board reaching `N` to the server writing it, in seconds;
    /// none until both are known.
    pub fn total(&self) -> Option<f64> {
        Some(self.served.as_ref()?.at - self.reached.as_ref()?.at)
    }

    /// How much of that came after a green release serving `N` was installed: what the machine's
    /// own switch took, in seconds; none until both are known.
    pub fn after_available(&self) -> Option<f64> {
        let available = self.available.as_ref()?.at;
        let served = self.served.as_ref()?.at;
        Some(served - available.max(self.reached.as_ref().map_or(available, |r| r.at)))
    }
}

impl Ledger {
    /// The ledger in the machine folder `machine`: an empty one while there is none.
    ///
    /// # Errors
    ///
    /// When it is there and cannot be read, or a later build wrote a format this one cannot.
    pub fn read(machine: &Path) -> Result<Ledger, String> {
        let path = path(machine);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Ledger {
                    version: VERSION,
                    updates: Vec::new(),
                });
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let ledger: Ledger =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        if ledger.version > VERSION {
            return Err(format!(
                "{}: format {} is a later build's; this one reads {VERSION}",
                path.display(),
                ledger.version
            ));
        }
        Ok(ledger)
    }

    /// Write it whole into the machine folder `machine`, a reader finding the old or the new.
    ///
    /// # Errors
    ///
    /// When it cannot be written.
    pub fn write(&self, machine: &Path) -> Result<(), String> {
        let path = path(machine);
        let mut ledger = self.clone();
        ledger.version = VERSION;
        let text = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())? + "\n";
        std::fs::create_dir_all(machine).map_err(|e| crate::text::os_error(&e, machine))?;
        crate::text::write_whole(&path, &text).map_err(|e| crate::text::os_error(&e, &path))
    }

    /// The newest entry of `project`'s board at `schema`, whatever its generation.
    pub fn newest(&self, project: &str, schema: i64) -> Option<&Update> {
        (self.updates.iter())
            .filter(|update| update.project == project && update.schema == schema)
            .max_by_key(|update| update.generation)
    }

    /// The open entry of `project`'s board at `schema`, if one is open.
    pub fn open(&self, project: &str, schema: i64) -> Option<&Update> {
        self.newest(project, schema).filter(|update| update.open())
    }

    /// Every open entry of `project`'s board.
    pub fn open_of<'a>(&'a self, project: &'a str) -> impl Iterator<Item = &'a Update> + 'a {
        (self.updates.iter()).filter(move |update| update.project == project && update.open())
    }
}

/// The ledger's path in the machine folder `machine`.
pub fn path(machine: &Path) -> PathBuf {
    machine.join(FILE)
}

/// `seconds` as the owner reads a wait: `4 h 46 min`, `3 min 10 s`, `40 s`.
pub fn said(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    match (hours, minutes) {
        (0, 0) => format!("{rest} s"),
        (0, _) => format!("{minutes} min {rest} s"),
        _ => format!("{hours} h {minutes} min"),
    }
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
