//! Strict decoding of the bounded inventory a project's machine-work reader writes (`[board]
//! machine_work_reader`), with trusted local identity attached,
//! and the board's own recorder's operations ([`super::recorded`]) beside it: one inventory of
//! both ([`Inventory::merge`]). Terminal metadata stays private; only the projection's atomic
//! native entry may consume it.

use super::{Kind, Operation, State};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BYTES: usize = 4 * 1024 * 1024 + 65_536;
pub(super) const MAX_ROWS: usize = 4096;

pub struct Inventory {
    pub(super) machine_id: Option<String>,
    pub(super) uncertain: bool,
    pub(super) operations: Vec<Operation>,
    pub(super) finished: Vec<Operation>,
    pub(super) revisions: BTreeMap<String, Vec<u8>>,
    /// Nonces the board's own recorder wrote ([`super::recorded`]). Their recorder bound their
    /// task when it began them: no receiver attribution rewrites it. Once finished and swept from
    /// the work folder, one never comes back, so a receiver may forget it ([`Projection`]).
    ///
    /// [`Projection`]: super::Projection
    pub(super) recorded: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u8,
    complete: bool,
    uncertain: bool,
    truncated: bool,
    records: Vec<Row>,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    Active,
    Finished,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Setup,
    Queued,
    Running,
    Finished,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Row {
    evidence: Evidence,
    operation_id: String,
    parent_id: Option<String>,
    timing_run_id: Option<String>,
    handle: Option<String>,
    checkout: String,
    recipe: String,
    kind: Kind,
    state: Phase,
    resource: String,
    reason: String,
    phase_since_epoch_ms: u64,
    owner_pid: u32,
    child_pid: Option<u32>,
    exit_code: Option<i32>,
}

fn nonce(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Inventory {
    /// The native reader already validates framing, raw codec, filename and lease evidence.
    /// Decode its whole response before publishing anything; truncated/malformed output fails closed.
    /// An explicit uncertain record read retains unknown occupancy while other validated rows survive.
    pub fn decode(bytes: &[u8], machine_id: Option<&str>) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES || machine_id.is_some_and(|id| !super::projection::identity(id))
        {
            return Err("native inventory size or trusted identity is invalid".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if value
            .get("records")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|rows| {
                rows.iter()
                    .any(|row| row.as_object().is_none_or(|row| row.len() != 15))
            })
        {
            return Err("native operation has missing or extra fields".into());
        }
        drop(value);
        // Decode the original bytes too: a Value alone would discard duplicate JSON keys.
        let input: Input = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if input.version != 1 || input.complete || input.truncated || input.records.len() > MAX_ROWS
        {
            return Err("native inventory is unsupported, truncated or exceeds its bound".into());
        }
        let mut result = Self {
            machine_id: machine_id.map(str::to_owned),
            uncertain: input.uncertain,
            operations: Vec::new(),
            finished: Vec::new(),
            revisions: BTreeMap::new(),
            recorded: BTreeSet::new(),
        };
        let mut nonces = BTreeSet::new();
        for row in input.records {
            // The reader emits one validated pathname per nonce, never conflicting versions.
            if !nonces.insert(row.operation_id.clone()) {
                return Err("native inventory repeats an operation nonce".into());
            }
            let terminal = row.evidence == Evidence::Finished
                && row.state == Phase::Finished
                && row.exit_code.is_some();
            let active = row.evidence == Evidence::Active
                && matches!(row.state, Phase::Setup | Phase::Queued | Phase::Running)
                && row.exit_code.is_none();
            if !(terminal || active)
                || !nonce(&row.operation_id)
                || row
                    .parent_id
                    .as_deref()
                    .is_some_and(|id| !nonce(id) || id == row.operation_id)
                || row
                    .handle
                    .as_deref()
                    .is_some_and(|id| !super::projection::identity(id))
                || row.timing_run_id.as_ref().is_some_and(|id| id.len() > 4096)
                || [&row.checkout, &row.recipe, &row.resource, &row.reason]
                    .iter()
                    .any(|text| text.len() > 4096)
                || row.owner_pid == 0
                || row.child_pid == Some(0)
                || row.kind == Kind::Reservation
            {
                return Err("native operation fields or completion evidence are invalid".into());
            }
            result.revisions.insert(
                row.operation_id.clone(),
                serde_json::to_vec(&row).map_err(|error| error.to_string())?,
            );
            let operation = Operation {
                machine_id: machine_id.map(str::to_owned),
                operation_id: Some(row.operation_id),
                parent_id: row.parent_id,
                handle: row.handle,
                task: None,
                checkout: row.checkout,
                recipe: row.recipe,
                kind: if row.state == Phase::Setup {
                    Kind::Setup
                } else {
                    row.kind
                },
                state: if row.state == Phase::Queued {
                    State::Queued
                } else {
                    State::Running
                },
                resource: row.resource,
                reason: row.reason,
                phase_since: Some(row.phase_since_epoch_ms as f64 / 1000.0),
            };
            if terminal {
                result.finished.push(operation);
            } else {
                result.operations.push(operation);
            }
        }
        Ok(result)
    }

    /// Canonical publication bytes, independent of JSON field order. Keys are native nonces;
    /// bytes only detect a new publication, never machine/operation identity or producer-clock age.
    pub fn revisions(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.revisions
    }

    /// The nonces of this read the board's own recorder wrote.
    pub fn recorded_ids(&self) -> &BTreeSet<String> {
        &self.recorded
    }

    /// One inventory of two sources read for the same machine: every row of each, uncertain when
    /// either is. A nonce both name is refused, as one source repeating it is.
    pub fn merge(mut self, other: Inventory) -> Result<Self, String> {
        if self.machine_id != other.machine_id {
            return Err("native inventories of different machines cannot merge".into());
        }
        for (id, bytes) in other.revisions {
            if self.revisions.insert(id, bytes).is_some() {
                return Err("native inventory repeats an operation nonce".into());
            }
        }
        if self.revisions.len() > MAX_ROWS {
            return Err("native inventory exceeds its bound".into());
        }
        self.uncertain |= other.uncertain;
        self.operations.extend(other.operations);
        self.finished.extend(other.finished);
        self.recorded.extend(other.recorded);
        Ok(self)
    }

    /// A trusted adapter may bind explicit task context once, before the operation's first sample.
    /// Apply the same retained binding to completion; current display roles never rewrite it. An
    /// operation the board's recorder wrote keeps the task it recorded, none included.
    pub fn attribute(&mut self, mut task: impl FnMut(&Operation) -> Option<String>) {
        let recorded = &self.recorded;
        for operation in self.operations.iter_mut().chain(&mut self.finished) {
            if operation
                .operation_id
                .as_ref()
                .is_some_and(|id| recorded.contains(id))
            {
                continue;
            }
            operation.task = task(operation);
        }
    }

    /// Without stable machine identity, keep native work visible as unassigned evidence.
    pub fn unassigned(self) -> Vec<Operation> {
        self.operations
            .into_iter()
            .map(|mut operation| {
                operation.machine_id = None;
                operation
            })
            .collect()
    }
}
