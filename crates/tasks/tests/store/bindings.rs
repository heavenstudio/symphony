//! The checkout an in-process key's binding keeps: where a subagent's own tool calls run, and since
//! when, beside the session its orchestrator registered elsewhere.

use crate::common::{Fixture, NOW};
use std::collections::BTreeMap;
use symphony_board::{Beat, Binding, NewSession};

/// What the board keeps of the key the session `h` beats through.
fn binding_of(f: &Fixture, h: &str) -> Binding {
    f.store.agent_bindings().unwrap().remove(h).unwrap()
}

/// Subagent key `agent-1`'s binding, in `worktree` on `branch` since `came`, its calls there read
/// from `from` on.
fn kept(worktree: &str, branch: &str, came: f64, from: f64) -> Binding {
    Binding {
        agent: "agent-1".into(),
        in_process: true,
        worktree: worktree.into(),
        branch: branch.into(),
        placed_at: came,
        stay_from: from,
    }
}

/// Subagent key `agent-1`, bound to a session registered in the main checkout.
struct Subagent<'a> {
    f: &'a Fixture,
    handle: String,
}

impl<'a> Subagent<'a> {
    fn bound(f: &'a Fixture) -> Self {
        let handle = f.session_with(NewSession {
            worktree: "/main".into(),
            ..NewSession::new("worker")
        });
        assert!(f.store.bind_agent("agent-1", &handle, false).unwrap());
        Subagent { f, handle }
    }

    /// A tool call of its, `by` seconds after the board's clock, from the checkout given.
    fn call(&self, by: f64, place: Option<(&str, &str)>) {
        self.f.advance(by);
        let beat = self
            .f
            .store
            .agent_beat_in("agent-1", &Beat::default(), place);
        assert_eq!(beat.unwrap().as_deref(), Some(self.handle.as_str()));
    }
}

#[test]
fn a_subagents_binding_keeps_the_checkout_its_tool_calls_run_in_as_it_moves() {
    let f = Fixture::empty();
    let s = Subagent::bound(&f);
    let h = s.handle.clone();
    assert_eq!(binding_of(&f, &h), kept("", "", 0.0, 0.0), "none seen yet");
    s.call(10.0, Some(("/wt/a", "tools/a")));
    let a = NOW + 10.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", a, a));
    s.call(10.0, Some(("/wt/a", "tools/a")));
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", a, a),
        "it came there once"
    );
    s.call(10.0, Some(("/wt/a", "")));
    let detached = NOW + 30.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "", detached, detached));
    s.call(10.0, Some(("/wt/b", "tools/b")));
    let b = NOW + 40.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b", b, b));
    s.call(10.0, None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/b", "tools/b", b, b),
        "an unknown checkout moves nothing"
    );
    // Its session keeps where it registered, for readers that know no binding.
    assert_eq!(f.store.session(&h).unwrap().row.worktree, "/main");
}

/// The adversarial review's clock probe at the store: a call there that reads a time before any
/// other did lowers where its stay may date from, never when it came; a move starts both again.
#[test]
fn a_clock_that_went_back_while_it_was_there_widens_its_stay_and_never_moves_when_it_came() {
    let f = Fixture::empty();
    let s = Subagent::bound(&f);
    let h = s.handle.clone();
    s.call(0.0, Some(("/wt/a", "tools/a")));
    s.call(-5.0, Some(("/wt/a", "tools/a")));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", NOW, NOW - 5.0));
    s.call(2.0, Some(("/wt/a", "tools/a")));
    s.call(-4.0, None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", NOW, NOW - 5.0),
        "only the earliest counts, and only a call there"
    );
    s.call(-1.0, Some(("/wt/a", "tools/a")));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", NOW, NOW - 8.0));
    s.call(20.0, Some(("/wt/b", "tools/b")));
    let b = NOW + 12.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b", b, b));
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
    f.advance(-5.0);
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
    assert_eq!(binding_of(&f, &other), kept("", "", 0.0, 0.0));
}

#[test]
fn a_full_sessions_binding_keeps_no_checkout_and_reads_as_no_subagents() {
    let f = Fixture::empty();
    let h = f.session("worker");
    assert!(f.store.bind_agent("session-1", &h, true).unwrap());
    f.store
        .agent_beat_in("session-1", &Beat::default(), Some(("/wt/a", "a")))
        .unwrap();
    let full = Binding {
        agent: "session-1".into(),
        ..Binding::default()
    };
    assert_eq!(binding_of(&f, &h), full);
    assert_eq!(
        f.store.bound_agents().unwrap(),
        BTreeMap::from([(h, "session-1".to_string())])
    );
}
