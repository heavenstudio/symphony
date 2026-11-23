//! The hour: an agent holding a role on open work (an executor in progress, blocked or in review, a
//! reviewer of work in review or awaiting its re-review) closes once it has sat idle at its prompt
//! for `IDLE_HOUR`, saying what brings it back; work it does in that hour starts it again. An
//! addressed post still keeps it at once, and an orchestrator, a chat, the caller and a terminal on
//! screen never close.

use super::{Agent, How, restore};
use crate::{Hooked, World};
use serde_json::json;
use std::cell::Cell;
use std::collections::BTreeMap;
use symphony_board::Raw;
use symphony_board::launch::reap::{
    CHANGED, Closed, IDLE_HOUR, Kept, REAP_SECONDS, Sweep, TOOL_CALL,
};
use symphony_board::launch::screen::ON_SCREEN;

#[test]
fn a_live_reservation_keeps_a_ready_agents_pane_and_names_it_in_a_dry_run() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", None);
    let task = w.store().get("sim/jump").unwrap();
    assert_eq!(task.status, "ready");
    assert_eq!(task.roles.len(), 1, "the reservation is its only hold");
    assert_eq!(task.holders("reserved"), [handle.as_str()]);
    let before = w.changes();
    let dry = w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    let why = "it is holder of the reservation on sim/jump, ready: it closes once idle an hour";
    assert_eq!(dry.closed, []);
    assert_eq!(
        dry.kept,
        [Kept {
            pane: terminal.clone(),
            handle: handle.clone(),
            why: why.into(),
            idle: true,
            told: true,
        }]
    );
    assert_eq!(w.changes(), before, "the dry run changes no terminal");
    assert!(!w.reap_state().exists(), "the dry run records no idle mark");
    w.kept(&terminal, How::default());
}

#[test]
fn a_lapsed_done_or_cancelled_reservation_releases_the_pane_after_two_sweeps() {
    for end in ["expired", "done", "cancelled"] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", None);
        assert_eq!(w.sweep().closed, [], "{end}: live reservation");
        if end == "expired" {
            // No write removes it: the board's view must age the reservation out itself.
            w.f.advance(IDLE_HOUR);
        } else {
            w.finish_reserved_task(&handle, end);
            assert_eq!(
                w.store().get("sim/jump").unwrap().holders("reserved"),
                [handle.as_str()]
            );
        }
        let first = w.sweep();
        assert_eq!(
            first.closed,
            [],
            "{end}: the changed hold starts the finished wait"
        );
        assert_eq!(first.idle, std::slice::from_ref(&handle), "{end}");
        let [closed] = <[_; 1]>::try_from(w.sweep().closed).unwrap();
        assert_eq!(closed.pane, terminal, "{end}");
        assert_eq!(closed.why, super::FINISHED, "{end}");
        assert!(!w.has_terminal(&terminal), "{end}");
        assert!(
            w.store().sessions_of(&handle).unwrap()[0].closed.is_some(),
            "{end}"
        );
    }
}

#[test]
fn a_reservation_uses_the_existing_idle_hour_and_holds_its_parent_too() {
    for descendant in [false, true] {
        let w = World::new();
        let parent = w.f.session("worker");
        let holder = if descendant {
            w.sub_session("worker", &parent)
        } else {
            parent.clone()
        };
        w.store()
            .reserve("sim/jump", &holder, 2.0 * IDLE_HOUR)
            .unwrap();
        let terminal = w.agent(Agent::named(&parent));
        let who = if descendant {
            format!("its session {holder} is")
        } else {
            "it is".into()
        };
        let held = format!(
            "{who} holder of the reservation on sim/jump, ready: it closes once idle an hour"
        );
        w.short_of_the_hour(&terminal, &parent, &held);
        let [closed] = <[_; 1]>::try_from(w.sweep().closed).unwrap();
        assert_eq!(closed.pane, terminal);
        assert!(closed.why.starts_with("idle an hour"), "{closed:?}");
        assert!(
            closed.why.contains("reservation on sim/jump, ready"),
            "{closed:?}"
        );
        assert!(!w.has_terminal(&terminal));
    }
}

