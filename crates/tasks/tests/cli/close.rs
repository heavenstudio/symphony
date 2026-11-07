//! `just tasks close SLUG HANDLE` on a private board, a throwaway repository and the fake hc: what
//! it says it closed, and a refusal's exit.

use crate::launch::{NO_PID, Terminals};
use serde_json::json;

#[test]
fn a_close_says_what_it_closed_with_its_restore_line_and_a_refusal_is_exit_1() {
    let mut p = Terminals::new("close");
    let orch = p.orchestrator();
    let given = p.dir.display().to_string();
    p.as_(
        &orch,
        &[
            "launch",
            "sim/jump",
            "--kind",
            "claude",
            "--worktree",
            &given,
        ],
    );
    let [s] = <[_; 1]>::try_from(p.board().task_sessions("sim/jump").unwrap()).unwrap();
    p.edit_terminals(|t| {
        t.insert("status".into(), json!("idle"));
        t.insert("pid".into(), json!(NO_PID));
    });
    // The caller's own terminal is refused.
    p.set("HC_TERMINAL_ID", &s.pane);
    let own = p.as_(&orch, &["close", "sim/jump", &s.handle]);
    assert_eq!(
        (own.code, own.out.as_str(), own.err),
        (
            1,
            "",
            format!(
                "✖ terminal {} is the one you run in: a session never closes its own terminal\n",
                s.pane
            )
        )
    );
    p.set("HC_TERMINAL_ID", "");
    let ran = p.as_(&orch, &["close", "sim/jump", &s.handle]);
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (
            0,
            format!(
                "closed pane {} of {} (closed by {orch}); restore: just tasks restore sim/jump {}\n",
                s.pane, s.handle, s.handle
            ),
            ""
        )
    );
    assert!(!p.addresses().contains(&s.pane));
    let again = p.as_(&orch, &["close", "sim/jump", &s.handle]);
    assert_eq!(
        (again.code, again.err),
        (
            1,
            format!(
                "✖ {} has no open session on sim/jump and runs in no terminal of this board\n",
                s.handle
            )
        )
    );
}

#[test]
fn a_close_names_its_two_arguments() {
    let p = Terminals::new("close-usage");
    let ran = p.run(&["close", "sim/jump"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err),
        (
            2,
            "",
            "usage: just tasks close [-h] [--as ACTOR] slug handle\njust tasks close: error: the \
             following arguments are required: handle\n"
                .to_string()
        )
    );
}
