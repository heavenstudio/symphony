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
