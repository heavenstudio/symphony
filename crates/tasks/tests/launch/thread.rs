//! A Codex launch's thread id: the agent says it when it registers its callback, which may come
//! before the launch writes its record or after; the record keeps it either way, and a session that
//! closes before it ever arrived says so.

use crate::common::refused;
use crate::{Hooked, World};
use serde_json::json;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{close, launch, restore};

#[test]
fn a_codex_callback_before_or_after_the_launch_record_leaves_a_resumable_id() {
    for early in [true, false] {
        let w = World::new();
        // An early agent registers on its first message, before launch writes its record. A late
        // agent has not registered even when launch returns and its record is read back.
        let mut hc = Hooked::new(&w);
        hc.after = Box::new(|args| {
            if early && args[0] == "send" {
                let handle = args[2].rsplit(' ').next().unwrap();
                w.store()
                    .callback_in(handle, Some("codex:0abc-12"), false, false, Some(args[1]))
                    .unwrap();
            }
        });
        let s = launch(w.store(), &hc, &w.machine(), &w.ask("sim/jump"))
            .unwrap()
            .row;
        assert_eq!(
            w.store().task_sessions("sim/jump").unwrap(),
            std::slice::from_ref(&s)
        );
        if !early {
            assert_eq!(s.claude_session, "codex:");
            // Register through a separate store: the launcher's first result cannot supply the
            // thread that this later registration must persist for restore.
            w.f.other()
                .callback_in(
                    &s.handle,
                    Some("codex:0abc-12"),
                    false,
                    false,
                    Some(&s.pane),
                )
                .unwrap();
        }
        let [recorded] =
            <[_; 1]>::try_from(w.f.other().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(recorded.claude_session, "codex:0abc-12");
        let line = format!(
            "launched {} as executor: pane {}, agent session {}, worktree {}, branch sim/jump",
            s.handle, s.pane, s.claude_session, s.worktree
        );
        assert!(w.f.said("sim/jump").contains(&line), "{line}");
        // The recorded id must survive closure and callback retirement, so restore needs no
        // registration or launcher's cached result to resume the exact conversation.
        w.hc().close(&s.pane).unwrap();
        w.store()
            .close_task_session("sim/jump", &s.handle, "idle", "restore line")
            .unwrap();
        w.store()
            .callback_in(&s.handle, None, false, false, None)
            .unwrap();
        restore(&w.f.other(), &w.hc(), &w.machine(), "sim/jump", None, None).unwrap();
        let run = w.calls_of(&["run"]).pop().unwrap();
        assert_eq!(run[run.len() - 2..], ["resume", "0abc-12"]);
        assert_eq!(
            w.store().task_sessions("sim/jump").unwrap()[0].claude_session,
            "codex:0abc-12"
        );
    }
}

#[test]
fn a_codex_launch_whose_thread_never_arrives_says_so_when_its_terminal_closes() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    assert_eq!(s.claude_session, "codex:");
    w.edit_terminal(&s.pane, |t| {
        t.insert("status".into(), json!("idle"));
    });
    let m = w.machine();
    close(
        w.store(),
        &w.hc(),
        &*m.processes,
        &w.orch,
        "sim/jump",
        &s.handle,
    )
    .unwrap();
    assert_eq!(
        w.f.said("sim/jump").last().unwrap(),
        &format!(
            "closed the pane of {h} (closed by {}); restore: just tasks restore sim/jump {h}. Its \
             Codex thread id never arrived, so restore cannot resume its conversation: start it \
             again with just tasks launch sim/jump",
            w.orch,
            h = s.handle
        )
    );
    assert_eq!(
        w.store().task_sessions("sim/jump").unwrap()[0].claude_session,
        "codex:"
    );
    let again = restore(w.store(), &w.hc(), &w.machine(), "sim/jump", None, None);
    assert_eq!(
        refused(again),
        format!(
            "{} has no recorded conversation id: its conversation cannot be resumed (start it \
             again with `just tasks launch`)",
            s.handle
        )
    );
}
