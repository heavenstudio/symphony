use super::{Monitor, quiet_child};
use symphony_board::NewLaunch;

pub(super) fn launched_child(m: &Monitor) -> (String, String) {
    let (orch, child) = quiet_child(m);
    let launch = NewLaunch {
        claude_session: "sess-c".into(),
        worktree: "/wt/child".into(),
        branch: "sim/a".into(),
        pane: "t1".into(),
        ..NewLaunch::new("sim/a", &child, "executor")
    };
    m.board().record_launch(&launch).unwrap();
    (orch, child)
}
