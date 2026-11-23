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
    Store::opening_on(
        fixture.other_database(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::read().at(schema),
    )
    .map_err(|e| e.to_string())
}

/// A scratch native recipient. Its stop wakes accept and joins the owned thread; frame reads have
/// only a stall backstop. The short socket path is unique to this fixture's database.
pub(super) struct Listener {
    pub(super) path: PathBuf,
    stopped: Arc<AtomicBool>,
    messages: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    pub(super) fn new(fixture: &Fixture) -> Self {
        Self::named(fixture, "")
    }

    /// Another of `fixture`'s listeners, told apart by `name`.
    pub(super) fn named(fixture: &Fixture, name: &str) -> Self {
        let path = PathBuf::from(format!(
            "/tmp/symphony-retry-{}-{}{name}.sock",
            std::process::id(),
            fixture.store.database().database()
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

    pub(super) fn through(&self, marker: &str) -> Vec<String> {
        let mut messages = Vec::new();
        loop {
            let frame = self
                .messages
                .recv_timeout(test_time::backstop(Duration::from_secs(10)))
                .unwrap_or_else(|error| {
                    panic!(
                        "callback {marker} stalled: {error}; {}",
                        test_time::Load::now()
                    )
                });
            let envelope: serde_json::Value = serde_json::from_str(&frame).unwrap();
            let text = envelope["message"]["content"].as_str().unwrap().to_string();
            let found = text.contains(marker);
            messages.push(text);
            if found {
                return messages;
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
fn the_clock_recovers_an_acknowledged_owner_task_after_its_writer_disappears() {
    let tree = Tree::new();
    let fixture = Fixture::new(Seed::default().task("tools/base", "ready"));
    let orchestrator = fixture.session("orchestrator");
    let sender = fixture.session("worker");
    fixture
        .store
        .take_role("tools/base", &orchestrator, "orchestrator")
        .unwrap();
    fixture.advance(symphony_board::health::LIVE_SECONDS + 1.0);
    fixture.store.touch(&orchestrator).unwrap();
    let listener = Listener::new(&fixture);
    fixture
        .store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    assert!(
        fixture
            .store
            .send_agent(&sender, &orchestrator, "before-acknowledgement")
            .unwrap()
    );
    listener.through("before-acknowledgement");

    let new = symphony_board::NewTask::new(
        "tools/clock-owner",
        "Owner task after interruption",
        "owner",
    );
    let fast = fixture.other().dispatching_by(Box::new(|| {
        panic!("optional work ran before acknowledgement")
    }));
    let queries = fast.database().queries();
    let count = || queries.within(&(i64::MIN..=i64::MAX)).0.count;
    let before = count();
    let (made, cpu) = test_time::cpu::spent(|| {
        fast.persist_owner_task(&new, "clock-owner-receipt")
            .unwrap()
    });
    let acknowledgement = count() - before;
    assert_eq!(acknowledgement, 1);
    assert!(made.roles.is_empty());
    assert_eq!(made.created_by, "owner");
    drop(fast); // No in-memory callback, queued worker or save Store remains to finish the give.

    let (hub, clock) = clocked(&tree);
    let mut store = None;
    let mut offline = true;
    let mut connect = |_: &std::path::Path| {
        if std::mem::take(&mut offline) {
            Err("private board temporarily unavailable".into())
        } else {
            Ok(fixture.other())
        }
    };
    hub.board_tick(&mut store, &mut connect);
    assert!(store.is_none());
    assert!(fixture.store.get(&new.slug).unwrap().roles.is_empty());
    pass(&clock, 1);
    let before_completion = count();
    hub.board_tick(&mut store, &mut connect);
    let completion = count() - before_completion;
    assert!(store.is_some());
    assert!(
        hub.task_snapshot()
            .unwrap()
            .tasks
            .iter()
            .any(|task| task.slug == new.slug)
    );
    let given = format!("GIVEN 1 open task is yours to orchestrate: {}", new.slug);
    // This waits for the real clock's native delivery, before a manual send can flush anything.
    let mut messages = listener.through(&given);
    let task = fixture.store.get(&new.slug).unwrap();
    assert_eq!(task.holders("orchestrator"), [orchestrator.as_str()]);
    assert_eq!(task.created_by, "owner");
    assert_eq!(
        task.fields["_symphony"]["owner_save"],
        "clock-owner-receipt"
    );
    let thread = fixture.store.thread(&new.slug).unwrap();
    assert_eq!(thread.len(), 2);
    assert_eq!(
        (thread[0].sender.as_str(), thread[0].text.as_str()),
        ("owner", "created by owner")
    );
    assert!(matches!(
        fixture
            .other()
            .persist_owner_task(&new, "clock-owner-receipt"),
        Err(symphony_board::Error::Refused(_))
    ));
    drop(store);
    let root = hub.root.clone();
    drop(hub);

    let mut restarted = Hub::new(&root, &root.join("build/client"));
    restarted.clock = Box::new(|| NOW);
    let mut store = None;
    for _ in 0..2 {
        restarted.board_tick(&mut store, &mut |_| Ok(fixture.other()));
    }
    assert_eq!(fixture.store.thread(&new.slug).unwrap().len(), 2);
    assert!(
        fixture
            .store
            .send_agent(&sender, &orchestrator, "after-clock-restart")
            .unwrap()
    );
    messages.extend(listener.through("after-clock-restart"));
    assert_eq!(
        messages
            .iter()
            .flat_map(|text| text.lines())
            .filter(|line| *line == given)
            .count(),
        1,
        "{messages:?}"
    );
    println!(
        "owner acknowledgement: {acknowledgement} request, {cpu:?} CPU; later clock completion: {completion} requests"
    );
}

#[test]
fn the_clock_recovers_creation_delivery_without_an_orchestrator_give() {
    let tree = Tree::new();
    let fixture = Fixture::new(Seed::default().task("tools/parent", "ready"));
    let orchestrator = fixture.session("orchestrator");
    let sender = fixture.session("worker");
    fixture
        .store
        .take_role("tools/parent", &orchestrator, "orchestrator")
        .unwrap();
    let listener = Listener::new(&fixture);
    fixture
        .store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    assert!(
        fixture
            .store
            .send_agent(&sender, &orchestrator, "before-inherited-creation")
            .unwrap()
    );
    listener.through("before-inherited-creation");
    let new = symphony_board::NewTask {
        parent: Some("tools/parent".into()),
        ..symphony_board::NewTask::new("tools/parent/owner", "Owner child", "owner")
    };
    let writer = fixture.other().dispatching_by(Box::new(|| {
        panic!("creation delivery ran before acknowledgement")
    }));
    writer
        .persist_owner_task(&new, "inherited-owner-receipt")
        .unwrap();
    drop(writer);
    let (hub, _) = clocked(&tree);
    let mut store = None;
    hub.board_tick(&mut store, &mut |_| Ok(fixture.other()));
    assert!(store.is_some());
    let event = format!("NEW {} ready — {}", new.slug, new.title);
    // No give can flush this creation: orchestration is inherited and the clock writes no role.
    let mut messages = listener.through(&event);
    let task = fixture.store.get(&new.slug).unwrap();
    assert!(
        task.roles.is_empty(),
        "no orchestrator role was given to the child"
    );
    let thread = fixture.store.thread(&new.slug).unwrap();
    assert_eq!(
        thread.len(),
        1,
        "no give or other write assisted callback recovery"
    );
    assert_eq!(
        (thread[0].sender.as_str(), thread[0].text.as_str()),
        ("owner", "created by owner")
    );
    let root = hub.root.clone();
    drop(store);
    drop(hub);
    let mut restarted = Hub::new(&root, &root.join("build/client"));
    restarted.clock = Box::new(|| NOW);
    let mut store = None;
    restarted.board_tick(&mut store, &mut |_| Ok(fixture.other()));
    assert!(
        fixture
            .store
            .send_agent(&sender, &orchestrator, "after-inherited-restart")
            .unwrap()
    );
    messages.extend(listener.through("after-inherited-restart"));
    assert_eq!(
        messages
            .iter()
            .flat_map(|text| text.lines())
            .filter(|line| *line == event)
            .count(),
        1,
        "{messages:?}"
    );
    assert_eq!(fixture.store.thread(&new.slug).unwrap().len(), 1);
}

#[test]
fn schema_open_failures_are_bounded_across_ticks_and_recover_only_when_due() {
    let tree = Tree::new();
    let (hub, clock) = clocked(&tree);
    let fixture = Fixture::new(Seed::default().task("tools/pending", "ready"));
    // A newer reader cannot open this board. It must never migrate it to help a retry.
    let newer = Schema::this_build().then_each("");
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
    // The client's observations use the real wall clock, independently of the board's seeded clock.
    let count = || {
        old.database()
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

#[test]
fn the_clocks_give_settles_a_held_tasks_lost_holder_once_under_its_retry() {
    use crate::testing::board::{held_flow, take_edge};
    use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
    let tree = Tree::new();
    let (hub, _clock) = clocked(&tree);
    let fixture = Fixture::new(Seed::default());
    let (orch, amber) = (fixture.session("orchestrator"), fixture.session("worker"));
    fixture
        .store
        .activate_workflow(&held_flow(), &symphony_board::workflow::Host::local())
        .unwrap();
    fixture
        .store
        .create(&symphony_board::NewTask::new("sim/jump", "Jump", &orch))
        .unwrap();
    take_edge(&fixture.store, "sim/jump", &amber, "claim");
    fixture.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
    fixture.store.touch(&orch).unwrap();
    let at = || {
        let p = fixture
            .store
            .workflow_position("sim/jump")
            .unwrap()
            .unwrap();
        format!("{}/{}@{}", p.phase, p.state, p.revision)
    };
    // With no session pulling, waiting, watching or writing, the clock recovers the lost holder
    // by its position's action, and says it wrote; once.
    assert!(hub.give(&fixture.store, &fixture.store.clock_board().unwrap()));
    assert_eq!(at(), "build/available@3");
    assert!(!hub.give(&fixture.store, &fixture.store.clock_board().unwrap()));
    assert_eq!(at(), "build/available@3");
}

/// The hold's file of the board of the repository at `root`, where a reach reads it.
fn hold_file(root: &std::path::Path) -> PathBuf {
    let folder = root.join(".git").join(symphony_board::server::FOLDER);
    std::fs::create_dir_all(&folder).unwrap();
    folder.join("hold")
}

/// Hold the board of the repository at `root` as a move does, starting and stopping nothing: what
/// a reach of it is refused with until [`release`].
fn hold(root: &std::path::Path) -> String {
    let hold = symphony_board::server::Hold {
        by: "silver-crane-104".into(),
        why: "its move to PostgreSQL".into(),
        at: 0,
        pid: 1,
    };
    std::fs::write(hold_file(root), serde_json::to_string(&hold).unwrap()).unwrap();
    symphony_board::server::ServerError::Held(hold).to_string()
}

fn release(root: &std::path::Path) {
    std::fs::remove_file(hold_file(root)).unwrap();
}

#[test]
fn a_held_boards_page_reader_opens_at_the_first_request_after_its_release() {
    let tree = Tree::new();
    let (mut hub, clock) = clocked(&tree);
    let root = hub.root.clone();
    let fixture = Fixture::empty();
    let attempts = Arc::new(AtomicUsize::new(0));
    let refusal = Arc::new(Mutex::new(Some(hold(&root))));
    let (count, refused) = (attempts.clone(), refusal.clone());
    hub.connect_reader = Box::new(move |_| {
        count.fetch_add(1, SeqCst);
        match refused.lock().unwrap().clone() {
            Some(refusal) => Err(refusal),
            None => reader(&fixture, Schema::this_build()),
        }
    });
    // Held for over a minute: an attempt at 0, 1, 3, 7, 15, 31 and 63 s, the next due at 123 s.
    for second in 0..=64 {
        let said = hub.reader().err().unwrap();
        assert!(said.contains("held by silver-crane-104"), "{said}");
        if second < 64 {
            pass(&clock, 1);
        }
    }
    assert_eq!(attempts.load(SeqCst), 7);
    // Released at 64 s, with 59 s of its deadline left: the next request opens it.
    release(&root);
    *refusal.lock().unwrap() = None;
    let store = hub.reader().unwrap();
    assert_eq!(attempts.load(SeqCst), 8);
    // A board that is down afterwards, and not held, keeps its deadlines however it is repaired.
    hub.reader_failed(&store);
    *refusal.lock().unwrap() = Some("the board's server did not come up".into());
    assert!(hub.reader().is_err());
    *refusal.lock().unwrap() = None;
    for _ in 0..1000 {
        assert!(hub.reader().is_err(), "down, repaired before its deadline");
    }
    assert_eq!(attempts.load(SeqCst), 9);
    pass(&clock, 1);
    assert!(hub.reader().is_ok());
}

#[test]
fn a_released_board_that_is_not_up_yet_waits_one_second_not_the_holds_minute() {
    let tree = Tree::new();
    let (mut hub, clock) = clocked(&tree);
    let root = hub.root.clone();
    let fixture = Fixture::empty();
    let refusal = Arc::new(Mutex::new(Some(hold(&root))));
    let refused = refusal.clone();
    hub.connect_reader = Box::new(move |_| match refused.lock().unwrap().clone() {
        Some(refusal) => Err(refusal),
        None => reader(&fixture, Schema::this_build()),
    });
    for _ in 0..=64 {
        assert!(hub.reader().is_err());
        pass(&clock, 1);
    }
    release(&root);
    *refusal.lock().unwrap() = Some("the board's server did not come up".into());
    assert_eq!(
        hub.reader().err().as_deref(),
        Some("the board's server did not come up"),
        "the release made an attempt due"
    );
    *refusal.lock().unwrap() = None;
    assert!(
        hub.reader().is_err(),
        "down now, and so on its own deadline"
    );
    pass(&clock, 1);
    assert!(hub.reader().is_ok());
}

#[test]
fn a_held_boards_worker_opens_at_the_first_tick_after_its_release() {
    let tree = Tree::new();
    let (hub, clock) = clocked(&tree);
    let fixture = Fixture::new(Seed::default().task("tools/pending", "ready"));
    let refusal = hold(&hub.root);
    let released = AtomicBool::new(false);
    let attempts = AtomicUsize::new(0);
    let mut connect = |_: &std::path::Path| {
        attempts.fetch_add(1, SeqCst);
        if released.load(SeqCst) {
            reader(&fixture, Schema::this_build())
        } else {
            Err(refusal.clone())
        }
    };
    let mut board = None;
    let start = *clock.lock().unwrap();
    // A tick each tenth of a second for 100 s of hold.
    for tick in 0..1_000 {
        *clock.lock().unwrap() = start + Duration::from_millis(tick * 100);
        hub.board_tick(&mut board, &mut connect);
    }
    assert_eq!(attempts.load(SeqCst), 7, "at 0, 1, 3, 7, 15, 31 and 63 s");
    let said = hub.task_snapshot().err().unwrap();
    assert!(said.contains("held by silver-crane-104"), "{said}");
    // Released at 100 s, 23 s before its deadline: the next tick opens and reads it.
    release(&hub.root);
    released.store(true, SeqCst);
    hub.board_tick(&mut board, &mut connect);
    assert_eq!(attempts.load(SeqCst), 8);
    assert!(board.is_some());
    assert_eq!(hub.task_snapshot().unwrap().tasks.len(), 1);
}

#[test]
fn a_hold_that_does_not_read_still_holds_and_a_folder_no_repository_holds_is_never_held() {
    let tree = Tree::new();
    let (hub, _clock) = clocked(&tree);
    assert!(!hub.board_held());
    std::fs::write(hold_file(&hub.root), "not a hold").unwrap();
    assert!(hub.board_held());
    release(&hub.root);
    assert!(!hub.board_held());
    let plain = tree.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(!Hub::new(&plain, &plain.join("build/client")).board_held());
}
