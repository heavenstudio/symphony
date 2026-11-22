//! Who is doing what: the agents API (`/api/agents`, `just tasks agents [--json]`).
//!
//! The board is the record. Each agent is a board session; its task sessions say which task it runs, in
//! which worktree and branch, as which Claude session and in which hc terminal. hc's terminals and the
//! test-slot locks ([`super::probes`]) are probes that say what each agent is doing at this moment;
//! they feed the answer, never replace the record, and a probe that cannot be asked is named
//! `unavailable` with the answer still built from the board.
//!
//! The JSON is a contract, listed in `docs/BOARD_API.md#the-agents-api`:
//!
//! ```text
//! {"generated": ts, "probes": {"herdr": "ok"|"unavailable", "slots": "ok"|"unavailable"}, "agents": [{
//!     "handle", "kind", "model", "parent",
//!     "task", "role",                 # the task it works now (an open one first) and its role there
//!     "tasks": [{"slug", "role", "status"}],     # every task it holds a role on, bar orchestrating
//!     "following",                    # how many open tasks it orchestrates
//!     "worktree", "branch",           # where it really works (its task session, else its pane)
//!     "agent", "session_id", "pane",  # claude|codex, its conversation id, its hc terminal
//!     "state", "state_detail",
//!     "health",                       # the board's, except that a run in progress reads live
//!     "last_tool_call",               # when a tool call of its agent last beat (null before one)
//!     "last_message": {"slug", "ts", "text"} | null,
//!     "slots": {"open", "cap"} | null,
//!     "session_url",                  # the web address its session gave, else null
//!     "in_process",                   # a subagent: it has no pane, its parent's session holds it
//!     "tokens": {"input", "cache_write_5m", "cache_write_1h", "cache_read", "output", "calls"}
//!         | null,                     # what its model calls took, all told, as its beats said
//!     "cost": {"dollars", "input", "cache_write_5m", "cache_write_1h", "cache_read", "output",
//!              "unpriced": [{"model", "speed", "tokens"}], "prices"}
//!         | null}]}                   # their cost estimated at API list prices read on `prices`
//! ```
//!
//! `state` is one of `working`, `running_test` (its worktree holds a test slot or is queued for one;
//! `state_detail` says `{"recipe", "phase": "holding"|"waiting", "since"}`), `waiting_on_input` (hc
//! sees an approval or question), `idle`, `gone` (no pane and no sign of life). An agent with no pane
//! whose listener the idle sweep confirms is `idle` once it is at rest ([`at_rest`]): the
//! confirmation keeps its health live, so its last tool call says what its health cannot. An agent waiting on its
//! own verify is `running_test` and reads live, never lost. A run is the agent's when tagged with its
//! handle, else by exact worktree to an agent alone in it: never by a directory prefix, never to agents
//! sharing a worktree. A quiet in-process agent with no run is judged by what works in the worktree
//! its binding recorded (`agents/place.rs`).

mod place;

use crate::health::{Health, LIVE_SECONDS};
use crate::json_object::object;
use crate::rows::SessionRow;
use crate::rows::TaskSessionRow;
use crate::usage::{Cost, Prices, Tokens};
use crate::{Binding, Error, Session, Store, Task};
pub use place::{Began, PROCESS, PROCESS_SCAN_SECONDS, Process, ProcessScan};
use place::{Stay, resolve, waited_on};
use serde_json::Value;
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
mod attribution;
mod selection;
pub(crate) use attribution::find_pane;
use attribution::{cut, fallback_owners, owned_pane};
use selection::{first, following_by_handle, pane_state, primary, tasks_by_handle};

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
    /// How many open tasks it orchestrates (neither done nor cancelled), which `tasks` leaves out:
    /// what the agents page's Following list counts.
    pub following: usize,
    pub worktree: String,
    pub branch: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub pane: Option<String>,
    pub callback: Option<String>,
    pub chat: bool,
    /// Its listener answered the idle sweep's last round, so no STALE or LOST line is said of it
    /// while it does. Not on the wire.
    pub confirmed: bool,
    pub state: &'static str,
    pub detail: Option<Detail>,
    pub health: Health,
    pub last_tool_call: Option<f64>,
    pub last_message: Option<Said>,
    /// Under a cap on an orchestrator's open tasks ([`crate::slots`]), the open slots it holds and
    /// the cap: for an orchestrator, and for any session open work counts against.
    pub slots: Option<Slots>,
    /// The web address its session gave (`register --session-url`, `callback --session-url`):
    /// where its Go to session control links ([`crate::session_url`]).
    pub session_url: Option<String>,
    /// It runs inside another session (a subagent): its binding says so. Such an agent has no pane
    /// of its own, so its Go to session control goes to its parent's.
    pub in_process: bool,
    /// What its model calls took, all told, as its beats recorded them ([`crate::usage`]); none
    /// before one recorded any.
    pub tokens: Option<Tokens>,
    /// What those tokens cost, estimated at the API list prices every build embeds
    /// ([`crate::usage::price`]); none beside no tokens.
    pub cost: Option<Cost>,
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

