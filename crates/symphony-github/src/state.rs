//! What the exchange keeps, in the folder the board names for it (`symphony/integrations/github`
//! in the git common dir): the repository it is bound to, a mapping for each task it exchanges,
//! the curated text of each task opted in, the preview waiting to be applied, and a journal of
//! every operation. One lock in that folder serializes whoever writes it, for this project
//! alone. The state file is replaced whole, and the journal is appended and synced before the
//! operation it names goes on.

use crate::plan::Preview;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

/// The repository the state is bound to, by its number, which a rename or a transfer keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub repository_id: i64,
    pub node_id: String,
    /// Its name when last read: a display name.
    pub full_name: String,
}

/// Which side a mapping began on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// An issue imported as a proposal: the local side is the task's title and brief.
    Import,
    /// A task published as an issue: the local side is its curated text, never its brief.
    Export,
}

/// Why a mapping is exchanged no further until its operator says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    /// Both sides changed since they were last the same.
    Conflict,
    /// GitHub answers 404 for the issue: missing or not accessible.
    Inaccessible,
    /// The task the mapping names is no longer the one it was kept for.
    Suspended(String),
}

impl Flag {
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Flag::Conflict => "conflict: both sides changed".into(),
            Flag::Inaccessible => "the issue is missing or not accessible".into(),
            Flag::Suspended(why) => format!("suspended: {why}"),
        }
    }
}

/// One task and the issue it is exchanged with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub origin: Origin,
    /// The task's number under `generation`, the board's recovery generation it was read at.
    pub task_id: i64,
    pub generation: String,
    pub issue_number: i64,
    pub issue_id: i64,
    pub issue_node: String,
    pub url: String,
    /// The digest of each side when they were last synchronized.
    pub local: String,
    pub remote: String,
    /// `open` or `closed`, as last seen.
    pub remote_state: String,
    /// The newest comment update read, and the comments read that carry it.
    pub comments_since: Option<String>,
    #[serde(default)]
    pub comments_seen: Vec<i64>,
    pub flag: Option<Flag>,
}

/// The curated public text of a task opted in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub title: String,
    pub body: String,
}

/// The pull request a task is delivered by ([`crate::deliver`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pull {
    pub number: i64,
    pub url: String,
    /// The task's number on the board when it was bound: a slug given to another task is
    /// another pull request's.
    pub task: i64,
    /// The commit it was squashed as, once the board closed the task.
    pub landed: Option<String>,
}

/// The GitHub Release a release's tag is published as ([`crate::release`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Released {
    /// The release's number at GitHub.
    pub id: i64,
    pub url: String,
    /// The commit the release was asked for: read as the tag's at GitHub before one is made,
    /// and the request's own word for one found there.
    pub commit: String,
}

/// Which side an operator kept of a conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Local,
    Remote,
}

/// Everything kept but the journal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub binding: Option<Binding>,
    /// By the task's slug.
    #[serde(default)]
    pub mappings: BTreeMap<String, Mapping>,
    #[serde(default)]
    pub drafts: BTreeMap<String, Draft>,
    #[serde(default)]
    pub resolutions: BTreeMap<String, Side>,
    /// The pull request each delivered task is bound to, by its slug.
    #[serde(default)]
    pub pulls: BTreeMap<String, Pull>,
    /// The GitHub Release each release is published as, by its tag.
    #[serde(default)]
    pub releases: BTreeMap<String, Released>,
    /// The issues an operator dropped from a task's mapping, by the task's slug and the issue's
    /// id: none is mapped to that task again, or taken for its publication.
    #[serde(default)]
    pub dropped: BTreeMap<String, BTreeSet<i64>>,
    /// The rulesets in effect on the default branch as someone who administers the repository
    /// verified them (`rulesets --verify`), by number: who bypasses each, which the App's token is
    /// not shown ([`crate::preflight`]).
    #[serde(default)]
    pub rulesets: BTreeMap<i64, Verified>,
    /// The `updated_at` the next listing starts from.
    pub cursor: Option<String>,
    /// The settings' revision the cursor was read under. Under another, what the listing
    /// passed by may be selected now, so the next listing starts from the beginning.
    #[serde(default)]
    pub cursor_revision: Option<String>,
    /// Why nothing is dispatched until a preview succeeds again.
    pub paused: Option<String>,
    /// The second before which nothing is sent.
    pub rate_limited_until: Option<i64>,
}

/// One ruleset as it was verified: who bypassed it when GitHub said it last changed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verified {
    /// The repository's number it was read of.
    pub repository: i64,
    /// When GitHub said it last changed, as it said it.
    pub updated_at: String,
    /// Who may bypass it: each actor's type, number and mode.
    pub bypass: Vec<(String, Option<i64>, String)>,
    /// The second it was verified at.
    pub at: i64,
}

impl State {
    /// Whether an operator dropped the issue `issue_id` from the task `slug`'s mapping.
    #[must_use]
    pub fn dropped_from(&self, slug: &str, issue_id: i64) -> bool {
        self.dropped
            .get(slug)
            .is_some_and(|issues| issues.contains(&issue_id))
    }
}

/// The state folder.
pub struct Store {
    dir: PathBuf,
}

