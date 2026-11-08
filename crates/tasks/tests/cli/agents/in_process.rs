//! A quiet in-process agent waiting on what it started: a live process of its own in the worktree
//! its binding recorded reads `running_test` and live; one it cannot have started does not.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use symphony_board::cli::agents::PROCESS_SCAN_SECONDS;

/// A linked worktree's top folder (its `.git` a file naming a git dir checked out on `sim/jump`), in
/// a scratch folder of its own, resolved.
fn linked_worktree(name: &str) -> PathBuf {
    let scratch = crate::scratch(&format!("agents-{name}"));
    let (tree, gitdir) = (scratch.join("wt"), scratch.join("gitdir"));
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/sim/jump\n").unwrap();
    std::fs::write(tree.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    tree.canonicalize().unwrap()
}

/// A process working in `cwd`, started at `started` by the board's clock.
fn process(cwd: &Path, started: f64) -> Process {
    Process {
        cwd: cwd.to_path_buf(),
        started: Some(started),
    }
}

impl Case {
    /// A subagent its orchestrator registered in the main checkout, which claimed `sim/jump`: its
    /// board command binds it, and that call, in `tree`, records where it works, now.
    fn subagent_in(&self, tree: &Path) -> String {
        let handle = self.f.session_with(NewSession {
            parent: Some(self.orch.clone()),
            worktree: "/main".into(),
            branch: "main".into(),
            ..NewSession::new("worker")
        });
        self.f.store.claim("sim/jump", &handle).unwrap();
        assert!(
            self.f
                .store
                .bind_agent("agent-sub", &handle, false)
                .unwrap()
        );
        self.call_in("agent-sub", tree);
        handle
    }

    /// A tool call of subagent `agent` in `tree`, as the heartbeat hook hands it on.
    fn call_in(&self, agent: &str, tree: &Path) {
        let place = Some((tree.to_str().unwrap(), "sim/jump"));
        self.f
            .store
            .agent_beat_in(agent, &Beat::default(), place)
            .unwrap();
    }

    /// The machine lists these processes.
    fn processes_in(&self, listed: Vec<Process>) {
        *self.processes.borrow_mut() = Some(listed);
    }

    /// Past the line where a quiet executor's work is claimable.
    fn past_the_lost_line(&self) {
        self.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    }
}

#[test]
fn an_in_process_agent_works_where_its_binding_recorded_not_where_it_registered() {
    let c = Case::new();
    let tree = linked_worktree("placed");
    let handle = c.subagent_in(&tree);
    let a = c.agent(&handle);
    assert_eq!(
        (a.worktree.as_str(), a.branch.as_str()),
        (tree.to_str().unwrap(), "sim/jump")
    );
    assert_eq!(a.session_id.as_deref(), Some("agent-sub"));
}

#[test]
fn a_quiet_in_process_agent_whose_worktree_holds_a_process_it_started_is_running_and_never_lost() {
    let c = Case::new();
    let tree = linked_worktree("running");
    let handle = c.subagent_in(&tree);
    // The test binary of a crate in that worktree, as cargo starts it, beside one elsewhere.
    c.processes_in(vec![
        process(Path::new("/"), NOW),
        process(&tree.join("crates/tasks"), NOW),
    ]);
    c.past_the_lost_line();
    assert_eq!(c.f.health(&handle), Health::Lost);
    let view = c.view();
    let a = found(&view, &handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Live));
    let me = parsed(&view)["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["handle"] == handle.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        me["state_detail"],
        json!({"recipe": "process", "phase": "running", "since": null})
    );
    assert_eq!(me["health"], "live");
}

#[test]
fn the_same_agent_with_no_process_in_its_worktree_is_lost_as_before() {
    let c = Case::new();
    let tree = linked_worktree("idle-tree");
    let handle = c.subagent_in(&tree);
    c.processes_in(vec![
        process(Path::new("/"), NOW),
        process(&tree.with_file_name("wt-other"), NOW),
        process(tree.parent().unwrap(), NOW),
    ]);
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health, a.detail), ("gone", Health::Lost, None));
    assert_eq!(c.listed.get(), 1);
}

/// The adversarial review's probe: a process another started in the worktree before the subagent
/// came there keeps nobody alive, however long it runs.
#[test]
fn a_process_that_began_before_the_agent_came_is_not_its_even_a_day_on() {
    let c = Case::new();
    let tree = linked_worktree("stray-before");
    c.f.advance(-60.0);
    let stray = process(&tree, c.f.now());
    c.f.advance(60.0);
    let handle = c.subagent_in(&tree);
    c.processes_in(vec![stray]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).health, Health::Lost);
    c.f.advance(24.0 * 3600.0);
    assert_eq!(c.agent(&handle).health, Health::Lost);
}

#[test]
fn a_process_that_began_after_the_agents_last_tool_call_is_not_its() {
    let c = Case::new();
    let tree = linked_worktree("stray-after");
    let handle = c.subagent_in(&tree);
    c.f.advance(120.0);
    c.processes_in(vec![process(&tree, c.f.now())]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).health, Health::Lost);
}

