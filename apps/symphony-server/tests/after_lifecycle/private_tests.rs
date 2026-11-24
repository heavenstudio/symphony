//! A server of its own: the port record it keeps in its checkout's artifacts folder, removed on
//! each shutdown signal and left when another owner replaced it, and its end with the process or
//! the test that started it.

use super::*;

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

/// How many servers of its own [`a_stop_sent_as_soon_as_a_server_can_be_seen_ends_it_by_the_stop_path_every_time`]
/// starts and stops at the first moment it can be seen.
const SEEN_RUNS: usize = 10;

/// A stop sent at the first moment a server of its own can be seen, its port record's appearance,
/// ends it by the stop path every time: its signals are watched before it can be seen, so it exits
/// well, says it stopped and leaves no port record. The record is looked for by the tests' wait (a
/// file's event is one the machine's watcher drops under load). The last moment of its start, its
/// URL line, is `front::signals_tests::a_stop_sent_as_the_url_line_is_written_ends_it_by_the_stop_path`'s,
/// which holds that line so that its stop comes before it every time.
#[test]
fn a_stop_sent_as_soon_as_a_server_can_be_seen_ends_it_by_the_stop_path_every_time() {
    let repo = Repo::new("seen-stop");
    let record = repo.tree.join("artifacts/docs-app.port");
    for run in 1..=SEEN_RUNS {
        let mut server = repo.private(&repo.tree);
        until("its port record's appearance", || {
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "the server ended before its port record's appearance: {}",
                server.output()
            );
            record.exists().then_some(())
        });
        eprintln!("· SIGTERM at its port record's appearance, run {run} of {SEEN_RUNS}");
        server.signal(libc::SIGTERM);
        server.success();
        assert!(
            !record.exists(),
            "a stop at its port record's appearance left the record (run {run} of {SEEN_RUNS})"
        );
        assert!(
            server.output().contains("· Symphony server stopped"),
            "a stop at its port record's appearance did not run the stop path (run {run} of \
             {SEEN_RUNS}): {}",
            server.output()
        );
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
    // Ended at the case's end only while its pid still names this server: once it followed its
    // parent, the pid may name a process started since.
    struct Owned(fixture::killed::Owned);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0
                .signal(libc::SIGKILL, "the case's end: a server it orphaned");
        }
    }
    let _owned = Owned(fixture::killed::Owned::of(pid));
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
    // cat reads its input, which its Process holds open: it lives until the case kills it.
    let mut marker = Command::new("/bin/cat");
    marker.stdin(Stdio::piped());
    let mut marker = repo.run(marker);
    let mut command = repo.command(&repo.tree);
    command
        .args(["--port", "0", "--host", "127.0.0.1"])
        .env("SYMPHONY_TEST_PID", marker.child.id().to_string());
    let mut server = repo.run(command);
    let actual = server.ready();
    marker.signal(libc::SIGKILL);
    assert!(!marker.wait().success());
    server.success();
    assert!(server.output().contains("the test that started it ended"));
    assert!(!repo.tree.join("artifacts/docs-app.port").exists());
    assert!(front(actual.port).is_none());
}

/// A release installed in `repo`'s machine folder as an install leaves it
/// ([`super::release_tests::installed`]): the server in it, through the `current` link.
fn installed_release(repo: &Repo, name: &str, mark: &str) -> PathBuf {
    super::release_tests::installed(repo, name, mark, None);
    repo.home.join("current/bin/symphony-server")
}

#[test]
fn a_server_run_from_an_installed_release_serves_the_client_beside_it() {
    let repo = Repo::new("released-client");
    let serve = |binary: &Path| {
        let mut released = Command::new(binary);
        released
            .current_dir(&repo.tree)
            .env_clear()
            .envs(repo.environment())
            .stdin(Stdio::null())
            .args(["--root", repo.tree.to_str().unwrap()])
            .args(["--port", "0", "--host", "127.0.0.1"]);
        repo.run(released)
    };
    let first = installed_release(&repo, "release-1", "first");
    let mut server = serve(&first);
    let port = server.ready().port;
    let (status, page) = answer(port, "/");
    assert_eq!(status, 200, "{page}");
    // The release's own page, the project's shell written into it.
    assert!(
        page.contains("src=\"/assets/app-first.js\"") && page.ends_with("first</body></html>"),
        "{page}"
    );
    let (status, file) = answer(port, "/assets/app-first.js");
    assert_eq!((status, file.as_str()), (200, "console.log('first')"));
    server.signal(libc::SIGTERM);
    server.success();
    // Another release, another client: nothing of the first is compiled into the server.
    let second = installed_release(&repo, "release-2", "second");
    let mut server = serve(&second);
    let port = server.ready().port;
    let (_, page) = answer(port, "/");
    assert!(page.ends_with("second</body></html>"), "{page}");
    assert_eq!(
        get(port, "/assets/app-first.js").map(|got| got.0),
        Some(404)
    );
    server.signal(libc::SIGTERM);
    server.success();
}
