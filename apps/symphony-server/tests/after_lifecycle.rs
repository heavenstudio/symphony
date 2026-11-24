//! Lifecycle and `just symphony` regressions at the real native executable boundary.
//! Builds are fixture commands; the recipe, sockets, records, recovery and shutdown are real.
//! Every server has a scratch repository, an OS-assigned port and the owning test's lifetime pid.

#[path = "after_lifecycle/fixture_sdk.rs"]
mod fixture;

#[path = "common/bash_env.rs"]
mod bash_env;

#[path = "after_lifecycle/discovery_tests.rs"]
mod discovery_tests;

#[path = "after_lifecycle/projects_tests.rs"]
mod projects_tests;

#[path = "after_lifecycle/private_tests.rs"]
mod private_tests;

#[path = "after_lifecycle/board_schema_tests.rs"]
mod board_schema_tests;

#[path = "after_lifecycle/release_tests.rs"]
mod release_tests;

#[path = "after_lifecycle/worktree_tests.rs"]
mod worktree_tests;

use fixture::{Repo, answer, front, get, repository, status, until, write};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::sys::{Command, Stdio};
use symphony_server::home::{Home, ServerError};

#[test]
fn bash_env_does_not_add_output_to_build_fixtures() {
    bash_env::run("bash_env_does_not_add_output_to_build_fixtures", || {
        let repo = Repo::new("bash-env");
        let mut command = Command::new(repo.main.join("tools/fixture-build"));
        command
            .current_dir(&repo.tree)
            .env("SYMPHONY_LIFECYCLE_BINARY", fixture::APP)
            .env("SYMPHONY_LIFECYCLE_LOG", &repo.builds)
            .args(["slots", "cargo", "build"]);
        let mut process = repo.run(command);
        process.success();
        assert_eq!(process.output(), "");
        assert_eq!(repo.build_events().len(), 1);
    });
}

#[test]
fn build_fixtures_reuse_the_built_image_and_replace_old_outputs_without_writing_through_links() {
    let repo = Repo::new("build-image");
    // Commands also share their executable images, including the checkout made by git, and with
    // every other checkout on the machine. Copying fresh scripts per case, or linking a verify
    // run's freshly checked-out sources, sends each one through first-exec validation on a loaded
    // macOS host.
    for (source, command) in [
        ("build.sh", repo.main.join("tools/fixture-build")),
        ("build.sh", repo.tree.join("tools/rs-bin.sh")),
        ("ps.sh", repo.bin.join("ps")),
        ("open.sh", repo.bin.join("open")),
        ("open.sh", repo.bin.join("xdg-open")),
    ] {
        let source = fixture::script_source(source);
        let shared = fs::metadata(fixture::machine_copy::machine_copy(&source)).unwrap();
        let installed = fs::metadata(&command).unwrap();
        assert_eq!(
            (installed.dev(), installed.ino()),
            (shared.dev(), shared.ino()),
            "fixture commands must reuse the machine's copy of the executable source"
        );
        assert_eq!(fs::read(&command).unwrap(), fs::read(&source).unwrap());
    }
    let original = fs::metadata(fixture::APP).unwrap();
    for install in [false, true] {
        let out = if install {
            repo.install.join("bin/symphony-server")
        } else {
            repo.tree.join(".target/debug/symphony-server")
        };
        write(&out, "old build output\n");
        // The second invocation sees an existing link, as a stopped main's rebuild does.
        for _ in 0..2 {
            let mut command = Command::new(repo.main.join("tools/fixture-build"));
            command
                .current_dir(&repo.tree)
                .env("SYMPHONY_LIFECYCLE_BINARY", fixture::APP)
                .env("SYMPHONY_LIFECYCLE_LOG", &repo.builds)
                .args(["slots", "cargo"]);
            if install {
                command.arg("install").arg("--root").arg(&repo.install);
            } else {
                command.arg("build");
            }
            repo.run(command).success();
            let built = fs::metadata(&out).unwrap();
            assert_eq!((built.dev(), built.ino()), (original.dev(), original.ino()));
            assert_eq!(built.len(), original.len());
            assert_eq!(built.modified().unwrap(), original.modified().unwrap());
        }
    }
    assert_eq!(repo.build_events().len(), 4);
}

