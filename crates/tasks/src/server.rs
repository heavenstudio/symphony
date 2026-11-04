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
use crate::sys::{Command, output, pid_alive, terminate};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The SurrealDB the board runs on: `tools/surreal.sh`'s `VERSION`, which fetches it.
pub const SURREAL_VERSION: &str = "3.3.0";
/// The board's folder in the git common dir.
pub const FOLDER: &str = "sf-tasks";

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
/// which a closed pane would send its whole session), with none of its starter's files, and reaped by
/// init, as `sh` ends at once.
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
            ServerError::NoSurreal(path) => write!(
                f,
                "SurrealDB {SURREAL_VERSION} is not at {}: it is fetched once per machine \
                 (docs/workflow/TASKS.md#where-it-lives)",
                path.display()
            ),
            ServerError::NotUp(why) | ServerError::Io(why) => f.write_str(why),
            ServerError::Reclaim(why) => write!(
                f,
                "the reclaim stopped and the board runs on its old store, untouched: {why}"
            ),
        }
    }
}

/// How the board's SurrealKV keeps what it is given, as the environment of every server this module
/// starts: SurrealDB 3.3 reads `SURREAL_SURREALKV_*` for its storage engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Storage {
    /// A value of more bytes than this goes apart from its key, to the value log. SurrealKV deletes a
    /// value-log file only once no sstable points into it, so every version of a row rewritten in
    /// place stays there for as long as one older value in that file is live: 102 GiB of log held
    /// a board of 0.5 GiB (tools/tasks-db-space). Kept beside their keys, in the sstables, the
    /// versions a rewrite leaves are dropped by compaction. A store with values already in the log
    /// still reads them.
    pub value_log_threshold: u64,
    /// How many bytes are written before they go to an sstable: the most the write-ahead log holds.
    /// One transaction must fit in it.
    pub memtable: u64,
}

impl Storage {
    /// The board's. Its largest value, a task's brief, is 1.7 MB, far below 64 MiB, and its export's
    /// largest statement 1.8 MB; SurrealKV's own memtable on a 64 GiB machine is 4 GiB.
    pub const BOARD: Storage = Storage {
        value_log_threshold: 64 << 20,
        memtable: 256 << 20,
    };

    fn environment(self) -> [(&'static str, String); 2] {
        [
            (
                "SURREAL_SURREALKV_VLOG_THRESHOLD",
                self.value_log_threshold.to_string(),
            ),
            (
                "SURREAL_SURREALKV_MAX_MEMTABLE_SIZE",
                self.memtable.to_string(),
            ),
        ]
    }
}

/// What [`Home::reclaim`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Reclaimed {
    /// The bytes the board's store took on disk before, and takes now.
    pub before: u64,
    pub after: u64,
    /// Every database carried, as `namespace/database`.
    pub databases: Vec<String>,
    /// Where the exports stay, one SurrealQL file a database.
    pub exports: PathBuf,
    /// The board's server, on the new store.
    pub running: Running,
}

/// A store's databases, and the users and accesses of its root and of each namespace, which a
/// database's export does not hold.
#[derive(Debug, PartialEq)]
struct Layout {
    databases: Vec<(String, String)>,
    definitions: BTreeMap<String, Value>,
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
        let pinned = || {
            common
                .join("sf-surreal")
                .join(SURREAL_VERSION)
                .join("surreal")
        };
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
        let out = output(Command::new("ps").args(["-ww", "-axo", "pid=,command="])).ok()?;
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

