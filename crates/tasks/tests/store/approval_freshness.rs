//! Approval observations through the existing fake-HC child, native listener and durable outbox.
use super::super::callbacks::Listener;
use super::*;
use symphony_board::cli::attach::PaneWatch;
use symphony_board::{NewSession, NewTask};

fn terminal(id: &str, status: &str, turn: Option<u64>) -> Value {
    let mut value = pane(id, status);
    if let Some(turn) = turn {
        value["turn"] = turn.into();
    }
    value
}

fn notice(who: &str, task: &str, pane: &str) -> String {
    format!(
        "BLOCKED {who} {task} {pane} — observed waiting on an approval or a question; inspect its terminal before acting"
    )
}

fn blocked(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.starts_with("BLOCKED "))
        .cloned()
        .collect()
}

fn watch(f: &Fixture, handle: &str) -> PaneWatch {
    let mut values = f
        .store
        .client()
        .query(
            "RETURN (SELECT VALUE pane_watch FROM ONLY type::record('callback_state', $handle));",
            json!({"handle":handle}),
        )
        .unwrap();
    serde_json::from_value(values.remove(0)).unwrap()
}

fn pending(f: &Fixture, handle: &str) -> Vec<Value> {
    f.store.client().query(
        "RETURN SELECT key, text, attempts, sequence FROM callback_event WHERE handle = $handle AND delivered != true ORDER BY sequence;",
        json!({"handle":handle})).unwrap()[0].as_array().unwrap().clone()
}

fn seed(f: &Fixture, handle: &str, key: &str, text: &str) {
    f.store.client().query(
        "LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
         CREATE type::record('callback_event', [$handle, $key]) CONTENT {
             key:$key, handle:$handle, text:$text, slug:NONE, message:NONE, attempts:0, sequence:$counted.last
         } RETURN NONE;",
        json!({"handle":handle, "key":key, "text":text})).unwrap();
}

fn restore(f: &Fixture, handle: &str) -> Listener {
    Listener::new(&format!("{}-{handle}", f.store.client().database()))
}

fn uncertain_samples(f: &Fixture, orch: &str, worker: &str, initial: &[Value]) {
    panes(None);
    tick(f);
    assert_eq!(
        pending(f, orch).len(),
        2,
        "one outage beside the known block"
    );
    let mut named = terminal("t5", "working", Some(1));
    named["name"] = worker.into();
    let mut duplicate = named.clone();
    duplicate["address"] = "t6".into();
    for (why, sample) in [
        (
            "unknown counter, recovered inventory",
            json!([terminal("t2", "working", None)]),
        ),
        (
            "duplicate identity",
            json!([
                terminal("t2", "working", Some(1)),
                terminal("t2", "blocked", Some(1))
            ]),
        ),
        ("unmatched identity", json!([])),
        ("unknown status", json!([terminal("t2", "", Some(1))])),
        ("duplicate handle fallback", json!([named, duplicate])),
    ] {
        panes(Some(sample));
        tick(f);
        assert_eq!(
            pending(f, orch),
            initial,
            "{why} cannot resolve a known block"
        );
        assert_eq!(watch(f, orch).known[worker].turn, Some(1));
    }
}