#[test]
fn a_process_is_its_from_when_its_binding_recorded_the_worktree_to_its_last_tool_call() {
    let c = Case::new();
    let tree = linked_worktree("stay");
    let handle = c.subagent_in(&tree);
    c.f.advance(300.0);
    c.call_in("agent-sub", &tree); // the same worktree: it came at NOW, and was last seen now
    let last = c.f.now();
    c.past_the_lost_line();
    let reads = |started: f64| {
        c.processes_in(vec![process(&tree, started)]);
        c.agent(&handle).health
    };
    // The kernel's stamp and the board's clock are read apart: a second either side still counts.
    for started in [NOW - 1.0, NOW, NOW + 150.0, last, last + 1.0] {
        assert_eq!(reads(started), Health::Live, "{started}");
    }
    for started in [NOW - 1.5, last + 1.5] {
        assert_eq!(reads(started), Health::Lost, "{started}");
    }
}

/// The reviews' probe: a process working in its worktree whose start the kernel would not give
/// may be its, so the board cannot tell: unknown, never lost. One of its own still reads live, and
/// one whose start cannot be read elsewhere says nothing of it.
#[test]
fn a_process_there_whose_start_cannot_be_read_leaves_it_unknown_not_lost() {
    let c = Case::new();
    let tree = linked_worktree("unreadable");
    let scratch_repo = tree.join(".target/tmp/case/repo");
    std::fs::create_dir_all(scratch_repo.join(".git")).unwrap();
    let handle = c.subagent_in(&tree);
    let unreadable = |cwd: PathBuf| Process { cwd, started: None };
    c.processes_in(vec![
        unreadable(tree.join("crates/tasks")),
        process(&tree, NOW - 60.0),
    ]);
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Unknown));
    assert_eq!(a.detail.unwrap().phase, "unknown");
    c.processes_in(vec![unreadable(tree.clone()), process(&tree, NOW)]);
    let a = c.agent(&handle);
    assert_eq!(
        (a.health, a.detail.unwrap().phase.as_str()),
        (Health::Live, "running")
    );
    c.processes_in(vec![
        unreadable(tree.with_file_name("wt-other")),
        unreadable(scratch_repo),
    ]);
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("gone", Health::Lost));
}

/// The adversarial review's clock probe: placed at NOW, the clock went back 5 s before its
/// background build there, whose process so began before it came by the clock. It may be its: the
/// board cannot tell, never that it is gone. One that began before the earliest time a call of its
/// there read is not.
#[test]
fn a_build_begun_after_the_clock_went_back_is_a_doubt_not_an_absence() {
    let c = Case::new();
    let tree = linked_worktree("clock-back");
    let handle = c.subagent_in(&tree);
    c.f.advance(-5.0);
    c.call_in("agent-sub", &tree); // the build's call, its process begun right after
    c.processes_in(vec![process(&tree, NOW - 4.9)]);
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Unknown));
    c.processes_in(vec![process(&tree, NOW - 6.5)]);
    assert_eq!(c.agent(&handle).health, Health::Lost);
    c.processes_in(vec![process(&tree, NOW - 6.5), process(&tree, NOW + 0.5)]);
    assert_eq!(c.agent(&handle).health, Health::Live, "its own proves it");
}

/// The adversarial review's other probe: bound in one checkout, the subagent's background build
/// starts in another, and only that one holds a process of its own past the lost line.
#[test]
fn a_subagent_that_moved_is_judged_by_the_checkout_its_last_calls_ran_in() {
    let c = Case::new();
    let (first, then) = (linked_worktree("moved-a"), linked_worktree("moved-b"));
    let handle = c.subagent_in(&first);
    c.f.advance(30.0);
    c.call_in("agent-sub", &then);
    let came = c.f.now();
    assert_eq!(c.agent(&handle).worktree, then.to_str().unwrap());
    c.processes_in(vec![process(&first, NOW), process(&then, came)]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).state, "running_test");
    // What it left in the first is not looked at; nor, in the second, what began before it came.
    c.processes_in(vec![process(&first, NOW), process(&then, NOW)]);
    assert_eq!(c.agent(&handle).health, Health::Lost);
}

#[test]
fn a_process_in_a_checkout_beneath_its_worktree_is_not_its() {
    let c = Case::new();
    let tree = linked_worktree("nested");
    let scratch_repo = tree.join(".target/tmp/case/repo");
    std::fs::create_dir_all(scratch_repo.join(".git")).unwrap();
    let handle = c.subagent_in(&tree);
    c.processes_in(vec![
        process(&scratch_repo.join("sub"), NOW),
        process(&scratch_repo, NOW),
    ]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).health, Health::Lost);
}

#[test]
fn an_in_process_agent_in_the_main_checkout_is_not_judged_by_its_processes() {
    let c = Case::new();
    // The main checkout: its `.git` the common dir, a folder every agent registered there shares.
    let main = crate::scratch("agents-main-checkout")
        .canonicalize()
        .unwrap();
    std::fs::create_dir_all(main.join(".git")).unwrap();
    let handle = c.subagent_in(&main);
    c.processes_in(vec![process(&main.join("tools"), NOW)]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).health, Health::Lost);
    assert_eq!(
        c.listed.get(),
        0,
        "no listing for a folder it could not judge"
    );
}

