//! Volume-only PULL conditions use the ordinary durable acknowledgement and retry account.

use super::callbacks::Listener;
use super::common::{Fixture, NOW, Seed, session};
use super::quiet_callbacks::listen;
use serde::Deserialize;
use serde_json::json;
use symphony_board::NewTask;

mod canonical;

#[test]
fn a_health_driven_count_change_within_the_low_band_does_not_repeat_the_warning() {
    let mut orch = session("orchestrator");
    orch.kind = "orchestrator".into();
    let mut worker = session("worker");
    worker.busy_until = Some(NOW + 1200.0);
    let f = Fixture::new(
        Seed::default()
            .session_row(orch)
            .session_row(worker)
            .task("tools/root", "in_progress")
            .role("tools/root", "orchestrator", "orchestrator")
            .role("tools/root", "worker", "executor"),
    );
    let count = || {
        f.store
            .slot_holdings()
            .unwrap()
            .get("orchestrator")
            .map_or(0, |held| held.len())
    };
    assert_eq!(f.health("orchestrator"), symphony_board::Health::Live);
    assert_eq!(count(), 1);
    let listener = listen(&f, "orchestrator");
    let busy = "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";
    assert_eq!(
        listener.text(),
        crate::quiet_callbacks::with_volume("orchestrator", 1, busy)
    );

    // Its callback stays registered, but this non-beating owner no longer holds a slot.
    f.advance(601.0);
    assert_eq!(f.health("orchestrator"), symphony_board::Health::Left);
    assert_eq!(f.health("worker"), symphony_board::Health::Live);
    assert_eq!(count(), 0);
    let board = f.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&f.store, &[], None, f.now(), &[])
        .unwrap()
        .only_for(&board, "orchestrator");
    assert_eq!(
        digest.event_lines(),
        [
            busy.to_owned(),
            "  low_volume orchestrator open=0 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership".to_owned(),
        ]
    );
    f.store.dispatch_callbacks().unwrap();
    assert!(listener.drain(&f, "worker", "orchestrator").is_empty());
    f.other().dispatch_callbacks().unwrap();
    assert!(listener.drain(&f, "worker", "orchestrator").is_empty());
}

fn start(f: &Fixture, orch: &str, worker: &str, slug: &str) {
    let store = f.elsewhere();
    store.create(&NewTask::new(slug, "Work", "owner")).unwrap();
    store.take_role(slug, orch, "orchestrator").unwrap();
    store.claim(slug, worker).unwrap();
}

fn warnings(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .filter(|line| line.starts_with("low_volume "))
        .map(String::as_str)
        .collect()
}

#[derive(Debug, Deserialize)]
struct Waiting {
    key: String,
    text: String,
    attempts: u32,
}

fn waiting(f: &Fixture, orch: &str) -> Vec<Waiting> {
    let rows = (f.store.raw().table("callback_event"))
        .matching(json!({"handle": orch}))
        .fields(&["key", "text", "attempts", "delivered"])
        .unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_value(rows).unwrap();
    let waits = |row: &serde_json::Value| {
        let pull = row["key"]
            .as_str()
            .is_some_and(|key| key.starts_with("pull-"));
        row["delivered"] != json!(true) && pull
    };
    serde_json::from_value(rows.into_iter().filter(waits).collect()).unwrap()
}

