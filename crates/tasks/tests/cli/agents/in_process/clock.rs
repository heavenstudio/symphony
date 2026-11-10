//! Delayed clock readings use the real scratch store and health view. No host clock changes.

use super::*;

/// Both pinned reviews' timelines: one process already proved owned, the other beyond the old
/// bound. Missing readings leave either unknown; old/equal delayed samples cannot imply absence.
#[test]
fn a_delayed_boot_sample_cannot_erase_a_newer_unreadable_bound() {
    struct Timeline {
        known: f64,
        started: f64,
        unreadable_at: f64,
        delayed: f64,
        known_health: Health,
    }
    for timeline in [
        Timeline {
            known: 101.2,
            started: 101.1,
            unreadable_at: 101.3,
            delayed: 100.0,
            known_health: Health::Live,
        },
        Timeline {
            known: 200.0,
            started: 201.25,
            unreadable_at: 201.5,
            delayed: 200.0,
            known_health: Health::Lost,
        },
        // Even inside the old reading's slack, a delayed sample must not turn unknown into Live.
        Timeline {
            known: 101.2,
            started: 100.5,
            unreadable_at: 101.3,
            delayed: 100.0,
            known_health: Health::Live,
        },
    ] {
        for (unreadable, delayed) in [(false, false), (false, true), (true, false), (true, true)] {
            let c = Case::new();
            let tree = linked_worktree("delayed-boot");
            let handle = c.subagent_reading(&tree, Some(&since_boot(100.0)));
            c.f.advance(timeline.known - 100.0);
            c.call_reading("agent-sub", &tree, Some(&since_boot(timeline.known)));
            c.processes_in(vec![booted(&tree, timeline.started)]);
            c.f.advance(timeline.unreadable_at - timeline.known);
            if unreadable {
                c.call_reading("agent-sub", &tree, None);
            }
            if delayed {
                c.f.advance(0.1);
                c.call_reading("agent-sub", &tree, Some(&since_boot(timeline.delayed)));
            }
            c.past_the_lost_line();
            let expected = if unreadable {
                Health::Unknown
            } else {
                timeline.known_health
            };
            let a = c.agent(&handle);
            assert_eq!(
                a.health, expected,
                "known={}, start={}, unreadable={unreadable}, delayed={delayed}: {a:?}",
                timeline.known, timeline.started
            );
            assert_eq!(
                a.state,
                if expected == Health::Lost {
                    "gone"
                } else {
                    "running_test"
                }
            );
        }
    }
}

/// A sample above the old watermark may still precede the missing sample. It can prove ownership
/// through its own reading, but cannot prove the absence of a process beyond it.
#[test]
fn subsequent_samples_prove_ownership_without_inventing_a_missing_upper_bound() {
    for shift in [-5.0, 5.0] {
        let c = Case::new();
        let tree = linked_worktree("boot-gap-recovery");
        let handle = c.subagent_reading(&tree, Some(&since_boot(100.0)));
        c.call_reading("agent-sub", &tree, Some(&since_boot(200.0)));
        c.call_reading("agent-sub", &tree, None);
        c.call_reading("agent-sub", &tree, None);
        c.call_reading("agent-sub", &tree, Some(&since_boot(200.1)));
        c.processes_in(vec![booted(&tree, 201.25)]);
        c.past_the_lost_line();
        assert_eq!(
            c.agent(&handle).health,
            Health::Unknown,
            "delayed above watermark"
        );
        c.call_reading("agent-sub", &tree, Some(&since_boot(202.0)));
        c.past_the_lost_line();
        assert_eq!(
            c.agent(&handle).health,
            Health::Live,
            "new evidence proves ownership"
        );
        c.f.advance(shift);
        assert_eq!(
            c.agent(&handle).health,
            Health::Live,
            "quiet wall correction {shift}"
        );
        c.processes_in(vec![booted(&tree, 98.5)]);
        assert_eq!(
            c.agent(&handle).health,
            Health::Lost,
            "before placement stays excluded"
        );
        c.processes_in(vec![booted(&tree, 203.5)]);
        assert_eq!(
            c.agent(&handle).health,
            Health::Unknown,
            "upper bound still uncertain"
        );
        c.processes_in(Vec::new());
        assert_eq!(
            c.agent(&handle).health,
            Health::Lost,
            "no process still means lost"
        );
    }
}

#[test]
fn missing_readings_and_delayed_old_boots_cannot_revive_a_previous_boot() {
    let c = Case::new();
    let tree = linked_worktree("boot-gap-reboot");
    let handle = c.subagent_reading(&tree, Some(&since_boot(100.0)));
    c.call_reading("agent-sub", &tree, Some(&since_boot(200.0)));
    c.call_reading("agent-sub", &tree, None);
    let rebooted = Boottime {
        boot: "boot-2".into(),
        at: 5.0,
    };
    c.call_reading("agent-sub", &tree, Some(&rebooted));
    c.call_reading("agent-sub", &tree, Some(&since_boot(999.0)));
    c.processes_in(vec![booted(&tree, 150.0)]);
    c.past_the_lost_line();
    assert_eq!(
        c.agent(&handle).health,
        Health::Lost,
        "old boot cannot return"
    );
    c.processes_in(vec![Process {
        cwd: tree.clone(),
        started: Some(Began::AfterBoot(rebooted)),
    }]);
    assert_eq!(
        c.agent(&handle).health,
        Health::Unknown,
        "no placement in this boot"
    );
    c.call_reading("agent-sub", &tree, None);
    // A missing reading may span another reboot. This larger boot-2 sample can still be delayed
    // from before that call; it cannot rule out a process born in boot-3.
    c.call_reading(
        "agent-sub",
        &tree,
        Some(&Boottime {
            boot: "boot-2".into(),
            at: 5.5,
        }),
    );
    c.processes_in(vec![Process {
        cwd: tree.clone(),
        started: Some(Began::AfterBoot(Boottime {
            boot: "boot-3".into(),
            at: 1.25,
        })),
    }]);
    c.past_the_lost_line();
    assert_eq!(
        c.agent(&handle).health,
        Health::Unknown,
        "unreadable call may be in a new boot"
    );
    let next = linked_worktree("boot-gap-moved");
    c.call_reading("agent-sub", &next, Some(&since_boot(300.0)));
    c.processes_in(vec![booted(&tree, 150.0), booted(&next, 301.5)]);
    c.past_the_lost_line();
    assert_eq!(
        c.agent(&handle).health,
        Health::Lost,
        "new checkout has a reliable bound"
    );
    c.processes_in(vec![booted(&next, 300.5)]);
    assert_eq!(
        c.agent(&handle).health,
        Health::Live,
        "new checkout owns its process"
    );
}
