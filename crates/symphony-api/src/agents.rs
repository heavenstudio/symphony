use crate::{AgentProbes, MachineWork};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_field_names, reason = "the field is the wire's name")]
pub struct Agents {
    #[serde(default)]
    pub machine_work: MachineWork,
    pub generated: f64,
    pub probes: AgentProbes,
    pub agents: Vec<Agent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSlots {
    pub open: usize,
    pub cap: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldTask {
    pub slug: String,
    pub role: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    pub recipe: String,
    pub phase: String,
    pub since: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentSaid {
    pub slug: String,
    pub ts: f64,
    pub text: String,
}
