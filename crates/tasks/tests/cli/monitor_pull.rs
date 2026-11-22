//! PULL observations through the monitor private board and clock.

use super::*;
use crate::children::Child as _;
use symphony_board::sweep::trigger::now as wall_clock;

#[test]
fn held_count_changes_warn_before_the_periodic_pull_and_failed_reads_report_unknown() {
    let m = Monitor::new("held-volume");
    let orch = m.register("orchestrator");
    let worker = m.register("worker");
    for n in 0..4 {
        let slug = format!("sim/held-{n}");
        m.ok(&orch, &["create", &slug, "Held"]);
        m.ok(&orch, &["role", &slug, "orchestrator"]);
        m.ok(&worker, &["claim", &slug]);
        let store = m.board();
        store.move_to(&slug, "review", &worker).unwrap();
        store.hold_blocked(&slug, &worker).unwrap();
    }
    m.watch(&orch, Watch::polls(4), &|m, n| match n {
        0 => {
            let held = json!(["hold", "sim/held-3"]);
            m.board().raw().row("counter", held).add("ts", 1).unwrap();
        }
        1 => {
            let slots = m.board();
            let slots = slots.raw().row("counter", json!(["slots", 7]));
            slots.create(json!({"last": 0})).unwrap();
        }
        2 => {
            let slots = json!(["slots", 7]);
            m.board().raw().row("counter", slots).delete().unwrap();
        }
        _ => {}
    });
    assert_eq!(
        m.starting(&["  low_volume "]),
        [format!(
            "  low_volume {orch} open=3 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
        )]
    );
    assert_eq!(m.starting(&["WATCH scan failed:"]).len(), 1);
    assert_eq!(
        m.pulls().len(),
        1,
        "no initial warning at four, no false zero on error, no duplicate after recovery: {:?}",
        m.lines()
    );
}

#[test]
fn a_volume_only_crossing_is_said_once_before_the_periodic_pull() {
    let m = Monitor::new("volume-crossing");
    let orch = m.register("orchestrator");
    let worker = m.register("worker");
    for n in 0..4 {
        let slug = format!("sim/work-{n}");
        m.ok(&orch, &["create", &slug, "Work"]);
        m.ok(&orch, &["role", &slug, "orchestrator"]);
        m.ok(&worker, &["claim", &slug]);
    }
    m.watch(&orch, Watch::polls(4), &|m, n| {
        if n == 0 {
            m.ok(&worker, &["move", "sim/work-3", "cancelled"]);
        }
    });
    let low = m.starting(&["  low_volume "]);
    assert_eq!(
        low,
        [format!(
            "  low_volume {orch} open=3 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
        )]
    );
}

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
    assert_eq!(
        pulls,
        [
            "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0"
        ]
    );
    assert_eq!(
        m.starting(&["  low_volume "]),
        [format!(
            "  low_volume {orch} open=1 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
        )]
    );
    assert_eq!(
        m.starting(&["LOST ", "STALE "]),
        Vec::<String>::new(),
        "{:?}",
        m.lines()
    );
    // An orphan entry would trigger a PULL; the verifying child creates no actionable gap.
}

/// `quiet_child` as an in-process subagent: its board command ran in the case's main checkout, and
/// it has a linked worktree of its own, `wt` on `sim/a`, returned. The board's clock is the
/// machine's from the start, as a process's start is.
fn subagent_child(m: &Monitor) -> (String, String, PathBuf) {
    m.now.set(wall_clock());
    let (orch, child) = quiet_child(m);
    let tree = m.dir.join("wt");
    git(
        &m.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "sim/a",
            tree.to_str().unwrap(),
        ],
    );
    let mut bind = bash(&format!("TASKS_SESSION={child} just tasks show sim/a"))
        .agent("agent-c")
        .0;
    bind["cwd"] = json!(m.repo.to_str().unwrap());
    m.now.set(wall_clock());
    m.beat(&bind);
    m.now.set(m.last_seen(&child));
    (orch, child, tree)
}

/// The subagent's background launch from its worktree `tree` (jade-heron's probe: bound elsewhere,
/// no board command since): its call's hook, the process it leaves working there, its hook again
/// once the call has returned.
fn launched(m: &Monitor, tree: &std::path::Path) -> Working {
    let call = bash("cargo test")
        .agent("agent-c")
        .with("run_in_background", json!(true))
        .0;
    let hook = |mut event: Value| {
        event["cwd"] = json!(tree.to_str().unwrap());
        m.now.set(wall_clock());
        m.beat(&event);
    };
    hook(call.clone());
    let waited = Working::in_folder(tree);
    hook(crate::beat::Event(call).ended().0);
    waited
}

/// A process working in a folder, as a subagent's background `cargo test` works in its worktree,
/// until it is dropped however the test ends: a `cat` on a pipe only this test holds, so it ends
/// with the test process too, with no deadline of its own.
struct Working(sys::Child);

impl Working {
    fn in_folder(folder: &std::path::Path) -> Self {
        let mut cat = sys::Command::of("cat");
        cat.current_dir(folder)
            .stdin(sys::Stdio::piped())
            .stdout(sys::Stdio::null());
        Self(cat.spawn().unwrap())
    }
}

impl Drop for Working {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Two-hundred-second waits, a digest at each, to past the lost line: few looks, so few listings of
/// the machine's processes.
fn long_waits_past_the_lost_line() -> Watch {
    Watch {
        interval: 200.0,
        pull_every: 30.0,
        ..Watch::polls(((LIVE_SECONDS + STALE_SECONDS) / 200.0) as usize + 2)
    }
}

#[test]
fn an_in_process_child_whose_worktree_holds_a_live_process_is_neither_lost_nor_orphaned() {
    let m = Monitor::new("in-process-waiting");
    let (orch, child, tree) = subagent_child(&m);
    // What it waits on: a child of the test runner working in its worktree, as its background
    // `cargo test` was.
    let waited = launched(&m, &tree);
    let began = m.now.get();
    m.watch(&orch, long_waits_past_the_lost_line(), &idle);
    drop(waited);
    assert!(
        m.now.get() - began > LIVE_SECONDS + STALE_SECONDS,
        "now={}, began={began}, required={}",
        m.now.get(),
        LIVE_SECONDS + STALE_SECONDS
    );
    assert_eq!(
        m.health(&child),
        "lost",
        "the board alone would call it lost"
    );
    assert_eq!(
        m.starting(&["LOST ", "STALE ", "  in_progress_orphaned "]),
        Vec::<String>::new(),
        "{:?}",
        m.lines()
    );
}

#[test]
fn the_same_in_process_child_with_no_process_in_its_worktree_is_lost_and_its_task_orphaned() {
    let m = Monitor::new("in-process-gone");
    let (orch, child, tree) = subagent_child(&m);
    drop(launched(&m, &tree)); // its run ended before it went quiet
    m.watch(&orch, long_waits_past_the_lost_line(), &idle);
    assert_eq!(
        m.starting(&["LOST "]),
        [format!("LOST {child} — its work is claimable: sim/a")]
    );
    assert!(
        m.has("  in_progress_orphaned p2 sim/a — A"),
        "{:?}",
        m.lines()
    );
}
