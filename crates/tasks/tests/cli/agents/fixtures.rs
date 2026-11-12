//! Fixtures for legacy agent wire and local host attribution.
use super::Case;
use crate::common::NOW;
use serde_json::{Value, json};
use symphony_board::NewSession;
use symphony_board::cli::agents::Run;

pub(super) const AGENT_KEYS: [&str; 22] = [
    "handle",
    "kind",
    "model",
    "parent",
    "task",
    "role",
    "tasks",
    "worktree",
    "branch",
    "agent",
    "session_id",
    "pane",
    "callback",
    "chat",
    "state",
    "state_detail",
    "health",
    "last_tool_call",
    "last_message",
    "slots",
    "session_url",
    "in_process",
];

pub(super) fn local_session(kind: &str) -> NewSession {
    NewSession {
        host: symphony_board::sys::hostname(),
        ..NewSession::new(kind)
    }
}

impl Case {
    /// A verify in `worktree`, in `phase`, by no session the slot names.
    pub(super) fn run_of(&self, phase: &str, worktree: &str, since: f64, recipe: &str) {
        self.add_run(Run {
            pid: 1,
            worktree: worktree.into(),
            recipe: recipe.into(),
            phase: phase.into(),
            since: Some(since),
            pool: "run".into(),
            handle: None,
        });
    }

    /// An observed terminal carries the named worker's conversation, not just its pane id.
    pub(super) fn pane(&self, handle: &str, id: &str, status: &str, extra: Value) {
        let mut pane = json!({
            "address": id, "agent": "claude", "status": status, "cwd": "/wt/sim-jump",
            "state": "live", "argv": ["claude", "--session-id", format!("sess-{handle}")],
        });
        for (key, value) in extra.as_object().unwrap() {
            pane[key] = value.clone();
        }
        self.panes
            .borrow_mut()
            .get_or_insert_with(Vec::new)
            .push(pane);
    }

    /// A run the slot records as `handle`'s.
    pub(super) fn tagged(&self, handle: &str, worktree: &str) {
        self.add_run(Run {
            pid: 1,
            worktree: worktree.into(),
            recipe: "verify-full".into(),
            phase: "holding".into(),
            since: Some(NOW - 5.0),
            pool: "run".into(),
            handle: Some(handle.into()),
        });
    }
}
