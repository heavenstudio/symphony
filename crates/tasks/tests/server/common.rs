//! A folder of its own per test, for a board and its server, served by the guarded `surreal`; and,
//! from [`crate::own_board`], that `surreal` (a repository of its own, its board in its git dir, is
//! that module's `Repository`). A server a test leaves running is stopped when its folder goes, by
//! the pid its address file records, and only if that process serves this folder. A pipe watchdog
//! also ends every server if the test process is killed. A healthy occupier takes a port a server
//! was told to bind, and answers there as a server would.

pub use crate::own_board::{
    GuardedSurreal, Repository, command_of, gone, parent_of, serves_data, signal, surreal,
};
use std::convert::Infallible;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::server::{Home, Running};
use symphony_board::sys::{Child, Command, Spawning as _, Stdio};

pub struct Scratch {
    pub dir: PathBuf,
    surreal: GuardedSurreal,
}

/// Each line a [`Home`] tells, kept in order ([`Home::telling`]).
#[derive(Clone, Default)]
pub struct Told(Arc<Mutex<Vec<String>>>);

impl Told {
    /// What a [`Home::telling`] takes, to keep each line it tells here.
    pub fn sink(&self) -> impl Fn(&str) + Send + Sync + 'static {
        let lines = Arc::clone(&self.0);
        move |line| lines.lock().unwrap().push(line.to_string())
    }

    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// How many folders of tests that failed in runs since ended are kept, the newest.
const KEPT: usize = 5;

/// The folders [`Scratch`] kept under `root` for tests that failed (`server-PID-NAME`) in runs that
/// have since ended, all but the newest `keep` removed. A run still going keeps every one of its
/// own; nothing else under `root` is touched.
pub fn prune_kept(root: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let ended = |name: &str| {
        name.strip_prefix("server-")
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<i64>().ok())
            .is_some_and(|pid| !symphony_board::sys::pid_alive(pid))
    };
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(ended))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_dir()
                .then_some((meta.modified().ok()?, entry.path()))
        })
        .collect();
    kept.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in kept.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(path);
    }
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static PRUNED: std::sync::Once = std::sync::Once::new();
        PRUNED.call_once(|| prune_kept(Path::new(env!("CARGO_TARGET_TMPDIR")), KEPT));
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("server-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let surreal = GuardedSurreal::new(&dir);
        Scratch { dir, surreal }
    }

    pub fn home(&self) -> Home {
        Home::new(&self.dir, self.surreal())
    }

    pub fn surreal(&self) -> &Path {
        self.surreal.path()
    }

    /// Write the address file as a server would have.
    pub fn record(&self, running: &Running) {
        std::fs::write(
            self.dir.join("server.json"),
            serde_json::to_string(running).unwrap(),
        )
        .unwrap();
    }

    /// The data a server of this folder is started on, as its command line names it.
    pub fn data(&self) -> String {
        format!("surrealkv://{}", self.dir.join("data").display())
    }

    /// Stop `pid` if it serves this folder.
    pub fn stop_if_ours(&self, pid: i64) {
        if serves_data(pid, &self.data()) {
            signal(pid, "KILL");
            gone(pid);
        }
    }
}

