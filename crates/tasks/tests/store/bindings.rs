//! The checkout an in-process key's binding keeps: where a subagent's own tool calls run, and since
//! when, beside the session its orchestrator registered elsewhere.

use crate::common::{Fixture, NOW};
use std::collections::BTreeMap;
use symphony_board::{Beat, NewSession};

/// What the board keeps of the key `h` beats through: (key, in-process, worktree, branch, since).
fn binding_of(f: &Fixture, h: &str) -> (String, bool, String, String, f64) {
    let b = f.store.agent_bindings().unwrap().remove(h).unwrap();
    (b.agent, b.in_process, b.worktree, b.branch, b.placed_at)
}

#[test]
fn a_subagents_binding_keeps_the_checkout_its_tool_calls_run_in_as_it_moves() {
    let f = Fixture::empty();
    let h = f.session_with(NewSession {
        worktree: "/main".into(),
        ..NewSession::new("worker")
    });
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    let call = |place| {
        f.advance(10.0);
        let beat = f.store.agent_beat_in("agent-1", &Beat::default(), place);
        assert_eq!(beat.unwrap().as_deref(), Some(h.as_str()));
    };
    let kept = |worktree: &str, branch: &str, since: f64| {
        (
            "agent-1".into(),
            true,
            worktree.into(),
            branch.into(),
            since,
        )
    };
    assert_eq!(binding_of(&f, &h), kept("", "", 0.0), "none seen yet");
    call(Some(("/wt/a", "tools/a")));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", NOW + 10.0));
    call(Some(("/wt/a", "tools/a")));
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", NOW + 10.0),
        "it came there once"
    );
    call(Some(("/wt/a", "")));
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "", NOW + 30.0),
        "a detached head"
    );
    call(Some(("/wt/b", "tools/b")));
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b", NOW + 40.0));
    call(None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/b", "tools/b", NOW + 40.0),
        "an unknown checkout moves nothing"
    );
    // Its session keeps where it registered, for readers that know no binding.
    assert_eq!(f.store.session(&h).unwrap().row.worktree, "/main");
}

#[test]
fn a_beat_from_a_checkout_is_one_request_as_a_beat_without_one() {
    let f = Fixture::empty();
    let h = f.session("worker");
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    let requests = |place| {
        let before = f.store.client().requests();
        f.store
            .agent_beat_in("agent-1", &Beat::default(), place)
            .unwrap();
        f.store.client().requests() - before
    };
    assert_eq!(requests(Some(("/wt/a", "a"))), requests(None));
    assert_eq!(requests(Some(("/wt/b", "b"))), requests(None));
}

#[test]
fn a_key_bound_to_no_session_keeps_no_checkout() {
    let f = Fixture::empty();
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert!(!f.store.bind_agent("agent-1", &h, false).unwrap());
    let beat = f
        .store
        .agent_beat_in("agent-1", &Beat::default(), Some(("/wt/a", "a")));
    assert_eq!(beat.unwrap(), None);
    let other = f.session("worker");
    assert!(f.store.bind_agent("agent-1", &other, false).unwrap());
    assert_eq!(
        binding_of(&f, &other),
        ("agent-1".into(), true, String::new(), String::new(), 0.0)
    );
}

#[test]
fn a_full_sessions_binding_keeps_no_checkout_and_reads_as_no_subagents() {
    let f = Fixture::empty();
    let h = f.session("worker");
    assert!(f.store.bind_agent("session-1", &h, true).unwrap());
    f.store
        .agent_beat_in("session-1", &Beat::default(), Some(("/wt/a", "a")))
        .unwrap();
    assert_eq!(
        binding_of(&f, &h),
        ("session-1".into(), false, String::new(), String::new(), 0.0)
    );
    assert_eq!(
        f.store.bound_agents().unwrap(),
        BTreeMap::from([(h, "session-1".to_string())])
    );
}
