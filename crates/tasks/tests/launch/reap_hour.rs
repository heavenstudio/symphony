//! The hour: an agent holding a role on open work (an executor in progress, blocked or in review, a
//! reviewer of work in review or awaiting its re-review) closes once it has sat idle at its prompt
//! for `IDLE_HOUR`, saying what brings it back; work it does in that hour starts it again. An
//! addressed post still keeps it at once, and an orchestrator, a chat, the caller and a terminal on
//! screen never close.

use super::{Agent, How, restore};
use crate::World;
use crate::common::RawWrite;
use serde_json::json;
use std::collections::BTreeMap;
use symphony_board::launch::reap::{Closed, IDLE_HOUR, Kept, REAP_SECONDS, Sweep};
use symphony_board::launch::screen::ON_SCREEN;

impl World {
    /// A reviewer of `slug`, which its executor took to review: the reviewer's handle.
    fn reviewer_on(&self, slug: &str) -> String {
        self.executor_on("review", slug);
        let reviewer = self.f.session("reviewer");
        self.store().take_role(slug, &reviewer, "reviewer").unwrap();
        reviewer
    }

    /// A sweep, then the clock moved to a second short of the hour since it: the sweep then, which
    /// keeps the terminal of `handle` for `held`, said once at the first.
    fn short_of_the_hour(&self, terminal: &str, handle: &str, held: &str) {
        let kept = |told| Sweep {
            kept: vec![Kept {
                pane: terminal.into(),
                handle: handle.into(),
                why: held.into(),
                idle: true,
                told,
            }],
            ..Sweep::default()
        };
        assert_eq!(self.sweep(), kept(true));
        self.f.advance(IDLE_HOUR - REAP_SECONDS - 1.0);
        assert_eq!(self.sweep(), kept(false), "a second short of the hour");
        assert!(self.has_terminal(terminal));
    }

    /// The sweep past the hour closes the terminal of `handle` for `why`, with its restore line on
    /// `slug`'s thread.
    fn closes_past_the_hour(&self, terminal: &str, handle: &str, slug: &str, why: &str) {
        let line = restore(slug, handle);
        let closed = Closed {
            pane: terminal.into(),
            handle: handle.into(),
            why: why.into(),
            restore: line.clone(),
        };
        assert_eq!(self.sweep().closed, [closed]);
        assert!(!self.has_terminal(terminal));
        let said = self.f.said(slug);
        assert_eq!(
            said.last(),
            Some(&format!(
                "closed the pane of {handle} ({why}); restore: {line}"
            ))
        );
    }
}

#[test]
fn a_reviewer_idle_an_hour_closes_while_its_task_stays_in_review_and_one_idle_less_stays() {
    // Its verdict is land's (`just land-approve`), which the board does not hold: the task stays
    // in review, waiting for the other stamps.
    let w = World::new();
    let reviewer = w.reviewer_on("sim/jump");
    let terminal = w.agent(Agent::named(&reviewer));
    let held = "it is reviewer of sim/jump, in review: it closes once idle an hour";
    w.short_of_the_hour(&terminal, &reviewer, held);
    let why = "idle an hour as reviewer of sim/jump, in review; a reviewer launch for its next \
               tip resumes it, a post to it restores it";
    w.closes_past_the_hour(&terminal, &reviewer, "sim/jump", why);
    assert_eq!(w.store().get("sim/jump").unwrap().status, "review");
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&reviewer).unwrap()).unwrap();
    assert_eq!((s.role.as_str(), s.closed_why.as_str()), ("reviewer", why));
}

#[test]
fn a_reviewer_awaiting_its_re_review_closes_after_the_hour_too() {
    let w = World::new();
    let reviewer = w.reviewer_on("sim/jump");
    w.store()
        .verdict("sim/jump", &reviewer, "changes", "fix the edge")
        .unwrap();
    let terminal = w.agent(Agent::named(&reviewer));
    let held = "it is reviewer of sim/jump, awaiting its re-review: it closes once idle an hour";
    w.short_of_the_hour(&terminal, &reviewer, held);
    let why = "idle an hour as reviewer of sim/jump, awaiting its re-review; a reviewer launch \
               for its next tip resumes it, a post to it restores it";
    w.closes_past_the_hour(&terminal, &reviewer, "sim/jump", why);
}

#[test]
fn an_executor_waiting_in_review_blocked_or_in_progress_closes_after_the_hour() {
    for (status, said, back) in [
        (
            "review",
            "in review",
            "a changes verdict or a post to it restores it",
        ),
        ("blocked", "blocked", "a post to it restores it"),
        ("in_progress", "in progress", "a post to it restores it"),
    ] {
        let w = World::new();
        let executor = w.executor_on(status, "sim/jump");
        let terminal = w.agent(Agent::named(&executor));
        let held = format!("it is executor of sim/jump, {said}: it closes once idle an hour");
        w.short_of_the_hour(&terminal, &executor, &held);
        let why = format!("idle an hour as executor of sim/jump, {said}; {back}");
        w.closes_past_the_hour(&terminal, &executor, "sim/jump", &why);
        assert_eq!(w.store().get("sim/jump").unwrap().status, status);
    }
}

