use super::common::{Fixture, sweep_presence};
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod in_process;

pub(super) struct Listener {
    pub(super) path: PathBuf,
    stop: mpsc::Sender<()>,
    messages: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    pub(super) fn new(database: &str) -> Self {
        let path =
            PathBuf::from("/tmp").join(format!("sf-idle-{}-{database}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (send, messages) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(true).unwrap();
                        let deadline = Instant::now() + Duration::from_secs(3);
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
        let deadline = Instant::now() + Duration::from_secs(3);
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
        let frame = self.messages.recv_timeout(Duration::from_secs(3)).unwrap();
        let value: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        value["message"]["content"].as_str().unwrap().to_string()
    }

    /// A real delivery barrier proves absence without a timing-based empty receive.
    pub(super) fn drain(&self, f: &Fixture, sender: &str, handle: &str) -> Vec<String> {
        f.store
            .send_agent(sender, handle, "delivery-barrier")
            .unwrap();
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
    f.store
        .client()
        .query(
            "RETURN SELECT * OMIT id FROM ONLY type::record('callback_state', $handle);",
            json!({"handle":handle}),
        )
        .unwrap()
        .remove(0)
}

#[test]
fn plain_orchestrators_receive_queued_chat_and_preserve_explicit_priority() {
    let f = Fixture::empty();
    let first = f.session("orchestrator");
    let second = f.session("orchestrator");
    assert_eq!(f.store.chat_recipient().unwrap(), None);
    f.store
        .owner_callback("before-registration", "queued owner ask")
        .unwrap();
    let listener = Listener::new(f.store.client().database());
    let address = format!("uds:{}", listener.path.display());
    f.store
        .callback(&first, Some(&address), false, false)
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), Some(first.clone()));
    listener.receive("queued owner ask");
    assert_eq!(
        f.store
            .client()
            .query(
                "RETURN type::record('chat_event', 'before-registration').delivered;",
                json!({})
            )
            .unwrap()[0],
        true
    );

    let other = Listener::new(&format!("{}-other", f.store.client().database()));
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
    assert_eq!(f.store.chat_recipient().unwrap(), Some(winner.clone()));
    let fallback = f.session("worker");
    let fallback_listener = Listener::new(&format!("{}-fallback", f.store.client().database()));
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
        .client()
        .query(
            "UPDATE type::record('callback_state', $handle) SET chat = false RETURN NONE;",
            json!({"handle":fallback}),
        )
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), Some(fallback.clone()));
    let seat = f.session("worker");
    let seat_listener = Listener::new(&format!("{}-seat", f.store.client().database()));
    f.store
        .callback(
            &seat,
            Some(&format!("uds:{}", seat_listener.path.display())),
            true,
            false,
        )
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), Some(seat.clone()));
    f.store.callback(&seat, None, false, false).unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), Some(fallback.clone()));
    f.store.callback(&fallback, None, false, false).unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), Some(winner));

    f.store.callback(&first, None, false, false).unwrap();
    f.store.leave(&second).unwrap();
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
        let f = Fixture::empty();
        let orchestrator = f.session("orchestrator");
        let listener = Listener::new(f.store.client().database());
        let address = format!("uds:{}", listener.path.display());
        f.store
            .callback(&orchestrator, Some(&address), chat, if_free)
            .unwrap();
        if chat && !seat {
            // An if-free registration can remain a fallback after another seat has displaced it.
            f.store
                .client()
                .query(
                    "UPDATE type::record('callback_state', $handle) SET chat = false RETURN NONE;",
                    json!({"handle":orchestrator}),
                )
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

        let listener = Listener::new(f.store.client().database());
        sweep_callbacks(&f, &listener);
        let recovered = state(&f, &orchestrator);
        assert_eq!(recovered["chat_resume"], false);
        assert_eq!(recovered["chat"], seat);
        assert_eq!(recovered["chat_fallback"], chat && if_free);
        f.store.dispatch_callbacks().unwrap();
        listener.receive("recover this owner ask");

        let fallback = f.session("worker");
        let other = Listener::new(&format!("{}-late", f.store.client().database()));
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
        let f = Arc::new(Fixture::empty());
        let handle = f.session("worker");
        let listener = Listener::new(f.store.client().database());
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
                .client()
                .query(
                    "RETURN type::record('callback_lease', $handle).until ?? 0;",
                    json!({"handle":target}),
                )
                .unwrap()
                .remove(0);
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
                .client()
                .query(
                    "RETURN type::record('callback_event', [$handle, 'chat-overlap']).attempts;",
                    json!({"handle":handle}),
                )
                .unwrap()[0],
            4,
            "both dispatchers must have recorded their real failed send"
        );
        let suspended = state(&f, &handle);
        assert_eq!(suspended["chat_resume_seat"], true);
        assert_eq!(suspended["chat_resume_fallback"], if_free);

        let listener = Listener::new(f.store.client().database());
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
    let f = Fixture::empty();
    let orchestrator = f.session("orchestrator");
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .client()
        .query(
            "UPDATE type::record('session', $handle) SET last_seen = $seen RETURN NONE;",
            json!({"handle":orchestrator, "seen":f.now()-symphony_board::health::LIVE_SECONDS-1.0}),
        )
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), None);
    f.store.client().query(
        "UPDATE type::record('callback_state', $handle) SET confirmed_until = $until RETURN NONE;",
        json!({"handle":orchestrator, "until":f.now()+60.0}),
    ).unwrap();
    assert_eq!(
        f.store.chat_recipient().unwrap(),
        Some(orchestrator.clone())
    );
    f.store
        .client()
        .query(
            "UPDATE type::record('callback_state', $handle) SET chat_resume = true RETURN NONE;",
            json!({"handle":orchestrator}),
        )
        .unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), None);
}

