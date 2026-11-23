//! The board's own SurrealDB server (slice 3 of `tools/docs-app-rust-port`; the owner, card
//! tasks-store-engine: "server should start the db").
//!
//! It keeps its data on disk in `symphony/tasks/` in the git common dir, so every worktree reaches
//! the one board, and it is found by the address it wrote there. Whatever needs the board starts it
//! when it is not running ([`Home::ensure`]), under a lock so callers at once start one, detached
//! so it outlives the caller. That is also the restart: a server that was killed is started again
//! on the same data. A recorded server older than the pinned SurrealDB is replaced; a newer one, a
//! tree ahead of this one, is used as it is.
//!
//! Every server it starts keeps the board's values in its sstables ([`Storage`]), where compaction
//! reclaims what a rewrite leaves, and [`Home::reclaim`] rebuilds a store that grew before it did.
//!
//! A server is never killed. It is asked to end (SIGTERM) and waited for while it writes what its
//! memtable holds to an sstable and closes its store ([`Home::end`]); one that has not answered yet
//! is still starting, replaying its log, and is never signalled at all, nor given up on: a server
//! that does not come up within the patience is left running and the caller told so, and how to
//! tell a replay from a hang. A start on a store whose log may not fit the board's memtable (one an
//! end without a clean close left, written under a larger one) keeps the store as it is first, a
//! snapshot never pruned ([`Home::keep`]), then replays the log in a private server whose memtable
//! holds it, and stops that one cleanly ([`Home::replay`]); any other start of a board outside a
//! scratch place takes a snapshot first ([`Home::snapshot_before_start`]). A replay's server is
//! never the board's: its record ([`left`]) names it until it has closed the store, so one its
//! starter left running is stopped cleanly by the next start once it answers, never before, and the
//! board's own started in its place. While the board is held ([`Home::hold`]) nothing starts it.
//! A caller that waits on another's start says what it waits on ([`Home::lock`]). A caller whose
//! thread has others to serve, the Symphony's reads, is refused once its bound on that wait passes
//! ([`Home::waiting_at_most`]), and a start of its own that replays runs on a thread of its own, the
//! caller refused at once naming it ([`Home::starting_apart`]).
//!
//! A board kept in a scratch place ([`scratch`]: under the temporary folder or a checkout's
//! `artifacts/`) is a fixture's: its server is not detached but supervised ([`OWNED_START`]), and
//! ends when the process that started it ends, however it ends.
//!
//! Beside SurrealDB, the board's PostgreSQL ([`postgres`]) starts and stops under the same
//! starters' lock. The home's durable engine identity selects what repository discovery and the
//! machine server read ([`Home::board_server`]); a home with none is SurrealDB's while it holds a
//! SurrealDB store, and a board not made yet is made on PostgreSQL ([`backend`]).

use crate::port::{self, NotStarted, Start};
use crate::rpc;
use crate::sys::{Stdio, command, output, pid_alive, wait_output};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod apart;
mod ending;
mod export;
#[cfg(test)]
mod fetch_tests;
mod hold;
mod left;
pub use left::{LEFT_STOPS, LeftReplay};
mod moving;
pub use moving::{EXPORT, Journal, Kind, Moved, STEPS};
pub mod postgres;
pub use postgres::{Postgres, Serving};
mod reclaim;
mod recovery;
pub use recovery::FileAuthority;
mod scratch;
pub mod snapshot;
mod starters;
mod storage;

pub use apart::replaying_in;
pub use export::EXPORT_LIMIT;
pub use hold::{Hold, Holding};
pub use reclaim::{Reclaimed, bytes};
use scratch::Lifeline;
pub use scratch::scratch;
use snapshot::Before;
pub use snapshot::{Kept, Snapshot, Taken};
use starters::{Doing, Starters};
pub use storage::Storage;
mod backend;
pub use backend::Engine;
use storage::store;

/// The SurrealDB the board runs on: `tools/surreal.sh`'s `VERSION`, which fetches it.
pub const SURREAL_VERSION: &str = "3.3.0";

/// SurrealDB's name for each build of [`SURREAL_VERSION`] the board runs on and the SHA-256 it
/// publishes for that build's tarball, as `tools/surreal.sh` pins them (`pinned`), by the machine
/// (`std::env::consts::OS` and `ARCH`) each runs on.
const SURREAL_BUILDS: [(&str, &str, &str, &str); 4] = [
    (
        "macos",
        "aarch64",
        "darwin-arm64",
        "75e37adf5f9aacfa1308df71193b2d0e0727bc6532105541c083e71271cd7fc9",
    ),
    (
        "macos",
        "x86_64",
        "darwin-amd64",
        "c8e560d37c9f6f04b95f22791723122b5579f1fa5ac5102d6d82c36e38b72710",
    ),
    (
        "linux",
        "x86_64",
        "linux-amd64",
        "44aeab565f7e7e39d2d0bf0583c8aae648babc91373c70b2658288d95bbbcd55",
    ),
    (
        "linux",
        "aarch64",
        "linux-arm64",
        "f03356497f875057126641f06757671542e0a7b75f18fd13820c2dab29349d74",
    ),
];

/// The shell that fetches SurrealDB [`SURREAL_VERSION`] for the machine `os` and `arch` name
/// (`std::env::consts`) to the file `surreal`, as `tools/surreal.sh` fetches it: SurrealDB's release
/// tarball, its SHA-256 checked against the pinned one before the `surreal` it holds is unpacked in
/// a folder beside `surreal` and renamed into place, so an unpack cut short never leaves a partial
/// binary where the board runs one. The tarball and that folder go however it ends. None for a
/// machine no build is pinned for.
pub fn surreal_fetch(surreal: &Path, os: &str, arch: &str) -> Option<String> {
    let (_, _, build, sha) = SURREAL_BUILDS
        .iter()
        .find(|(of, on, _, _)| *of == os && *on == arch)?;
    fetch_line(surreal, build, sha, os)
}

