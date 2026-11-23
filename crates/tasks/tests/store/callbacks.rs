use super::common::{Fixture, sweep_presence};
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use symphony_board::ChatSeat;

mod in_process;

/// How long a [`Listener`] waits for a frame to close, or for the next frame: the backstop of the
/// 3 s it once had. Every frame it waits for was delivered within the write that sent it
/// ([`Listener::drain`] checks so), so these waits only read what its socket holds already.
const LISTENING: Duration = test_time::backstop(Duration::from_secs(3));

pub(super) struct Listener {
    pub(super) path: PathBuf,
    stop: mpsc::Sender<()>,
    messages: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    pub(super) fn new(database: &str) -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "symphony-idle-{}-{database}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (send, messages) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(true).unwrap();
                        let deadline = Instant::now() + LISTENING;
                        let mut bytes = Vec::new();
                        let mut buffer = [0; 4096];
                        loop {
                            match stream.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    assert!(
                                        Instant::now() < deadline,
                                        "callback frame did not close"
                                    );
                                    if stopped.recv_timeout(Duration::from_millis(10)).is_ok() {
                                        return;
                                    }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                                Err(error) => panic!("callback frame: {error}"),
                            }
                        }
                        if !bytes.is_empty() {
                            send.send(String::from_utf8(bytes).unwrap()).unwrap();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.recv_timeout(Duration::from_millis(10)).is_ok() {
                            break;
                        }
                    }
                    Err(error) => panic!("callback listener: {error}"),
                }
            }
        });
        Self {
            path,
            stop,
            messages,
            thread: Some(thread),
        }
    }

    fn receive(&self, marker: &str) {
        let deadline = Instant::now() + LISTENING;
        loop {
            let text = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("callback {marker}: {error}"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["type"], "user");
            assert_eq!(value["message"]["role"], "user");
            if value["message"]["content"]
                .as_str()
                .unwrap()
                .contains(marker)
            {
                return;
            }
        }
    }

    pub(super) fn text(&self) -> String {
        let frame = self.messages.recv_timeout(LISTENING).unwrap();
        let value: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        value["message"]["content"].as_str().unwrap().to_string()
    }

    /// A real delivery barrier proves absence without a timing-based empty receive. The barrier is
    /// delivered within the write that sends it, as `send_agent` answers: the guard on how fast a
    /// post reaches its recipient, a count and not a time, so the listener's own waits on what is
    /// in its socket already can be backstops without letting a slower delivery pass.
    pub(super) fn drain(&self, f: &Fixture, sender: &str, handle: &str) -> Vec<String> {
        let delivered = f
            .store
            .send_agent(sender, handle, "delivery-barrier")
            .unwrap();
        assert!(
            delivered,
            "the barrier to {handle} was left undelivered by its write"
        );
        let barrier = format!("AGENT from {sender} to {handle} — delivery-barrier");
        let mut lines = Vec::new();
        loop {
            let text = self.text();
            for line in text.lines() {
                if line == barrier {
                    return lines;
                }
                lines.push(line.to_string());
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = self.thread.take().unwrap().join();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn state(f: &Fixture, handle: &str) -> Value {
    f.store.raw().row("callback_state", handle).get().unwrap()
}

#[test]
fn plain_orchestrators_receive_queued_chat_and_preserve_explicit_priority() {
    let f = Fixture::new(crate::common::Seed::default());
    let first = f.session("orchestrator");
    let second = f.session("orchestrator");
    assert_eq!(f.store.chat_seat().unwrap(), None);
    f.store
        .owner_callback("before-registration", "queued owner ask")
        .unwrap();
    let listener = Listener::new(f.store.database().database());
    let address = format!("uds:{}", listener.path.display());
    f.store
        .callback(&first, Some(&address), false, false)
        .unwrap();
    // A plain callback hears chat as a fallback, holding no seat.
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((first.clone(), ChatSeat::Orchestrator))
    );
    assert_eq!(f.store.chat_recipient().unwrap(), Some(first.clone()));
    listener.receive("queued owner ask");
    assert_eq!(
        f.store
            .raw()
            .row("chat_event", "before-registration")
            .field("delivered")
            .unwrap(),
        true
    );

    let other = Listener::new(&format!("{}-other", f.store.database().database()));
    f.store
        .callback(
            &second,
            Some(&format!("uds:{}", other.path.display())),
            false,
            false,
        )
        .unwrap();
    let winner = first.clone().min(second.clone());
    // This checks store integration; the unit case permutes rows to guard the tie-break itself.
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((winner.clone(), ChatSeat::Orchestrator))
    );
    let fallback = f.session("worker");
    let fallback_listener = Listener::new(&format!("{}-fallback", f.store.database().database()));
    f.store
        .callback(
            &fallback,
            Some(&format!("uds:{}", fallback_listener.path.display())),
            true,
            true,
        )
        .unwrap();
    // A fallback remains preferred even when it does not occupy the explicit seat.
    f.store
        .raw()
        .row("callback_state", fallback.as_str())
        .set(json!({"chat": false}))
        .unwrap();
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((fallback.clone(), ChatSeat::NextInLine))
    );
    let seat = f.session("worker");
    let seat_listener = Listener::new(&format!("{}-seat", f.store.database().database()));
    f.store
        .callback(
            &seat,
            Some(&format!("uds:{}", seat_listener.path.display())),
            true,
            false,
        )
        .unwrap();
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((seat.clone(), ChatSeat::Held))
    );
    f.store.callback(&seat, None, false, false).unwrap();
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((fallback.clone(), ChatSeat::NextInLine))
    );
    f.store.callback(&fallback, None, false, false).unwrap();
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((winner, ChatSeat::Orchestrator))
    );

    f.store.callback(&first, None, false, false).unwrap();
    f.store.leave(&second).unwrap();
    assert_eq!(f.store.chat_seat().unwrap(), None);
    assert_eq!(f.store.chat_recipient().unwrap(), None);
}

