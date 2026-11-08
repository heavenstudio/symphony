//! A sweep preserves the callback-provided Codex thread through an interrupted close.

use super::*;

/// A sweep that stops after it recorded its close, right before the kill.
pub(super) fn sweep_stopped_before_its_kill(w: &World) {
    let mut interrupted = Hooked::new(w);
    interrupted.instead = Box::new(|args| {
        if args[0] == "kill" {
            panic!("sweep stopped before the terminal closed");
        }
        None
    });
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        w.sweep_with(&interrupted, How::default())
    }));
    assert_eq!(
        stopped.unwrap_err().downcast_ref::<&str>(),
        Some(&"sweep stopped before the terminal closed")
    );
}

#[test]
fn an_interrupted_close_of_a_codex_agent_keeps_the_thread_its_callback_gave_and_its_one_line() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent {
        extra: json!({"agent": "codex", "argv": ["codex", "--no-daemon", "hi"]}),
        by: By::Label,
        ..Agent::named(&handle)
    });
    register_codex(&w, &handle, "0abc", Some(&terminal));
    w.sweep();
    let before = w.f.said("sim/jump").len();
    sweep_stopped_before_its_kill(&w);
    let recorded = w.store().sessions_of(&handle).unwrap();
    assert_eq!(recorded[0].claude_session, "codex:0abc");
    assert!(w.has_terminal(&terminal));
    assert_eq!(terminals(&w.sweep()), [terminal.as_str()]);
    assert_eq!(w.store().sessions_of(&handle).unwrap(), recorded);
    assert_eq!(w.f.said("sim/jump").len(), before + 1);
}
