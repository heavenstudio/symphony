//! A Codex launch's thread id: the agent says it when it registers its callback, which may come
//! before the launch writes its record or after; the record keeps it either way, and a session that
//! closes before it ever arrived says so.

use crate::common::refused;
use crate::{Hooked, World};
use serde_json::json;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{close, launch, restore};

#[test]
fn a_codex_callback_registered_before_the_launch_record_leaves_a_resumable_id() {
    let w = World::new();
    // The agent registers its callback from its terminal as soon as it has its first message, before
    // the launch writes its record.
    let mut early = Hooked::new(&w);
    early.after = Box::new(|args| {
        if args[0] == "send" {
            let handle = args[2].rsplit(' ').next().unwrap();
            w.store()
                .callback_in(handle, Some("codex:0abc-12"), false, false, Some(args[1]))
                .unwrap();
        }
    });
    let s = launch(w.store(), &early, &w.machine(), &w.ask("sim/jump"))
        .unwrap()
        .row;
    assert_eq!(s.claude_session, "codex:0abc-12");
    assert_eq!(
        w.store().task_sessions("sim/jump").unwrap(),
        std::slice::from_ref(&s)
    );
    let line = format!(
        "launched {} as executor: pane {}, agent session codex:0abc-12, worktree {}, branch sim/jump",
        s.handle, s.pane, s.worktree
    );
    assert!(w.f.said("sim/jump").contains(&line), "{line}");
    // Closed, it resumes that thread.
    w.hc().close(&s.pane).unwrap();
    w.store()
        .close_task_session("sim/jump", &s.handle, "idle", "restore line")
        .unwrap();
    restore(w.store(), &w.hc(), &w.machine(), "sim/jump", None, None).unwrap();
    let run = w.calls_of(&["run"]).pop().unwrap();
    assert_eq!(run[run.len() - 2..], ["resume", "0abc-12"]);
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