fn sweep_callbacks(f: &Fixture, listener: &Listener) {
    sweep_presence(&f.store, listener.path.with_extension("reap.json"));
}

#[test]
fn failed_chat_delivery_and_presence_sweep_restore_the_registered_priority() {
    for (chat, if_free, seat) in [
        (false, false, false),
        (true, false, true),
        (true, true, true),
        (true, true, false),
    ] {
        let f = Fixture::new(crate::common::Seed::default());
        let orchestrator = f.session("orchestrator");
        let listener = Listener::new(f.store.database().database());
        let address = format!("uds:{}", listener.path.display());
        f.store
            .callback(&orchestrator, Some(&address), chat, if_free)
            .unwrap();
        if chat && !seat {
            // An if-free registration can remain a fallback after another seat has displaced it.
            f.store
                .raw()
                .row("callback_state", orchestrator.as_str())
                .set(json!({"chat": false}))
                .unwrap();
        }
        assert_eq!(
            f.store.chat_recipient().unwrap(),
            Some(orchestrator.clone())
        );
        // Removing the socket leaves its registered owner process alive: a real retryable error.
        drop(listener);
        f.store
            .owner_callback("failed-owner-chat", "recover this owner ask")
            .unwrap();
        for _ in 0..2 {
            f.advance(6.0);
            f.store.dispatch_callbacks().unwrap();
        }
        let suspended = state(&f, &orchestrator);
        assert_eq!(suspended["chat_resume"], true);
        assert_eq!(suspended["chat_resume_seat"], seat);
        assert_eq!(suspended["chat_resume_fallback"], chat && if_free);
        assert_eq!(f.store.chat_recipient().unwrap(), None);

        let listener = Listener::new(f.store.database().database());
        sweep_callbacks(&f, &listener);
        let recovered = state(&f, &orchestrator);
        assert_eq!(recovered["chat_resume"], false);
        assert_eq!(recovered["chat"], seat);
        assert_eq!(recovered["chat_fallback"], chat && if_free);
        f.store.dispatch_callbacks().unwrap();
        listener.receive("recover this owner ask");

        let fallback = f.session("worker");
        let other = Listener::new(&format!("{}-late", f.store.database().database()));
        f.store
            .callback(
                &fallback,
                Some(&format!("uds:{}", other.path.display())),
                true,
                true,
            )
            .unwrap();
        assert_eq!(
            f.store.chat_recipient().unwrap(),
            Some(if seat { orchestrator } else { fallback }),
            "a recovered plain recipient or non-seat fallback must not occupy the seat"
        );
    }
}

#[test]
fn stale_failed_delivery_preserves_the_priority_saved_by_a_newer_dispatcher() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    for if_free in [false, true] {
        let f = Arc::new(Fixture::new(crate::common::Seed::default()));
        let handle = f.session("worker");
        let listener = Listener::new(f.store.database().database());
        f.store
            .callback(
                &handle,
                Some(&format!("uds:{}", listener.path.display())),
                true,
                if_free,
            )
            .unwrap();
        drop(listener);
        f.store
            .owner_callback("overlap", "owner chat across overlapping failures")
            .unwrap();
        f.advance(6.0);
        f.store.dispatch_callbacks().unwrap();
        f.advance(6.0);

        let leased_reads = Arc::new(AtomicUsize::new(0));
        let meanwhile = Arc::clone(&f);
        let reads = Arc::clone(&leased_reads);
        let target = handle.clone();
        let newer = f.other();
        let stale = f.busy(move || {
            let lease = meanwhile
                .store
                .raw()
                .row("callback_lease", target.as_str())
                .field("until")
                .unwrap();
            if lease.as_f64().unwrap_or(0.0) > 0.0 && reads.fetch_add(1, Ordering::SeqCst) == 4 {
                // After refreshing the board, checking pending work, selecting due events and
                // checking its deadline before it sends, the leased worker reads the clock to
                // record its failed send. Its send began inside its lease. Expire the lease at
                // that fifth read so another real dispatcher suspends the recipient before the
                // stale worker records the same failure.
                meanwhile.advance(61.0);
                newer.dispatch_callbacks().unwrap();
                assert_eq!(state(&meanwhile, &target)["chat_resume"], true);
            }
        });
        stale.dispatch_callbacks().unwrap();
        assert!(leased_reads.load(Ordering::SeqCst) >= 5);
        assert_eq!(
            f.store
                .raw()
                .row("callback_event", json!([handle, "chat-overlap"]))
                .field("attempts")
                .unwrap(),
            4,
            "both dispatchers must have recorded their real failed send"
        );
        let suspended = state(&f, &handle);
        assert_eq!(suspended["chat_resume_seat"], true);
        assert_eq!(suspended["chat_resume_fallback"], if_free);

        let listener = Listener::new(f.store.database().database());
        sweep_callbacks(&f, &listener);
        let recovered = state(&f, &handle);
        assert_eq!(recovered["chat_resume"], false);
        assert_eq!(recovered["chat"], true);
        assert_eq!(recovered["chat_fallback"], if_free);
        f.store.dispatch_callbacks().unwrap();
        listener.receive("owner chat across overlapping failures");
    }
}

