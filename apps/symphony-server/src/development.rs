//! A loopback-only development front door: DX owns pages/assets/WebSockets; an owned Symphony
//! server owns APIs/media/SSE. Forward bytes unchanged so Host and Origin still match on saves.
//! No proxy or development code is started by the production server.

use std::io::{BufRead, BufReader, Read};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony::children::{self, sessionless};
use symphony_board::sys::{Child, Command};
use symphony_board::{port, sys};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The children this front door started, each leading a group of its own. A leader that has ended
/// is left unreaped ([`crate::sys::exited`]) until the last signal to its group, so the group's id
/// is still its own and no signal reaches a group numbered after it.
struct Children(Vec<Child>);

impl Children {
    fn drain(children: &[Child], until: Instant) -> bool {
        loop {
            // Each leader's group is waited for as well: DX can exit before cargo/rustc, and its
            // exit alone does not release the build folder safely.
            let running = children.iter().any(|child| {
                !crate::sys::exited(child) || crate::sys::group_runs(i64::from(child.id()))
            });
            if !running {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Stop every group, then reap each leader: how each child, by its pid, ended.
    fn stop(&mut self) -> Result<Vec<(u32, ExitStatus)>, String> {
        let mut children = std::mem::take(&mut self.0);
        if children.is_empty() {
            return Ok(Vec::new());
        }
        for child in &children {
            crate::sys::signal_group(i64::from(child.id()), libc::SIGTERM);
        }
        if !Self::drain(&children, Instant::now() + Duration::from_secs(3)) {
            for child in &children {
                crate::sys::signal_group(i64::from(child.id()), libc::SIGKILL);
            }
            if !Self::drain(&children, Instant::now() + Duration::from_secs(2)) {
                let groups: Vec<_> = children.iter().map(Child::id).collect();
                return Err(format!(
                    "owned child process groups {groups:?} did not stop within 5 s"
                ));
            }
        }
        let mut ended = Vec::new();
        for child in &mut children {
            let status = child
                .wait()
                .map_err(|e| format!("reap child {}: {e}", child.id()))?;
            ended.push((child.id(), status));
        }
        eprintln!("· symphony-ui-dev: owned child process groups stopped");
        Ok(ended)
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        if let Err(why) = self.stop() {
            eprintln!("symphony-ui-dev cleanup: {why}");
        }
    }
}

fn capture(
    reader: impl Read + Send + 'static,
    output: Arc<Mutex<Option<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&bytes);
            eprint!("{line}");
            if let Some(output) = output.lock().expect("child output mutex").as_mut() {
                output.push_str(&line);
            }
        }
    })
}

