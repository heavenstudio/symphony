//! The agents' activity as the machine's one Symphony server keeps it (`docs/workflow/AGENT_ACTIVITY.md`):
//! one [`AgentLine`] a line in the day files `symphony/agents/YYYY-MM-DD.jsonl` in the project's
//! git common dir, a line for each change in a session's health and, once an hour, an
//! [`AgentHour`] of what the hour held. The server writes them (`apps/symphony-server`,
//! `live/agent_activity.rs`) and the Reports page reads their hours, which join the hourly
//! roll-up's ([`crate::ReportHour`]) and the board's health hours ([`crate::HealthHour`]) on
//! [`AgentHour::start`], the hour's first second in all three.
//!
//! A line is an object whose `type` names which. It is read by that `type`, one struct at a time:
//! serde's buffered reading of a tagged enum cannot read a number under `serde_json`'s
//! `arbitrary_precision`, which the workspace builds it with, so [`AgentLine`] is written and never
//! read whole.

use serde::{Deserialize, Serialize};

/// The version of the lines' shape, in every line.
pub const AGENT_ACTIVITY_SCHEMA: u32 = 1;

/// One line of a day file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AgentLine {
    /// The server began watching: what happened between the line before and this one went unseen.
    Start(AgentStart),
    /// A session as the first snapshot after a start found it, which is no change.
    Seen(AgentSeen),
    /// A session whose health changed between two snapshots.
    Change(AgentChange),
    /// What an hour held.
    Hour(AgentHour),
}

impl AgentLine {
    /// The UTC day whose file holds it, `YYYY-MM-DD`: an hour's, its start's; any other, its
    /// time's.
    pub fn day(&self) -> String {
        crate::run_day(match self {
            AgentLine::Start(line) => line.at,
            AgentLine::Seen(line) => line.at,
            AgentLine::Change(line) => line.at,
            AgentLine::Hour(line) => line.start,
        })
    }
}

/// Every line names its writer: `pid`, the server's process, and `generation`, when it began
/// recording (its start line's `at`), so two servers of one hand-off are told apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentStart {
    pub schema: u32,
    /// When, by the server's clock, in seconds since the epoch.
    pub at: f64,
    pub pid: u32,
    pub generation: f64,
    /// The sessions on the board.
    pub sessions: usize,
    /// Of them, those that had left: no line says them.
    pub left: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSeen {
    pub schema: u32,
    pub at: f64,
    pub pid: u32,
    pub generation: f64,
    pub handle: String,
    pub kind: String,
    pub model: String,
    /// The task it holds a role on, and that role: of its executor, reviewer and orchestrator
    /// roles, one on an unfinished task first, then executing before reviewing before
    /// orchestrating, then the newest; none when it holds none.
    pub task: Option<String>,
    pub role: Option<String>,
    /// `live`, `stale`, `unknown` or `lost`: a session seen `left` is counted on the start line.
    pub health: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChange {
    pub schema: u32,
    pub at: f64,
    pub pid: u32,
    pub generation: f64,
    pub handle: String,
    pub kind: String,
    pub model: String,
    pub task: Option<String>,
    pub role: Option<String>,
    /// Its health in the snapshot before; none for a session new on the board.
    pub from: Option<String>,
    /// Its health in this one; none for a session gone from the board.
    pub to: Option<String>,
}

/// What one writer watched of an hour. An hour can have more than one, from two writers (a server
/// stopped and another started, a reload's hand-off) or one whose commit failed; no two cover the
/// same moment, and `docs/workflow/AGENT_ACTIVITY.md`, *Reading an hour*, says how they add up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentHour {
    pub schema: u32,
    /// When it was written.
    pub at: f64,
    pub pid: u32,
    pub generation: f64,
    /// The hour's start, a whole UTC hour in seconds since the epoch: the roll-up's `start`, on
    /// which the files join.
    pub start: f64,
    /// Where what it watched begins: the hour's start, or the first snapshot in it after a start.
    pub from: f64,
    /// Where it ends: the hour's end, or when the server stopped.
    pub to: f64,
    /// The seconds of it between two snapshots at most two minutes apart: what the counts below
    /// cover.
    pub observed_s: f64,
    /// The snapshots taken in it.
    pub samples: u64,
    /// By the session's kind and model.
    #[serde(default)]
    pub sessions: Vec<SessionHour>,
    /// Live sessions holding a role on a task in progress or in review, by that role and the
    /// session's model.
    #[serde(default)]
    pub running: Vec<RoleModel>,
    /// The same by role alone, each session once for each role it holds.
    #[serde(default)]
    pub running_roles: Vec<RoleLevel>,
    /// The same sessions, each once.
    #[serde(default)]
    pub running_agents: Level,
    /// Sessions holding such a role whose health is live, stale or unknown, not lost or left: an
    /// agent waiting on a long build makes no tool call and reads stale, but still holds its work.
    #[serde(default)]
    pub holding: Vec<RoleModel>,
    #[serde(default)]
    pub holding_roles: Vec<RoleLevel>,
    #[serde(default)]
    pub holding_agents: Level,
    /// Live sessions by their kind, whatever they hold.
    #[serde(default)]
    pub live: Vec<KindLevel>,
    /// The machine's load average over 1 and over 15 minutes, read at each snapshot; none when it
    /// could not be read.
    #[serde(default)]
    pub load_1m: Option<Gauge>,
    #[serde(default)]
    pub load_15m: Option<Gauge>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionHour {
    pub kind: String,
    pub model: String,
    /// Sessions whose row says they connected in the hour, up to `to`.
    pub started: u64,
    /// Changes to stale, and to lost, seen in it.
    pub went_stale: u64,
    pub went_lost: u64,
    /// Seconds sessions spent stale, and lost, within `observed_s`.
    pub stale_s: f64,
    pub lost_s: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleModel {
    pub role: String,
    pub model: String,
    pub peak: u64,
    pub mean: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleLevel {
    pub role: String,
    pub peak: u64,
    pub mean: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KindLevel {
    pub kind: String,
    pub peak: u64,
    pub mean: f64,
}

/// A count over the hour: the most at any time in it, and its mean over the time watched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub peak: u64,
    pub mean: f64,
}

/// A reading over the hour, as [`Level`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Gauge {
    pub peak: f64,
    pub mean: f64,
}
