//! Addressed work follows its recipient's role lifetime and subsequent activity.

use super::*;

#[test]
fn leaving_after_answering_does_not_hide_a_replacements_reply() {
    let c = Case::new();
    c.task("sim/a", "review");
    let a = c.f.session("reviewer");
    let b = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &a, "reviewer").unwrap();
    c.f.store
        .record_launch(&symphony_board::NewLaunch::new("sim/a", &a, "reviewer"))
        .unwrap();
    c.f.store.release("sim/a", &a, Some("reviewer")).unwrap();
    c.f.store.take_role("sim/a", &b, "reviewer").unwrap();
    c.f.store
        .record_launch(&symphony_board::NewLaunch::new("sim/a", &b, "reviewer"))
        .unwrap();
    c.f.store
        .say("sim/a", "owner", "review", Some("@reviewer"))
        .unwrap();
    c.f.store
        .say("sim/a", &b, "review completed", None)
        .unwrap();
    c.f.store.leave(&b).unwrap();
    c.f.advance(31.0 * 60.0);
    assert_eq!(c.digest().count("unanswered"), 0);
}

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
            .record_launch(&symphony_board::NewLaunch {
                claude_session: conversation.into(),
                ..symphony_board::NewLaunch::new("sim/a", handle, role)
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

#[test]
fn unanswered_digest_rows_follow_the_tasks_orchestrator() {
    for cap in [None, Some(1)] {
        let c = Case::new();
        c.task("sim/a", "in_progress");
        let worker = c.executor("sim/a");
        let owner = c.f.session("orchestrator");
        let other = c.f.session("orchestrator");
        c.f.store
            .take_role("sim/a", &owner, "orchestrator")
            .unwrap();
        let post =
            c.f.store
                .say("sim/a", "owner", "continue", Some(&worker))
                .unwrap();
        c.f.advance(31.0 * 60.0);
        // Keep task ownership live while the executor's heartbeat expires.
        c.f.store.touch(&owner).unwrap();
        let store = c.f.other().capping(cap);
        let digest =
            || symphony_board::cli::pull::digest(&store, &[], None, c.f.now(), &[]).unwrap();
        let board = store.board().unwrap();
        let heard = digest().only_for(&board, &owner);
        assert_eq!(heard.count("unanswered"), 1);
        assert!(
            heard
                .event_lines()
                .join("\n")
                .contains(&format!("post #{}", post.id))
        );
        assert_eq!(heard.cap, cap);
        assert_eq!(heard.slots, cap.map(|_| 1));
        assert_eq!(heard.lines()[0].contains("slots=1/1"), cap.is_some());
        let withheld = digest().only_for(&board, &other);
        assert_eq!(withheld.count("unanswered"), 0);
        assert!(withheld.entries().iter().all(|e| e.0 != "unanswered"));
        assert!(!withheld.event_lines().join("\n").contains("unanswered"));
        assert_eq!(withheld.slots, cap.map(|_| 0));
        assert_eq!(withheld.lines()[0].contains("slots=0/1"), cap.is_some());
    }
}

#[test]
fn descendant_actions_answer_the_recipient_at_any_depth_with_equal_clocks() {
    for action in [
        "message", "verdict", "claim", "done", "move", "release", "join",
    ] {
        let c = Case::new();
        c.task("sim/a", "review");
        let recipient = c.f.session("reviewer");
        c.f.store
            .take_role("sim/a", &recipient, "reviewer")
            .unwrap();
        c.task_with(c.sub("instructions/one", "sim/a"), "ready");
        let child = if action == "message" {
            "instructions/one"
        } else {
            "elsewhere/deep"
        };
        c.task_with(c.sub("elsewhere/deep", "instructions/one"), "ready");
        if matches!(action, "done" | "move" | "release") {
            c.f.store.claim(child, &recipient).unwrap();
        }
        if action == "verdict" {
            c.f.historical_review(child);
            c.f.store.take_role(child, &recipient, "reviewer").unwrap();
        }
        let post =
            c.f.store
                .say("sim/a", "owner", "review instruction", Some("@reviewer"))
                .unwrap();
        match action {
            "message" => {
                c.f.store.say(child, &recipient, "answered", None).unwrap();
            }
            "verdict" => {
                c.f.store
                    .verdict(child, &recipient, "approve", "reviewed")
                    .unwrap();
            }
            "claim" => {
                c.f.store.claim(child, &recipient).unwrap();
            }
            "done" => {
                c.f.store.done(child, &recipient, "integrated").unwrap();
            }
            "move" => {
                c.f.store.move_to(child, "blocked", &recipient).unwrap();
            }
            "release" => {
                c.f.store.release(child, &recipient, None).unwrap();
            }
            "join" => {
                c.f.store.take_role(child, &recipient, "reviewer").unwrap();
            }
            _ => unreachable!(),
        }
        let reply = c.f.store.thread(child).unwrap().pop().unwrap();
        assert!(reply.id > post.id, "{action}");
        assert_eq!(reply.ts, post.ts, "equal-clock sequence: {action}");
        c.f.advance(31.0 * 60.0);
        assert_eq!(c.digest().count("unanswered"), 0, "{action}");
    }
}

#[test]
fn descendant_answers_keep_sender_sequence_ancestry_and_role_handoff_fences() {
    for elapsed in [0.0, 1.0] {
        let c = Case::new();
        c.task("sim/a", "review");
        c.task("unrelated", "ready");
        c.task_with(c.sub("sim/a/same-prefix", "unrelated"), "ready");
        c.task_with(c.sub("different-name", "sim/a"), "ready");
        let old = c.f.session("reviewer");
        let next = c.f.session("reviewer");
        c.f.store.take_role("sim/a", &old, "reviewer").unwrap();
        let earlier =
            c.f.store
                .say("different-name", &old, "before post", None)
                .unwrap();
        c.f.advance(elapsed);
        let post =
            c.f.store
                .say("sim/a", "owner", "old instruction", Some("@reviewer"))
                .unwrap();
        assert!(earlier.id < post.id);
        c.f.store.leave(&old).unwrap(); // Automatic departure does not answer the post.
        c.f.store.take_role("sim/a", &next, "reviewer").unwrap();
        c.f.store
            .say("different-name", &next, "replacement answer", None)
            .unwrap();
        c.f.store.revive(&old).unwrap();
        c.f.store
            .say("sim/a/same-prefix", &old, "unrelated answer", None)
            .unwrap();
        c.f.store.leave(&old).unwrap();
        c.f.store
            .say("different-name", "owner", "other sender", None)
            .unwrap();
        c.f.advance(31.0 * 60.0);
        let lines = c.digest().lines().join("\n");
        assert!(lines.contains(&format!("post #{}", post.id)), "{lines}");
        c.f.store.revive(&old).unwrap();
        c.f.store
            .say("different-name", &old, "actual recipient answer", None)
            .unwrap();
        c.f.store.leave(&old).unwrap();
        assert_eq!(c.digest().count("unanswered"), 0);
    }
}

/// A launch record no reader decodes, on the open task the post waits on: its start a NaN, which
/// the database holds and reads out as text, where a reader takes a number. The unanswered scan
/// reads that task's launch records and fails, while the board and the digest's other reads answer.
const UNREADABLE_LAUNCH: &str = "INSERT INTO task_session (id, slug, handle, role, \
    claude_session, worktree, branch, pane, model, started, closed, closed_why, restore) VALUES \
    ('[\"sim/a\", \"nobody\"]', 'sim/a', 'nobody', 'executor', '', '', '', '', '', 'NaN', 1.0, \
    '', '')";
const READABLE_LAUNCHES: &str = "DELETE FROM task_session WHERE id = '[\"sim/a\", \"nobody\"]'";

/// `sql` sent as it is on `f`'s board.
fn write(f: &crate::common::Fixture, sql: &str) {
    let client = f.other_database().into_postgresql();
    client.simple(sql).unwrap();
}

/// The scan reads the launches of the open tasks it asks about alone: one no reader decodes, on a
/// finished task, is never read, and the post stays in the digest.
#[test]
fn a_launch_on_a_task_the_scan_does_not_ask_about_is_never_read() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    c.task("sim/b", "done");
    c.f.store
        .say("sim/a", "owner", "continue", Some("@executor"))
        .unwrap();
    c.f.advance(31.0 * 60.0);
    let write = |sql: &str| write(&c.f, sql);
    write(&UNREADABLE_LAUNCH.replace("sim/a", "sim/b"));
    assert!(c.f.store.all_task_sessions().is_err());
    assert_eq!(slugs(&c.digest(), "unanswered"), ["sim/a"]);
    write(&READABLE_LAUNCHES.replace("sim/a", "sim/b"));
    assert!(c.f.store.all_task_sessions().is_ok());
}

#[test]
fn a_failed_scan_leaves_every_other_list_and_the_post_returns_with_the_scan() {
    let mut c = Case::new();
    c.task("sim/a", "in_progress");
    c.task("sim/b", "ready");
    c.task("sim/c", "review");
    c.landed.insert("sim/c".into());
    let post =
        c.f.store
            .say("sim/a", "owner", "continue", Some("@executor"))
            .unwrap();
    c.f.advance(31.0 * 60.0);
    let whole = c.digest();
    assert_eq!(slugs(&whole, "unanswered"), ["sim/a"]);
    let others = |d: &Digest| -> BTreeSet<(String, String)> {
        (d.entries().into_iter())
            .filter(|entry| entry.0 != "unanswered")
            .collect()
    };
    assert!(
        ["ready_unstaffed", "review_unreviewed", "landed_open"]
            .iter()
            .all(|name| whole.count(name) > 0),
        "{:?}",
        whole.lines()
    );

    let write = |sql: &str| write(&c.f, sql);
    write(UNREADABLE_LAUNCH);
    assert!(c.f.store.all_task_sessions().is_err());
    let skipped = c.digest();
    assert_eq!(skipped.count("unanswered"), 0);
    assert_eq!(skipped.entries(), others(&whole));
    assert_eq!(skipped.executing, whole.executing);
    assert_eq!(
        skipped.lines(),
        (whole.lines().into_iter())
            .map(|line| line.replace(" unanswered=1", ""))
            .filter(|line| !line.starts_with("  unanswered "))
            .collect::<Vec<_>>()
    );

    write(READABLE_LAUNCHES);
    let back = c.digest();
    assert_eq!(back.entries(), whole.entries());
    assert!(
        (back.lines().iter()).any(|line| line.contains(&format!("post #{} —", post.id))),
        "{:?}",
        back.lines()
    );
}
