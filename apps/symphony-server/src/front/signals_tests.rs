//! A server's signals are watched from the moment their watch is made, before anything outside can
//! see the server: one sent before the watch is first polled is kept and acted on, never met by
//! its default action, which ends the process. Each case raises its signal in a child process of
//! its own, so the rest of this test run never receives it, and a child its signal ended fails the
//! case by that end.

use super::*;
use crate::testing::Tree;
use std::os::unix::process::ExitStatusExt;
use symphony_board::sys::{self, Stdio};

/// Set in the child each case runs itself in.
const CHILD: &str = "SYMPHONY_SIGNALS_CHILD";
/// What a child says once its case has passed.
const DONE: &str = "signals case: done";

/// The wall-clock backstop of every wait here: each waits on its event, and only a stall reaches
/// this (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// Run the case `test` (its name under `front::signals_tests`) again in a child of its own, which
/// takes the child's side, and require that child to end well, saying [`DONE`].
fn in_a_child(test: &str) {
    let child = sys::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("front::signals_tests::{test}"),
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env_remove("TASKS_SESSION")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let done = sys::wait_output(child).unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        done.status.signal(),
        None,
        "the child ended by a signal it raised itself: its default action, unwatched: {said}"
    );
    assert!(done.status.success(), "{said}");
    assert!(said.contains(DONE), "{said}");
}

/// Send `signal` to this process, the child a case runs in.
fn raise(signal: libc::c_int) {
    // SAFETY: raise(3) takes any signal number; it returns non-zero only for an invalid one.
    assert_eq!(unsafe { libc::raise(signal) }, 0, "raise({signal})");
}

/// A stop sent after its watch is made and before the watch is polled resolves it at its first
/// poll: SIGTERM, SIGINT and SIGHUP each, a watch of its own made for each.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_sent_before_its_watch_is_polled_is_kept_and_resolves_it() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child("a_stop_sent_before_its_watch_is_polled_is_kept_and_resolves_it");
    }
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let stop = signalled();
        raise(signal);
        tokio::time::timeout(BACKSTOP, stop)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "the watch to see signal {signal}, sent before its first poll: stalled: \
                     the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
                    test_time::Load::now()
                )
            });
    }
    println!("{DONE}");
}

/// A stop sent as a server starts, before it listens or records its port, is kept through the
/// start and acted on once it is done: the stop path runs, the port record is cleared and nothing
/// listens on its port.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_sent_before_the_server_listens_stops_it_once_started_leaving_nothing() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child(
            "a_stop_sent_before_the_server_listens_stops_it_once_started_leaving_nothing",
        );
    }
    let tree = Tree::new();
    tree.write("root/docs/README.md", "# Docs\n");
    let root = tree.path("root");
    let stop = signalled();
    raise(libc::SIGTERM);
    assert!(
        !port_file(&root).exists(),
        "a port recorded before the start"
    );
    let started = start(
        Serve {
            home: None,
            root: root.clone(),
            port: 0,
            hosts: vec!["127.0.0.1".into()],
            install: None,
        },
        false,
        None,
    )
    .await
    .unwrap();
    let port = started.serve.port;
    assert_eq!(
        std::fs::read_to_string(port_file(&root)).unwrap(),
        format!("{port}\n"),
        "the start records its port"
    );
    let status = tokio::time::timeout(BACKSTOP, started.serve_until_stopped(stop))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the stop kept through the start to end the server: stalled: the wait's \
                 wall-clock backstop of {BACKSTOP:?} ran out; {}",
                test_time::Load::now()
            )
        });
    assert_eq!(status, 0);
    assert!(
        !port_file(&root).exists(),
        "the server stopped without clearing its port record"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "something still listens on the stopped server's port {port}"
    );
    println!("{DONE}");
}

/// A hand-off asked (SIGUSR1) after its watch is made and before its loop starts is kept, and
/// asked of the server once the loop starts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hand_off_asked_before_its_loop_starts_is_kept_and_asked_of_the_server() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child(
            "a_hand_off_asked_before_its_loop_starts_is_kept_and_asked_of_the_server",
        );
    }
    let tree = Tree::new();
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
    let asked = hand_off_asked().expect("SIGUSR1 can be watched");
    raise(libc::SIGUSR1);
    let wakes = Wakes::new();
    shared.wake_at_handover(wakes.waker());
    assert!(
        !shared.handover.load(Ordering::SeqCst),
        "a hand-off asked of the server before its loop started"
    );
    handover_when_asked(&shared, asked);
    let woken = wakes.wait(Some(BACKSTOP));
    assert!(
        woken,
        "the kept request to wake the hand-off's waiters: stalled: the wait's wall-clock \
         backstop of {BACKSTOP:?} ran out; {}",
        test_time::Load::now()
    );
    assert!(shared.handover.load(Ordering::SeqCst));
    shared.stop();
    println!("{DONE}");
}