#[test]
fn an_ancestry_count_change_invalidates_pull_without_status_staffing_or_ready_changes() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    for n in 0..3 {
        start(&f, &orch, &worker, &format!("sim/task-{n}"));
    }
    let store = f.elsewhere();
    store
        .create(&NewTask {
            status: "backlog".into(),
            ..NewTask::new("sim/later", "Later", "owner")
        })
        .unwrap();
    store.take_role("sim/later", &orch, "orchestrator").unwrap();
    store
        .create(&NewTask {
            instruction: true,
            parent: Some("sim/later".into()),
            ..NewTask::new("sim/slice", "Slice", "owner")
        })
        .unwrap();
    store.take_role("sim/slice", &orch, "orchestrator").unwrap();
    store.claim("sim/slice", &worker).unwrap();
    let listener = listen(&f, &orch);
    assert!(!listener.text().contains("low_volume "));
    let before = symphony_board::cli::attach::observe(&f.store, &orch, "orchestrator").unwrap();
    for parent in ["sim/task-0", "sim/later", "sim/task-0"] {
        store.set_parent("sim/slice", Some(parent), &orch).unwrap();
        let after = symphony_board::cli::attach::observe(&f.store, &orch, "orchestrator").unwrap();
        assert_eq!(
            (
                &before.roles,
                &before.statuses,
                &before.ready,
                &before.children
            ),
            (&after.roles, &after.statuses, &after.ready, &after.children)
        );
        let below = parent == "sim/task-0";
        assert_eq!(before.staffing == after.staffing, !below);
        f.other().dispatch_callbacks().unwrap();
        let told = listener.drain(&f, &worker, &orch);
        assert_eq!(warnings(&told).len(), usize::from(below), "{told:?}");
        if below {
            assert!(warnings(&told)[0].contains("open=3 threshold=4"));
        }
    }
}

#[test]
fn low_volume_crosses_clears_and_reenters_without_ready_gaps_or_periodic_repeats() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    for n in 0..4 {
        start(&f, &orch, &sender, &format!("sim/task-{n}"));
    }
    let listener = listen(&f, &orch);
    assert!(!listener.text().contains("low_volume "));
    f.elsewhere()
        .move_to("sim/task-3", "cancelled", &sender)
        .unwrap();
    f.other().dispatch_callbacks().unwrap();
    let first = listener.drain(&f, &sender, &orch);
    let warning = format!(
        "low_volume {orch} open=3 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
    );
    assert_eq!(warnings(&first), [warning.as_str()]);
    f.elsewhere()
        .move_to("sim/task-2", "cancelled", &sender)
        .unwrap();
    f.other().dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &sender, &orch)).is_empty());
    for n in 4..6 {
        start(&f, &orch, &sender, &format!("sim/task-{n}"));
    }
    f.other().dispatch_callbacks().unwrap();
    assert!(warnings(&listener.drain(&f, &sender, &orch)).is_empty());
    f.elsewhere()
        .move_to("sim/task-5", "cancelled", &sender)
        .unwrap();
    f.other().dispatch_callbacks().unwrap();
    assert_eq!(
        warnings(&listener.drain(&f, &sender, &orch)),
        [warning.as_str()]
    );
    for age in [0.0, 601.0, 86_400.0] {
        f.advance(age);
        for handle in [&orch, &sender] {
            f.elsewhere().touch(handle).unwrap();
        }
        f.other().dispatch_callbacks().unwrap();
        assert!(warnings(&listener.drain(&f, &sender, &orch)).is_empty());
    }
}

#[test]
fn a_waiting_volume_warning_keeps_its_key_and_attempts_as_the_count_changes() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    for n in 0..4 {
        start(&f, &orch, &sender, &format!("sim/task-{n}"));
    }
    {
        let gone = listen(&f, &orch);
        assert!(!gone.text().contains("low_volume "));
    }
    f.elsewhere()
        .move_to("sim/task-3", "cancelled", &sender)
        .unwrap();
    f.store.dispatch_callbacks_snapshot().unwrap();
    let before = waiting(&f, &orch);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].attempts, 1);
    assert!(before[0].text.contains("open=3 threshold=4 cap=none"));
    f.elsewhere()
        .move_to("sim/task-2", "cancelled", &sender)
        .unwrap();
    f.store.dispatch_callbacks_snapshot().unwrap();
    let after = waiting(&f, &orch);
    assert_eq!(after.len(), 1);
    assert_eq!(
        (&after[0].key, after[0].attempts),
        (&before[0].key, before[0].attempts)
    );
    assert!(after[0].text.contains("open=2 threshold=4 cap=none"));
    let back = Listener::new(&format!("{}-volume-back", f.store.database().database()));
    f.store
        .callback(
            &orch,
            Some(&format!("uds:{}", back.path.display())),
            false,
            false,
        )
        .unwrap();
    let told = back.drain(&f, &sender, &orch);
    assert_eq!(warnings(&told).len(), 1, "{told:?}");
    assert!(warnings(&told)[0].contains("open=2 threshold=4 cap=none"));
    assert!(waiting(&f, &orch).is_empty());
}
