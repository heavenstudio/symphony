//! The board's own SurrealDB server (slice 3 of `tools/docs-app-rust-port`; the owner, card
//! tasks-store-engine: "server should start the db").
//!
//! It keeps its data on disk in `sf-tasks/` in the git common dir, so every worktree reaches the one
//! board, and it is found by the address it wrote there. Whatever needs the board starts it when it is
//! not running ([`Home::ensure`]), under a lock so callers at once start one, detached so it outlives
//! the caller. That is also the restart: a server that was killed is started again on the same data. A
//! recorded server older than the pinned SurrealDB is replaced; a newer one, a tree ahead of this one,
//! is used as it is.
//!
//! Every server it starts keeps the board's values in its sstables ([`Storage`]), where compaction
//! reclaims what a rewrite leaves, and [`Home::reclaim`] rebuilds a store that grew before it did.
//!
//! A server is never killed. It is asked to end (SIGTERM) and waited for while it writes what its
//! memtable holds to an sstable and closes its store ([`Home::end`]); one that has not answered yet
//! is still starting, replaying its log, and is never signalled at all, nor given up on: a server
//! that does not come up within the patience is left running and the caller told so, and how to
//! tell a replay from a hang. A start on a store whose log may not fit the board's memtable (one an
//! end without a clean close left, written under a larger one) keeps the store as it is first
//! ([`Home::keep`]), then replays the log in a private server whose memtable holds it, and stops
//! that one cleanly ([`Home::replay`]). While the board is held ([`Home::hold`]) nothing starts it.
//! A caller that waits on another's start says what it waits on ([`Home::lock`]).
//!
//! A board kept in a scratch place ([`scratch`]: under the temporary folder or a checkout's
//! `artifacts/`) is a fixture's: its server is not detached but supervised ([`OWNED_START`]), and
//! ends when the process that started it ends, however it ends.

use crate::port::{self, NotStarted, Start};
use crate::rpc;
use crate::sys::{Stdio, command, output, pid_alive, terminate, wait_output};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(test)]
mod fetch_tests;
mod hold;
mod kept;
mod reclaim;
mod scratch;
mod starters;
mod storage;

pub use hold::{Hold, Holding};
pub use kept::{KEPT, Kept};
pub use reclaim::{Reclaimed, bytes};
use scratch::Lifeline;
pub use scratch::scratch;
use starters::{Doing, Starters};
pub use storage::Storage;
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

/// [`surreal_fetch`] of SurrealDB's build `build`, whose tarball's SHA-256 is `sha`, on `os`.
fn fetch_line(surreal: &Path, build: &str, sha: &str, os: &str) -> Option<String> {
    let folder = surreal.parent()?;
    let quote = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
    let tarball = folder.join(format!("surreal-v{SURREAL_VERSION}.{build}.tgz"));
    let check = if os == "macos" {
        "shasum -a 256 -c"
    } else {
        "sha256sum -c"
    };
    // In the same folder as `surreal`, so the move is a rename.
    let unpack = folder.join(format!(".surreal-v{SURREAL_VERSION}.unpack"));
    Some(format!(
        "mkdir -p {folder} && curl -fsSLo {tgz} \
         https://download.surrealdb.com/v{SURREAL_VERSION}/surreal-v{SURREAL_VERSION}.{build}.tgz \
         && printf '%s  %s\\n' {sha} {tgz} | {check} && mkdir -p {unpack} && tar -xzf {tgz} -C \
         {unpack} surreal && mv -f {unpacked} {surreal}; s=$?; rm -f {tgz} {unpacked}; rmdir \
         {unpack} 2>/dev/null; [ $s -eq 0 ]",
        folder = quote(folder),
        tgz = quote(&tarball),
        unpack = quote(&unpack),
        unpacked = quote(&unpack.join("surreal")),
        surreal = quote(surreal),
    ))
}
/// The board's folder in the git common dir.
pub const FOLDER: &str = "sf-tasks";
/// Where the pinned `surreal` is kept, in the git common dir: where `tools/surreal.sh` fetches it,
/// and [`surreal_fetch`] says how to.
const PINNED: &str = "sf-surreal";