#[test]
fn a_start_beside_a_server_on_loopback_alone_fails_at_once_and_leaves_nothing() {
    for host in ["127.0.0.1", "::1"] {
        let mut repo = Repo::new("occupied");
        let listener = match TcpListener::bind((host, 0)) {
            Ok(listener) => listener,
            Err(error) if host == "::1" && error.kind() == std::io::ErrorKind::AddrNotAvailable => {
                eprintln!("IPv6 unavailable on this machine: {error}");
                continue;
            }
            Err(error) => panic!("cannot own {host}: {error}"),
        };
        repo.port = Some(listener.local_addr().unwrap().port());
        assert_ne!(repo.port(), 8765);
        let mut command = repo.command(&repo.main);
        if host == "::1" {
            // The default server binds IPv4 first. An independently occupied IPv4 port must not
            // prevent this case from exercising the IPv6 listener we actually own. A server of its
            // own is refused the one server's port, so the one server is on a port of its pick.
            command.env("SYMPHONY_PORT", "0").args([
                "--port",
                &repo.port().to_string(),
                "--host",
                host,
            ]);
        }
        let mut contender = repo.run(command);
        assert_eq!(contender.wait().code(), Some(2), "{}", contender.output());
        let log = if host == "::1" {
            contender.output()
        } else {
            fs::read_to_string(repo.home.join("server.log")).unwrap()
        };
        let attempted = if cfg!(target_os = "linux") && host == "127.0.0.1" {
            "0.0.0.0"
        } else {
            host
        };
        assert!(
            log.contains(&format!("{attempted} port {}", repo.port())),
            "{log}"
        );
        assert!(log.contains("could not listen"), "{log}");
        assert!(repo.record().is_none());
        assert!(!repo.main.join("artifacts/docs-app.port").exists());
        // The contender has exited before the original listener is released. In particular this
        // never finds a free port, drops its owner, and tries to reacquire that port for the test.
        assert_eq!(listener.local_addr().unwrap().port(), repo.port());
        assert!(!symphony_server::sys::pid_alive(i64::from(
            contender.child.id()
        )));
    }
}

#[test]
fn concurrent_starts_from_main_and_a_worktree_publish_one_native_server() {
    let repo = Repo::new("concurrent");
    // Spawn all contenders before waiting on any. Each prints the pid it actually found; requiring
    // that same pid in every result catches duplicate starts without scanning machine processes.
    // The worktree's are the owner's `--shared` starts, which may start the one server too.
    let mut starts: Vec<_> = (0..8)
        .map(|n| {
            let mut command = repo.command(if n % 2 == 0 { &repo.main } else { &repo.tree });
            if n % 2 == 1 {
                command.arg("--shared");
            }
            (n, repo.run(command))
        })
        .collect();
    for (_, child) in &mut starts {
        child.success();
    }
    repo.remember();
    let actual = repo.check_server();
    for (n, child) in starts {
        let page = if n % 2 == 0 {
            "/"
        } else {
            "/@feature~lifecycle/"
        };
        let page = repo.page(repo.port(), page);
        assert!(
            child
                .output()
                .contains(&format!("http://127.0.0.1:{}{page}", repo.port())),
            "{}",
            child.output()
        );
        assert!(
            child.output().contains(&format!("pid {}", actual.pid)),
            "{}",
            child.output()
        );
    }
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert_eq!(log.matches("→ Symphony at ").count(), 1, "{log}");
    for line in log.lines() {
        assert!(
            line.starts_with('[') && line.get(24..27) == Some("Z] "),
            "undated server output: {line}"
        );
    }
    assert!(TcpListener::bind(("127.0.0.1", repo.port())).is_err());
}

#[test]
fn missing_malformed_stale_and_lying_records_recover_the_same_running_server() {
    let repo = Repo::new("record recovery");
    repo.start(&repo.main, false);
    let running = repo.record().unwrap();
    let mut dead = Command::new("/usr/bin/true").spawn().unwrap();
    let stale_pid = dead.id();
    dead.wait().unwrap();
    let record = |pid, port, root: &str| {
        serde_json::json!({"pid": pid, "port": port, "root": root}).to_string()
    };
    for contents in [
        None,
        Some(String::new()),
        Some("{malformed".into()),
        Some(record(i64::from(stale_pid), running.port, &running.root)),
        Some(record(running.pid, 0, &running.root)),
        Some(record(running.pid, running.port, "/wrong root")),
        Some(record(
            i64::from(std::process::id()),
            running.port,
            &running.root,
        )),
    ] {
        let address = repo.home.join("server.json");
        fs::remove_file(&address).unwrap();
        if let Some(contents) = contents {
            fs::write(&address, contents).unwrap();
        }
        repo.start(&repo.main, false);
        assert_eq!(repo.record(), Some(running.clone()));
        assert_eq!(repo.check_server().pid, running.pid);
    }
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert_eq!(
        log.matches("→ Symphony at ").count(),
        1,
        "recovery restarted the server: {log}"
    );
}

