//! The machine's agent count over a made-up agents view and terminal list, and what the limit
//! says of it.

use super::*;
use serde_json::{Value, json};

fn terminal(value: Value) -> Object {
    value.as_object().unwrap().clone()
}

/// An agent's terminal `address` named `name`, hc calling it `app`.
fn agent_terminal(address: &str, name: &str, app: &str) -> Object {
    terminal(json!({"address": address, "name": name, "status": {"app": app, "state": "idle"}}))
}

/// `handle`, a live `kind` session with no agent terminal of its own and, with `pane`, that pane.
fn session(handle: &str, kind: &str, pane: Option<&str>) -> Agent {
    Agent {
        handle: handle.into(),
        kind: kind.into(),
        model: String::new(),
        parent: None,
        task: None,
        role: None,
        tasks: Vec::new(),
        following: 0,
        worktree: String::new(),
        branch: String::new(),
        agent: None,
        session_id: None,
        pane: pane.map(str::to_string),
        callback: None,
        chat: false,
        state: "working",
        detail: None,
        health: Health::Live,
        last_tool_call: None,
        last_message: None,
        slots: None,
        session_url: None,
        in_process: false,
        tokens: None,
    }
}

#[test]
fn nothing_running_counts_none() {
    assert_eq!(counted(&[], &[]), 0);
}

#[test]
fn each_terminal_running_an_agent_counts_and_a_shell_does_not() {
    let shell = terminal(json!({"address": "t3", "name": "zsh", "status": "live"}));
    let by_foreground =
        terminal(json!({"address": "t4", "name": "x", "foreground": {"name": "claude"}}));
    let terminals = [
        agent_terminal("t1", "amber-heron", "codex"),
        agent_terminal("t2", "coral-hare", "claude"),
        shell,
        by_foreground,
    ];
    assert_eq!(counted(&[], &terminals), 3);
}

#[test]
fn a_live_session_with_no_terminal_counts_once_and_one_matched_to_a_terminal_is_not_counted_again()
{
    let terminals = [
        agent_terminal("t1", "amber-heron", "codex"),
        agent_terminal("t2", "x", "claude"),
    ];
    let agents = [
        // Matched by name to t1, by pane to t2: already counted as their terminals.
        session("amber-heron", "worker", None),
        session("coral-hare", "reviewer", Some("t2")),
        // Running outside hc (a subagent, a session started by hand): counted.
        session("ochre-owl", "worker", None),
        session("slate-falcon", "orchestrator", Some("w1:p9")),
    ];
    assert_eq!(counted(&agents, &terminals), 4);
}

#[test]
fn a_session_that_is_not_live_has_an_agent_terminal_or_is_no_agent_is_not_counted() {
    let mut lost = session("ochre-owl", "worker", None);
    lost.health = Health::Lost;
    let mut stale = session("amber-heron", "worker", None);
    stale.health = Health::Stale;
    let mut in_terminal = session("coral-hare", "reviewer", Some("t9"));
    in_terminal.agent = Some("codex".into());
    let owner = session("owner", "owner", None);
    let chat = session("chat-doc", "chat", None);
    assert_eq!(counted(&[lost, stale, in_terminal, owner, chat], &[]), 0);
}

#[test]
fn no_limit_refuses_nothing_whatever_runs() {
    assert_eq!(over(Ok(40), None, "symphony.toml"), None);
    let unknown = Err(Error::Refused("hc cannot be asked".into()));
    assert_eq!(over(unknown, None, "symphony.toml"), None);
}

#[test]
fn a_launch_is_refused_at_the_limit_and_allowed_under_it() {
    assert_eq!(over(Ok(2), Some(3), "/repo/symphony.toml"), None);
    assert_eq!(
        over(Ok(3), Some(3), "/repo/symphony.toml").unwrap(),
        "this machine runs 3 agents, at its limit of 3 ([limits] machine_agents in \
         /repo/symphony.toml)"
    );
    assert_eq!(
        over(Ok(5), Some(3), "/repo/symphony.toml").unwrap(),
        "this machine runs 5 agents, at its limit of 3 ([limits] machine_agents in \
         /repo/symphony.toml)"
    );
}

#[test]
fn next_warns_only_past_the_limit_as_its_asker_is_counted() {
    assert_eq!(past(Ok(9), None, "/repo/symphony.toml"), None);
    assert_eq!(past(Ok(2), Some(3), "/repo/symphony.toml"), None);
    assert_eq!(past(Ok(3), Some(3), "/repo/symphony.toml"), None);
    assert_eq!(
        past(Ok(4), Some(3), "/repo/symphony.toml").unwrap(),
        "this machine runs 4 agents, past its limit of 3 ([limits] machine_agents in \
         /repo/symphony.toml): no more start until some finish"
    );
    let unknown = Err(Error::Refused("hc cannot be asked".into()));
    assert_eq!(
        past(unknown, Some(3), "/repo/symphony.toml").unwrap(),
        "hc cannot be asked: [limits] machine_agents in /repo/symphony.toml holds it to 3"
    );
}

#[test]
fn a_count_that_cannot_be_made_is_refused_under_a_limit_and_says_why() {
    let unknown = Err(Error::Refused("hc cannot be asked".into()));
    assert_eq!(
        over(unknown, Some(3), "/repo/symphony.toml").unwrap(),
        "hc cannot be asked: [limits] machine_agents in /repo/symphony.toml holds it to 3"
    );
}
