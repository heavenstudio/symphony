//! What an agent waits on, which reads `running_test`: a test-slot run of its own, or, for a quiet
//! in-process agent, a live process in the worktree its binding recorded.

use super::*;
use std::path::Path;
use symphony_board::cli::agents::PROCESS_SCAN_SECONDS;

// -- running a test --------------------------------------------------------------------

#[test]
fn an_agent_holding_a_slot_is_running_a_test() {
    let c = Case::new();
    let handle = c.worker();
    c.pane("t5", "idle", json!({}));
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify-full");
    let a = c.agent(&handle);
    assert_eq!(a.state, "running_test");
    assert_eq!(a.detail, detail("verify-full", "holding", NOW - 90.0));
}

#[test]
fn a_running_test_is_its_state_detail_in_the_json() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    let waiting = c.runs.borrow().clone().unwrap().remove(0);
    c.add_run(Run {
        since: None,
        ..waiting
    });
    let view = parsed(&c.view());
    let me = view["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["handle"] == handle.as_str())
        .unwrap();
    assert_eq!(
        me["state_detail"],
        json!({"recipe": "verify", "phase": "waiting", "since": null})
    );
}

#[test]
fn an_agent_waiting_for_a_slot_says_so() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    let a = c.agent(&handle);
    assert_eq!(a.state, "running_test");
    assert_eq!(a.detail, detail("verify", "waiting", NOW - 90.0));
}

#[test]
fn a_waiting_agent_is_never_lost() {
    let c = Case::new();
    let handle = c.worker();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    assert_eq!(c.agent(&handle).health, Health::Lost);
    c.run_of("waiting", "/wt/sim-jump", NOW, "verify-full");
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Live));
}

#[test]
fn the_same_agent_without_the_run_reads_gone_and_lost() {
    let c = Case::new();
    let handle = c.worker();
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("gone", Health::Lost));
}

#[test]
fn a_run_in_another_worktree_is_not_this_agents() {
    let c = Case::new();
    let handle = c.worker();
    c.pane("t5", "idle", json!({}));
    c.run_of("holding", "/wt/other", NOW - 90.0, "verify-full");
    assert_eq!(c.agent(&handle).state, "idle");
}

#[test]
fn a_run_in_a_subdirectory_of_the_worktree_is_not_its_agents() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of(
        "holding",
        "/wt/sim-jump/artifacts/export",
        NOW - 90.0,
        "verify-full",
    );
    assert_eq!(c.agent(&handle).state, "working");
}

/// azure-raven's live case: one run under lamp-works/.worktrees/x made 25 main-checkout agents read
/// running_test.
#[test]
fn a_run_in_a_worktree_nested_under_the_main_checkout_is_not_every_main_agents() {
    let c = Case::new();
    let mains: Vec<String> = (0..3).map(|_| c.main_session()).collect();
    let lost = c.main_session();
    c.f.store.claim("sim/jump", &lost).unwrap(); // work another could take, so it reads lost rather than left
    c.run_of("holding", "/main/.worktrees/x", NOW - 90.0, "verify-full");
    c.f.advance(LIVE_SECONDS + STALE_SECONDS + 600.0);
    for h in &mains {
        c.f.store.touch(h).unwrap(); // the main-checkout agents beat; the last one stays quiet and is lost
    }
    let view = c.view();
    for h in &mains {
        assert_ne!(found(&view, h).state, "running_test", "{h}");
    }
    let a = found(&view, &lost);
    assert_eq!((a.state, a.health), ("gone", Health::Lost));
}

#[test]
fn a_run_in_the_main_checkout_is_not_attributed_to_agents_sharing_it() {
    let c = Case::new();
    let (a, b) = (c.main_session(), c.main_session());
    c.run_of("holding", "/main", NOW - 90.0, "verify-full");
    assert_eq!(
        (c.agent(&a).state, c.agent(&b).state),
        ("working", "working")
    );
}

#[test]
fn a_run_is_attributed_when_one_agent_alone_holds_the_worktree() {
    let c = Case::new();
    let handle = c.worker_in("/wt/alone");
    c.main_session();
    c.run_of("holding", "/wt/alone", NOW - 90.0, "verify-full");
    assert_eq!(c.agent(&handle).state, "running_test");
}

#[test]
fn a_run_matches_its_worktree_through_a_symbolic_link() {
    let c = Case::new();
    let tmp = std::env::temp_dir().join(format!("agents-link-{}", std::process::id()));
    let real = tmp.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = tmp.join("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let handle = c.worker_in(link.to_str().unwrap());
    c.run_of("holding", real.to_str().unwrap(), NOW - 90.0, "verify-full");
    let state = c.agent(&handle).state;
    std::fs::remove_dir_all(&tmp).unwrap();
    assert_eq!(state, "running_test");
}

#[test]
fn the_monitor_helper_agrees_with_the_view() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify-full");
    assert_eq!(
        running_tests(&c.f.store, &c.probes()).unwrap()[&handle].phase,
        "waiting"
    );
    let mains: Vec<String> = (0..2).map(|_| c.main_session()).collect();
    c.run_of("holding", "/main", NOW - 90.0, "verify-full");
    assert!(
        !running_tests(&c.f.store, &c.probes())
            .unwrap()
            .contains_key(&mains[0])
    );
}

#[test]
fn a_holding_run_beats_a_waiting_one() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "test-sim");
    assert_eq!(c.agent(&handle).detail.unwrap().phase, "holding");
}

