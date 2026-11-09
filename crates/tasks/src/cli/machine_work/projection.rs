use super::{Counts, Freshness, Kind, Machine, Operation, Snapshot, State};
use std::collections::BTreeMap;

/// A complete inventory from one authenticated machine publisher. The transport assigns the
/// generation on a publisher restart; a producer cannot retire another producer by its own clock.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Sample {
    pub machine_id: String,
    pub label: String,
    pub producer_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub complete: bool,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejected {
    Limit,
    Identity,
    OutOfOrder,
    Ownership,
    ConflictingOperation,
    ReceiverClock,
}

struct Accepted {
    sample: Sample,
    receipt_ms: u64,
    connected: bool,
    conflict: bool,
}

/// Bounded receiver-side inventory. Limits bound reports, not admission or execution.
pub struct Projection {
    machines: BTreeMap<String, Accepted>,
    max_machines: usize,
    max_operations: usize,
    ttl_ms: u64,
}

impl Projection {
    pub fn new(max_machines: usize, max_operations: usize, ttl_ms: u64) -> Self {
        Self {
            machines: BTreeMap::new(),
            max_machines,
            max_operations,
            ttl_ms,
        }
    }

    /// Called only on new transport observations, with the original receiver-assigned receipt.
    /// Duplicate/older samples never renew a receipt. Partial inventories retain missing work;
    /// only a new complete inventory establishes completion.
    pub fn observe(&mut self, mut sample: Sample, receipt_ms: u64) -> Result<(), Rejected> {
        let old = self.machines.get(&sample.machine_id);
        if !identity(&sample.machine_id)
            || !identity(&sample.producer_id)
            || sample.label.len() > 256
        {
            if let Some(old) = self.machines.get_mut(&sample.machine_id) {
                old.conflict = true;
            }
            return Err(Rejected::Identity);
        }
        if old.is_none() && self.machines.len() >= self.max_machines
            || sample.operations.len() > self.max_operations
        {
            if let Some(old) = self.machines.get_mut(&sample.machine_id) {
                old.conflict = true;
            }
            return Err(Rejected::Limit);
        }
        if let Some(old) = old {
            if receipt_ms < old.receipt_ms {
                return Err(Rejected::ReceiverClock);
            }
            if sample.generation < old.sample.generation
                || sample.generation == old.sample.generation
                    && (sample.producer_id != old.sample.producer_id
                        || sample.sequence <= old.sample.sequence)
            {
                return Err(Rejected::OutOfOrder);
            }
        }
        let normalized = normalize(&sample);
        if normalized.is_err()
            && let Some(old) = self.machines.get_mut(&sample.machine_id)
        {
            old.conflict = true;
        }
        sample.operations = normalized?;
        if let Some(old) = self.machines.get(&sample.machine_id)
            && sample.operations.iter().any(|next| {
                old.sample.operations.iter().any(|before| {
                    before.operation_id == next.operation_id
                        && (before.handle != next.handle
                            || before.task != next.task
                            || before.checkout != next.checkout
                            || before.parent_id != next.parent_id)
                })
            })
        {
            if let Some(old) = self.machines.get_mut(&sample.machine_id) {
                old.conflict = true;
            }
            return Err(Rejected::Ownership);
        }
        if !sample.complete
            && let Some(old) = self.machines.get(&sample.machine_id)
        {
            for operation in &old.sample.operations {
                if !sample
                    .operations
                    .iter()
                    .any(|o| o.operation_id == operation.operation_id)
                {
                    sample.operations.push(operation.clone());
                }
            }
            if sample.operations.len() > self.max_operations {
                if let Some(old) = self.machines.get_mut(&sample.machine_id) {
                    old.conflict = true;
                }
                return Err(Rejected::Limit);
            }
        }
        self.machines.insert(
            sample.machine_id.clone(),
            Accepted {
                sample,
                receipt_ms,
                connected: true,
                conflict: false,
            },
        );
        Ok(())
    }

    /// A feed disconnect does not establish that its last-known operations finished.
    pub fn disconnect(&mut self, machine_id: &str) {
        if let Some(old) = self.machines.get_mut(machine_id) {
            old.connected = false;
        }
    }

    pub fn snapshot(&self, now_ms: u64) -> Snapshot {
        let machines: Vec<_> = self
            .machines
            .values()
            .map(|old| {
                let age = now_ms.checked_sub(old.receipt_ms);
                let freshness = if old.conflict {
                    Freshness::Conflict
                } else if !old.connected {
                    Freshness::Disconnected
                } else if age.is_none_or(|a| a > self.ttl_ms) {
                    Freshness::Stale
                } else if !old.sample.complete {
                    Freshness::Incomplete
                } else {
                    Freshness::Fresh
                };
                Machine {
                    machine_id: old.sample.machine_id.clone(),
                    label: old.sample.label.clone(),
                    producer_id: old.sample.producer_id.clone(),
                    sequence: old.sample.sequence,
                    age_ms: age,
                    complete: old.sample.complete,
                    counts: (freshness == Freshness::Fresh).then(|| counts(&old.sample.operations)),
                    reason: match freshness {
                        Freshness::Fresh => "complete receiver observation",
                        Freshness::Stale => "observation expired or receiver clock unavailable",
                        Freshness::Disconnected => "publisher disconnected; last-known work",
                        Freshness::Incomplete => "partial inventory; last-known work retained",
                        _ => "inventory rejected; last-known work retained",
                    }
                    .into(),
                    freshness,
                    operations: old.sample.operations.clone(),
                }
            })
            .collect();
        // The receiver's machine list does not prove all distributed publishers exist.
        Snapshot {
            machines,
            ..Snapshot::default()
        }
    }
}

fn normalize(sample: &Sample) -> Result<Vec<Operation>, Rejected> {
    let mut found = BTreeMap::new();
    for operation in &sample.operations {
        if operation.machine_id.as_deref() != Some(&sample.machine_id)
            || operation
                .operation_id
                .as_deref()
                .is_none_or(|id| !identity(id))
            || operation
                .parent_id
                .as_deref()
                .is_some_and(|id| !identity(id))
            || operation.handle.as_deref().is_some_and(|id| !identity(id))
            || operation.task.as_deref().is_some_and(|id| !identity(id))
            || operation.parent_id.is_some() && operation.parent_id == operation.operation_id
            || operation
                .phase_since
                .is_some_and(|since| !since.is_finite())
            || [
                &operation.checkout,
                &operation.recipe,
                &operation.resource,
                &operation.reason,
            ]
            .iter()
            .any(|s| s.len() > 4096)
        {
            return Err(Rejected::Ownership);
        }
        if let Some(before) = found.insert(&operation.operation_id, operation)
            && before != operation
        {
            return Err(Rejected::ConflictingOperation);
        }
    }
    Ok(found.into_values().cloned().collect())
}

fn identity(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
}

fn counts(operations: &[Operation]) -> Counts {
    let mut counts = Counts::default();
    for operation in operations {
        match (&operation.kind, &operation.state) {
            (Kind::Build, State::Running) => counts.running_builds += 1,
            (Kind::Build, State::Queued) => counts.queued_builds += 1,
            (Kind::Test, State::Running) => counts.running_tests += 1,
            (Kind::Test, State::Queued) => counts.queued_tests += 1,
            _ => {}
        }
    }
    counts
}
