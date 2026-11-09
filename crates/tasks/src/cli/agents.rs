//! Who is doing what: the agents API (`/api/agents`, `just tasks agents [--json]`).
//!
//! The board is the record. Each agent is a board session; its task sessions say which task it runs, in
//! which worktree and branch, as which Claude session and in which hc terminal. hc's terminals and the
//! test-slot locks ([`super::probes`]) are probes that say what each agent is doing at this moment;
//! they feed the answer, never replace the record, and a probe that cannot be asked is named
//! `unavailable` with the answer still built from the board.
//!
//! The JSON is a contract, listed in docs/workflow/TASKS.md#the-agents-api:
//!
//! ```text
//! {"generated": ts, "probes": {"herdr": "ok"|"unavailable", "slots": "ok"|"unavailable"}, "agents": [{
//!     "handle", "kind", "model", "parent",
//!     "task", "role",                 # the task it works now (an open one first) and its role there
//!     "tasks": [{"slug", "role", "status"}],     # every task it holds a role on, bar orchestrating
//!     "worktree", "branch",           # where it really works (its task session, else its pane)
//!     "agent", "session_id", "pane",  # claude|codex, its conversation id, its hc terminal
//!     "state", "state_detail",
//!     "health",                       # the board's, except that a run in progress reads live
//!     "last_tool_call",               # when a tool call of its agent last beat (null before one)
//!     "last_message": {"slug", "ts", "text"} | null}]}
//! ```
//!
//! `state` is one of `working`, `running_test` (its worktree holds a test slot or is queued for one;
//! `state_detail` says `{"recipe", "phase": "holding"|"waiting", "since"}`), `waiting_on_input` (hc
//! sees an approval or question), `idle`, `gone` (no pane and no sign of life). An agent waiting on its
//! own verify is `running_test` and reads live, never lost. A run is the agent's when tagged with its
//! handle, else by exact worktree to an agent alone in it: never by a directory prefix, never to agents
//! sharing a worktree.

use crate::health::Health;
use crate::pyjson::Py;
use crate::rows::TaskSessionRow;
use crate::{Error, Session, Store, Task};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
mod attribution;
mod selection;
pub(crate) use attribution::find_pane;
use attribution::{cut, owned_pane, resolve};
use selection::{first, pane_state, primary, tasks_by_handle};

/// The longest last message shown, in characters, before it is cut.
pub const MESSAGE_CHARS: usize = 200;

/// A probe's answer, or why it could not be asked.
pub type Answer<T> = Result<T, String>;

/// An hc terminal (a pane of the Herdr app), as much of it as the view reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    /// Its hc address (`t7`).
    pub id: String,
    /// `claude` or `codex`.
    pub agent: Option<String>,
    /// hc's status word: `working`, `blocked`, `idle`, `done`, `error`.
    pub status: Option<String>,
    /// The agent's conversation id, when what runs in it names one.
    pub session: Option<String>,
    /// The folder its foreground process runs in, else the one it started in; "" if neither.
    pub cwd: String,
    pub title: Option<String>,
    pub name: Option<String>,
}

/// A test run holding a slot or queued for one (`test_slots.runs`).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub pid: i64,
    /// Its git top level.
    pub worktree: String,
    pub recipe: String,
    /// `holding` or `waiting`.
    pub phase: String,
    pub since: Option<f64>,
    /// `run` or `render`.
    pub pool: String,
    /// The board session it runs for.
    pub handle: Option<String>,
}

/// A probe of the machine: its answer, or why it could not be asked.
pub type Probe<'a, T> = Box<dyn Fn() -> Answer<T> + 'a>;
/// The git branch checked out in a folder.
pub type BranchOf<'a> = Box<dyn Fn(&str) -> Answer<String> + 'a>;

