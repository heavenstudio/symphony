//! Resuming an existing Codex turn is not the same as starting a fresh conversation, and a resumed
//! conversation of either agent can open on a question.

use super::{close_terminal, launched, restored, told};
use crate::common::refused;
use crate::{FAKE, Hooked, World};
use serde_json::json;
use std::cell::Cell;
use std::fs;
use symphony_board::NewLaunch;
use symphony_board::launch::{CONFIRM_LOOKS, PROMPT_WAIT, restore};
use symphony_board::rows::TaskSessionRow;

fn closed_codex(w: &World) -> TaskSessionRow {
    let s = w.launch("sim/jump").row;
    let s = w.store().record_launch(&NewLaunch {
        claude_session: "codex:recorded-thread".into(),
        worktree: s.worktree,
        branch: s.branch,
        pane: s.pane,
        model: s.model,
        ..NewLaunch::new("sim/jump", &s.handle, &s.role)
    });
    let s = s.unwrap();
    close_terminal(w, &s);
    w.sleeps.borrow_mut().clear();
    s
}

fn native_queue(w: &mut World) {
    let bin = w.dir().join("native");
    fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink(FAKE, bin.join("codex")).unwrap();
    let path = format!("{}:{}", bin.display(), w.env.vars["PATH"]);
    w.set("PATH", &path);
}

#[test]
fn a_resumed_turn_that_becomes_idle_gets_exactly_its_restore_message() {
    let mut w = World::new();
    let s = closed_codex(&w);
    w.set("FAKE_HC_BUSY", "1");
    let looks = Cell::new(0);
    let hc = Hooked {
        after: Box::new(|args| {
            if args[0] == "get" {
                looks.set(looks.get() + 1);
                if looks.get() == CONFIRM_LOOKS - 1 {
                    w.edit_terminal(args[1], |t| {
                        t.insert("status".into(), json!("idle"));
                    });
                }
            }
        }),
        ..Hooked::new(&w)
    };
    let [again] = <[_; 1]>::try_from(
        restore(
            w.store(),
            &hc,
            &w.machine(),
            "sim/jump",
            Some(&s.handle),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(looks.get(), CONFIRM_LOOKS);
    assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; CONFIRM_LOOKS]);
    assert_eq!(w.sent(&again.row.pane), [told(&s.handle)]);
    assert!(w.calls_of(&["queue"]).is_empty());
    assert_eq!(again.row.closed, None);
    assert_eq!(w.live(), [again.row.pane]);
}

#[test]
fn a_resumed_turn_still_working_is_kept_and_its_native_queue_result_is_reported() {
    for fails in [false, true] {
        let mut w = World::new();
        let s = closed_codex(&w);
        native_queue(&mut w);
        w.set("FAKE_HC_BUSY", "1");
        if fails {
            w.set("FAKE_HC_FAIL", "queue");
        }
        let why = refused(restored(&w, "sim/jump", Some(&s.handle)));
        let [pane] = <[_; 1]>::try_from(w.live()).unwrap();
        let delivery = if fails {
            "restore message queue was not confirmed: codex queue exited exit status: 1; do not resend without checking the conversation"
        } else {
            "restore message queued for when it is idle; delivery is not yet confirmed"
        };
        assert_eq!(
            why,
            format!(
                "{} resumed in terminal {pane} and is still working; kept alive with its record reopened; {delivery}",
                s.handle
            )
        );
        assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; CONFIRM_LOOKS]);
        assert_eq!(w.terminal(&pane)["status"], "working");
        assert!(w.sent(&pane).is_empty());
        assert!(w.calls_of(&["kill", &pane]).is_empty());
        assert!(w.calls_of(&["rm", &pane]).is_empty());
        assert_eq!(
            w.calls_of(&["queue"]),
            [vec![
                "queue".to_string(),
                "--thread".into(),
                "recorded-thread".into(),
                "--message".into(),
                told(&s.handle),
            ]]
        );
        let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(record.pane, pane);
        assert_eq!(record.closed, None);
        assert_eq!(record.claude_session, s.claude_session);
        assert!(
            restored(&w, "sim/jump", Some(&s.handle))
                .unwrap()
                .is_empty()
        );
        assert_eq!(w.calls_of(&["queue"]).len(), 1, "no blind retry");
    }
}

#[test]
fn a_resumed_codex_blocked_on_a_person_is_still_reported_and_cleaned_up() {
    let mut w = World::new();
    let s = closed_codex(&w);
    w.set("FAKE_HC_BLOCKED", "1");
    let before = w.store().task_sessions("sim/jump").unwrap();
    let why = refused(restored(&w, "sim/jump", Some(&s.handle)));
    let removed = w.calls_of(&["rm"]);
    let pane = &removed.last().unwrap()[1];
    assert_ne!(pane, &s.pane);
    assert_eq!(
        why,
        format!(
            "the agent in terminal {pane} waits on a person before it took its first message: Quick safety check: Is this a project you created or one you trust?"
        )
    );
    assert!(w.live().is_empty());
    assert!(w.calls_of(&["send", pane]).is_empty());
    assert!(w.calls_of(&["queue"]).is_empty());
    assert!(w.calls_of(&["keys", pane]).is_empty());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), before);
}

#[test]
fn a_restore_blocked_on_a_dialog_hc_only_calls_open_says_what_the_terminal_showed() {
    let mut w = World::new();
    let s = launched(&w).row;
    close_terminal(&w, &s);
    // A question a resumed conversation can open with, which hc's status names only as a dialog.
    let dialog = w.dir().join("dialog.txt");
    let screen = "  This conversation is long and old.\n\n  Resume it whole or from a summary?\n\n\
                  ❯ 1. Resume from summary\n  2. Resume whole\n  3. Don't ask again\n";
    fs::write(&dialog, screen).unwrap();
    w.set("FAKE_HC_BLOCKED", "1");
    w.set("FAKE_HC_BLOCKED_SCREEN", dialog.to_str().unwrap());
    let before = w.store().task_sessions("sim/jump").unwrap();
    let why = refused(restored(&w, "sim/jump", Some(&s.handle)));
    let removed = w.calls_of(&["rm"]);
    let pane = &removed.last().unwrap()[1];
    // The terminal is closed with the failed restore, so the refusal is where the question stays.
    assert_eq!(
        why,
        format!(
            "the agent in terminal {pane} waits on a person before it took its first message: \
             dialog open; its terminal showed: This conversation is long and old. | Resume it \
             whole or from a summary? | ❯ 1. Resume from summary | 2. Resume whole | 3. Don't ask \
             again"
        )
    );
    assert!(w.live().is_empty());
    assert!(w.calls_of(&["keys", pane]).is_empty());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), before);
}