#[test]
fn legacy_writers_do_not_repeat_unknown_notices_across_the_lost_threshold() {
    use symphony_board::{Health, NewSession};

    let f = Fixture::empty();
    let orchestrator = f.session("orchestrator");
    let child = f.session_with(NewSession {
        parent: Some(orchestrator.clone()),
        ..NewSession::new("worker")
    });
    f.task("tools/mixed");
    f.store.claim("tools/mixed", &child).unwrap();
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    assert_eq!(state(&f, &orchestrator)["seen"]["children"][&child], "Live");
    f.store.client().query(
        "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:abcdef01', host = $host, confirmed_until = $until, lease = $lease RETURN NONE;",
        json!({"handle":child, "host":super::common::HOST, "until":f.now()-1.0, "lease":f.now()+1_000_000.0}),
    ).unwrap();
    let event_count = || {
        f.store
            .client()
            .query(
                "RETURN (SELECT VALUE last FROM ONLY type::record('counter', ['callback-event', $handle])) ?? 0;",
                json!({"handle":orchestrator}),
            )
            .unwrap()[0]
            .as_u64()
            .unwrap()
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
        f.store.client().query(
            "UPDATE type::record('session', $handle) SET connected = $seen, last_seen = $seen, busy_until = IF $busy { $until } ELSE { NONE }, pid = IF $busy { 999999999 } ELSE { NONE }, host = $host RETURN NONE;",
            json!({"handle":child, "seen":f.now()-age, "busy":busy, "until":f.now()+60.0, "host":super::common::HOST}),
        ).unwrap();
        assert_eq!(f.health(&child), health);
        let before = event_count();
        for round in 0..4 {
            // Main writes its ordinary Seen and preserves fields it does not know. This is the
            // persisted old-writer boundary, not a test-only mode in the new dispatcher.
            let mut legacy_seen = state(&f, &orchestrator)["seen"].clone();
            legacy_seen["children"][&child] = json!(legacy_health);
            f.store
                .client()
                .query(
                    "UPDATE type::record('callback_state', $handle) SET seen = $seen RETURN NONE;",
                    json!({"handle":orchestrator, "seen":legacy_seen}),
                )
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
    f.store.client().query(
        "UPDATE type::record('callback_state', $handle) SET confirmed_until = $until RETURN NONE;",
        json!({"handle":child, "until":f.now()-601.0}),
    ).unwrap();
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
    let f = Fixture::empty();
    assert!(f.store.dispatch_callbacks_snapshot().unwrap().is_none());
    let worker = f.session("worker");
    let sender = f.session("worker");
    f.task("tools/idle");
    f.store.claim("tools/idle", &worker).unwrap();
    f.store
        .take_role("tools/idle", &sender, "orchestrator")
        .unwrap();
    let listener = Listener::new(f.store.client().database());
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
        .client()
        .query(
            "UPDATE type::record('callback_lease', $handle) SET until = $until RETURN NONE;",
            json!({"handle":worker,"until":f.now()+60.0}),
        )
        .unwrap();
    assert!(
        !f.store
            .send_agent(&sender, &worker, "idle-agent-pending")
            .unwrap()
    );
    f.store
        .client()
        .query(
            "UPDATE type::record('callback_lease', $handle) SET until = 0 RETURN NONE;",
            json!({"handle":worker}),
        )
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
        f.store.client().query(
        "RETURN {states: (SELECT * FROM callback_state ORDER BY id), counters: (SELECT * FROM counter ORDER BY id), leases: (SELECT * FROM callback_lease ORDER BY id)};",
        json!({}),
    ).unwrap()
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
    let requests = f.store.client().requests();
    let began = Instant::now();
    f.store.dispatch_callbacks_snapshot().unwrap();
    let requests = f.store.client().requests() - requests;
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
    let f = Fixture::empty();
    let worker = f.session("worker");
    f.task("tools/idle");
    f.store.claim("tools/idle", &worker).unwrap();
    let listener = Listener::new(f.store.client().database());
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
        store
            .client()
            .query(
                "RETURN (SELECT VALUE last FROM ONLY type::record('counter', ['callback-event', $handle])) ?? 0;",
                json!({"handle":worker}),
            )
            .unwrap()
            .remove(0)
    };
    let before_events = events(&f.store);
    f.store
        .client()
        .query(
            "UPDATE type::record('task', 'tools/idle') SET status = 'review' RETURN NONE;",
            json!({}),
        )
        .unwrap();

    // The slow dispatcher has already read the transient board when its clock is read. A newer
    // dispatcher then observes the original state again, matching the saved seen/statuses.
    let other = f.other();
    let slow = f.slow(move || {
        other
            .client()
            .query(
                "UPDATE type::record('task', 'tools/idle') SET status = 'in_progress' RETURN NONE;",
                json!({}),
            )
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
    use super::common::{GONE, Seed};
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
    let requests = f.store.client().requests();
    f.advance(GONE);
    let board = f.store.clock_board().unwrap();
    assert_eq!(
        f.store.client().requests() - requests,
        1,
        "only the marker is read"
    );
    assert_eq!(board.sessions[0].health, symphony_board::Health::Lost);
    assert_eq!(board.tasks[0].roles[0].health, symphony_board::Health::Lost);
    f.store
        .client()
        .query(
            "UPDATE task SET title = 'Changed by another writer' RETURN NONE;",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        f.store.clock_board().unwrap().tasks[0].title,
        "Changed by another writer"
    );
    let marker = f
        .store
        .client()
        .query(
            &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
            json!({}),
        )
        .unwrap();
    assert!(f.store.client().query("BEGIN TRANSACTION; UPDATE task SET title = 'Uncommitted'; THROW 'rollback'; COMMIT TRANSACTION;", json!({})).is_err());
    assert_eq!(
        f.store
            .client()
            .query(
                &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
                json!({})
            )
            .unwrap(),
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
    use super::common::{Seed, session};
    let mut child = session("child");
    child.parent = Some("parent".into());
    let f = Fixture::new(
        Seed::default()
            .session("parent")
            .session_row(child)
            .task("tools/child", "in_progress")
            .role("tools/child", "child", "executor"),
    );
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            "parent",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    let requests = f.store.client().requests();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        f.store.client().requests() - requests,
        3,
        "the warm tick must reach the idle shortcut before the clock advances"
    );
    let marker = f
        .store
        .client()
        .query(
            &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
            json!({}),
        )
        .unwrap();
    f.advance(91.0);
    assert_eq!(
        f.store
            .client()
            .query(
                &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
                json!({})
            )
            .unwrap(),
        marker
    );
    f.store.dispatch_callbacks().unwrap();
    listener.receive("STALE child");
    f.advance(510.0);
    f.store.dispatch_callbacks().unwrap();
    listener.receive("LOST child");
}

#[test]
fn finished_history_is_not_retained_and_new_ready_work_still_arrives() {
    use super::common::{Seed, session};
    let mut orch = session("orchestrator");
    orch.kind = "orchestrator".into();
    // Keep health and the canonical count stable while the periodic check falls due.
    orch.busy_until = Some(super::common::NOW + 1200.0);
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
    let listener = Listener::new(f.store.client().database());
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
        super::quiet_callbacks::with_volume("orchestrator", 1, pull)
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
            .client()
            .query(
                "RETURN type::record('callback_lease', 'orchestrator').pull;",
                json!({})
            )
            .unwrap()[0],
        json!(f.now()),
        "the small scheduling checkpoint must still advance"
    );
    assert!(listener.drain(&f, "sender", "orchestrator").is_empty());
    f.store
        .client()
        .query(
            "UPDATE type::record('task', 'tools/history-0') SET status = 'ready' RETURN NONE;",
            json!({}),
        )
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
    let f = Arc::new(Fixture::empty());
    let receiver = f.session("worker");
    let sender = f.session("worker");
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            &receiver,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .client()
        .query(
            "UPDATE type::record('callback_lease', $handle) SET until = $until RETURN NONE;",
            json!({"handle":receiver,"until":f.now()+60.0}),
        )
        .unwrap();
    assert!(
        !f.store
            .send_agent(&sender, &receiver, "serialized-direct-message")
            .unwrap()
    );
    f.store
        .client()
        .query(
            "UPDATE type::record('callback_lease', $handle) SET until = 0 RETURN NONE;",
            json!({"handle":receiver}),
        )
        .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let checked = Arc::clone(&entered);
    let meanwhile = Arc::clone(&f);
    let other = f.other();
    let slow = f.busy(move || {
        let lease = || {
            meanwhile
                .store
                .client()
                .query(
                    "RETURN SELECT * FROM ONLY type::record('callback_lease', $handle);",
                    json!({"handle":receiver}),
                )
                .unwrap()
        };
        let before = lease();
        if before[0]["until"].as_f64().unwrap_or(0.0) > meanwhile.now()
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

#[test]
fn a_failed_chat_waits_without_writes_and_retries_at_its_deadline() {
    let f = Fixture::empty();
    let worker = f.session("worker");
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            &worker,
            Some(&format!("uds:{}", listener.path.display())),
            true,
            false,
        )
        .unwrap();
    drop(listener);
    f.store.client().query("DEFINE EVENT state_writes ON callback_state WHEN true THEN { UPSERT callback_audit:writes SET count = (count ?? 0) + 1; };", json!({})).unwrap();
    f.store
        .owner_callback("deadline", "retry this owner message")
        .unwrap();
    let state_writes = || {
        f.store
            .client()
            .query("RETURN callback_audit:writes.count;", json!({}))
            .unwrap()
    };
    let writes = state_writes();
    assert_eq!(
        writes,
        vec![json!(1)],
        "the first failure marks the callback blocked"
    );
    let rows = || {
        f.store.client().query("RETURN {states: (SELECT * FROM callback_state ORDER BY id), events: (SELECT * FROM callback_event ORDER BY id), counters: (SELECT * FROM counter ORDER BY id), leases: (SELECT * FROM callback_lease ORDER BY id)};", json!({})).unwrap()
    };
    let before = rows();
    assert_eq!(
        before[0]["events"][0]["attempts"], 1,
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
        rows()[0]["events"][0]["attempts"],
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
    use super::common::Seed;
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/contended", "in_progress")
            .role("tools/contended", "worker", "executor"),
    );
    let listener = Listener::new(f.store.client().database());
    f.store
        .callback(
            "worker",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    f.store
        .client()
        .query(
            "UPDATE callback_lease SET until = $until RETURN NONE;
        UPDATE task SET status = 'review' RETURN NONE;",
            json!({"until":f.now()+60.0}),
        )
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        state(&f, "worker")["seen"]["statuses"]["tools/contended"],
        "in_progress"
    );
    f.store
        .client()
        .query(
            "UPDATE callback_lease SET until = 0 RETURN NONE;",
            json!({}),
        )
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
    use super::common::{GONE, NOW, Seed, session};
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
    let listener = Listener::new(f.store.client().database());
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
        let before = store.client().requests();
        store.dispatch_callbacks().unwrap();
        store.client().requests() - before
    };
    let baseline = dispatch_requests(&f.store);
    let fresh_baseline = dispatch_requests(&f.other());
    let observation = state(&f, "orchestrator");
    let counters = || {
        f.store
            .client()
            .query("RETURN SELECT * FROM counter ORDER BY id;", json!({}))
            .unwrap()
    };
    let before_counters = counters();
    let leases = || {
        f.store
            .client()
            .query(
                "RETURN SELECT * FROM callback_lease ORDER BY id;",
                json!({}),
            )
            .unwrap()
    };

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
    assert_eq!(checked[0][0]["pull"], json!(f.now()));
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
    assert_eq!(leases()[0][0]["pull"], json!(f.now()));
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
    let f = Fixture::empty();
    let worker = f.session("worker");
    let listener = Listener::new(f.store.client().database());
    let address = format!("uds:{}", listener.path.display());
    f.store
        .callback(&worker, Some(&address), false, false)
        .unwrap();
    let lease = || {
        f.store
            .client()
            .query(
                "RETURN SELECT * FROM ONLY type::record('callback_lease', $handle);",
                json!({"handle":worker}),
            )
            .unwrap()
            .remove(0)
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