#[test]
fn a_plain_orchestrator_needs_live_health_or_confirmed_native_presence() {
    let f = Fixture::new(crate::common::Seed::default());
    let orchestrator = f.session("orchestrator");
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .raw()
        .row("session", orchestrator.as_str())
        .set(json!({"last_seen": f.now() - symphony_board::health::LIVE_SECONDS - 1.0}))
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), None);
    f.store
        .raw()
        .row("callback_state", orchestrator.as_str())
        .set(json!({"confirmed_until": f.now() + 60.0}))
        .unwrap();
    assert_eq!(
        f.store.chat_recipient().unwrap(),
        Some(orchestrator.clone())
    );
    f.store
        .raw()
        .row("callback_state", orchestrator.as_str())
        .set(json!({"chat_resume": true}))
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), None);
}

#[test]
fn legacy_writers_do_not_repeat_unknown_notices_across_the_lost_threshold() {
    use symphony_board::{Health, NewSession};

    let f = Fixture::new(crate::common::Seed::default());
    let orchestrator = f.session("orchestrator");
    let child = f.session_with(NewSession {
        parent: Some(orchestrator.clone()),
        ..NewSession::new("worker")
    });
    f.task("tools/mixed");
    f.store.claim("tools/mixed", &child).unwrap();
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    assert_eq!(state(&f, &orchestrator)["seen"]["children"][&child], "Live");
    f.store
        .raw()
        .row("callback_state", child.as_str())
        .put(json!({
            "handle": child, "address": "codex:abcdef01", "host": crate::common::HOST,
            "confirmed_until": f.now() - 1.0, "lease": f.now() + 1_000_000.0,
        }))
        .unwrap();
    let event_count = || {
        let counter = json!(["callback-event", orchestrator]);
        match f.store.raw().row("counter", counter).field("last").unwrap() {
            Value::Null => 0,
            last => last.as_u64().unwrap(),
        }
    };

    // Keep the parent clock fixed so PULL is not due. Age only the child, as an older writer
    // observes it: Stale, then Lost, then busy with a dead local monitor, then a fresh heartbeat.
    for (age, busy, legacy_health, health, notices) in [
        (500.0, false, "Stale", Health::Unknown, 1),
        (700.0, false, "Lost", Health::Unknown, 0),
        (0.0, false, "Live", Health::Live, 0),
        (700.0, true, "Stale", Health::Unknown, 1),
        (700.0, false, "Lost", Health::Unknown, 0),
        (0.0, false, "Live", Health::Live, 0),
        (700.0, false, "Lost", Health::Unknown, 1),
    ] {
        let seen = f.now() - age;
        let (busy_until, pid) = if busy {
            (json!(f.now() + 60.0), json!(999_999_999))
        } else {
            (Value::Null, Value::Null)
        };
        f.store
            .raw()
            .row("session", child.as_str())
            .set(json!({
                "connected": seen, "last_seen": seen, "busy_until": busy_until, "pid": pid,
                "host": crate::common::HOST,
            }))
            .unwrap();
        assert_eq!(f.health(&child), health);
        let before = event_count();
        for round in 0..4 {
            // Main writes its ordinary Seen and preserves fields it does not know. This is the
            // persisted old-writer boundary, not a test-only mode in the new dispatcher.
            let mut legacy_seen = state(&f, &orchestrator)["seen"].clone();
            legacy_seen["children"][&child] = json!(legacy_health);
            f.store
                .raw()
                .row("callback_state", orchestrator.as_str())
                .set(json!({"seen": legacy_seen}))
                .unwrap();
            f.store.dispatch_callbacks_snapshot().unwrap();
            let after = state(&f, &orchestrator);
            assert_eq!(
                after["seen"]["children"][&child], legacy_health,
                "main must see the same heartbeat baseline at age {age}, round {round}"
            );
            assert_eq!(
                after["unknown_children"],
                if health == Health::Unknown {
                    json!([child])
                } else {
                    json!([])
                },
                "fresh Live evidence must clear the richer observation"
            );
            assert_eq!(
                event_count() - before,
                notices,
                "alternating writers must not create another event at age {age}, round {round}"
            );
        }
        if notices != 0 {
            listener.receive(&format!("LIVENESS UNKNOWN {child}"));
        }
    }

    // Expiring the Unknown grace must clear its richer observation, queue LOST once, and leave
    // a heartbeat-derived baseline that both dispatcher generations can read without repeating it.
    f.store
        .raw()
        .row("callback_state", child.as_str())
        .set(json!({"confirmed_until": f.now() - 601.0}))
        .unwrap();
    assert_eq!(f.health(&child), Health::Lost);
    f.store.dispatch_callbacks_snapshot().unwrap();
    let after_first = event_count();
    for round in 0..4 {
        f.store.dispatch_callbacks_snapshot().unwrap();
        let after = state(&f, &orchestrator);
        assert_eq!(after["seen"]["children"][&child], "Lost");
        assert_eq!(after["unknown_children"], json!([]));
        assert_eq!(event_count(), after_first, "events repeat in round {round}");
    }
    let sender = f.session("worker");
    let lines = listener.drain(&f, &sender, &orchestrator);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with(&format!("LOST {child} —")))
            .count(),
        1,
        "the newly claimable work also triggers PULL, but LOST is delivered once: {lines:?}"
    );
    assert!(f.store.claim("tools/mixed", &orchestrator).is_ok());
}

