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

use crate::port::{self, NotStarted, Start};
use crate::rpc;
use crate::sys::{command, output, pid_alive, terminate};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod reclaim;
mod storage;

pub use reclaim::{Reclaimed, bytes};
pub use storage::Storage;
use storage::store;

/// The SurrealDB the board runs on: `tools/surreal.sh`'s `VERSION`, which fetches it.
pub const SURREAL_VERSION: &str = "3.3.0";
/// The board's folder in the git common dir.
pub const FOLDER: &str = "sf-tasks";
/// Where `tools/surreal.sh` keeps the pinned `surreal`, in the git common dir.
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
/// How long a server asked to end is given before it is made to.
const GRACE: Duration = Duration::from_secs(20);
/// The server starts detached: in a session of its own with no terminal (SurrealDB ends on a hangup,
/// which a closed pane would send its whole session), with none of its starter's files nor its
/// starter's agent session and hc terminal ([`crate::sys::SESSION_VARS`]), and reaped by init, as
/// `sh` ends at once.
const START: &str = r#""$0" start --unauthenticated --no-banner --log warn --bind "$1" "$2" >>"$3" 2>&1 </dev/null & echo $!"#;

/// Why the board's server cannot be had.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    /// The pinned `surreal` is not where it is fetched to.
    NoSurreal(PathBuf),
    /// It was started and did not come up, or it runs and does not answer: the reason and its log.
    NotUp(String),
    /// The board's folder could not be read or written.
    Io(String),
    /// A reclaim stopped before its swap ([`Home::reclaim`]): what failed. The board's server runs on
    /// the old store again, untouched.
    Reclaim(String),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::NoSurreal(path) => match pinned_in(path) {
                Some(common) => write!(
                    f,
                    "SurrealDB {SURREAL_VERSION} is not at {}: `tools/surreal.sh` fetches it into \
                     this repository's common dir (`SF_GIT_COMMON_DIR={} bash tools/surreal.sh` \
                     in a Symphony checkout), or `SF_SURREAL` names another",
                    path.display(),
                    common.display()
                ),
                None => write!(
                    f,
                    "SurrealDB {SURREAL_VERSION} is not at {}, where `SF_SURREAL` names it",
                    path.display()
                ),
            },
            ServerError::NotUp(why) | ServerError::Io(why) => f.write_str(why),
            ServerError::Reclaim(why) => write!(
                f,
                "the reclaim stopped and the board runs on its old store, untouched: {why}"
            ),
        }
    }
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

/// Where the board's server keeps its data, its address, its lock and its log.
pub struct Home {
    dir: PathBuf,
    surreal: PathBuf,
    patience: Duration,
    storage: Storage,
}

impl Home {
    /// The board in `dir`, served by the `surreal` binary at `surreal`.
    pub fn new(dir: &Path, surreal: &Path) -> Home {
        Home {
            dir: dir.into(),
            surreal: surreal.into(),
            patience: PATIENCE,
            storage: Storage::BOARD,
        }
    }

    /// The board of the repository whose common dir is `common`: `sf-tasks/` there, served by the
    /// pinned `surreal` that `tools/surreal.sh` fetches beside it, or by `SF_SURREAL`'s.
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