    /// Rebuild the board's store from an export of itself, which holds only what is live. Under the
    /// starters' lock: end the board's server, recorded or not; start one on the store that is
    /// recorded nowhere, and export every database it holds with SurrealDB's own `export`; import
    /// them into a new store, started with this board's [`Storage`]; export that one again and
    /// compare each export byte for byte, and the root's and every namespace's users and accesses;
    /// then swap the stores, start the board's server on the new one, and remove the old. Until the
    /// swap the old store is untouched, and a failure starts the board's server on it again. The
    /// exports stay in `reclaim/exports/`, until the next reclaim.
    pub fn reclaim(&self) -> Result<Reclaimed, ServerError> {
        if !self.surreal.is_file() {
            return Err(ServerError::NoSurreal(self.surreal.clone()));
        }
        let _held = self.lock()?;
        let work = self.dir.join(RECLAIM);
        let data = self.dir.join(DATA);
        self.finish_swap()?;
        let halted = self.halt()?;
        let before = bytes(&data);
        if work.exists() {
            fs::remove_dir_all(&work).map_err(io("could not remove", &work))?;
        }
        let (exports, check, fresh) = (work.join("exports"), work.join("check"), work.join(DATA));
        for folder in [&exports, &check] {
            fs::create_dir_all(folder).map_err(io("could not make", folder))?;
        }
        let databases = match self.rebuild(&data, &fresh, &exports, &check, &halted) {
            Ok(databases) => databases,
            Err(problem) => {
                let _ = fs::remove_dir_all(&fresh);
                return Err(self.serve_again(problem));
            }
        };
        let old = work.join(OLD);
        if let Err(problem) = fs::rename(&data, &old) {
            return Err(self.serve_again(ServerError::Reclaim(format!(
                "could not move {}: {problem}",
                data.display()
            ))));
        }
        let running = match fs::rename(&fresh, &data)
            .map_err(io("could not move", &fresh))
            .and_then(|()| self.start())
        {
            Ok(running) => running,
            Err(problem) => {
                if data.exists() {
                    fs::rename(&data, &fresh).map_err(io("could not move", &data))?;
                }
                fs::rename(&old, &data).map_err(io("could not move", &old))?;
                return Err(self.serve_again(ServerError::Reclaim(problem.to_string())));
            }
        };
        fs::remove_dir_all(&old).map_err(io("could not remove", &old))?;
        fs::remove_dir_all(&check).map_err(io("could not remove", &check))?;
        Ok(Reclaimed {
            before,
            after: bytes(&data),
            databases,
            exports,
            running,
        })
    }

    /// Why a reclaim stopped, the old store in place, once the board's server is started on that
    /// store again; with the start's own failure beside it, if it fails too.
    fn serve_again(&self, problem: ServerError) -> ServerError {
        let Err(again) = self.start() else {
            return problem;
        };
        let why = match problem {
            ServerError::Reclaim(why) => why,
            other => other.to_string(),
        };
        ServerError::NotUp(format!(
            "the reclaim stopped with the board's old store in place, untouched, and the board's \
             server did not start on it again: {again}. The reclaim stopped because: {why}"
        ))
    }

    /// What a reclaim cut short between its two moves left: the old store put back where the new
    /// one never arrived. One cut later leaves the old store beside the new, in the work folder the
    /// reclaim removes next. Under the lock.
    fn finish_swap(&self) -> Result<(), ServerError> {
        let (old, data) = (self.dir.join(RECLAIM).join(OLD), self.dir.join(DATA));
        if old.exists() && !data.exists() {
            fs::rename(&old, &data).map_err(io("could not move", &old))?;
        }
        Ok(())
    }

    /// End the board's server, recorded or found on its data, and forget its address: the ports
    /// they had, which a client may still hold as the board's. Under the lock.
    fn halt(&self) -> Result<Vec<u16>, ServerError> {
        let mut ports = Vec::new();
        if let Some(running) = self.recorded() {
            if self.serves(running.pid) {
                self.end(running.pid)?;
            }
            let address = self.dir.join(ADDRESS);
            fs::remove_file(&address).map_err(io("could not remove", &address))?;
            ports.push(running.addr);
        }
        if let Some(running) = self.unrecorded() {
            self.end(running.pid)?;
            ports.push(running.addr);
        }
        Ok(ports
            .iter()
            .filter_map(|addr| addr.parse::<SocketAddr>().ok())
            .map(|addr| addr.port())
            .collect())
    }