#[test]
fn idle_callback_snapshots_drain_queued_work_without_rewriting_observations() {
    let f = Fixture::new(crate::common::Seed::default());
    assert!(f.store.dispatch_callbacks_snapshot().unwrap().is_none());
    let worker = f.session("worker");
    let sender = f.session("worker");
    f.task("tools/idle");
    f.store.claim("tools/idle", &worker).unwrap();
    f.store
        .take_role("tools/idle", &sender, "orchestrator")
        .unwrap();
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &worker,
            Some(&format!("uds:{}", listener.path.display())),
            true,
            false,
        )
        .unwrap();
    let before = state(&f, &worker);
    assert_eq!(
        before["statuses"],
        json!({}),
        "workers do not need the global status map"
    );
    assert!(before["snapshot_sequence"].as_u64().unwrap() > 0);

    // Another dispatcher owns this recipient while the direct event is queued.
    f.store
        .raw()
        .row("callback_lease", worker.as_str())
        .set(json!({"until": f.now() + 60.0}))
        .unwrap();
    assert!(
        !f.store
            .send_agent(&sender, &worker, "idle-agent-pending")
            .unwrap()
    );
    f.store
        .raw()
        .row("callback_lease", worker.as_str())
        .set(json!({"until": 0}))
        .unwrap();

    let snapshot = f.store.dispatch_callbacks_snapshot().unwrap().unwrap();
    assert_eq!(snapshot.tasks[0].slug, "tools/idle");
    assert_eq!(snapshot.tasks[0].status, "in_progress");
    listener.receive("idle-agent-pending");
    let after = state(&f, &worker);
    assert_eq!(
        after, before,
        "outbox delivery leaves the unchanged observation alone"
    );
    assert_eq!(after["seen"], before["seen"]);
    assert_eq!(after["statuses"], before["statuses"]);
    assert_eq!(after["inbox_cursor"], before["inbox_cursor"]);
    let idle_rows = || {
        let rows = |table: &str| f.store.raw().table(table).get().unwrap();
        json!({
            "states": rows("callback_state"), "counters": rows("counter"),
            "leases": rows("callback_lease"),
        })
    };
    let idle_before = idle_rows();
    for _ in 0..3 {
        f.store.dispatch_callbacks_snapshot().unwrap();
        assert_eq!(
            idle_rows(),
            idle_before,
            "an idle tick writes no state, counter or lease"
        );
    }
    let requests = f.store.database().requests();
    let began = Instant::now();
    f.store.dispatch_callbacks_snapshot().unwrap();
    let requests = f.store.database().requests() - requests;
    eprintln!(
        "idle callback: {requests} requests in {:.3} ms",
        began.elapsed().as_secs_f64() * 1000.0
    );
    assert!(
        requests <= 8,
        "an unchanged chat worker must not rescan its inbox: {requests} requests"
    );

    f.store
        .say("tools/idle", "owner", "idle-thread-pending", Some(&worker))
        .unwrap();
    listener.receive("idle-thread-pending");
    assert!(f.store.inbox(&worker).unwrap().is_empty());
    f.store
        .owner_callback("idle-chat-key", "idle-chat-pending")
        .unwrap();
    listener.receive("idle-chat-pending");
    // Task writes return their callback flag in the task snapshot. They must still dispatch
    // synchronously, without waiting for the server clock or a manual snapshot sweep.
    let moved = f.store.move_to("tools/idle", "review", &sender).unwrap();
    assert_eq!(moved.status, "review");
    listener.receive("STATUS tools/idle in_progress → review");
    assert_eq!(
        state(&f, &worker)["seen"]["statuses"]["tools/idle"],
        "review"
    );
    assert!(
        state(&f, &worker)["snapshot_sequence"].as_u64().unwrap()
            > before["snapshot_sequence"].as_u64().unwrap()
    );
}