#[test]
fn a_record_naming_an_unrelated_owned_process_never_makes_cleanup_signal_it() {
    let repo = Repo::new("foreign-record-cleanup");
    // It reads its input, which this test holds open to the end: it never ends by itself.
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "read -r line"]).stdin(Stdio::piped());
    let mut unrelated = repo.run(command);
    let _held = unrelated.child.stdin.take().unwrap();
    write(
        &repo.home.join("server.json"),
        &serde_json::json!({
            "pid": unrelated.child.id(), "port": 1, "root": repo.main
        })
        .to_string(),
    );
    // A corrupt record is input, never proof that the fixture started that pid as a server.
    repo.remember();
    repo.stop();
    assert!(!repo.home.join("server.json").exists());
    repo.cleanup();
    assert!(
        unrelated.child.try_wait().unwrap().is_none(),
        "cleanup signalled a foreign record's pid"
    );
    // The record is foreign to Repo, but this Process owns the child. Its drop must end and
    // reap that exact child before returning: the child, whose input is held, would never end.
    let pid = libc::pid_t::try_from(unrelated.child.id()).unwrap();
    let dropping = std::thread::spawn(move || drop(unrelated));
    let dropped = test_time::until_true(
        "Process::drop to end its child rather than wait for it",
        fixture::BACKSTOP,
        || dropping.is_finished(),
    );
    let mut status = 0;
    // SAFETY: the recorded pid is this test's child; WNOHANG only observes/reaps that child.
    let observed = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    let error = (observed == -1)
        .then(std::io::Error::last_os_error)
        .and_then(|error| error.raw_os_error());
    if observed == 0 {
        // Preserve the failed observation before cleanup so a broken drop cannot leak a child
        // or be hidden by this fallback. A positive waitpid result already reaped the child.
        fixture::killed::send(
            i64::from(pid),
            libc::SIGKILL,
            "the case ends the child a Process drop left",
        );
        // SAFETY: the nonblocking observation proved this exact owned child is still running.
        while unsafe { libc::waitpid(pid, &mut status, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
        {}
    }
    if let Err(stall) = dropped {
        panic!("{stall}");
    }
    if let Err(failure) = dropping.join() {
        std::panic::resume_unwind(failure);
    }
    assert_eq!(
        (observed, error),
        (-1, Some(libc::ECHILD)),
        "Process::drop left its child {pid} alive or unreaped"
    );
}

#[test]
fn a_killed_shared_server_is_replaced_by_one_that_answers() {
    let repo = Repo::new("killed-shared");
    let check_progress = |process: fixture::Process| {
        assert!(
            !repo
                .base
                .join(format!("env.{}", process.child.id()))
                .exists(),
            "a start on an unheld known port read the process table: {}",
            process.output()
        );
        let said = process.output();
        for stage in [
            "start command entered at ",
            "begin checkout discovery",
            "end checkout discovery:",
            "begin startup lock",
            "end startup lock:",
            "begin unrecorded server discovery",
            "end unrecorded server discovery:",
            "begin spawn detached server",
            "end spawn detached server:",
            "end ensure shared server:",
        ] {
            assert!(
                said.contains(stage),
                "missing startup progress {stage}: {said}"
            );
        }
    };
    check_progress(repo.start(&repo.main, false));
    let first = repo.record().unwrap();
    fixture::killed::send(first.pid, libc::SIGKILL, "the case kills the shared server");
    until("killed shared server", || {
        (!symphony_server::sys::pid_alive(first.pid)).then_some(())
    });
    check_progress(repo.start(&repo.main, false));
    let second = repo.check_server();
    assert_ne!(second.pid, first.pid);
}

#[test]
fn shared_starts_report_and_ignore_in_repository_root_and_host_overrides() {
    let repo = Repo::new("ignored-options");
    // Main's start starts the one server; the worktree's finds it (`worktree_tests`).
    for dir in [&repo.main, &repo.tree] {
        let mut command = repo.command(dir);
        command.args([
            "--root",
            repo.tree.to_str().unwrap(),
            "--host",
            "invalid-host",
        ]);
        let mut start = repo.run(command);
        start.success();
        repo.remember();
        assert!(
            start
                .output()
                .contains("are ignored for the one server; use --port")
        );
        repo.check_server();
    }
}

#[test]
fn record_write_failure_leaves_no_partial_and_does_not_restart_the_server() {
    let repo = Repo::new("record-write");
    repo.start(&repo.main, false);
    let running = repo.record().unwrap();
    let address = repo.home.join("server.json");
    fs::remove_file(&address).unwrap();
    fs::create_dir(&address).unwrap();
    let mut process = repo.run(repo.command(&repo.main));
    assert_eq!(process.wait().code(), Some(2));
    assert!(
        process.output().contains("could not write"),
        "{}",
        process.output()
    );
    let partials: Vec<_> = fs::read_dir(&repo.home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("server.json."))
        .collect();
    assert!(partials.is_empty(), "{partials:?}");
    assert_eq!(status(running.port).pid, running.pid);
    fs::remove_dir(&address).unwrap();
    repo.start(&repo.main, false);
    assert_eq!(repo.record(), Some(running));
}

#[test]
fn a_recorded_server_that_never_answers_is_looked_at_only_within_the_patience() {
    let repo = Repo::new("silent-record");
    // Each look connects here, is counted and is closed unanswered; the test's own connection,
    // which sends nothing, ends the count.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let looks = std::thread::spawn(move || {
        let mut looks = 0;
        for stream in listener.incoming() {
            if stream.unwrap().read(&mut [0]).unwrap_or(0) == 0 {
                break;
            }
            looks += 1;
        }
        looks
    });
    // A stand-in whose command line is this home's server on that port; it runs until let go.
    let server = repo.base.join("stand-in/symphony-server");
    write(&server, "#!/bin/sh\nread -r line\n");
    fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(&server);
    command
        .args(["serve", "--home", repo.home.to_str().unwrap()])
        .args(["--root", repo.main.to_str().unwrap()])
        .args(["--port", &port.to_string()])
        .stdin(Stdio::piped());
    let stand_in = repo.run(command);
    let pid = stand_in.child.id();
    write(
        &repo.home.join("server.json"),
        &serde_json::json!({"pid": pid, "port": port, "root": repo.main}).to_string(),
    );
    let found = Home::new(
        &repo.home,
        Path::new(fixture::APP),
        &repo.main,
        0,
        Vec::new(),
    )
    .patience(Duration::ZERO)
    .ensure();
    let Err(ServerError::NotUp(why)) = &found else {
        panic!("a silent server was taken for an answer: {found:?}");
    };
    assert!(
        why.contains(&format!(
            "(pid {pid}, port {port}) runs and does not answer"
        )),
        "{why}"
    );
    drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
    // One look before the lock and one under it: a zero patience looks again none, where the
    // default minute would look hundreds of times.
    assert_eq!(looks.join().unwrap(), 2);
    drop(stand_in);
}