    /// Export every database of the store at `data` into `exports`, import each into a new store at
    /// `fresh`, and export that one into `check`: the databases, once every pair of exports and the
    /// two stores' layouts are the same. A store without the board's own database is refused. Each
    /// server is started on none of the `halted` ports, so a client still holding the board's old
    /// address writes into neither store, and is ended before this returns.
    fn rebuild(
        &self,
        data: &Path,
        fresh: &Path,
        exports: &Path,
        check: &Path,
        halted: &[u16],
    ) -> Result<Vec<String>, ServerError> {
        let file = |ns: &str, db: &str| format!("{ns}.{db}.surql");
        let old = self.spawn(&store(data), halted)?;
        let board = (crate::NAMESPACE.to_string(), crate::DATABASE.to_string());
        let read = layout(&old).and_then(|held| {
            if !held.databases.contains(&board) {
                let named: Vec<String> = held
                    .databases
                    .iter()
                    .map(|(ns, db)| format!("{ns}/{db}"))
                    .collect();
                return Err(ServerError::Reclaim(format!(
                    "the store at {} holds no {}/{}, the board's own database; it holds [{}]",
                    data.display(),
                    board.0,
                    board.1,
                    named.join(", ")
                )));
            }
            for (ns, db) in &held.databases {
                self.transfer("export", &old, ns, db, &exports.join(file(ns, db)))?;
            }
            Ok(held)
        });
        self.end(old.pid)?;
        let held = read?;
        let new = self.spawn(&store(fresh), halted)?;
        let built = (|| {
            for (ns, db) in &held.databases {
                self.transfer("import", &new, ns, db, &exports.join(file(ns, db)))?;
            }
            for (ns, db) in &held.databases {
                let (first, again) = (exports.join(file(ns, db)), check.join(file(ns, db)));
                self.transfer("export", &new, ns, db, &again)?;
                let read = |path: &Path| fs::read(path).map_err(io("could not read", path));
                if read(&first)? != read(&again)? {
                    return Err(ServerError::Reclaim(format!(
                        "{ns}/{db} exported from the new store is not its export from the old: {} against {}",
                        again.display(),
                        first.display()
                    )));
                }
            }
            let rebuilt = layout(&new)?;
            let missing: Vec<String> = held
                .databases
                .iter()
                .filter(|pair| !rebuilt.databases.contains(pair))
                .map(|(ns, db)| format!("{ns}/{db}"))
                .collect();
            if !missing.is_empty() {
                return Err(ServerError::Reclaim(format!(
                    "the new store lacks {}",
                    missing.join(", ")
                )));
            }
            for (scope, defined) in &held.definitions {
                if rebuilt.definitions.get(scope) != Some(defined) {
                    return Err(ServerError::Reclaim(format!(
                        "the users and accesses of {} differ in the new store: {defined} against {}",
                        if scope.is_empty() { "the root" } else { scope },
                        rebuilt.definitions.get(scope).unwrap_or(&Value::Null)
                    )));
                }
            }
            Ok(())
        })();
        self.end(new.pid)?;
        built.map(|()| {
            held.databases
                .iter()
                .map(|(ns, db)| format!("{ns}/{db}"))
                .collect()
        })
    }