#[test]
fn idle_unchanged_observation_prevents_an_older_snapshot_from_emitting_a_transient() {
    let f = Fixture::new(crate::common::Seed::default());
    let worker = f.session("worker");
    f.task("tools/idle");
    f.store.claim("tools/idle", &worker).unwrap();
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &worker,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    let before = state(&f, &worker);
    assert_eq!(before["seen"]["statuses"]["tools/idle"], "in_progress");
    assert_eq!(before["statuses"], json!({}));
    assert!(before["snapshot_sequence"].as_u64().unwrap() > 0);
    let events = |store: &symphony_board::Store| {
        let counter = json!(["callback-event", worker]);
        match store.raw().row("counter", counter).field("last").unwrap() {
            Value::Null => json!(0),
            last => last,
        }
    };
    let before_events = events(&f.store);
    f.store
        .raw()
        .row("task", "tools/idle")
        .set(json!({"status": "review"}))
        .unwrap();

    // The slow dispatcher has already read the transient board when its clock is read. A newer
    // dispatcher then observes the original state again, matching the saved seen/statuses.
    let other = f.other();
    let slow = f.slow(move || {
        other
            .raw()
            .row("task", "tools/idle")
            .set(json!({"status": "in_progress"}))
            .unwrap();
        let board = other.dispatch_callbacks_snapshot().unwrap().unwrap();
        assert_eq!(board.tasks[0].status, "in_progress");
    });
    let board = slow.dispatch_callbacks_snapshot().unwrap().unwrap();
    assert_eq!(
        board.tasks[0].status, "in_progress",
        "the older read must refresh"
    );
    let after = state(&f, &worker);
    assert_eq!(after["seen"], before["seen"]);
    assert_eq!(after["statuses"], before["statuses"]);
    assert_eq!(
        after, before,
        "refreshing an old read needs no observation write"
    );
    assert_eq!(
        events(&f.store),
        before_events,
        "no transient callback was allocated"
    );
}

#[test]
fn cached_board_inputs_age_without_a_board_read_and_refresh_after_mutation() {
    use crate::common::{GONE, Seed};
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/clock", "in_progress")
            .role("tools/clock", "worker", "executor"),
    );
    assert_eq!(
        f.store.clock_board().unwrap().sessions[0].health,
        symphony_board::Health::Live
    );
    let requests = f.store.database().requests();
    f.advance(GONE);
    let board = f.store.clock_board().unwrap();
    assert_eq!(
        f.store.database().requests() - requests,
        1,
        "only the marker is read"
    );
    assert_eq!(board.sessions[0].health, symphony_board::Health::Lost);
    assert_eq!(board.tasks[0].roles[0].health, symphony_board::Health::Lost);
    f.store
        .raw()
        .table("task")
        .set(json!({"title": "Changed by another writer"}))
        .unwrap();
    assert_eq!(
        f.store.clock_board().unwrap().tasks[0].title,
        "Changed by another writer"
    );
    let marker = f.store.marker().unwrap();
    match f.store.database() {
        symphony_board::Database::SurrealDb(client) => assert!(client.query("BEGIN TRANSACTION; UPDATE task SET title = 'Uncommitted'; THROW 'rollback'; COMMIT TRANSACTION;", json!({})).is_err()),
        symphony_board::Database::PostgreSql(client) => {
            assert!(client.simple("BEGIN; UPDATE task SET title = 'Uncommitted'; DO $test$ BEGIN RAISE EXCEPTION 'rollback'; END $test$; COMMIT;").is_err());
            client.simple("ROLLBACK").unwrap();
        }
    }
    assert_eq!(
        f.store.marker().unwrap(),
        marker,
        "a rolled-back write must not publish a marker"
    );
    assert_eq!(
        f.store.clock_board().unwrap().tasks[0].title,
        "Changed by another writer"
    );
}

