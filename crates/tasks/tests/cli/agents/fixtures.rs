//! Fixtures for legacy agent wire and local host attribution.
use symphony_board::NewSession;

pub(super) const AGENT_KEYS: [&str; 20] = [
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
];

pub(super) fn local_session(kind: &str) -> NewSession {
    NewSession {
        host: symphony_board::sys::hostname(),
        ..NewSession::new(kind)
    }
}
