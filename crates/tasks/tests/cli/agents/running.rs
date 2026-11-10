//! Legacy slot attribution retains explicit local ownership evidence beside in-process waits.

use super::*;

// -- running a test --------------------------------------------------------------------

#[test]
fn an_agent_holding_a_slot_is_running_a_test() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "idle", json!({}));
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify-full");
    let a = c.agent(&handle);
    assert_eq!(a.state, "running_test");
    assert_eq!(a.detail, detail("verify-full", "holding", NOW - 90.0));
}

#[test]
fn a_running_test_is_its_state_detail_in_the_json() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&handle, "t5", "idle", json!({}));
    c.run_of("holding", "/wt/other", NOW - 90.0, "verify-full");
    assert_eq!(c.agent(&handle).state, "idle");
}

#[test]
fn a_run_in_a_subdirectory_of_the_worktree_is_not_its_agents() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&a, "t1", "working", json!({"name": a, "cwd": "/main"}));
    c.pane(&b, "t2", "working", json!({"name": b, "cwd": "/main"}));
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
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("holding", real.to_str().unwrap(), NOW - 90.0, "verify-full");
    let state = c.agent(&handle).state;
    std::fs::remove_dir_all(&tmp).unwrap();
    assert_eq!(state, "running_test");
}

#[test]
fn the_monitor_helper_agrees_with_the_view() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
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
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("waiting", "/wt/sim-jump", NOW - 90.0, "verify");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "test-sim");
    assert_eq!(c.agent(&handle).detail.unwrap().phase, "holding");
}

#[test]
fn of_two_runs_the_one_that_began_first_is_shown() {
    let c = Case::new();
    let handle = c.worker();
    c.pane(&handle, "t5", "working", json!({}));
    c.run_of("holding", "/wt/sim-jump", NOW - 10.0, "test-sim");
    c.run_of("holding", "/wt/sim-jump", NOW - 90.0, "verify");
    assert_eq!(
        c.agent(&handle).detail,
        detail("verify", "holding", NOW - 90.0)
    );
}
