//! Unanswered-post recovery against private hc terminals.

use super::*;
use crate::Hooked;
use std::cell::Cell;
use symphony_board::launch::restore_unanswered;

#[test]
fn recovery_blocked_by_a_close_keeps_its_attempt_reserved_and_allows_manual_restore() {
    let w = World::new();
    let session = launched(&w).row;
    w.store().claim(&session.slug, &session.handle).unwrap();
    close_terminal(&w, &session);
    let post = w
        .store()
        .say(&session.slug, "owner", "continue", Some("@executor"))
        .unwrap();
    w.f.advance(31.0 * 60.0);
    let hold = w.store().take_session(&session.handle, "a close").unwrap();
    let mut warnings = Vec::new();
    assert!(
        restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings)
            .unwrap()
            .is_empty()
    );
    let warning = String::from_utf8(warnings).unwrap();
    assert_eq!(warning.lines().count(), 1, "{warning}");
    assert!(warning.contains(&format!("post #{}", post.id)), "{warning}");
    assert!(
        warning.contains(&format!("a close of {} is under way", session.handle)),
        "{warning}"
    );
    assert!(
        warning.contains(&format!(
            "just tasks restore {} {}",
            session.slug, session.handle
        )),
        "{warning}"
    );
    assert_eq!(w.calls_of(&["run"]).len(), 1);
    assert!(
        w.store().task_sessions(&session.slug).unwrap()[0]
            .closed
            .is_some()
    );

    w.store().release_session(&session.handle, hold).unwrap();
    let mut warnings = Vec::new();
    assert!(
        restore_unanswered(&w.f.other(), &w.hc(), &w.machine(), &mut warnings)
            .unwrap()
            .is_empty(),
        "the failed automatic attempt remains reserved after the close releases its hold"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(w.calls_of(&["run"]).len(), 1);
    let digest = symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
    assert_eq!(digest.count("unanswered"), 1);

    let resumed = restored(&w, &session.slug, Some(&session.handle)).unwrap();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].row.claude_session, session.claude_session);
    assert!(resumed[0].row.closed.is_none());
    assert_eq!(w.calls_of(&["run"]).len(), 2);
}

