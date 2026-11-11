//! One local native scan per existing board tick; every HTTP/feed consumer reads its cached view.
//! The project's own reader, when it names one (`SF_MACHINE_WORK_READER`, or a prepared
//! `.target/bin/sf-slots`), else the built-in reader of Symphony's run records
//! ([`symphony_board::cli::machine_work::runs`]).

use super::*;
use symphony_board::cli::machine_work::{
    Evidence, Operation, Projection, Sample, Snapshot, native::Inventory, runs,
};
use symphony_board::rows::TaskSessionRow;

const HISTORY_LIMIT: usize = 4096;
pub(super) const READER_ENV: &str = "SF_MACHINE_WORK_READER";
/// How long a task's last finished run stays after it ended (a week): the evidence of open work.
const LAST_RUN_SECONDS: f64 = 7.0 * 86_400.0;

/// What a tick's scan attributes runs by: the board it read, and the bindings, read only when a
/// run not bound yet asks for them.
pub(super) struct Context<'a> {
    pub records: &'a [TaskSessionRow],
    pub tasks: &'a [symphony_board::Task],
    pub bindings: &'a dyn Fn() -> BTreeMap<String, symphony_board::Binding>,
}

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

pub(super) struct Sampler {
    projection: Projection,
    reader: Result<PathBuf, String>,
    /// The reader was named explicitly: it answers, or the observation is unknown.
    named: bool,
    identity: Option<Result<String, String>>,
    revisions: BTreeMap<String, Vec<u8>>,
    sequence: u64,
    began: Instant,
    tasks: BTreeMap<String, Option<String>>,
    unassigned: Vec<Operation>,
    reason: String,
    /// The built-in reader's last view, when it was the one read.
    records: Option<RunView>,
}

/// What the built-in reader keeps between ticks.
#[derive(Default)]
struct RunView {
    /// Each run's session and task, bound when it was first seen; dropped with its record.
    owners: BTreeMap<String, runs::Owner>,
    operations: Vec<Operation>,
    /// Moves only when the runs going change, so an unchanged tick sends the feed nothing.
    sequence: u64,
    /// Each task's last finished run.
    last: BTreeMap<String, runs::Run>,
}

impl Sampler {
    pub(super) fn new(root: &Path, supplied: Option<&Path>) -> Self {
        Self {
            projection: Projection::new(64, HISTORY_LIMIT, 10_000),
            reader: Self::reader(root, supplied),
            named: supplied.is_some(),
            identity: None,
            revisions: BTreeMap::new(),
            sequence: 0,
            began: Instant::now(),
            tasks: BTreeMap::new(),
            unassigned: Vec::new(),
            reason: String::new(),
            records: None,
        }
    }

