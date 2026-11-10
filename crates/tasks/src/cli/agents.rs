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
//! sharing a worktree. A quiet in-process agent with no run is judged by what works in the worktree
//! its binding recorded (`agents/place.rs`).

mod place;

use crate::health::{FINISHED, Health};
use crate::pyjson::Py;
use crate::rows::TaskSessionRow;
use crate::{Binding, Error, Session, Store, Task};
pub use place::{Began, PROCESS, PROCESS_SCAN_SECONDS, Process, ProcessScan};
use place::{Stay, resolve, waited_on};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    /// HC's observation counter, increased when status leaves working; not a prompt id.
    pub turn: Option<u64>,
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
/// slot runs, the git branch checked out in a folder, and the working directory of every process.
pub struct Probes<'a> {
    pub panes: Probe<'a, Vec<Pane>>,
    pub runs: Probe<'a, Vec<Run>>,
    pub branch_of: BranchOf<'a>,
    /// Every live process the machine lists, where it works and when it started, asked only for an
    /// in-process agent gone quiet in a worktree of its own.
    pub processes: Probe<'a, ProcessScan>,
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

fn rank(role: &str) -> u8 {
    match role {
        "executor" => 0,
        "reviewer" => 1,
        "reserved" => 2,
        _ => 9,
    }
}

fn pane_state(status: &str) -> Option<&'static str> {
    match status {
        "working" => Some("working"),
        "blocked" => Some("waiting_on_input"),
        "idle" | "done" => Some("idle"),
        _ => None,
    }
}

/// Index roles once, rather than scanning every task for every historical session on each tick.
fn tasks_by_handle(tasks: &[Task]) -> HashMap<&str, Vec<(&Task, &str)>> {
    let mut held = HashMap::<&str, Vec<(&Task, &str)>>::new();
    for task in tasks {
        let mut roles = HashMap::<&str, &str>::new();
        for role in task.roles.iter().filter(|r| r.role != "orchestrator") {
            let best = roles.entry(&role.handle).or_insert(&role.role);
            if rank(&role.role) < rank(best) {
                *best = &role.role;
            }
        }
        for (handle, role) in roles {
            held.entry(handle).or_default().push((task, role));
        }
    }
    held
}

/// The task it works now: an open one by role, priority and slug; else the finished one updated last.
fn primary<'t>(held: &[(&'t Task, &'t str)]) -> Option<(&'t Task, &'t str)> {
    let open = held
        .iter()
        .filter(|(t, _)| !FINISHED.contains(&t.status.as_str()))
        .min_by(|(a, ra), (b, rb)| {
            (rank(ra), a.priority, &a.slug).cmp(&(rank(rb), b.priority, &b.slug))
        });
    if let Some(found) = open {
        return Some(*found);
    }
    let mut last: Option<(&Task, &str)> = None;
    for &(t, role) in held {
        if last.is_none_or(|(l, _)| t.updated > l.updated) {
            last = Some((t, role));
        }
    }
    last
}

/// Of `runs`, a holding one before a waiting one, the earliest first.
fn first<'r>(runs: &[&'r Run]) -> Option<&'r Run> {
    let holding: Vec<&Run> = runs
        .iter()
        .copied()
        .filter(|r| r.phase == "holding")
        .collect();
    let pool = if holding.is_empty() {
        runs
    } else {
        holding.as_slice()
    };
    let since = |r: &Run| r.since.unwrap_or(0.0);
    let mut found: Option<&Run> = None;
    for &r in pool {
        if found.is_none_or(|f| since(r) < since(f)) {
            found = Some(r);
        }
    }
    found
}

/// The pane that is this agent's: its recorded one, else one running its conversation, else one
/// named for its handle.
pub(crate) fn find_pane<'p>(
    panes: &'p [Pane],
    record: Option<&TaskSessionRow>,
    bound: Option<&str>,
    handle: &str,
) -> Option<&'p Pane> {
    if let Some(record) = record
        && let Some(pane) = panes.iter().rev().find(|p| p.id == record.pane)
    {
        return Some(pane);
    }
    let wanted: Vec<&str> = [
        record.map(|r| {
            r.claude_session
                .strip_prefix("codex:")
                .unwrap_or(&r.claude_session)
        }),
        bound,
    ]
    .into_iter()
    .flatten()
    .filter(|w| !w.is_empty())
    .collect();
    if let Some(pane) = panes
        .iter()
        .find(|p| p.session.as_deref().is_some_and(|s| wanted.contains(&s)))
    {
        return Some(pane);
    }
    panes
        .iter()
        .find(|p| p.title.as_deref() == Some(handle) || p.name.as_deref() == Some(handle))
}

