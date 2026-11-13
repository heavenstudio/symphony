//! The advisory condition consumes the public ledger; it does not count task rows itself.

use super::*;

fn warning(handle: &str, count: usize, cap: &str) -> String {
    format!(
        "  low_volume {handle} open={count} threshold=4 cap={cap} — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
    )
}

#[test]
fn low_volume_uses_the_recipients_canonical_count_and_existing_cap() {
    for count in [0, 1, 3, 4, 8] {
        let c = Case::new();
        let orch = c.f.session("orchestrator");
        let other = c.f.session("orchestrator");
        for n in 0..count {
            let slug = format!("sim/mine-{n}");
            c.task(&slug, "in_progress");
            c.f.store.take_role(&slug, &orch, "orchestrator").unwrap();
        }
        c.task("sim/theirs", "review");
        c.f.store
            .take_role("sim/theirs", &other, "orchestrator")
            .unwrap();
        let board = c.f.store.board().unwrap();
        for cap in [None, Some(8)] {
            let store = c.f.other().capping(cap);
            let scoped = digest(&store, &[], None, NOW, &[])
                .unwrap()
                .only_for(&board, &orch);
            let key = ("low_volume".into(), orch.clone());
            assert_eq!(scoped.entries().contains(&key), count < 4);
            let warnings: Vec<_> = scoped
                .lines()
                .into_iter()
                .filter(|l| l.starts_with("  low_volume "))
                .collect();
            let expected = if count < 4 {
                vec![warning(
                    &orch,
                    count,
                    if cap.is_some() { "8" } else { "none" },
                )]
            } else {
                vec![]
            };
            assert_eq!(warnings, expected);
            assert!(
                !scoped
                    .entries()
                    .contains(&("low_volume".into(), other.clone()))
            );
            let acknowledged = BTreeSet::from([key]);
            assert!(
                !scoped
                    .event_lines_since(&acknowledged)
                    .iter()
                    .any(|l| l.starts_with("  low_volume "))
            );
        }
    }
}

#[test]
fn low_volume_holds_to_the_threshold_the_store_is_held_to_and_zero_says_it_to_none() {
    let c = Case::new();
    let orch = c.f.session("orchestrator");
    for n in 0..2 {
        let slug = format!("sim/mine-{n}");
        c.task(&slug, "in_progress");
        c.f.store.take_role(&slug, &orch, "orchestrator").unwrap();
    }
    let board = c.f.store.board().unwrap();
    for (low, heard) in [(3, true), (2, false), (0, false)] {
        let store = c.f.other().limiting(symphony_board::limits::Limits {
            low_volume: low,
            ..Default::default()
        });
        let warnings: Vec<_> = digest(&store, &[], None, NOW, &[])
            .unwrap()
            .only_for(&board, &orch)
            .lines()
            .into_iter()
            .filter(|l| l.starts_with("  low_volume "))
            .collect();
        let expected = if heard {
            vec![format!(
                "  low_volume {orch} open=2 threshold={low} cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
            )]
        } else {
            vec![]
        };
        assert_eq!(warnings, expected, "low_volume = {low}");
    }
}

#[test]
fn only_orchestrator_recipients_receive_the_condition() {
    let c = Case::new();
    for kind in ["worker", "reviewer", "orchestrator"] {
        let who = c.f.session(kind);
        let retained = c.f.store.board().unwrap();
        let low = ("low_volume".into(), who.clone());
        assert_eq!(
            c.digest()
                .only_for(&retained, &who)
                .entries()
                .contains(&low),
            kind == "orchestrator"
        );
        c.f.store.leave(&who).unwrap();
        assert!(
            !c.digest()
                .only_for(&retained, &who)
                .entries()
                .contains(&low)
        );
    }
}