/// [`surreal_fetch`] of SurrealDB's build `build`, whose tarball's SHA-256 is `sha`, on `os`. The
/// line names `surreal`'s folder once, as the shell variable `d` every step reads, so it stays short
/// enough to read and to paste whole at any path depth.
fn fetch_line(surreal: &Path, build: &str, sha: &str, os: &str) -> Option<String> {
    let folder = surreal.parent()?;
    // Read inside the double quotes of `"$d/NAME"`: a name the shell would read there is no build's.
    let name = surreal.file_name()?.to_str().filter(|name| {
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    })?;
    let folder = format!("'{}'", folder.display().to_string().replace('\'', "'\\''"));
    let check = if os == "macos" {
        "shasum -a 256 -c"
    } else {
        "sha256sum -c"
    };
    // The tarball and the unpack folder are in the same folder as `surreal`, so the move is a
    // rename.
    Some(format!(
        "d={folder} && mkdir -p \"$d\" && curl -fsSLo \"$d/{TARBALL}\" \
         https://download.surrealdb.com/v{SURREAL_VERSION}/surreal-v{SURREAL_VERSION}.{build}.tgz \
         && printf '%s  %s\\n' {sha} \"$d/{TARBALL}\" | {check} && mkdir -p \"$d/{UNPACK}\" && tar \
         -xzf \"$d/{TARBALL}\" -C \"$d/{UNPACK}\" surreal && mv -f \"$d/{UNPACK}/surreal\" \
         \"$d/{name}\"; s=$?; rm -f \"$d/{TARBALL}\" \"$d/{UNPACK}/surreal\"; rmdir \
         \"$d/{UNPACK}\" 2>/dev/null; [ $s -eq 0 ]",
    ))
}

/// The fetch's tarball and the folder it unpacks in, beside the `surreal` it installs
/// ([`fetch_line`]); neither outlasts the fetch.
const TARBALL: &str = "surreal.tgz";
const UNPACK: &str = ".unpack";

/// The board's folder in the git common dir.
pub const FOLDER: &str = crate::folders::TASKS;

/// In the board's folder: where the server answers, the lock its starters take, its log, its data.
pub(crate) const ADDRESS: &str = "server.json";
const LOCK: &str = "lock";
const LOG: &str = "server.log";
const DATA: &str = "data";
/// In the board's folder, where a reclaim works ([`Home::reclaim`]): the `exports/` it keeps, the new
/// store it builds in `data/`, the exports of that store it checks in `check/`, and the old store,
/// `old/`, from the swap to its removal.
const RECLAIM: &str = "reclaim";
const OLD: &str = "old";
/// How long a server is waited for to come up or to answer, on a machine as loaded as this one gets.
const PATIENCE: Duration = Duration::from_secs(60);
/// How many patiences a private replay of a log ([`Home::replay`]) is waited for to come up: a log
/// of gigabytes takes minutes.
const REPLAYING: u32 = 10;
/// How the server's log's line before a private replay's server's own lines begins
/// ([`Home::replay`], [`replay_begins`]).
pub const REPLAY_BEGINS: &str = "· a private replay of the store's log begins";
/// How the server's log's line once a private replay's server closed the store cleanly begins,
/// before what follows it: [`REPLAY_ENDS`] at a start, another at a stop of what serves the data.
pub const REPLAY_THROUGH: &str = "· the private replay is through, its server closed the store \
                                  cleanly";
/// The line the server's log holds once a private replay's server closed the store cleanly, at a
/// start, whose own replay it was or whose starter left it running ([`left`]).
pub const REPLAY_ENDS: &str = "· the private replay is through, its server closed the store \
                               cleanly: the board's server starts below";

/// The server's log's line before a private replay of a log of `logged` bytes: its server's lines
/// and its clean stop's `Goodbye!` follow, the replay's, not the board's server's, and the snapshot
/// named just before it is the store as it was before the replay.
pub fn replay_begins(logged: u64) -> String {
    format!(
        "{REPLAY_BEGINS} ({}): its server's lines and its clean stop's `Goodbye!` below are the \
         replay's, not the board's server's, and the snapshot named above is the store before it, \
         its log not yet replayed",
        crate::size(logged)
    )
}
/// How long a server asked to end is waited for while it closes its store, before it is left to
/// finish and the stop said to be still under way: a memtable of the board's 256 MiB is written in
/// seconds, one a replay took up in a minute or two.
const CLOSING: Duration = Duration::from_secs(600);
/// The server starts detached: in a session of its own with no terminal (SurrealDB ends on a hangup,
/// which a closed pane would send its whole session), with none of its starter's files nor its
/// starter's agent session and hc terminal ([`crate::sys::SESSION_VARS`]), and reaped by init, as
/// `sh` ends at once. No signal is blocked and every one is at its default, whatever its starter
/// ignored or blocked (so a SIGTERM reaches it), but SIGINT and SIGQUIT, which `sh` ignores in a
/// job it starts in the background until SurrealDB sets its own handlers.
const START: &str = r#""$0" start --unauthenticated --no-banner --log warn --bind "$1" "$2" >>"$3" 2>&1 </dev/null & echo $!"#;
/// [`START`] for a scratch board's server: the same server, with the same command line, under a
/// supervisor shell of its own session, its parent while it runs, so it is never left without
/// one. The supervisor's watchdog reads the starter's [`Lifeline`], given as `sh`'s input, and
/// ends the server once that reads its end: when the process that started it ends, however it
/// ends; never a server already asked to end, which `$4`, the folder of those ([`ending`]), names,
/// as a second signal ends SurrealDB before it has closed its store. The supervisor ends with the
/// server and says its pid, as `START` does.
const OWNED_START: &str = r#"exec 3<&0 </dev/null
(
    "$0" start --unauthenticated --no-banner --log warn --bind "$1" "$2" >>"$3" 2>&1 3<&- &
    server=$!
    echo "$server"
    exec >/dev/null 2>&1
    ( while IFS= read -r line; do :; done <&3; [ -e "$4/$server" ] || kill "$server" 2>/dev/null ) &
    watchdog=$!
    exec 3<&-
    wait "$server"
    kill "$watchdog" 2>/dev/null
) &"#;

