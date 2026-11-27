//! One local native scan per existing board tick; every HTTP/feed consumer reads its cached view.
//! A scan reads the board's own recorder's operations (`tasks run`, in the common dir's
//! `symphony/tasks/work/`) directly, and a project's own reader beside them when it has one: the
//! executable its `symphony.toml` names (`[board] machine_work_reader`), or the one
//! `SYMPHONY_MACHINE_WORK_READER` names.

use super::*;
use symphony_board::cli::machine_work::{
    Evidence, Operation, Projection, Sample, Snapshot, native::Inventory, recorded,
};
use symphony_board::rows::TaskSessionRow;

const HISTORY_LIMIT: usize = 4096;
pub(super) const READER_ENV: symphony_env::Var = symphony_env::MACHINE_WORK_READER;

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
    /// The project's own reader, if it has one ([`Sampler::reader`]).
    reader: Result<Option<PathBuf>, String>,
    /// The reader was named (`SYMPHONY_MACHINE_WORK_READER`): it must answer, where the project's
    /// path may hold none.
    supplied: bool,
    /// How long the reader is given to answer a scan
    /// ([`symphony_board::cli::probes::NATIVE_READER_WAIT`]); a test of anything but that deadline
    /// gives it a stall's backstop.
    wait: Duration,
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
            wait: symphony_board::cli::probes::NATIVE_READER_WAIT,
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

    /// Retain a prepared pathname once, without PATH search, cache enumeration or source hashing:
    /// the one `supplied` names, else the one the project at `root` names (`[board]
    /// machine_work_reader`), else none. This is selection, not an image seal:
    /// unavailable/changed/old programs remain Unknown.
    fn reader(root: &Path, supplied: Option<&Path>) -> Result<Option<PathBuf>, String> {
        let reader = match supplied {
            Some(named) => named.to_path_buf(),
            None => match &symphony::project::Project::of(root)
                .board
                .machine_work_reader
            {
                Some(prepared) => root.join(prepared),
                None => return Ok(None),
            },
        };
        if !reader.is_absolute() {
            return Err("native inventory reader needs an absolute path".into());
        }
        Ok(Some(reader))
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
            // A named reader must answer; the project's answers when it put one at its path.
            let slots = match &reader {
                Some(reader) if self.supplied || reader.exists() => {
                    let mut env = symphony::children::board_env();
                    env.cwd = root.to_path_buf();
                    let bytes = symphony_board::cli::probes::native_inventory(
                        &env, reader, common, self.wait,
                    )
                    .map_err(|error| {
                        format!("native inventory reader {}: {error}", reader.display())
                    })?;
                    Some(bytes)
                }
                _ => None,
            };
            let read = Read { recorded, slots };
            if self.identity.is_none() {
                // Unsupported/malformed readers must not initialize even local identity storage.
                read.inventory(None)?;
                // Nor does a machine where nothing was ever recorded and no reader answers.
                if read.slots.is_none() && read.recorded.is_empty() {
                    return Err(match &reader {
                        Some(reader) => format!(
                            "no build or test recorded yet, and no project reader at {}",
                            reader.display()
                        ),
                        None => format!(
                            "no build or test recorded yet, and the project names no reader \
                             ([board] machine_work_reader in {})",
                            symphony::project::FILE
                        ),
                    });
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
    /// One tick's read of the board on `store`. On a board ahead of this build's schema that its
    /// store may not write ([`Hub::read_only_ahead`]) it reads alone: no chat journal, owner's
    /// asks, callbacks or gives are written, as the schema guard would refuse each. On one ahead
    /// that it may, it does all it does at its own.
    pub(crate) fn board_read(&self, store: &symphony_board::Store) -> Result<(), String> {
        let ahead = self.read_only_ahead(store).is_some();
        if !ahead
            && let Some(callbacks) = self.callbacks.lock().recover().as_mut()
            && let Some(Err(error)) =
                self.chat_retry.lock().recover().run(&self.retry_clock, || {
                    measured("chat journal", || callbacks.flush(&self.root))
                })
        {
            eprintln!("CHAT deferred: {error}");
        }
        if !ahead {
            self.keep_asks(store);
        }
        self.note_asks(store);
        self.health.reading(store.query_log());
        let now = self.now();
        // After a sleep, nothing judges the sessions until the idle sweep has looked at their
        // listeners again ([`super::presence`]): not the dispatch, not the give.
        let judging = !ahead && self.judges();
        let snapshot = if !judging {
            None
        } else {
            match self
                .dispatch_retry
                .lock()
                .recover()
                .run(&self.retry_clock, || {
                    measured("callbacks", || store.dispatch_callbacks_snapshot())
                }) {
                Some(Ok(snapshot)) => {
                    self.health.dispatched(now, Ok(()));
                    snapshot
                }
                Some(Err(error)) => {
                    eprintln!("CALLBACK deferred: {error}");
                    self.health.dispatched(now, Err(&error.to_string()));
                    None
                }
                None => None,
            }
        };
        self.tell_health(now);
        let read = || measured("board read", || store.clock_board()).map_err(|e| e.to_string());
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => read()?,
        };
        // Each tick gives the open tasks no orchestrator holds on the board it read, when one that
        // stays could take them ([`symphony_board::Store::give_if_due`]): every project the server
        // serves, whatever starts its sweeps. The give's own write delivers its GIVEN line.
        let snapshot = match judging && self.give(store, &snapshot) {
            true => read()?,
            false => snapshot,
        };
        // The blocks the owner's word ended, from the same read, as the dispatch ends a `task:`
        // block; the next tick's dispatch delivers each move's STATUS line.
        let snapshot = match judging && self.keep_decided(store, &snapshot) {
            true => read()?,
            false => snapshot,
        };
        // The board's health counts the open tasks nobody orchestrates from this snapshot too.
        self.health.unowned(&snapshot);
        // The one server's agents' day files, from this snapshot: no board read or write of its own.
        self.record_agents(&snapshot);
        let records = store.all_task_sessions().unwrap_or_default();
        let evidence =
            self.machine_work
                .lock()
                .recover()
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
        let project = symphony::project::Project::of(&self.root);
        // Each agent's way into its session, its pane only where the project addresses panes: the
        // board's store's setting over the project file's.
        let panes = store.stored();
        let panes = panes.panes(&project.agents.panes);
        symphony_api::resolve_sessions(&mut agents.agents, panes == "hc");
        let mut guidance = BTreeMap::new();
        for (id, limits) in &project.board.machine_wip {
            guidance.insert(
                id.clone(),
                symphony_api::WorkGuidance {
                    build: limits.build,
                    test: limits.test,
                },
            );
        }
        agents.machine_work.guidance = guidance;
        self.update_board(store, snapshot, agents)
    }

    /// A round of the idle sweep began, on the hub's clocks ([`super::presence`]).
    pub(crate) fn sweep_began(&self) {
        let (wall, mono) = ((self.clock)(), (self.retry_clock)());
        (self.presence.lock()).recover().began(wall, mono);
    }

    /// The round of the idle sweep under way ended, answered or failed.
    pub(crate) fn sweep_ended(&self) {
        (self.presence.lock()).recover().ended();
    }

    /// Whether this tick judges the sessions ([`super::presence::Presence::judges`]); the log says
    /// when a wait for the sweep begins, and when one ends with no round.
    pub(crate) fn judges(&self) -> bool {
        let (wall, mono) = ((self.clock)(), (self.retry_clock)());
        let judging = (self.presence.lock()).recover().judges(wall, mono);
        match judging {
            super::presence::Judging::Now => true,
            super::presence::Judging::Waits { began } => {
                if began {
                    eprintln!(
                        "· callbacks wait for the idle sweep: the machine slept since its last round began"
                    );
                }
                false
            }
            super::presence::Judging::Anyway { first } => {
                if first {
                    eprintln!(
                        "· callbacks judged without the idle sweep: no round ended within {} s awake of the wake",
                        super::presence::AT_MOST.as_secs()
                    );
                }
                true
            }
        }
    }

    /// The assignment pass has its own retry: a callback failure never delays a healthy give,
    /// and successful reads or dispatches never clear a failing give's deadline. After the give,
    /// on the board as it left it, the board settles what the tasks a custom workflow holds need
    /// of it ([`symphony_board::Store::try_settle_held_if_due`]: a lost holder recovered, a
    /// dependant cancelled by its position's policy) under the same retry, each refusal said on
    /// stderr: the clock is the reader that makes it so on a board where no session pulls,
    /// waits, monitors or writes. Whether either wrote.
    pub(super) fn give(
        &self,
        store: &symphony_board::Store,
        board: &symphony_board::Board,
    ) -> bool {
        let pass = || {
            let given = store.try_give_if_due(board)?;
            let settled = match given {
                true => store.try_settle_held_if_due(&store.clock_board()?)?,
                false => store.try_settle_held_if_due(board)?,
            };
            Ok::<bool, symphony_board::Error>(given || settled)
        };
        match self
            .give_retry
            .lock()
            .recover()
            .run(&self.retry_clock, pass)
        {
            Some(Ok(given)) => given,
            Some(Err(error)) => {
                eprintln!(
                    "giving open tasks an orchestrator, or settling the tasks a workflow holds, \
                     deferred: {error}"
                );
                false
            }
            None => false,
        }
    }

    pub(crate) fn agent_snapshot(&self) -> Result<symphony_api::Agents, String> {
        self.agents
            .lock()
            .recover()
            .clone()
            .ok_or_else(|| "the agents observation is starting".into())
    }

    pub(crate) fn machine_snapshot(&self) -> symphony_api::MachineWork {
        self.agents
            .lock()
            .recover()
            .as_ref()
            .map(|agents| agents.machine_work.clone())
            .unwrap_or_default()
    }

    pub(crate) fn observed_page(&self, mut page: symphony_api::TaskPage) -> symphony_api::TaskPage {
        page.machine_work = self.machine_snapshot();
        page.sessions = self.sessions_of(page.task.roles.iter().map(|r| r.handle.as_str()));
        page
    }

    /// Where the Go to session control of each of `handles` goes, as the last agents observation
    /// resolved it: a handle with none known, or not observed, is left out.
    pub(crate) fn sessions_of<'a>(
        &self,
        handles: impl IntoIterator<Item = &'a str>,
    ) -> std::collections::BTreeMap<String, symphony_api::SessionGo> {
        let agents = self.agents.lock().recover();
        let Some(agents) = agents.as_ref() else {
            return std::collections::BTreeMap::new();
        };
        let mut sessions = std::collections::BTreeMap::new();
        for (handle, go) in handles.into_iter().filter_map(|handle| {
            let agent = agents.agents.iter().find(|a| a.handle == handle)?;
            Some((handle.to_string(), agent.go.clone()?))
        }) {
            sessions.insert(handle, go);
        }
        sessions
    }

    /// Where `handle`'s Go to session control goes now, or why there is nowhere: the agents
    /// observation is starting, it lists no such agent, or nothing is known of its session.
    pub(crate) fn session_of(&self, handle: &str) -> Result<symphony_api::SessionGo, String> {
        let agents = self.agents.lock().recover();
        let agents = agents
            .as_ref()
            .ok_or_else(|| "the agents observation is starting".to_string())?;
        let agent = agents
            .agents
            .iter()
            .find(|a| a.handle == handle)
            .ok_or_else(|| format!("no agent {handle} is connected"))?;
        agent.go.clone().ok_or_else(|| {
            format!("nothing is known of {handle}'s session: no web address, no pane")
        })
    }
}

#[cfg(test)]
mod tests;