/// Only a child that exits saying its address is taken is retried. Retries share one startup
/// deadline, and its entire owned group is stopped before starting another.
fn start_child(
    until: Instant,
    parent: i64,
    cancelled: &AtomicBool,
    mut command: impl FnMut(SocketAddr) -> Command,
) -> Result<(Child, SocketAddr), String> {
    port::start_on_free_port(port::ATTEMPTS, until, |addr| {
        if cancelled.load(Ordering::Relaxed) || crate::sys::parent() != parent {
            return Err("development startup cancelled".to_string());
        }
        if addr.port() == crate::home::DEFAULT_PORT {
            // Never start a private child on the shared server's port, even when it is down.
            return Ok(port::Start::Ended("AddrInUse: shared server port".into()));
        }
        let mut launch = command(addr);
        let program = launch.get_program().to_string_lossy().into_owned();
        // Its output is read to the end after it exits: no child another thread spawns may hold it.
        let mut child = sys::spawn_output(sessionless(&mut launch).process_group(0))
            .map_err(|e| format!("start {program} on {addr}: {e}"))?;
        #[cfg(test)]
        tests::SPAWNED.set(Some(child.id()));
        let output = Arc::new(Mutex::new(Some(String::new())));
        let readers = [
            capture(child.stdout.take().expect("piped stdout"), output.clone()),
            capture(child.stderr.take().expect("piped stderr"), output.clone()),
        ];
        let mut owned = Children(vec![child]);
        loop {
            if cancelled.load(Ordering::Relaxed) || crate::sys::parent() != parent {
                return Err("development startup cancelled".to_string());
            }
            if crate::sys::exited(&owned.0[0]) {
                owned.stop()?;
                for reader in readers {
                    reader.join().map_err(|_| "read development child output")?;
                }
                return Ok(port::Start::Ended(
                    output
                        .lock()
                        .expect("child output mutex")
                        .take()
                        .expect("startup output"),
                ));
            }
            if port::listens(owned.0[0].id(), addr, children::output).map_err(|e| e.to_string())? {
                // Continue showing output during development, without retaining an unbounded log.
                output.lock().expect("child output mutex").take();
                return Ok(port::Start::Up(owned.0.remove(0)));
            }
            if Instant::now() >= until {
                return Err(format!(
                    "development child on {addr} did not listen before its startup deadline"
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
    .map_err(|e| e.to_string())
}

/// Called only by the dedicated symphony-ui-dev binary, under the cargo wrapper.
pub fn main(args: &[String]) -> Result<(), String> {
    let [root, dx, port] = args else {
        return Err("usage: symphony-ui-dev ROOT DX PORT (just symphony-ui-dev PORT)".into());
    };
    let port: u16 = port.parse().map_err(|e| format!("port {port}: {e}"))?;
    if port == crate::home::DEFAULT_PORT {
        return Err("the shared server's port is never a development port".into());
    }
    let root = Path::new(root).canonicalize().map_err(|e| e.to_string())?;
    let dx = PathBuf::from(dx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(run(root, dx, port))
}

async fn run(root: PathBuf, dx: PathBuf, port: u16) -> Result<(), String> {
    let parent = crate::sys::parent();
    // Install both signal handlers before waiting for a child to listen. block_on runs this
    // future on the main thread; the signal task runs on a worker during synchronous retries.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| e.to_string())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let stopping = cancelled.clone();
    struct Signals(tokio::task::JoinHandle<()>);
    impl Drop for Signals {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _signals = Signals(tokio::spawn(async move {
        tokio::select! {
            _ = interrupt.recv() => {},
            _ = terminate.recv() => {},
        }
        stopping.store(true, Ordering::Relaxed);
    }));
    // Bind the public port first: a collision starts no child and never reuses another server.
    let front = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| format!("listen on port {port}: {e}"))?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let server = executable.with_file_name("symphony-server");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .ok_or("run symphony-ui-dev through the cargo wrapper")?
        .join("symphony-ui-dev");
    let mut children = Children(Vec::new());
    // DX listens before starting its client build. Both starts share one bounded startup window.
    let until = Instant::now() + Duration::from_secs(600);
    let api_start = start_child(until, parent, &cancelled, |addr| {
        let mut command = Command::new(&server);
        command.args(["serve", "--root"]).arg(&root).args([
            "--port",
            &addr.port().to_string(),
            "--host",
            "127.0.0.1",
        ]);
        command
    });
    let (child, api) = match api_start {
        Ok(started) => started,
        Err(_) if cancelled.load(Ordering::Relaxed) => return Ok(()),
        Err(why) => return Err(why),
    };
    children.0.push(child);
    let web_start = start_child(until, parent, &cancelled, |addr| {
        let mut command = Command::new(&dx);
        command
            .current_dir(symphony::reload::workspace(&root).join("apps/symphony-ui"))
            .env("CARGO_INCREMENTAL", "1")
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_PROFILE_WASM_DEV_PANIC", "abort")
            .env("CARGO_PROFILE_WASM_DEV_DEBUG", "0")
            .env("CARGO_PROFILE_WASM_DEV_OPT_LEVEL", "1")
            .args([
                "serve",
                "--web",
                "--locked",
                "--no-default-features",
                "--features",
                "web,devtools",
                "--bin",
                "symphony-ui-dev",
                "--addr",
                "127.0.0.1",
                "--port",
                &addr.port().to_string(),
                "--open",
                "false",
                "--interactive",
                "false",
                "--debug-symbols",
                "false",
            ]);
        command
    });
    let (child, web) = match web_start {
        Ok(started) => started,
        Err(_) if cancelled.load(Ordering::Relaxed) => return Ok(()),
        Err(why) => return Err(why),
    };
    children.0.push(child);
    let (api, web) = (api.port(), web.port());
    println!(
        "→ symphony-ui development at http://{}/tasks (pid {}, parent {parent}, API {api}, DX {web}; Ctrl-C stops both)",
        front.local_addr().map_err(|e| e.to_string())?,
        std::process::id()
    );
    let nav = std::sync::Arc::new(crate::api::project_view(&root));
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let outcome = 'serving: loop {
        tokio::select! {
            _ = tick.tick() => {
                if cancelled.load(Ordering::Relaxed) {
                    break Ok(());
                }
                if crate::sys::parent() != parent {
                    eprintln!("· symphony-ui-dev: parent ended, stopping owned children");
                    break Ok(());
                }
                if let Some(child) = children.0.iter().find(|child| crate::sys::exited(child)) {
                    break 'serving Err(child.id());
                }
            }
            accepted = front.accept() => {
                let (stream, _) = accepted.map_err(|e| e.to_string())?;
                let nav = nav.clone();
                tokio::spawn(async move {
                    if let Err(why) = forward(stream, api, web, &nav).await {
                        eprintln!("symphony-ui-dev request: {why}");
                    }
                });
            }
        }
    };
    let ended = children.stop()?;
    outcome.map_err(|pid| match ended.iter().find(|(each, _)| *each == pid) {
        Some((_, status)) => format!("child {pid} ended: {status}"),
        None => format!("child {pid} ended"),
    })
}

async fn forward(
    mut stream: TcpStream,
    api: u16,
    web: u16,
    nav: &symphony_api::ProjectView,
) -> Result<(), String> {
    // One request per upstream connection. Force HTTP/1.1 close for ordinary requests so a
    // keep-alive connection cannot send its next API request to the page server (or vice versa).
    // Upgrade connections keep their headers: WebSocket frames and SSE are streamed untouched.
    let mut request = Vec::new();
    let header_end = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut part = [0_u8; 4096];
            let count = stream.read(&mut part).await.map_err(|e| e.to_string())?;
            if count == 0 {
                return Err("incomplete request headers".to_string());
            }
            request.extend_from_slice(&part[..count]);
            if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                return Ok(at + 4);
            }
            if request.len() > 65_536 {
                return Err("request headers exceed 64 KiB".into());
            }
        }
    })
    .await
    .map_err(|_| "request headers timed out".to_string())??;
    let headers = std::str::from_utf8(&request[..header_end]).map_err(|e| e.to_string())?;
    let target = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("missing request path")?;
    let path = crate::api::decode_path(target.split('?').next().unwrap_or(target))
        .ok_or("bad request path")?;
    let (_, path) = crate::api::scope_path(&path);
    let port = if crate::api::client_page(nav, path)
        || path.starts_with("/assets/")
        || path.starts_with("/wasm/")
        || path.starts_with("/_dioxus")
    {
        web
    } else {
        api
    };
    let upgrade = headers
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("upgrade:"));
    let mut outgoing = Vec::new();
    for line in headers.split("\r\n").filter(|line| !line.is_empty()) {
        if !upgrade && line.to_ascii_lowercase().starts_with("connection:") {
            continue;
        }
        outgoing.extend_from_slice(line.as_bytes());
        outgoing.extend_from_slice(b"\r\n");
    }
    if !upgrade {
        outgoing.extend_from_slice(b"Connection: close\r\n");
    }
    outgoing.extend_from_slice(b"\r\n");
    outgoing.extend_from_slice(&request[header_end..]);
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut upstream = match TcpStream::connect(address).await {
        Ok(stream) => stream,
        Err(e) => {
            let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 29\r\n\r\nDevelopment server starting.\n").await;
            return Err(format!("connect to {address}: {e}"));
        }
    };
    upstream
        .write_all(&outgoing)
        .await
        .map_err(|e| e.to_string())?;
    tokio::io::copy_bidirectional(&mut stream, &mut upstream)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The startup deadline a case gives a child it means to come up, or a check it means to
    /// reach: a backstop only a stall reaches (RULES.md `tests/no-test-fails-on-load`), as no case
    /// here is about that deadline but the one that gives an expired one.
    pub(super) const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(10));

    thread_local! {
        /// The child [`start_child`] last spawned on this thread, recorded as it is spawned, before
        /// any deadline is looked at.
        pub(super) static SPAWNED: Cell<Option<u32>> = const { Cell::new(None) };
    }

    /// The child [`start_child`] spawns next on this thread, its group killed and reaped if a
    /// failing assertion left it.
    struct ChildPid;

    impl ChildPid {
        fn new() -> Self {
            SPAWNED.set(None);
            Self
        }

        fn pid(&self) -> Option<i64> {
            SPAWNED.get().map(i64::from)
        }

        fn assert_stopped(&self) {
            let pid = self.pid().expect("the spawned child recorded its pid");
            assert!(
                !crate::sys::group_runs(pid),
                "owned child group {pid} leaked"
            );
        }
    }

    impl Drop for ChildPid {
        fn drop(&mut self) {
            // A failing cleanup assertion must still end the fake child this case started.
            if let Some(pid) = self.pid()
                && crate::sys::group_runs(pid)
            {
                crate::sys::signal_group(pid, libc::SIGKILL);
                // SAFETY: this PID was recorded by this case's own spawned child.
                unsafe {
                    libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), 0);
                }
            }
        }
    }

    #[test]
    fn child_startup_retries_only_taken_addresses_and_bounds_starts() {
        for (message, expired, expected_starts) in [
            (
                "Address already in use (os error 48)",
                false,
                port::ATTEMPTS,
            ),
            ("AddrInUse", false, port::ATTEMPTS),
            ("compile failed", false, 1),
            ("startup deadline", true, 1),
        ] {
            let until = Instant::now() + if expired { Duration::ZERO } else { BACKSTOP };
            let mut ports = Vec::new();
            let pid = ChildPid::new();
            let result = start_child(
                until,
                crate::sys::parent(),
                &AtomicBool::new(false),
                |addr| {
                    assert!(!ports.contains(&addr.port()), "a retry reused its port");
                    ports.push(addr.port());
                    let mut command = Command::new("/bin/sh");
                    command.args([
                        "-c",
                        if expired {
                            "exec sleep 86400"
                        } else if message.ends_with("(os error 48)") {
                            "echo \"$1\"; exit 1"
                        } else if message == "AddrInUse" {
                            "printf '\\377\\n' >&2; echo \"$1\" >&2; exit 1"
                        } else {
                            "echo \"$1\" >&2; exit 1"
                        },
                        "fake-child",
                        message,
                    ]);
                    command
                },
            );
            let why = result.unwrap_err();
            // The expired-deadline row starts a silent child and fails on the startup deadline.
            assert!(why.contains(message), "{why}");
            assert_eq!(ports.len(), expected_starts, "{why}");
            if message == "AddrInUse" {
                assert!(
                    why.contains('\u{fffd}'),
                    "invalid UTF-8 was not retained lossily: {why}"
                );
            }
            if expired {
                pid.assert_stopped();
            }
        }
    }

    #[test]
    fn a_child_that_cannot_spawn_or_never_listens_is_not_retried() {
        for executable in ["/no-such-development-child", "/bin/sh"] {
            let mut starts = 0;
            let pid = ChildPid::new();
            let result = start_child(
                Instant::now(),
                crate::sys::parent(),
                &AtomicBool::new(false),
                |_| {
                    starts += 1;
                    let mut command = Command::new(executable);
                    command.args(["-c", "exec sleep 86400"]);
                    command
                },
            );
            assert!(result.is_err());
            assert_eq!(starts, 1);
            let why = result.unwrap_err();
            if executable == "/bin/sh" {
                pid.assert_stopped();
            } else {
                assert!(why.contains(executable), "{why}");
            }
            assert!(
                why.contains(if executable == "/bin/sh" {
                    "startup deadline"
                } else {
                    "No such file"
                }),
                "{why}"
            );
        }
    }

    #[test]
    fn cancellation_stops_startup_without_another_attempt() {
        for (before_start, parent) in [
            (true, crate::sys::parent()),
            (false, crate::sys::parent()),
            (false, i64::MIN),
        ] {
            let cancelled = AtomicBool::new(before_start);
            let mut starts = 0;
            let pid = ChildPid::new();
            let result = start_child(Instant::now() + BACKSTOP, parent, &cancelled, |_| {
                starts += 1;
                cancelled.store(true, Ordering::Relaxed);
                let mut command = Command::new("/bin/sh");
                command.args(["-c", "exec sleep 86400"]);
                command
            });
            assert!(result.unwrap_err().contains("cancelled"));
            assert_eq!(
                starts,
                usize::from(!before_start && parent == crate::sys::parent())
            );
            if starts == 1 {
                pid.assert_stopped();
            } else {
                assert!(pid.pid().is_none(), "no child was spawned");
            }
        }
    }

    // Invoked as a fake child by the next case. In an ordinary test run it checks the same
    // listener ownership contract on an OS-assigned port and returns immediately.
    #[test]
    fn fake_listener() {
        let child_addr = std::env::var("SYMPHONY_DEV_FAKE_LISTENER").ok();
        let listener =
            std::net::TcpListener::bind(child_addr.as_deref().unwrap_or("127.0.0.1:0")).unwrap();
        assert!(
            port::listens(
                std::process::id(),
                listener.local_addr().unwrap(),
                sys::output
            )
            .unwrap()
        );
        if child_addr.is_some() {
            loop {
                std::thread::park();
            }
        }
    }

    #[test]
    fn a_retry_keeps_the_child_that_itself_listens_on_the_new_address() {
        let mut starts = 0;
        let mut tried = Vec::new();
        let (child, addr) = start_child(
            Instant::now() + BACKSTOP,
            crate::sys::parent(),
            &AtomicBool::new(false),
            |addr| {
                starts += 1;
                assert!(!tried.contains(&addr));
                tried.push(addr);
                if starts == 1 {
                    let mut command = Command::new("/bin/sh");
                    command.args(["-c", "echo 'Address already in use' >&2; exit 1"]);
                    command
                } else {
                    let mut command = Command::new(std::env::current_exe().unwrap());
                    command
                        .args([
                            "--exact",
                            "development::tests::fake_listener",
                            "--nocapture",
                        ])
                        .env("SYMPHONY_DEV_FAKE_LISTENER", addr.to_string());
                    command
                }
            },
        )
        .unwrap();
        let mut owned = Children(vec![child]);
        // The deliberately failed first start must be retried; a real collision on a later
        // reservation is handled by the same bound.
        assert!((2..=port::ATTEMPTS).contains(&starts));
        assert_ne!(addr, tried[0]);
        assert!(port::listens(owned.0[0].id(), addr, sys::output).unwrap());
        assert!(std::net::TcpStream::connect(addr).is_ok());
        let pid = owned.0[0].id();
        owned.stop().unwrap();
        assert!(!crate::sys::group_runs(i64::from(pid)));
    }

    #[test]
    fn a_stop_signals_a_group_whose_leader_ended_before_it_reaps_the_leader() {
        let mut command = Command::new("/bin/sh");
        // The sleep lives until its group is signalled.
        command.args(["-c", "/bin/sleep 86400 </dev/null >/dev/null 2>&1 & exit 3"]);
        let child = sessionless(&mut command).process_group(0).spawn().unwrap();
        let pid = child.id();
        test_time::until_true("the leader to exit", BACKSTOP, || {
            crate::sys::exited(&child)
        })
        .unwrap();
        // The leader is a zombie its group still names, the sleep it left still running.
        assert!(crate::sys::pid_alive(i64::from(pid)));
        assert!(crate::sys::group_runs(i64::from(pid)));
        let ended = Children(vec![child]).stop().unwrap();
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].0, pid);
        assert_eq!(ended[0].1.code(), Some(3));
        assert!(!crate::sys::group_runs(i64::from(pid)));
    }
}
