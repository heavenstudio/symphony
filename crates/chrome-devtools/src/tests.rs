//! The driver's own cases: its leash, cleanup, waits and protocol bounds, against stand-in browsers
//! and transports, and against the real Chrome when this machine has one (`SYMPHONY_CHROME` names
//! it).

use std::io::ErrorKind;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::profiles::{self, next_profile};
use crate::{
    BrowserGroup, CLEANUP_CPU, Chrome, Leash, Owner, POLL, PROTOCOL_CPU, WaitBudget, chrome_args,
    cleanup_profile, devtools_endpoint, find, process, signal_group,
};

/// This test binary as the leash: it runs [`browser_leash`] alone.
pub(crate) fn leash() -> Leash {
    Leash::new(
        std::env::current_exe().unwrap(),
        &["--exact", "tests::browser_leash", "--nocapture"],
    )
}

/// A process id as the kernel's calls take one.
fn pid(pid: u32) -> libc::pid_t {
    libc::pid_t::try_from(pid).unwrap()
}

#[test]
fn a_zero_allowance_keeps_initial_readiness_and_bounds_both_active_and_idle_replies() {
    let member = |cpu| process::Member {
        pid: 1,
        parent: 0,
        state: 1,
        cpu,
    };
    let mut budget = WaitBudget::new(Duration::ZERO, &[member(Duration::ZERO)]);
    assert!(!budget.exhausted(&[member(Duration::ZERO)]));
    assert!(!budget.exhausted(&[member(Duration::from_millis(1))]));
    assert!(budget.exhausted(&[member(Duration::from_secs(60))]));
    let mut idle = WaitBudget::new(Duration::ZERO, &[]);
    assert!(idle.poll(&[]));
}

/// Reexecuted in a separate process, with no shared mutable environment in the suite.
#[test]
fn browser_leash() {
    let Some(code) = crate::leash_main() else {
        let args = chrome_args(Path::new("/private/profile"));
        assert!(args.contains(&"--mute-audio".into()));
        assert!(args.contains(&"--user-data-dir=/private/profile".into()));
        // The page under test runs at foreground priority however Chrome counts its tab.
        assert!(args.contains(&"--disable-renderer-backgrounding".into()));
        assert!(args.contains(&"--disable-backgrounding-occluded-windows".into()));
        return;
    };
    std::process::exit(code);
}

#[test]
fn profile_cleanup_rechecks_after_helpers_and_reports_other_errors() {
    use std::io::BufRead;

    let profile = next_profile().unwrap();
    std::fs::create_dir_all(profile.join("nested")).unwrap();
    std::fs::write(profile.join("nested/data"), "profile").unwrap();
    // Announce readiness before cleanup starts. Recreate the profile only after its first
    // removal, so returning on that removal instead of waiting for helpers always leaks it.
    let mut browser = BrowserGroup(
        Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf 'ready\n'
while test -d "$1"; do /bin/sleep 0.01; done
mkdir "$1"
"#,
                "profile-writer",
            ])
            .arg(&profile)
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    std::io::BufReader::new(browser.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let group = browser.0.id();
    let reaper = std::thread::spawn(move || browser.0.wait().unwrap());
    let cleanup = cleanup_profile(&profile, group);
    if cleanup.is_err() {
        // A regression must also stop this owned helper instead of stranding its waiter.
        let _ = signal_group(group, 9);
    }
    let status = reaper.join().unwrap();
    cleanup.unwrap();
    assert!(status.success());
    assert!(!profile.exists(), "cleanup must remove the whole profile");
    cleanup_profile(&profile, group).unwrap();

    // A path that is not a directory is a real fixture error, not a retryable writer race.
    std::fs::write(&profile, "not a directory").unwrap();
    let error = cleanup_profile(&profile, group).unwrap_err();
    std::fs::remove_file(&profile).unwrap();
    assert_eq!(error.kind(), ErrorKind::NotADirectory);
}

/// Keep a removed CPU guard from hanging its regression: end the owned group
/// after more CPU work, so the caller can assert on the wrong completion result.
fn cpu_watchdog(
    group: u32,
    limit: Duration,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<bool>) {
    let (done, receive) = std::sync::mpsc::channel();
    let watcher = std::thread::spawn(move || {
        let mut budget = None;
        while matches!(
            receive.recv_timeout(POLL),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            // A transient snapshot error while a churned child exits is not CPU work.
            let Ok(members) = process::members(group) else {
                continue;
            };
            let budget = budget.get_or_insert_with(|| process::Budget::new(limit, &members));
            if budget.exhausted(&members) {
                let _ = signal_group(group, 9);
                return true;
            }
        }
        false
    });
    (done, watcher)
}

struct StandIn {
    owner: Owner,
    binary: PathBuf,
    group: Option<u32>,
}

impl StandIn {
    fn new(script: &str) -> Self {
        Self::under(script, &leash())
    }

    fn under(script: &str, leash: &Leash) -> Self {
        let binary = next_profile().unwrap();
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let owner = Owner::launch(binary.to_str().unwrap(), leash).unwrap();
        Self {
            owner,
            binary,
            group: None,
        }
    }

    fn group(&mut self) -> u32 {
        let group = self
            .owner
            .ready("recorded stand-in PID", PROTOCOL_CPU, |profile| {
                let pid = std::fs::read_to_string(profile.join("browser.pid")).ok()?;
                if !pid.ends_with('\n') {
                    return None;
                }
                pid.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
            })
            .unwrap();
        self.group = Some(group);
        group
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        // A probe of BrowserGroup's kill must not strand this fixture's spinner.
        // The separate native leash also owns it if this test is killed outright.
        if let Some(group) = self.group {
            let _ = signal_group(group, 9);
        }
        let _ = std::fs::remove_file(&self.binary);
    }
}

