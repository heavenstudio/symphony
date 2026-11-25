//! Bound receiver inventories and retain nonce ownership even after complete disappearance.

use super::{Counts, Freshness, Kind, Machine, Operation, Snapshot, State};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Last accepted feed receipt, used by the receiver-clock fence even for mixed-age work.
    receipt_ms: u64,
    /// Native phase receipts remain separate: a changed sibling cannot renew retained work.
    native_receipts: BTreeMap<String, u64>,
    connected: bool,
    conflict: bool,
    history_exhausted: bool,
    uncertain: bool,
    owners: BTreeMap<String, Ownership>,
}

impl Accepted {
    fn exhaust_history(&mut self) {
        self.conflict = true;
        self.history_exhausted = true;
    }
}

/// History owns these fields because a complete inventory may drop the live operation.
struct Ownership {
    handle: Option<String>,
    task: Option<String>,
    checkout: String,
    parent_id: Option<String>,
    terminal: bool,
    /// Written by the board's own recorder, whose finished records leave its work folder for
    /// good: once terminal and no longer read, the nonce is forgotten ([`Projection::retire`]).
    recorded: bool,
}

impl Ownership {
    fn of(operation: &Operation, recorded: bool) -> Self {
        Self {
            handle: operation.handle.clone(),
            task: operation.task.clone(),
            checkout: operation.checkout.clone(),
            parent_id: operation.parent_id.clone(),
            terminal: false,
            recorded,
        }
    }

    fn matches(&self, operation: &Operation) -> bool {
        self.handle == operation.handle
            && self.task == operation.task
            && self.checkout == operation.checkout
            && self.parent_id == operation.parent_id
    }
}

/// Bounded receiver-side inventory. Limits bound reports, not admission or execution.
pub struct Projection {
    machines: BTreeMap<String, Accepted>,
    max_machines: usize,
    max_operations: usize,
    ttl_ms: u64,
}

impl Projection {
    /// `max_operations` bounds both live rows and lifetime nonce ownership per machine.
    /// History is never evicted, including on publisher restart, but for the board recorder's
    /// finished nonces, forgotten once their records are gone ([`Self::retire`]). At its bound,
    /// unseen nonces latch the machine unknown for this projection's lifetime, retaining
    /// last-known work.
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
    pub fn observe(&mut self, sample: Sample, receipt_ms: u64) -> Result<(), Rejected> {
        self.observe_batch(sample, Vec::new(), receipt_ms, None, &BTreeSet::new())
    }

    /// Forget each finished nonce the board's recorder wrote that `inventory` no longer reads: its
    /// record left the work folder for good, so it can neither return nor be reused, and its
    /// ownership need not count against the lifetime bound.
    fn retire(&mut self, machine_id: &str, inventory: &super::native::Inventory) {
        if let Some(old) = self.machines.get_mut(machine_id) {
            let read = inventory.revisions();
            old.owners
                .retain(|id, owner| !(owner.recorded && owner.terminal && !read.contains_key(id)));
        }
    }

    /// Trusted-local raw evidence is always partial. Live rows and decoded completions share
    /// every ordering, clock, identity, bound and ownership fence, before any active row retires.
    pub fn observe_native(
        &mut self,
        mut sample: Sample,
        inventory: super::native::Inventory,
        receipt_ms: u64,
    ) -> Result<(), Rejected> {
        if inventory.machine_id.as_deref() != Some(sample.machine_id.as_str()) {
            return Err(Rejected::Identity);
        }
        self.retire(&sample.machine_id, &inventory);
        sample.complete = false;
        sample.operations = inventory.operations;
        self.observe_batch(
            sample,
            inventory.finished,
            receipt_ms,
            Some(inventory.uncertain),
            &inventory.recorded,
        )
    }

    fn observe_batch(
        &mut self,
        mut sample: Sample,
        finished: Vec<Operation>,
        receipt_ms: u64,
        native_uncertainty: Option<bool>,
        recorded: &BTreeSet<String>,
    ) -> Result<(), Rejected> {
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
        if old.is_none() && self.machines.len() >= self.max_machines {
            return Err(Rejected::Limit);
        }
        // Only a new observation from the current feed may exhaust accepted history.
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
        if sample.operations.len().saturating_add(finished.len()) > self.max_operations {
            if let Some(old) = self.machines.get_mut(&sample.machine_id) {
                // Rejected rows may contain unseen nonces. This path must retain the same
                // uncertainty as the normalized history bound, without accepting any rows.
                old.exhaust_history();
            }
            return Err(Rejected::Limit);
        }
        let mut terminal: BTreeSet<_> = BTreeSet::new();
        terminal.extend(
            finished
                .iter()
                .filter_map(|operation| operation.operation_id.clone()),
        );
        let mut all = sample.clone();
        all.operations.extend(finished);
        let normalized = normalize(&all);
        if normalized.is_err()
            && let Some(old) = self.machines.get_mut(&sample.machine_id)
        {
            old.conflict = true;
        }
        all.operations = normalized?;
        self.validate_ownership(&all)?;
        if let Some(old) = self.machines.get_mut(&sample.machine_id)
            && all.operations.iter().any(|operation| {
                let id = operation.operation_id.as_deref().expect("normalized id");
                !terminal.contains(id) && old.owners.get(id).is_some_and(|owner| owner.terminal)
            })
        {
            old.conflict = true;
            return Err(Rejected::ConflictingOperation);
        }
        sample.operations = all
            .operations
            .iter()
            .filter(|operation| {
                !terminal.contains(operation.operation_id.as_ref().expect("normalized id"))
            })
            .cloned()
            .collect();
        if !sample.complete
            && let Some(old) = self.machines.get(&sample.machine_id)
        {
            for operation in &old.sample.operations {
                if !terminal.contains(operation.operation_id.as_ref().expect("normalized id"))
                    && !sample
                        .operations
                        .iter()
                        .any(|o| o.operation_id == operation.operation_id)
                {
                    sample.operations.push(operation.clone());
                }
            }
            // The ownership-history bound already covers the union of old and new live ids.
        }
        let native_receipts = if native_uncertainty.is_some() {
            native_receipts(&sample, self.machines.get(&sample.machine_id), receipt_ms)
        } else {
            BTreeMap::new()
        };
        let mut owners = self
            .machines
            .remove(&sample.machine_id)
            .map(|old| old.owners)
            .unwrap_or_default();
        for operation in &all.operations {
            let id = operation
                .operation_id
                .clone()
                .expect("normalized operation id");
            let written = recorded.contains(&id);
            let owner = owners
                .entry(id)
                .or_insert_with(|| Ownership::of(operation, written));
            owner.terminal |=
                terminal.contains(operation.operation_id.as_ref().expect("normalized id"));
        }
        self.machines.insert(
            sample.machine_id.clone(),
            Accepted {
                sample,
                receipt_ms,
                native_receipts,
                connected: true,
                conflict: false,
                history_exhausted: false,
                uncertain: native_uncertainty.unwrap_or(false),
                owners,
            },
        );
        Ok(())
    }

