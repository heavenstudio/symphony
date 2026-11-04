//! Addressed work follows its recipient's role lifetime and subsequent activity.

use super::*;

#[test]
fn unanswered_posts_observe_age_role_health_answers_and_recovery_commands() {
    for status in ["in_progress", "blocked", "review"] {
        let c = Case::new();
        c.task("sim/a", "in_progress");
        let executor = c.executor("sim/a");
        if status != "in_progress" {
            c.f.store.move_to("sim/a", status, &executor).unwrap();
        }
        for to in ["@executor", executor.as_str()] {
            let post =
                c.f.store
                    .say("sim/a", "owner", "reconcile", Some(to))
                    .unwrap();
            c.f.advance(29.0 * 60.0);
            assert_eq!(c.digest().count("unanswered"), 0);
            c.f.advance(2.0 * 60.0);
            let d = c.digest();
            assert_eq!(slugs(&d, "unanswered"), ["sim/a"]);
            assert!(
                d.lines()
                    .iter()
                    .any(|line| line.contains(&format!("post #{}", post.id))
                        && line.contains("just tasks launch sim/a --role executor"))
            );
            assert_eq!(
                c.verifying(std::slice::from_ref(&executor))
                    .count("unanswered"),
                0
            );
            c.f.store
                .say("sim/a", &executor, "reconciled", None)
                .unwrap();
            assert_eq!(c.digest().count("unanswered"), 0);
        }
    }
}

#[test]
fn a_closed_reviewer_and_a_left_recorded_executor_are_detected() {
    let c = Case::new();
    c.task("sim/a", "review");
    let executor = c.executor("sim/a");
    let reviewer = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &reviewer, "reviewer").unwrap();
    for (handle, role, conversation) in [
        (&executor, "executor", "conversation"),
        (&reviewer, "reviewer", "codex:thread"),
    ] {
        c.f.store
            .record_launch(&tasks::NewLaunch {
                claude_session: conversation.into(),
                ..tasks::NewLaunch::new("sim/a", handle, role)
            })
            .unwrap();
        c.f.store
            .close_task_session("sim/a", handle, "closed", "restore")
            .unwrap();
        c.f.store
            .say("sim/a", "owner", "continue", Some(&format!("@{role}")))
            .unwrap();
    }
    c.f.store.leave(&executor).unwrap();
    c.f.advance(31.0 * 60.0);
    c.f.store.touch(&reviewer).unwrap();
    let d = c.digest();
    assert_eq!(d.count("unanswered"), 1);
    let lines = d.lines().join("\n");
    for handle in [&executor, &reviewer] {
        assert!(
            lines.contains(&format!("just tasks restore sim/a {handle}")),
            "{lines}"
        );
    }
    c.f.store.revive(&executor).unwrap();
    c.f.store.say("sim/a", &executor, "answered", None).unwrap();
    c.f.store.say("sim/a", &reviewer, "answered", None).unwrap();
    assert_eq!(c.digest().count("unanswered"), 0);
}

#[test]
fn a_replacement_does_not_inherit_a_predecessors_role_addressed_post() {
    for role in ["executor", "reviewer"] {
        for elapsed in [0.0, 1.0] {
            let c = Case::new();
            c.task(
                "sim/a",
                if role == "executor" {
                    "in_progress"
                } else {
                    "review"
                },
            );
            let old = if role == "executor" {
                c.executor("sim/a")
            } else {
                let h = c.f.session("reviewer");
                c.f.store.take_role("sim/a", &h, role).unwrap();
                h
            };
            let previous =
                c.f.store
                    .say("sim/a", "owner", "old request", Some(&format!("@{role}")))
                    .unwrap();
            c.f.advance(elapsed);
            c.f.store.release("sim/a", &old, Some(role)).unwrap();
            let replacement = c.f.session(if role == "executor" {
                "worker"
            } else {
                "reviewer"
            });
            if role == "executor" {
                c.f.store.claim("sim/a", &replacement).unwrap();
            } else {
                c.f.store.take_role("sim/a", &replacement, role).unwrap();
            }
            c.f.advance(31.0 * 60.0);
            assert_eq!(c.digest().count("unanswered"), 0, "{role}, {elapsed}");
            let next =
                c.f.store
                    .say("sim/a", "owner", "new request", Some(&format!("@{role}")))
                    .unwrap();
            c.f.advance(31.0 * 60.0);
            let lines = c.digest().lines().join("\n");
            assert!(lines.contains(&format!("post #{} —", next.id)), "{lines}");
            assert!(
                !lines.contains(&format!("post #{} —", previous.id)),
                "{lines}"
            );
        }
    }
}

#[test]
fn the_addressed_agents_status_move_answers_its_post() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    let executor = c.executor("sim/a");
    c.f.store
        .say("sim/a", "owner", "continue", Some("@executor"))
        .unwrap();
    c.f.advance(31.0 * 60.0);
    assert_eq!(c.digest().count("unanswered"), 1);
    c.f.store.move_to("sim/a", "blocked", &executor).unwrap();
    c.f.advance(31.0 * 60.0);
    assert_eq!(c.digest().count("unanswered"), 0);
}