#[test]
fn a_stop_reads_the_process_table_only_while_something_holds_the_port() {
    let mut repo = Repo::new("census-scope");
    // The local end of a connection this test holds: bound, so no port-0 bind is given it, and
    // listened on by nothing, so a connection to it is refused.
    let listening = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let connected = TcpStream::connect(listening.local_addr().unwrap()).unwrap();
    let free = connected.local_addr().unwrap().port();
    let busy = listening.local_addr().unwrap().port();
    for (port, census) in [(free, false), (busy, true)] {
        repo.port = Some(port);
        let mut command = repo.command(&repo.main);
        command.arg("--stop");
        let mut stop = repo.run(command);
        stop.success();
        assert!(
            stop.output().contains("no Symphony was running"),
            "{}",
            stop.output()
        );
        // The ps fixture leaves env.<its caller's pid> behind each time it runs.
        let read = repo.base.join(format!("env.{}", stop.child.id())).exists();
        assert_eq!(read, census, "port {port}: {}", stop.output());
    }
}

#[test]
fn a_profiled_server_times_each_stage_of_its_warm_start() {
    let repo = Repo::new("profiled-start");
    // Main holds a register and another branch's worktree adds a card to it, so the warm's branch
    // cards read main's and the fork's register (`git cat-file --batch`).
    let register = |cards: &str| {
        format!("# Decisions\n\n<!-- register:start -->\n{cards}<!-- register:end -->\n")
    };
    let held_card = "- [ ] **held** — Proposal held.\n";
    write(&repo.main.join("docs/DECISIONS.md"), &register(held_card));
    repository(&repo.main, &["add", "docs/DECISIONS.md"]);
    repository(&repo.main, &["commit", "-qm", "A register"]);
    let registers = repo.base.join("registers");
    repository(
        &repo.main,
        &[
            "worktree",
            "add",
            "-qb",
            "feature/registers",
            registers.to_str().unwrap(),
        ],
    );
    write(
        &registers.join("docs/DECISIONS.md"),
        &register(&format!(
            "{held_card}- [ ] **branch-only** — Proposal branch-only.\n"
        )),
    );
    // the branch posts its card: only a committed card reaches the queue
    repository(&registers, &["commit", "-qam", "Post branch-only"]);
    let mut command = repo.command(&repo.tree);
    command
        .args(["--port", "0", "--host", "127.0.0.1"])
        .env("SYMPHONY_PROFILE", "1");
    let mut server = repo.run(command);
    let actual = server.ready();
    // Only the card the branch added is its own: had a register read come back empty, the
    // card it shares with main and with its fork would be listed too.
    let (code, queue) = answer(actual.port, "/api/queue");
    assert_eq!(code, 200, "{queue}");
    let queue: symphony_api::Queue = serde_json::from_str(&queue).unwrap();
    let proposed: Vec<_> = queue
        .waiting
        .iter()
        .filter(|card| card.doc.starts_with("@feature~registers/"))
        .map(|card| card.slug.as_str())
        .collect();
    assert_eq!(proposed, ["branch-only"], "{:?}", queue.waiting);
    let output = server.output();
    for stage in [
        "hub new",
        "hub branch trees",
        "hub checkout",
        "branch cards",
    ] {
        assert!(
            output.contains(&format!("· review timing start: {stage}")),
            "{output}"
        );
        assert!(
            output.contains(&format!("· review timing: {stage}: ")),
            "{output}"
        );
    }
    server.signal(libc::SIGTERM);
    server.success();
    let said = server.output();
    assert!(said.contains("· Symphony server stopped"), "{said}");
}

