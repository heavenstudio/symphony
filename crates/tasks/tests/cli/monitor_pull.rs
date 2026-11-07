//! PULL observations through the monitor private board and clock.

use super::*;

#[test]
fn an_orchestrator_gets_a_digest_when_it_attaches() {
    let m = Monitor::new("digest");
    let orch = m.register("orchestrator");
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.watch(&orch, Watch::polls(1), &idle);
    let pulls = m.pulls();
    assert_eq!(pulls.len(), 1, "{:?}", m.lines());
    assert!(pulls[0].contains("ready_unstaffed=1"), "{pulls:?}");
    assert!(m.has("  ready_unstaffed p2 sim/a — A"), "{:?}", m.lines());
}

#[test]
fn a_worker_gets_no_digest() {
    let m = Monitor::new("no-digest");
    let worker = m.register("worker");
    m.watch(&worker, Watch::polls(3), &idle);
    assert_eq!(m.pulls(), Vec::<String>::new());
}

#[test]
fn the_digest_does_not_repeat_when_its_interval_passes() {
    let m = Monitor::new("digest-interval");
    let orch = m.register("orchestrator");
    m.ok(&orch, &["create", "sim/a", "A"]);
    let every = |polls| Watch {
        pull_every: 60.0,
        ..Watch::polls(polls)
    };
    m.watch(&orch, every(5), &idle); // 4 polls x 3 s: well inside a minute
    assert_eq!(m.pulls().len(), 1);
    m.lines.borrow_mut().clear();
    m.watch(&orch, every(25), &idle); // 24 polls x 3 s: a minute and more
    assert_eq!(m.pulls().len(), 1);
}

#[test]
fn a_gap_departure_is_silent_and_its_return_is_delivered_once() {
    for returns in [false, true] {
        let m = Monitor::new(&format!("digest-change-{returns}"));
        let (orch, worker) = (m.register("orchestrator"), m.register("worker"));
        m.ok(&orch, &["create", "sim/a", "A"]);
        m.watch(
            &orch,
            Watch {
                pull_every: 2.0,
                ..Watch::polls(8)
            },
            &|m, n| {
                if n == 1 {
                    m.ok(&worker, &["claim", "sim/a"]);
                } else if n == 2 && returns {
                    m.ok(&worker, &["release", "sim/a"]);
                }
            },
        );
        let pulls = m.pulls();
        assert_eq!(pulls.len(), if returns { 2 } else { 1 }, "{:?}", m.lines());
        assert!(
            pulls.iter().all(|line| line.contains("ready_unstaffed=1")),
            "{pulls:?}"
        );
    }
}

#[test]
fn a_landed_branch_under_an_open_task_is_in_the_digest() {
    let m = Monitor::new("landed");
    let (orch, worker) = (m.register("orchestrator"), m.register("worker"));
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.ok(&orch, &["create", "sim/b", "B"]);
    m.ok(&worker, &["claim", "sim/a"]);
    m.ok(&worker, &["claim", "sim/b"]);
    m.land("sim/a");
    git(&m.repo, &["branch", "sim/b"]); // cut, never used
    m.watch(&orch, Watch::polls(0), &idle);
    assert!(m.pulls()[0].contains("landed_open=1"), "{:?}", m.lines());
    let line = "  landed_open p2 sim/a — A   (landed on main, task still in_progress)";
    assert!(m.has(line), "{:?}", m.lines());
}

#[test]
fn idle_agents_come_from_the_agents_view() {
    let mut m = Monitor::new("idle-agents");
    let (orch, worker) = (m.register("orchestrator"), m.register("worker"));
    m.board().bind_agent("sess-w", &worker, false).unwrap();
    m.hc();
    *m.panes.borrow_mut() = json!([{"address": "t1", "agent": "claude", "status": "idle",
                                    "name": worker, "cwd": "/x", "state": "live"}]);
    m.watch(&orch, Watch::polls(1), &idle);
    assert!(m.pulls()[0].contains("idle=1"), "{:?}", m.lines());
    assert!(m.has(&format!("  idle {worker}")), "{:?}", m.lines());
}

/// `quiet_child`, launched into a worktree of its own, `/wt/child`.
#[test]
fn unanswered_stream_lines_are_once_per_post_despite_other_pull_changes() {
    let m = Monitor::new("unanswered");
    let orch = m.register("orchestrator");
    let worker = m.register("worker");
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.ok(&worker, &["claim", "sim/a"]);
    let first = m
        .board()
        .say("sim/a", "owner", "continue", Some("@executor"))
        .unwrap();
    m.now.set(NOW + 31.0 * 60.0);
    let second = Cell::new(0);
    m.watch(
        &orch,
        Watch {
            polls: 5,
            interval: 31.0 * 60.0,
            pull_every: 1.0,
            ..Watch::polls(5)
        },
        &|m, n| match n {
            0 => {
                m.ok(&orch, &["create", "sim/b", "B"]);
            }
            1 => {
                m.board().touch(&worker).unwrap();
            }
            2 => {
                second.set(
                    m.board()
                        .say("sim/a", "owner", "another request", Some(&worker))
                        .unwrap()
                        .id,
                );
            }
            _ => {}
        },
    );
    let lines = m.starting(&["  unanswered "]);
    assert_eq!(lines.len(), 2, "{lines:?}");
    for id in [first.id, second.get()] {
        assert!(id > 0);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains(&format!("post #{id} —")))
                .count(),
            1,
            "{lines:?}"
        );
    }
}

#[test]
fn an_unlaunched_child_verifying_in_a_trial_merge_is_neither_lost_nor_orphaning_its_task() {
    let m = Monitor::new("trial-merge");
    let (orch, child) = quiet_child(&m);
    let _run = m.slot(
        "/wt/trial-merge",
        true,
        m.now.get(),
        Some(&child),
        "verify-full",
    );
    let began = m.now.get();
    // A digest every poll: the last comes past the lost line (600 s), where only the run tagged with
    // the child's handle keeps it from reading lost and its task orphaned.
    let w = Watch {
        pull_every: 30.0,
        ..Watch::past_the_lost_line()
    };
    m.watch(&orch, w, &idle);
    assert!(m.now.get() - began > LIVE_SECONDS + STALE_SECONDS + 30.0);
    let pulls = m.pulls();
    assert!(pulls.is_empty(), "{:?}", m.lines());
    assert_eq!(
        m.starting(&["LOST ", "STALE "]),
        Vec::<String>::new(),
        "{:?}",
        m.lines()
    );
    // An orphan entry would trigger a PULL; the verifying child creates no actionable gap.
}
