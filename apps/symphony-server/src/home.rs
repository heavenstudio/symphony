//! The one Symphony server of the machine, and the folder it is found by: the machine folder
//! (`symphony::machine::dir`), which every project and every worktree shares (the shape of the
//! board's server, `Home` in `symphony/crates/tasks/src/server.rs`). The server's build and client
//! stay in the project's `sf-symphony/` in its git common dir, its install folder, and a record a
//! server before the machine folder left there is read once, to find that server.
//!
//! It holds the server's record (its port and pid), the lock its starters take, and its log. A start
//! that finds the recorded server answering prints it and starts nothing ([`Home::ensure`]); one that
//! does not takes the lock, looks again, and starts it, detached, so it outlives the starter: callers
//! at once start one. That is also the restart: a server that was killed is started again, on the
//! same port. A record that lies (garbage, a dead pid, a port the server is not on) is mended from the
//! server's own command line, or replaced by the server it starts. `--stop` ends it ([`Home::stop`]).

use crate::probe;
use crate::sys::{pid_alive, signal};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use symphony_board::sys::{Child, Command, Stdio};

mod discovery;

/// The project's install folder in its git common dir: the server's build and its client, and
/// the record of a server started before the machine folder.
pub const FOLDER: &str = "sf-symphony";
/// The app's port: the owner's bookmarks, and the phone's.
pub const DEFAULT_PORT: u16 = 8765;
/// In the folder: where the server answers, the lock its starters take, its log.
pub const ADDRESS: &str = "server.json";
const LOCK: &str = "lock";
pub const LOG: &str = "server.log";
/// How long a server is waited for to come up or to answer, on a machine as loaded as this one gets.
const PATIENCE: Duration = Duration::from_secs(60);
/// How long a server asked to end is given before it is made to.
const GRACE: Duration = Duration::from_secs(20);
/// How long one look at a server may take.
const LOOK: Duration = Duration::from_secs(2);
/// How long one read of the process table may take; a `ps` that does not end is killed.
const CENSUS: Duration = Duration::from_secs(30);
/// The starter's environment the one server and its app keep: whichever checkout, agent or test
/// starts it, it serves every one, so it is no one's (an agent's `TASKS_SESSION` would sign the
/// owner's board writes). The test's pid, which a test's server ends with, and a test's machine
/// folder, whose registry that server reads, are kept.
const KEPT: [&str; 6] = [
    "HOME",
    "PATH",
    "USER",
    "LANG",
    crate::front::TEST_PID.name,
    symphony::machine::DIR_ENV,
];

/// Why the one server cannot be had.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    /// It was started and did not come up, or it runs and does not answer: the reason and its log.
    NotUp(String),
    /// The folder could not be read or written.
    Io(String),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::NotUp(why) | ServerError::Io(why) => f.write_str(why),
        }
    }
}

fn io(what: &str, path: &Path) -> impl Fn(std::io::Error) -> ServerError {
    let said = format!("{what} {}", path.display());
    move |problem| ServerError::Io(format!("{said}: {problem}"))
}

/// A server this module started, as its record has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Running {
    pub port: u16,
    pub pid: i64,
    pub root: String,
}

/// Where the one server keeps its record, its lock and its log, and how it is started.
pub struct Home {
    dir: PathBuf,
    legacy: Option<PathBuf>,
    legacy_record: OnceLock<Option<Running>>,
    install: Option<PathBuf>,
    exe: PathBuf,
    root: PathBuf,
    port: u16,
    hosts: Vec<String>,
    patience: Duration,
}

impl Home {
    /// The server in `dir`, started as `exe serve` on `port` and `hosts`, serving `root`.
    pub fn new(dir: &Path, exe: &Path, root: &Path, port: u16, hosts: Vec<String>) -> Home {
        Home {
            dir: dir.into(),
            legacy: None,
            legacy_record: OnceLock::new(),
            install: None,
            exe: exe.into(),
            root: root.into(),
            port,
            hosts,
            patience: PATIENCE,
        }
    }

    /// The folder whose record a server started before this one's folder left: read once, when
    /// this folder has none, to find that server.
    pub fn legacy(self, dir: &Path) -> Home {
        Home {
            legacy: Some(dir.into()),
            ..self
        }
    }