impl Drop for Scratch {
    /// A test that failed keeps its folder, its server's log in it, and says whether its guard had
    /// closed before the test ended: a guard closed early refuses every start without a word.
    fn drop(&mut self) {
        let failed = std::thread::panicking();
        if failed {
            eprintln!(
                "{} is kept; its guard {} closed before its test ended",
                self.dir.display(),
                if self.dir.join("surreal-processes/closed").exists() {
                    "had"
                } else {
                    "had not"
                }
            );
        }
        self.surreal.shutdown();
        let recorded = std::fs::read_to_string(self.dir.join("server.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Running>(&text).ok());
        if let Some(running) = recorded {
            self.stop_if_ours(running.pid);
        }
        if !failed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn exists(path: &Path) -> bool {
    path.try_exists().unwrap()
}

/// Installs `body` behind the prepared shell image and checks that it exits with no arguments,
/// before any startup patience begins. The argument guard keeps that check from starting a server
/// or counting a start. The script's path.
pub fn script(path: &Path, body: &str) -> PathBuf {
    crate::fixture_script::write(
        path,
        &format!("#!/bin/sh\n[ $# -gt 0 ] || exit 0\n{body}\n"),
        false,
    );
    let status = std::process::Command::new(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status_locked()
        .unwrap();
    assert!(
        status.success(),
        "{} with no arguments: {status}",
        path.display()
    );
    path.to_path_buf()
}

/// Reads a request's head from `stream` and answers it as a healthy server does (`200 OK`, no
/// body), then closes the connection.
pub fn answer_healthy(mut stream: TcpStream) {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
        request.push(byte[0]);
    }
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}

/// Takes `addr` (port 0 for any) with a listener that answers each request as a healthy server does
/// (`200 OK`, no body): a process that took a child's port. From its first answer on, the file
/// `probed` holds how many it has given, each count written whole. It holds the port until the test
/// process ends. The address it took.
pub fn healthy_occupier(addr: SocketAddr, probed: &Path) -> SocketAddr {
    let listener = TcpListener::bind(addr).unwrap();
    let taken = listener.local_addr().unwrap();
    let probed = probed.to_path_buf();
    let next = probed.with_extension("next");
    std::thread::spawn(move || {
        let mut answered = 0;
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            answer_healthy(stream);
            answered += 1;
            std::fs::write(&next, answered.to_string()).unwrap();
            std::fs::rename(&next, &probed).unwrap();
        }
    });
    taken
}

#[test]
fn one_watchdog_ends_every_server_on_eof_even_with_inherited_high_writers() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    struct Servers(Vec<Child>);
    impl Drop for Servers {
        fn drop(&mut self) {
            for child in &mut self.0 {
                if child.try_wait().unwrap().is_none() {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
        }
    }
    let mut s = Scratch::new("watchdog-eof");
    use std::os::unix::fs::MetadataExt;
    let stand_in = script(&s.dir.join("no-op"), "exit 0");
    let guarded = std::fs::metadata(s.surreal()).unwrap();
    let ordinary = std::fs::metadata(stand_in).unwrap();
    assert_eq!(
        (guarded.dev(), guarded.ino()),
        (ordinary.dev(), ordinary.ino())
    );
    let mut servers = Servers(Vec::new());
    for index in 0..2 {
        let writer = s.surreal.watchdog.stdin.as_ref().unwrap().as_raw_fd();
        let log = s.dir.join(format!("server-{index}.log"));
        // Started again on another port only if its port was taken before it bound it, every
        // start within the one readiness bound, the backstop of the 20 s it once had; up once the
        // server itself listens there.
        let until = Instant::now() + test_time::backstop(Duration::from_secs(20));
        symphony_board::port::start_on_free_port(
            symphony_board::port::ATTEMPTS,
            until,
            |reserved| {
                let addr = reserved.to_string();
                let out = std::fs::File::create(&log).unwrap();
                let mut command = Command::new(s.surreal());
                command
                    .args([
                        "start",
                        "--unauthenticated",
                        "--bind",
                        &addr,
                        "--log",
                        "error",
                    ])
                    .arg(format!(
                        "surrealkv://{}",
                        s.dir.join(format!("data-{index}")).display()
                    ))
                    .stdin(Stdio::null())
                    .stdout(out.try_clone().unwrap())
                    .stderr(out);
                // A writer above fd 3 models the descriptor a helper can inherit from its native test.
                // Without fixture_close_fds the server retains it and the watchdog can never see EOF.
                // It is handed at 64 or above, away from shell redirections; this process's copy is
                // close-on-exec from its making and goes once the server has started.
                // SAFETY: F_DUPFD_CLOEXEC makes a new descriptor, which the OwnedFd alone then owns.
                let inherited = unsafe {
                    let fd = libc::fcntl(writer, libc::F_DUPFD_CLOEXEC, 64);
                    assert_ne!(fd, -1, "{}", std::io::Error::last_os_error());
                    OwnedFd::from_raw_fd(fd)
                };
                servers
                    .0
                    .push(command.pass_fd(inherited.as_raw_fd()).spawn().unwrap());
                drop(inherited);
                let server = servers.0.last_mut().unwrap();
                loop {
                    if server.try_wait().unwrap().is_some() {
                        servers.0.pop();
                        let said = std::fs::read_to_string(&log).unwrap();
                        return Ok::<_, Infallible>(symphony_board::port::Start::Ended(said));
                    }
                    if symphony_board::rpc::healthy(&addr)
                        && symphony_board::port::listens(
                            server.id(),
                            reserved,
                            symphony_board::sys::output,
                        )
                        .unwrap()
                    {
                        return Ok(symphony_board::port::Start::Up(()));
                    }
                    assert!(Instant::now() < until, "server readiness: {addr}");
                    std::thread::sleep(Duration::from_millis(20));
                }
            },
        )
        .unwrap_or_else(|why| panic!("server {index}: {why}"));
    }
    let watchdog_pid = i64::from(s.surreal.watchdog.id());
    assert!(s.surreal.watchdog.try_wait().unwrap().is_none());
    assert!(
        s.surreal.shutdown().success(),
        "the watchdog must exit on EOF without a forced kill"
    );
    assert!(
        !symphony_board::sys::pid_alive(watchdog_pid),
        "the watchdog must be reaped"
    );
    for server in &mut servers.0 {
        test_time::until_true(
            "each server ended at its watchdog's EOF",
            test_time::backstop(Duration::from_secs(5)),
            || server.try_wait().unwrap().is_some(),
        )
        .unwrap();
    }
    // A wrapper already forked when shutdown scans may register late. The closed marker must
    // refuse it, rather than let it start a server after its watchdog has gone. The wrapper ends
    // without a word; a server it started would print its banner, even on a port taken meanwhile.
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let late_log = s.dir.join("late.log");
    let out = std::fs::File::create(&late_log).unwrap();
    servers.0.push(
        Command::new(s.surreal())
            .args([
                "start",
                "--unauthenticated",
                "--bind",
                &addr,
                "--log",
                "error",
            ])
            .arg(format!("surrealkv://{}", s.dir.join("late-data").display()))
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap(),
    );
    let late = servers.0.last_mut().unwrap();
    let status = test_time::until(
        "the late wrapper ended, starting no server after shutdown",
        test_time::backstop(Duration::from_secs(5)),
        || late.try_wait().unwrap(),
    )
    .unwrap();
    assert_eq!(
        status.code(),
        Some(1),
        "a closed fixture must refuse new servers"
    );
    assert_eq!(
        std::fs::read_to_string(&late_log).unwrap(),
        "",
        "the wrapper must refuse before a server runs"
    );
}

/// Only the folders of runs that have ended are pruned, the newest kept; a running process's, and
/// what is not a kept folder, stay.
#[test]
fn kept_folders_of_ended_runs_are_pruned_to_the_newest() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("prune-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ended = || {
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        child.id()
    };
    let ours = std::process::id();
    let names = [
        format!("server-{}-oldest", ended()),
        format!("server-{}-older", ended()),
        format!("server-{ours}-running"),
        format!("server-{}-newest", ended()),
        "server-runs".to_string(),
    ];
    for name in &names {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let file = format!("server-{}-file", ended());
    std::fs::write(root.join(&file), "").unwrap();
    prune_kept(&root, 1);
    let mut left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut expected = vec![names[2].clone(), names[3].clone(), names[4].clone(), file];
    expected.sort();
    assert_eq!(left, expected);
    let _ = std::fs::remove_dir_all(&root);
}