#[test]
fn shutdown_diagnostics_keep_browser_exit_and_suite_eof_distinct_after_profile_cleanup() {
    use std::io::{Seek, Write};
    for (script, close_suite, expected) in [
        (
            "#!/bin/sh\nprintf 'browser exit detail\\n' >&2\nexit 47\n",
            false,
            "browser exited:",
        ),
        (
            "#!/bin/sh\nprintf 'browser waiting detail\\n' >&2\nexec /usr/bin/tail -f /dev/null\n",
            true,
            "suite stdin closed",
        ),
    ] {
        let mut stand_in = StandIn::new(script);
        if close_suite {
            // The launcher's PID record is written after opening the stderr log.
            stand_in.group();
            let diagnostics = stand_in.owner.diagnostics();
            assert!(diagnostics.contains("status=Ok(None)"), "{diagnostics}");
            assert!(
                diagnostics.contains("shutdown=not recorded yet"),
                "{diagnostics}"
            );
            drop(stand_in.owner.leash.stdin.take());
        }
        // Child::wait closes its stdin. Hold the suite's lifetime pipe separately so
        // observing a browser exit cannot itself request an EOF shutdown.
        let lifetime = stand_in.owner.leash.stdin.take();
        let status = stand_in.owner.leash.wait().unwrap();
        drop(lifetime);
        assert_eq!(status.code(), Some(if close_suite { 0 } else { 47 }));
        assert!(!stand_in.owner.profile.exists());
        let diagnostics = stand_in.owner.diagnostics();
        assert!(diagnostics.contains(expected), "{diagnostics}");
        if !close_suite {
            assert!(diagnostics.contains("47"), "{diagnostics}");
            assert!(diagnostics.contains("browser exit detail"), "{diagnostics}");
        }
        assert!(diagnostics.contains("members=Ok([])"), "{diagnostics}");
        let exit = stand_in.owner.profile.with_extension("exit");
        let stderr = stand_in.owner.profile.with_extension("stderr");
        assert!(!exit.exists());
        assert!(!stderr.exists());
        // An arbitrarily long Chrome log must not turn one socket failure into huge output.
        stand_in.owner.stderr.rewind().unwrap();
        stand_in
            .owner
            .stderr
            .write_all(format!("discarded{}kept", "x".repeat(9_000)).as_bytes())
            .unwrap();
        let diagnostics = stand_in.owner.diagnostics();
        assert!(!diagnostics.contains("discarded"));
        assert!(diagnostics.ends_with("kept"));
        assert!(diagnostics.len() < 9_000);
        drop(stand_in);
        assert!(!exit.exists());
        assert!(!stderr.exists());
    }
}

#[test]
fn a_browser_that_cannot_start_fails_the_launch_promptly_with_why() {
    let missing = next_profile().unwrap().join("no-such-chrome");
    let failure = Chrome::launch(missing.to_str().unwrap(), &leash())
        .err()
        .expect("a missing browser must fail its launch");
    let failure = failure.to_string();
    assert!(failure.contains("Chrome's leash exited"), "{failure}");
    assert!(failure.contains("browser did not start"), "{failure}");
    assert!(failure.contains(missing.to_str().unwrap()), "{failure}");
}

/// Set on the run of [`a_quiet_leash_keeps_its_reports_off_the_launchers_stderr`] that launches.
const QUIET: &str = "SF_CHROME_QUIET_LEASH";

#[test]
fn a_quiet_leash_keeps_its_reports_off_the_launchers_stderr() {
    const TEST: &str = "tests::a_quiet_leash_keeps_its_reports_off_the_launchers_stderr";
    if let Ok(quiet) = std::env::var(QUIET) {
        let leash = if quiet == "1" {
            leash().quiet()
        } else {
            leash()
        };
        let mut stand_in = StandIn::under("#!/bin/sh\nexit 3\n", &leash);
        let lifetime = stand_in.owner.leash.stdin.take();
        assert_eq!(stand_in.owner.leash.wait().unwrap().code(), Some(3));
        drop(lifetime);
        return;
    }
    for (quiet, reported) in [("0", true), ("1", false)] {
        let output = suite_child(TEST)
            .env(QUIET, quiet)
            .stdout(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(
            stderr.contains("Chrome shutdown"),
            reported,
            "quiet={quiet}: {stderr}"
        );
    }
}

#[test]
fn an_unexpected_protocol_reset_fails_with_the_browser_exit_evidence() {
    let binary = next_profile().unwrap();
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'reset exit detail\\n' >&2\nexit 47\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = Owner::launch(binary.to_str().unwrap(), &leash()).unwrap();
    let lifetime = owner.leash.stdin.take();
    assert_eq!(owner.leash.wait().unwrap().code(), Some(47));
    drop(lifetime);
    std::fs::remove_file(binary).unwrap();

    // A peer dropping TCP without a WebSocket close exercises the reported transport
    // boundary without requiring a real Chrome crash.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    drop(peer);
    let mut chrome = Chrome {
        owner,
        socket: WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(stream),
            tungstenite::protocol::Role::Client,
            None,
        ),
        next_id: 0,
        session: None,
        observer: None,
        events: Vec::new(),
        arrivals: Vec::new(),
        deadline: None,
    };
    let failure = chrome
        .read(false)
        .expect_err("an unexpected reset must still fail the case")
        .to_string();
    assert!(failure.contains("reading from Chrome:"), "{failure}");
    assert!(failure.contains("browser exited:"), "{failure}");
    assert!(failure.contains("reset exit detail"), "{failure}");
}

#[test]
fn cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill() {
    if std::env::var_os("SF_CHROME_CLEANUP_SPINNER").is_none() {
        let output =
            suite_child("tests::cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill")
                .env("SF_CHROME_CLEANUP_SPINNER", "1")
                .output()
                .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let report = stdout
            .lines()
            .find_map(|line| line.strip_prefix("CLEANUP_SPINNER "))
            .expect("cleanup fixture report");
        let (group, seconds): (u32, u64) = serde_json::from_str(report).unwrap();
        let lines = stderr
            .lines()
            .filter(|line| line.starts_with(&format!("Chrome cleanup group={group} ")))
            .count();
        assert!(lines > 0, "the live helper must be diagnosed");
        // Two cleanup calls: one CPU failure and one ESRCH wait after the final kill.
        assert!(
            u64::try_from(lines).unwrap() <= seconds + 2,
            "{lines} ESRCH diagnostics in {seconds} seconds"
        );
        return;
    }

    let mut stand_in = StandIn::new(
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
/usr/bin/tail -f /dev/null &
main=$!
(
    while :; do
        /bin/sleep 0.01 &
        i=0
        while test "$i" -lt 2000; do i=$((i+1)); done
        wait
    done
) &
helper=$!
printf '%s %s\n' "$main" "$helper" > "$profile/spinner-ready"
wait "$main"
wait "$helper"
"#,
    );
    let group = stand_in.group();
    let (main, helper) = stand_in
        .owner
        .ready("spinning helper", PROTOCOL_CPU, |profile| {
            let ready = std::fs::read_to_string(profile.join("spinner-ready")).ok()?;
            if !ready.ends_with('\n') {
                return None;
            }
            let pids: Vec<u32> = ready
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            (pids.len() == 2 && pids.iter().all(|pid| *pid > 0)).then(|| (pids[0], pids[1]))
        })
        .unwrap();
    // SAFETY: the owned fixture announced this positive main PID; killing it leaves
    // its helper running in the recorded group, under the separate leash's ownership.
    assert_eq!(unsafe { libc::kill(pid(main), 9) }, 0);
    assert!(
        process::members(group)
            .unwrap()
            .iter()
            .any(|member| member.pid == helper)
    );
    let (done, watcher) = cpu_watchdog(group, CLEANUP_CPU * 2);
    let started = Instant::now();
    let result = cleanup_profile(&stand_in.owner.profile, group);
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a missed spinning helper must fail cleanup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(error.to_string().contains("CPU after SIGKILL"), "{error}");
    eprintln!("spinning helper={helper} group={group}: {error}; watchdog killed={forced}");
    println!(
        "CLEANUP_SPINNER {}",
        json!([group, started.elapsed().as_secs()])
    );
}