#[test]
fn finished_tasks_and_review_roles_do_not_inflate_the_count() {
    let c = Case::new();
    let orch = c.f.session("orchestrator");
    let reviewer = c.f.session("reviewer");
    for (slug, end) in [("sim/done", "done"), ("sim/cancelled", "cancelled")] {
        c.task(slug, "in_progress");
        c.f.store.take_role(slug, &orch, "orchestrator").unwrap();
        if end == "done" {
            c.f.store.move_to(slug, "review", &orch).unwrap();
            c.f.store.take_role(slug, &reviewer, "reviewer").unwrap();
            c.f.store
                .verdict(slug, &reviewer, "approve", "Complete")
                .unwrap();
        } else {
            c.f.store.move_to(slug, end, &orch).unwrap();
        }
    }
    c.task("sim/reviewing", "review");
    c.f.store
        .take_role("sim/reviewing", &orch, "reviewer")
        .unwrap();
    let board = c.f.store.board().unwrap();
    assert!(
        c.digest()
            .only_for(&board, &orch)
            .lines()
            .contains(&warning(&orch, 0, "none"))
    );
}

#[test]
fn unknown_native_presence_keeps_owned_open_slots() {
    let c = Case::new();
    let orch = c.f.session("orchestrator");
    for n in 0..4 {
        let slug = format!("sim/held-{n}");
        c.task(&slug, "in_progress");
        c.f.store.take_role(&slug, &orch, "orchestrator").unwrap();
    }
    // The existing native metadata contract produces Unknown through Store::board itself.
    c.f.store.client().query(
        "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:abcdef01', host = $host, confirmed_until = $until, lease = $lease RETURN NONE;
         UPDATE type::record('session', $handle) SET last_seen = $seen RETURN NONE;",
        serde_json::json!({"handle":orch, "host":crate::common::HOST, "until":NOW-1.0, "lease":NOW+1_000_000.0, "seen":NOW-500.0}),
    ).unwrap();
    let board = c.f.store.board().unwrap();
    assert_eq!(c.f.health(&orch), symphony_board::Health::Unknown);
    let capped = c.f.other().capping(Some(8));
    let scoped = digest(&capped, &[], None, NOW, &[])
        .unwrap()
        .only_for(&board, &orch);
    assert_eq!(scoped.slots, Some(4));
    assert!(!scoped.entries().contains(&("low_volume".into(), orch)));
}

#[test]
fn retained_ownership_snapshots_do_not_replace_the_captured_volume_in_either_direction() {
    for (old_count, fresh_count) in [(3, 4), (4, 3)] {
        let c = Case::new();
        let orch = c.f.session("orchestrator");
        let peer = c.f.session("orchestrator");
        for n in 0..old_count {
            let slug = format!("sim/owned-{n}");
            c.task(&slug, "in_progress");
            c.f.store.take_role(&slug, &orch, "orchestrator").unwrap();
        }
        let retained = c.f.store.board().unwrap();
        if fresh_count > old_count {
            c.task("sim/owned-3", "in_progress");
            c.f.store
                .take_role("sim/owned-3", &orch, "orchestrator")
                .unwrap();
        } else {
            c.f.store
                .move_to("sim/owned-3", "cancelled", &orch)
                .unwrap();
        }
        let capped = c.f.other().capping(Some(8));
        let observed = digest(&capped, &[], None, NOW, &[]).unwrap();
        let global = observed.entries();
        let heard = observed.only_for(&retained, &orch);
        let key = ("low_volume".into(), orch.clone());
        assert_eq!(global.contains(&key), fresh_count < 4);
        assert_eq!(heard.entries().contains(&key), fresh_count < 4);
        assert!(heard.entries().is_subset(&global));
        assert!(global.contains(&("low_volume".into(), peer.clone())));
        assert!(!heard.entries().contains(&("low_volume".into(), peer)));
        assert_eq!(heard.slots, Some(fresh_count));
        assert!(heard.lines()[0].ends_with(&format!("slots={fresh_count}/8")));
        let warnings: Vec<_> = heard
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("  low_volume "))
            .collect();
        assert_eq!(
            warnings,
            if fresh_count < 4 {
                vec![warning(&orch, fresh_count, "8")]
            } else {
                vec![]
            }
        );
        assert!(
            !heard
                .event_lines_since(&heard.entries())
                .iter()
                .any(|line| line.starts_with("  low_volume "))
        );
    }
}