#[test]
fn processes_that_cannot_be_listed_leave_a_quiet_in_process_agent_unknown_not_lost() {
    let c = Case::new();
    let tree = linked_worktree("unlisted");
    let handle = c.subagent_in(&tree);
    *c.processes.borrow_mut() = None;
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Unknown));
    assert_eq!(
        a.detail,
        Some(Detail {
            recipe: "process".into(),
            phase: "unknown".into(),
            since: None,
        })
    );
}

#[test]
fn the_processes_are_listed_only_for_a_quiet_in_process_agent_and_once_a_view() {
    let c = Case::new();
    let tree = linked_worktree("once");
    let handle = c.subagent_in(&tree);
    c.processes_in(vec![process(&tree, NOW)]);
    assert_eq!(c.agent(&handle).state, "working");
    assert_eq!(c.listed.get(), 0, "a live agent is not looked for");
    c.f.store
        .create(&NewTask::new("sim/other", "Other", "owner"))
        .unwrap();
    let other = c.f.session("worker");
    c.f.store.claim("sim/other", &other).unwrap();
    assert!(c.f.store.bind_agent("agent-other", &other, false).unwrap());
    c.call_in("agent-other", &tree);
    c.past_the_lost_line();
    let view = c.view();
    assert_eq!(found(&view, &handle).state, "running_test");
    assert_eq!(found(&view, &other).state, "running_test");
    assert_eq!(
        c.listed.get(),
        1,
        "one listing serves every agent of a view"
    );
}

#[test]
fn a_slot_run_of_its_own_is_shown_before_any_process() {
    let c = Case::new();
    let tree = linked_worktree("slot-first");
    let handle = c.subagent_in(&tree);
    c.processes_in(vec![process(&tree, NOW)]);
    c.tagged(&handle, tree.to_str().unwrap());
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!(a.detail.unwrap().recipe, "verify-full");
    assert_eq!(c.listed.get(), 0);
}

#[test]
fn a_full_sessions_checkout_is_not_kept_on_its_binding() {
    let c = Case::new();
    let tree = linked_worktree("full");
    let handle = c.main_session();
    assert!(c.f.store.bind_agent("session-1", &handle, true).unwrap());
    c.call_in("session-1", &tree);
    assert_eq!(c.agent(&handle).worktree, "/main");
}

#[test]
fn a_long_lived_readers_process_listing_is_kept_a_while_by_its_store() {
    let c = Case::new();
    let probes = c.probes().kept_by(&c.f.store);
    for _ in 0..3 {
        (probes.processes)().unwrap();
    }
    assert_eq!(c.listed.get(), 1);
    c.f.advance(PROCESS_SCAN_SECONDS - 1.0);
    (probes.processes)().unwrap();
    assert_eq!(c.listed.get(), 1);
    c.f.advance(1.0);
    (probes.processes)().unwrap();
    assert_eq!(
        c.listed.get(),
        2,
        "a listing as old as the window is asked again"
    );
    c.f.advance(-5.0);
    (probes.processes)().unwrap();
    assert_eq!(c.listed.get(), 3, "a clock that went back asks again");
    // A listing that failed is kept the same while: it is asked again once the window passes.
    *c.processes.borrow_mut() = None;
    c.f.advance(PROCESS_SCAN_SECONDS);
    assert!((probes.processes)().is_err());
    assert!((probes.processes)().is_err());
    assert_eq!(c.listed.get(), 4);
}

/// The adversarial review's probe: a listing that takes longer than the window is dated when it
/// ends, so the view right after it lists nothing again.
#[test]
fn a_slow_listing_is_kept_from_when_it_ended() {
    let c = Case::new();
    let slow = Probes {
        processes: Box::new(|| {
            c.listed.set(c.listed.get() + 1);
            c.f.advance(PROCESS_SCAN_SECONDS + 1.0);
            Ok(Vec::new())
        }),
        ..c.probes()
    }
    .kept_by(&c.f.store);
    (slow.processes)().unwrap();
    (slow.processes)().unwrap();
    assert_eq!(c.listed.get(), 1);
}

#[test]
fn a_listing_that_panicked_leaves_the_kept_one_readable() {
    let c = Case::new();
    c.processes_in(vec![process(Path::new("/"), NOW)]);
    let probes = c.probes();
    let kept = c.f.store.kept_scan(&*probes.processes).unwrap();
    c.f.advance(PROCESS_SCAN_SECONDS);
    let broke = catch_unwind(AssertUnwindSafe(|| {
        c.f.store.kept_scan(&|| panic!("lsof broke"))
    }));
    assert!(broke.is_err());
    // The lock is poisoned with the last listing whole: it is read, and replaced once due.
    c.f.advance(-1.0);
    assert_eq!(c.f.store.kept_scan(&*probes.processes).unwrap(), kept);
    c.f.advance(1.0);
    c.processes_in(Vec::new());
    assert_eq!(
        c.f.store.kept_scan(&*probes.processes).unwrap(),
        Vec::<Process>::new()
    );
}