/// Drain queued protocol messages until the peer closes the connection, with or without a
/// WebSocket close handshake. Other protocol and I/O errors are fixture failures.
fn wait_for_protocol_eof(
    socket: &mut WebSocket<impl std::io::Read + std::io::Write>,
) -> tungstenite::Result<()> {
    use tungstenite::Error;
    use tungstenite::error::ProtocolError;

    loop {
        match socket.read() {
            Ok(_) => {}
            Err(
                Error::ConnectionClosed
                | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
            ) => {
                return Ok(());
            }
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::BrokenPipe
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn protocol_eof_drains_messages_and_rejects_nonclosure_errors() {
    use std::io::Cursor;
    use tungstenite::protocol::Role;

    for ending in [b"".as_slice(), b"\x88\x00"] {
        // Two server text frames precede either TCP EOF or a WebSocket close followed by EOF.
        let frames = [b"\x81\x01a\x81\x01b".as_slice(), ending].concat();
        let mut socket = WebSocket::from_raw_socket(Cursor::new(frames), Role::Client, None);
        wait_for_protocol_eof(&mut socket).unwrap();
        assert!(matches!(
            socket.read(),
            Err(tungstenite::Error::AlreadyClosed)
        ));
    }
    // A reserved opcode is a protocol failure, not evidence that the browser exited.
    let mut socket =
        WebSocket::from_raw_socket(Cursor::new(b"\x83\x00".to_vec()), Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::InvalidOpcode(3)
        ))
    ));

    // An open but silent peer is not EOF; exercise WouldBlock without a timing deadline.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    let mut socket = WebSocket::from_raw_socket(stream, Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock
    ));

    struct FailedIo(ErrorKind);
    impl std::io::Read for FailedIo {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
    }
    impl std::io::Write for FailedIo {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(self.0.into())
        }
    }
    for kind in [
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
        ErrorKind::TimedOut,
    ] {
        let mut socket = WebSocket::from_raw_socket(FailedIo(kind), Role::Client, None);
        let result = wait_for_protocol_eof(&mut socket);
        if kind == ErrorKind::TimedOut {
            assert!(matches!(result, Err(tungstenite::Error::Io(error)) if error.kind() == kind));
        } else {
            result.unwrap();
        }
    }
}

const SUITE_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGHUP, libc::SIGTERM];

fn suite_signal_disposition(disposition: libc::sighandler_t) -> std::io::Result<()> {
    // SAFETY: sigemptyset and sigaction are async-signal-safe. The initialized action
    // contains no handler pointer and changes only the calling process's dispositions.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = disposition;
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for signal in SUITE_SIGNALS {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// This test binary run again on the case `test` alone.
pub(crate) fn suite_child(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture"]);
    // Background shells can pass SIG_IGN through exec. Every catchable signal in
    // the lifecycle matrix must terminate this child; SIGKILL cannot be ignored.
    // SAFETY: the hook changes only the forked child's dispositions through
    // async-signal-safe calls, before it executes the suite fixture.
    unsafe { command.pre_exec(|| suite_signal_disposition(libc::SIG_DFL)) };
    command
}

#[test]
fn suite_child_resets_inherited_ignored_signals() {
    const TEST: &str = "tests::suite_child_resets_inherited_ignored_signals";
    const MODE: &str = "SF_CHROME_SUITE_SIGNAL_MODE";
    let mode = std::env::var(MODE).ok();
    if let Some(mode) = mode {
        let expected = match mode.as_str() {
            "ignored-parent" => libc::SIG_IGN,
            "reset-child" => libc::SIG_DFL,
            _ => panic!("unknown suite signal fixture mode: {mode}"),
        };
        for signal in SUITE_SIGNALS {
            // SAFETY: a null action queries this process's disposition into initialized storage.
            let action = unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut action), 0);
                action
            };
            assert_eq!(action.sa_sigaction, expected, "signal {signal} in {mode}");
        }
        if mode == "ignored-parent" {
            assert!(
                suite_child(TEST)
                    .env(MODE, "reset-child")
                    .status()
                    .unwrap()
                    .success(),
                "suite child must reset its ignored parent's signal dispositions"
            );
        }
        return;
    }

    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent
        .args(["--exact", TEST, "--nocapture"])
        .env(MODE, "ignored-parent");
    // SAFETY: only this forked parent's dispositions change, through the same
    // async-signal-safe calls. The concurrent test runner keeps its own dispositions.
    unsafe { parent.pre_exec(|| suite_signal_disposition(libc::SIG_IGN)) };
    assert!(parent.status().unwrap().success());
}

