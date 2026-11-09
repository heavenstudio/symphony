//! Reporting only: receiver-clock evidence, separate from admission and task lifecycle.
//!
//! Native publication/transport supply samples. Reading a stored sample again must keep its
//! original receiver receipt; neither a producer's wall clock nor a board read renews freshness.

use serde::{Deserialize, Serialize};

mod projection;
pub use projection::{Projection, Rejected, Sample};
mod attribution;
pub(crate) use attribution::attribute;
pub(crate) use attribution::json;

/// One receiver read and its positively local host label. The label fences fallback attribution;
/// it is not a stable machine identity and never supplies capacity evidence.
pub struct Evidence {
    pub local_host: String,
    pub snapshot: Snapshot,
}

/// A missing publisher is unknown, including when no operations were observed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    #[default]
    Unknown,
    Fresh,
    Stale,
    Disconnected,
    Incomplete,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Build,
    Test,
    Command,
    Setup,
    /// A slot admission, disk room, or build lock; never proof of execution.
    Reservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// None for legacy local locks without a stable machine identity.
    pub machine_id: Option<String>,
    /// Stable across queued/running transitions. Wrappers and children of the *same* operation
    /// repeat this id; independent children have distinct ids, even under one parent.
    /// None for legacy locks without an operation nonce; PID/path are not operation identities.
    pub operation_id: Option<String>,
    pub parent_id: Option<String>,
    pub handle: Option<String>,
    pub task: Option<String>,
    pub checkout: String,
    pub recipe: String,
    pub kind: Kind,
    pub state: State,
    pub resource: String,
    pub reason: String,
    /// Source timestamp for display only; never used to decide age or freshness.
    pub phase_since: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub running_builds: usize,
    pub queued_builds: usize,
    pub running_tests: usize,
    pub queued_tests: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub machine_id: String,
    pub label: String,
    pub producer_id: String,
    pub sequence: u64,
    pub age_ms: Option<u64>,
    pub freshness: Freshness,
    pub complete: bool,
    pub reason: String,
    /// None means unknown occupancy, never zero/free. Reservations remain in operations.
    pub counts: Option<Counts>,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Shared transport coverage. Local slot probes alone cannot establish this as fresh.
    pub freshness: Freshness,
    pub machines: Vec<Machine>,
    /// Evidence without stable machine ownership, retained without capacity claims.
    pub unassigned: Vec<Operation>,
}

#[cfg(test)]
mod tests;