    /// The folder holding the server's build and client (`--install`), which a server it starts
    /// serves its client from and rebuilds into.
    pub fn install(self, dir: &Path) -> Home {
        Home {
            install: Some(dir.into()),
            ..self
        }
    }

    /// How long a server that is starting, or that runs and does not answer, is waited for.
    pub fn patience(self, patience: Duration) -> Home {
        Home { patience, ..self }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What the record says, without asking the server: the fast path, which goes to
    /// [`Home::ensure`] when the server does not answer.
    pub fn recorded(&self) -> Option<Running> {
        match fs::read_to_string(self.dir.join(ADDRESS)) {
            Ok(text) => serde_json::from_str(&text).ok(),
            Err(error) if error.kind() == ErrorKind::NotFound => self
                .legacy_record
                .get_or_init(|| {
                    let text = fs::read_to_string(self.legacy.as_ref()?.join(ADDRESS)).ok()?;
                    serde_json::from_str(&text).ok()
                })
                .clone(),
            Err(_) => None,
        }
    }

    /// The recorded server, if it answers as itself, found without the lock: the fast path of
    /// [`Home::ensure`], and all of `--if-up`.
    pub fn up(&self) -> Option<Running> {
        self.recorded().filter(|running| self.healthy(running))
    }

    /// The one server, answering: the recorded one, else one started now. The recorded one counts
    /// only if it answers as itself (a record can name any port, and any pid), and a record that
    /// names this folder's server wrongly is mended from that server's own command line.
    pub fn ensure(&self) -> Result<Running, ServerError> {
        if let Some(running) = self.up() {
            return Ok(running);
        }
        let _held = crate::cli::startup("startup lock", || self.lock())?;
        let found = self
            .recorded()
            .and_then(|recorded| self.running_of(recorded.pid))
            .or_else(|| crate::cli::startup("unrecorded server discovery", || self.unrecorded()));
        if let Some(running) = found {
            if self.answers(&running) {
                if self.recorded().as_ref() != Some(&running) {
                    self.record(&running)?;
                }
                return Ok(running);
            }
            return Err(ServerError::NotUp(format!(
                "the Symphony server (pid {}, port {}) runs and does not answer; its log is {}",
                running.pid,
                running.port,
                self.dir.join(LOG).display()
            )));
        }
        self.start()
    }

    /// Stop this folder's server, if it runs, and forget its record: what it was, if anything.
    pub fn stop(&self) -> Result<Option<Running>, ServerError> {
        let _held = self.lock()?;
        let legacy_selected = !self.dir.join(ADDRESS).exists() && self.recorded().is_some();
        let found = self
            .recorded()
            .and_then(|recorded| self.running_of(recorded.pid))
            .or_else(|| self.unrecorded());
        if let Some(running) = &found {
            self.end(running.pid)?;
        }
        let address = self.dir.join(ADDRESS);
        if address.exists() {
            fs::remove_file(&address).map_err(io("could not remove", &address))?;
        }
        if legacy_selected && let Some(legacy) = &self.legacy {
            let address = legacy.join(ADDRESS);
            match fs::remove_file(&address) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(io("could not remove", &address)(error)),
            }
        }
        Ok(found)
    }

    /// Every starter and stopper of this folder's server, one at a time.
    pub(crate) fn lock(&self) -> Result<File, ServerError> {
        self.lock_until(|| false)
    }

    pub(crate) fn lock_until(&self, stopping: impl Fn() -> bool) -> Result<File, ServerError> {
        fs::create_dir_all(&self.dir).map_err(io("could not make", &self.dir))?;
        let directory = if !self.dir.join(ADDRESS).exists() && self.recorded().is_some() {
            self.legacy.as_deref().unwrap_or(&self.dir)
        } else {
            &self.dir
        };
        let path = directory.join(LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io("could not open", &path))?;
        symphony::reload::lock_until(&lock, stopping)
            .map_err(|why| ServerError::Io(format!("could not lock {}: {why}", path.display())))?;
        Ok(lock)
    }

    /// The server `pid` is, when it is this folder's: running, with `serve --home <this folder>` on
    /// its command line, which also names its port and root. A pid recorded once may since have gone
    /// to any other process, which is never signalled.
    fn running_of(&self, pid: i64) -> Option<Running> {
        if !pid_alive(pid) {
            return None;
        }
        let mut ps = Command::new("ps");
        ps.args(["-ww", "-o", "command=", "-p", &pid.to_string()]);
        let out = crate::sys::output_within(ps, CENSUS).ok()?;
        self.parse(pid, String::from_utf8_lossy(&out.stdout).trim())
    }

