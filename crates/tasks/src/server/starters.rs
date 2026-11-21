//! The starters' lock (`symphony/tasks/lock`), which every start, stop, hold, release and reclaim
//! of the board's server and every snapshot of its store takes, one at a time, and what its holder
//! does (`symphony/tasks/lock.json`, written whole by the holder once it has the lock, and removed
//! before it lets the lock go).
//!
//! A caller that finds the lock held waits, and once it has waited [`GRACE`] says through the
//! board's [`Home::telling`] what it waits on, as the holder's note says it (a start of the board's
//! server, a private replay of the store's log and its size, a stop, the hold's stop, a reclaim,
//! a snapshot), then again every [`AGAIN`] while it waits. A caller with a bound
//! ([`Home::waiting_at_most`]: the Symphony's reads, whose thread serves its pages too) says nothing
//! and is refused once the bound passes, the refusal naming the same. A holder whose note is
//! missing, or names a process that has ended, is a build that writes none, and is said as one.

use super::{Home, LOCK, LOG, ServerError, io};
use crate::sys::{FileLock, local_minute, pid_alive};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// In the board's folder: what the starters' lock's holder does, while it holds it.
pub(super) const NOTE: &str = "lock.json";
/// How long a caller waits on the lock before it says what it waits on: a start of the board's
/// server without a replay takes about a second.
const GRACE: Duration = Duration::from_secs(1);
/// How often a caller still waiting on the lock says so again.
const AGAIN: Duration = Duration::from_secs(30);
/// How often a waiting caller tries the lock.
const POLL: Duration = Duration::from_millis(50);
/// What a replay is said as, then its log's size, then [`REPLAYED_BEFORE`]: the words a page reads a
/// replay by ([`super::apart::replaying_in`]).
pub(super) const REPLAYING: &str = "a private replay of the store's log";
/// What follows a replay's size where it is said.
pub(super) const REPLAYED_BEFORE: &str = " before the board's server starts";

/// What the starters' lock's holder does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Doing {
    /// Finds the board's server, waiting until it answers, or starts it ([`Home::ensure`]).
    Starting,
    /// Replays the store's log in a private server before the board's starts ([`Home::replay`]).
    Replaying,
    /// Stops the board's server ([`Home::stop`]).
    Stopping,
    /// Holds the board's database stopped ([`Home::hold`]).
    Holding,
    /// Releases the hold ([`Home::release`]).
    Releasing,
    /// Rebuilds the board's store ([`Home::reclaim`]).
    Reclaiming,
    /// Takes a snapshot of the board's store ([`Home::snapshot`]).
    Snapshotting,
    /// Moves the board's store to another engine, or restores a snapshot over it
    /// ([`Home::move_to`], [`super::Postgres::restore_over`]).
    Moving,
}

/// What `symphony/tasks/lock.json` says: who holds the starters' lock, doing what, since when.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Note {
    /// The process that holds it.
    pub pid: i64,
    pub doing: Doing,
    /// Since when it does it, in seconds since the epoch.
    pub at: f64,
    /// For a replay, the bytes of the log it replays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logged: Option<u64>,
}

/// The starters' lock, held: its note goes before the lock does.
pub(super) struct Starters {
    note: PathBuf,
    lock: FileLock,
}

impl Starters {
    /// Say that its holder now does `doing`, for a replay of `logged` bytes. A note that cannot be
    /// written leaves waiters saying they wait on a holder that says nothing, and stops nothing.
    pub(super) fn doing(&self, doing: Doing, logged: Option<u64>) {
        let note = Note {
            pid: i64::from(std::process::id()),
            doing,
            at: crate::store::now(),
            logged,
        };
        let next = self
            .note
            .with_extension(format!("json.{}", std::process::id()));
        let text = serde_json::to_string(&note).expect("a note serialises");
        if fs::write(&next, text).is_err() || fs::rename(&next, &self.note).is_err() {
            let _ = fs::remove_file(&next);
        }
    }

    /// Whether the lock this holds is still the one in the board's folder: a folder removed while
    /// it was held, made again or not, holds another or none.
    pub(super) fn here(&self) -> bool {
        use std::os::unix::fs::MetadataExt as _;
        match (
            self.lock.0.metadata(),
            fs::metadata(self.note.with_file_name(LOCK)),
        ) {
            (Ok(held), Ok(there)) => held.dev() == there.dev() && held.ino() == there.ino(),
            _ => false,
        }
    }
}