#[cfg(test)]
thread_local! {
    /// Set by the library's tests on their own thread: how many of the next reads of the process
    /// table the board tells servers by fail (`ps` in [`Home::listed`] and [`Home::serves`], the
    /// kernel's stamp in [`Home::process_start`]), as one can on a loaded machine; `u32::MAX` for
    /// every one.
    static PS_FAILS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether this `ps` fails, as a library test made it ([`PS_FAILS`]).
#[cfg(test)]
fn ps_fails() -> bool {
    let left = PS_FAILS.get();
    if left != u32::MAX {
        PS_FAILS.set(left.saturating_sub(1));
    }
    left > 0
}

/// Why the board's server cannot be had.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    /// The pinned `surreal` is not where it is fetched to, and the board has no store: a project
    /// with no board on this machine.
    NoSurreal(PathBuf),
    /// The board's store is there and the `surreal` that serves it is not: a board that is unwell,
    /// as after a SurrealDB version bump before the new binary is fetched, or with
    /// `SYMPHONY_SURREAL` naming a file that is not there.
    Unserved { surreal: PathBuf, data: PathBuf },
    /// It was started and did not come up, or it runs and does not answer: the reason and its log.
    NotUp(String),
    /// The board's folder could not be read or written.
    Io(String),
    /// A reclaim stopped before its swap ([`Home::reclaim`]): what failed. The board's server runs on
    /// the old store again, untouched.
    Reclaim(String),
    /// The board's database is held stopped ([`Home::hold`]): nothing starts it until the release.
    Held(Hold),
    /// A release asked of a hold another handle set, without naming its holder ([`Home::release`]).
    NotHolder(Hold),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::NoSurreal(path) => {
                write!(f, "{}{}", no_surreal(), path.display())?;
                how_to_have(f, path)
            }
            ServerError::Unserved { surreal, data } => {
                write!(
                    f,
                    "the board's store at {} cannot be served: SurrealDB {SURREAL_VERSION}, which \
                     serves it, is not at {}",
                    data.display(),
                    surreal.display()
                )?;
                how_to_have(f, surreal)
            }
            ServerError::NotUp(why) | ServerError::Io(why) => f.write_str(why),
            ServerError::Reclaim(why) => write!(
                f,
                "the reclaim stopped and the board runs on its old store, untouched: {why}"
            ),
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

/// What [`ServerError::NoSurreal`] says before the path it looked at
/// ([`crate::board_health::absent`] reads it), and [`ServerError::Unserved`] never says.
pub(crate) fn no_surreal() -> String {
    format!("SurrealDB {SURREAL_VERSION} is not at ")
}

/// How to have the `surreal` that is not at `path`: the command that fetches the pinned one there,
/// or the `SYMPHONY_SURREAL` that named it.
fn how_to_have(f: &mut fmt::Formatter<'_>, path: &Path) -> fmt::Result {
    if pinned_in(path).is_none() {
        return f.write_str(", where `SYMPHONY_SURREAL` names it");
    }
    match surreal_fetch(path, std::env::consts::OS, std::env::consts::ARCH) {
        Some(fetch) => write!(f, ": fetch it there (`{fetch}`)")?,
        None => f.write_str(": put its release binary for this machine there")?,
    }
    f.write_str(", or name another with `SYMPHONY_SURREAL`")
}

/// The common dir `surreal` is the pinned binary of ([`Home::in_common`]):
/// `<common>/symphony/surreal/<SURREAL_VERSION>/surreal`; none for a binary anywhere else, one
/// `SYMPHONY_SURREAL` names.
fn pinned_in(surreal: &Path) -> Option<&Path> {
    let tail = pinned(Path::new(""));
    if !surreal.ends_with(&tail) {
        return None;
    }
    surreal.ancestors().nth(tail.components().count())
}

/// Where the pinned `surreal` is kept in the git common dir `common`: where `tools/surreal.sh`
/// fetches it, and [`surreal_fetch`] says how to.
fn pinned(common: &Path) -> PathBuf {
    common
        .join(crate::folders::SURREAL)
        .join(SURREAL_VERSION)
        .join("surreal")
}

#[inline(never)]
fn io(what: &str, path: &Path) -> impl Fn(std::io::Error) -> ServerError {
    let said = format!("{what} {}", path.display());
    move |problem| ServerError::Io(format!("{said}: {problem}"))
}

/// A server this module started, as its address file records it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Running {
    pub addr: String,
    pub pid: i64,
    pub version: String,
}

/// Where what a start or a wait says goes, a line at a time ([`Home::telling`]).
type Tell = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// Where the board's server keeps its data, its address, its lock and its log.
#[derive(Clone)]
pub struct Home {
    dir: PathBuf,
    /// The folder `dir` is kept in, never made by the board ([`Home::make`]): the repository's
    /// common dir for [`Home::in_common`], else the folder holding `dir`.
    within: PathBuf,
    surreal: PathBuf,
    patience: Duration,
    closing: Duration,
    storage: Storage,
    /// Whether its servers are supervised; by its place ([`scratch`]) unless a test says.
    supervised: Option<bool>,
    /// Whether a snapshot of its store is taken before each start ([`snapshot`]); by its place
    /// unless a test says.
    snapshotting: Option<bool>,
    tell: Tell,
    /// How long a caller waits on the starters' lock before it is refused; none: until it is free.
    lock_wait: Option<Duration>,
    /// Whether a start that replays the store's log runs apart from its caller
    /// ([`Home::starting_apart`]).
    apart: bool,
    /// Where a test cuts a move's or a restore's switch ([`Home::stopping_after`]).
    cut: Option<moving::Cut>,
}

impl Home {
    /// The board in `dir`, served by the `surreal` binary at `surreal`.
    pub fn new(dir: &Path, surreal: &Path) -> Home {
        Home {
            dir: dir.into(),
            within: dir.parent().unwrap_or(dir).into(),
            surreal: surreal.into(),
            patience: PATIENCE,
            closing: CLOSING,
            storage: Storage::BOARD,
            supervised: None,
            snapshotting: None,
            tell: std::sync::Arc::new(|line| eprintln!("{line}")),
            lock_wait: None,
            apart: false,
            cut: None,
        }
    }

    /// Where what its starts and waits say goes, a line at a time: the store kept before a replay,
    /// the replay, what a wait on another's start waits on. Its own stderr unless a caller says.
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

