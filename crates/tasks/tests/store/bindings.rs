//! The checkout an in-process key's binding keeps: where a subagent's own tool calls run, beside the
//! session its orchestrator registered elsewhere.

use crate::common::Fixture;
use std::collections::BTreeMap;
use symphony_board::NewSession;

/// What the board keeps of the key `h` beats through: (key, in-process, worktree, branch).
fn binding_of(f: &Fixture, h: &str) -> (String, bool, String, String) {
    let b = f.store.agent_bindings().unwrap().remove(h).unwrap();
    (b.agent, b.in_process, b.worktree, b.branch)
}

#[test]
fn a_subagents_binding_keeps_the_checkout_its_board_commands_ran_in_as_it_moves() {
    let f = Fixture::empty();
    let h = f.session_with(NewSession {
        worktree: "/main".into(),
        ..NewSession::new("worker")
    });
    let bind = |place| f.store.bind_agent_in("agent-1", &h, false, place).unwrap();
    assert!(bind(Some(("/wt/a", "tools/a"))));
    let kept =
        |worktree: &str, branch: &str| ("agent-1".into(), true, worktree.into(), branch.into());
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a"));
    assert!(bind(Some(("/wt/a", ""))));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", ""), "a detached head");
    assert!(bind(Some(("/wt/b", "tools/b"))));
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b"));
    assert!(bind(None));
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/b", "tools/b"),
        "an unknown checkout moves nothing"
    );
    // Its session keeps where it registered, for readers that know no binding.
    assert_eq!(f.store.session(&h).unwrap().row.worktree, "/main");
}

#[test]
fn binding_again_in_the_checkout_kept_writes_no_more_than_a_bind_without_one() {
    let f = Fixture::empty();
    let h = f.session("worker");
    assert!(
        f.store
            .bind_agent_in("agent-1", &h, false, Some(("/wt/a", "a")))
            .unwrap()
    );
    let requests = |place| {
        let before = f.store.client().requests();
        assert!(f.store.bind_agent_in("agent-1", &h, false, place).unwrap());
        f.store.client().requests() - before
    };
    assert_eq!(requests(Some(("/wt/a", "a"))), requests(None));
}

#[test]
fn a_binding_refused_keeps_no_checkout() {
    let f = Fixture::empty();
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert!(
        !f.store
            .bind_agent_in("agent-1", &h, false, Some(("/wt/a", "a")))
            .unwrap()
    );
    let other = f.session("worker");
    assert!(f.store.bind_agent("agent-1", &other, false).unwrap());
    assert_eq!(
        binding_of(&f, &other),
        ("agent-1".into(), true, String::new(), String::new())
    );
}

#[test]
fn a_full_sessions_binding_keeps_no_checkout_and_reads_as_no_subagents() {
    let f = Fixture::empty();
    let h = f.session("worker");
    assert!(
        f.store
            .bind_agent_in("session-1", &h, true, Some(("/wt/a", "a")))
            .unwrap()
    );
    assert_eq!(
        binding_of(&f, &h),
        ("session-1".into(), false, String::new(), String::new())
    );
    assert_eq!(
        f.store.bound_agents().unwrap(),
        BTreeMap::from([(h, "session-1".to_string())])
    );
}