/// What can be asked about the machine, each a function so tests give their own: hc's terminals, the
/// slot runs, and the git branch checked out in a folder.
pub struct Probes<'a> {
    pub panes: Probe<'a, Vec<Pane>>,
    pub runs: Probe<'a, Vec<Run>>,
    pub branch_of: BranchOf<'a>,
}

/// What a `running_test` agent runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub recipe: String,
    pub phase: String,
    pub since: Option<f64>,
}

/// A task an agent holds a role on.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub slug: String,
    pub role: String,
    pub status: String,
}

/// An agent's last words on the board.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub slug: String,
    pub ts: f64,
    pub text: String,
}

/// One agent of the view: the contract's keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub handle: String,
    pub kind: String,
    pub model: String,
    pub parent: Option<String>,
    pub task: Option<String>,
    pub role: Option<String>,
    pub tasks: Vec<Held>,
    pub worktree: String,
    pub branch: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub pane: Option<String>,
    pub callback: Option<String>,
    pub chat: bool,
    pub state: &'static str,
    pub detail: Option<Detail>,
    pub health: Health,
    pub last_tool_call: Option<f64>,
    pub last_message: Option<Said>,
    /// Under a cap on an orchestrator's open tasks ([`crate::slots`]), the open slots it holds and
    /// the cap: for an orchestrator, and for any session open work counts against.
    pub slots: Option<Slots>,
}

/// The open slots a session holds against the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    pub open: usize,
    pub cap: usize,
}

/// The agents API's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub machine_work: super::machine_work::Snapshot,
    pub generated: f64,
    /// Whether hc could be asked (the `herdr` probe of the JSON, a name the wire keeps).
    pub herdr: bool,
    /// Whether the slot locks could be read.
    pub slots: bool,
    pub agents: Vec<Agent>,
}

fn ok(asked: bool) -> Py {
    Py::str(if asked { "ok" } else { "unavailable" })
}

impl View {
    /// The contract, in its key order.
    pub fn json(&self) -> Py {
        Py::obj(vec![
            ("generated", Py::Float(self.generated)),
            (
                "probes",
                Py::obj(vec![("herdr", ok(self.herdr)), ("slots", ok(self.slots))]),
            ),
            (
                "agents",
                Py::List(self.agents.iter().map(Agent::json).collect()),
            ),
            (
                "machine_work",
                super::machine_work::json(&self.machine_work),
            ),
        ])
    }
}

impl Agent {
    fn json(&self) -> Py {
        let held = self
            .tasks
            .iter()
            .map(|t| {
                Py::obj(vec![
                    ("slug", Py::str(&t.slug)),
                    ("role", Py::str(&t.role)),
                    ("status", Py::str(&t.status)),
                ])
            })
            .collect();
        let detail = self.detail.as_ref().map_or(Py::Null, |d| {
            Py::obj(vec![
                ("recipe", Py::str(&d.recipe)),
                ("phase", Py::str(&d.phase)),
                ("since", Py::opt_float(d.since)),
            ])
        });
        let said = self.last_message.as_ref().map_or(Py::Null, |m| {
            Py::obj(vec![
                ("slug", Py::str(&m.slug)),
                ("ts", Py::Float(m.ts)),
                ("text", Py::str(&m.text)),
            ])
        });
        let slots = self.slots.map_or(Py::Null, |s| {
            let count = |n: usize| Py::Int(i64::try_from(n).unwrap_or(i64::MAX));
            Py::obj(vec![("open", count(s.open)), ("cap", count(s.cap))])
        });
        Py::obj(vec![
            ("handle", Py::str(&self.handle)),
            ("kind", Py::str(&self.kind)),
            ("model", Py::str(&self.model)),
            ("parent", Py::opt_str(self.parent.as_deref())),
            ("task", Py::opt_str(self.task.as_deref())),
            ("role", Py::opt_str(self.role.as_deref())),
            ("tasks", Py::List(held)),
            ("worktree", Py::str(&self.worktree)),
            ("branch", Py::str(&self.branch)),
            ("agent", Py::opt_str(self.agent.as_deref())),
            ("session_id", Py::opt_str(self.session_id.as_deref())),
            ("pane", Py::opt_str(self.pane.as_deref())),
            ("callback", Py::opt_str(self.callback.as_deref())),
            ("chat", Py::Bool(self.chat)),
            ("state", Py::str(self.state)),
            ("state_detail", detail),
            ("health", Py::str(self.health.as_str())),
            ("last_tool_call", Py::opt_float(self.last_tool_call)),
            ("last_message", said),
            ("slots", slots),
        ])
    }
}

