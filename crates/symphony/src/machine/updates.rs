//! The machine's schema updates (docs/workflow/RELEASES.md, "An update"): one entry for each time a
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
//!
//! **Its lock.** The machine's server tries a held update again on its own ([`super::held`]), so
//! two processes write the ledger and move boards: a release run (every kind, a refresh's
//! included) and that server. A release run takes [`LOCK`] in the machine folder right after its
//! own run lock and holds it until it lets go of that one, waiting while a retry holds it
//! ([`lock`]); the retry only tries it, and does nothing while anything holds it ([`try_lock`]).
//! The retry never takes the run's own lock, so no order of the two waits on the other.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::taskcli::CliBoard;

/// The ledger, in the machine folder.
pub const FILE: &str = "updates.json";
/// The lock a release run and the machine server's retry of a held update take, in the machine
/// folder.
pub const LOCK: &str = "updates.lock";
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
        let ledger: Ledger = serde_json::from_slice(text.as_bytes())
            .map_err(|error| format!("{}: {error}", path.display()))?;
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

/// The machine's updates lock, held: let go as it drops, as its file closes. A child forked while
/// it is held keeps no copy past its exec, as the file is opened close-on-exec.
#[derive(Debug)]
pub struct Lock {
    file: File,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// The lock's file in the machine folder `machine`, open: the folder is never made here.
fn lock_file(machine: &Path) -> Result<File, String> {
    let path = machine.join(LOCK);
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| crate::text::os_error(&e, &path))
}

/// The machine's updates lock in `machine`, taken: waited for while another process holds it, as
/// a release run waits for a retry under way, whose every command has its bound.
///
/// # Errors
///
/// When its file cannot be opened or locked.
pub fn lock(machine: &Path) -> Result<Lock, String> {
    let file = lock_file(machine)?;
    file.lock()
        .map_err(|e| crate::text::os_error(&e, &machine.join(LOCK)))?;
    Ok(Lock { file })
}

/// The machine's updates lock in `machine`, taken without waiting: none while another holds it.
///
/// # Errors
///
/// When its file cannot be opened, or the attempt fails otherwise.
pub fn try_lock(machine: &Path) -> Result<Option<Lock>, String> {
    let file = lock_file(machine)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lock { file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(crate::text::os_error(&e, &machine.join(LOCK))),
    }
}

/// What an update's times say: how long it took, and how much of it after a release serving it,
/// as a sentence ending in a space; nothing until both ends are known.
pub fn took(update: &Update) -> String {
    match (update.total(), update.after_available()) {
        (Some(total), Some(after)) => format!(
            "It took {} from the board reaching schema {} to the server writing it, {} of it after \
             a release serving it was installed. ",
            said(total),
            update.schema,
            said(after)
        ),
        (Some(total), None) => format!("It took {}. ", said(total)),
        _ => String::new(),
    }
}

/// When the board in the git common dir `common` reached `schema`, as its own record says: the
/// first store that saw it there wrote `schema-seen.log` then ([`symphony_board::live::seen`]);
/// none when the record says another schema, or there is none.
pub fn seen_at(common: &Path, schema: i64) -> Option<Stamp> {
    let record = symphony_board::live::seen::read(common)?;
    (record.version == schema).then(|| Stamp {
        at: record.at,
        source: "schema-seen.log: the first store that saw the board at this schema; the move was \
                 then or just before"
            .into(),
    })
}

/// When the release `name` was installed in the machine folder `machine`: its manifest's time,
/// which its install writes last.
pub fn installed_at(machine: &Path, name: &str) -> Option<f64> {
    let manifest = super::release::folder(machine, name).join(super::release::MANIFEST);
    let modified = std::fs::metadata(manifest)
        .and_then(|meta| meta.modified())
        .ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_secs_f64())
}

/// Every release installed whole in the machine folder `machine`.
pub fn every_installed(machine: &Path) -> Vec<super::release::Manifest> {
    let Ok(entries) = std::fs::read_dir(machine.join(super::release::RELEASES)) else {
        return Vec::new();
    };
    (entries.filter_map(Result::ok))
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            super::release::installed(machine, &name).ok().flatten()
        })
        .collect()
}