    /// SurrealDB's own `export` or `import` (`verb`) of database `ns`/`db` on `running`, to or from
    /// `file`.
    fn transfer(
        &self,
        verb: &str,
        running: &Running,
        ns: &str,
        db: &str,
        file: &Path,
    ) -> Result<(), ServerError> {
        let endpoint = format!("http://{}", running.addr);
        let out = output(
            Command::new(&self.surreal)
                .args([
                    verb,
                    "--endpoint",
                    &endpoint,
                    "--namespace",
                    ns,
                    "--database",
                    db,
                ])
                .arg(file),
        )
        .map_err(io(&format!("could not run {verb} with"), &self.surreal))?;
        if out.status.success() {
            return Ok(());
        }
        Err(ServerError::Reclaim(format!(
            "{verb} of {ns}/{db} at {} ({}) failed: {}",
            running.addr,
            file.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
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
        let Ok(out) =
            output(Command::new("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]))
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
    /// moved aside and never replaced is refused, never started anew and empty in its place.
    fn start(&self) -> Result<Running, ServerError> {
        let old = self.dir.join(RECLAIM).join(OLD);
        if !self.dir.join(DATA).exists() && old.exists() {
            return Err(ServerError::Io(format!(
                "the board's store is at {}: a reclaim moved it there and stopped; \
                 `just tasks-reclaim` puts it back",
                old.display()
            )));
        }
        let running = self.spawn(&self.data(), &[])?;
        self.record(&running)?;
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
                Command::new("/bin/sh")
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

/// A store at `path`, as a server's command line names it.
fn store(path: &Path) -> String {
    format!("surrealkv://{}", path.display())
}

/// The bytes `path` takes on disk, every file in it counted by the blocks it holds, as `du` counts
/// them: what a reclaim frees, measured. A clone's shared blocks count in each copy.
pub fn bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    let own = meta.blocks() * 512;
    if !meta.is_dir() {
        return own;
    }
    fs::read_dir(path).map_or(own, |entries| {
        own + entries
            .filter_map(Result::ok)
            .map(|entry| bytes(&entry.path()))
            .sum::<u64>()
    })
}

/// The databases the server `running` holds, sorted, with the users and accesses of its root (the
/// empty scope) and of each namespace, refused as [`names`] refuses an answer.
fn layout(running: &Running) -> Result<Layout, ServerError> {
    let client = rpc::Client::new(&running.addr, crate::NAMESPACE, crate::DATABASE);
    let ask = |sql: &str| {
        client.unscoped(sql, json!({})).map_err(|problem| {
            ServerError::Reclaim(format!(
                "could not read {sql:?} at {}: {problem}",
                running.addr
            ))
        })
    };
    let defined = |info: &Value| json!({"users": info["users"], "accesses": info["accesses"]});
    let root = ask("INFO FOR ROOT;")?.pop().unwrap_or(Value::Null);
    let mut layout = Layout {
        databases: Vec::new(),
        definitions: BTreeMap::from([(String::new(), defined(&root))]),
    };
    for ns in names(&root, "namespaces", "the root", &running.addr)? {
        let info = ask(&format!("USE NS {ns}; INFO FOR NS;"))?
            .pop()
            .unwrap_or(Value::Null);
        for db in names(&info, "databases", &ns, &running.addr)? {
            layout.databases.push((ns.clone(), db));
        }
        layout.definitions.insert(ns, defined(&info));
    }
    layout.databases.sort();
    Ok(layout)
}

/// The names under `field` of `info`, the `INFO` answer for `scope` at `addr`. An answer without
/// that object is refused, never read as none: a reclaim would carry nothing and remove the store.
/// So is a name the board's RPC could not say.
fn names(info: &Value, field: &str, scope: &str, addr: &str) -> Result<Vec<String>, ServerError> {
    let Some(named) = info.get(field).and_then(Value::as_object) else {
        return Err(ServerError::Reclaim(format!(
            "INFO for {scope} at {addr} answered no {field} object: {info}"
        )));
    };
    match named.keys().find(|name| !rpc::plain(name)) {
        Some(name) => Err(ServerError::Reclaim(format!(
            "{name:?} at {addr} is not a plain name for a namespace or database"
        ))),
        None => Ok(named.keys().cloned().collect()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(info: &Value, field: &str) -> String {
        names(info, field, "the root", "127.0.0.1:1")
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn an_info_answer_without_its_names_is_refused_never_read_as_none() {
        for info in [
            json!({"users": {}, "accesses": {}}),
            json!({"namespaces": null}),
            json!({"namespaces": ["sf"]}),
            Value::Null,
        ] {
            let said = refusal(&info, "namespaces");
            assert!(
                said.contains("INFO for the root at 127.0.0.1:1 answered no namespaces object"),
                "{said}"
            );
        }
    }

    #[test]
    fn an_info_answer_names_what_it_holds_and_refuses_a_name_the_board_cannot_say() {
        let info = json!({"databases": {"tasks": "DEFINE …", "scratch_board": "DEFINE …"}});
        let mut held = names(&info, "databases", "sf", "127.0.0.1:1").unwrap();
        held.sort();
        assert_eq!(held, ["scratch_board", "tasks"]);
        let empty = names(&json!({"databases": {}}), "databases", "sf", "127.0.0.1:1");
        assert_eq!(empty.unwrap(), Vec::<String>::new());
        let said = refusal(
            &json!({"namespaces": {"sf": "", "not-plain": ""}}),
            "namespaces",
        );
        assert!(
            said.contains("\"not-plain\" at 127.0.0.1:1 is not a plain name"),
            "{said}"
        );
    }
}
