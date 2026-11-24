//! The board's home and its database server (slice 3 of `tools/docs-app-rust-port`; the owner,
//! card tasks-store-engine: "server should start the db").
//!
//! The home is `symphony/tasks/` in the git common dir ([`Home::in_common`]), so every worktree
//! reaches the one board. Its server is the machine's PostgreSQL ([`postgres`]), started on the
//! home's data when a command needs it and stopped by the owner's word, every start, stop, hold,
//! release, snapshot and reclaim under the starters' lock ([`Home::lock`]): a caller that waits on
//! another's says what it waits on, and one whose thread has others to serve, the Symphony's
//! reads, is refused once its bound on that wait passes ([`Home::waiting_at_most`]). While the
//! board is held ([`Home::held`]) nothing starts it.
//!
//! A board kept in a scratch place ([`scratch`]: under the temporary folder or a checkout's
//! `artifacts/`) is a fixture's: its server ends when the process that started it ends, however it
//! ends.
//!
//! The home's durable engine identity selects what a command and the machine server reach
//! ([`Home::board_server`], [`backend`]): a board not made yet is made on PostgreSQL. A home that
//! holds SurrealDB data, as a board an earlier release kept, is recognised and refused, never
//! started nor taken for a board not made yet: such a board moves by an earlier release
//! ([`GONE`]).

use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod export;
mod hold;
mod moving;
pub use moving::{EXPORT, Journal, Kind, Moved, STEPS};
pub mod postgres;
pub use postgres::{Postgres, Serving};
mod recovery;
pub use recovery::FileAuthority;
mod scratch;
pub mod snapshot;
mod starters;

pub use export::EXPORT_LIMIT;
pub use hold::Hold;
pub use scratch::scratch;
use starters::Doing;
mod backend;
pub use backend::Engine;

/// The board's folder in the git common dir.
pub const FOLDER: &str = crate::folders::TASKS;

/// How a board still on SurrealDB is moved, which a build without SurrealDB says wherever it
/// meets one: by an earlier release, the last that serves SurrealDB, never by this one.
pub const GONE: &str = "a board still on SurrealDB moves to PostgreSQL with an earlier release's \
                        `tasks-move postgresql --as HANDLE` (release-2026-11-22.3 has it), run \
                        from its project's main checkout, then `tasks recovery rebind`, before \
                        it takes this release";

/// In the board's folder: the address file a SurrealDB server of an earlier release recorded,
/// which a port-only reach still reads ([`crate::live::recorded`]).
pub(crate) const ADDRESS: &str = "server.json";
/// In the board's folder: the lock its starters take.
const LOCK: &str = "lock";
/// In the board's folder: a SurrealDB store, which an earlier release kept, and where that
/// release's reclaim put the store it moved aside (`reclaim/old`): what a home is recognised as
/// SurrealDB's by ([`Home::backend`]).
const DATA: &str = "data";
const RECLAIM: &str = "reclaim";
const OLD: &str = "old";
/// How long a server is waited for to come up or to end, on a machine as loaded as this one gets.
const PATIENCE: Duration = Duration::from_secs(60);

/// Why the board's server cannot be had.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    /// It was started and did not come up, or it runs and does not answer: the reason and its log.
    NotUp(String),
    /// The board's folder could not be read or written, or its home refuses what was asked.
    Io(String),
    /// The board's database is held stopped ([`Home::held`]): nothing starts it until the release.
    Held(Hold),
    /// A release asked of a hold another handle set, without naming its holder ([`Home::release`]).
    NotHolder(Hold),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::NotUp(why) | ServerError::Io(why) => f.write_str(why),
            ServerError::Held(hold) => write!(
                f,
                "the board's database is {hold}; nothing starts it until its holder releases it \
                 (`just tasks database release`, run by {by}, or by another handle naming it with \
                 `--of`): wait for that, or ask {by}",
                by = hold.by
            ),
            ServerError::NotHolder(hold) => write!(
                f,
                "the board's database is {hold}; only {by} releases it, or another handle naming \
                 it (`--of {by}`), so that no hold is released by mistake in the middle of work on \
                 the data",
                by = hold.by
            ),
        }
    }
}

#[inline(never)]
fn io(what: &str, path: &Path) -> impl Fn(std::io::Error) -> ServerError {
    let said = format!("{what} {}", path.display());
    move |problem| ServerError::Io(format!("{said}: {problem}"))
}

/// The board's server as a reach names it ([`Home::board_server`]): where it answers
/// (`postgresql:SOCKET_FOLDER`), its process, and the release of the programs that serve it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Running {
    pub addr: String,
    pub pid: i64,
    pub version: String,
}

/// Where what a start or a wait says goes, a line at a time ([`Home::telling`]).
type Tell = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// Where the board's server keeps its data, its lock and its hold.
#[derive(Clone)]
pub struct Home {
    dir: PathBuf,
    /// The folder `dir` is kept in, never made by the board ([`Home::make`]): the repository's
    /// common dir for [`Home::in_common`], else the folder holding `dir`.
    within: PathBuf,
    patience: Duration,
    /// Whether its servers end with the process that starts them; by its place ([`scratch`])
    /// unless a test says.
    supervised: Option<bool>,
    tell: Tell,
    /// How long a caller waits on the starters' lock before it is refused; none: until it is free.
    lock_wait: Option<Duration>,
    /// Where a test cuts a restore's switch ([`Home::stopping_after`]).
    cut: Option<moving::Cut>,
}