#[test]
fn an_idle_clock_reports_stale_and_lost_children_without_a_board_mutation() {
    use crate::common::{Seed, session};
    let mut child = session("child");
    child.parent = Some("parent".into());
    let f = Fixture::new(
        Seed::default()
            .session("parent")
            .session_row(child)
            .task("tools/child", "in_progress")
            .role("tools/child", "child", "executor"),
    );
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "parent",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    let requests = f.store.database().requests();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        f.store.database().requests() - requests,
        4,
        "the warm tick reaches the idle shortcut after its task-block reconciliation read"
    );
    let marker = f.store.marker().unwrap();
    f.advance(91.0);
    assert_eq!(f.store.marker().unwrap(), marker);
    f.store.dispatch_callbacks().unwrap();
    listener.receive("STALE child");
    f.advance(510.0);
    f.store.dispatch_callbacks().unwrap();
    listener.receive("LOST child");
}

#[test]
fn finished_history_is_not_retained_and_new_ready_work_still_arrives() {
    use crate::common::{Seed, session};
    let mut orch = session("orchestrator");
    orch.kind = "orchestrator".into();
    // Keep health and the canonical count stable while the periodic check falls due.
    orch.busy_until = Some(crate::common::NOW + 1200.0);
    let mut seed = Seed::default()
        .session("sender")
        .session_row(orch)
        .task("tools/root", "in_progress")
        .role("tools/root", "orchestrator", "orchestrator");
    for i in 0..256 {
        let slug = format!("tools/history-{i}");
        seed = seed
            .task(&slug, if i % 2 == 0 { "done" } else { "cancelled" })
            .role(&slug, "orchestrator", "orchestrator");
    }
    let f = Fixture::new(seed);
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "orchestrator",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    let pull = "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=1 review_unreviewed=0 landed_open=0 idle=0\nin_progress_orphaned p2 tools/root — Task tools/root";
    assert_eq!(
        listener.text(),
        crate::quiet_callbacks::with_volume("orchestrator", 1, pull)
    );
    assert_eq!(f.health("orchestrator"), symphony_board::Health::Live);
    assert_eq!(f.store.slot_holdings().unwrap()["orchestrator"].len(), 1);
    let saved = state(&f, "orchestrator");
    assert_eq!(saved["statuses"], json!({"tools/root":"in_progress"}));
    assert_eq!(
        saved["seen"]["statuses"],
        json!({"tools/root":"in_progress"})
    );
    assert_eq!(saved["seen"]["roles"].as_array().unwrap().len(), 1);
    assert_eq!(saved["seen"]["verdict_tasks"], json!(["tools/root"]));
    assert!(
        serde_json::to_vec(&saved).unwrap().len() < 2000,
        "finished history must not grow the row"
    );
    f.advance(601.0);
    assert_eq!(f.health("orchestrator"), symphony_board::Health::Live);
    assert_eq!(f.store.slot_holdings().unwrap()["orchestrator"].len(), 1);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        state(&f, "orchestrator"),
        saved,
        "a periodic digest with no new action leaves observation state unchanged"
    );
    assert_eq!(
        f.store
            .raw()
            .row("callback_lease", "orchestrator")
            .field("pull")
            .unwrap(),
        json!(f.now()),
        "the small scheduling checkpoint must still advance"
    );
    assert!(listener.drain(&f, "sender", "orchestrator").is_empty());
    f.store
        .raw()
        .row("task", "tools/history-0")
        .set(json!({"status": "ready"}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    listener.receive("NEW tools/history-0 ready");
}

#[test]
fn a_second_dispatcher_cannot_take_a_live_small_recipient_lease() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let f = Arc::new(Fixture::new(crate::common::Seed::default()));
    let receiver = f.session("worker");
    let sender = f.session("worker");
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &receiver,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .raw()
        .row("callback_lease", receiver.as_str())
        .set(json!({"until": f.now() + 60.0}))
        .unwrap();
    assert!(
        !f.store
            .send_agent(&sender, &receiver, "serialized-direct-message")
            .unwrap()
    );
    f.store
        .raw()
        .row("callback_lease", receiver.as_str())
        .set(json!({"until": 0}))
        .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let checked = Arc::clone(&entered);
    let meanwhile = Arc::clone(&f);
    let other = f.other();
    let slow = f.busy(move || {
        let lease = || {
            meanwhile
                .store
                .raw()
                .row("callback_lease", receiver.as_str())
                .get()
                .unwrap()
        };
        let before = lease();
        if before["until"].as_f64().unwrap_or(0.0) > meanwhile.now()
            && !checked.swap(true, Ordering::SeqCst)
        {
            other.dispatch_callbacks().unwrap();
            assert_eq!(
                lease(),
                before,
                "a competing dispatcher must neither take nor release the lease"
            );
        }
    });
    slow.dispatch_callbacks().unwrap();
    assert!(
        entered.load(Ordering::SeqCst),
        "the competitor ran while the first dispatcher held the lease"
    );
    listener.receive("serialized-direct-message");
}

