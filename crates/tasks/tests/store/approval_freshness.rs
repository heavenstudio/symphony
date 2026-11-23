//! Approval observations through the existing fake-HC child, native listener and durable outbox.

use super::super::callbacks::Listener;
use super::*;
use symphony_board::cli::attach::PaneWatch;
use symphony_board::{NewSession, NewTask, Store};

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
    watch_store(&f.store, handle)
}

fn watch_store(store: &Store, handle: &str) -> PaneWatch {
    let watch = (store.raw().row("callback_state", handle)).field("pane_watch");
    serde_json::from_value(watch.unwrap()).unwrap()
}

fn pending(f: &Fixture, handle: &str) -> Vec<Value> {
    pending_store(&f.store, handle)
}

fn pending_store(store: &Store, handle: &str) -> Vec<Value> {
    let waiting = (store.raw().table("callback_event")).matching(json!({"handle": handle}));
    let fields = ["key", "text", "attempts", "sequence", "delivered"];
    let mut rows = waiting.fields(&fields).unwrap().as_array().unwrap().clone();
    rows.retain(|row| row["delivered"] != json!(true));
    rows.sort_by_key(|row| row["sequence"].as_i64());
    for row in &mut rows {
        row.as_object_mut().unwrap().remove("delivered");
    }
    rows
}

fn seed(f: &Fixture, handle: &str, key: &str, text: &str) {
    let counter = f
        .store
        .raw()
        .row("counter", json!(["callback-event", handle]));
    counter.add("last", 1).unwrap();
    let sequence = counter.field("last").unwrap();
    f.store
        .raw()
        .row("callback_event", json!([handle, key]))
        .put(json!({"key":key,"handle":handle,"text":text,"attempts":0,"sequence":sequence}))
        .unwrap();
}

fn restore(f: &Fixture, handle: &str) -> Listener {
    Listener::new(&format!("{}-{handle}", f.store.database().database()))
}

