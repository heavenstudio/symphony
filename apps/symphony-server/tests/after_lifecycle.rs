//! Lifecycle and `just symphony` regressions at the real native executable boundary.
//! Builds are fixture commands; the recipe, sockets, records, recovery and shutdown are real.
//! Every server has a scratch repository, an OS-assigned port and the owning test's lifetime pid.

#[path = "after_lifecycle/fixture.rs"]
mod fixture;

#[path = "common/bash_env.rs"]
mod bash_env;

#[path = "after_lifecycle/discovery_tests.rs"]
mod discovery_tests;

use fixture::{AGENTS, Repo, front, get, repository, until, write};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};
use symphony_server::home::{Home, ServerError};
use tasks::sys::{Command, Stdio};

#[test]
fn symphony_recipes_find_and_stop_a_live_predecessor_record_on_a_private_port() {
    let repo = Repo::new("symphony-predecessor-record");
    let legacy = repo.main.join(".git/sf-docs");
    fs::create_dir_all(&legacy).unwrap();
    let mut command = repo.command(&repo.main);
    command.args([
        "serve",
        "--home",
        legacy.to_str().unwrap(),
        "--root",
        repo.main.to_str().unwrap(),
        "--port",
        "0",
        "--host",
        "127.0.0.1",
    ]);
    let mut predecessor = repo.run(command);
    let actual = predecessor.ready();
    assert_ne!(actual.port, 8765);
    let recorded = symphony_server::home::Running {
        port: actual.port,
        pid: actual.pid,
        root: actual.root.clone(),
    };
    write(
        &legacy.join("server.json"),
        &serde_json::to_string(&recorded).unwrap(),
    );
    // Port-zero ownership recovery reads the serving process's announced port from its home log.
    write(&legacy.join("server.log"), &predecessor.output());
    let mut find = repo.recipe(&repo.tree);
    find.arg("--if-up");
    let mut found = repo.run(find);
    found.success();
    assert!(
        found
            .output()
            .contains(&format!("http://127.0.0.1:{}/", actual.port))
    );
    assert_eq!(front(actual.port).unwrap().pid, actual.pid);
    assert!(
        !repo.home.join("server.json").exists(),
        "discovery must not start a second server"
    );
    let mut stop = repo.recipe(&repo.tree);
    stop.arg("--stop");
    let mut stopping = repo.run(stop);
    predecessor.success();
    stopping.success();
    assert!(!legacy.join("server.json").exists());
    assert!(front(actual.port).is_none());
}

#[test]
fn bash_env_does_not_add_output_to_build_fixtures() {
    bash_env::run("bash_env_does_not_add_output_to_build_fixtures", || {
        let repo = Repo::new("bash-env");
        let mut command = Command::new(repo.main.join("tools/fixture-build"));
        command
            .current_dir(&repo.tree)
            .env("SF_LIFECYCLE_BINARY", fixture::APP)
            .env("SF_LIFECYCLE_LOG", &repo.builds)
            .args(["sf-slots", "cargo", "build"]);
        let mut process = repo.run(command);
        process.success();
        assert_eq!(process.output(), "");
        assert_eq!(repo.build_events().len(), 1);
    });
}

