//! Idle panes with pending posts, re-review, or fully finished work.

use super::*;

#[test]
fn a_landing_changes_message_keeps_the_reviewer_until_the_fix_is_reviewed() {
    let w = World::new();
    let executor = w.executor_on("review", "sim/jump");
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    // land-approve writes changes through Store::say, without changing the task's status.
    w.store()
        .say(
            "sim/jump",
            &reviewer,
            &format!("{reviewer} asks for changes on sim/jump at tip 012345abcdef: fix the edge"),
            Some("@executor"),
        )
        .unwrap();
    assert_eq!(w.store().get("sim/jump").unwrap().status, "review");
    w.store()
        .move_to("sim/jump", "in_progress", &executor)
        .unwrap();
    let pane = w.agent(Agent::named(&reviewer));
    w.kept(&pane, How::default());
    w.store().move_to("sim/jump", "review", &executor).unwrap();
    w.store()
        .verdict("sim/jump", &reviewer, "approve", "fixed")
        .unwrap();
    assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
}

#[test]
fn a_status_answer_does_not_keep_a_finished_agents_pane() {
    let w = World::new();
    let handle = w.executor_on("in_progress", "sim/jump");
    w.store()
        .say("sim/jump", "owner", "finish this", Some(&handle))
        .unwrap();
    w.store().move_to("sim/jump", "review", &handle).unwrap();
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    w.store()
        .verdict("sim/jump", &reviewer, "approve", "finished")
        .unwrap();
    let pane = w.agent(Agent::named(&handle));
    assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
}

#[test]
fn a_finished_idle_agent_is_closed_on_the_second_idle_sweep() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    let first = w.sweep();
    assert_eq!(
        first,
        Sweep {
            closed: vec![],
            idle: vec![handle.clone()]
        }
    );
    assert!(w.has_pane(&pane));
    let second = w.sweep();
    assert_eq!(
        second.closed,
        [Closed {
            pane: pane.clone(),
            handle: handle.clone(),
            restore: restore("sim/jump", &handle),
        }]
    );
    assert!(!w.has_pane(&pane));
}

#[test]
fn an_unanswered_post_keeps_the_handle_and_its_ancestors_until_it_answers() {
    for descendant in [false, true] {
        for recipient in ["handle", "role", "released"] {
            let w = World::new();
            let parent = w.f.session("worker");
            let handle = if descendant {
                w.sub_session("worker", &parent)
            } else {
                parent.clone()
            };
            w.store().claim("sim/jump", &handle).unwrap();
            w.store().move_to("sim/jump", "review", &handle).unwrap();
            let reviewer = w.f.session("reviewer");
            w.store()
                .take_role("sim/jump", &reviewer, "reviewer")
                .unwrap();
            w.store()
                .verdict("sim/jump", &reviewer, "approve", "finished")
                .unwrap();
            if recipient == "released" {
                w.store().release("sim/jump", &handle, None).unwrap();
            }
            w.store()
                .say(
                    "sim/jump",
                    "owner",
                    "follow up",
                    Some(if recipient == "role" {
                        "@executor"
                    } else {
                        &handle
                    }),
                )
                .unwrap();
            let pane = w.agent(Agent::named(&parent));
            w.kept(&pane, How::default());
            w.store()
                .say("sim/jump", &handle, "answered", None)
                .unwrap();
            assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
        }
    }
}

#[test]
fn a_reviewer_waiting_for_its_changes_to_be_fixed_keeps_its_pane() {
    for reason in ["fix this", ""] {
        let w = World::new();
        let executor = w.executor_on("review", "sim/jump");
        let reviewer = w.f.session("reviewer");
        w.store()
            .take_role("sim/jump", &reviewer, "reviewer")
            .unwrap();
        w.store()
            .verdict("sim/jump", &reviewer, "changes", reason)
            .unwrap();
        assert_eq!(w.store().get("sim/jump").unwrap().status, "in_progress");
        let pane = w.agent(Agent::named(&reviewer));
        w.kept(&pane, How::default());
        w.store().move_to("sim/jump", "review", &executor).unwrap();
        w.store()
            .verdict("sim/jump", &reviewer, "approve", "fixed")
            .unwrap();
        assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
    }
}