/// Its words on one line, its whitespace as Python's `str.split` finds it, cut to [`MESSAGE_CHARS`].
fn cut(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.chars().count() <= MESSAGE_CHARS {
        return line;
    }
    let mut short: String = line.chars().take(MESSAGE_CHARS).collect();
    short.push('…');
    short
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
    /// An in-process agent's binding, when `worktree` is the one it recorded: when it came there,
    /// by the board's clock and the clock since boot, and the earliest time a call of its there read
    /// (earlier only if the clock went back).
    placed: Option<&'a Binding>,
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
    // A tick of the server clock asks hc once: its pane watch's sample, when it took one.
    let panes = store.clock_panes().unwrap_or_else(|| (probes.panes)()).ok();
    let runs = (probes.runs)().ok();
    let records = store.all_task_sessions()?;
    let bindings = store.agent_bindings()?;
    let bound = |handle: &str| bindings.get(handle).map(|b| b.agent.as_str());
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
            let record = on_primary.last().or(mine.last()).copied();
            let pane = some_panes.and_then(|panes| find_pane(panes, record, bound(handle), handle));
            let pane_cwd = pane.map_or("", |p| p.cwd.as_str());
            // A subagent is registered where its orchestrator ran `register`; its binding records
            // the checkout its own tool calls run in.
            let placed = bindings
                .get(handle)
                .filter(|b| b.in_process && !b.worktree.is_empty());
            let (worktree, branch) = match (record, placed) {
                (Some(r), _) => (r.worktree.clone(), r.branch.clone()),
                (None, _) if !pane_cwd.is_empty() => (pane_cwd.to_string(), branch_of(pane_cwd)),
                (None, Some(b)) => (b.worktree.clone(), b.branch.clone()),
                (None, None) => (session.row.worktree.clone(), session.row.branch.clone()),
            };
            Row {
                session,
                held,
                primary,
                record,
                pane,
                worktree,
                branch,
                placed: placed.filter(|_| record.is_none() && pane.is_none()),
            }
        })
        .collect();
    // Every session, listed or not, so `include_left` changes who is shown and nothing else.
    let known: HashSet<&str> = rows.iter().map(|r| r.session.row.handle.as_str()).collect();
    let some_runs = runs.as_deref().filter(|r| !r.is_empty());
    let mut sharing: HashMap<String, usize> = HashMap::new();
    if some_runs.is_some() {
        for row in &rows {
            *sharing.entry(real(&row.worktree)).or_default() += 1;
        }
    }
    // The machine's processes are listed at most once a view, and only for a quiet in-process agent.
    let scanned: OnceCell<Answer<ProcessScan>> = OnceCell::new();
    let scan = || scanned.get_or_init(|| (probes.processes)()).clone();
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
            if !tagged.is_empty() || sharing[&here] != 1 || here.is_empty() {
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
        let quiet = matches!(health, Health::Stale | Health::Lost | Health::Unknown);
        // An in-process agent makes no tool call while it waits on what it started in the
        // background: a process it started, still working in its own worktree, is that wait.
        let waiting = match (run, row.placed) {
            (None, Some(b)) if quiet => {
                let stay = Stay {
                    came: b.placed_at,
                    from: b.stay_from,
                    last: s.row.last_tool.unwrap_or(b.placed_at),
                    came_boot: b.placed_boot.as_ref(),
                    last_boot: b.last_boot.as_ref(),
                    boot_uncertain: b.boot_uncertain,
                };
                waited_on(&real(&row.worktree), &stay, &scan)
            }
            _ => None,
        };
        let (state, detail) = if let Some(run) = run {
            if quiet {
                health = Health::Live;
            }
            let detail = Detail {
                recipe: run.recipe.clone(),
                phase: run.phase.clone(),
                since: run.since,
            };
            ("running_test", Some(detail))
        } else if let Some((waits, detail)) = waiting {
            health = waits;
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
        .or_else(|| nonempty(bound(handle)))
        .or_else(|| {
            callbacks
                .get(handle)
                .and_then(|callback| callback.address.strip_prefix("codex:"))
                .map(str::to_string)
        });
        let pane = match row.pane {
            Some(p) => Some(p.id.clone()),
            None => nonempty(row.record.map(|r| r.pane.as_str())),
        };
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