// SurrealDB alone: installs a SurrealDB event to audit callback-state writes during retry.
#[test]
fn a_failed_chat_waits_without_writes_and_retries_at_its_deadline() {
    let f = Fixture::empty();
    let worker = f.session("worker");
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &worker,
            Some(&format!("uds:{}", listener.path.display())),
            true,
            false,
        )
        .unwrap();
    drop(listener);
    f.store.client().query("DEFINE TABLE callback_audit SCHEMALESS; DEFINE EVENT state_writes ON callback_state WHEN true THEN { UPSERT callback_audit:writes SET count = (count ?? 0) + 1; };", json!({})).unwrap();
    f.store
        .owner_callback("deadline", "retry this owner message")
        .unwrap();
    let state_writes = || {
        f.store
            .raw()
            .row("callback_audit", "writes")
            .field("count")
            .unwrap()
    };
    let writes = state_writes();
    assert_eq!(
        writes,
        json!(1),
        "the first failure marks the callback blocked"
    );
    let rows = || {
        let rows = |table: &str| f.store.raw().table(table).get().unwrap();
        json!({
            "states": rows("callback_state"), "events": rows("callback_event"),
            "counters": rows("counter"), "leases": rows("callback_lease"),
        })
    };
    let before = rows();
    assert_eq!(
        before["events"][0]["attempts"], 1,
        "the first delivery failed"
    );
    for elapsed in [0.0, 4.0] {
        f.advance(elapsed);
        f.store.dispatch_callbacks().unwrap();
        assert_eq!(rows(), before, "backoff takes no lease and writes nothing");
    }
    f.advance(1.0);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        rows()["events"][0]["attempts"],
        2,
        "the clock retries without another board mutation"
    );
    assert_eq!(
        state_writes(),
        writes,
        "another failure must not rewrite an already blocked state row"
    );
}

