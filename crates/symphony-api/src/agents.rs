//! Additive agent wire records preserve legacy state alongside optional machine-work evidence.

use crate::{AgentProbes, MachineWork};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[expect(clippy::struct_field_names, reason = "the field is the wire's name")]
pub struct Agents {
    #[serde(default)]
    pub machine_work: MachineWork,
    pub generated: f64,
    /// Written as the wire carries it, the panes probe under both its names.
    #[cfg_attr(
        feature = "typescript",
        ts(as = "crate::agent_probes::ProbesWire", inline)
    )]
    pub probes: AgentProbes,
    pub agents: Vec<Agent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[expect(clippy::struct_field_names, reason = "the field is the wire's name")]
pub struct Agent {
    pub handle: String,
    pub kind: String,
    pub model: String,
    pub parent: Option<String>,
    pub task: Option<String>,
    pub role: Option<String>,
    pub tasks: Vec<HeldTask>,
    pub worktree: String,
    pub branch: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub pane: Option<String>,
    #[serde(default)]
    pub callback: Option<String>,
    #[serde(default)]
    pub chat: bool,
    pub state: String,
    pub state_detail: Option<AgentRun>,
    pub health: String,
    pub last_tool_call: Option<f64>,
    pub last_message: Option<AgentSaid>,
    /// The open tasks it holds against the board's cap on an orchestrator's, when one is set.
    #[serde(default)]
    pub slots: Option<AgentSlots>,
    /// The web address its session gave (`tasks register --session-url`, `tasks callback
    /// --session-url`), when it gave one.
    #[serde(default)]
    pub session_url: Option<String>,
    /// It runs inside another session (a subagent), so it has no pane of its own.
    #[serde(default)]
    pub in_process: bool,
    /// Where its Go to session control goes, as the server resolved it ([`resolve_sessions`]):
    /// none when nothing is known.
    #[serde(default)]
    pub go: Option<SessionGo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentSlots {
    pub open: usize,
    pub cap: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HeldTask {
    pub slug: String,
    pub role: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentRun {
    pub recipe: String,
    pub phase: String,
    pub since: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentSaid {
    pub slug: String,
    pub ts: f64,
    pub text: String,
}

/// Where an agent's Go to session control goes (docs/TASKS.md, "Going to a session"): a session
/// the server knows a way into, by its web address, its pane on the server's machine, or both.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SessionGo {
    /// Whose session it reaches: the agent's own, or for a subagent with no way in of its own,
    /// the parent session that runs it.
    pub handle: String,
    /// That session's web address: the control links to it.
    #[serde(default)]
    pub url: Option<String>,
    /// The hc terminal that shows that session on the server's machine, offered only when the
    /// project addresses its panes (`[agents] panes = "hc"`).
    #[serde(default)]
    pub pane: Option<String>,
}

/// What a Go to session control does for the page that shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionControl<'a> {
    /// A button: the server's machine shows the pane in the Herdr app and brings it forward
    /// ([`AGENT_FOCUS_PATH`]).
    Focus(&'a str),
    /// An ordinary link to the session's web address.
    Link(&'a str),
    /// The pane's screen, read only ([`AGENT_SCREEN_PATH`]): what a page on another machine sees.
    Screen(&'a str),
}

/// Whether a page may link to `url` as a session's web address: an `http://` or `https://` URL
/// (in any case) naming a host, with no space or control character in it. The board checks an
/// address when it is set and again when it is read; whatever reaches a page is checked once
/// more here, where the link is made, so no `javascript:` or other scheme ever becomes one.
pub fn web_address(url: &str) -> bool {
    let after = |scheme: &str| {
        url.get(..scheme.len())
            .filter(|start| start.eq_ignore_ascii_case(scheme))
            .and_then(|_| url.get(scheme.len()..))
    };
    let Some(rest) = ["https://", "http://"].into_iter().find_map(after) else {
        return false;
    };
    !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && rest
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|host| !host.is_empty())
}

impl SessionGo {
    /// The control a page shows, `local` when it was asked from the server's own machine: a pane
    /// is focused there, else its web address is linked, else its screen is shown. An address
    /// that is no [`web_address`] is never linked: the session is gone to by its pane, or not at
    /// all.
    pub fn control(&self, local: bool) -> Option<SessionControl<'_>> {
        let url = self.url.as_deref().filter(|url| web_address(url));
        match (&self.pane, url) {
            (Some(pane), _) if local => Some(SessionControl::Focus(pane)),
            (_, Some(url)) => Some(SessionControl::Link(url)),
            (Some(pane), None) => Some(SessionControl::Screen(pane)),
            (None, None) => None,
        }
    }

    /// Whether it goes to another session than `handle`'s own: a subagent's parent's.
    pub fn via_parent(&self, handle: &str) -> bool {
        self.handle != handle
    }
}

/// How many parents an in-process agent's way in is looked for through, a subagent's subagent
/// among them, before the search stops.
const PARENTS: usize = 8;

/// Fill each agent's [`Agent::go`]: its own session's web address (one that is a
/// [`web_address`]) and, when `panes`, its pane; else, for an in-process agent, the first of its
/// parents, in turn, that has one. A parent that is not listed, or a loop of parents, ends the
/// search with nothing known.
pub fn resolve_sessions(agents: &mut [Agent], panes: bool) {
    let own = |agent: &Agent| {
        let pane = agent.pane.clone().filter(|p| panes && !p.is_empty());
        let url = agent.session_url.clone().filter(|u| web_address(u));
        (pane.is_some() || url.is_some()).then(|| SessionGo {
            handle: agent.handle.clone(),
            url,
            pane,
        })
    };
    let by_handle: std::collections::HashMap<&str, &Agent> =
        agents.iter().map(|a| (a.handle.as_str(), a)).collect();
    let resolved: Vec<Option<SessionGo>> = agents
        .iter()
        .map(|agent| {
            let mut at = agent;
            for _ in 0..=PARENTS {
                if let Some(go) = own(at) {
                    return Some(go);
                }
                if !at.in_process {
                    return None;
                }
                at = at
                    .parent
                    .as_deref()
                    .filter(|parent| *parent != at.handle)
                    .and_then(|parent| by_handle.get(parent).copied())?;
            }
            None
        })
        .collect();
    for (agent, go) in agents.iter_mut().zip(resolved) {
        agent.go = go;
    }
}

/// `POST {"handle"}`: the server's machine shows that agent's session in the Herdr app
/// ([`SessionControl::Focus`]); refused for a request from another machine. Answers
/// [`AgentFocus`].
pub const AGENT_FOCUS_PATH: &str = "/api/agent-focus";

/// `GET ?handle=`: the visible screen of that agent's session's pane ([`SessionControl::Screen`]).
/// Answers [`AgentScreen`].
pub const AGENT_SCREEN_PATH: &str = "/api/agent-screen";

/// What a focus did, as the page says it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentFocus {
    pub said: String,
}

/// A pane's visible screen, as it was read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AgentScreen {
    /// Whose session it shows: the agent's own, or its parent's.
    pub handle: String,
    pub pane: String,
    pub text: String,
    /// When it was read, in seconds since the epoch.
    pub read_at: f64,
}

#[cfg(test)]
mod tests;
