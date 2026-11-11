//! One local native scan per existing board tick; every HTTP/feed consumer reads its cached view.
//! A scan reads the board's own recorder's operations (`tasks run`, in the common dir's
//! `sf-tasks/work/`) directly, and a project's own reader beside them when it has one: an
//! `sf-slots` at its prepared path, or the one `SF_MACHINE_WORK_READER` names.

use super::*;
use symphony_board::cli::machine_work::{
    Evidence, Operation, Projection, Sample, Snapshot, native::Inventory, recorded,
};
use symphony_board::rows::TaskSessionRow;

const HISTORY_LIMIT: usize = 4096;
pub(super) const READER_ENV: &str = "SF_MACHINE_WORK_READER";

/// Component matching excludes sibling names and traversal, and is never machine identity.
fn in_checkout(cwd: &str, checkout: &str) -> bool {
    use std::path::Component;
    let cwd = Path::new(cwd);
    let checkout = Path::new(checkout);
    cwd.is_absolute()
        && checkout.is_absolute()
        && cwd
            .components()
            .chain(checkout.components())
            .all(|part| !matches!(part, Component::ParentDir | Component::CurDir))
        && cwd.starts_with(checkout)
}

/// A first observation has no trusted launch time. Require one task across all possible
/// recorded contexts for this handle, including closed launches and other checkouts.
/// A matching open checkout is supporting context, never proof to discard conflicting history.
/// Frozen bindings are handled by the caller and never rewritten when the current role changes.
fn initial_task(operation: &Operation, records: &[TaskSessionRow]) -> Option<String> {
    let handle = operation.handle.as_deref()?;
    let mut task = None;
    let mut open = false;
    for record in records.iter().filter(|record| record.handle == handle) {
        let matching = in_checkout(&operation.checkout, &record.worktree);
        if task.is_some_and(|task| task != record.slug.as_str()) {
            return None;
        }
        task = Some(record.slug.as_str());
        open |= matching && record.closed.is_none();
    }
    if open { task.map(str::to_owned) } else { None }
}

/// One scan's evidence, read once: the recorder's operations, and the project reader's answer when
/// the project has a reader.
struct Read {
    recorded: recorded::Found,
    slots: Option<Vec<u8>>,
}

impl Read {
    /// Both sources as one inventory on `machine_id`; either one malformed refuses the whole read.
    fn inventory(&self, machine_id: Option<&str>) -> Result<Inventory, String> {
        let inventory = Inventory::recorded(&self.recorded, machine_id)?;
        match &self.slots {
            Some(bytes) => inventory.merge(Inventory::decode(bytes, machine_id)?),
            None => Ok(inventory),
        }
    }
}

pub(super) struct Sampler {
    projection: Projection,
    reader: Result<PathBuf, String>,
    /// The reader was named (`SF_MACHINE_WORK_READER`): it must answer, where the prepared path
    /// may hold none.
    supplied: bool,
    identity: Option<Result<String, String>>,
    revisions: BTreeMap<String, Vec<u8>>,
    /// The nonces among `revisions` the board's own recorder wrote.
    recorded: BTreeSet<String>,
    sequence: u64,
    began: Instant,
    tasks: BTreeMap<String, Option<String>>,
    unassigned: Vec<Operation>,
    reason: String,
}

impl Sampler {
    pub(super) fn new(root: &Path, supplied: Option<&Path>) -> Self {
        Self {
            projection: Projection::new(64, HISTORY_LIMIT, 10_000),
            reader: Self::reader(root, supplied),
            supplied: supplied.is_some(),
            identity: None,
            revisions: BTreeMap::new(),
            recorded: BTreeSet::new(),
            sequence: 0,
            began: Instant::now(),
            tasks: BTreeMap::new(),
            unassigned: Vec::new(),
            reason: String::new(),
        }
    }