#[test]
fn pane_task_and_supported_counter_advances_reach_the_native_recipient() {
    if !in_child_with_hc(
        "blocked_panes::freshness::pane_task_and_supported_counter_advances_reach_the_native_recipient",
    ) {
        return;
    }
    let f = Fixture::empty();
    let (orch, worker, o) = waiting_worker(&f);
    for (turn, count) in [(1, 1), (1, 0), (2, 1)] {
        panes(Some(json!([terminal("t2", "blocked", Some(turn))])));
        tick(&f);
        assert_eq!(
            blocked(&o.drain(&f, &worker, &orch)),
            vec![notice(&worker, "tests/pane", "t2"); count]
        );
    }
    f.advance(1.0);
    launch(&f, "tests/pane", &worker, "t3");
    panes(Some(json!([terminal("t3", "blocked", Some(0))])));
    tick(&f);
    assert_eq!(
        blocked(&o.drain(&f, &worker, &orch)),
        [notice(&worker, "tests/pane", "t3")]
    );
    f.task("tests/second");
    f.store
        .take_role("tests/second", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/second", &worker).unwrap();
    f.advance(1.0);
    launch(&f, "tests/second", &worker, "t3");
    seed(
        &f,
        &orch,
        "pane-910-0",
        &notice(&worker, "tests/pane", "t3"),
    );
    panes(None);
    tick(&f);
    let outage = o.drain(&f, &worker, &orch);
    assert!(
        blocked(&outage).is_empty(),
        "a changed launch retires its old task notice during an outage"
    );
    assert!(
        outage
            .iter()
            .any(|line| line.starts_with("WATCH hc unanswered at observation:"))
    );
    assert!(watch(&f, &orch).known.is_empty());
    panes(Some(json!([terminal("t3", "blocked", Some(0))])));
    tick(&f);
    assert_eq!(
        blocked(&o.drain(&f, &worker, &orch)),
        [notice(&worker, "tests/second", "t3")]
    );
    let saved = watch(&f, &orch);
    assert_eq!(saved.known.len(), 1);
    assert_eq!(saved.known[&worker].task, "tests/second");
    assert_eq!(saved.known[&worker].turn, Some(0));
}

#[test]
fn pending_rows_keep_uncertainty_and_retire_only_proven_obsolete_observations() {
    if !in_child_with_hc(
        "blocked_panes::freshness::pending_rows_keep_uncertainty_and_retire_only_proven_obsolete_observations",
    ) {
        return;
    }
    let f = Fixture::empty();
    let (orch, worker, o) = waiting_worker(&f);
    drop(o);
    panes(Some(json!([terminal("t2", "blocked", Some(1))])));
    tick(&f);
    let initial = pending(&f, &orch);
    assert_eq!(initial.len(), 1, "{initial:?}");
    assert_eq!(initial[0]["attempts"], 1);
    uncertain_samples(&f, &orch, &worker, &initial);
    panes(Some(json!([terminal("t2", "working", Some(1))])));
    tick(&f);
    assert!(pending(&f, &orch).is_empty());
    assert!(watch(&f, &orch).blocked.is_empty());
    assert_eq!(watch(&f, &orch).known[&worker].turn, Some(1));
    let legacy =
        format!("BLOCKED {worker} tests/pane t2 — its terminal waits on an approval or a question");
    seed(&f, &orch, "pane-900-0", &legacy);
    tick(&f);
    assert!(
        pending(&f, &orch).is_empty(),
        "unchanged checkpoints still reconcile legacy rows"
    );
    for turn in [1, 0] {
        panes(Some(json!([terminal("t2", "blocked", Some(turn))])));
        tick(&f);
        assert!(
            pending(&f, &orch).is_empty(),
            "resolved equal/older turns cannot reannounce"
        );
    }
    panes(Some(json!([terminal("t2", "blocked", Some(2))])));
    tick(&f);
    assert_eq!(pending(&f, &orch).len(), 1);
    f.advance(1.0);
    launch(&f, "tests/pane", &worker, "t3");
    panes(Some(json!([terminal("t3", "blocked", Some(0))])));
    tick(&f);
    let latest = pending(&f, &orch);
    assert_eq!(
        latest.len(),
        1,
        "the new pane supersedes the queued old episode"
    );
    assert!(
        latest[0]["text"]
            .as_str()
            .unwrap()
            .contains(" tests/pane t3 — ")
    );
    let back = restore(&f, &orch);
    f.advance(6.0);
    tick(&f);
    assert_eq!(
        blocked(&back.drain(&f, &worker, &orch)),
        [notice(&worker, "tests/pane", "t3")]
    );
    f.store
        .close_task_session("tests/pane", &worker, "fixture closed", "restore fixture")
        .unwrap();
    seed(
        &f,
        &orch,
        "pane-901-0",
        &notice(&worker, "tests/pane", "t3"),
    );
    tick(&f);
    assert!(watch(&f, &orch).known.is_empty());
    assert!(
        blocked(&back.drain(&f, &worker, &orch)).is_empty(),
        "closed launches withdraw old pane notices"
    );
}

#[test]
fn legacy_nonclock_transport_preserves_unrelated_prose() {
    if !in_child_with_hc(
        "blocked_panes::freshness::legacy_nonclock_transport_preserves_unrelated_prose",
    ) {
        return;
    }
    let f = Fixture::empty();
    let (orch, worker, o) = waiting_worker(&f);
    f.store.client().query("UPDATE type::record('callback_state', $handle) SET pane_watch = $watch RETURN NONE;",
        json!({"handle":orch, "watch":{"blocked":{worker.clone():{"task":"tests/pane", "pane":"t2"}}, "unanswered":false}})).unwrap();
    let legacy =
        format!("BLOCKED {worker} tests/pane t2 — its terminal waits on an approval or a question");
    seed(&f, &orch, "pane-899-0", &legacy);
    panes(Some(json!([terminal("t2", "blocked", Some(1))])));
    tick(&f);
    assert_eq!(
        blocked(&o.drain(&f, &worker, &orch)),
        [notice(&worker, "tests/pane", "t2")]
    );
    assert_eq!(watch(&f, &orch).known[&worker].turn, Some(1));
    let arbitrary = format!("BLOCKED {worker} tests/pane t2 — user supplied prose");
    seed(&f, &orch, "pane-900-0", &legacy);
    seed(&f, &orch, "event-900", &legacy);
    seed(&f, &orch, "pane-900-1", &arbitrary);
    seed(
        &f,
        &orch,
        "pane-900-2",
        "WATCH hc unanswered: down — blocked terminals go unreported until it answers",
    );
    panes(Some(json!([terminal("t2", "working", None)])));
    // The barrier sends directly through the outbox, with no clock watch or new HC sample.
    let lines = o.drain(&f, &worker, &orch);
    assert_eq!(lines, [notice(&worker, "tests/pane", "t2"), legacy.clone(), arbitrary.clone(),
        "WATCH hc unanswered at observation: down — blocked terminals may have gone unreported; inspect hc before acting".into()]);
    f.store.send_agent(&worker, &orch, &legacy).unwrap();
    assert_eq!(
        o.text(),
        format!("AGENT from {worker} to {orch} — {legacy}")
    );
    seed(&f, &orch, "pane-user", &legacy);
    seed(&f, &orch, "pane-901-1", &arbitrary);
    seed(&f, &orch, "event-901", &legacy);
    tick(&f);
    assert_eq!(
        o.drain(&f, &worker, &orch),
        [legacy.clone(), arbitrary, legacy],
        "clock reconciliation must preserve unrecognized pane-like rows and general event prose"
    );
}

#[test]
fn a_direct_coordinator_reaches_its_parent_owner_without_exposing_delegated_workers() {
    if !in_child_with_hc(
        "blocked_panes::freshness::a_direct_coordinator_reaches_its_parent_owner_without_exposing_delegated_workers",
    ) {
        return;
    }
    let f = Fixture::empty();
    let root = f.session("orchestrator");
    let foreign = f.session("orchestrator");
    let mut child = NewSession::new("orchestrator");
    child.parent = Some(root.clone());
    let coordinator = f.session_with(child);
    let mut child = NewSession::new("worker");
    child.parent = Some(coordinator.clone());
    let worker = f.session_with(child);
    let unrelated = f.session("worker");
    f.task("tests/root");
    for (slug, parent) in [
        ("tests/delegated", Some("tests/root")),
        ("tests/delegated/work", Some("tests/delegated")),
        ("tests/foreign", None),
    ] {
        let mut task = NewTask::new(slug, slug, "owner");
        task.parent = parent.map(str::to_string);
        f.task_with(task);
    }
    for (slug, owner, executor, id) in [
        ("tests/delegated", &coordinator, &coordinator, "t4"),
        ("tests/delegated/work", &coordinator, &worker, "t5"),
        ("tests/foreign", &foreign, &unrelated, "t9"),
    ] {
        f.store.take_role(slug, owner, "orchestrator").unwrap();
        f.store.claim(slug, executor).unwrap();
        launch(&f, slug, executor, id);
    }
    f.store
        .take_role("tests/root", &root, "orchestrator")
        .unwrap();
    panes(Some(json!([
        terminal("t4", "blocked", Some(1)),
        terminal("t5", "blocked", Some(1)),
        terminal("t9", "blocked", Some(1))
    ])));
    let (a, b, c) = (
        listen(&f, &root),
        listen(&f, &coordinator),
        listen(&f, &foreign),
    );
    for (listener, handle) in [(&a, &root), (&b, &coordinator), (&c, &foreign)] {
        listener.drain(&f, &worker, handle);
    }
    tick(&f);
    assert_eq!(
        blocked(&a.drain(&f, &worker, &root)),
        [notice(&coordinator, "tests/delegated", "t4")]
    );
    assert_eq!(
        blocked(&b.drain(&f, &worker, &coordinator)),
        [notice(&worker, "tests/delegated/work", "t5")]
    );
    assert_eq!(
        blocked(&c.drain(&f, &worker, &foreign)),
        [notice(&unrelated, "tests/foreign", "t9")]
    );
}

#[test]
fn delayed_clock_samples_cannot_rewind_newer_blocked_or_resolved_evidence() {
    if !in_child_with_hc(
        "blocked_panes::freshness::delayed_clock_samples_cannot_rewind_newer_blocked_or_resolved_evidence",
    ) {
        return;
    }
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for (old_status, old_turn, new_status, new_turn) in [
        ("working", 1, "blocked", 2),
        ("blocked", 2, "working", 3),
        ("blocked", 2, "working", 2),
    ] {
        let f = Fixture::empty();
        let (orch, worker, o) = waiting_worker(&f);
        drop(o);
        panes(Some(json!([terminal("t2", "blocked", Some(1))])));
        tick(&f);
        let initial = pending(&f, &orch);
        assert_eq!(initial.len(), 1);
        let log = PathBuf::from(std::env::var("FAKE_HC_LOG").unwrap());
        let before = std::fs::read_to_string(&log).unwrap().lines().count();
        panes(Some(json!([terminal("t2", old_status, Some(old_turn))])));
        let ran = Arc::new(AtomicBool::new(false));
        let fired = Arc::clone(&ran);
        let newer = f.other();
        let slow = f.busy(move || {
            if std::fs::read_to_string(&log).unwrap().lines().count() > before
                && !fired.swap(true, Ordering::SeqCst)
            {
                panes(Some(json!([terminal("t2", new_status, Some(new_turn))])));
                newer.dispatch_callbacks_snapshot().unwrap();
            }
        });
        slow.dispatch_callbacks_snapshot().unwrap();
        assert!(
            ran.load(Ordering::SeqCst),
            "newer clock must commit after the old HC sample"
        );
        let saved = watch(&f, &orch);
        assert_eq!(saved.known[&worker].turn, Some(new_turn));
        assert_eq!(saved.blocked.contains_key(&worker), new_status == "blocked");
        if let Some(blocked) = saved.blocked.get(&worker) {
            assert_eq!(blocked.turn, Some(new_turn));
        }
        let queued = pending(&f, &orch);
        assert_eq!(
            queued.len(),
            usize::from(new_status == "blocked"),
            "{queued:?}"
        );
        if !queued.is_empty() {
            assert_ne!(queued[0]["key"], initial[0]["key"]);
        }
        let back = restore(&f, &orch);
        f.advance(6.0);
        tick(&f);
        assert_eq!(
            blocked(&back.drain(&f, &worker, &orch)),
            vec![notice(&worker, "tests/pane", "t2"); usize::from(new_status == "blocked")]
        );
    }
}