#[test]
fn of_two_runs_the_one_that_began_first_is_shown() {
    let c = Case::new();
    let handle = c.worker();
    c.run_of("holding", "/wt/sim-jump", NOW - 10.0, "test-sim");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify");
    assert_eq!(
        c.agent(&handle).detail,
        detail("verify", "holding", NOW - 90.0)
    );
}

// -- an in-process agent waiting on what it started ------------------------------------

/// A linked worktree's top folder (its `.git` a file), in a scratch folder of its own.
fn linked_worktree(name: &str) -> PathBuf {
    let tree = crate::scratch(&format!("agents-{name}")).join("wt");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join(".git"), "gitdir: /nowhere/worktrees/wt\n").unwrap();
    tree
}

impl Case {
    /// A subagent its orchestrator registered in the main checkout, which claimed `sim/jump` and
    /// whose board commands run in `tree` on `sim/jump`: its binding records them.
    fn subagent_in(&self, tree: &Path) -> String {
        let handle = self.f.session_with(NewSession {
            parent: Some(self.orch.clone()),
            worktree: "/main".into(),
            branch: "main".into(),
            ..NewSession::new("worker")
        });
        self.f.store.claim("sim/jump", &handle).unwrap();
        let place = Some((tree.to_str().unwrap(), "sim/jump"));
        assert!(
            self.f
                .store
                .bind_agent_in("agent-sub", &handle, false, place)
                .unwrap()
        );
        handle
    }

    /// The machine's processes work in these folders.
    fn processes_in(&self, folders: &[PathBuf]) {
        *self.cwds.borrow_mut() = Some(folders.to_vec());
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
fn a_quiet_in_process_agent_whose_worktree_holds_a_live_process_is_running_and_never_lost() {
    let c = Case::new();
    let tree = linked_worktree("running");
    let handle = c.subagent_in(&tree);
    // The test binary of a crate in that worktree, as cargo starts it.
    c.processes_in(&[PathBuf::from("/"), tree.join("crates/tasks")]);
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
    let beside = tree.with_file_name("wt-other");
    c.processes_in(&[
        PathBuf::from("/"),
        beside,
        tree.parent().unwrap().to_path_buf(),
    ]);
    c.past_the_lost_line();
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health, a.detail), ("gone", Health::Lost, None));
    assert_eq!(c.listed.get(), 1);
}

#[test]
fn a_process_in_a_checkout_beneath_its_worktree_is_not_its() {
    let c = Case::new();
    let tree = linked_worktree("nested");
    let scratch_repo = tree.join(".target/tmp/case/repo");
    std::fs::create_dir_all(scratch_repo.join(".git")).unwrap();
    let handle = c.subagent_in(&tree);
    c.processes_in(&[scratch_repo.join("sub"), scratch_repo]);
    c.past_the_lost_line();
    assert_eq!(c.agent(&handle).health, Health::Lost);
}

#[test]
fn an_in_process_agent_in_the_main_checkout_is_not_judged_by_its_processes() {
    let c = Case::new();
    // The main checkout: its `.git` the common dir, a folder every agent registered there shares.
    let main = crate::scratch("agents-main-checkout");
    std::fs::create_dir_all(main.join(".git")).unwrap();
    let handle = c.subagent_in(&main);
    c.processes_in(&[main.join("tools")]);
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
    *c.cwds.borrow_mut() = None;
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
    c.processes_in(std::slice::from_ref(&tree));
    assert_eq!(c.agent(&handle).state, "working");
    assert_eq!(c.listed.get(), 0, "a live agent is not looked for");
    c.f.store
        .create(&NewTask::new("sim/other", "Other", "owner"))
        .unwrap();
    let other = c.f.session("worker");
    c.f.store.claim("sim/other", &other).unwrap();
    assert!(
        c.f.store
            .bind_agent_in(
                "agent-other",
                &other,
                false,
                Some((tree.to_str().unwrap(), ""))
            )
            .unwrap()
    );
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
    c.processes_in(std::slice::from_ref(&tree));
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
    let place = Some((tree.to_str().unwrap(), "sim/jump"));
    assert!(
        c.f.store
            .bind_agent_in("session-1", &handle, true, place)
            .unwrap()
    );
    assert_eq!(c.agent(&handle).worktree, "/main");
}

#[test]
fn a_long_lived_readers_process_listing_is_kept_a_while_by_its_store() {
    let c = Case::new();
    let probes = c.probes().kept_by(&c.f.store);
    for _ in 0..3 {
        (probes.cwds)().unwrap();
    }
    assert_eq!(c.listed.get(), 1);
    c.f.advance(PROCESS_SCAN_SECONDS - 1.0);
    (probes.cwds)().unwrap();
    assert_eq!(c.listed.get(), 1);
    c.f.advance(1.0);
    (probes.cwds)().unwrap();
    assert_eq!(
        c.listed.get(),
        2,
        "a listing as old as the window is asked again"
    );
    c.f.advance(-5.0);
    (probes.cwds)().unwrap();
    assert_eq!(c.listed.get(), 3, "a clock that went back asks again");
    // A listing that failed is kept the same while: it is asked again once the window passes.
    *c.cwds.borrow_mut() = None;
    c.f.advance(PROCESS_SCAN_SECONDS);
    assert!((probes.cwds)().is_err());
    assert!((probes.cwds)().is_err());
    assert_eq!(c.listed.get(), 4);
}