/// The board binary that writes a board at `schema`: the `tasks` of the release `current` names
/// when it serves that schema, else of the release installed last that does.
pub fn writer_of(machine: &Path, schema: i64) -> Option<PathBuf> {
    let current = super::release::current_installed(machine).filter(|m| m.serves.schema == schema);
    let name = current.map(|m| m.release).or_else(|| {
        (every_installed(machine).into_iter())
            .filter(|manifest| manifest.serves.schema == schema)
            .map(|manifest| {
                let at = installed_at(machine, &manifest.release).unwrap_or(0.0);
                (manifest.release, at)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(name, _)| name)
    })?;
    let tasks = super::release::binary(machine, &name, "tasks");
    tasks.is_file().then_some(tasks)
}

/// The board an update's task is on, as far as its ledger tells it: a session that posts and
/// closes, and the task's status.
pub trait TaskBoard {
    /// A session of kind filer: its handle.
    ///
    /// # Errors
    ///
    /// When the board refuses it or cannot be reached.
    fn register(&self) -> Result<String, String>;
    /// # Errors
    ///
    /// When the board refuses it or cannot be reached.
    fn leave(&self, handle: &str) -> Result<(), String>;
    /// The status of the task `slug`, none when the board has no such task.
    ///
    /// # Errors
    ///
    /// When the board cannot be read.
    fn status(&self, slug: &str) -> Result<Option<String>, String>;
    /// # Errors
    ///
    /// When the board refuses it or cannot be reached.
    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String>;
    /// Close `slug` with `why` on its thread.
    ///
    /// # Errors
    ///
    /// When the board refuses it or cannot be reached.
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String>;
}

impl TaskBoard for CliBoard {
    fn register(&self) -> Result<String, String> {
        let session = self
            .call(
                "register",
                &json!({"kind": symphony_board::red::FILER, "model": "release"}),
            )
            .map_err(|e| e.0)?;
        session["handle"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("the board registered no handle: {session}"))
    }

    fn leave(&self, handle: &str) -> Result<(), String> {
        self.call("leave", &json!({"handle": handle}))
            .map(|_| ())
            .map_err(|e| e.0)
    }

    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        match self.call("get", &json!({"slug": slug})) {
            Ok(task) => Ok(task["status"].as_str().map(str::to_string)),
            Err(refused) if refused.0.starts_with(symphony_board::Error::NO_TASK) => Ok(None),
            Err(error) => Err(error.0),
        }
    }

    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
        self.call("say", &json!({"slug": slug, "sender": by, "text": text}))
            .map(|_| ())
            .map_err(|e| e.0)
    }

    /// The filer takes the reviewer role first: work started, or held by an executor, is moved
    /// only by a session holding a role on it, and a task has one orchestrator.
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
        let _ = self.call(
            "take_role",
            &json!({"slug": slug, "handle": by, "role": "reviewer"}),
        );
        self.say(slug, by, why)?;
        self.call(
            "move",
            &json!({"slug": slug, "status": "cancelled", "handle": by}),
        )
        .map(|_| ())
        .map_err(|e| e.0)
    }
}

/// The task `slug` closed on `board` with `words`, as a session of its own, when it is still an
/// open red; one closed already, or gone, is left as it is.
///
/// # Errors
///
/// When the board refuses a step or cannot be reached: the task may be still open.
pub fn close_task(board: &dyn TaskBoard, slug: &str, words: &str) -> Result<(), String> {
    let filer = board.register()?;
    let closed = match board.status(slug) {
        Ok(Some(status)) if symphony_board::red::open(&status) => board.close(slug, &filer, words),
        Ok(_) => Ok(()),
        Err(why) => Err(why),
    };
    let _ = board.leave(&filer);
    closed
}

/// A post on the task `slug` on `board`, as a session of its own.
///
/// # Errors
///
/// When the board refuses it or cannot be reached.
pub fn post(board: &dyn TaskBoard, slug: &str, text: &str) -> Result<(), String> {
    let filer = board.register()?;
    let said = board.say(slug, &filer, text);
    let _ = board.leave(&filer);
    said
}

/// `seconds` as the owner reads a wait: `4 h 46 min`, `3 min 10 s`, `40 s`; to the second under an
/// hour, and to the nearest minute from an hour on.
pub fn said(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    if seconds < 3600 {
        let (minutes, rest) = (seconds / 60, seconds % 60);
        return match minutes {
            0 => format!("{rest} s"),
            _ => format!("{minutes} min {rest} s"),
        };
    }
    let minutes = (seconds + 30) / 60;
    format!("{} h {} min", minutes / 60, minutes % 60)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