/// In the board's folder: where the server answers, the lock its starters take, its log, its data.
const ADDRESS: &str = "server.json";
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
/// ends. The supervisor ends with the server and says its pid, as `START` does.
const OWNED_START: &str = r#"exec 3<&0 </dev/null
(
    "$0" start --unauthenticated --no-banner --log warn --bind "$1" "$2" >>"$3" 2>&1 3<&- &
    server=$!
    echo "$server"
    exec >/dev/null 2>&1
    ( while IFS= read -r line; do :; done <&3; kill "$server" 2>/dev/null ) &
    watchdog=$!
    exec 3<&-
    wait "$server"
    kill "$watchdog" 2>/dev/null
) &"#;

/// Why the board's server cannot be had.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    /// The pinned `surreal` is not where it is fetched to, and the board has no store: a project
    /// with no board on this machine.
    NoSurreal(PathBuf),
    /// The board's store is there and the `surreal` that serves it is not: a board that is unwell,
    /// as after a SurrealDB version bump before the new binary is fetched, or with `SF_SURREAL`
    /// naming a file that is not there.
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
/// or the `SF_SURREAL` that named it.
fn how_to_have(f: &mut fmt::Formatter<'_>, path: &Path) -> fmt::Result {
    if pinned_in(path).is_none() {
        return f.write_str(", where `SF_SURREAL` names it");
    }
    match surreal_fetch(path, std::env::consts::OS, std::env::consts::ARCH) {
        Some(fetch) => write!(f, ": fetch it there (`{fetch}`)")?,
        None => f.write_str(": put its release binary for this machine there")?,
    }
    f.write_str(", or name another with `SF_SURREAL`")
}

/// The common dir `surreal` is the pinned binary of ([`Home::in_common`]): `<common>/sf-surreal/
/// <SURREAL_VERSION>/surreal`; none for a binary anywhere else, one `SF_SURREAL` names.
fn pinned_in(surreal: &Path) -> Option<&Path> {
    let version = surreal.parent()?;
    let folder = version.parent()?;
    (surreal.file_name()? == "surreal"
        && version.file_name()? == SURREAL_VERSION
        && folder.file_name()? == PINNED)
        .then_some(folder.parent()?)
}

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
pub struct Home {
    dir: PathBuf,
    surreal: PathBuf,
    patience: Duration,
    closing: Duration,
    storage: Storage,
    /// Whether its servers are supervised; by its place ([`scratch`]) unless a test says.
    supervised: Option<bool>,
    tell: Tell,
    /// How long a caller waits on the starters' lock before it is refused; none: until it is free.
    lock_wait: Option<Duration>,
}