/// One session as the board and the probes place it, before any run is matched.
struct Row<'a> {
    session: &'a Session,
    held: Vec<(&'a Task, &'a str)>,
    primary: Option<(&'a Task, &'a str)>,
    record: Option<&'a TaskSessionRow>,
    pane: Option<&'a Pane>,
    worktree: String,
    branch: String,
}

/// The agents API's answer as of `now`: every session's agent, bar those that left (and, unless
/// `include_left`, those the board calls left that have neither a pane nor a run).
pub fn describe(
    store: &Store,
    probes: &Probes,
    now: f64,
    include_left: bool,
) -> Result<View, Error> {
    let board = store.board()?;
    describe_board(store, probes, now, include_left, &board)
}

/// Build the agents view from the server clock's existing board read.
pub fn describe_board(
    store: &Store,
    probes: &Probes,
    now: f64,
    include_left: bool,
    board: &crate::Board,
) -> Result<View, Error> {
    describe_board_observed(
        store,
        probes,
        now,
        include_left,
        board,
        super::machine_work::Evidence {
            local_host: crate::sys::hostname(),
            snapshot: super::machine_work::Snapshot::default(),
        },
    )
}

/// Production projection seam for a receiver's authenticated machine snapshot. Without that
/// transport evidence, describe_board retains local reservations and explicitly unknown capacity.
pub fn describe_board_observed(
    store: &Store,
    probes: &Probes,
    now: f64,
    include_left: bool,
    board: &crate::Board,
    evidence: super::machine_work::Evidence,
) -> Result<View, Error> {
    // A tick of the server clock asks hc once: its pane watch's sample, when it took one.
    let panes = store.clock_panes().unwrap_or_else(|| (probes.panes)()).ok();
    let runs = (probes.runs)().ok();
    let records = store.all_task_sessions()?;
    let bound = store.bound_agents()?;
    let spoke = store.last_messages()?;
    let callbacks = store.callbacks()?;
    let open_slots = store
        .slots()
        .map(|cap| (cap, crate::slots::Ledger::of_board(board).counts()));
    let mut held_by_handle = tasks_by_handle(&board.tasks);
    let mut by_handle: HashMap<&str, Vec<&TaskSessionRow>> = HashMap::new();
    for r in &records {
        by_handle.entry(r.handle.as_str()).or_default().push(r);
    }
    let resolved: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    let real = |path: &str| -> String {
        if let Some(found) = resolved.borrow().get(path) {
            return found.clone();
        }
        let found = resolve(path);
        resolved.borrow_mut().insert(path.into(), found.clone());
        found
    };
    let branch_of = |cwd: &str| (probes.branch_of)(cwd).unwrap_or_default();
    let some_panes = panes.as_deref().filter(|p| !p.is_empty());
    let rows: Vec<Row> = board
        .sessions
        .iter()
        .map(|session| {
            let handle = session.row.handle.as_str();
            let held = held_by_handle.remove(handle).unwrap_or_default();
            let primary = primary(&held);
            let mine = by_handle.get(handle).map(Vec::as_slice).unwrap_or(&[]);
            let on_primary: Vec<&TaskSessionRow> = mine
                .iter()
                .copied()
                .filter(|r| primary.is_some_and(|(t, _)| r.slug == t.slug))
                .collect();
            let active: Vec<_> = mine
                .iter()
                .copied()
                .filter(|r| r.closed.is_none())
                .collect();
            let record = active.last().or(on_primary.last()).or(mine.last()).copied();
            let pane = some_panes.and_then(|panes| {
                let bound = bound.get(handle).map(String::as_str);
                let active_record = record.filter(|r| r.closed.is_none());
                owned_pane(panes, active_record, bound, handle)
            });
            let pane_cwd = pane.map_or("", |p| p.cwd.as_str());
            let (worktree, branch) = match record {
                Some(r) => (r.worktree.clone(), r.branch.clone()),
                None if !pane_cwd.is_empty() => (pane_cwd.to_string(), branch_of(pane_cwd)),
                None => (session.row.worktree.clone(), session.row.branch.clone()),
            };
            Row {
                session,
                held,
                primary,
                record,
                pane,
                worktree,
                branch,
            }
        })
        .collect();
    // Every session, listed or not, so `include_left` changes who is shown and nothing else.
    let known: HashSet<&str> = rows.iter().map(|r| r.session.row.handle.as_str()).collect();
    let some_runs = runs.as_deref().filter(|r| !r.is_empty());
    let mut sharing: HashMap<String, usize> = HashMap::new();
    if some_runs.is_some() {
        for row in &rows {
            // A matching host label only adds a possible sharer; it never proves ownership.
            if row.record.is_none_or(|r| r.closed.is_none())
                && (row.pane.is_some()
                    || !evidence.local_host.is_empty()
                        && row.session.row.host == evidence.local_host)
            {
                *sharing.entry(real(&row.worktree)).or_default() += 1;
            }
        }
    }
    let local_owners: HashSet<&str> = rows
        .iter()
        .filter(|row| row.pane.is_some())
        .map(|row| row.session.row.handle.as_str())
        .chain(
            runs.as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|run| run.handle.as_deref()),
        )
        .collect();
    let mut machine_work = evidence.snapshot;
    super::machine_work::attribute(
        &mut machine_work,
        runs.as_deref().unwrap_or_default(),
        &records,
        &local_owners,
    );
    let mut agents = Vec::new();
    for row in rows {
        let s = row.session;
        let handle = s.row.handle.as_str();
        // A worktree several agents share (every one registered in the main checkout) says nothing
        // about which of them a run there belongs to: only a run tagged with the handle, or an agent
        // alone in its worktree, is matched.
        let run = some_runs.and_then(|runs| {
            let tagged: Vec<&Run> = runs
                .iter()
                .filter(|r| r.handle.as_deref() == Some(handle))
                .collect();
            let here = real(&row.worktree);
            if !tagged.is_empty()
                || sharing.get(&here) != Some(&1)
                || here.is_empty()
                || row.pane.is_none()
                || row.record.is_some_and(|r| r.closed.is_some())
            {
                return first(&tagged);
            }
            let in_it: Vec<&Run> = runs
                .iter()
                .filter(|r| r.handle.as_deref().is_none_or(|h| !known.contains(h)))
                .filter(|r| real(&r.worktree) == here)
                .collect();
            first(&in_it)
        });
        // Listed: not one that said it left; and, unless `include_left`, not one the board calls left
        // (quiet, nothing to take over) that has neither a pane nor a run, which says it is gone.
        // A native registration remains visible while idle, even without a pane or tool heartbeat.
        // Its health still describes the last activity; registration never invents a live beat.
        let gone = s.health == Health::Left
            && row.pane.is_none()
            && run.is_none()
            && !callbacks.contains_key(handle);
        if !include_left && (s.row.left_at.is_some() || gone) {
            continue;
        }
        let mut health = s.health;
        let (state, detail) = if let Some(run) = run {
            if matches!(health, Health::Stale | Health::Lost | Health::Unknown) {
                health = Health::Live;
            }
            let detail = Detail {
                recipe: run.recipe.clone(),
                phase: run.phase.clone(),
                since: run.since,
            };
            ("running_test", Some(detail))
        } else if let Some(state) = row
            .pane
            .and_then(|p| p.status.as_deref())
            .and_then(pane_state)
        {
            (state, None)
        } else {
            let state = match health {
                Health::Live => "working",
                Health::Stale => "idle",
                Health::Unknown => "unknown",
                _ => "gone",
            };
            (state, None)
        };
        let nonempty = |text: Option<&str>| text.filter(|t| !t.is_empty()).map(str::to_string);
        let session_id = nonempty(row.record.map(|r| {
            r.claude_session
                .strip_prefix("codex:")
                .unwrap_or(&r.claude_session)
        }))
        .or_else(|| nonempty(row.pane.and_then(|p| p.session.as_deref())))
        .or_else(|| nonempty(bound.get(handle).map(String::as_str)))
        .or_else(|| {
            callbacks
                .get(handle)
                .and_then(|callback| callback.address.strip_prefix("codex:"))
                .map(str::to_string)
        });
        let pane = row.pane.map(|p| p.id.clone());
        agents.push(Agent {
            handle: handle.into(),
            kind: s.row.kind.clone(),
            model: s.row.model.clone(),
            parent: s.row.parent.clone(),
            task: row.primary.map(|(t, _)| t.slug.clone()),
            role: row.primary.map(|(_, role)| role.to_string()),
            tasks: row
                .held
                .iter()
                .map(|(t, role)| Held {
                    slug: t.slug.clone(),
                    role: role.to_string(),
                    status: t.status.clone(),
                })
                .collect(),
            worktree: row.worktree,
            branch: row.branch,
            agent: row.pane.and_then(|p| p.agent.clone()),
            session_id,
            pane,
            callback: callbacks
                .get(handle)
                .map(|callback| callback.address.clone()),
            chat: callbacks.get(handle).is_some_and(|callback| callback.chat),
            state,
            detail,
            health,
            last_tool_call: s.row.last_tool,
            last_message: spoke.get(handle).map(|m| Said {
                slug: m.slug.clone(),
                ts: m.ts,
                text: cut(&m.text),
            }),
            slots: open_slots.as_ref().and_then(|(cap, counts)| {
                let open = counts.get(handle).copied().unwrap_or(0);
                (s.row.kind == "orchestrator" || open > 0).then_some(Slots { open, cap: *cap })
            }),
        });
    }
    Ok(View {
        machine_work,
        generated: now,
        herdr: panes.is_some(),
        slots: runs.is_some(),
        agents,
    })
}