#[test]
fn chrome_dies_with_its_suite() {
    use std::io::{BufRead, Write};
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os("SF_CHROME_SUITE_CHILD").is_some() {
        let fake_binary = std::env::var("SF_SYMPHONY_FAKE_BROWSER").ok();
        let real = fake_binary
            .is_none()
            .then(|| Chrome::launch(&find().expect("real Chrome capability"), &leash()).unwrap());
        let mut fake = fake_binary.map(|binary| Owner::launch(&binary, &leash()).unwrap());
        let profile = real
            .as_ref()
            .map(|chrome| &chrome.owner.profile)
            .or_else(|| fake.as_ref().map(|owner| &owner.profile))
            .unwrap()
            .clone();
        let helper = fake.as_mut().map(|owner| {
            owner
                .ready("stand-in helper handshake", PROTOCOL_CPU, |profile| {
                    if let Ok(ready) = std::fs::read_to_string(profile.join("fake-ready"))
                        && ready.ends_with('\n')
                    {
                        let pids: Vec<u32> = ready
                            .split_whitespace()
                            .filter_map(|pid| pid.parse().ok())
                            .collect();
                        let recorded = std::fs::read_to_string(profile.join("browser.pid"))
                            .ok()
                            .filter(|text| text.ends_with('\n'))
                            .and_then(|text| text.trim().parse::<u32>().ok());
                        if pids.len() == 2 && recorded == Some(pids[0]) {
                            return Some(pids[1]);
                        }
                    }
                    None
                })
                .expect("stand-in helper handshake")
        });
        let endpoint = real
            .as_ref()
            .map(|_| std::fs::read_to_string(profile.join("DevToolsActivePort")).unwrap());
        let group: u32 = std::fs::read_to_string(profile.join("browser.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let leash = real
            .as_ref()
            .map(|chrome| chrome.owner.leash.id())
            .or_else(|| fake.as_ref().map(|owner| owner.leash.id()))
            .unwrap();
        println!(
            "CHROME_READY {}",
            json!([profile, endpoint, group, helper, leash])
        );
        std::io::stdout().flush().unwrap();
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        return;
    }
    struct Suite(std::process::Child);
    impl Drop for Suite {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Sweep(Option<u32>, PathBuf);
    impl Drop for Sweep {
        fn drop(&mut self) {
            if let Some(group) = self.0 {
                let _ = signal_group(group, 9);
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let scratch = Scratch(next_profile().unwrap());
    std::fs::create_dir(&scratch.0).unwrap();
    let fake_browser = scratch.0.join("browser");
    std::fs::write(
        &fake_browser,
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
test -n "$profile" || exit 2
trap '' HUP TERM
(
    while :; do
        mkdir -p "$profile/writing/nested"
        printf '%s\n' writing > "$profile/writing/nested/data"
        rm -f "$profile/writing/nested/data"
    done
) 2>/dev/null &
helper=$!
trap 'exit 0' TERM
printf '%s %s\n' "$$" "$helper" > "$profile/fake-ready"
wait "$helper"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake_browser, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut variants = vec![Some(fake_browser)];
    if find().is_some() {
        variants.push(None);
    } else {
        eprintln!("no Chrome: real browser capability unavailable; stand-in still tested");
    }
    for fake_browser in variants {
        for (signal, expected, closed_stderr) in [
            ("normal", None, false),
            ("-TERM", Some(libc::SIGTERM), false),
            ("-HUP", Some(libc::SIGHUP), false),
            ("-INT", Some(libc::SIGINT), false),
            ("-KILL", Some(libc::SIGKILL), false),
            ("-KILL", Some(libc::SIGKILL), true),
        ] {
            let mut command = suite_child("tests::chrome_dies_with_its_suite");
            if let Some(binary) = &fake_browser {
                command.env("SF_SYMPHONY_FAKE_BROWSER", binary);
            } else {
                command.env_remove("SF_SYMPHONY_FAKE_BROWSER");
            }
            let mut suite = Suite(
                command
                    .env("SF_CHROME_SUITE_CHILD", "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(if closed_stderr {
                        Stdio::piped()
                    } else {
                        Stdio::inherit()
                    })
                    .spawn()
                    .unwrap(),
            );
            let stdout = suite.0.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    if let Some(ready) = line.strip_prefix("CHROME_READY ") {
                        let _ =
                            send.send(
                                serde_json::from_str::<(
                                    PathBuf,
                                    Option<String>,
                                    u32,
                                    Option<u32>,
                                    u32,
                                )>(ready)
                                .unwrap(),
                            );
                    }
                }
            });
            let (profile, endpoint, group, helper, leash) =
                receive.recv().expect("suite protocol handshake");
            let mut sweep = Sweep(Some(group), profile.clone());
            assert!(
                signal_group(group, 0).is_ok(),
                "recorded browser group must be alive before stopping its suite"
            );
            if let Some(helper) = helper {
                // SAFETY: getpgid only queries the positive helper PID the owned fixture announced.
                let actual = unsafe { libc::getpgid(pid(helper)) };
                assert_eq!(
                    actual,
                    pid(group),
                    "the never-exiting helper must be alive in the browser's group"
                );
            }
            assert_eq!(
                std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let mut protocol = endpoint.map(|endpoint| {
                let mut lines = endpoint.lines();
                let port = lines.next().unwrap().to_string();
                let path = lines.next().unwrap();
                let (socket, _) =
                    tungstenite::connect(format!("ws://127.0.0.1:{port}{path}")).unwrap();
                (socket, port)
            });
            if closed_stderr {
                // The suite has announced its live browser and leash. Lose the output reader
                // before killing the suite, as when a piped cargo test loses its consumer.
                drop(suite.0.stderr.take().expect("piped suite stderr reader"));
            }
            let input = suite.0.stdin.take(); // Kept to the reap: Child::wait closes it first.
            if let Some(expected) = expected {
                assert!(
                    Command::new("/bin/kill")
                        .args([signal, &suite.0.id().to_string()])
                        .status()
                        .unwrap()
                        .success()
                );
                assert_eq!(suite.0.wait().unwrap().signal(), Some(expected));
            } else {
                drop(input);
                assert!(suite.0.wait().unwrap().success());
            }
            reader.join().unwrap();
            // Observe protocol closure, then require the whole recorded group to be gone below.
            // A closing WebSocket alone can precede the browser releasing its listening socket.
            if let Some((socket, _)) = &mut protocol {
                wait_for_protocol_eof(socket)
                    .unwrap_or_else(|error| panic!("browser protocol after {signal}: {error}"));
            }
            loop {
                // SAFETY: the suite announced this exact leash PID at startup. Signal zero
                // only queries it, including its zombie until the parent reaps it.
                if unsafe { libc::kill(pid(leash), 0) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(3) {
                        break;
                    }
                    assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");
                }
                std::thread::sleep(POLL);
            }
            // The leash's exit is the boundary under test. Waiting here for a leaked
            // helper/profile would hide a premature exit instead of failing the case.
            assert_eq!(
                signal_group(group, 0)
                    .err()
                    .and_then(|error| error.raw_os_error()),
                Some(3),
                "browser group outlived its leash after {signal}"
            );
            assert!(
                !profile.exists(),
                "profile outlived its leash after {signal}"
            );
            for extension in ["stderr", "exit"] {
                assert!(
                    !profile.with_extension(extension).exists(),
                    "{extension} log outlived its leash after {signal}"
                );
            }
            // An ended suite took its folder with it; the next run's sweep takes a killed one's.
            profiles::sweep();
            let folder = profile.parent().unwrap();
            assert!(!folder.exists(), "suite's folder outlived {signal}");
            sweep.0 = None;
            if let Some((_, port)) = &protocol {
                assert!(
                    TcpStream::connect(format!("127.0.0.1:{port}")).is_err(),
                    "DevTools survived {signal}"
                );
            }
        }
    }
}

#[test]
fn startup_requires_complete_readiness_and_observes_leash_exit() {
    let profile = next_profile().unwrap();
    std::fs::create_dir(&profile).unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", "read -r release"])
        .stdin(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id();
    let release = child.stdin.take().unwrap();
    let owner = Owner {
        leash: child,
        profile: profile.clone(),
        stderr: std::fs::File::open("/dev/null").unwrap(),
        shutdown: std::fs::File::open("/dev/null").unwrap(),
    };
    // These are actual partial/invalid writes at the file boundary Chrome uses.
    for (pid, port) in [
        (
            "123",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        ("123\n", "1234\n"),
        (
            "123\n",
            "0\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        (
            "123\n",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789ab",
        ),
    ] {
        std::fs::write(profile.join("browser.pid"), pid).unwrap();
        std::fs::write(profile.join("DevToolsActivePort"), port).unwrap();
        assert!(devtools_endpoint(&profile).is_none());
    }
    // The owned child is idle on its pipe; its wall wait cannot spend CPU credit.
    std::fs::write(profile.join("browser.pid"), format!("{group}\n")).unwrap();
    let waiter = std::thread::spawn(move || {
        let mut owner = owner;
        let endpoint = owner
            .ready("complete test port", PROTOCOL_CPU, devtools_endpoint)
            .unwrap();
        (owner, endpoint)
    });
    std::fs::write(
        profile.join("DevToolsActivePort"),
        "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
    )
    .unwrap();
    let (mut owner, endpoint) = waiter.join().unwrap();
    assert_eq!(
        endpoint,
        (
            "1234".into(),
            "/devtools/browser/12345678-1234-1234-1234-123456789abc".into()
        )
    );
    std::fs::remove_file(profile.join("DevToolsActivePort")).unwrap();
    drop(release);
    assert!(!owner.leash.wait().unwrap().success());
    let error = owner
        .ready("missing port after exit", PROTOCOL_CPU, devtools_endpoint)
        .unwrap_err();
    assert!(error.to_string().contains("exited"), "{error}");
    drop(owner);
    assert!(!profile.exists());

    // The real leash owns a stand-in that burns CPU without publishing a port.
    let mut stand_in = StandIn::new("#!/bin/sh\nwhile :; do :; done\n");
    let group = stand_in.group();
    let (done, watcher) = cpu_watchdog(group, Duration::from_secs(1));
    let result = stand_in.owner.ready(
        "missing stand-in port",
        Duration::from_millis(50),
        devtools_endpoint,
    );
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a spinning browser without a port must fail startup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(
        error
            .to_string()
            .contains("CPU without missing stand-in port"),
        "{error}"
    );
    eprintln!("spinning startup group={group}: {error}; watchdog killed={forced}");
}

#[test]
fn browser_waits_keep_events_and_bound_idle_polls_without_charging_observer_delay() {
    use tungstenite::protocol::Role;

    // Exercise the actual transport and kernel meter with an idle owned process.
    // Chrome's background startup work is legitimate CPU and cannot serve as an
    // idle fixture for a deliberately small allowance.
    let profile = next_profile().unwrap();
    std::fs::create_dir(&profile).unwrap();
    let idle = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let peer = std::thread::spawn(move || {
        let mut socket = WebSocket::from_raw_socket(peer.try_clone().unwrap(), Role::Server, None);
        let (send, receive) = std::sync::mpsc::channel::<Value>();
        let writer = std::thread::spawn(move || {
            let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
            for value in receive {
                if socket.send(Message::text(value.to_string())).is_err() {
                    break;
                }
            }
        });
        let mut evaluations = 0;
        let mut idle_evaluations = 0;
        let mut pending_markers = 0;
        let mut scripted = Vec::<std::ops::RangeInclusive<u64>>::new();
        while let Ok(Message::Text(text)) = socket.read() {
            let command: Value = serde_json::from_str(text.as_str()).unwrap();
            assert_eq!(command["method"], "Runtime.evaluate");
            let id = command["id"].as_u64().unwrap();
            if scripted.iter().any(|range| range.contains(&id)) {
                assert_eq!(command["sessionId"], "observer");
                pending_markers += 1;
                continue;
            }
            if matches!(
                command["params"]["expression"].as_str(),
                Some("pending-wait" | "pending-eval")
            ) {
                let polls = if command["params"]["expression"] == "pending-wait" {
                    2
                } else {
                    2400
                };
                scripted.push(id + 1..=id + polls);
                send.send(json!({"method":"Fixture.observed", "params":command["params"]}))
                    .unwrap();
                // A finite wire transcript also fails promptly if the driver stops
                // observing: after these turns the pending expression becomes true.
                // Only the driver's allowance may reject it before that late reply.
                for marker in id + 1..=id + polls {
                    send.send(json!({"id":marker,"result":{"result":{"value":null}}}))
                        .unwrap();
                }
                send.send(json!({"id":id,"result":{"result":{"value":true}}}))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == "protocol-error" {
                send.send(json!({"id":id,"error":{"message":"fixture protocol error"}}))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == "evaluation-error" {
                send.send(json!({"id":id,"result":{"exceptionDetails":{"text":"fixture evaluation error"}}})).unwrap();
                continue;
            }
            let value = if command["params"]["expression"] == "ready" {
                evaluations += 1;
                if evaluations == 1 {
                    // Reply only after this owned timer process exits. The first
                    // result is always false after >100 ms wall time, with no
                    // work charged to the idle process's 100 ms CPU allowance.
                    assert!(
                        Command::new("/bin/sleep")
                            .arg("0.25")
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                json!(evaluations > 1)
            } else if command["params"]["expression"] == "never-ready" {
                idle_evaluations += 1;
                // Close the transport on excess retries so a regression fails promptly
                // even though the owned process stays idle and its CPU never advances.
                assert!(
                    idle_evaluations <= 3,
                    "idle wait exceeded its poll allowance"
                );
                json!(false)
            } else if command["params"]["expression"] == "after-timeout" {
                json!("usable")
            } else {
                // Acknowledge pump's observation marker between evaluations.
                Value::Null
            };
            send.send(json!({
                "id": command["id"], "result": {"result": {"value": value}}
            }))
            .unwrap();
        }
        drop(send);
        writer.join().unwrap();
        (evaluations, idle_evaluations, pending_markers)
    });
    let mut controlled = Chrome {
        owner: Owner {
            leash: idle,
            profile,
            stderr: std::fs::File::open("/dev/null").unwrap(),
            shutdown: std::fs::File::open("/dev/null").unwrap(),
        },
        socket: WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
        next_id: 0,
        session: Some("page".into()),
        observer: Some("observer".into()),
        events: Vec::new(),
        arrivals: Vec::new(),
        deadline: None,
    };
    assert!(
        controlled
            .wait_for("ready", Duration::from_millis(100))
            .unwrap()
    );
    assert!(!controlled.wait_for("never-ready", Duration::ZERO).unwrap());
    assert!(
        !controlled
            .wait_for("never-ready", Duration::from_millis(26))
            .unwrap()
    );
    assert!(
        !controlled
            .wait_for("pending-wait", Duration::from_millis(26))
            .unwrap()
    );
    let why = controlled
        .eval("pending-eval")
        .expect_err("plain eval must bound a promise that outlives all observer turns")
        .to_string();
    assert!(why.contains("2400 completed observer polls"), "{why}");
    assert_eq!(controlled.eval("after-timeout").unwrap(), json!("usable"));
    for (expression, expected) in [
        ("protocol-error", "fixture protocol error"),
        ("evaluation-error", "fixture evaluation error"),
    ] {
        let why = controlled
            .wait_for(expression, Duration::ZERO)
            .expect_err("an evaluation failure must not become a false condition")
            .to_string();
        assert!(why.contains(expected), "{why}");
    }
    assert!(
        controlled
            .events
            .iter()
            .any(|event| event["method"] == "Fixture.observed"
                && event["params"]["expression"] == "pending-eval")
    );
    drop(controlled);
    assert_eq!(peer.join().unwrap(), (2, 3, 2402));

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open("data:text/html,<p>ready</p>").unwrap();
    // open consumed the event from the wire but kept it for later observers. Even
    // a zero CPU allowance must accept an event/condition that already succeeded.
    chrome
        .wait_event("Page.domContentEventFired", Duration::ZERO)
        .unwrap();
    assert!(
        chrome
            .wait_for("document.querySelector('p')", Duration::ZERO)
            .unwrap()
    );
    assert!(
        !chrome
            .wait_for("new Promise(() => {})", Duration::from_millis(26))
            .unwrap()
    );
    assert!(
        !chrome
            .wait_for("Promise.resolve(false)", Duration::ZERO)
            .unwrap()
    );
    assert!(
        chrome
            .wait_for("Promise.resolve(true)", Duration::ZERO)
            .unwrap()
    );
    chrome.eval("window.idlePolls=0").unwrap();
    assert!(
        !chrome
            .wait_for(
                "(window.idlePolls++, document.querySelector('#missing'))",
                Duration::from_millis(26)
            )
            .unwrap()
    );
    let polls = chrome.eval("window.idlePolls").unwrap().as_u64().unwrap();
    assert!(
        (1..=2).contains(&polls),
        "idle page evaluated {polls} times"
    );
    // A false condition that itself does renderer work must also exhaust its allowance.
    assert!(!chrome
        .wait_for(
            "(() => { const until=performance.now()+50; while(performance.now()<until) {} return false; })()",
            Duration::from_millis(1)
        )
        .unwrap());
    chrome
        .eval("window.observed=false; setTimeout(()=>{window.observed=true}, 5)")
        .unwrap();
    chrome.pump(Duration::from_millis(10)).unwrap();
    assert!(
        chrome
            .wait_for("window.observed === true", PROTOCOL_CPU)
            .unwrap()
    );
    chrome
        .eval("setTimeout(() => location.href='about:blank#next', 10)")
        .unwrap();
    // Page navigation may discard its own promises. The observation marker's
    // session belongs to the stable blank target and must still answer.
    chrome.pump(Duration::from_millis(50)).unwrap();
    assert!(
        chrome
            .wait_for("location.href === 'about:blank#next'", PROTOCOL_CPU)
            .unwrap()
    );
    chrome.open("data:text/html,<p>after</p>").unwrap();
    assert_eq!(
        chrome
            .eval("document.querySelector('p').textContent")
            .unwrap(),
        json!("after")
    );
}

#[test]
fn navigation_and_protocol_replies_end_on_completed_observer_turns() {
    use tungstenite::protocol::Role;

    for (navigation, observer) in [(true, true), (false, true), (false, false)] {
        let profile = next_profile().unwrap();
        std::fs::create_dir(&profile).unwrap();
        let idle = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        let peer = std::thread::spawn(move || {
            let mut reader =
                WebSocket::from_raw_socket(peer.try_clone().unwrap(), Role::Server, None);
            let (send, receive) = std::sync::mpsc::channel::<Value>();
            let writer = std::thread::spawn(move || {
                let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
                for value in receive {
                    if socket.send(Message::text(value.to_string())).is_err() {
                        break;
                    }
                }
            });
            let mut held = false;
            let mut polls = 0;
            while let Ok(Message::Text(text)) = reader.read() {
                let command: Value = serde_json::from_str(text.as_str()).unwrap();
                let id = command["id"].as_u64().unwrap();
                if !held {
                    assert_eq!(
                        command["method"],
                        if navigation {
                            "Page.navigate"
                        } else {
                            "Fixture.held"
                        }
                    );
                    held = true;
                    if navigation {
                        // Navigation is accepted, but its document never finishes parsing
                        // within the allowance. open must fail on the missing event.
                        send.send(json!({"id":id,"result":{}})).unwrap();
                    }
                    send.send(json!({"method":"Fixture.unrelated"})).unwrap();
                    // A finite transcript supplies the late success even if observation
                    // is removed. That regression returns successfully instead of hanging
                    // the test, and must fail the expect_err below. Replies are queued on
                    // a separate writer so observer requests cannot fill both TCP buffers.
                    for marker in id + 1..=id + 2401 {
                        send.send(json!({"id":marker,"result":{}})).unwrap();
                    }
                    send.send(if navigation {
                        json!({"method":"Page.domContentEventFired"})
                    } else {
                        json!({"id":id,"result":{}})
                    })
                    .unwrap();
                } else {
                    assert_eq!(
                        command["method"],
                        if observer {
                            "Runtime.evaluate"
                        } else {
                            "Target.getTargets"
                        }
                    );
                    if observer {
                        assert_eq!(command["sessionId"], "observer");
                    } else {
                        assert!(command.get("sessionId").is_none());
                    }
                    polls += 1;
                }
            }
            drop(send);
            writer.join().unwrap();
            (held, polls)
        });
        let mut chrome = Chrome {
            owner: Owner {
                leash: idle,
                profile,
                stderr: std::fs::File::open("/dev/null").unwrap(),
                shutdown: std::fs::File::open("/dev/null").unwrap(),
            },
            socket: WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
            next_id: 0,
            session: Some("page".into()),
            observer: observer.then(|| "observer".into()),
            events: Vec::new(),
            arrivals: Vec::new(),
            deadline: None,
        };
        let started = Instant::now();
        let why = if navigation {
            chrome
                .open("https://fixture.invalid/held")
                .map(|()| Value::Null)
        } else {
            chrome.call("Fixture.held", json!({}))
        }
        .expect_err("a late event or reply must exhaust completed observer turns")
        .to_string();
        assert!(
            why.contains(if navigation {
                "Page.domContentEventFired"
            } else {
                "Fixture.held"
            }),
            "{why}"
        );
        assert!(why.contains("2400 completed observer polls"), "{why}");
        assert!(
            chrome
                .events
                .iter()
                .any(|event| event["method"] == "Fixture.unrelated")
        );
        eprintln!(
            "navigation={navigation} observer={observer} bounded in {:?}: {why}",
            started.elapsed()
        );
        drop(chrome);
        assert_eq!(
            peer.join().unwrap(),
            (true, if navigation { 2401 } else { 2400 })
        );
    }
}

#[test]
fn cpu_exhaustion_stops_renderer_work_before_the_poll_allowance() {
    use tungstenite::protocol::Role;

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open("data:text/html,<p>CPU fixture</p>").unwrap();
    chrome.eval("window.cpuPolls=0").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let mut renderer = std::mem::replace(
        &mut chrome.socket,
        WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
    );
    let observer = chrome.observer.clone().unwrap();
    let mut renderer_id = chrome.next_id;
    const WORK: &str = "(() => {window.cpuPolls++; let checksum=2166136261; for(let i=0;i<500000000;i++) {checksum=Math.imul(checksum ^ i,16777619);} window.cpuChecksum=checksum; return false;})()";
    let peer = std::thread::spawn(move || {
        let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
        let mut evaluations = 0;
        while let Ok(Message::Text(text)) = socket.read() {
            let mut command: Value = serde_json::from_str(text.as_str()).unwrap();
            let id = command["id"].clone();
            if command["sessionId"] == observer {
                // Observer replies wait behind the renderer's fixed work in this
                // transport. They cannot spend the allowance while work is pending.
                socket
                    .send(Message::text(
                        json!({"id":id,"result":{"result":{"value":null}}}).to_string(),
                    ))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == WORK {
                evaluations += 1;
                assert!(evaluations <= 4, "renderer exceeded its poll allowance");
            }
            renderer_id += 1;
            command["id"] = json!(renderer_id);
            renderer.send(Message::text(command.to_string())).unwrap();
            loop {
                let text = match renderer.read() {
                    Ok(Message::Text(text)) => text,
                    Ok(_) => continue,
                    Err(tungstenite::Error::Io(error))
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        continue;
                    }
                    Err(error) => panic!("renderer relay: {error}"),
                };
                let mut reply: Value = serde_json::from_str(text.as_str()).unwrap();
                if reply["id"] == json!(renderer_id) {
                    reply["id"] = id;
                    socket.send(Message::text(reply.to_string())).unwrap();
                    break;
                }
            }
        }
        evaluations
    });
    assert!(!chrome.wait_for(WORK, Duration::from_millis(100)).unwrap());
    let polls = chrome.eval("window.cpuPolls").unwrap().as_u64().unwrap();
    assert!(
        (1..4).contains(&polls),
        "CPU must end the wait before four polls, got {polls}"
    );
    assert_eq!(
        chrome
            .eval("typeof window.cpuChecksum === 'number'")
            .unwrap(),
        json!(true)
    );
    drop(chrome);
    assert_eq!(peer.join().unwrap(), polls);
}

#[test]
fn requests_follow_each_id_through_cache_failure_and_cancellation() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"1","type":"Fetch","request":{"url":"http://a/api"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"2","type":"Script","request":{"url":"http://a/s.js"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"3","type":"Fetch","request":{"url":"http://a/gone"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"4","type":"Fetch","request":{"url":"http://a/aborted"}}),
        ),
        // An event naming no request, and a request whose start was never seen, are left out.
        event("Page.loadEventFired", json!({})),
        event(
            "Network.loadingFinished",
            json!({"requestId":"9","encodedDataLength":5}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId":"1","response":{"status":404}}),
        ),
        event(
            "Network.dataReceived",
            json!({"requestId":"1","encodedDataLength":10}),
        ),
        event(
            "Network.loadingFinished",
            json!({"requestId":"1","encodedDataLength":12}),
        ),
        event("Network.requestServedFromCache", json!({"requestId":"2"})),
        event(
            "Network.loadingFailed",
            json!({"requestId":"3","errorText":"net::ERR_CONNECTION_REFUSED"}),
        ),
        event(
            "Network.loadingFailed",
            json!({"requestId":"4","errorText":"net::ERR_ABORTED","canceled":true}),
        ),
    ];
    let requests = crate::requests(&events);
    let urls: Vec<_> = requests
        .iter()
        .map(|request| request.url.as_str())
        .collect();
    assert_eq!(
        urls,
        [
            "http://a/api",
            "http://a/s.js",
            "http://a/gone",
            "http://a/aborted"
        ]
    );
    let [api, script, gone, aborted] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!((api.status, api.bytes, api.finished), (404, 12.0, true));
    assert!(api.failed.is_none() && !api.canceled);
    assert!(script.from_cache && !script.finished && script.failed.is_none());
    assert_eq!(gone.failed.as_deref(), Some("net::ERR_CONNECTION_REFUSED"));
    assert!(!gone.canceled && !gone.finished);
    assert_eq!(aborted.failed.as_deref(), Some("net::ERR_ABORTED"));
    assert!(aborted.canceled);
}

#[test]
fn an_open_stream_counts_its_head_and_every_chunk_read_and_names_its_pings() {
    // As Chrome reported a stand-in feed: the head, `: open`, two pings and a delta. Each
    // chunk's encodedDataLength is the previous chunk's, so it never names the last one.
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let chunk = |data: u32, encoded: u32| {
        event(
            "Network.dataReceived",
            json!({"requestId": "feed", "dataLength": data, "encodedDataLength": encoded}),
        )
    };
    let message = |name: &str| {
        event(
            "Network.eventSourceMessageReceived",
            json!({"requestId": "feed", "eventName": name, "data": ""}),
        )
    };
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "feed", "type": "EventSource", "request": {"url": "http://h/api/feed"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "glue", "type": "Script", "request": {"url": "http://h/a.js"}}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId": "feed", "response": {"status": 200, "encodedDataLength": 77}}),
        ),
        chunk(8, 0),
        event(
            "Network.dataReceived",
            json!({"requestId": "glue", "dataLength": 900, "encodedDataLength": 300}),
        ),
        event(
            "Network.loadingFinished",
            json!({"requestId": "glue", "encodedDataLength": 512}),
        ),
        chunk(19, 8),
        message(symphony_api::PING_EVENT),
        chunk(19, 19),
        message(symphony_api::PING_EVENT),
        chunk(45, 19),
        message(symphony_api::FEED_EVENT),
    ];
    let requests = crate::requests(&events);
    let [feed, glue] = requests.as_slice() else {
        panic!("two requests: {requests:#?}")
    };
    assert_eq!(
        (feed.kind.as_str(), feed.status, feed.bytes, feed.pings),
        ("EventSource", 200, 77.0 + 8.0 + 19.0 + 19.0 + 45.0, 2)
    );
    assert!(!feed.finished);
    assert_eq!((glue.bytes, glue.pings, glue.finished), (512.0, 0, true));
}

#[test]
fn a_page_that_cannot_load_fails_navigate_naming_why_but_still_opens() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    // A port this test bound and let go: nothing listens there now.
    let refused = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    let why = chrome
        .navigate(&refused, PROTOCOL_CPU)
        .unwrap_err()
        .to_string();
    assert!(why.contains("ERR_CONNECTION_REFUSED"), "{why}");
    assert!(why.contains(&refused), "{why}");
    chrome.open(&refused).unwrap();

    // A server that takes the connection and never answers holds the page within the allowance.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let held = format!("http://{}/", silent.local_addr().unwrap());
    let holder = std::thread::spawn(move || {
        let (connection, _) = silent.accept().unwrap();
        connection
    });
    let started = Instant::now();
    let why = chrome
        .navigate(&held, Duration::from_millis(500))
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("Page.navigate") || why.contains("Page.domContentEventFired"),
        "{why}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "a 500 ms allowance held the page for {:?}",
        started.elapsed()
    );
    drop(holder.join().unwrap());

    let navigated = Instant::now();
    chrome
        .navigate(
            "data:text/html,<p style='margin:0;height:40px;background:rgb(255,0,0)'>",
            PROTOCOL_CPU,
        )
        .unwrap();
    // This page's events, each with when it was read, in order; the earlier pages' are forgotten.
    let read: Vec<(Instant, &Value)> = chrome.events_read().collect();
    assert_eq!(read.len(), chrome.events().len());
    assert!(
        read.iter()
            .any(|(_, event)| event["method"] == "Page.domContentEventFired"),
        "{read:?}"
    );
    assert!(read.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    let now = Instant::now();
    assert!(
        read.iter().all(|(at, _)| *at >= navigated && *at <= now),
        "{read:?}"
    );
    let picture = chrome
        .screenshot(json!({"clip": {"x": 0, "y": 0, "width": 30, "height": 20, "scale": 1}}))
        .unwrap();
    assert_eq!(&picture[..8], b"\x89PNG\r\n\x1a\n");
    // The IHDR chunk's width and height, big-endian, after its length and name.
    let width = u32::from_be_bytes(picture[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(picture[20..24].try_into().unwrap());
    assert_eq!((width, height), (30, 20));
}

#[test]
fn a_deadline_ends_every_wait_soon_after_it_with_an_error_that_says_so() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome
        .navigate("data:text/html,<p>still</p>", PROTOCOL_CPU)
        .unwrap();
    // Without a deadline each of these would wait out the protocol's own allowance: a minute of
    // browser CPU or as many observation turns, at least a minute.
    const NEVER: &str = "new Promise(() => {})";
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let held = format!("http://{}/", silent.local_addr().unwrap());
    type Wait = fn(&mut Chrome, &str) -> crate::Result<()>;
    let waits: [(&str, Wait); 3] = [
        ("an evaluation", |chrome, _| chrome.eval(NEVER).map(drop)),
        ("a protocol call", |chrome, _| {
            chrome
                .call(
                    "Runtime.evaluate",
                    json!({"expression": NEVER, "awaitPromise": true}),
                )
                .map(drop)
        }),
        ("a navigation", |chrome, held| {
            chrome.navigate(held, PROTOCOL_CPU)
        }),
    ];
    for (wait, run) in waits {
        let started = Instant::now();
        chrome.set_deadline(Some(started + Duration::from_millis(500)));
        let error = run(&mut chrome, &held).unwrap_err();
        let took = started.elapsed();
        assert!(error.past_deadline(), "{wait}: {error}");
        assert!(
            error.to_string().contains("no reply before the deadline"),
            "{wait}: {error}"
        );
        assert!(
            took >= Duration::from_millis(500) && took < Duration::from_secs(30),
            "{wait} ended {took:?} after its start, its deadline 500 ms"
        );
    }
    // Lifted, the waits are the protocol's own again: the page still answers.
    chrome.set_deadline(None);
    chrome
        .navigate("data:text/html,<p>again</p>", PROTOCOL_CPU)
        .unwrap();
    assert_eq!(
        chrome.eval("document.body.textContent").unwrap(),
        json!("again")
    );
    // A failure of Chrome's own is no deadline.
    let thrown = chrome
        .eval("(() => { throw new Error('own') })()")
        .unwrap_err();
    assert!(!thrown.past_deadline(), "{thrown}");
    drop(silent);
}
