//! Review roles live on the parent, while their resumable instruction conversation lives on a child.

use super::*;
use symphony_board::launch::restore_unanswered;

#[test]
fn a_departed_reviewer_without_a_launch_is_flagged_but_never_started_automatically() {
    for by_role in [false, true] {
        let w = World::new();
        let executor = w.f.session("worker");
        w.store().claim("sim/jump", &executor).unwrap();
        w.store().move_to("sim/jump", "review", &executor).unwrap();
        let reviewer = w.f.session("reviewer");
        w.store()
            .take_role("sim/jump", &reviewer, "reviewer")
            .unwrap();
        let post = w
            .store()
            .say(
                "sim/jump",
                "owner",
                "finish review",
                Some(if by_role { "@reviewer" } else { &reviewer }),
            )
            .unwrap();
        w.store().leave(&reviewer).unwrap();
        assert!(w.store().task_sessions("sim/jump").unwrap().is_empty());
        assert!(
            w.store()
                .get("sim/jump")
                .unwrap()
                .holders("reviewer")
                .is_empty()
        );
        w.f.advance(29.0 * 60.0);
        let digest =
            || symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        assert_eq!(digest().count("unanswered"), 0);
        w.f.advance(2.0 * 60.0);
        let d = digest();
        let lines = d.lines().join("\n");
        assert_eq!(d.count("unanswered"), 1, "{lines}");
        assert!(lines.contains(&format!("post #{}", post.id)), "{lines}");
        assert!(
            lines.contains("just tasks launch sim/jump --role reviewer"),
            "{lines}"
        );
        let mut warnings = Vec::new();
        assert!(
            restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings)
                .unwrap()
                .is_empty()
        );
        assert_eq!(w.calls_of(&["run"]).len(), 0);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn a_departed_replacement_keeps_its_role_posts_without_reviving_its_predecessor() {
    for parent in ["sim/jump", "sim/other"] {
        for before_departure in [false, true] {
            for another_reviewer in [false, true] {
                let w = World::new();
                let executor = w.f.session("worker");
                w.store().claim("sim/jump", &executor).unwrap();
                w.store().move_to("sim/jump", "review", &executor).unwrap();
                if parent != "sim/jump" {
                    w.task(parent, None);
                }
                w.task("sim/jump/review-a", Some(parent));
                let a = instruction_reviewer(&w, "sim/jump/review-a");
                if parent != "sim/jump" {
                    w.store()
                        .take_role("sim/jump", &a.handle, "reviewer")
                        .unwrap();
                }
                w.store()
                    .release("sim/jump", &a.handle, Some("reviewer"))
                    .unwrap();
                close_terminal(&w, &a);
                w.task("sim/jump/review-b", Some(parent));
                let b = instruction_reviewer(&w, "sim/jump/review-b");
                if parent != "sim/jump" {
                    w.store()
                        .take_role("sim/jump", &b.handle, "reviewer")
                        .unwrap();
                }
                if !before_departure {
                    w.store().leave(&b.handle).unwrap();
                }
                let post = w
                    .store()
                    .say("sim/jump", "owner", "finish review", Some("@reviewer"))
                    .unwrap();
                if before_departure {
                    w.store().leave(&b.handle).unwrap();
                }
                close_terminal(&w, &b);
                if another_reviewer {
                    w.task("sim/jump/review-c", Some(parent));
                    let c = instruction_reviewer(&w, "sim/jump/review-c");
                    if parent != "sim/jump" {
                        w.store()
                            .take_role("sim/jump", &c.handle, "reviewer")
                            .unwrap();
                    }
                }
                w.f.advance(31.0 * 60.0);
                let digest =
                    symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[])
                        .unwrap();
                let lines = digest.lines().join("\n");
                assert_eq!(digest.count("unanswered"), 1, "{lines}");
                assert!(
                    lines.contains(&format!("just tasks restore {} {}", b.slug, b.handle)),
                    "{lines}"
                );
                assert!(!lines.contains(&a.handle), "{lines}");
                let starts = w.calls_of(&["run"]).len();
                let mut warnings = Vec::new();
                let restored =
                    restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings).unwrap();
                assert_eq!(restored.len(), 1, "{restored:?} {warnings:?}");
                assert!(restored[0].contains(&b.handle), "{restored:?}");
                assert!(restored[0].contains(&format!("post #{}", post.id)));
                assert_eq!(w.calls_of(&["run"]).len(), starts + 1);
                assert!(
                    w.store().task_sessions(&a.slug).unwrap()[0]
                        .closed
                        .is_some()
                );
                let resumed = &w.store().task_sessions(&b.slug).unwrap()[0];
                assert!(resumed.closed.is_none());
                assert_eq!(resumed.claude_session, b.claude_session);
                assert!(warnings.is_empty(), "{warnings:?}");
                assert!(
                    restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings)
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn an_unanswered_review_post_resumes_its_instruction_conversation() {
    for closed in [false, true] {
        for (departed, by_role, parent) in [
            (false, false, "sim/jump"),
            (false, true, "sim/jump"),
            (true, false, "sim/jump"),
            (true, true, "sim/jump"),
            (false, false, "sim/other"),
            (false, true, "sim/other"),
            (true, false, "sim/other"),
            (true, true, "sim/other"),
        ] {
            let w = World::new();
            let executor = w.f.session("worker");
            w.store().claim("sim/jump", &executor).unwrap();
            w.store().move_to("sim/jump", "review", &executor).unwrap();
            if parent != "sim/jump" {
                w.task(parent, None);
            }
            w.task("sim/jump/review", Some(parent));
            let s = instruction_reviewer(&w, "sim/jump/review");
            if parent != "sim/jump" {
                w.store()
                    .take_role("sim/jump", &s.handle, "reviewer")
                    .unwrap();
            }
            w.store().claim(&s.slug, &s.handle).unwrap();
            if departed {
                w.store().leave(&s.handle).unwrap();
                if !by_role {
                    let other = w.f.session("reviewer");
                    w.store().take_role("sim/jump", &other, "reviewer").unwrap();
                }
            }
            if closed {
                close_terminal(&w, &s);
            } else {
                w.edit_terminals(|panes| panes.clear());
            }
            let post = w
                .store()
                .say(
                    "sim/jump",
                    "owner",
                    "finish the review",
                    Some(if by_role { "@reviewer" } else { &s.handle }),
                )
                .unwrap();
            w.f.advance(31.0 * 60.0);
            let command = format!("just tasks restore {} {}", s.slug, s.handle);
            let digest =
                symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
            assert_eq!(digest.count("unanswered"), 1);
            assert!(digest.lines().iter().any(|line| line.contains(&command)));
            let mut warnings = Vec::new();
            let restored =
                restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings).unwrap();
            assert_eq!(
                restored.len(),
                1,
                "{closed} {departed} {by_role}: {warnings:?}"
            );
            assert!(warnings.is_empty());
            let session = w.store().task_sessions(&s.slug).unwrap().remove(0);
            assert_eq!(session.claude_session, s.claude_session);
            assert!(session.closed.is_none());
            assert_eq!(w.calls_of(&["run"]).len(), 1);
            assert!(w.store().thread("sim/jump").unwrap().iter().any(|m| {
                m.sender == "board" && m.text.contains(&format!("post #{}", post.id))
            }));
            assert!(
                restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(w.calls_of(&["run"]).len(), 1);
        }
    }
}
