//! A folder of its own per test, for a board and its server, and the pinned `surreal` to serve it
//! (the store tests' `surreal()`, the one way every test binary finds it). A server a test leaves
//! running is stopped when its folder goes, by the pid its address file records, and only if that
//! process serves this folder. A pipe watchdog also ends every server if the test process is killed.
//! A healthy occupier takes a port a server was told to bind, and answers there as a server would.

use std::convert::Infallible;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use symphony_board::server::{Home, Running};
use symphony_board::sys::{Child, Command, Spawning as _, Stdio};

pub fn surreal() -> PathBuf {
    static FOUND: OnceLock<PathBuf> = OnceLock::new();
    FOUND.get_or_init(crate::store::surreal).clone()
}

/// A test-only executable passed through `SF_SURREAL` (or directly to `Home`). It execs the real
/// server, preserving the pid and arguments that production records. Its watchdog reads a pipe
/// whose only writer belongs to this fixture: EOF also happens on abort or SIGKILL, without Drop.
/// Like the store tests' server guard, no test-owned write descriptor reaches a child after exec.
pub struct GuardedSurreal {
    binary: PathBuf,
    watchdog: Child,
}

impl GuardedSurreal {
    pub fn new(dir: &Path) -> Self {
        let records = dir.join("surreal-processes");
        std::fs::create_dir(&records).unwrap();
        let helper = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tools/fixture_process.sh"
        );
        // One reader on one real pipe (symphony_board::sys::pipe): a reader blocked on a FIFO can remain
        // blocked on macOS after its last writer closes. Keep the watchdog owned and reap it on
        // normal teardown; its own group survives an interrupted suite's group.
        let watchdog = crate::store::spawn_with_input(
            Command::new("bash")
                .args([
                    "-c",
                    r#"
. "$1"
fixture_close_fds
fixture_wait_eof
: > "$2/closed"
for record in "$2"/server-*; do
    [ -f "$record" ] || continue
    pid=${record##*/server-}
    IFS= read -r data < "$record" || continue
    command=$(ps -ww -o command= -p "$pid")
    case "$command" in
        *"$data"*) kill -KILL "$pid" 2>/dev/null ;;
    esac
done
exit 0
"#,
                    "watch-surreal",
                ])
                .arg(helper)
                .arg(&records)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0),
        )
        .unwrap();
        let binary = dir.join("guarded-surreal");
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
        let body = format!(
            r#". {}
fixture_close_fds
if [ "$1" = start ]; then
    for data do :; done
    printf '%s\n' "$data" > {}/server-$$ || exit 1
    # Register before checking closure: shutdown either sees our record, or we refuse to start.
    [ ! -e {}/closed ] || exit 1
fi
exec {} "$@"
"#,
            quote(helper),
            quote(&records.display().to_string()),
            quote(&records.display().to_string()),
            quote(&surreal().display().to_string())
        );
        // Re-exec before closing descriptors: the interpreter's input script is also above 2.
        crate::fixture_script::write(
            &binary,
            &format!(
                "#!/bin/sh\nexec /bin/bash -c {} guarded-surreal \"$@\"\n",
                quote(&body)
            ),
            false,
        );
        Self { binary, watchdog }
    }

    pub fn path(&self) -> &Path {
        &self.binary
    }

    pub fn shutdown(&mut self) -> ExitStatus {
        drop(self.watchdog.stdin.take());
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(5) {
            if let Some(status) = self.watchdog.try_wait().unwrap() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.watchdog.kill();
        self.watchdog.wait().unwrap()
    }
}

impl Drop for GuardedSurreal {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The command line of process `pid`, if it runs.
pub fn command_of(pid: i64) -> Option<String> {
    let out = symphony_board::sys::output(Command::new("ps").args([
        "-ww",
        "-o",
        "command=",
        "-p",
        &pid.to_string(),
    ]))
    .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

pub fn signal(pid: i64, name: &str) {
    let _ = std::process::Command::new("kill")
        .arg(format!("-{name}"))
        .arg(pid.to_string())
        .status_locked();
}

/// Waits until `pid` is gone, for up to 20 s: whether it went.
pub fn gone(pid: i64) -> bool {
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(20) {
        if !symphony_board::sys::pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

pub struct Scratch {
    pub dir: PathBuf,
    surreal: GuardedSurreal,
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
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
        if command_of(pid).is_some_and(|line| line.contains(&self.data())) {
            signal(pid, "KILL");
            gone(pid);
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.surreal.shutdown();
        let recorded = std::fs::read_to_string(self.dir.join("server.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Running>(&text).ok());
        if let Some(running) = recorded {
            self.stop_if_ours(running.pid);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
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
        // start within the one readiness bound; up once the server itself listens there.
        let until = Instant::now() + Duration::from_secs(20);
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
        let began = Instant::now();
        while server.try_wait().unwrap().is_none() {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "a server survived EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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
    let began = Instant::now();
    let status = loop {
        if let Some(status) = late.try_wait().unwrap() {
            break status;
        }
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "a late server started after shutdown"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
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