#[test]
fn a_server_of_its_own_is_refused_the_one_servers_port_on_every_address() {
    let mut repo = Repo::new("reserved-port");
    // The port is this test's: a server that bound it would fail to listen, not be refused.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    repo.port = Some(port);
    // Loopback, and an address no interface has (TEST-NET-1), are refused alike, before any bind.
    for host in ["127.0.0.1", "192.0.2.1"] {
        let mut command = repo.command(&repo.tree);
        command.args(["--port", &port.to_string(), "--host", host]);
        let mut own = repo.run(command);
        assert_eq!(own.wait().code(), Some(2), "{}", own.output());
        assert!(
            own.output().contains(&format!(
                "port {port} is the one server's (`just symphony`)"
            )),
            "{host}: {}",
            own.output()
        );
        assert!(!repo.tree.join("artifacts/docs-app.port").exists());
    }
    assert!(repo.record().is_none());
}

#[test]
fn the_native_server_has_no_interpreter_child() {
    let repo = Repo::new("native-only");
    let mut server = repo.private(&repo.tree);
    let actual = server.ready();
    assert_eq!(actual.app, None);
    // Inspect only this owned process. Native serving must settle with no resident child at all;
    // transient repository/identity commands may finish between readiness and this observation.
    until("native server without resident children", || {
        children(actual.pid).is_empty().then_some(())
    });
    assert_eq!(answer(actual.port, "/api/front").0, 200);
    server.signal(libc::SIGTERM);
    server.success();
}

#[cfg(target_os = "macos")]
fn children(pid: i64) -> Vec<i32> {
    let mut pids = [0_i32; 1024];
    // SAFETY: a writable pid buffer of the stated size, querying only our recorded server pid.
    let bytes = unsafe {
        libc::proc_listchildpids(
            i32::try_from(pid).unwrap(),
            pids.as_mut_ptr().cast(),
            i32::try_from(std::mem::size_of_val(&pids)).unwrap(),
        )
    };
    assert!(bytes >= 0, "child enumeration failed");
    pids.into_iter().filter(|pid| *pid > 0).collect()
}

#[cfg(target_os = "linux")]
fn children(pid: i64) -> Vec<i32> {
    fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap()
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect()
}
