//! The one Symphony server of a repository, and the folder it is found by: `sf-symphony/` in the git common
//! dir, which every worktree shares (the shape of the board's server, `Home` in
//! `symphony/crates/tasks/src/server.rs`).
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
use tasks::sys::{Child, Command, Stdio};

/// The one server's folder in the git common dir.
pub const FOLDER: &str = "sf-symphony";
const LEGACY_FOLDER: &str = "sf-docs";
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
/// owner's board writes). The test's pid, which a test's server ends with, is kept.
const KEPT: [&str; 5] = ["HOME", "PATH", "USER", "LANG", crate::front::TEST_PID];

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
            legacy: (dir.file_name().is_some_and(|name| name == FOLDER))
                .then(|| dir.parent().map(|parent| parent.join(LEGACY_FOLDER)))
                .flatten(),
            legacy_record: OnceLock::new(),
            exe: exe.into(),
            root: root.into(),
            port,
            hosts,
            patience: PATIENCE,
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

    /// A server of this folder that the record does not name (its starter was killed between its
    /// answer and its record, or the record was lost): it holds the port, so no other could start.
    /// The process table is read only while something holds this folder's port or the record's, or
    /// when the port is the server's own pick; a start or a stop on a free port reads none.
    fn unrecorded(&self) -> Option<Running> {
        let ports = [Some(self.port), self.recorded().map(|running| running.port)];
        if !ports.contains(&Some(0)) && !ports.into_iter().flatten().any(held) {
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

    /// Whether the server at `running`'s port answers, as `running`'s pid.
    fn healthy(&self, running: &Running) -> bool {
        probe::front(running.port, LOOK).is_some_and(|front| {
            front.pid == running.pid
                && front.root == running.root
                && Path::new(&front.root) == self.root
        })
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
        let temp = self.root.join("artifacts/docs-tmp");
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
mod tests {
    use super::*;

    #[test]
    fn server_records_prefer_the_symphony_home_and_read_the_predecessor_once() {
        let tree = crate::testing::Tree::new();
        let home = Home::new(
            &tree.path(FOLDER),
            Path::new("/bin/symphony-server"),
            Path::new("/r"),
            0,
            vec![],
        );
        assert_eq!(home.recorded(), None);
        let old = Running {
            port: 41001,
            pid: 41,
            root: "/r".into(),
        };
        let new = Running {
            port: 41002,
            pid: 42,
            root: "/r".into(),
        };
        let legacy = format!("{LEGACY_FOLDER}/{ADDRESS}");
        tree.write(&legacy, &serde_json::to_string(&old).unwrap());
        assert_eq!(
            home.recorded(),
            None,
            "an absent predecessor is read only once"
        );
        let reader = Home::new(
            &tree.path(FOLDER),
            Path::new("/bin/symphony-server"),
            Path::new("/r"),
            0,
            vec![],
        );
        assert_eq!(reader.recorded(), Some(old.clone()));
        tree.write(&legacy, &serde_json::to_string(&new).unwrap());
        assert_eq!(
            reader.recorded(),
            Some(old),
            "the predecessor record is cached"
        );
        tree.write(
            &format!("{FOLDER}/{ADDRESS}"),
            &serde_json::to_string(&new).unwrap(),
        );
        assert_eq!(
            reader.recorded(),
            Some(new.clone()),
            "the primary record wins"
        );
        tree.write(&format!("{FOLDER}/{ADDRESS}"), "broken");
        assert_eq!(
            reader.recorded(),
            None,
            "a corrupt primary must not select an old record"
        );
        fs::remove_file(tree.path(&format!("{FOLDER}/{ADDRESS}"))).unwrap();
        assert_eq!(reader.recorded().unwrap().pid, 41);
        let arbitrary = Home::new(
            &tree.path("private-home"),
            Path::new("/bin/symphony-server"),
            Path::new("/r"),
            0,
            vec![],
        );
        assert_eq!(
            arbitrary.recorded(),
            None,
            "private homes do not adopt unrelated records"
        );
    }

    #[test]
    fn a_predecessor_record_is_up_only_when_its_private_server_answers_as_recorded() {
        let tree = crate::testing::Tree::new();
        let (port, looking) = statuses(vec![status(41, 0, false), status(99, 0, false)]);
        tree.write(
            &format!("{LEGACY_FOLDER}/{ADDRESS}"),
            &serde_json::to_string(&Running {
                port,
                pid: 41,
                root: "/r".into(),
            })
            .unwrap(),
        );
        let home = Home::new(
            &tree.path(FOLDER),
            Path::new("/bin/symphony-server"),
            Path::new("/r"),
            port,
            vec![],
        );
        assert_eq!(home.up().unwrap().pid, 41);
        assert_eq!(
            home.up(),
            None,
            "a different process cannot satisfy the record"
        );
        drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
        assert_eq!(looking.join().unwrap(), 2);
        assert_eq!(home.up(), None, "a dead listener is not a live predecessor");
        let command = format!(
            "/old/bin/review-server serve --home {} --root /r --port {port}",
            tree.path(LEGACY_FOLDER).display()
        );
        assert_eq!(home.parse(41, &command).unwrap().port, port);
        tree.write(
            &format!("{LEGACY_FOLDER}/{LOG}"),
            &format!("→ Symphony at http://127.0.0.1:{port}/  (pid 41)\n"),
        );
        assert_eq!(
            home.parse(41, &command.replace(&format!("--port {port}"), "--port 0"))
                .unwrap()
                .port,
            port
        );
        assert_eq!(
            home.parse(
                41,
                &command.replace("/old/bin/review-server", "/old/bin/other")
            ),
            None
        );
        assert_eq!(
            home.parse(41, &command.replace("/sf-docs ", "/unrelated ")),
            None
        );
    }

    /// A server on loopback whose status answers are `answers`, one per look, in turn; a look past
    /// the last is closed unanswered. A connection that sends nothing ends it, with the looks it had.
    fn statuses(answers: Vec<symphony_api::Front>) -> (u16, std::thread::JoinHandle<usize>) {
        use std::io::Write;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let looking = std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            let mut looks = 0;
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => request.push(byte[0]),
                        _ => break,
                    }
                }
                if request.is_empty() {
                    return looks;
                }
                looks += 1;
                if let Some(front) = answers.next() {
                    let body = serde_json::to_string(&front).unwrap();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
            }
            looks
        });
        (port, looking)
    }

    /// The status of the server `pid`, up, with `orphans` counted or still `uncounted`.
    fn status(pid: i64, orphans: usize, uncounted: bool) -> symphony_api::Front {
        symphony_api::Front {
            pid,
            port: 1,
            root: "/r".into(),
            ready: true,
            app: None,
            tailnet: None,
            orphans,
            uncounted,
            census_pending: false,
        }
    }

    /// What `home.counted` returns for the server `pid` whose looks are answered `answers`, and how
    /// many looks it took.
    fn counted_of(
        home: Home,
        pid: i64,
        answers: Vec<symphony_api::Front>,
    ) -> (Option<symphony_api::Front>, usize) {
        let (port, looking) = statuses(answers);
        let running = Running {
            port,
            pid,
            root: "/r".into(),
        };
        let found = home.counted(&running);
        drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
        (found, looking.join().unwrap())
    }

    #[test]
    fn a_start_reads_the_leftovers_once_the_first_census_has_counted_within_its_patience() {
        let home = || Home::new(Path::new("/h"), Path::new("/x"), Path::new("/r"), 0, vec![]);
        // Looked at until it says it has counted: that answer, and no look after it.
        let (found, looks) = counted_of(
            home(),
            7,
            vec![status(7, 0, true), status(7, 0, true), status(7, 2, false)],
        );
        assert_eq!(found, Some(status(7, 2, false)));
        assert_eq!(looks, 3);
        // A zero patience takes the first answer, counted or not: the wait is the patience's.
        let (found, looks) = counted_of(
            home().patience(Duration::ZERO),
            7,
            vec![status(7, 0, true), status(7, 2, false)],
        );
        assert_eq!(found, Some(status(7, 0, true)));
        assert_eq!(looks, 1);
        // Another server on the port is no count of this one's to wait for.
        let (found, looks) = counted_of(home(), 7, vec![status(8, 0, true), status(7, 2, false)]);
        assert_eq!(found, Some(status(8, 0, true)));
        assert_eq!(looks, 1);
        // The census's start event releases the starter without needing its completion event.
        let mut pending = status(7, 0, true);
        pending.census_pending = true;
        let (found, looks) = counted_of(home(), 7, vec![pending.clone(), status(7, 2, false)]);
        assert_eq!(found, Some(pending));
        assert_eq!(looks, 1);
    }

    #[test]
    fn the_announced_port_is_the_last_line_of_that_pid() {
        let said = "→ Symphony at http://127.0.0.1:41000/  (pid 7)\n\
                    → Symphony at http://127.0.0.1:41001/  (pid 8)\n\
                    → Symphony at http://127.0.0.1:41002/  (pid 7)\n\
                    → Symphony at http://127.0.0.1:x/  (pid 9)\n\
                    mentions http://127.0.0.1:42000/ (pid 10) and goes on\n";
        assert_eq!(announced_port(said, 7), Some(41002));
        assert_eq!(announced_port(said, 8), Some(41001));
        assert_eq!(announced_port(said, 9), None);
        assert_eq!(announced_port(said, 10), None);
        assert_eq!(announced_port(said, 11), None);
        assert_eq!(announced_port("", 7), None);
    }

    #[test]
    fn a_port_is_held_unless_a_connection_to_it_is_refused() {
        let listening = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listening.local_addr().unwrap().port();
        assert!(held(port));
        // The local end of a connection this test holds: bound, and listened on by nothing.
        let connected = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(!held(connected.local_addr().unwrap().port()));
    }

    #[test]
    fn recovery_of_a_server_on_a_picked_port_reads_the_port_from_its_log() {
        let dir = std::env::temp_dir().join(format!("home-picked-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(LOG),
            "→ Symphony at http://127.0.0.1:43210/  (pid 42)\n",
        )
        .unwrap();
        let home = Home::new(
            &dir,
            Path::new("/bin/symphony-server"),
            Path::new("/r"),
            0,
            vec![],
        );
        let command = format!(
            "{0}/bin/symphony-server serve --home {0} --root /r --port 0 --host 127.0.0.1",
            dir.display()
        );
        let found = home.parse(42, &command).map(|running| running.port);
        // Another pid, with nothing in the log, has no port to name.
        let other = home.parse(43, &command);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, Some(43210));
        assert_eq!(other, None);
    }

    #[test]
    fn recovery_reads_whole_paths_and_only_this_homes_server() {
        let home = Home::new(
            Path::new("/repo with spaces/.git/sf-symphony"),
            Path::new("/bin/symphony-server"),
            Path::new("/repo with spaces"),
            8765,
            Vec::new(),
        );
        let command = "/repo with spaces/.git/sf-symphony/bin/symphony-server serve --home /repo with spaces/.git/sf-symphony --root /repo with spaces --port 8765 --host 127.0.0.1 --app stub";
        assert_eq!(
            home.parse(42, command),
            Some(Running {
                port: 8765,
                pid: 42,
                root: "/repo with spaces".into(),
            })
        );
        for bad in [
            command.replace("/bin/symphony-server serve", "/bin/other serve"),
            command.replace("--home /repo with spaces/.git/sf-symphony", "--home /other"),
            command.replace("--port 8765", "--port invalid"),
            command.replace(" --root /repo with spaces", ""),
        ] {
            assert_eq!(home.parse(42, &bad), None, "{bad}");
        }
    }
}