fn uncertain_samples(f: &Fixture, orch: &str, worker: &str, before: &[Value]) {
    hc_down_for_the_stretch(f);
    let queued = pending(f, orch);
    assert_eq!(queued.len(), 2, "one outage beside the known block");
    // The known block is the same row, its delivery tried once more over the outage's stretch.
    let mut initial = before.to_vec();
    initial[0]["attempts"] = json!(before[0]["attempts"].as_i64().unwrap() + 1);
    assert!(queued.contains(&initial[0]), "{queued:?}");
    let initial = initial.as_slice();
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
    let f = Fixture::new(crate::common::Seed::default());
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
    hc_down_for_the_stretch(&f);
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
    let saved = watch(&f, &orch);
    assert!(saved.known.is_empty(), "{saved:?}");
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
    let f = Fixture::new(crate::common::Seed::default());
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
    let queued = pending(&f, &orch);
    assert!(queued.is_empty(), "{queued:?}");
    let saved = watch(&f, &orch);
    assert!(saved.blocked.is_empty(), "{saved:?}");
    assert_eq!(saved.known[&worker].turn, Some(1));
    let legacy =
        format!("BLOCKED {worker} tests/pane t2 — its terminal waits on an approval or a question");
    seed(&f, &orch, "pane-900-0", &legacy);
    tick(&f);
    let queued = pending(&f, &orch);
    assert!(queued.is_empty(), "unchanged checkpoint: {queued:?}");
    for turn in [1, 0] {
        panes(Some(json!([terminal("t2", "blocked", Some(turn))])));
        tick(&f);
        let queued = pending(&f, &orch);
        assert!(queued.is_empty(), "resolved turn {turn}: {queued:?}");
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
    assert_eq!(latest[0]["text"], notice(&worker, "tests/pane", "t3"));
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
    let saved = watch(&f, &orch);
    assert!(saved.known.is_empty(), "{saved:?}");
    let heard = blocked(&back.drain(&f, &worker, &orch));
    assert!(heard.is_empty(), "closed launch: {heard:?}");
}

#[test]
fn legacy_nonclock_transport_preserves_unrelated_prose() {
    if !in_child_with_hc(
        "blocked_panes::freshness::legacy_nonclock_transport_preserves_unrelated_prose",
    ) {
        return;
    }
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, worker, o) = waiting_worker(&f);
    let pane_watch = json!({"blocked":{worker.clone():{"task":"tests/pane", "pane":"t2"}},
        "unanswered":false});
    (f.store.raw().row("callback_state", orch.as_str()))
        .set(json!({"pane_watch": pane_watch}))
        .unwrap();
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
    let f = Fixture::new(crate::common::Seed::default());
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
        let f = Fixture::new(crate::common::Seed::default());
        let (orch, worker, o) = waiting_worker(&f);
        drop(o);
        panes(Some(json!([terminal("t2", "blocked", Some(1))])));
        tick(&f);
        let initial = pending(&f, &orch);
        assert_eq!(initial.len(), 1);
        let log = PathBuf::from(std::env::var("FAKE_HC_LOG").unwrap());
        let before = std::fs::read_to_string(&log).unwrap().lines().count();
        panes(Some(json!([terminal("t2", old_status, Some(old_turn))])));
        // Share the one-shot interleaving barrier with the owned delayed clock closure.
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

#[test]
fn an_old_named_pane_cannot_rewind_a_newer_observed_launch() {
    if !in_child_with_hc(
        "blocked_panes::freshness::an_old_named_pane_cannot_rewind_a_newer_observed_launch",
    ) {
        return;
    }
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    for (status, turn) in [
        ("blocked", Some(0)),
        ("working", Some(0)),
        ("blocked", None),
        ("working", None),
    ] {
        let f = Fixture::new(crate::common::Seed::default());
        let (orch, worker, o) = waiting_worker(&f);
        drop(o);
        let named = |id, status, turn| {
            let mut value = terminal(id, status, turn);
            value["name"] = worker.clone().into();
            value
        };
        panes(Some(json!([named("t2", "blocked", Some(3))])));
        tick(&f);
        assert_eq!(watch(&f, &orch).known[&worker].turn, Some(3));
        assert_eq!(pending(&f, &orch).len(), 1);
        f.advance(1.0);
        let log = PathBuf::from(std::env::var("FAKE_HC_LOG").unwrap());
        let baseline = std::fs::read_to_string(&log).unwrap().lines().count();
        // Arm an advance so the delayed clock enters the write path before the new launch.
        panes(Some(json!([named("t2", "blocked", Some(4))])));
        // The delayed clock owns the closure; retain its barrier and committed evidence here.
        let ran = Arc::new(AtomicBool::new(false));
        let fired = Arc::clone(&ran);
        let committed = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&committed);
        let newer = f.other();
        let (new_worker, new_orch) = (worker.clone(), orch.clone());
        let slow = f.busy(move || {
            if std::fs::read_to_string(&log).unwrap().lines().count() > baseline
                && !fired.swap(true, Ordering::SeqCst)
            {
                newer
                    .record_launch(&NewLaunch {
                        pane: "t3".into(),
                        ..NewLaunch::new("tests/pane", &new_worker, "executor")
                    })
                    .unwrap();
                let mut current = terminal("t3", status, turn);
                current["name"] = new_worker.clone().into();
                panes(Some(json!([current])));
                newer.dispatch_callbacks_snapshot().unwrap();
                let saved = watch_store(&newer, &new_orch);
                assert_eq!(saved.known[&new_worker].pane, "t3");
                assert_eq!(saved.known[&new_worker].turn, turn);
                assert_eq!(saved.blocked.contains_key(&new_worker), status == "blocked");
                let queued = pending_store(&newer, &new_orch);
                assert_eq!(queued.len(), usize::from(status == "blocked"));
                if status == "blocked" {
                    assert_eq!(queued[0]["text"], notice(&new_worker, "tests/pane", "t3"));
                }
                *captured.lock().unwrap() = Some((saved, queued));
            }
        });
        slow.dispatch_callbacks_snapshot().unwrap();
        assert!(ran.load(Ordering::SeqCst), "newer clock must commit first");
        let (saved, queued) = committed.lock().unwrap().take().unwrap();
        assert_eq!(watch(&f, &orch), saved, "{status} {turn:?}");
        assert_eq!(
            pending(&f, &orch),
            queued,
            "queued rows must remain unchanged"
        );
        let back = restore(&f, &orch);
        f.advance(6.0);
        tick(&f);
        assert_eq!(
            blocked(&back.drain(&f, &worker, &orch)),
            vec![notice(&worker, "tests/pane", "t3"); usize::from(status == "blocked")]
        );
    }
}