/// The lock, held until dropped.
pub struct Locked(#[expect(dead_code, reason = "held for its lock, released when it drops")] File);

fn wrong(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

impl Store {
    /// The store in `dir`, which is made if it is not there.
    ///
    /// # Errors
    ///
    /// The folder cannot be made.
    pub fn open(dir: &Path) -> Result<Store, String> {
        fs::create_dir_all(dir).map_err(|error| wrong(dir, error))?;
        Ok(Store { dir: dir.into() })
    }

    /// The store in `dir` if the folder is there: a read that makes nothing.
    #[must_use]
    pub fn existing(dir: &Path) -> Option<Store> {
        dir.is_dir().then(|| Store { dir: dir.into() })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The lock every writer of this folder takes.
    ///
    /// # Errors
    ///
    /// Another process holds it: nothing waits.
    pub fn lock(&self) -> Result<Locked, String> {
        let path = self.path("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| wrong(&path, error))?;
        match file.try_lock() {
            Ok(()) => Ok(Locked(file)),
            Err(TryLockError::WouldBlock) => Err(format!(
                "another run holds {}: one writer at a time",
                path.display()
            )),
            Err(TryLockError::Error(error)) => Err(wrong(&path, error)),
        }
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Result<Option<T>, String> {
        let path = self.path(name);
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|error| wrong(&path, error)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(wrong(&path, error)),
        }
    }

    /// `value` as the whole of file `name`: written beside it, synced, then moved over it.
    fn replace<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let path = self.path(name);
        let beside = self.path(&format!("{name}.new"));
        let text = serde_json::to_string_pretty(value).map_err(|error| wrong(&path, error))?;
        let write = || -> std::io::Result<()> {
            let mut file = File::create(&beside)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&beside, &path)?;
            File::open(&self.dir)?.sync_all()
        };
        write().map_err(|error| wrong(&path, error))
    }

    /// The state kept, or none: never kept, or lost.
    ///
    /// # Errors
    ///
    /// The file is there and does not read.
    pub fn state(&self) -> Result<Option<State>, String> {
        self.read("state.json")
    }

    /// # Errors
    ///
    /// The file could not be written.
    pub fn save(&self, state: &State) -> Result<(), String> {
        self.replace("state.json", state)
    }

    /// The preview waiting to be applied.
    ///
    /// # Errors
    ///
    /// The file is there and does not read.
    pub fn preview(&self) -> Result<Option<Preview>, String> {
        self.read("preview.json")
    }

    /// # Errors
    ///
    /// The file could not be written.
    pub fn keep_preview(&self, preview: &Preview) -> Result<(), String> {
        self.replace("preview.json", preview)
    }

    /// No preview waits any more.
    ///
    /// # Errors
    ///
    /// The file is there and could not be removed.
    pub fn drop_preview(&self) -> Result<(), String> {
        let path = self.path("preview.json");
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(wrong(&path, error)),
        }
    }

    /// One more line of the journal, on disk before this returns. A last line cut short by a
    /// crash, which no line end closes, is cut away first: appended to, it would hide this one.
    ///
    /// # Errors
    ///
    /// The line could not be written or synced: the operation it names must not go on.
    pub fn journal(&self, entry: &Entry) -> Result<(), String> {
        let path = self.path("journal.jsonl");
        let mut line = serde_json::to_string(entry).map_err(|error| wrong(&path, error))?;
        line.push('\n');
        let write = || -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&path)?;
            let length = file.metadata()?.len();
            if length > 0 {
                let mut last = [0];
                file.seek(SeekFrom::End(-1))?;
                file.read_exact(&mut last)?;
                if last != *b"\n" {
                    let text = fs::read(&path)?;
                    let kept = text
                        .iter()
                        .rposition(|byte| *byte == b'\n')
                        .map_or(0, |at| at + 1);
                    file.set_len(u64::try_from(kept).map_err(std::io::Error::other)?)?;
                }
            }
            file.write_all(line.as_bytes())?;
            file.sync_all()
        };
        write().map_err(|error| wrong(&path, error))
    }

    /// The journal, oldest first. What follows its last line end is a line cut short by a crash
    /// before its operation went on, and is left out, whatever bytes the cut left: one may end
    /// inside a character. Any other line that does not read is a refusal naming it: a line
    /// dropped may be an intent, and an intent forgotten is an operation made twice.
    ///
    /// # Errors
    ///
    /// The file is there and could not be read, or a line of it does not read.
    pub fn entries(&self) -> Result<Vec<Entry>, String> {
        let path = self.path("journal.jsonl");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(wrong(&path, error)),
        };
        let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(Vec::new());
        };
        bytes[..end]
            .split(|byte| *byte == b'\n')
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_slice(line).map_err(|error| {
                    wrong(
                        &path,
                        format!(
                            "line {} does not read ({error}): an operation it names may be \
                             forgotten, so nothing goes on until the line is put right",
                            index + 1
                        ),
                    )
                })
            })
            .collect()
    }
}

/// What a journal line says of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    /// Written before the operation is dispatched.
    Intent,
    /// Its result, written before anything kept moves on by it.
    Ack,
    /// It was not made and is not to be: what it found (a conflict, an issue not accessible)
    /// is kept instead.
    Skipped,
    /// It was not made this run: nothing was written by it, and a later run makes it.
    Failed,
    /// Its result is not known: the next run finds out before trying again.
    Unknown,
    /// Every operation of a preview is acknowledged.
    Applied,
}

/// One line of the journal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since 1970.
    pub at: i64,
    pub event: Event,
    /// The preview it belongs to, by digest.
    pub preview: String,
    /// The operation's id, its place in that preview for all but a publication.
    pub op: String,
    /// What kind of operation, and the task or issue it is for.
    pub kind: String,
    pub target: String,
    /// The digest of the content it writes.
    pub digest: String,
    /// What it expects to find: the digest of the remote text it replaces, or the board's token
    /// with its generation and the title and brief it replaces, which the board does not keep.
    pub base: Value,
    /// The settings' revision it was dispatched under.
    pub config: String,
    /// Who ran it.
    pub by: String,
    /// An acknowledgment's result, or why it failed.
    pub result: Value,
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