impl Home {
    /// The board in `dir`, served by the `surreal` binary at `surreal`.
    pub fn new(dir: &Path, surreal: &Path) -> Home {
        Home {
            dir: dir.into(),
            surreal: surreal.into(),
            patience: PATIENCE,
            closing: CLOSING,
            storage: Storage::BOARD,
            supervised: None,
            tell: std::sync::Arc::new(|line| eprintln!("{line}")),
            lock_wait: None,
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

    /// The board of the repository whose common dir is `common`: `sf-tasks/` there, served by the
    /// pinned `surreal` beside it ([`surreal_fetch`]), or by `SF_SURREAL`'s.
    pub fn in_common(common: &Path, sf_surreal: Option<&Path>) -> Home {
        let pinned = || common.join(PINNED).join(SURREAL_VERSION).join("surreal");
        Home::new(
            &common.join(FOLDER),
            &sf_surreal.map_or_else(pinned, Path::to_path_buf),
        )
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

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn surreal(&self) -> &Path {
        &self.surreal
    }

    /// What the address file says, without asking the server: the fast path, which goes to
    /// [`Home::ensure`] when the server does not answer.
    pub fn recorded(&self) -> Option<Running> {
        let text = fs::read_to_string(self.dir.join(ADDRESS)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The board's server, answering: the recorded one, else one started now. The recorded one counts only
    /// if its process serves this board's data (an address file can name any server that answers), and
    /// one older than the pinned version is replaced; a newer one is used as it is, so that two trees
    /// pinning two versions never end each other's server. An answer on a server's address, recorded or
    /// found, counts only from that process's own listener there ([`port::listens`]): a process that
    /// took its port answers too. A recorded server that ends while it is waited for is started again.
    /// One that runs and does not answer within the patience, recorded or not, is left running,
    /// never ended nor joined by a second: it is still replaying its log, or stuck, and the refusal
    /// says so. Refused while the board is held ([`Home::hold`]).
    pub fn ensure(&self) -> Result<Running, ServerError> {
        self.unheld()?;
        if let Some(running) = self.recorded()
            && !older(&running.version)
            && self.serves(running.pid)
            && rpc::healthy(&running.addr)
            && owns(&running).unwrap_or(false)
        {
            return Ok(running);
        }
        let starters = self.lock(Doing::Starting)?;
        self.unheld()?;
        if let Some(running) = self.recorded()
            && self.serves(running.pid)
        {
            match self.answers(&running)? {
                Waited::Answers if older(&running.version) => self.end(running.pid)?,
                Waited::Answers => return Ok(running),
                Waited::Ended => {}
                Waited::Silent => return Err(self.silent(&running)),
            }
        }
        if let Some(running) = self.unrecorded() {
            match self.answers(&running)? {
                Waited::Answers => {
                    self.record(&running)?;
                    return Ok(running);
                }
                Waited::Ended => {}
                Waited::Silent => return Err(self.silent(&running)),
            }
        }
        self.start(&starters)
    }

    /// Why a server on this board's data that runs and has not answered within the patience is
    /// not the board's yet, and what becomes of it.
    fn silent(&self, running: &Running) -> ServerError {
        ServerError::NotUp(format!(
            "the board's server (pid {}, {}) runs and does not answer within {} s: it is still \
             starting (a store replaying a long log takes minutes) or stuck, and is left running, \
             never ended nor joined by a second on its data; the next command waits for it again. \
             {}. Its log is {}",
            running.pid,
            running.addr,
            self.patience.as_secs(),
            self.unanswered(running.pid),
            self.dir.join(LOG).display()
        ))
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

    /// A server on this board's data that the address file does not name (its starter was killed between
    /// its answer and its record): it holds the data's lock, so no other could start. Only a process
    /// running the board's `surreal` counts ([`server_on`]), never the supervisor shell of a scratch
    /// board's server or its watchdog, which keep the server's words among their own arguments. Its
    /// address is the one its command line binds; its version is taken as the pinned one, which
    /// started it.
    fn unrecorded(&self) -> Option<Running> {
        let out = output(command("ps").args(["-ww", "-axo", "pid=,command="])).ok()?;
        let data = self.data();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let (pid, line) = line.trim().split_once(char::is_whitespace)?;
                let addr = server_on(line.trim_start(), &self.surreal, &data)?;
                Some(Running {
                    addr,
                    pid: pid.parse().ok()?,
                    version: SURREAL_VERSION.into(),
                })
            })
    }

    /// Stop the recorded server cleanly, if it runs, and forget its address: what it was, if
    /// anything. One still starting is waited for first, never signalled before it answers
    /// ([`Home::stop_one`]). Refused while the board is held, which stopped it already.
    pub fn stop(&self) -> Result<Option<Running>, ServerError> {
        let _held = self.lock(Doing::Stopping)?;
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
    fn serves(&self, pid: i64) -> bool {
        if !pid_alive(pid) {
            return false;
        }
        let Ok(out) = output(command("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]))
        else {
            return false;
        };
        last_argument_is(String::from_utf8_lossy(&out.stdout).trim(), &self.data())
    }

    /// What the server `running` names comes to within the patience: it answers on its address from
    /// a listener of its own ([`owns`]), it ends, or it runs and does neither.
    fn answers(&self, running: &Running) -> Result<Waited, ServerError> {
        let began = Instant::now();
        loop {
            if rpc::healthy(&running.addr) && owns(running)? {
                return Ok(Waited::Answers);
            }
            if !pid_alive(running.pid) {
                return Ok(Waited::Ended);
            }
            if began.elapsed() >= self.patience {
                return Ok(Waited::Silent);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Ask `pid`, a server that answers, to end (SIGTERM) and wait while it closes: SurrealDB writes
    /// what its memtable holds to an sstable and empties its log before it ends. It is never made
    /// to end: a second signal does nothing once its handler is set, and a kill would leave its log
    /// for the next start to replay. One still running after the closing patience is left to finish,
    /// and the stop said to be under way. Only a server that answers is asked: one still starting
    /// has no handler yet, and the signal would end it at once, as a kill does.
    fn end(&self, pid: i64) -> Result<(), ServerError> {
        terminate(pid, false);
        let began = Instant::now();
        while began.elapsed() < self.closing {
            if !pid_alive(pid) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(ServerError::NotUp(format!(
            "the board's server (pid {pid}) is still closing its store {} s after it was asked to \
             end: it is left to finish, never killed; its log is {}",
            self.closing.as_secs(),
            self.dir.join(LOG).display()
        )))
    }

    /// Start the board's server ([`Home::spawn`]) and record it. Under the lock. A store a reclaim
    /// moved aside and never replaced is refused, never started anew and empty in its place. A
    /// server whose record fails is ended before the failure is said: left running, nobody's, the
    /// next [`Home::ensure`] would adopt it ([`Home::unrecorded`]) on whatever store it holds, one a
    /// reclaim's rollback has since moved aside among them. Refused while the board is held. A log
    /// that may not fit the board's memtable is replayed first ([`Home::replay`]), `starters` saying
    /// so to whoever waits on them.
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
        self.replay(starters)?;
        let running = self.spawn(&self.data(), &[])?;
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
    /// not come up by then is left running, unrecorded, and adopted by a later
    /// [`Home::ensure`] once it answers. Under the lock, `starters` saying to whoever waits on them
    /// that a replay runs, of how large a log.
    ///
    /// Before its server starts, the store is kept as it is ([`Home::keep`]): a store that went
    /// back holds a log segment no server applies, and the replay's clean stop removes it. A store
    /// that can be neither cloned nor copied is not replayed, and the start refused. Where the
    /// store is kept is told ([`Home::telling`]).
    fn replay(&self, starters: &Starters) -> Result<(), ServerError> {
        let data = self.dir.join(DATA);
        let logged = storage::logged(&data);
        let Some(storage) = self.storage.replaying(logged) else {
            return Ok(());
        };
        starters.doing(Doing::Replaying, Some(logged));
        let kept = self.keep(crate::store::now())?;
        (self.tell)(&format!(
            "· the log of the board's store at {} holds {}, more than the board's memtable takes: \
             it is replayed whole in a private server before the board's server starts (a log of \
             gigabytes takes minutes). The store as it was is kept at {}, {}: on a store that went \
             back, it alone keeps the log segment no server applies and the replay's stop \
             removes. The board never removes it: remove it once the board reads as it should",
            data.display(),
            crate::size(logged),
            kept.path.display(),
            if kept.cloned {
                "a copy-on-write clone"
            } else {
                "a plain copy, as this volume cannot clone"
            }
        ));
        let except: Vec<u16> = self
            .recorded()
            .and_then(|running| running.addr.parse::<SocketAddr>().ok())
            .map(|addr| addr.port())
            .into_iter()
            .collect();
        let replayed = self.spawn_as(
            &self.data(),
            &except,
            storage,
            self.patience.saturating_mul(REPLAYING),
            "the private replay's server",
        )?;
        self.end(replayed.pid)?;
        starters.doing(Doing::Starting, None);
        (self.tell)(
            "· the private replay is through, and its server closed the store cleanly: the \
             board's server starts",
        );
        Ok(())
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
        )
    }

    /// Start a server on `data` on a free port, none of `except`, with `storage`, and wait up to
    /// `patience` until it answers. SurrealDB is told a reserved port to bind; one that ends saying
    /// the port was taken meanwhile is started again on another ([`port`]), every start within the
    /// one patience. It is up once it answers and holds the port's listener itself
    /// ([`port::listens`]): a process that took its port answers too, while this server is still on
    /// its way to ending. One that has not answered by then is left running, never signalled: it is
    /// still replaying its log, and a signal before its handlers are set ends it at once. `what`
    /// names the server in what is said of it.
    fn spawn_as(
        &self,
        data: &str,
        except: &[u16],
        storage: Storage,
        patience: Duration,
        what: &str,
    ) -> Result<Running, ServerError> {
        self.unheld()?;
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
            loop {
                if rpc::healthy(&addr) {
                    match port::listens(child, reserved, output) {
                        Ok(true) => {
                            return Ok(Start::Up(Running {
                                addr,
                                pid,
                                version: SURREAL_VERSION.into(),
                            }));
                        }
                        Ok(false) => {}
                        Err(problem) => {
                            self.end(pid)?;
                            return Err(ServerError::NotUp(format!(
                                "{what} (pid {pid}, {addr}) answered, and {problem}; \
                                 its log is {}",
                                log.display()
                            )));
                        }
                    }
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
#[derive(Debug, Clone, Copy, PartialEq)]
enum Waited {
    /// It answers on its address from a listener of its own.
    Answers,
    /// Its process ended.
    Ended,
    /// It runs and does not answer from a listener of its own.
    Silent,
}

/// Whether the process `running` names holds the listener on its address itself
/// ([`port::listens`]): a health answer there is that server's only then. A record whose pid or
/// address cannot be one, or an `lsof` that cannot say, is the error.
fn owns(running: &Running) -> Result<bool, ServerError> {
    let failed = |why: &dyn fmt::Display| {
        ServerError::NotUp(format!(
            "could not tell whether the board's server (pid {}, {}) listens there: {why}",
            running.pid, running.addr
        ))
    };
    let pid = u32::try_from(running.pid).map_err(|problem| failed(&problem))?;
    let addr: SocketAddr = running.addr.parse().map_err(|problem| failed(&problem))?;
    port::listens(pid, addr, output).map_err(|problem| failed(&problem))
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
/// data `DIR` the board's own (`COMMON/sf-tasks/data`) in a git common dir (one holding `HEAD`).
/// None for any other command line: a server on other data or in memory, any other program, and a
/// scratch board's supervisor shell, whose last argument is its log.
pub fn board_served(command: &str) -> Option<PathBuf> {
    let at = command.rfind(" surrealkv://")?;
    let data = &command[at + 1..];
    let path = Path::new(data.strip_prefix("surrealkv://")?);
    let folder = path.parent()?;
    if path.file_name()? != DATA || folder.file_name()? != FOLDER {
        return None;
    }
    let (rest, addr) = command[..at].rsplit_once(' ')?;
    addr.parse::<SocketAddr>().ok()?;
    let rest = rest.strip_suffix(" --bind")?;
    if !(rest.contains(" start ") || rest.ends_with(" start")) {
        return None;
    }
    let common = folder.parent()?;
    common.join("HEAD").is_file().then(|| common.to_path_buf())
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