#[test]
fn recovery_resumes_once_per_post_for_closed_or_missing_open_conversations() {
    for closed in [false, true] {
        let w = World::new();
        let s = launched(&w).row;
        w.store().claim(&s.slug, &s.handle).unwrap();
        if closed {
            close_terminal(&w, &s);
        } else {
            w.edit_terminals(|panes| panes.clear());
        }
        let post = w
            .store()
            .say(&s.slug, "owner", "reconcile", Some("@executor"))
            .unwrap();
        let mut warnings = Vec::new();
        let recover = |store: &symphony_board::Store, warnings: &mut Vec<u8>| {
            restore_unanswered(store, &w.hc(), &w.machine(), warnings).unwrap()
        };
        w.f.advance(29.0 * 60.0);
        assert!(recover(w.store(), &mut warnings).is_empty());
        w.f.advance(2.0 * 60.0);
        let lines = recover(w.store(), &mut warnings);
        assert_eq!(lines.len(), 1, "{warnings:?}");
        assert!(lines[0].contains(&format!("post #{}", post.id)));
        assert_eq!(w.calls_of(&["run"]).len(), 2);
        let current = w.store().task_sessions(&s.slug).unwrap().remove(0);
        assert_eq!(current.claude_session, s.claude_session);
        close_terminal(&w, &current);
        assert!(
            recover(&w.f.other(), &mut warnings).is_empty(),
            "durable once-per-post claim"
        );
        w.store().say(&s.slug, &s.handle, "answered", None).unwrap();
        assert!(recover(w.store(), &mut warnings).is_empty());
        let next = w
            .store()
            .say(&s.slug, "owner", "new request", Some(&s.handle))
            .unwrap();
        w.f.advance(31.0 * 60.0);
        assert_eq!(recover(w.store(), &mut warnings).len(), 1);
        assert_eq!(w.calls_of(&["run"]).len(), 3);
        let recorded: Vec<_> = w
            .store()
            .thread(&s.slug)
            .unwrap()
            .into_iter()
            .filter(|p| p.sender == "board" && p.text.starts_with("restored "))
            .collect();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[1].text.contains(&format!("post #{}", next.id)));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn a_departed_executor_replacement_resumes_without_reviving_its_predecessor() {
    for before_departure in [false, true] {
        let w = World::new();
        let a = launched(&w).row;
        w.store().claim(&a.slug, &a.handle).unwrap();
        w.store()
            .release(&a.slug, &a.handle, Some("executor"))
            .unwrap();
        close_terminal(&w, &a);
        let b = w
            .try_launch(&Ask {
                kind: Some("claude".into()),
                worktree: Some(a.worktree.clone().into()),
                ..w.ask(&a.slug)
            })
            .unwrap()
            .row;
        w.store().claim(&b.slug, &b.handle).unwrap();
        w.store().move_to(&b.slug, "review", &b.handle).unwrap();
        assert_eq!(a.worktree, b.worktree);
        assert_ne!(a.handle, b.handle);
        if !before_departure {
            w.store().leave(&b.handle).unwrap();
        }
        let post = w
            .store()
            .say(&b.slug, "owner", "finish the fix", Some("@executor"))
            .unwrap();
        if before_departure {
            w.store().leave(&b.handle).unwrap();
        }
        close_terminal(&w, &b);
        assert!(
            w.store()
                .get(&b.slug)
                .unwrap()
                .holders("executor")
                .is_empty()
        );
        assert_eq!(w.store().task_sessions(&b.slug).unwrap().len(), 2);
        w.f.advance(31.0 * 60.0);
        let digest =
            symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        let lines = digest.lines().join("\n");
        assert_eq!(digest.count("unanswered"), 1, "{lines}");
        assert!(
            lines.contains(&format!("just tasks restore {} {}", b.slug, b.handle)),
            "{lines}"
        );
        assert!(!lines.contains(&a.handle), "{lines}");
        let mut warnings = Vec::new();
        let recovered =
            restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings).unwrap();
        assert_eq!(recovered.len(), 1, "{recovered:?} {warnings:?}");
        assert!(recovered[0].contains(&b.handle));
        assert!(recovered[0].contains(&format!("post #{}", post.id)));
        assert_eq!(w.calls_of(&["run"]).len(), 3);
        let sessions = w.store().task_sessions(&b.slug).unwrap();
        let old = sessions.iter().find(|s| s.handle == a.handle).unwrap();
        let resumed = sessions.iter().find(|s| s.handle == b.handle).unwrap();
        assert!(old.closed.is_some());
        assert!(resumed.closed.is_none());
        assert_eq!(resumed.claude_session, b.claude_session);
        close_terminal(&w, resumed);
        assert!(
            restore_unanswered(&w.f.other(), &w.hc(), &w.machine(), &mut warnings)
                .unwrap()
                .is_empty()
        );
        assert_eq!(w.calls_of(&["run"]).len(), 3);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn recovery_leaves_answered_present_and_unrecorded_conversations_alone() {
    for case in [
        "answered",
        "status-answer",
        "present",
        "unrecorded",
        "native",
        "native-open",
        "unknown-open",
    ] {
        let w = World::new();
        let s = launched(&w).row;
        w.store().claim(&s.slug, &s.handle).unwrap();
        let post = w
            .store()
            .say(&s.slug, "owner", "reconcile", Some(&s.handle))
            .unwrap();
        if case == "answered" {
            w.store().say(&s.slug, &s.handle, "answer", None).unwrap();
        }
        if case == "status-answer" {
            w.store().move_to(&s.slug, "blocked", &s.handle).unwrap();
        }
        if case.ends_with("-open") {
            w.edit_terminals(|panes| panes.clear());
        } else if case != "present" {
            close_terminal(&w, &s);
        }
        if case == "unrecorded" {
            w.store()
                .record_launch(&NewLaunch::new(&s.slug, &s.handle, &s.role))
                .unwrap();
        }
        w.f.advance(31.0 * 60.0);
        if case.starts_with("native") || case == "unknown-open" {
            w.store().client().query(
                "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:alive-thread', host = 'this-host', confirmed_until = $until, lease = $until RETURN NONE;",
                serde_json::json!({"handle":s.handle, "until":w.f.now()+if case == "unknown-open" { -1.0 } else { 390.0 }}),
            ).unwrap();
        }
        let mut warnings = Vec::new();
        assert!(
            restore_unanswered(w.store(), &w.hc(), &w.machine(), &mut warnings)
                .unwrap()
                .is_empty(),
            "{case}: {warnings:?}"
        );
        assert_eq!(w.calls_of(&["run"]).len(), 1, "{case}: post #{}", post.id);
        assert!(warnings.is_empty());
        let digest =
            symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        if case == "unrecorded" {
            assert!(
                digest
                    .lines()
                    .iter()
                    .any(|line| line.contains("just tasks launch sim/jump --role executor"))
            );
        }
        if case.ends_with("-open") {
            assert_eq!(digest.count("unanswered"), 0, "{case}");
        }
    }
}

#[test]
fn unanswered_recovery_does_not_restart_a_reviewer_who_answered_on_its_instruction() {
    for late in [false, true] {
        let w = World::new();
        let executor = w.f.session("worker");
        w.store().claim("sim/jump", &executor).unwrap();
        w.store().move_to("sim/jump", "review", &executor).unwrap();
        w.task("instructions/review", Some("sim/jump"));
        let reviewer = instruction_reviewer(&w, "instructions/review");
        w.store().claim(&reviewer.slug, &reviewer.handle).unwrap();
        close_terminal(&w, &reviewer);
        w.store()
            .say(
                "sim/jump",
                "owner",
                "finish the instruction",
                Some(&reviewer.handle),
            )
            .unwrap();
        let answered = Cell::new(false);
        let answer = || {
            w.store()
                .say(&reviewer.slug, &reviewer.handle, "review complete", None)
                .unwrap();
            answered.set(true);
        };
        if !late {
            answer();
        }
        w.f.advance(31.0 * 60.0);
        let mut hc = Hooked::new(&w);
        hc.after = Box::new(|args| {
            if late && args.first() == Some(&"ls") && !answered.get() {
                answer();
            }
        });
        let mut warnings = Vec::new();
        assert!(
            restore_unanswered(w.store(), &hc, &w.machine(), &mut warnings)
                .unwrap()
                .is_empty()
        );
        assert!(
            answered.get(),
            "late answer reached the inventory/recheck boundary"
        );
        assert!(w.calls_of(&["run"]).is_empty());
        assert!(
            w.store().task_sessions(&reviewer.slug).unwrap()[0]
                .closed
                .is_some()
        );
        assert!(warnings.is_empty());
        w.f.advance(31.0 * 60.0);
        let digest =
            symphony_board::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        assert_eq!(digest.count("unanswered"), 0);
    }
}