impl Home {
    /// The board in `dir`, as [`Home::at`] gives it. The second argument, the `surreal` binary an
    /// earlier release served the board by, is read by nothing: a board's server is its
    /// PostgreSQL ([`Home::postgresql`]). It keeps its signature by the two-release rule (RULES.md
    /// `code/consumer-api-two-releases`), as a consumer's code calls it, until a release after the
    /// consumer moves to [`Home::at`] removes it.
    pub fn new(dir: &Path, _surreal: &Path) -> Home {
        Home::at(dir)
    }

    /// The board in `dir`.
    pub fn at(dir: &Path) -> Home {
        Home {
            dir: dir.into(),
            within: dir.parent().unwrap_or(dir).into(),
            patience: PATIENCE,
            supervised: None,
            tell: std::sync::Arc::new(|line| eprintln!("{line}")),
            lock_wait: None,
            cut: None,
        }
    }

    /// Where what its starts and waits say goes, a line at a time: a start of its server, what a
    /// wait on another's start waits on. Its own stderr unless a caller says.
    pub fn telling(self, tell: impl Fn(&str) + Send + Sync + 'static) -> Home {
        Home {
            tell: std::sync::Arc::new(tell),
            ..self
        }
    }

    /// Refuse a caller that has waited `bound` on another's start, stop or hold (the starters'
    /// lock), naming it, rather than wait for it to end, and say nothing meanwhile: a read whose
    /// thread has other callers to serve. Otherwise a caller waits until the lock is free, saying
    /// what it waits on.
    pub fn waiting_at_most(self, bound: Duration) -> Home {
        Home {
            lock_wait: Some(bound),
            ..self
        }
    }

    /// The board of the repository whose common dir is `common`: `symphony/tasks/` there.
    pub fn in_common(common: &Path) -> Home {
        Home {
            within: common.into(),
            ..Home::at(&common.join(FOLDER))
        }
    }

    /// `folder`, the board's own or one in it, made where it is not there, with each folder between
    /// it and the one the board is kept in (`within`), one at a time, but never that one or a
    /// folder above it: a start, stop or ask under way when the project's folder or its `.git` is
    /// removed never makes them again, and is refused, saying [`crate::project::GONE`], so that it
    /// counts as a project with no board.
    fn make(&self, folder: &Path) -> Result<(), ServerError> {
        let mut made = self.within.clone();
        for part in folder
            .strip_prefix(&self.within)
            .unwrap_or(folder)
            .components()
        {
            made.push(part);
            match fs::create_dir(&made) {
                Ok(()) => {}
                Err(problem)
                    if problem.kind() == std::io::ErrorKind::AlreadyExists && made.is_dir() => {}
                Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ServerError::Io(format!(
                        "{}, which holds the board's folder, is gone, and nothing is made there \
                         again: {}",
                        self.within.display(),
                        crate::project::GONE
                    )));
                }
                Err(problem) => return Err(io("could not make", &made)(problem)),
            }
        }
        Ok(())
    }

    /// How long a server that is starting, or one asked to stop, is waited for.
    pub fn patience(self, patience: Duration) -> Home {
        Home { patience, ..self }
    }

    /// Whether the servers this board starts end with the process that starts them, whatever its
    /// place: a test of one start in a folder that may be either, as a checkout's target folder
    /// under the temporary folder is a scratch place. Otherwise a board in a scratch place
    /// ([`scratch`]) ends with its starter and every other outlives it.
    pub fn supervised(self, supervised: bool) -> Home {
        Home {
            supervised: Some(supervised),
            ..self
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The refusal of what was asked of a board still on SurrealDB, which this build serves no
    /// more: where it is, how such a board moves ([`GONE`]), and `nothing`, what was not done.
    pub(crate) fn on_surrealdb(&self, nothing: &str) -> ServerError {
        ServerError::Io(format!(
            "the board in {} is on SurrealDB, which this build serves no more: {GONE}. {nothing}",
            self.dir.display()
        ))
    }
}

/// The common dir of the repository whose board `command` serves, a command line as `ps` prints it:
/// a board's PostgreSQL, `…/postgres -D DATA`, its data `DATA` the board's own
/// (`COMMON/symphony/tasks/postgresql`) in a git common dir (one holding `HEAD`). None for any other
/// command line: a server on other data, any other program.
pub fn board_served(command: &str) -> Option<PathBuf> {
    let (program, data) = command.rsplit_once(" -D ")?;
    if !Path::new(program)
        .file_name()
        .is_some_and(|name| name == "postgres")
    {
        return None;
    }
    let data = Path::new(data);
    if data.file_name()? != postgres::DATA {
        return None;
    }
    let common = common_of(data.parent()?)?;
    common.join("HEAD").is_file().then(|| common.to_path_buf())
}

/// The common dir `folder` is the board's folder of, as [`board_served`] reads it: [`FOLDER`] in it.
fn common_of(folder: &Path) -> Option<&Path> {
    if !folder.ends_with(FOLDER) {
        return None;
    }
    folder
        .ancestors()
        .nth(Path::new(FOLDER).components().count())
}

/// Whether `command`, a command line as `ps` prints it, ends with the argument `last`.
fn last_argument_is(command: &str, last: &str) -> bool {
    command
        .strip_suffix(last)
        .is_some_and(|rest| rest.ends_with(' '))
}

/// What the log says from byte `from` on.
fn since(log: &Path, from: u64) -> String {
    let mut said = String::new();
    if let Ok(mut file) = File::open(log)
        && file.seek(SeekFrom::Start(from)).is_ok()
    {
        let _ = file.read_to_string(&mut said);
    }
    said
}

#[cfg(test)]
mod tests;
