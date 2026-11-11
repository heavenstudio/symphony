//! Incomplete absence stays unknown through the Store, view, monitor and PULL consumers.

use super::*;
use symphony_board::cli::attach::{Watch, watch};

#[test]
fn partial_process_evidence_never_emits_lost_but_complete_absence_does() {
    for (name, complete, owned, expected) in [
        ("partial-absence", false, false, Health::Unknown),
        ("partial-presence", false, true, Health::Live),
        ("complete-absence", true, false, Health::Lost),
    ] {
        let c = Case::new();
        let tree = linked_worktree(name);
        let handle = c.subagent_in(&tree);
        // Arm PULL in every case, including those with no orphaned work to announce.
        c.f.task("sim/unrelated");
        let listed = if owned {
            vec![process(&tree, NOW)]
        } else {
            vec![]
        };
        *c.processes.borrow_mut() = Some(ProcessScan { listed, complete });
        let lines = RefCell::new(Vec::new());
        let probes = c.probes();
        watch(
            &c.f.store,
            &c.orch,
            &Watch {
                interval: LIVE_SECONDS + STALE_SECONDS + 600.0,
                beat: 20.0,
                polls: Some(1),
                probes: Some(&probes),
                landed: None,
                tips: None,
                pull_every: Some(1.0),
            },
            &mut |line| lines.borrow_mut().push(line.to_string()),
            &mut |seconds| c.f.advance(seconds),
        )
        .unwrap();
        let a = c.agent(&handle);
        assert_eq!(a.health, expected, "{name}: {a:?}");
        assert_eq!(
            c.f.health(&handle),
            Health::Lost,
            "the raw row ages in every case"
        );
        let lines = lines.into_inner();
        for prefix in ["LOST ", "  in_progress_orphaned "] {
            let emitted = lines.iter().any(|line| line.starts_with(prefix));
            assert_eq!(emitted, complete, "{name}, prefix={prefix:?}: {lines:?}");
        }
        assert!(
            lines.iter().any(|line| line.starts_with("PULL ")),
            "{name}: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("ready_unstaffed p2 sim/unrelated")),
            "{name}: {lines:?}"
        );
    }
}

#[test]
fn the_kept_scan_preserves_incompleteness_until_a_fresh_authoritative_scan() {
    let c = Case::new();
    let tree = linked_worktree("cached-partial");
    let handle = c.subagent_in(&tree);
    c.past_the_lost_line();
    *c.processes.borrow_mut() = Some(ProcessScan {
        listed: vec![],
        complete: false,
    });
    let probes = c.probes().kept_by(&c.f.store);
    let health = || {
        found(
            &describe(&c.f.store, &probes, c.f.now(), false).unwrap(),
            &handle,
        )
        .health
    };
    assert_eq!(health(), Health::Unknown);
    c.processes_in(vec![]);
    assert_eq!(
        health(),
        Health::Unknown,
        "cached partial evidence remains partial"
    );
    c.f.advance(PROCESS_SCAN_SECONDS);
    assert_eq!(
        health(),
        Health::Lost,
        "fresh complete absence permits aging"
    );
    assert_eq!(c.listed.get(), 2);
}