    /// The board of the repository whose common dir is `common`: `symphony/tasks/` there, served by
    /// the pinned `surreal` beside it ([`surreal_fetch`]), or by `named` (`SYMPHONY_SURREAL`'s).
    pub fn in_common(common: &Path, named: Option<&Path>) -> Home {
        Home {
            within: common.into(),
            ..Home::new(
                &common.join(FOLDER),
                &named.map_or_else(|| pinned(common), Path::to_path_buf),
            )
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

    /// How long a server that is starting, or that runs and does not answer, is waited for.
    pub fn patience(self, patience: Duration) -> Home {
        Home { patience, ..self }
    }

    /// How long a server asked to end is waited for while it closes its store, before the stop is
    /// said to be still under way and the server left to finish ([`Home::end`]).
    pub fn closing(self, closing: Duration) -> Home {
        Home { closing, ..self }
    }

    /// How the servers this board starts keep their data: [`Storage::BOARD`] unless a test asks
    /// for a memtable small enough to flush, or for the storage a store grew under.
    pub fn storage(self, storage: Storage) -> Home {
        Home { storage, ..self }
    }

    /// Whether the servers this board starts are supervised ([`OWNED_START`]) or detached
    /// ([`START`]), whatever its place: a test of one start in a folder that may be either, as a
    /// checkout's target folder under the temporary folder is a scratch place. Otherwise a board in
    /// a scratch place ([`scratch`]) is supervised and every other detached.
    pub fn supervised(self, supervised: bool) -> Home {
        Home {
            supervised: Some(supervised),
            ..self
        }
    }

    /// Whether a snapshot of its store is taken before each start of its server
    /// ([`Home::snapshot`]), whatever its place: a test's. Otherwise a board outside a scratch place
    /// ([`scratch`]) takes one and a scratch one does not.
    pub fn snapshotting(self, snapshotting: bool) -> Home {
        Home {
            snapshotting: Some(snapshotting),
            ..self
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn surreal(&self) -> &Path {
        &self.surreal
    }

    /// Whether the board's store is there, as it is once a server has run on it: none is a board
    /// not made yet. A server that runs makes the store, never the board: only a writer does that
    /// ([`crate::schema::decide`]).
    pub fn has_store(&self) -> bool {
        fs::symlink_metadata(self.dir.join(DATA)).is_ok()
    }

    /// What the address file says, without asking the server: the fast path, which goes to
    /// [`Home::ensure`] when the server does not answer.
    pub fn recorded(&self) -> Option<Running> {
        let text = fs::read_to_string(self.dir.join(ADDRESS)).ok()?;
        crate::objects::parse(&text)
    }

    /// The board's server, answering: the recorded one, else one started now. The recorded one counts only
    /// if its process serves this board's data (an address file can name any server that answers), and
    /// one older than the pinned version is replaced; a newer one is used as it is, so that two trees
    /// pinning two versions never end each other's server. An answer on a server's address, recorded or
    /// found, counts only from that process's own listener there ([`port::listens`]): a process that
    /// took its port answers too. Where that cannot be told (no `lsof` runs), the answer counts for a
    /// server kept, started or found, and the start or the wait that took it says so in one line
    /// ([`unsure`]); one that would be ended on it (an older one, a replay's) is not, the caller
    /// refused naming why ([`untold`]): nothing is ended on a check that could not say. A recorded
    /// server that ends while it is waited for is started again.
    /// One that runs and does not answer within the patience, recorded or not, is left running,
    /// never ended nor joined by a second: it is still replaying its log, or stuck, and the refusal
    /// says so. Refused while the board is held ([`Home::hold`]). A caller starting apart
    /// ([`Home::starting_apart`]) is refused at once while this process runs a start of the board
    /// apart.
    ///
    /// A private replay's server is never the board's ([`left`]): while the replay's record stands,
    /// the recorded server is not taken without the lock, and under it the replay is settled first.
    /// Every server on the data is the replay's while its record stands, the one the address file
    /// names included: one that has not answered is waited for, never signalled before it answers,
    /// the caller refused naming the replay; once it answers it is stopped cleanly and the board's
    /// own server started. A caller starting apart has that stop and start run apart, as a replay
    /// is. The record goes only once nothing it may name runs; while that cannot be told (a `ps`
    /// that fails), the caller is refused and nothing is adopted or started.
    pub fn ensure(&self) -> Result<Running, ServerError> {
        self.require_surreal()?;
        self.unheld()?;
        if let Some(running) = self.recorded()
            && !older(&running.version)
            && !self.dir.join(left::RECORD).exists()
            && self.serves(running.pid)
            && rpc::healthy(&running.addr)
            // Unknown is the server's, said when it was started or found; not on each read.
            && owns(&running).unwrap_or(true)
        {
            return Ok(running);
        }
        if self.apart {
            self.apart_under_way()?;
        }
        let starters = self.lock(Doing::Starting)?;
        self.require_surreal()?;
        self.unheld()?;
        if let Some(left) = self.left_replay_through()? {
            if self.apart {
                return Err(self.start_apart(storage::logged(&self.dir.join(DATA))));
            }
            self.stop_left_replay(&left, REPLAY_ENDS)?;
        }
        if let Some(running) = self.recorded()
            && self.serves(running.pid)
        {
            match self.answers(&running) {
                Waited::Answers if older(&running.version) => self.end(running.pid)?,
                Waited::Unsure(why) if older(&running.version) => {
                    return Err(untold(&running, &why));
                }
                Waited::Answers => return Ok(running),
                Waited::Unsure(why) => return Ok(self.kept_unsure(running, &why)),
                Waited::Ended => {}
                Waited::Silent => return Err(self.silent(&running)),
            }
        }
        if let Some(running) = self.unrecorded() {
            let running = match self.answers(&running) {
                Waited::Answers => running,
                Waited::Unsure(why) => self.kept_unsure(running, &why),
                Waited::Ended => return self.start(&starters),
                Waited::Silent => return Err(self.silent(&running)),
            };
            self.record(&running)?;
            return Ok(running);
        }
        self.start(&starters)
    }

    /// `running`, which answers where whether from its own listener cannot be told (`why`), kept
    /// as the board's server, and that said in one line ([`unsure`]).
    fn kept_unsure(&self, running: Running, why: &str) -> Running {
        (self.tell)(&unsure(
            "the board's server",
            running.pid,
            &running.addr,
            &why,
        ));
        running
    }

    /// Why a server on this board's data that runs and has not answered within the wait
    /// ([`Home::answer_wait`]) is not the board's yet, and what becomes of it.
    fn silent(&self, running: &Running) -> ServerError {
        ServerError::NotUp(format!(
            "the board's server (pid {}, {}) runs and does not answer within {} s: it is still \
             starting (a store replaying a long log takes minutes) or stuck, and is left running, \
             never ended nor joined by a second on its data; the next command waits for it again. \
             {}. Its log is {}",
            running.pid,
            running.addr,
            self.answer_wait().as_secs_f64(),
            self.unanswered(running.pid),
            self.dir.join(LOG).display()
        ))
    }

    /// How long a server on this board's data that runs is waited for to answer
    /// ([`Home::answers`]): the patience, or a bounded caller's bound when it is shorter
    /// ([`Home::waiting_at_most`]), so the Symphony's read is never held by a replay whose
    /// starter has gone, such as a Symphony restarted while its start ran apart.
    fn answer_wait(&self) -> Duration {
        self.lock_wait
            .map_or(self.patience, |bound| bound.min(self.patience))
    }

    /// What a caller is told of `pid`, a server on this board's data that has not answered: how
    /// long it has run and on how much CPU, the log its store holds, how a replay is told from a
    /// hang, and what the owner does with one that hangs. Never a signal of its own: a server is
    /// ended before it answers only by the owner's hand, once it hangs, as a replay ended in its
    /// middle can lose what it had not yet written.
    fn unanswered(&self, pid: i64) -> String {
        let ran = output(command("ps").args(["-o", "etime=,time=", "-p", &pid.to_string()]))
            .ok()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|words| words.len() == 2)
            .map_or_else(String::new, |words| {
                format!("It has run {}, on {} of CPU, ", words[0], words[1])
            });
        format!(
            "{ran}on a store whose log holds {}: a server replaying its log works the CPU all \
             along and answers once it is through (a log of gigabytes takes minutes), while one \
             whose CPU time stands still across a minute hangs. The owner holds the board then \
             (`just tasks database hold WHY --as HANDLE`, which leaves that server running and \
             keeps anything from starting another), ends it (`kill {pid}`), and releases the hold",
            crate::size(storage::logged(&self.dir.join(DATA)))
        )
    }

    /// A server on this board's data, as [`Home::listed`] finds it; none when `ps` fails. Past the
    /// recorded one, which [`Home::ensure`] and the hold's stop take first, it is one the address
    /// file does not name (its starter was killed between its answer and its record): it holds the
    /// data's lock, so no other could start. A private replay's server is one too, and is never
    /// adopted: every start and stop settles the replay's record before it looks for one
    /// ([`Home::left_replay_through`]).
    fn unrecorded(&self) -> Option<Running> {
        self.listed().ok().flatten()
    }

    /// The first server on this board's data that a listing of every process (`ps -axo`) shows,
    /// whether or not the address file names it; an error when `ps` cannot be run or fails, as then
    /// no server on the data can be ruled out. Only a process running the board's `surreal` counts
    /// ([`server_on`]), never the supervisor shell of a scratch board's server or its watchdog,
    /// which keep the server's words among their own arguments. Its address is the one its command
    /// line binds; its version is taken as the pinned one, which started it.
    fn listed(&self) -> std::io::Result<Option<Running>> {
        #[cfg(test)]
        if ps_fails() {
            return Err(std::io::Error::other("ps failed, as a test made it"));
        }
        let out = output(command("ps").args(["-ww", "-axo", "pid=,command="]))?;
        if !out.status.success() {
            return Err(std::io::Error::other(format!(
                "ps exited with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let data = self.data();
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let (pid, line) = line.trim().split_once(char::is_whitespace)?;
                let addr = server_on(line.trim_start(), &self.surreal, &data)?;
                Some(Running {
                    addr,
                    pid: pid.parse().ok()?,
                    version: SURREAL_VERSION.into(),
                })
            }))
    }

    /// Stop the recorded server cleanly, if it runs, and forget its address: what it was, if
    /// anything. One still starting is waited for first, never signalled before it answers
    /// ([`Home::stop_one`]). Refused while the board is held, which stopped it already.
    pub fn stop(&self) -> Result<Option<Running>, ServerError> {
        self.require_surreal()?;
        let _held = self.lock(Doing::Stopping)?;
        self.require_surreal()?;
        self.unheld()?;
        let Some(running) = self.recorded() else {
            return Ok(None);
        };
        if self.serves(running.pid) {
            self.stop_one(&running)?;
        }
        self.forget()?;
        Ok(Some(running))
    }

    /// The address file removed, when there is one.
    fn forget(&self) -> Result<(), ServerError> {
        let address = self.dir.join(ADDRESS);
        match fs::remove_file(&address) {
            Err(problem) if problem.kind() != std::io::ErrorKind::NotFound => {
                Err(io("could not remove", &address)(problem))
            }
            _ => Ok(()),
        }
    }

    /// The data a server of this board is started on, as its command line names it.
    fn data(&self) -> String {
        store(&self.dir.join(DATA))
    }

    /// Why no server can be started, its `surreal` not being there: a board with no store has
    /// none to serve ([`ServerError::NoSurreal`]); one whose store is there, or cannot be told to
    /// be gone, is a board that cannot be served ([`ServerError::Unserved`]), never no board.
    pub(crate) fn no_surreal(&self) -> ServerError {
        let data = self.dir.join(DATA);
        let gone = matches!(
            fs::symlink_metadata(&data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
        );
        if gone {
            ServerError::NoSurreal(self.surreal.clone())
        } else {
            ServerError::Unserved {
                surreal: self.surreal.clone(),
                data,
            }
        }
    }

    /// Whether `pid` is this board's server: running, and on this board's data, its command line's
    /// last argument. A pid recorded once may since have gone to any other process, which is never
    /// signalled, a scratch board's supervisor shell among them: it names the data before its log.
    /// A `ps` that fails says no, so a no is never taken for the pid having ended ([`left`]).
    fn serves(&self, pid: i64) -> bool {
        if !pid_alive(pid) {
            return false;
        }
        #[cfg(test)]
        if ps_fails() {
            return false;
        }
        let Ok(out) = output(command("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]))
        else {
            return false;
        };
        last_argument_is(String::from_utf8_lossy(&out.stdout).trim(), &self.data())
    }

    /// When process `pid` started, as the kernel stamps it ([`crate::sys::process_starts`]): to the
    /// microsecond on macOS, in clock ticks after this boot on Linux, so any reading of one process
    /// says the same, which its later programs keep. Another process given that pid later says
    /// another, so a pid recorded with it ([`left`], [`ending`]) is told from one since reused. An
    /// error when the process has ended or its start cannot be read. Public as the form those
    /// records write it in, which a record an older build wrote holds too.
    pub fn process_start(pid: i64) -> std::io::Result<String> {
        #[cfg(test)]
        if ps_fails() {
            return Err(std::io::Error::other("ps failed, as a test made it"));
        }
        match crate::sys::process_starts(&[pid]).pop() {
            Some(crate::sys::Start::At(at)) => Ok(format!("{at:.6}")),
            Some(crate::sys::Start::AfterBoot(crate::sys::Boottime { boot, at })) => {
                Ok(format!("{at:.2} s after boot {boot}"))
            }
            Some(crate::sys::Start::Ended) => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("pid {pid} has ended"),
            )),
            _ => Err(std::io::Error::other(format!(
                "the kernel does not say when pid {pid} started"
            ))),
        }
    }

    /// What the server `running` names comes to within the wait ([`Home::answer_wait`]): it answers
    /// on its address from a listener of its own ([`owns`]), it ends, or it runs and does neither.
    /// One that answers where whether from its own listener cannot be told is [`Waited::Unsure`].
    fn answers(&self, running: &Running) -> Waited {
        let began = Instant::now();
        let wait = self.answer_wait();
        loop {
            if rpc::healthy(&running.addr) {
                match owns(running) {
                    Ok(true) => return Waited::Answers,
                    Ok(false) => {}
                    Err(why) => return Waited::Unsure(why),
                }
            }
            if !pid_alive(running.pid) {
                return Waited::Ended;
            }
            if began.elapsed() >= wait {
                return Waited::Silent;
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again for the board server's answer or end, which no wait reports for a process it did not start"
            )]
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Start the board's server ([`Home::spawn`]) and record it. Under the lock. A store a reclaim
    /// moved aside and never replaced is refused, never started anew and empty in its place. A
    /// server whose record fails is ended before the failure is said: left running, nobody's, the
    /// next [`Home::ensure`] would adopt it ([`Home::unrecorded`]) on whatever store it holds, one a
    /// reclaim's rollback has since moved aside among them. Refused while the board is held, and
    /// when its `surreal` is not there. Then, before anything is spawned, the store as the last
    /// server left it is kept: for a log that may not fit the board's memtable, the snapshot its
    /// replay never runs without ([`Home::replay`]), `starters` saying so to whoever waits on them;
    /// otherwise the snapshot every start takes ([`Home::snapshot_before_start`]). A start whose
    /// log is to be replayed, for a caller starting apart ([`Home::starting_apart`]), is run on a
    /// thread of its own, the caller refused at once naming it. A start with no replay says it
    /// starts, in one line ([`Home::telling`]): it takes seconds, which would otherwise pass in
    /// silence; a replay says its own. SurrealDB makes every missing folder of its store's path, so
    /// a board's folder removed while its server starts comes back, its repository's with it: a
    /// start whose folder no longer holds the lock it took ([`Starters::here`]) gives up, its
    /// server ended before it is recorded, and is refused saying [`crate::project::GONE`].
    fn start(&self, starters: &Starters) -> Result<Running, ServerError> {
        self.unheld()?;
        let old = self.dir.join(RECLAIM).join(OLD);
        if !self.dir.join(DATA).exists() && old.exists() {
            return Err(ServerError::Io(format!(
                "the board's store is at {}: a reclaim moved it there and stopped; \
                 `tasks-reclaim` puts it back",
                old.display()
            )));
        }
        if !self.surreal.is_file() {
            return Err(self.no_surreal());
        }
        let logged = storage::logged(&self.dir.join(DATA));
        if self.apart && self.storage.replaying(logged).is_some() {
            return Err(self.start_apart(logged));
        }
        if !self.replay(starters)? {
            (self.tell)(&format!(
                "· the board's server is not running: it starts now, on its store at {}, which \
                 takes some seconds",
                self.dir.join(DATA).display()
            ));
            self.snapshot_before_start();
        }
        let spawned = self.spawn(&self.data(), &[]);
        if !starters.here() {
            let ended = match &spawned {
                Ok(running) => self.end(running.pid).err(),
                Err(_) => None,
            };
            return Err(ServerError::Io(format!(
                "the board's folder {} went while its server started, and is not served: {}{}",
                self.dir.display(),
                crate::project::GONE,
                ended
                    .map(|why| format!("; and then {why}"))
                    .unwrap_or_default()
            )));
        }
        let running = spawned?;
        if let Err(problem) = self.record(&running) {
            return Err(match self.end(running.pid) {
                Ok(()) => problem,
                Err(again) => ServerError::NotUp(format!("{problem}; and then {again}")),
            });
        }
        Ok(running)
    }

    /// The log of the board's store replayed, when it may not fit the board's memtable, by a
    /// private server whose memtable holds it all, then stopped cleanly, which writes it to
    /// sstables and empties the log; nothing when the log is empty, as a clean close leaves it, or
    /// small enough. A log written under a larger memtable (a server started without the board's
    /// [`Storage`]) and replayed under the board's fills one memtable after another: SurrealKV
    /// 0.21 then records the whole log as written to sstables at the first, while the rest is only
    /// in memory, and a server that ends without a clean close before its next flush loses that
    /// rest at the following start. The private server binds no port the board's record names, so
    /// no client of the board writes to it, and is waited for [`REPLAYING`] patiences; one that does
    /// not come up by then is left running, never signalled. Its record ([`left`]), written before
    /// it starts and again with its pid and address once a start of it says them, stands until it
    /// has closed the store, so a server this start leaves running, or one whose starter ended, is
    /// never adopted as the board's: a later [`Home::ensure`] stops it cleanly once it answers and
    /// starts the board's own. Under the lock, `starters` saying to whoever waits on them that a
    /// replay runs, of how large a log. Whether it ran.
    ///
    /// Before its server starts, the store is kept as it is, a snapshot ([`Home::keep`]): a store
    /// that went back holds a log segment no server applies, and the replay's clean stop removes
    /// it. That snapshot is the only one the start takes, and it is never pruned, as it holds the
    /// log. A store that can be neither cloned nor copied is not replayed, and the start refused.
    /// Where the store is kept is told ([`Home::telling`]) and written to the server's log. A
    /// record that cannot be written refuses the replay too, before its server starts.
    fn replay(&self, starters: &Starters) -> Result<bool, ServerError> {
        let data = self.dir.join(DATA);
        let logged = storage::logged(&data);
        let Some(storage) = self.storage.replaying(logged) else {
            return Ok(false);
        };
        starters.doing(Doing::Replaying, Some(logged));
        let kept = self.keep(crate::store::now(), Before::Replay)?;
        let record = left::Replay {
            by: i64::from(std::process::id()),
            at: crate::store::now(),
            logged,
            memtable: storage.memtable,
            server: None,
            started: None,
        };
        self.mark_replay(&record)?;
        let how = match &kept {
            Kept::Cloned(path) => format!(
                "The store as it was is kept at {}, a copy-on-write clone",
                path.display()
            ),
            Kept::Copied(path) => format!(
                "The store as it was is kept at {}, a plain copy, as this volume cannot clone",
                path.display()
            ),
            Kept::Already(path) => format!(
                "The store is as an earlier start kept it at {}, which keeps it still",
                path.display()
            ),
        };
        (self.tell)(&format!(
            "· the log of the board's store at {} holds {}, more than the board's memtable takes: \
             it is replayed whole in a private server before the board's server starts (a log of \
             gigabytes takes minutes). {how}: on a store that went back, it alone keeps the log \
             segment no server applies and the replay's stop removes. The board never removes it: \
             remove it once the board reads as it should",
            data.display(),
            crate::size(logged),
        ));
        let except: Vec<u16> = self
            .recorded()
            .and_then(|running| running.addr.parse::<SocketAddr>().ok())
            .map(|addr| addr.port())
            .into_iter()
            .collect();
        self.note_in_log(&replay_begins(logged));
        // Every start of its server, retries on another port among them: the record goes after a
        // failed start only once none of them runs.
        let started = std::cell::RefCell::new(Vec::new());
        let replayed = self
            .spawn_as(
                &self.data(),
                &except,
                storage,
                self.patience.saturating_mul(REPLAYING),
                "the private replay's server",
                // Named as soon as a start of it says its pid. The record names every server on
                // the data the replay's while it stands, so a name that cannot be written leaves
                // the server running, the start refused saying so.
                &|server| {
                    started.borrow_mut().push(server.pid);
                    self.mark_replay(&left::Replay {
                        server: Some(server.clone()),
                        started: Home::process_start(server.pid).ok(),
                        ..record.clone()
                    })
                    .map_err(|problem| left::unnamed(server, &problem))
                },
            )
            .inspect_err(|_| self.settle_failed_replay(&started.borrow()))?;
        self.end(replayed.pid)?;
        self.forget_replay()?;
        self.note_in_log(REPLAY_ENDS);
        starters.doing(Doing::Starting, None);
        (self.tell)(
            "· the private replay is through, and its server closed the store cleanly: the \
             board's server starts",
        );
        Ok(true)
    }

    /// Start a server on `data` on a free port, none of `except`, with this board's [`Storage`],
    /// and wait the patience until it answers ([`Home::spawn_as`]).
    fn spawn(&self, data: &str, except: &[u16]) -> Result<Running, ServerError> {
        self.spawn_as(
            data,
            except,
            self.storage,
            self.patience,
            "the board's server",
            &|_| Ok(()),
        )
    }

    /// Start a server on `data` on a free port, none of `except`, with `storage`, and wait up to
    /// `patience` until it answers. SurrealDB is told a reserved port to bind; one that ends saying
    /// the port was taken meanwhile is started again on another ([`port`]), every start within the
    /// one patience. It is up once it answers and holds the port's listener itself
    /// ([`port::listens`]): a process that took its port answers too, while this server is still on
    /// its way to ending. One whose listener cannot be told is up once it answers, a line saying so
    /// ([`unsure`]). One that has not answered by then is left running, never signalled: it is
    /// still replaying its log, and a signal before its handlers are set ends it at once. `what`
    /// names the server in what is said of it; `on_start` is handed each start's pid and address as
    /// soon as the start says them, before it is waited for, and its error refuses the start, the
    /// server left running. Each start's pid is new to this board's servers ([`Home::started`]).
    fn spawn_as(
        &self,
        data: &str,
        except: &[u16],
        storage: Storage,
        patience: Duration,
        what: &str,
        on_start: &dyn Fn(&Running) -> Result<(), ServerError>,
    ) -> Result<Running, ServerError> {
        self.unheld()?;
        self.spawn_held(data, except, storage, patience, what, on_start)
    }

    /// [`Home::spawn_as`] also while the board is held: a move's own server on the store its
    /// caller holds stopped ([`moving`]).
    fn spawn_held(
        &self,
        data: &str,
        except: &[u16],
        storage: Storage,
        patience: Duration,
        what: &str,
        on_start: &dyn Fn(&Running) -> Result<(), ServerError>,
    ) -> Result<Running, ServerError> {
        if !self.surreal.is_file() {
            return Err(self.no_surreal());
        }
        let log = self.dir.join(LOG);
        let until = Instant::now() + patience;
        // A scratch board's server is supervised, on this process's lifeline.
        let lifeline = if self.supervised.unwrap_or_else(|| scratch(&self.dir)) {
            let lifeline = Lifeline::of_this_process()
                .map_err(io("could not make a lifeline for", &self.dir))?;
            Some(lifeline)
        } else {
            None
        };
        let started = port::start_on_free_port_except(except, port::ATTEMPTS, until, |reserved| {
            let addr = reserved.to_string();
            let from = fs::metadata(&log).map_or(0, |meta| meta.len());
            let mut start = command("/bin/sh");
            start
                .new_session()
                .default_signals()
                .envs(storage.environment())
                .arg("-c")
                .arg(if lifeline.is_some() {
                    OWNED_START
                } else {
                    START
                })
                .arg(&self.surreal)
                .arg(&addr)
                .arg(data)
                .arg(&log);
            if lifeline.is_some() {
                start.arg(self.dir.join(ending::ENDING));
            }
            let out = match lifeline {
                // As `output` reads it, but with the lifeline as its input.
                Some(lifeline) => {
                    let input = lifeline
                        .reader()
                        .map_err(io("could not hand a lifeline to", &self.surreal))?;
                    start
                        .stdin(Stdio::from(input))
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .and_then(wait_output)
                }
                None => output(&mut start),
            }
            .map_err(io("could not start", &self.surreal))?;
            let not_started =
                || ServerError::NotUp(format!("{} was not started", self.surreal.display()));
            let pid: i64 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .map_err(|_| not_started())?;
            let child = u32::try_from(pid).map_err(|_| not_started())?;
            self.started(pid);
            on_start(&Running {
                addr: addr.clone(),
                pid,
                version: SURREAL_VERSION.into(),
            })?;
            loop {
                if rpc::healthy(&addr)
                    && port::listens(child, reserved, output).unwrap_or_else(|problem| {
                        (self.tell)(&unsure(what, pid, &addr, &problem));
                        true
                    })
                {
                    return Ok(Start::Up(Running {
                        addr,
                        pid,
                        version: SURREAL_VERSION.into(),
                    }));
                }
                if !pid_alive(pid) {
                    return Ok(Start::Ended(since(&log, from)));
                }
                if Instant::now() >= until {
                    return Err(ServerError::NotUp(format!(
                        "{what} (pid {pid}, {addr}) has not answered within {} s: it is still \
                         starting (a store replaying a long log takes minutes) or stuck, and is \
                         left running, never ended nor joined by a second on its data; the next \
                         command waits for it again. {}. Its log is {}",
                        patience.as_secs(),
                        self.unanswered(pid),
                        log.display()
                    )));
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: looks again for the board server's answer or end, which no wait reports for a process it did not start"
                )]
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let not_started = |said: &str| {
            ServerError::NotUp(format!(
                "{what} did not start: {}; its log is {}",
                said.trim(),
                log.display()
            ))
        };
        match started {
            Ok((running, _)) => Ok(running),
            Err(NotStarted::Failed(error)) => Err(error),
            Err(NotStarted::NoPort(problem)) => {
                Err(io("found no free port beside", &self.dir)(problem))
            }
            Err(NotStarted::Ended(said)) => Err(not_started(&said)),
            Err(NotStarted::InUse { attempts, output }) => Err(not_started(&format!(
                "its port was taken on each of {attempts} starts, the last saying {}",
                output.trim()
            ))),
        }
    }

    /// Write the address file whole, so a reader never sees half of it.
    fn record(&self, running: &Running) -> Result<(), ServerError> {
        let address = self.dir.join(ADDRESS);
        let next = self.dir.join(format!("{ADDRESS}.{}", std::process::id()));
        let text = serde_json::to_string(running).expect("a record serialises");
        fs::write(&next, text).map_err(io("could not write", &next))?;
        fs::rename(&next, &address).map_err(io("could not write", &address))
    }
}

/// What a server that runs came to, waited on within the patience ([`Home::answers`]).
#[derive(Debug, Clone, PartialEq)]
enum Waited {
    /// It answers on its address from a listener of its own.
    Answers,
    /// It answers on its address, and whether from a listener of its own cannot be told: why
    /// ([`owns`]). A caller that keeps it says so ([`unsure`]); one that would end it refuses
    /// ([`untold`]).
    Unsure(String),
    /// Its process ended.
    Ended,
    /// It runs and does not answer from a listener of its own.
    Silent,
}

/// Whether the process `running` names holds the listener on its address itself
/// ([`port::listens`]): a health answer there is that server's only then. Where that cannot be
/// told (a record whose pid or address cannot be one, an `lsof` that cannot run or say), why: an
/// unknown, never a no.
fn owns(running: &Running) -> Result<bool, String> {
    let pid = u32::try_from(running.pid).map_err(|problem| problem.to_string())?;
    let addr: SocketAddr = running
        .addr
        .parse()
        .map_err(|problem: std::net::AddrParseError| problem.to_string())?;
    port::listens(pid, addr, output).map_err(|problem| problem.to_string())
}

/// The one line said of `what`, the server `pid` that answers on `addr`, when whether it holds that
/// address's listener itself cannot be told (`why`): it is kept and taken as up. A check that could
/// not say is never a "not listening", so no server that answers is ended or passed over on it: one
/// was ended at each start where `lsof` could not run, and its next reader started another.
fn unsure(what: &str, pid: i64, addr: &str, why: &dyn fmt::Display) -> String {
    format!(
        "· {what} (pid {pid}, {addr}) answers, and whether it holds that port's listener itself \
         cannot be told, so it is kept as up: {why}"
    )
}

/// Why `running`, which answers on its address, is not ended when whether that answer is its own
/// cannot be told (`why`): a process that took its port answers too, while it may still be on its
/// way up, before its handlers are set, where a signal ends it at once. It is left running.
fn untold(running: &Running, why: &str) -> ServerError {
    ServerError::NotUp(format!(
        "could not tell whether the board's server (pid {}, {}) listens there: {why}; it is left \
         running, never ended on a check that could not say",
        running.pid, running.addr
    ))
}

/// Whether `version` is older than the pinned SurrealDB, release by release (`3.10.0` after `3.9.1`); one
/// that does not read as a version is taken as older, so it is replaced.
fn older(version: &str) -> bool {
    let read = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|n| n.parse().ok()).collect() };
    match (read(version), read(SURREAL_VERSION)) {
        (Some(running), Some(pinned)) => running < pinned,
        _ => true,
    }
}

/// The address `command` binds, a command line as `ps` prints it (its arguments joined by spaces, so
/// a path may hold one), when it is a server of the board's `surreal` on `data`, as [`START`] runs
/// it: `SURREAL start [OPTIONS] --bind ADDR DATA`. Its program is `surreal` itself, run as itself or
/// by a script's interpreter (`/bin/sh SURREAL start …`, a script in its place), or a binary named
/// `surreal`, which a wrapper in its place runs in its own process (the tests' guard). The supervisor
/// of [`OWNED_START`] and its watchdog are none: their command is `sh -c SCRIPT SURREAL ADDR DATA
/// LOG`, its last argument the log and its `--bind` followed by the script's `"$1"`.
fn server_on(command: &str, surreal: &Path, data: &str) -> Option<String> {
    if !last_argument_is(command, data) {
        return None;
    }
    let rest = &command[..command.len() - data.len() - 1];
    let (rest, addr) = rest.rsplit_once(' ')?;
    addr.parse::<SocketAddr>().ok()?;
    let rest = rest.strip_suffix(" --bind")?;
    let surreal = surreal.to_str()?;
    let runs = |program: &str| {
        program == surreal
            || program
                .strip_suffix(surreal)
                .is_some_and(|interpreter| interpreter.ends_with(' '))
            || Path::new(program)
                .file_name()
                .is_some_and(|name| name == "surreal")
    };
    rest.match_indices(" start")
        .any(|(at, word)| {
            let options = &rest[at + word.len()..];
            (options.is_empty() || options.starts_with(' ')) && runs(&rest[..at])
        })
        .then(|| addr.to_string())
}

/// The common dir of the repository whose board `command` serves, a command line as `ps` prints it:
/// a server started as [`START`] starts one, `… start [OPTIONS] --bind ADDR surrealkv://DIR`, its
/// data `DIR` the board's own (`COMMON/symphony/tasks/data`) in a git common dir (one holding
/// `HEAD`). None for any other command line: a server on other data or in memory,
/// any other program, and a scratch board's supervisor shell, whose last argument is its log.
pub fn board_served(command: &str) -> Option<PathBuf> {
    if let Some((program, data)) = command.rsplit_once(" -D ")
        && Path::new(program)
            .file_name()
            .is_some_and(|name| name == "postgres")
    {
        let data = Path::new(data);
        if data.file_name()? != postgres::DATA {
            return None;
        }
        let common = common_of(data.parent()?)?;
        return common.join("HEAD").is_file().then(|| common.to_path_buf());
    }
    let at = command.rfind(" surrealkv://")?;
    let data = &command[at + 1..];
    let path = Path::new(data.strip_prefix("surrealkv://")?);
    if path.file_name()? != DATA {
        return None;
    }
    let common = common_of(path.parent()?)?;
    let (rest, addr) = command[..at].rsplit_once(' ')?;
    addr.parse::<SocketAddr>().ok()?;
    let rest = rest.strip_suffix(" --bind")?;
    if !(rest.contains(" start ") || rest.ends_with(" start")) {
        return None;
    }
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
