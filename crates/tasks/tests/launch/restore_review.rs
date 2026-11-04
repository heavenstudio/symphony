//! Review roles live on the parent, while their resumable instruction conversation lives on a child.

use super::*;

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
        ] {
            let w = World::new();
            let executor = w.f.session("worker");
            w.store().claim("sim/jump", &executor).unwrap();
            w.store().move_to("sim/jump", "review", &executor).unwrap();
            if parent != "sim/jump" {
                w.task(parent, None);
            }
            w.task("sim/jump/review", Some(parent));
            let s = w.review("sim/jump/review").row;
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
                close_pane(&w, &s);
            } else {
                w.edit_panes(|panes| panes.clear());
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
            let digest = tasks::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
            assert_eq!(digest.count("unanswered"), 1);
            assert!(digest.lines().iter().any(|line| line.contains(&command)));
            let mut warnings = Vec::new();
            let restored =
                restore_unanswered(w.store(), &w.herdr(), &w.machine(), &mut warnings).unwrap();
            assert_eq!(
                restored.len(),
                1,
                "{closed} {departed} {by_role}: {warnings:?}"
            );
            assert!(warnings.is_empty());
            let session = w.store().task_sessions(&s.slug).unwrap().remove(0);
            assert_eq!(session.claude_session, s.claude_session);
            assert!(session.closed.is_none());
            assert_eq!(w.calls_of(&["agent", "start"]).len(), 2);
            assert!(w.store().thread("sim/jump").unwrap().iter().any(|m| {
                m.sender == "board" && m.text.contains(&format!("post #{}", post.id))
            }));
            assert!(
                restore_unanswered(w.store(), &w.herdr(), &w.machine(), &mut warnings)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(w.calls_of(&["agent", "start"]).len(), 2);
        }
    }
}