#[test]
fn a_session_under_the_agent_holding_the_role_closes_it_after_the_hour_naming_it() {
    let w = World::new();
    w.executor_on("review", "sim/jump");
    let parent = w.f.session("worker");
    let child = w.sub_session("reviewer", &parent);
    w.store().take_role("sim/jump", &child, "reviewer").unwrap();
    let terminal = w.agent(Agent::named(&parent));
    let held = format!(
        "its session {child} is reviewer of sim/jump, in review: it closes once idle an hour"
    );
    w.short_of_the_hour(&terminal, &parent, &held);
    let [closed] = <[_; 1]>::try_from(w.sweep().closed).unwrap();
    assert_eq!(
        closed.why,
        format!(
            "idle an hour as the parent of {child}, reviewer of sim/jump, in review; a reviewer \
             launch for its next tip resumes it, a post to it restores it"
        )
    );
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn a_tool_call_in_the_hour_or_a_change_of_its_tasks_status_starts_the_hour_again() {
    let w = World::new();
    let executor = w.executor_on("in_progress", "sim/jump");
    let terminal = w.agent(Agent::named(&executor));
    let started = w.f.now();
    w.sweep();
    // It worked between two sweeps, each finding it at its prompt: its agent beat half an hour in.
    w.f.other_client()
        .write(
            "UPDATE type::record('session', $handle) SET last_tool = $at RETURN NONE;",
            json!({"handle": executor, "at": started + IDLE_HOUR / 2.0}),
        )
        .unwrap();
    w.f.advance(IDLE_HOUR - REAP_SECONDS);
    assert_eq!(
        w.sweep().closed,
        [],
        "an hour since the first sweep, half since its beat"
    );
    w.f.advance(IDLE_HOUR / 2.0 - REAP_SECONDS);
    // Its task moved: the hour starts again from the sweep that finds it so.
    w.store().move_to("sim/jump", "review", &executor).unwrap();
    assert_eq!(w.sweep().closed, []);
    w.f.advance(IDLE_HOUR - REAP_SECONDS - 1.0);
    assert_eq!(w.sweep().closed, []);
    assert_eq!(super::terminals(&w.sweep()), [terminal.as_str()]);
}

#[test]
fn an_addressed_post_keeps_a_role_holder_past_the_hour() {
    let w = World::new();
    let executor = w.executor_on("review", "sim/jump");
    let post = w
        .store()
        .say("sim/jump", "owner", "one more thing", Some(&executor))
        .unwrap();
    let terminal = w.agent(Agent::named(&executor));
    w.sweep();
    w.f.advance(2.0 * IDLE_HOUR);
    let swept = w.sweep();
    assert_eq!(swept.closed, []);
    let why = format!(
        "an addressed post #{} on sim/jump waits for its answer",
        post.id
    );
    assert_eq!(
        swept.kept,
        [Kept {
            pane: terminal.clone(),
            handle: executor,
            why,
            idle: true,
            told: false,
        }]
    );
    assert!(w.has_terminal(&terminal));
}

#[test]
fn an_orchestrator_a_chat_the_caller_and_a_terminal_on_screen_never_close_and_each_says_why() {
    let mut w = World::new();
    let orchestrator = w.f.session("orchestrator");
    let chat = w.f.session("chat");
    let caller = w.reviewer_on("sim/jump");
    w.task("sim/dock", None);
    let watched = w.executor_on("in_progress", "sim/dock");
    let here = w.f.session("worker");
    let mut why = BTreeMap::new();
    for (handle, said) in [
        (&orchestrator, "an orchestrator never closes"),
        (&chat, "a chat never closes"),
        (&caller, "it is the session this sweep runs for"),
        (&watched, ON_SCREEN),
    ] {
        why.insert(w.agent(Agent::named(handle)), (handle.clone(), said, true));
    }
    let current = w.agent(Agent::named(&here));
    why.insert(
        current.clone(),
        (here, "it is the terminal this sweep runs in", false),
    );
    w.set("HC_TERMINAL_ID", &current);
    let on_screen = why.iter().find(|(_, (h, _, _))| *h == watched).unwrap().0;
    w.shows(json!({"agent": on_screen}));
    let how = How {
        caller: Some(&caller),
        ..How::default()
    };
    w.sweep_as(how);
    w.f.advance(2.0 * IDLE_HOUR);
    let swept = w.sweep_as(how);
    assert_eq!((swept.closed, swept.idle), (vec![], vec![]));
    let kept: BTreeMap<String, (String, &str, bool)> = swept
        .kept
        .iter()
        .map(|k| (k.pane.clone(), (k.handle.clone(), k.why.as_str(), k.idle)))
        .collect();
    assert_eq!(kept, why);
    assert!(
        swept.kept.iter().all(|k| !k.told),
        "said at the first sweep"
    );
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
}