fn ok(asked: bool) -> Value {
    Value::from(if asked { "ok" } else { "unavailable" })
}

impl View {
    /// The contract, in its key order.
    pub fn json(&self) -> Value {
        object(vec![
            ("generated", Value::from(self.generated)),
            (
                "probes",
                object(vec![("herdr", ok(self.herdr)), ("slots", ok(self.slots))]),
            ),
            (
                "agents",
                Value::Array(self.agents.iter().map(Agent::json).collect()),
            ),
            (
                "machine_work",
                super::machine_work::json(&self.machine_work),
            ),
        ])
    }
}

impl Agent {
    fn json(&self) -> Value {
        let held = self
            .tasks
            .iter()
            .map(|t| {
                object(vec![
                    ("slug", Value::from(t.slug.as_str())),
                    ("role", Value::from(t.role.as_str())),
                    ("status", Value::from(t.status.as_str())),
                ])
            })
            .collect();
        let detail = self.detail.as_ref().map_or(Value::Null, |d| {
            object(vec![
                ("recipe", Value::from(d.recipe.as_str())),
                ("phase", Value::from(d.phase.as_str())),
                ("since", Value::from(d.since)),
            ])
        });
        let said = self.last_message.as_ref().map_or(Value::Null, |m| {
            object(vec![
                ("slug", Value::from(m.slug.as_str())),
                ("ts", Value::from(m.ts)),
                ("text", Value::from(m.text.as_str())),
            ])
        });
        let count = |n: usize| Value::from(i64::try_from(n).unwrap_or(i64::MAX));
        let slots = self.slots.map_or(Value::Null, |s| {
            object(vec![("open", count(s.open)), ("cap", count(s.cap))])
        });
        object(vec![
            ("handle", Value::from(self.handle.as_str())),
            ("kind", Value::from(self.kind.as_str())),
            ("model", Value::from(self.model.as_str())),
            ("parent", Value::from(self.parent.as_deref())),
            ("task", Value::from(self.task.as_deref())),
            ("role", Value::from(self.role.as_deref())),
            ("tasks", Value::Array(held)),
            ("following", count(self.following)),
            ("worktree", Value::from(self.worktree.as_str())),
            ("branch", Value::from(self.branch.as_str())),
            ("agent", Value::from(self.agent.as_deref())),
            ("session_id", Value::from(self.session_id.as_deref())),
            ("pane", Value::from(self.pane.as_deref())),
            ("callback", Value::from(self.callback.as_deref())),
            ("chat", Value::from(self.chat)),
            ("state", Value::from(self.state)),
            ("state_detail", detail),
            ("health", Value::from(self.health.as_str())),
            ("last_tool_call", Value::from(self.last_tool_call)),
            ("last_message", said),
            ("slots", slots),
            ("session_url", Value::from(self.session_url.as_deref())),
            ("in_process", Value::from(self.in_process)),
            (
                "tokens",
                self.tokens.as_ref().map_or(Value::Null, Tokens::json),
            ),
            (
                "cost",
                (self.cost.as_ref()).map_or(Value::Null, |c| c.json(&Prices::embedded().read)),
            ),
        ])
    }
}