    fn receipt(&self) -> u64 {
        self.began
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// Retain a prepared pathname once, without PATH search, cache enumeration or source hashing.
    /// This is selection, not an image seal: unavailable/changed/old programs remain Unknown.
    fn reader(root: &Path, supplied: Option<&Path>) -> Result<PathBuf, String> {
        let reader = supplied
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join(".target/bin/sf-slots"));
        if !reader.is_absolute() {
            return Err("native inventory reader needs an absolute path".into());
        }
        Ok(reader)
    }

    fn trusted(&self) -> Result<String, String> {
        self.identity
            .as_ref()
            .ok_or("native machine identity unavailable")?
            .clone()
    }

    /// A project reader's answer alone, on this machine.
    #[cfg(test)]
    fn accept(
        &mut self,
        bytes: &[u8],
        records: &[TaskSessionRow],
        now: u64,
        label: &str,
    ) -> Result<(), String> {
        let inventory = Inventory::decode(bytes, Some(&self.trusted()?))?;
        self.observe(inventory, records, now, label)
    }

    fn observe(
        &mut self,
        mut inventory: Inventory,
        records: &[TaskSessionRow],
        now: u64,
        label: &str,
    ) -> Result<(), String> {
        let identity = &self.trusted()?;
        self.forget_recorded(&inventory);
        if self.sequence != 0
            && !inventory
                .revisions()
                .iter()
                .any(|(id, bytes)| self.revisions.get(id) != Some(bytes))
        {
            self.projection.scanned_native(identity, &inventory);
            self.reason.clear();
            return Ok(());
        }
        let revisions = inventory.revisions().clone();
        let mut bindings = self.tasks.clone();
        inventory.attribute(|operation| {
            let id = operation.operation_id.as_ref()?;
            if !bindings.contains_key(id) && bindings.len() < HISTORY_LIMIT {
                bindings.insert(id.clone(), initial_task(operation, records));
            }
            bindings.get(id).cloned().flatten()
        });
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("native sequence exhausted")?;
        self.projection
            .observe_native(
                Sample {
                    machine_id: identity.clone(),
                    label: label.into(),
                    producer_id: format!("local:{identity}"),
                    generation: 1,
                    sequence,
                    complete: false,
                    operations: Vec::new(),
                },
                inventory,
                now,
            )
            .map_err(|error| format!("native inventory rejected: {error:?}"))?;
        self.sequence = sequence;
        self.revisions.extend(revisions);
        self.tasks = bindings;
        self.reason.clear();
        Ok(())
    }

    /// The board recorder's records swept from its work folder never come back: their revisions
    /// go, as their ownership does in the projection, so neither grows with every build.
    fn forget_recorded(&mut self, inventory: &Inventory) {
        let read = inventory.revisions();
        let revisions = &mut self.revisions;
        self.recorded.retain(|id| {
            let kept = read.contains_key(id);
            if !kept {
                revisions.remove(id);
            }
            kept
        });
        self.recorded
            .extend(inventory.recorded_ids().iter().cloned());
    }

    fn unavailable(&mut self, reason: String) {
        if let Some(Ok(identity)) = &self.identity {
            self.projection.disconnect(identity);
        }
        self.reason = reason;
    }

