//! A project's approval authority, asked by its host for an attestation when an operation needs
//! one (docs/WORKFLOW.md, "Guards"): the operation's snapshot and the principal taking it go in,
//! the authority's signed statement comes back, and the board verifies it as it verifies one
//! pushed to it, by the key the project commits for that authority, never by who handed it over.
//! The seam adds no trust: an answer the board cannot verify refuses the operation.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::execution::Principal;

/// The board's place of the task an operation is taken from, as the request expects it: the
/// digest of the workflow revision, its activation's generation, the task's revision and its
/// phase and state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Snapshot {
    pub workflow: String,
    pub activation_generation: u64,
    pub task_revision: u64,
    pub phase: String,
    pub state: String,
}

impl Snapshot {
    pub fn new(
        workflow: String,
        activation_generation: u64,
        task_revision: u64,
        phase: String,
        state: String,
    ) -> Snapshot {
        Snapshot {
            workflow,
            activation_generation,
            task_revision,
            phase,
            state,
        }
    }
}

/// One guard of one operation, what an attestation of it must name: the project's committed id,
/// the task, the edge taken and the guard, the place it is taken from, the input revisions it
/// binds and the principal taking it, as the host authenticated them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Operation {
    pub project: String,
    pub task: String,
    pub edge: String,
    pub guard: String,
    pub snapshot: Snapshot,
    pub input_revisions: BTreeMap<String, String>,
    pub principal: Principal,
}

impl Operation {
    pub fn new(
        project: String,
        task: String,
        edge: String,
        guard: String,
        snapshot: Snapshot,
        input_revisions: BTreeMap<String, String>,
        principal: Principal,
    ) -> Operation {
        Operation {
            project,
            task,
            edge,
            guard,
            snapshot,
            input_revisions,
            principal,
        }
    }
}

/// An authority's statement as it signed it: the bytes of its JSON attestation, exactly as
/// signed, and its Ed25519 signature over them in lowercase hex. The host passes both on
/// untouched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Signed {
    pub payload: String,
    pub signature: String,
}

impl Signed {
    pub fn new(payload: String, signature: String) -> Signed {
        Signed { payload, signature }
    }
}

/// A project's approval authority, answering for one adapter its workflow declares and its
/// `[[workflow_authorities]]` commits a key for. A host taking a guarded edge with no attestation
/// given asks the authority of each of its guards' adapters, then hands the board what it
/// answered, which the board verifies as a pushed one: its signature by the committed key, the
/// operation, principal and accepted generation it names, and its time. Nothing is kept: each
/// operation asks again.
///
/// An authority bounds its own work to `within`: the host waits that long and refuses the
/// operation as `guard_unavailable` when no answer came, as it does for an error or a panic. A
/// linked call that never returns holds its thread for the host's life.
pub trait Authority: Send + Sync {
    /// The adapter it answers for, as the workflow's guards name it.
    fn adapter(&self) -> &str;
    /// Its signed attestation of `operation` in the project at `root`, a pass or not, or why it
    /// cannot say.
    fn attest(
        &self,
        root: &Path,
        operation: &Operation,
        within: Duration,
    ) -> Result<Signed, String>;
}

#[cfg(test)]
#[path = "authority_tests.rs"]
mod tests;
