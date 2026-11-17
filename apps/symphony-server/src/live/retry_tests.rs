//! Retry integration on scratch boards and an injected monotonic clock; no real waits.

use super::Hub;
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    repository,
};
use serde_json::json;
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use symphony_board::{Opening, Schema, Store};

fn clocked(tree: &Tree) -> (Hub, Arc<Mutex<Instant>>) {
    let root = repository(tree);
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let clock = Arc::new(Mutex::new(Instant::now()));
    let held = clock.clone();
    hub.retry_clock = Box::new(move || *held.lock().unwrap());
    hub.clock = Box::new(|| NOW);
    (hub, clock)
}

fn pass(clock: &Mutex<Instant>, seconds: u64) {
    let mut clock = clock.lock().unwrap();
    *clock += Duration::from_secs(seconds);
}

fn reader(fixture: &Fixture, schema: Schema) -> Result<Store, String> {
    Store::opening(
        fixture.other_client(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::read().at(schema),
    )
    .map_err(|e| e.to_string())
}

/// A scratch native recipient. Its stop wakes accept and joins the owned thread; frame reads have
/// only a stall backstop. The short socket path is unique to this fixture's database.
struct Listener {
    path: PathBuf,
    stopped: Arc<AtomicBool>,
    messages: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    fn new(fixture: &Fixture) -> Self {
        let path = PathBuf::from(format!(
            "/tmp/symphony-retry-{}-{}.sock",
            std::process::id(),
            fixture.store.client().database()
        ));
        let socket = UnixListener::bind(&path).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let (send, messages) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for stream in socket.incoming() {
                let mut stream = stream.unwrap();
                if stop.load(SeqCst) {
                    break;
                }
                stream.set_nonblocking(true).unwrap();
                let mut frame = Vec::new();
                test_time::until_true(
                    "the callback frame to close",
                    test_time::backstop(Duration::from_secs(10)),
                    || {
                        let mut buffer = [0; 4096];
                        match stream.read(&mut buffer) {
                            Ok(0) => true,
                            Ok(n) => {
                                frame.extend_from_slice(&buffer[..n]);
                                false
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::Interrupted
                                ) =>
                            {
                                stop.load(SeqCst)
                            }
                            Err(error) => panic!("callback read: {error}"),
                        }
                    },
                )
                .unwrap();
                if !frame.is_empty() {
                    send.send(String::from_utf8(frame).unwrap()).unwrap();
                }
            }
        });
        Self {
            path,
            stopped,
            messages,
            thread: Some(thread),
        }
    }

    fn received(&self, text: &str) {
        loop {
            let frame = self
                .messages
                .recv_timeout(test_time::backstop(Duration::from_secs(10)))
                .unwrap();
            if frame.contains(text) {
                break;
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stopped.store(true, SeqCst);
        let _ = UnixStream::connect(&self.path);
        let joined = self.thread.take().unwrap().join();
        let _ = std::fs::remove_file(&self.path);
        if !std::thread::panicking() {
            joined.unwrap();
        }
    }
}

#[test]
fn schema_open_failures_are_bounded_across_ticks_and_recover_only_when_due() {
    let tree = Tree::new();
    let (hub, clock) = clocked(&tree);
    let fixture = Fixture::new(Seed::default().task("tools/pending", "ready"));
    // A newer reader cannot open this board. It must never migrate it to help a retry.
    let newer = Schema::this_build().then("");
    let repaired = AtomicBool::new(false);
    let attempts = AtomicUsize::new(0);
    let mut connect = |_: &std::path::Path| {
        attempts.fetch_add(1, SeqCst);
        reader(
            &fixture,
            if repaired.load(SeqCst) {
                Schema::this_build()
            } else {
                newer.clone()
            },
        )
    };
    let mut board = None;
    let start = *clock.lock().unwrap();
    for tick in 0..36_000 {
        *clock.lock().unwrap() = start + Duration::from_millis(tick * 100);
        hub.board_tick(&mut board, &mut connect);
    }
    assert_eq!(attempts.load(SeqCst), 65);
    assert!(board.is_none());
    let report = hub.health.report(NOW);
    assert_eq!((report.dispatch.ok, report.dispatch.failed), (0, 65));
    assert!(report.dispatch.error.unwrap().contains("behind this build"));
    assert_eq!(
        fixture.store.board_version().unwrap(),
        Some(symphony_board::SCHEMA)
    );
    repaired.store(true, SeqCst);
    // A repair and thousands of wakes do not erase the deadline (last failure at 3543 s).
    for _ in 0..1_000 {
        hub.board_tick(&mut board, &mut connect);
    }
    assert_eq!(attempts.load(SeqCst), 65);
    *clock.lock().unwrap() = start + Duration::from_secs(3603);
    hub.board_tick(&mut board, &mut connect);
    assert!(board.is_some());
    assert_eq!(attempts.load(SeqCst), 66);
    assert_eq!(hub.task_snapshot().unwrap().tasks.len(), 1);
    assert_eq!(hub.health.report(NOW).dispatch.ok, 1);
    // Once healthy every tick still dispatches, without reopening.
    hub.board_tick(&mut board, &mut connect);
    assert_eq!(attempts.load(SeqCst), 66);
    assert_eq!(hub.health.report(NOW).dispatch.ok, 2);
}

#[test]
fn page_reader_opening_keeps_its_error_until_due_and_resets_after_success() {
    let tree = Tree::new();
    let (mut hub, clock) = clocked(&tree);
    let fixture = Fixture::empty();
    let attempts = Arc::new(AtomicUsize::new(0));
    let repaired = Arc::new(AtomicBool::new(false));
    let (count, fixed) = (attempts.clone(), repaired.clone());
    hub.connect_reader = Box::new(move |_| {
        count.fetch_add(1, SeqCst);
        if fixed.load(SeqCst) {
            reader(&fixture, Schema::this_build())
        } else {
            Err("schema mismatch".into())
        }
    });
    for _ in 0..1000 {
        assert_eq!(hub.reader().err().as_deref(), Some("schema mismatch"));
    }
    assert_eq!(attempts.load(SeqCst), 1);
    pass(&clock, 1);
    assert!(hub.reader().is_err());
    repaired.store(true, SeqCst);
    pass(&clock, 1);
    assert!(hub.reader().is_err(), "second failure waits two seconds");
    pass(&clock, 1);
    let store = hub.reader().unwrap();
    assert!(Arc::ptr_eq(&store, &hub.reader().unwrap()));
    assert_eq!(attempts.load(SeqCst), 3);
    hub.reader_failed(&store);
    repaired.store(false, SeqCst);
    assert!(hub.reader().is_err());
    pass(&clock, 1);
    repaired.store(true, SeqCst);
    assert!(
        hub.reader().is_ok(),
        "success reset the next failure's wait to one second"
    );
}

#[test]
fn failed_assignment_is_independent_capped_and_gives_pending_work_after_repair() {
    let tree = Tree::new();
    let (hub, clock) = clocked(&tree);
    let fixture = Fixture::new(
        Seed::default()
            .session_row(symphony_board::rows::SessionRow {
                handle: "cobalt".into(),
                kind: "orchestrator".into(),
                connected: NOW - 3600.0,
                last_seen: NOW,
                ..Default::default()
            })
            .task("tools/owned", "ready")
            .role("tools/owned", "cobalt", "orchestrator")
            .task("tools/pending", "ready"),
    );
    let old = reader(&fixture, Schema::this_build().before()).unwrap();
    let snapshot = old.clock_board().unwrap();
    // RPC observations use the real wall clock, independently of the board's seeded clock.
    let count = || {
        old.client()
            .queries()
            .report(symphony_board::sweep::trigger::now())
            .0
            .count
    };
    let start = *clock.lock().unwrap();
    let mut attempts = Vec::new();
    for tick in 0..36_000 {
        *clock.lock().unwrap() = start + Duration::from_millis(tick * 100);
        let before = count();
        assert!(!hub.give(&old, &snapshot));
        if count() != before {
            attempts.push(tick / 10);
        }
    }
    assert_eq!(&attempts[..8], [0, 1, 3, 7, 15, 31, 63, 123]);
    assert_eq!(attempts.len(), 65);
    assert!(
        fixture
            .store
            .get("tools/pending")
            .unwrap()
            .holders("orchestrator")
            .is_empty()
    );
    // Healthy dispatch/read work runs while assignment waits, without clearing its streak.
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.ok, 1);
    assert!(
        fixture
            .store
            .get("tools/pending")
            .unwrap()
            .holders("orchestrator")
            .is_empty()
    );
    *clock.lock().unwrap() = start + Duration::from_secs(3603);
    assert!(hub.give(&fixture.store, &snapshot));
    assert_eq!(
        fixture
            .store
            .get("tools/pending")
            .unwrap()
            .holders("orchestrator"),
        ["cobalt"]
    );
    // Success reset the state: another task can be assigned immediately.
    fixture
        .store
        .create(&symphony_board::NewTask::new("tools/next", "Next", "owner"))
        .unwrap();
    assert!(hub.give(&fixture.store, &fixture.store.clock_board().unwrap()));
    assert_eq!(
        fixture
            .store
            .get("tools/next")
            .unwrap()
            .holders("orchestrator"),
        ["cobalt"]
    );
}