    pub(super) fn scan(
        &mut self,
        root: &Path,
        common: Option<&Path>,
        records: &[TaskSessionRow],
    ) -> Evidence {
        let label = symphony_board::sys::hostname();
        let result = (|| {
            let common = common.ok_or("native common directory unavailable")?;
            let reader = self.reader.as_ref().map_err(Clone::clone)?.clone();
            let recorded = recorded::read(&recorded::in_common(common));
            // A named reader must answer; the prepared path answers when a project put one there.
            let slots = if self.supplied || reader.exists() {
                let mut env = symphony::children::board_env();
                env.cwd = root.to_path_buf();
                let bytes = symphony_board::cli::probes::native_inventory(&env, &reader, common)
                    .map_err(|error| {
                        format!("native inventory reader {}: {error}", reader.display())
                    })?;
                Some(bytes)
            } else {
                None
            };
            let read = Read { recorded, slots };
            if self.identity.is_none() {
                // Unsupported/malformed readers must not initialize even local identity storage.
                read.inventory(None)?;
                // Nor does a machine where nothing was ever recorded and no reader answers.
                if read.slots.is_none() && read.recorded.is_empty() {
                    return Err(format!(
                        "no build or test recorded yet, and no project reader at {}",
                        reader.display()
                    ));
                }
                self.identity = Some(symphony::machine::installation_id());
            }
            if self.identity.as_ref().is_some_and(Result::is_err) {
                // Preserve the first bounded unassigned evidence; absence cannot retire it.
                if self.unassigned.is_empty() {
                    self.unassigned = read.inventory(None)?.unassigned();
                }
                return Err(
                    "native machine identity is unavailable; work remains unassigned".into(),
                );
            }
            let inventory = read.inventory(Some(&self.trusted()?))?;
            self.observe(inventory, records, self.receipt(), &label)
        })();
        if let Err(reason) = result {
            self.unavailable(reason);
        }
        Evidence {
            local_host: label,
            snapshot: self.snapshot(self.receipt()),
        }
    }

    fn snapshot(&self, now: u64) -> Snapshot {
        let mut snapshot = self.projection.snapshot(now);
        snapshot.unassigned = self.unassigned.clone();
        if !self.reason.is_empty() {
            for machine in &mut snapshot.machines {
                machine.freshness = symphony_board::cli::machine_work::Freshness::Unknown;
                machine.counts = None;
                machine.reason = format!("native observation unknown: {}", self.reason);
            }
        }
        snapshot
    }
}

impl Hub {
    pub(super) fn board_read(&self, store: &symphony_board::Store) -> Result<(), String> {
        if let Some(callbacks) = self
            .callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            && let Err(error) = measured("chat journal", || callbacks.flush(&self.root))
        {
            eprintln!("CHAT deferred: {error}");
        }
        self.health.reading(store.client().queries());
        let dispatched = measured("callbacks", || store.dispatch_callbacks_snapshot());
        let now = self.now();
        let snapshot = match dispatched {
            Ok(snapshot) => {
                self.health.dispatched(now, Ok(()));
                snapshot
            }
            Err(error) => {
                eprintln!("CALLBACK deferred: {error}");
                self.health.dispatched(now, Err(&error.to_string()));
                None
            }
        };
        self.tell_health(now);
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => {
                measured("board read", || store.clock_board()).map_err(|error| error.to_string())?
            }
        };
        let records = store.all_task_sessions().unwrap_or_default();
        let evidence = self
            .machine_work
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .scan(&self.root, self.common.as_deref(), &records);
        let mut agents = measured("agents", || {
            taskview::agents_in_observed(
                store,
                &self.root,
                self.common.as_deref(),
                &snapshot,
                evidence,
            )
        })?;
        agents.machine_work.guidance = symphony::project::Project::of(&self.root)
            .board
            .machine_wip
            .iter()
            .map(|(id, guidance)| {
                (
                    id.clone(),
                    symphony_api::WorkGuidance {
                        build: guidance.build,
                        test: guidance.test,
                    },
                )
            })
            .collect();
        self.update_board(store, snapshot, agents)
    }

    pub(crate) fn agent_snapshot(&self) -> Result<symphony_api::Agents, String> {
        self.agents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or_else(|| "the agents observation is starting".into())
    }

    pub(crate) fn machine_snapshot(&self) -> symphony_api::MachineWork {
        self.agents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|agents| agents.machine_work.clone())
            .unwrap_or_default()
    }

    pub(crate) fn observed_page(&self, mut page: symphony_api::TaskPage) -> symphony_api::TaskPage {
        page.machine_work = self.machine_snapshot();
        page
    }
}

#[cfg(test)]
mod tests;