#[test]
fn build_fixtures_reuse_the_built_image_and_replace_old_outputs_without_writing_through_links() {
    let repo = Repo::new("build-image");
    // Commands also share their executable images, including the checkout made by git. Copying
    // fresh scripts per case sends each one through first-exec validation on a loaded macOS host.
    for (source, command) in [
        ("build.sh", repo.main.join("tools/fixture-build")),
        ("build.sh", repo.tree.join("tools/rs-bin.sh")),
        ("ps.sh", repo.bin.join("ps")),
        ("open.sh", repo.bin.join("open")),
        ("open.sh", repo.bin.join("xdg-open")),
    ] {
        let source = fs::metadata(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/after_lifecycle")
                .join(source),
        )
        .unwrap();
        let installed = fs::metadata(command).unwrap();
        assert_eq!(
            (installed.dev(), installed.ino()),
            (source.dev(), source.ino()),
            "fixture commands must reuse the executable source"
        );
    }
    let original = fs::metadata(fixture::APP).unwrap();
    for install in [false, true] {
        let out = if install {
            repo.home.join("bin/symphony-server")
        } else {
            repo.tree.join(".target/debug/symphony-server")
        };
        write(&out, "old build output\n");
        // The second invocation sees an existing link, as a stopped main's rebuild does.
        for _ in 0..2 {
            let mut command = Command::new(repo.main.join("tools/fixture-build"));
            command
                .current_dir(&repo.tree)
                .env("SF_LIFECYCLE_BINARY", fixture::APP)
                .env("SF_LIFECYCLE_LOG", &repo.builds)
                .args(["sf-slots", "cargo"]);
            if install {
                command.arg("install").arg("--root").arg(&repo.home);
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
            command.env("SF_SYMPHONY_PORT", "0").args([
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
    let mut starts: Vec<_> = (0..8)
        .map(|n| {
            let dir = if n % 2 == 0 { &repo.main } else { &repo.tree };
            (n, repo.run(repo.command(dir)))
        })
        .collect();
    for (_, child) in &mut starts {
        child.success();
    }
    repo.remember();
    let actual = repo.check_server();
    for (n, child) in starts {
        let page = if n % 2 == 0 {
            ""
        } else {
            "@feature~lifecycle/"
        };
        assert!(
            child
                .output()
                .contains(&format!("http://127.0.0.1:{}/{page}", repo.port())),
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
fn recipe_builds_main_for_shared_starts_and_reuses_the_installed_copy() {
    // A first start in either checkout installs from main; an already answering copy needs no
    // build. A branch never installs its own server into the shared scratch home.
    for from_tree in [false, true] {
        let repo = Repo::new("shared-recipe");
        let dir = if from_tree { &repo.tree } else { &repo.main };
        let start = repo.start(dir, true);
        let running = repo.check_server();
        assert!(start.output().contains(&format!("pid {}", running.pid)));
        let builds = repo.build_events();
        assert_eq!(builds.len(), 2, "{builds:?}");
        assert!(
            builds[0].starts_with(&format!("ui\t{}\t", repo.main.display())),
            "{builds:?}"
        );
        assert!(
            builds[1].starts_with(&format!("sf-slots\t{}\t", repo.main.display())),
            "{builds:?}"
        );
        assert!(builds[1].contains("cargo install"));
        assert!(repo.home.join("bin/symphony-server").is_file());
        assert_eq!(
            get(repo.port(), "/").unwrap().1,
            "<!doctype html><html>lifecycle client</html>\n"
        );
        repo.start(&repo.main, true);
        repo.start(&repo.tree, true);
        assert_eq!(repo.build_events(), builds, "healthy starts must not build");
        assert_eq!(repo.check_server().pid, running.pid);
        repo.stop();
        assert!(!repo.home.join("server.json").exists());
        if from_tree {
            repo.start(&repo.tree, true);
            assert_eq!(
                repo.build_events(),
                builds,
                "worktree uses an installed copy even while stopped"
            );
        } else {
            repo.start(&repo.main, true);
            assert_eq!(
                repo.build_events().len(),
                4,
                "main rebuilds a stopped installed copy"
            );
        }
    }
}

#[test]
fn recipe_private_starts_build_and_serve_the_current_checkout() {
    let repo = Repo::new("private-recipe");
    let external = repo.external("recipe-external");
    let cases = [
        (vec!["--port", "0"], &repo.tree),
        (vec!["0"], &repo.tree),
        (vec!["--root", external.to_str().unwrap()], &external),
    ];
    for (index, (args, root)) in cases.into_iter().enumerate() {
        let mut command = repo.recipe(&repo.tree);
        command.args(args).args(["--host", "127.0.0.1"]);
        let mut private = repo.run(command);
        let actual = private.ready();
        assert_eq!(actual.root, root.to_str().unwrap());
        assert!(repo.record().is_none());
        let builds = repo.build_events();
        assert_eq!(builds.len(), 2 * (index + 1), "{builds:?}");
        assert!(
            builds
                .iter()
                .all(|line| line.split('\t').nth(1) == repo.tree.to_str()),
            "{builds:?}"
        );
        assert!(
            builds
                .last()
                .unwrap()
                .contains("cargo build --locked -p symphony-host --bin symphony-server")
        );
        assert_eq!(get(actual.port, "/").unwrap().0, 200);
        private.signal(libc::SIGTERM);
        private.success();
        assert!(!root.join("artifacts/docs-app.port").exists());
    }
}

#[test]
fn if_up_never_starts_a_server_and_stop_never_requires_a_build() {
    let repo = Repo::new("if-up");
    let mut command = repo.command(&repo.tree);
    command.arg("--if-up");
    let mut absent = repo.run(command);
    assert_eq!(absent.wait().code(), Some(3));
    assert!(!repo.home.exists(), "the no-server path creates nothing");
    repo.start(&repo.tree, true);
    let running = repo.check_server();
    let builds = repo.build_events();
    let mut command = repo.recipe(&repo.main);
    command.args(["--if-up", "--stop"]);
    repo.run(command).success();
    assert_eq!(repo.build_events(), builds);
    assert!(!repo.home.join("server.json").exists());
    assert!(!repo.main.join("artifacts/docs-app.port").exists());
    until("stopped server pid", || {
        (!symphony_server::sys::pid_alive(running.pid)).then_some(())
    });
}

#[test]
fn missing_malformed_stale_and_lying_records_recover_the_same_running_server() {
    let repo = Repo::new("record recovery");
    repo.start(&repo.tree, false);
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
        repo.start(&repo.tree, false);
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
    let mut command = Command::new("/bin/sleep");
    command.arg("60");
    let mut unrelated = repo.run(command);
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
    // reap that exact child before returning, rather than waiting for the sleep to expire.
    let pid = libc::pid_t::try_from(unrelated.child.id()).unwrap();
    let began = Instant::now();
    drop(unrelated);
    let elapsed = began.elapsed();
    let mut status = 0;
    // SAFETY: the recorded pid is this test's child; WNOHANG only observes/reaps that child.
    let observed = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    let error = (observed == -1)
        .then(std::io::Error::last_os_error)
        .and_then(|error| error.raw_os_error());
    if observed == 0 {
        // Preserve the failed observation before cleanup so a broken drop cannot leak a child
        // or be hidden by this fallback. A positive waitpid result already reaped the child.
        symphony_server::sys::signal(i64::from(pid), libc::SIGKILL);
        // SAFETY: the nonblocking observation proved this exact owned child is still running.
        while unsafe { libc::waitpid(pid, &mut status, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
        {}
    }
    assert_eq!(
        (observed, error),
        (-1, Some(libc::ECHILD)),
        "Process::drop left its child {pid} alive or unreaped"
    );
    assert!(
        elapsed < fixture::PATIENCE,
        "Process::drop waited for its child instead of ending it: {elapsed:?}"
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
    symphony_server::sys::signal(first.pid, libc::SIGKILL);
    until("killed shared server", || {
        (!symphony_server::sys::pid_alive(first.pid)).then_some(())
    });
    check_progress(repo.start(&repo.tree, false));
    let second = repo.check_server();
    assert_ne!(second.pid, first.pid);
}

#[test]
fn shared_starts_report_and_ignore_in_repository_root_and_host_overrides() {
    let repo = Repo::new("ignored-options");
    for dir in [&repo.tree, &repo.main] {
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
    assert_eq!(front(running.port).unwrap().pid, running.pid);
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
fn a_fresh_start_returns_while_a_pending_census_can_still_update_the_count() {
    let repo = Repo::new("late-census");
    // One Symphony left under init, which no process of this case is.
    write(
        &repo.base.join("census-rows"),
        "999999 1 /usr/bin/interp -B tools/docs_server --root /gone --port 9\n",
    );
    // The server's first census reads this FIFO, and ends only once the case writes a line.
    let mut hold = fixture::fifo(&repo.base.join("census-hold"));
    let mut start = repo.run(repo.command(&repo.main));
    until("the census reading its hold", || {
        repo.base.join("census-held").exists().then_some(())
    });
    start.success();
    repo.remember();
    // No completion event has been sent: the starter returned while its count was unavailable.
    let up = front(repo.port()).expect("the server's status");
    assert!(
        up.ready && up.uncounted && up.census_pending,
        "the server is up before its first census ends: {up:?}"
    );
    assert_eq!(up.orphans, 0);
    writeln!(hold, "end the census").unwrap();
    let counted = until("the completed census", || {
        let status = repo.check_server();
        (!status.uncounted).then_some(status)
    });
    assert_eq!((counted.orphans, counted.uncounted), (1, false));
    assert!(!counted.census_pending);
    let mut again = repo.run(repo.command(&repo.main));
    again.success();
    assert!(
        again.output().contains(
            "· 1 Symphony processes from test runs run with no parent; none is stopped for you"
        ),
        "{}",
        again.output()
    );
}

#[test]
fn a_census_that_never_answers_holds_neither_a_start_nor_a_graceful_stop() {
    let repo = Repo::new("hung-census");
    // Keep the writer alive through both lifecycle events, without ever completing the scan.
    let _hold = fixture::fifo(&repo.base.join("census-hold"));
    let mut start = repo.run(repo.command(&repo.main));
    until("the census reading its hold", || {
        repo.base.join("census-held").exists().then_some(())
    });
    let ps: i64 = fs::read_to_string(repo.base.join("census-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(symphony_server::sys::pid_alive(ps));
    start.success();
    repo.remember();
    let pending = repo.check_server();
    assert!(pending.uncounted && pending.census_pending, "{pending:?}");
    repo.stop();
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert!(log.contains("· Symphony server stopped"), "{log}");
    until("the cancelled census child to be reaped", || {
        (!symphony_server::sys::pid_alive(ps)).then_some(())
    });
}

#[test]
fn a_profiled_server_times_each_census_round_and_a_signal_ends_a_pending_one_at_once() {
    let repo = Repo::new("profiled-census");
    // Main holds a register and another branch's worktree adds a card to it, so the warm's branch
    // cards read main's and the fork's register (`git cat-file --batch`) while the census is held.
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
    let rounds = |server: &fixture::Process| {
        server
            .output()
            .matches("· review timing: orphan census: ")
            .count()
    };
    let held = repo.base.join("census-held");
    let profiled = |hold: &Path| {
        let hold = fixture::fifo(hold);
        let mut command = repo.command(&repo.tree);
        command
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("SF_SYMPHONY_PROFILE", "1");
        let mut server = repo.run(command);
        let actual = server.ready();
        until("the census reading its hold", || {
            held.exists().then_some(())
        });
        // Only the card the branch added is its own: had a register read come back empty, the
        // card it shares with main and with its fork would be listed too.
        let (status, queue) = get(actual.port, "/api/queue").unwrap();
        assert_eq!(status, 200, "{queue}");
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
        assert_eq!(
            output
                .matches("· review timing start: orphan census")
                .count(),
            1,
            "{output}"
        );
        (server, hold)
    };

    // An answered round logs its time as its scan answers; the wait for the next round is untimed.
    let (mut server, mut hold) = profiled(&repo.base.join("census-hold"));
    assert_eq!(rounds(&server), 0, "{}", server.output());
    writeln!(hold, "end the census").unwrap();
    until("the answered round's timing", || {
        (rounds(&server) == 1).then_some(())
    });
    server.signal(libc::SIGTERM);
    server.success();
    assert_eq!(rounds(&server), 1, "{}", server.output());
    drop(hold);
    fs::remove_file(&held).unwrap();

    // A round that never answers holds no stop, and logs how long it was pending.
    let (mut server, _hold) = profiled(&repo.base.join("census-hold"));
    let ps: i64 = fs::read_to_string(repo.base.join("census-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(symphony_server::sys::pid_alive(ps));
    server.signal(libc::SIGTERM);
    server.success();
    let said = server.output();
    assert_eq!(rounds(&server), 1, "{said}");
    assert!(said.contains("· Symphony server stopped"), "{said}");
    until("the cut census child to be reaped", || {
        (!symphony_server::sys::pid_alive(ps)).then_some(())
    });
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
fn removing_the_starter_worktree_preserves_the_server_and_its_environment_isolation() {
    let repo = Repo::new("removed-starter");
    fs::create_dir(repo.tree.join("starter-tmp")).unwrap();
    let mut command = repo.recipe(&repo.tree);
    command
        .env("TASKS_SESSION", "starter-agent")
        .env("CODEX_THREAD_ID", "starter-thread")
        .env("CLAUDECODE", "1")
        .env("TASKS_SERVER", "http://invalid.invalid:1")
        .env("TASKS_DATABASE", "starter_private_board")
        .env("ASKS_DB", repo.tree.join("starter-asks"))
        .env("TMPDIR", repo.tree.join("starter-tmp"))
        .env("SF_SYMPHONY_UI_DIST", "/wrong-client")
        .env("LIFECYCLE_STARTER_ONLY", "must-not-arrive");
    repo.run(command).success();
    repo.remember();
    let running = repo.check_server();
    let env_path = repo.base.join(format!("env.{}", running.pid));
    let environment = until("server environment observation", || {
        fs::read_to_string(&env_path).ok()
    });
    let value = |name: &str| {
        environment
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
    };
    assert_eq!(value("parent"), Some(running.pid.to_string().as_str()));
    assert_eq!(value("cwd"), repo.main.to_str());
    assert_eq!(
        value("TMPDIR"),
        repo.main.join("artifacts/docs-tmp").to_str()
    );
    assert_eq!(value("HOME"), repo.base.to_str());
    assert!(
        value("PATH")
            .unwrap()
            .starts_with(repo.bin.to_str().unwrap())
    );
    assert_eq!(
        value("SF_SYMPHONY_TEST_PID"),
        Some(std::process::id().to_string().as_str())
    );
    for name in AGENTS.into_iter().chain([
        "TASKS_SERVER",
        "TASKS_DATABASE",
        "ASKS_DB",
        "SF_SYMPHONY_PORT",
        "SF_SYMPHONY_UI_DIST",
        "LIFECYCLE_STARTER_ONLY",
    ]) {
        assert!(value(name).is_none(), "starter variable {name} leaked");
    }
    repository(
        &repo.main,
        &["worktree", "remove", "--force", repo.tree.to_str().unwrap()],
    );
    assert!(!repo.tree.exists());
    assert_eq!(get(repo.port(), "/").unwrap().0, 200);
    assert_eq!(repo.check_server().pid, running.pid);
    repo.start(&repo.main, true);
    assert_eq!(repo.check_server().pid, running.pid);
    assert!(!repo.base.join("opened").exists());
}

#[test]
fn private_servers_clean_their_port_record_on_each_shutdown_signal() {
    let repo = Repo::new("signals");
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let mut server = repo.private(&repo.tree);
        let actual = server.ready();
        let record = repo.tree.join("artifacts/docs-app.port");
        assert_eq!(
            fs::read_to_string(&record).unwrap().trim(),
            actual.port.to_string()
        );
        server.signal(signal);
        server.success();
        assert!(!record.exists(), "signal {signal} left discovery state");
        assert!(front(actual.port).is_none());
        assert!(repo.record().is_none());
    }
}

#[test]
fn private_shutdown_preserves_a_replaced_port_record_and_main_discovery_record() {
    let repo = Repo::new("port-record");
    let mut server = repo.private(&repo.tree);
    server.ready();
    let record = repo.tree.join("artifacts/docs-app.port");
    fs::write(&record, "another owner\n").unwrap();
    server.signal(libc::SIGTERM);
    server.success();
    assert_eq!(fs::read_to_string(&record).unwrap(), "another owner\n");
    let main_record = repo.main.join("artifacts/docs-app.port");
    write(&main_record, "shared discovery\n");
    let mut server = repo.private(&repo.main);
    server.ready();
    assert_eq!(
        fs::read_to_string(&main_record).unwrap(),
        "shared discovery\n"
    );
    server.signal(libc::SIGTERM);
    server.success();
    assert_eq!(
        fs::read_to_string(&main_record).unwrap(),
        "shared discovery\n"
    );
}

#[test]
fn an_external_root_selects_a_private_ephemeral_server_without_a_port_argument() {
    let repo = Repo::new("external-root");
    let root = repo.external("external");
    let mut command = repo.command(&repo.tree);
    command.args(["--root", root.to_str().unwrap(), "--host", "127.0.0.1"]);
    let mut server = repo.run(command);
    let actual = server.ready();
    assert_eq!(actual.root, root.to_str().unwrap());
    assert!(repo.record().is_none());
    server.signal(libc::SIGTERM);
    server.success();
    assert!(!root.join("artifacts/docs-app.port").exists());
}

#[test]
fn a_private_server_ends_when_its_exact_starter_process_dies() {
    let repo = Repo::new("parent-death");
    let pid_file = repo.base.join("owned.pid");
    let log = repo.base.join("orphan-server");
    // The shell owns the child, writes its pid, then waits. It is killed only after the native
    // server acknowledges readiness, so the parent monitor is certainly armed first.
    let mut command = Command::new("/bin/bash");
    command.env_clear().envs(repo.environment()).current_dir(&repo.tree).stdin(Stdio::null())
        .args(["-c", "\"$1\" --port 0 --host 127.0.0.1 >\"$2.out\" 2>\"$2.err\" & child=$!; printf '%s\\n' \"$child\" >\"$3\"; wait \"$child\"", "lifecycle-parent", fixture::APP, log.to_str().unwrap(), pid_file.to_str().unwrap()]);
    let mut parent = repo.run(command);
    let pid: i64 = until("owned child pid", || {
        fs::read_to_string(&pid_file).ok()?.trim().parse().ok()
    });
    struct Owned(i64);
    impl Drop for Owned {
        fn drop(&mut self) {
            symphony_server::sys::signal(self.0, libc::SIGKILL);
        }
    }
    let _owned = Owned(pid);
    let actual = until("child native readiness", || {
        let port: u16 = fs::read_to_string(repo.tree.join("artifacts/docs-app.port"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        front(port).filter(|f| f.ready && f.pid == pid)
    });
    parent.signal(libc::SIGKILL);
    assert!(!parent.wait().success());
    until("server to follow its dead parent", || {
        (!symphony_server::sys::pid_alive(pid)).then_some(())
    });
    assert!(!repo.tree.join("artifacts/docs-app.port").exists());
    assert!(front(actual.port).is_none());
    assert!(
        fs::read_to_string(log.with_extension("err"))
            .unwrap()
            .contains("the process that started it ended")
    );
}

#[test]
fn a_server_ends_with_its_named_test_process_even_while_its_parent_lives() {
    let repo = Repo::new("test-lifetime");
    let mut marker = Command::new("/bin/sleep");
    marker.arg("60");
    let mut marker = repo.run(marker);
    let mut command = repo.command(&repo.tree);
    command
        .args(["--port", "0", "--host", "127.0.0.1"])
        .env("SF_SYMPHONY_TEST_PID", marker.child.id().to_string());
    let mut server = repo.run(command);
    let actual = server.ready();
    marker.signal(libc::SIGKILL);
    assert!(!marker.wait().success());
    server.success();
    assert!(server.output().contains("the test that started it ended"));
    assert!(!repo.tree.join("artifacts/docs-app.port").exists());
    assert!(front(actual.port).is_none());
}

#[test]
fn opener_decisions_cover_empty_agent_markers_piped_stdout_and_interactive_open() {
    let repo = Repo::new("opener");
    repo.start(&repo.main, true);
    for recipe in [false, true] {
        for name in AGENTS {
            for value in ["", "agent-present"] {
                let mut command = if recipe {
                    repo.recipe(&repo.tree)
                } else {
                    repo.command(&repo.tree)
                };
                command.args(["--if-up", "--open"]).env(name, value);
                let (mut child, _terminal) = repo.run_terminal(command);
                child.success();
                assert!(
                    child
                        .output()
                        .contains("printing the URL without opening a browser"),
                    "{name}={value:?}: {}",
                    child.output()
                );
                assert!(
                    !repo.base.join("opened").exists(),
                    "{name}={value:?} opened a browser"
                );
            }
        }
        let mut command = if recipe {
            repo.recipe(&repo.tree)
        } else {
            repo.command(&repo.tree)
        };
        command.args(["--if-up", "--open"]);
        let mut piped = repo.run(command);
        piped.success();
        assert!(
            piped
                .output()
                .contains("printing the URL without opening a browser")
        );
        assert!(!repo.base.join("opened").exists());
        let mut command = if recipe {
            repo.recipe(&repo.tree)
        } else {
            repo.command(&repo.tree)
        };
        command.args(["--if-up", "--open"]);
        let (mut interactive, _terminal) = repo.run_terminal(command);
        interactive.success();
        let opened = fs::read_to_string(repo.base.join("opened")).unwrap();
        assert_eq!(
            opened,
            format!("http://127.0.0.1:{}/@feature~lifecycle/\n", repo.port())
        );
        fs::remove_file(repo.base.join("opened")).unwrap();
    }
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
    assert_eq!(get(actual.port, "/api/front").unwrap().0, 200);
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