    /// A server this folder's record does not name. Known free ports need no process table read;
    /// unknown startup history keeps the recovery scan rather than guessing a server is absent.
    fn unrecorded(&self) -> Option<Running> {
        let (ports, unknown) = self.discovery_ports();
        if !unknown && !ports.into_iter().any(held) {
            return None;
        }
        let mut ps = Command::new("ps");
        ps.args(["-axww", "-o", "pid=,command="]);
        let out = crate::sys::output_within(ps, CENSUS).ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let (pid, command) = line.trim().split_once(char::is_whitespace)?;
                self.parse(pid.parse().ok()?, command.trim())
            })
    }

    /// `command`, when it is this folder's server: its port and root.
    fn parse(&self, pid: i64, command: &str) -> Option<Running> {
        // These delimiters are the argument order emitted by start(). Spaces inside a checkout's
        // path are part of that path, not argument boundaries in ps's unquoted output.
        let args = crate::sys::server_command(command)?;
        let (home, rest) = args.strip_prefix("--home ")?.split_once(" --root ")?;
        if home != self.dir.to_str()? && self.legacy.as_deref().and_then(Path::to_str) != Some(home)
        {
            return None;
        }
        let (root, rest) = rest.split_once(" --port ")?;
        let mut port: u16 = rest.split_whitespace().next()?.parse().ok()?;
        if port == 0 {
            // Started on a port of its own pick: the port it got is in its log.
            port = announced_port(&since(&Path::new(home).join(LOG), 0), pid)?;
        }
        Some(Running {
            port,
            pid,
            root: root.to_string(),
        })
    }

    /// Whether the server at `running`'s port answers, as `running`'s pid serving its root: the
    /// project that started it, whichever project asks.
    fn healthy(&self, running: &Running) -> bool {
        probe::front(running.port, LOOK)
            .is_some_and(|front| front.pid == running.pid && front.root == running.root)
    }

    /// Whether `running` answers within the patience, while it runs.
    fn answers(&self, running: &Running) -> bool {
        let began = Instant::now();
        loop {
            if self.healthy(running) {
                return true;
            }
            if !pid_alive(running.pid) || began.elapsed() >= self.patience {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The available count for `running`. A pending census releases the starter: the process table
    /// may never answer. Servers without the pending event retain the patience for their first count.
    pub fn counted(&self, running: &Running) -> Option<symphony_api::Front> {
        let began = Instant::now();
        loop {
            let front = probe::front(running.port, LOOK);
            let counting = front.as_ref().is_some_and(|front| {
                front.pid == running.pid && front.uncounted && !front.census_pending
            });
            if !counting || began.elapsed() >= self.patience {
                return front;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Ask `pid` to end, then make it; it is gone when this returns.
    fn end(&self, pid: i64) -> Result<(), ServerError> {
        for (how, wait) in [(libc::SIGTERM, GRACE), (libc::SIGKILL, GRACE)] {
            signal(pid, how);
            let began = Instant::now();
            while began.elapsed() < wait {
                if !pid_alive(pid) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(ServerError::NotUp(format!(
            "the Symphony server (pid {pid}) did not end when told to"
        )))
    }

    /// Start the server, detached, wait until it and its app answer, and record it. Under the lock.
    fn start(&self) -> Result<Running, ServerError> {
        let log = self.dir.join(LOG);
        let from = fs::metadata(&log).map_or(0, |meta| meta.len());
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(io("could not open", &log))?;
        let err = out.try_clone().map_err(io("could not open", &log))?;
        let mut command = Command::new(&self.exe);
        command
            .arg("serve")
            .arg("--home")
            .arg(&self.dir)
            .arg("--root")
            .arg(&self.root)
            .arg("--port")
            .arg(self.port.to_string());
        for host in &self.hosts {
            command.arg("--host").arg(host);
        }
        if let Some(install) = &self.install {
            command.arg("--install").arg(install);
        }
        command
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .env_clear();
        for name in KEPT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let artifacts = symphony::project::Project::of(&self.root).artifact("docs-tmp");
        let temp = self.root.join(artifacts);
        fs::create_dir_all(&temp).map_err(io("could not make a temp directory in", &self.root))?;
        command.env("TMPDIR", temp);
        // A session of its own: no terminal's hangup reaches it, and it outlives the starter, reaped
        // by init.
        command.new_session();
        let mut child = crate::cli::startup("spawn detached server", || command.spawn())
            .map_err(io("could not start", &self.exe))?;
        let pid = i64::from(child.id());
        // Port 0 is the server's to pick: it binds first and says which it got, so nothing else
        // can take a port between a probe and the bind.
        let port = if self.port == 0 {
            match crate::cli::startup("detached server port announcement", || {
                self.announced(&log, from, pid, &mut child)
            }) {
                Some(port) => port,
                None => {
                    if child.try_wait().ok().flatten().is_none() {
                        self.end(pid)?;
                        let _ = child.wait();
                    }
                    return Err(ServerError::NotUp(format!(
                        "the Symphony server did not start: {}; its log is {}",
                        since(&log, from).trim(),
                        log.display()
                    )));
                }
            }
        } else {
            self.port
        };
        let running = Running {
            port,
            pid,
            root: self.root.display().to_string(),
        };
        if !crate::cli::startup("detached server HTTP readiness", || {
            self.came_up(&running, &mut child)
        }) {
            if child.try_wait().ok().flatten().is_none() {
                self.end(running.pid)?;
                let _ = child.wait();
                return Err(ServerError::NotUp(format!(
                    "the Symphony server (pid {}, port {}) did not answer within {} s; its log is {}",
                    running.pid,
                    running.port,
                    self.patience.as_secs(),
                    log.display()
                )));
            }
            return Err(ServerError::NotUp(format!(
                "the Symphony server did not start: {}; its log is {}",
                since(&log, from).trim(),
                log.display()
            )));
        }
        self.record(&running)?;
        Ok(running)
    }

    /// The port the server `pid` bound, read from the line it prints once it listens, within the
    /// patience and while it runs.
    fn announced(&self, log: &Path, from: u64, pid: i64, child: &mut Child) -> Option<u16> {
        let began = Instant::now();
        while began.elapsed() < self.patience {
            if let Some(port) = announced_port(&since(log, from), pid) {
                return Some(port);
            }
            if child.try_wait().ok().flatten().is_some() {
                // It may have printed just before it ended.
                return announced_port(&since(log, from), pid);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// Whether the server just started answers, and its app behind it, within the patience. One that
    /// answers while its app does not yet is up: its pages wait for the app, and say so.
    fn came_up(&self, running: &Running, child: &mut Child) -> bool {
        let began = Instant::now();
        let mut up = false;
        while began.elapsed() < self.patience {
            if child.try_wait().ok().flatten().is_some() {
                return false;
            }
            match probe::front(running.port, LOOK) {
                Some(front) if front.pid == running.pid && front.ready => return true,
                Some(front) if front.pid == running.pid => up = true,
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        up
    }

    /// Write the record whole, so a reader never sees half of it.
    pub(crate) fn record(&self, running: &Running) -> Result<(), ServerError> {
        let address = self.dir.join(ADDRESS);
        let next = self.dir.join(format!("{ADDRESS}.{}", std::process::id()));
        let text = serde_json::to_string(running).expect("a record serialises");
        let result = fs::write(&next, text)
            .map_err(io("could not write", &next))
            .and_then(|()| fs::rename(&next, &address).map_err(io("could not write", &address)));
        if result.is_err() {
            let _ = fs::remove_file(&next);
        }
        result
    }
}

/// Whether anything holds loopback `port`, where the one server always listens: only a refused
/// connection says it is free. One that is accepted, or that does not finish within a look, is held.
fn held(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    !matches!(
        TcpStream::connect_timeout(&address, LOOK),
        Err(problem) if problem.kind() == ErrorKind::ConnectionRefused
    )
}

/// The port in the last `→ Symphony at http://127.0.0.1:PORT/  (pid PID)` line `said` holds for
/// `pid`.
fn announced_port(said: &str, pid: i64) -> Option<u16> {
    let tail = format!("(pid {pid})");
    said.lines()
        .filter(|line| line.trim_end().ends_with(&tail))
        .filter_map(|line| {
            line.split("http://127.0.0.1:")
                .nth(1)?
                .split('/')
                .next()?
                .parse()
                .ok()
        })
        .next_back()
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