impl Drop for Starters {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.note);
    }
}

impl Home {
    /// The starters' lock, taken to do `doing`: at once when it is free, else once its holder lets
    /// it go, saying what it waits on meanwhile, or refused once this board's bound on the wait
    /// passes ([`Home::waiting_at_most`]).
    pub(super) fn lock(&self, doing: Doing) -> Result<Starters, ServerError> {
        self.make(&self.dir)?;
        let path = self.dir.join(LOCK);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io("could not open", &path))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => self.wait_for(&file, &path)?,
            Err(TryLockError::Error(problem)) => return Err(io("could not lock", &path)(problem)),
        }
        let starters = Starters {
            note: self.dir.join(NOTE),
            lock: FileLock(file),
        };
        starters.doing(doing, None);
        Ok(starters)
    }

    /// Wait until `file`, the starters' lock at `path`, is this process's, saying what holds it.
    fn wait_for(&self, file: &File, path: &Path) -> Result<(), ServerError> {
        let began = Instant::now();
        let mut said: Option<Instant> = None;
        loop {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again for another starter's file, where its lock is the event"
            )]
            std::thread::sleep(POLL);
            match file.try_lock() {
                Ok(()) => return Ok(()),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(problem)) => {
                    return Err(io("could not lock", path)(problem));
                }
            }
            let waited = began.elapsed();
            if let Some(bound) = self.lock_wait {
                if waited >= bound {
                    return Err(ServerError::NotUp(format!(
                        "the board's starters' lock ({}) is held by {}; this read waits at most {} \
                         s for it, and the next one tries again",
                        path.display(),
                        self.starter(),
                        bound.as_secs_f64()
                    )));
                }
                continue;
            }
            let due = match said {
                None => waited >= GRACE,
                Some(at) => at.elapsed() >= AGAIN,
            };
            if due {
                (self.tell)(&if said.is_none() {
                    format!(
                        "· waiting on the board's starters' lock ({}), held by {}",
                        path.display(),
                        self.starter()
                    )
                } else {
                    format!(
                        "· still waiting on the board's starters' lock after {} s, held by {}",
                        waited.as_secs(),
                        self.starter()
                    )
                });
                said = Some(Instant::now());
            }
        }
    }

    /// What the starters' lock's holder does, as its note says, while that process runs: none when
    /// the note is missing, does not read, or names a process that has ended.
    pub(super) fn note(&self) -> Option<Note> {
        let text = fs::read_to_string(self.dir.join(NOTE)).ok()?;
        let note: Note = crate::objects::parse(&text)?;
        pid_alive(note.pid).then_some(note)
    }

    /// What holds the starters' lock, said: as its holder's note says, or a holder that writes none.
    fn starter(&self) -> String {
        self.note().map_or_else(
            || {
                "a starter that says nothing of what it does (a build without the lock's note)"
                    .to_string()
            },
            |note| self.said(&note),
        )
    }

    /// What the starters' lock's holder is doing, while one does: what `tasks database` says of it.
    pub fn under_way(&self) -> Option<String> {
        self.note().map(|note| self.said(&note))
    }

    /// `note`, said.
    pub(super) fn said(&self, note: &Note) -> String {
        let doing = match note.doing {
            Doing::Starting => "a start of the board's server (or a wait for it to answer)".into(),
            Doing::Replaying => format!(
                "{REPLAYING}{}{REPLAYED_BEFORE}: a log of gigabytes takes minutes, and its server's \
                 lines go to {}",
                note.logged
                    .map(|logged| format!(" of {}", crate::size(logged)))
                    .unwrap_or_default(),
                self.dir.join(LOG).display()
            ),
            Doing::Stopping => {
                "a stop of the board's server, waiting while it closes its store".into()
            }
            Doing::Holding => {
                "the hold's stop of the board's server, waiting while it closes its store".into()
            }
            Doing::Releasing => "a release of the board's hold".into(),
            Doing::Reclaiming => "a reclaim of the board's store (`tasks-reclaim`)".into(),
            Doing::Snapshotting => "a snapshot of the board's store (`tasks snapshot`)".into(),
            Doing::Moving => "a move of the board's store to another engine, or a restore of a \
                              snapshot over it (`tasks-move`)"
                .into(),
        };
        format!("pid {}, since {}: {doing}", note.pid, local_minute(note.at))
    }
}