    fn validate_ownership(&mut self, sample: &Sample) -> Result<(), Rejected> {
        let Some(old) = self.machines.get_mut(&sample.machine_id) else {
            return Ok(());
        };
        let mut new_ids = 0;
        for operation in &sample.operations {
            let id = operation
                .operation_id
                .as_deref()
                .expect("normalized operation id");
            match old.owners.get(id) {
                Some(owner) if !owner.matches(operation) => {
                    old.conflict = true;
                    return Err(Rejected::Ownership);
                }
                Some(_) => {}
                None => new_ids += 1,
            }
        }
        if old.history_exhausted || new_ids > self.max_operations.saturating_sub(old.owners.len()) {
            old.exhaust_history();
            return Err(Rejected::Limit);
        }
        Ok(())
    }

    /// A feed disconnect does not establish that its last-known operations finished.
    pub fn disconnect(&mut self, machine_id: &str) {
        if let Some(old) = self.machines.get_mut(machine_id) {
            old.connected = false;
        }
    }

    /// The adapter already matched accepted publication revisions. A scan changes connectivity
    /// and read uncertainty only; it never accepts rows, retires work or renews phase receipt.
    pub fn scanned_native(&mut self, machine_id: &str, inventory: &super::native::Inventory) {
        if inventory.machine_id.as_deref() != Some(machine_id) {
            return;
        }
        if let Some(old) = self.machines.get_mut(machine_id)
            && !old.sample.complete
        {
            old.connected = true;
            old.uncertain = inventory.uncertain;
        }
    }

    pub fn snapshot(&self, now_ms: u64) -> Snapshot {
        let machines: Vec<_> = self
            .machines
            .values()
            .map(|old| {
                // One machine freshness is conservative for mixed-age native work: even a
                // new sibling remains qualified by the oldest retained phase. An empty partial
                // inventory still has unknown capacity, regardless of its accepted feed receipt.
                let receipt = old.native_receipts.values().min().copied().unwrap_or(old.receipt_ms);
                let age = now_ms.checked_sub(old.receipt_ms)
                    .and_then(|_| now_ms.checked_sub(receipt));
                let freshness = if old.conflict {
                    Freshness::Conflict
                } else if !old.connected {
                    Freshness::Disconnected
                } else if old.uncertain {
                    Freshness::Unknown
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
                        Freshness::Conflict if old.history_exhausted => {
                            "nonce ownership history exhausted; last-known work retained"
                        }
                        Freshness::Fresh => "complete receiver observation",
                        Freshness::Stale if !old.native_receipts.is_empty() => {
                            "oldest retained phase expired or receiver clock unavailable; partial inventory"
                        }
                        Freshness::Stale => "observation expired or receiver clock unavailable",
                        Freshness::Disconnected => "publisher disconnected; last-known work",
                        Freshness::Incomplete => "partial inventory; last-known work retained",
                        Freshness::Unknown => "some native records unreadable or owners unconfirmed; last-known work retained",
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

/// Called only after all observation fences and partial carry-forward. Normalized native phases
/// keep their receipt when unchanged or absent; metadata outside Operation cannot make them newer.
fn native_receipts(
    sample: &Sample,
    old: Option<&Accepted>,
    receipt_ms: u64,
) -> BTreeMap<String, u64> {
    let mut previous = BTreeMap::new();
    previous.extend(
        (old.into_iter())
            .flat_map(|old| &old.sample.operations)
            .map(|operation| {
                (
                    operation.operation_id.as_deref().expect("normalized id"),
                    operation,
                )
            }),
    );
    let mut receipts = BTreeMap::new();
    receipts.extend(sample.operations.iter().map(|operation| {
        let id = operation.operation_id.as_deref().expect("normalized id");
        let retained = old.filter(|_| previous.get(id).copied() == Some(operation));
        let receipt = retained.map_or(receipt_ms, |old| {
            old.native_receipts
                .get(id)
                .copied()
                .unwrap_or(old.receipt_ms)
        });
        (id.to_owned(), receipt)
    }));
    receipts
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

pub(crate) fn identity(text: &str) -> bool {
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