    /// The last finished run of task `slug` the built-in reader keeps: its command, exit status,
    /// duration and tip.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the task page's gates read it next")
    )]
    pub(super) fn last_run(&self, slug: &str) -> Option<&runs::Run> {
        self.records.as_ref()?.last.get(slug)
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

    fn accept(
        &mut self,
        bytes: &[u8],
        records: &[TaskSessionRow],
        now: u64,
        label: &str,
    ) -> Result<(), String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or("native machine identity unavailable")?
            .as_ref()
            .map_err(Clone::clone)?;
        let mut inventory = Inventory::decode(bytes, Some(identity))?;
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

    fn unavailable(&mut self, reason: String) {
        if let Some(Ok(identity)) = &self.identity {
            self.projection.disconnect(identity);
        }
        self.reason = reason;
    }

    /// One tick's observation: the project's reader when it names one (explicitly, or by preparing
    /// `.target/bin/sf-slots`), else the run records.
    pub(super) fn scan(
        &mut self,
        root: &Path,
        common: Option<&Path>,
        context: &Context<'_>,
    ) -> Evidence {
        let label = symphony_board::sys::hostname();
        let program = self.named || self.reader.as_ref().is_ok_and(|reader| reader.exists());
        if !program {
            return self.scan_runs(common, context, label);
        }
        self.records = None;
        let records = context.records;
        let result = (|| {
            let common = common.ok_or("native common directory unavailable")?;
            let reader = self.reader.as_ref().map_err(Clone::clone)?;
            let mut env = symphony::children::board_env();
            env.cwd = root.to_path_buf();
            let bytes = symphony_board::cli::probes::native_inventory(&env, reader, common)
                .map_err(|error| {
                    format!("native inventory reader {}: {error}", reader.display())
                })?;
            if self.identity.is_none() {
                // Unsupported/malformed readers must not initialize even local identity storage.
                Inventory::decode(&bytes, None)?;
                self.identity = Some(symphony::machine::installation_id());
            }
            if self.identity.as_ref().is_some_and(Result::is_err) {
                // Preserve the first bounded unassigned evidence; absence cannot retire it.
                if self.unassigned.is_empty() {
                    self.unassigned = Inventory::decode(&bytes, None)?.unassigned();
                }
                return Err(
                    "native machine identity is unavailable; work remains unassigned".into(),
                );
            }
            self.accept(&bytes, records, self.receipt(), &label)
        })();
        if let Err(reason) = result {
            self.unavailable(reason);
        }
        Evidence {
            local_host: label,
            snapshot: self.snapshot(self.receipt()),
        }
    }

    /// The built-in reader's tick: the runs going now, each bound to its session and task when
    /// first seen, as one machine's partial inventory; and each task's last finished run kept. A
    /// project that never recorded a run has no folder and so no publisher: no machine.
    fn scan_runs(
        &mut self,
        common: Option<&Path>,
        context: &Context<'_>,
        label: String,
    ) -> Evidence {
        let mut view = self.records.take().unwrap_or_default();
        let result = (|| -> Result<bool, String> {
            let common = common.ok_or("run records: no git common directory")?;
            if !common.join(runs::DIR).is_dir() {
                view.operations.clear();
                return Ok(false);
            }
            let found = runs::read(common, &runs::alive)?;
            let seen: BTreeSet<&str> = (found.going.iter().chain(&found.finished))
                .map(|run| run.id.as_str())
                .collect();
            view.owners.retain(|id, _| seen.contains(id.as_str()));
            let unbound: Vec<&runs::Run> = (found.going.iter().chain(&found.finished))
                .filter(|run| !view.owners.contains_key(&run.id))
                .collect();
            if !unbound.is_empty() {
                let bindings = (context.bindings)();
                let owners = runs::Owners {
                    tasks: context.tasks,
                    bindings: &bindings,
                    records: context.records,
                };
                for run in unbound {
                    view.owners.insert(run.id.clone(), owners.of(run));
                }
            }
            let identity = self
                .identity
                .get_or_insert_with(symphony::machine::installation_id);
            let owner = |run: &runs::Run| view.owners.get(&run.id).cloned().unwrap_or_default();
            let operations: Vec<Operation> = (found.going.iter())
                .map(|run| {
                    let mut operation = runs::operation(run, "", owner(run));
                    operation.machine_id = identity.as_ref().ok().cloned();
                    operation
                })
                .collect();
            if operations != view.operations {
                view.sequence += 1;
                view.operations = operations;
            }
            let now = symphony_board::sweep::trigger::now();
            let finished = (found.finished.into_iter())
                .map(|run| {
                    let task = owner(&run).task;
                    let ended = run.ended.as_ref().map_or(run.started, |ended| ended.at);
                    (run, task.filter(|_| now - ended <= LAST_RUN_SECONDS))
                })
                .collect();
            view.last = runs::keep_last(common, finished, now);
            Ok(true)
        })();
        let mut snapshot = Snapshot::default();
        self.reason.clear();
        match (result, self.identity.as_ref()) {
            (Ok(true), Some(Ok(identity))) => snapshot.machines.push(runs::machine(
                identity,
                &label,
                view.sequence,
                view.operations.clone(),
            )),
            // Without an identity the runs stay visible, unassigned, with no machine to place them.
            (Ok(true), Some(Err(reason))) => {
                snapshot.unassigned = view.operations.clone();
                self.reason = format!("run records: {reason}; work remains unassigned");
            }
            (Ok(_), _) => {}
            (Err(reason), _) => {
                view.operations.clear();
                self.reason = reason;
            }
        }
        self.records = Some(view);
        Evidence {
            local_host: label,
            snapshot,
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
        let bindings = || store.agent_bindings().unwrap_or_default();
        let context = Context {
            records: &records,
            tasks: &snapshot.tasks,
            bindings: &bindings,
        };
        let evidence = self
            .machine_work
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .scan(&self.root, self.common.as_deref(), &context);
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