    /// How the servers this board starts keep their data: [`Storage::BOARD`] unless a test asks
    /// for a memtable small enough to flush, or for the storage a store grew under.
    pub fn storage(self, storage: Storage) -> Home {
        Home { storage, ..self }
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
    pub fn ensure(&self) -> Result<Running, ServerError> {
        if let Some(running) = self.recorded()
            && !older(&running.version)
            && self.serves(running.pid)
            && rpc::healthy(&running.addr)
            && owns(&running).unwrap_or(false)
        {
            return Ok(running);
        }
        let _held = self.lock()?;
        if let Some(running) = self.recorded()
            && self.serves(running.pid)
        {
            if older(&running.version) {
                self.end(running.pid)?;
            } else {
                match self.answers(&running)? {
                    Waited::Answers => return Ok(running),
                    Waited::Ended => {}
                    Waited::Silent => {
                        return Err(ServerError::NotUp(format!(
                            "the board's server (pid {}, {}) runs and does not answer; its log is {}",
                            running.pid,
                            running.addr,
                            self.dir.join(LOG).display()
                        )));
                    }
                }
            }
        }
        if let Some(running) = self.unrecorded() {
            if self.answers(&running)? == Waited::Answers {
                self.record(&running)?;
                return Ok(running);
            }
            self.end(running.pid)?;
        }
        self.start()
    }

    /// A server on this board's data that the address file does not name (its starter was killed between
    /// its answer and its record): it holds the data's lock, so no other could start. Its address is the
    /// one its command line binds; its version is taken as the pinned one, which started it.
    fn unrecorded(&self) -> Option<Running> {
        let out = output(command("ps").args(["-ww", "-axo", "pid=,command="])).ok()?;
        let data = self.data();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                let pid: i64 = words.next()?.parse().ok()?;
                let words: Vec<&str> = words.collect();
                if !words.contains(&"start") || !words.contains(&data.as_str()) {
                    return None;
                }
                let at = words.iter().position(|w| *w == "--bind")?;
                Some(Running {
                    addr: (*words.get(at + 1)?).to_string(),
                    pid,
                    version: SURREAL_VERSION.into(),
                })
            })
    }

    /// Stop the recorded server, if it runs, and forget its address: what it was, if anything.
    pub fn stop(&self) -> Result<Option<Running>, ServerError> {
        let _held = self.lock()?;
        let Some(running) = self.recorded() else {
            return Ok(None);
        };
        if self.serves(running.pid) {
            self.end(running.pid)?;
        }
        let address = self.dir.join(ADDRESS);
        fs::remove_file(&address).map_err(io("could not remove", &address))?;
        Ok(Some(running))
    }

    /// Every starter and stopper of this board, one at a time.
    fn lock(&self) -> Result<crate::sys::FileLock, ServerError> {
        fs::create_dir_all(&self.dir).map_err(io("could not make", &self.dir))?;
        let path = self.dir.join(LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io("could not open", &path))?;
        lock.lock().map_err(io("could not lock", &path))?;
        Ok(crate::sys::FileLock(lock))
    }

    /// The data a server of this board is started on, as its command line names it.
    fn data(&self) -> String {
        store(&self.dir.join(DATA))
    }

    /// Whether `pid` is this board's server: running, and on this board's data. A pid recorded once
    /// may since have gone to any other process, which is never signalled.
    fn serves(&self, pid: i64) -> bool {
        if !pid_alive(pid) {
            return false;
        }
        let Ok(out) = output(command("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]))
        else {
            return false;
        };
        String::from_utf8_lossy(&out.stdout).contains(&self.data())
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

    /// Ask `pid` to end, then make it; it is gone when this returns.
    fn end(&self, pid: i64) -> Result<(), ServerError> {
        for (hard, wait) in [(false, GRACE), (true, GRACE)] {
            terminate(pid, hard);
            let began = Instant::now();
            while began.elapsed() < wait {
                if !pid_alive(pid) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(ServerError::NotUp(format!(
            "the board's server (pid {pid}) did not end when told to"
        )))
    }

    /// Start the board's server ([`Home::spawn`]) and record it. Under the lock. A store a reclaim
    /// moved aside and never replaced is refused, never started anew and empty in its place. A
    /// server whose record fails is ended before the failure is said: left running, nobody's, the
    /// next [`Home::ensure`] would adopt it ([`Home::unrecorded`]) on whatever store it holds, one a
    /// reclaim's rollback has since moved aside among them.
    fn start(&self) -> Result<Running, ServerError> {
        let old = self.dir.join(RECLAIM).join(OLD);
        if !self.dir.join(DATA).exists() && old.exists() {
            return Err(ServerError::Io(format!(
                "the board's store is at {}: a reclaim moved it there and stopped; \
                 `tasks-reclaim` puts it back",
                old.display()
            )));
        }
        let running = self.spawn(&self.data(), &[])?;
        if let Err(problem) = self.record(&running) {
            return Err(match self.end(running.pid) {
                Ok(()) => problem,
                Err(again) => ServerError::NotUp(format!("{problem}; and then {again}")),
            });
        }
        Ok(running)
    }

    /// Start a server on `data` on a free port, none of `except`, with this board's [`Storage`], and
    /// wait until it answers. SurrealDB is told a reserved port to bind; one that ends saying the port was taken
    /// meanwhile is started again on another ([`port`]), every start within the one patience. It is
    /// up once it answers and holds the port's listener itself ([`port::listens`]): a process that
    /// took its port answers too, while this server is still on its way to ending.
    fn spawn(&self, data: &str, except: &[u16]) -> Result<Running, ServerError> {
        if !self.surreal.is_file() {
            return Err(ServerError::NoSurreal(self.surreal.clone()));
        }
        let log = self.dir.join(LOG);
        let until = Instant::now() + self.patience;
        let started = port::start_on_free_port_except(except, port::ATTEMPTS, until, |reserved| {
            let addr = reserved.to_string();
            let from = fs::metadata(&log).map_or(0, |meta| meta.len());
            let out = output(
                command("/bin/sh")
                    .new_session()
                    .envs(self.storage.environment())
                    .arg("-c")
                    .arg(START)
                    .arg(&self.surreal)
                    .arg(&addr)
                    .arg(data)
                    .arg(&log),
            )
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
                                "the board's server (pid {pid}, {addr}) answered, and {problem}; \
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
                    self.end(pid)?;
                    return Err(ServerError::NotUp(format!(
                        "the board's server (pid {pid}, {addr}) did not answer within {} s; its log is {}",
                        self.patience.as_secs(),
                        log.display()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let not_started = |said: &str| {
            ServerError::NotUp(format!(
                "the board's server did not start: {}; its log is {}",
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