#[test]
fn readable_board_dispatch_failure_does_not_stall_reads_chat_or_another_project() {
    let (first, second) = (Tree::new(), Tree::new());
    let (hub, clock) = clocked(&first);
    let (healthy, _) = clocked(&second);
    let fixture = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/pending", "ready"),
    );
    let healthy_board = Fixture::empty();
    let listener = Listener::new(&fixture);
    // A callback registered on a readable board forces dispatch to write its lease. This older
    // store's schema guard refuses that write while the Hub can keep reading the current board.
    fixture
        .store
        .callback(
            "worker",
            Some(&format!("uds:{}", listener.path.display())),
            true,
            false,
        )
        .unwrap();
    // Queue owner work without delivering it through another writer during the server's wait.
    fixture
        .elsewhere()
        .owner_callback("retry-owner", "Owner work survives retry")
        .unwrap();
    let old = reader(&fixture, Schema::this_build().before()).unwrap();
    hub.board_read(&old).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.failed, 1);
    assert!(
        hub.health
            .report(NOW)
            .dispatch
            .error
            .unwrap()
            .contains("nothing was written")
    );
    fixture
        .store
        .raw()
        .row("task", "tools/pending")
        .set(json!({"title": "Changed while deferred"}))
        .unwrap();
    // No more dispatch attempts before its deadline, but reads and another project's dispatch run.
    for _ in 0..3 {
        hub.board_read(&old).unwrap();
        healthy.board_read(&healthy_board.store).unwrap();
    }
    assert_eq!(hub.health.report(NOW).dispatch.failed, 1);
    assert_eq!(healthy.health.report(NOW).dispatch.ok, 3);
    assert_eq!(
        hub.task_snapshot().unwrap().tasks[0].title,
        "Changed while deferred"
    );
    // Chat uses independent state even when dispatch failed.
    assert_eq!(
        hub.chat_retry
            .lock()
            .unwrap()
            .run(&hub.retry_clock, || Ok::<_, ()>(7)),
        Some(Ok(7))
    );
    pass(&clock, 1);
    hub.board_read(&old).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.failed, 2);
    // Switching to a repaired store does not bypass the dispatch deadline.
    pass(&clock, 1);
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.ok, 0);
    pass(&clock, 1);
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.ok, 1);
    listener.received("Owner work survives retry");
    assert_eq!(
        fixture
            .store
            .raw()
            .row("chat_event", "retry-owner")
            .field("delivered")
            .unwrap(),
        true
    );
    fixture
        .elsewhere()
        .owner_callback("retry-owner-again", "New owner work after success")
        .unwrap();
    hub.board_read(&old).unwrap();
    assert_eq!(hub.health.report(NOW).dispatch.failed, 3);
    pass(&clock, 1);
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(
        hub.health.report(NOW).dispatch.ok,
        2,
        "success reset the failure streak"
    );
    listener.received("New owner work after success");
}