#[test]
fn a_contended_observation_is_retried_even_without_another_board_change() {
    use crate::common::Seed;
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/contended", "in_progress")
            .role("tools/contended", "worker", "executor"),
    );
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "worker",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .raw()
        .table("callback_lease")
        .set(json!({"until": f.now() + 60.0}))
        .unwrap();
    f.store
        .raw()
        .table("task")
        .set(json!({"status": "review"}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        state(&f, "worker")["seen"]["statuses"]["tools/contended"],
        "in_progress"
    );
    f.store
        .raw()
        .table("callback_lease")
        .set(json!({"until": 0}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    listener.receive("STATUS tools/contended in_progress → review");
    assert_eq!(
        state(&f, "worker")["seen"]["statuses"]["tools/contended"],
        "review"
    );
}

#[test]
fn unchanged_periodic_pull_rearms_across_lost_sessions_and_fresh_stores() {
    use crate::common::{GONE, NOW, Seed, session};
    let mut orchestrator = session("orchestrator");
    orchestrator.kind = "orchestrator".into();
    orchestrator.busy_until = Some(NOW + 5000.0);
    let mut lost = session("lost");
    lost.last_seen = NOW - GONE;
    let f = Fixture::new(
        Seed::default()
            .session_row(orchestrator)
            .session_row(lost)
            .task("tools/timed", "in_progress")
            .role("tools/timed", "lost", "executor"),
    );
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "orchestrator",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    listener.receive("PULL");
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        f.store
            .clock_board()
            .unwrap()
            .sessions
            .iter()
            .find(|session| session.row.handle == "lost")
            .unwrap()
            .health,
        symphony_board::Health::Lost,
        "a Lost session keeps the global idle shortcut off"
    );
    let dispatch_requests = |store: &symphony_board::Store| {
        let before = store.database().requests();
        store.dispatch_callbacks().unwrap();
        store.database().requests() - before
    };
    let baseline = dispatch_requests(&f.store);
    let fresh_baseline = dispatch_requests(&f.other());
    let observation = state(&f, "orchestrator");
    let counters = || f.store.raw().table("counter").get().unwrap();
    let before_counters = counters();
    let leases = || f.store.raw().table("callback_lease").get().unwrap();

    f.advance(601.0);
    let overdue = dispatch_requests(&f.store);
    assert!(
        overdue > baseline,
        "the first overdue tick computes its periodic digest"
    );
    eprintln!(
        "periodic PULL requests: warm idle {baseline}, due {overdue}, fresh idle {fresh_baseline}"
    );
    let checked = leases();
    assert_eq!(checked[0]["pull"], json!(f.now()));
    for _ in 0..5 {
        assert_eq!(
            dispatch_requests(&f.store),
            baseline,
            "an unchanged digest must rearm even while a session is Lost"
        );
        assert_eq!(
            leases(),
            checked,
            "ticks before the new deadline take no lease and write no checkpoint"
        );
    }
    assert_eq!(
        dispatch_requests(&f.other()),
        fresh_baseline,
        "a fresh command must read the durable deadline instead of recomputing the digest"
    );
    f.advance(599.0);
    assert_eq!(dispatch_requests(&f.store), baseline);
    assert_eq!(leases(), checked);
    f.advance(1.0);
    assert!(
        dispatch_requests(&f.store) > baseline,
        "the next periodic check still fires exactly when due"
    );
    assert_eq!(leases()[0]["pull"], json!(f.now()));
    assert_eq!(
        state(&f, "orchestrator"),
        observation,
        "periodic scheduling does not rewrite the large observation"
    );
    assert_eq!(
        counters(),
        before_counters,
        "a periodic checkpoint does not change counters or invalidate the board cache"
    );
}

#[test]
fn callback_pruning_retains_lease_revisions_across_retirement_and_registration() {
    let f = Fixture::new(crate::common::Seed::default());
    let worker = f.session("worker");
    let listener = Listener::new(f.store.database().database());
    let address = format!("uds:{}", listener.path.display());
    f.store
        .callback(&worker, Some(&address), false, false)
        .unwrap();
    let lease = || {
        f.store
            .raw()
            .row("callback_lease", worker.as_str())
            .get()
            .unwrap()
    };
    let before = lease();
    assert!(before["revision"].as_u64().unwrap() > 0);
    f.store.leave(&worker).unwrap();
    assert!(
        state(&f, &worker).is_null(),
        "leaving retires the callback state"
    );
    sweep_callbacks(&f, &listener);
    assert_eq!(
        lease(),
        before,
        "pruning must retain the revision even with no callback state"
    );
    f.store.revive(&worker).unwrap();
    f.store
        .callback(&worker, Some(&address), false, false)
        .unwrap();
    assert!(
        lease()["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap(),
        "registration must not reuse a revision that can identify a retained event"
    );
}

#[test]
fn chat_and_legacy_outbox_keys_keep_each_numeric_run_in_natural_order() {
    let f = Fixture::new(crate::common::Seed::default());
    let handle = f.session("orchestrator");
    let sender = f.session("worker");
    let expected = ["2", "10", "aa2b100", "aa10b1", "b2", "z"];
    for key in expected.iter().rev() {
        f.store
            .raw()
            .row("chat_event", *key)
            .put(json!({
                "key": key, "text": key, "delivered": false,
            }))
            .unwrap();
    }
    let words: Vec<_> = f
        .store
        .unheard_owner_events(&handle)
        .unwrap()
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    assert_eq!(words, expected);
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &handle,
            Some(&format!("uds:{}", listener.path.display())),
            true,
            false,
        )
        .unwrap();
    let delivered = || {
        listener
            .drain(&f, &sender, &handle)
            .into_iter()
            .filter(|line| expected.contains(&line.as_str()))
            .collect::<Vec<_>>()
    };
    assert_eq!(delivered(), expected);

    // Legacy queues can have one sequence for several events: key ordering breaks that tie.
    for key in expected.iter().rev() {
        f.store
            .raw()
            .row("callback_event", json!([handle, key]))
            .put(json!({
                "handle": handle, "key": key, "text": key, "sequence": 1,
                "attempts": 0, "delivered": false,
            }))
            .unwrap();
    }
    let keys: Vec<_> = f
        .store
        .callback_inbox(&handle)
        .unwrap()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(keys, expected);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(delivered(), expected);
}

#[test]
fn retirement_by_presence_and_a_held_close_clears_only_the_registered_channel() {
    use crate::common::{NoTerminals, Seed};
    use symphony_board::{NewLaunch, launch};
    for held in [false, true] {
        let f = Fixture::new(Seed::default());
        let worker = f.session("worker");
        let actor = f.session("orchestrator");
        let listener = Listener::new(f.store.database().database());
        let address = format!("uds:{}", listener.path.display());
        f.store
            .callback(&worker, Some(&address), false, false)
            .unwrap();
        if held {
            f.task("work/closed");
            f.store
                .take_role("work/closed", &actor, "orchestrator")
                .unwrap();
            f.store
                .record_launch(&NewLaunch {
                    pane: "old:pane".into(),
                    ..NewLaunch::new("work/closed", &worker, "executor")
                })
                .unwrap();
            let closure = launch::close(
                &f.store,
                &NoTerminals,
                &|_| panic!("a historical pane has no process"),
                &actor,
                "work/closed",
                &worker,
            )
            .unwrap();
            assert!(closure.closed);
            assert!(
                f.store.task_sessions("work/closed").unwrap()[0]
                    .closed
                    .is_some()
            );
        } else {
            // Older registrations without a listener identity cannot be used or retained.
            f.store
                .raw()
                .row("callback_state", worker.as_str())
                .set(json!({"owner":null}))
                .unwrap();
            let path = std::env::temp_dir().join(format!(
                "retire-{}-{}.json",
                std::process::id(),
                f.store.database().database()
            ));
            sweep_presence(&f.store, path);
        }
        assert!(
            !f.store
                .raw()
                .row("callback_state", worker.as_str())
                .exists()
                .unwrap()
        );
        assert!(!f.store.callbacks().unwrap().contains_key(&worker));
        assert!(
            f.store.session(&worker).unwrap().row.left_at.is_none(),
            "retiring the channel does not leave the session"
        );
    }
}