/// Every agent that has a slot run going, handle -> what it runs, from one view: the monitor asks this
/// once a poll, only when a child is quiet, never once per child.
pub fn running_tests(store: &Store, probes: &Probes) -> Result<BTreeMap<String, Detail>, Error> {
    let view = describe(store, probes, store.now(), false)?;
    Ok(view
        .agents
        .into_iter()
        .filter(|a| a.state == "running_test")
        .filter_map(|a| a.detail.map(|d| (a.handle, d)))
        .collect())
}

/// The same answer as columns, for the terminal.
pub fn table(view: &View) -> Vec<String> {
    view.agents
        .iter()
        .map(|a| {
            let state = match &a.detail {
                Some(d) => format!("{} ({} {})", a.state, d.phase, d.recipe),
                None => a.state.to_string(),
            };
            let or_dash = |v: Option<&str>| v.filter(|v| !v.is_empty()).unwrap_or("-").to_string();
            format!(
                "{:<18} {:<12} {:<17} {:<28} {:<9} {:<24} {}",
                a.handle,
                a.kind,
                state,
                or_dash(a.task.as_deref()),
                or_dash(a.role.as_deref()),
                or_dash(Some(&a.branch)),
                or_dash(a.pane.as_deref()),
            )
        })
        .collect()
}