/// Whether a session whose listener answers is at rest at `now`: outside any tool call, its last
/// one more than [`LIVE_SECONDS`] ago, the window its health would call it live for without the
/// confirmation. Before its first tool call, when it connected stands for that call. Never an
/// orchestrator or the chat, which wait at their prompts as their work.
fn at_rest(session: &SessionRow, now: f64) -> bool {
    !matches!(session.kind.as_str(), "orchestrator" | "chat")
        && !session.busy_until.is_some_and(|until| now <= until)
        && now - session.last_tool.unwrap_or(session.connected) > LIVE_SECONDS
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
    let bindings = store.agent_bindings()?;
    let bound = |handle: &str| bindings.get(handle).map(|b| b.agent.as_str());
    let spoke = store.last_messages()?;
    let callbacks = store.callbacks()?;
    let web = store.session_urls()?;
    // A task a custom workflow holds counts by its position's slot policy.
    let operating = store.operating_for(&board.tasks)?;
    let open_slots = store.slots().map(|cap| {
        let ledger = crate::slots::Ledger::of_board_on(board, operating.as_ref());
        (cap, ledger.counts())
    });
    let mut held_by_handle = tasks_by_handle(&board.tasks);
    let following = following_by_handle(&board.tasks);
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
                let bound = bound(handle);
                let active_record = record.filter(|r| r.closed.is_none());
                owned_pane(panes, active_record, bound, handle)
            });
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
    let fallbacks = if some_runs.is_some() {
        fallback_owners(&rows, &evidence.local_host, &real)
    } else {
        HashMap::new()
    };
    let mut machine_work = evidence.snapshot;
    super::machine_work::attribute(
        &mut machine_work,
        runs.as_deref().unwrap_or_default(),
        &records,
        &fallbacks,
        &real,
    );
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
            if !tagged.is_empty() {
                return first(&tagged);
            }
            let here = real(&row.worktree);
            if fallbacks.get(&here).copied() != Some(handle) {
                return None;
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
        let quiet = health.quiet();
        let confirmed = callbacks.get(handle).is_some_and(|c| c.confirmed(now));
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
                // A confirmed listener keeps its session seen, so its health cannot say it rests.
                Health::Live | Health::Stale if confirmed && at_rest(&s.row, now) => "idle",
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
                .strip_prefix(crate::launch::CODEX)
                .unwrap_or(&r.claude_session)
        }))
        .or_else(|| nonempty(row.pane.and_then(|p| p.session.as_deref())))
        .or_else(|| nonempty(bound(handle)))
        .or_else(|| {
            callbacks
                .get(handle)
                .and_then(|callback| callback.address.strip_prefix(crate::launch::CODEX))
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
            following: following.get(handle).copied().unwrap_or(0),
            worktree: row.worktree,
            branch: row.branch,
            agent: row.pane.and_then(|p| p.agent.clone()),
            session_id,
            pane,
            callback: callbacks
                .get(handle)
                .map(|callback| callback.address.clone()),
            chat: callbacks.get(handle).is_some_and(|callback| callback.chat),
            confirmed,
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
            session_url: web.get(handle).cloned(),
            in_process: bindings.get(handle).is_some_and(|b| b.in_process),
            tokens: None,
            cost: None,
        });
    }
    // What each listed agent's model calls took, all told, and their cost, in one read: a view
    // without it still answers, with none.
    let handles: Vec<&str> = agents.iter().map(|a| a.handle.as_str()).collect();
    let mut spent = store.spent_by(&handles).unwrap_or_default();
    for agent in &mut agents {
        if let Some(spend) = spent.remove(&agent.handle) {
            agent.tokens = Some(spend.tokens);
            agent.cost = Some(spend.cost);
        }
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

/// The same answer as columns under a header, for the terminal: what each agent does (its state)
/// beside the board's health of it, in the words `who` says it in; none for a view of nobody.
pub fn table(view: &View) -> Vec<String> {
    if view.agents.is_empty() {
        return Vec::new();
    }
    let header = [
        "HANDLE", "KIND", "STATE", "HEALTH", "TASK", "ROLE", "BRANCH", "TERMINAL",
    ];
    let mut rows = vec![header.iter().map(|h| h.to_string()).collect::<Vec<_>>()];
    rows.extend(view.agents.iter().map(|a| {
        let state = match &a.detail {
            Some(d) => format!("{} ({} {})", a.state, d.phase, d.recipe),
            None => a.state.to_string(),
        };
        let or_dash = |v: Option<&str>| v.filter(|v| !v.is_empty()).unwrap_or("-").to_string();
        vec![
            a.handle.clone(),
            a.kind.clone(),
            state,
            a.health.as_str().to_string(),
            or_dash(a.task.as_deref()),
            or_dash(a.role.as_deref()),
            or_dash(Some(&a.branch)),
            or_dash(a.pane.as_deref()),
        ]
    }));
    super::text::columns(&rows)
}