#[test]
fn a_reservation_acquired_during_either_last_look_prevents_the_close() {
    for nth in [2, 3] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        w.task("sim/dock", None);
        w.sweep();
        let gets = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.instead = Box::new(|args| {
            if args[0] == "get" && args[1] == terminal {
                gets.set(gets.get() + 1);
                if gets.get() == nth {
                    w.store().reserve("sim/dock", &handle, IDLE_HOUR).unwrap();
                }
            }
            None
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert_eq!(warned, "");
        assert_eq!(gets.get(), nth);
        assert_eq!(swept.closed, []);
        assert_eq!(swept.kept[0].why, CHANGED);
        assert!(w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap()[0].closed.is_none());
        assert!(w.calls_of(&["kill"]).is_empty());
    }
}

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
    Raw::of(&w.f.other_database())
        .row("session", json!(executor))
        .set(json!({"last_tool": started + IDLE_HOUR / 2.0}))
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

/// What happens on the board while the sweep's last looks at an agent's terminal wait.
#[derive(Clone, Copy, Debug)]
enum Meanwhile {
    /// Its agent makes a tool call.
    ToolCall,
    /// The owner asks it something.
    Post,
    /// It moves its task to review.
    Review,
}

#[test]
fn a_tool_call_a_post_or_a_move_while_the_close_looks_keeps_the_terminal_its_record_open() {
    for (status, nth, meanwhile) in [
        ("in_progress", 3, Meanwhile::ToolCall),
        ("in_progress", 3, Meanwhile::Post),
        ("in_progress", 3, Meanwhile::Review),
        ("in_progress", 2, Meanwhile::Post),
        ("done", 3, Meanwhile::ToolCall),
    ] {
        let case = format!("{status}, look {nth}, {meanwhile:?}");
        let w = World::new();
        let handle = w.executor_on(status, "sim/jump");
        let terminal = w.agent(Agent::named(&handle));
        w.sweep();
        if status == "in_progress" {
            w.f.advance(IDLE_HOUR - REAP_SECONDS);
        }
        // Its wait is over. The second look at it is the close's first, before its closure is
        // recorded; the third the close's own, after it, right before the kill. The board changes
        // just before the `nth` answers.
        let (gets, asked) = (Cell::new(0), Cell::new(0));
        let mut hooked = Hooked::new(&w);
        hooked.instead = Box::new(|args| {
            if args[0] == "get" && args[1] == terminal {
                gets.set(gets.get() + 1);
                if gets.get() != nth {
                    return None;
                }
                match meanwhile {
                    Meanwhile::ToolCall => {
                        Raw::of(&w.f.other_database())
                            .row("session", json!(handle))
                            .set(json!({"last_tool": w.f.now()}))
                            .unwrap();
                    }
                    Meanwhile::Post => {
                        let said = w
                            .store()
                            .say("sim/jump", "owner", "and this?", Some(&handle));
                        asked.set(said.unwrap().id);
                    }
                    Meanwhile::Review => {
                        w.store().move_to("sim/jump", "review", &handle).unwrap();
                    }
                }
            }
            None
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert_eq!((warned.as_str(), gets.get()), ("", nth), "{case}");
        let why = match meanwhile {
            Meanwhile::ToolCall => TOOL_CALL.to_string(),
            Meanwhile::Post => format!(
                "an addressed post #{} on sim/jump waits for its answer",
                asked.get()
            ),
            Meanwhile::Review => CHANGED.to_string(),
        };
        let kept = Kept {
            pane: terminal.clone(),
            handle: handle.clone(),
            why,
            idle: true,
            told: true,
        };
        assert_eq!((swept.closed, swept.kept), (vec![], vec![kept]), "{case}");
        assert!(w.has_terminal(&terminal), "{case}");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new(), "{case}");
        // Kept at its second look, it recorded nothing (its agent was started with no launch
        // record); at its third, the closure it recorded is open again, on the same terminal.
        let records = w.store().sessions_of(&handle).unwrap();
        if nth == 2 {
            assert!(records.is_empty(), "{case}: {records:?}");
        } else {
            let [s] = <[_; 1]>::try_from(records).unwrap();
            let open = (s.closed, s.pane.as_str());
            assert_eq!(open, (None, terminal.as_str()), "{case}");
        }
    }
}
