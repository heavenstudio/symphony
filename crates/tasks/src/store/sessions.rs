//! Sessions: registering, being seen, a Claude Code agent's tool calls beating for a handle, leaving
//! and coming back.

use super::postgres::sessions as pg;
use super::statement::both;
use super::surreal::sessions as sql;
use super::work::Pass;
use super::{Beside, Error, Failure, Found, REFUSED, Session, Store, refused, repr};
use crate::health::{self, EXECUTING};
use crate::lock::Recovered as _;
use crate::maps::Collect as _;
use crate::orchestration;
use crate::rows::SessionRow;
use crate::sys::Boottime;
use crate::usage::Spent;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};
use std::sync::Mutex;

/// What a session is; a `filer` is a tool's session that files reds ([`crate::red::FILER`]).
pub const KINDS: [&str; 6] = [
    "orchestrator",
    "worker",
    "reviewer",
    "chat",
    "app",
    crate::red::FILER,
];

const ADJECTIVES: [&str; 32] = [
    "amber", "azure", "brisk", "calm", "cobalt", "coral", "dusky", "ember", "fleet", "gilded",
    "glacial", "hazel", "ivory", "jade", "keen", "lucid", "lunar", "misty", "nimble", "ochre",
    "opal", "quiet", "rapid", "russet", "sable", "silver", "slate", "solar", "swift", "teal",
    "umber", "vivid",
];
const NOUNS: [&str; 30] = [
    "heron", "finch", "otter", "lynx", "wren", "kestrel", "marten", "ibis", "crane", "swift",
    "tern", "plover", "raven", "egret", "falcon", "gecko", "hare", "ferret", "osprey", "stoat",
    "vole", "lark", "robin", "owl", "badger", "sparrow", "petrel", "curlew", "dunlin", "merlin",
];

/// A session as it registers (`Store.register`'s arguments).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewSession {
    pub kind: String,
    pub parent: Option<String>,
    pub worktree: String,
    pub branch: String,
    pub model: String,
    /// Its monitor's process, if it has one.
    pub pid: Option<i64>,
    pub host: String,
    /// The areas it prefers work in.
    pub affinity: Vec<String>,
}

impl NewSession {
    pub fn new(kind: &str) -> NewSession {
        NewSession {
            kind: kind.into(),
            ..NewSession::default()
        }
    }
}

/// A tool call of a bound agent (`Store.agent_beat`'s arguments).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Beat {
    /// The call may run this long: the session reads live until then.
    pub busy_for: Option<f64>,
    /// The tool call's id.
    pub call: Option<String>,
    /// The call has returned.
    pub ended: bool,
}

/// The Claude Code key a session's tool calls beat through, as [`Store::agent_bindings`] reads it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Binding {
    /// The Claude Code session or agent id.
    pub agent: String,
    /// The key is an in-process agent's (a subagent's), not a full session's.
    pub in_process: bool,
    /// Where an in-process agent's tool calls run, as its last tool call's hook saw it: the top
    /// folder of that checkout, "" before one was seen. Its session row keeps where it registered
    /// (the main checkout, for a subagent its orchestrator registered).
    pub worktree: String,
    /// The branch checked out there then, "" when detached.
    pub branch: String,
    /// When its tool calls were first seen in that checkout, by the board's clock (0 before).
    pub placed_at: f64,
    /// The earliest time any of its calls in that checkout read: `placed_at`, or earlier when the
    /// clock went back while it was there, which leaves a process's start in that span in doubt.
    pub stay_from: f64,
    /// The clock since boot when its calls were first seen in that checkout, as that call read it:
    /// none where no start is dated on that clock (macOS) or the call did not read it. Setting the
    /// wall clock moves neither this nor a start Linux dates on it.
    pub placed_boot: Option<Boottime>,
    /// The clock since boot at its latest tool call, there or anywhere (the latest reading its calls
    /// gave, a new boot's replacing the last boot's): none when that call did not read it.
    pub last_boot: Option<Boottime>,
    /// A call in this checkout stay could not read the boot clock. Subsequent samples can prove
    /// ownership, but cannot bound absence: they may have been sampled before that unreadable call
    /// and delivered later. A new checkout stay starts the evidence again.
    pub boot_uncertain: bool,
}

/// Where a new session's handle comes from: Python's `random.Random`, as `register` draws on it.
pub trait Pick: Send {
    /// An index below `len` (`choice`).
    fn index(&mut self, len: usize) -> usize;
    /// A number from `from` up to, not including, `to` (`randrange`).
    fn number(&mut self, from: i64, to: i64) -> i64;
}

/// xorshift64*, seeded from this process's hash keys: enough to spread handles, nothing more.
pub(super) struct Random(u64);

impl Random {
    pub(super) fn seeded() -> Random {
        Random(RandomState::new().hash_one(std::process::id()) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl Pick for Random {
    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.next() % len.max(1) as u64).unwrap_or(0)
    }

    fn number(&mut self, from: i64, to: i64) -> i64 {
        let span = u64::try_from(to - from).unwrap_or(0).max(1);
        from + i64::try_from(self.next() % span).unwrap_or(0)
    }
}

impl Store {
    /// The handles of new sessions come from `pick`.
    pub fn picking(self, pick: Box<dyn Pick>) -> Store {
        Store {
            pick: Mutex::new(pick),
            ..self
        }
    }

    pub fn register(&self, new: &NewSession) -> Result<Session, Error> {
        if !KINDS.contains(&new.kind.as_str()) {
            return Err(refused(format!("kind must be one of {}", KINDS.join(", "))));
        }
        let affinity: Vec<&str> = new
            .affinity
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .collect();
        let orphan = new
            .parent
            .as_ref()
            .map(|parent| format!("{REFUSED}no parent session {}", repr(parent)));
        let (handle, found) = self.retrying(|| {
            let handles = both(sql::HANDLES, pg::HANDLES);
            let taken: Vec<String> = self.transact(handles, json!({}))?;
            let handle = self
                .new_handle(&taken.into_iter().collect())
                .map_err(Failure::Stop)?;
            let now = (self.clock)();
            let row = SessionRow {
                handle: handle.clone(),
                kind: new.kind.clone(),
                parent: new.parent.clone(),
                worktree: new.worktree.clone(),
                branch: new.branch.clone(),
                model: new.model.clone(),
                pid: new.pid,
                host: new.host.clone(),
                affinity: affinity.join(","),
                connected: now,
                last_seen: now,
                ..SessionRow::default()
            };
            let vars = json!({"handle": handle, "row": row, "parent": new.parent, "orphan": orphan,
                              "executing": EXECUTING});
            let found: Found = self.transact(both(sql::register(), pg::register()), vars)?;
            Ok((handle, found))
        })?;
        self.found(&handle, found)
    }

    fn new_handle(&self, taken: &HashSet<String>) -> Result<String, Error> {
        let mut pick = self.pick.lock().recover();
        for attempt in 0..200 {
            let adjective = ADJECTIVES[pick.index(ADJECTIVES.len()) % ADJECTIVES.len()];
            let mut handle = format!(
                "{adjective}-{}",
                NOUNS[pick.index(NOUNS.len()) % NOUNS.len()]
            );
            if attempt >= 50 {
                let _ = write!(handle, "-{}", pick.number(2, 1000));
            }
            if !taken.contains(&handle) {
                return Ok(handle);
            }
        }
        Err(refused("could not find a free session handle".into()))
    }

    /// The session is seen (the monitor's beat).
    pub fn touch(&self, handle: &str) -> Result<(), Error> {
        let body = sql::touch();
        let pg = pg::touch();
        let vars = self.acting(handle, (self.clock)(), json!({}));
        self.retrying(|| self.transact::<Value>(both(&body, pg.clone()), vars.clone()))
            .map(|_| ())
    }

    /// The session is seen, as [`Store::touch`] says it, by one try alone: what the recorder of a
    /// run says of the session its record names while the run goes on (`cli/recorder.rs`). A board
    /// moving under it is not waited for: the recorder says it again at its next time.
    pub(crate) fn seen_once(&self, handle: &str) -> Result<(), Error> {
        let vars = self.acting(handle, (self.clock)(), json!({}));
        match self.transact::<Value>(both(sql::touch(), pg::touch()), vars) {
            Ok(_) => Ok(()),
            Err(Failure::Again) => Err(Error::Db("the board moved under it".into())),
            Err(Failure::Stop(error)) => Err(error),
        }
    }

    /// Let the tool calls of Claude Code agent `agent` beat for `handle` (`agent_beat`). One agent
    /// beats for one handle, the last it named. Never a session that left, nor one with a monitor of
    /// its own (an orchestrator, a main session): its monitor already beats for it, and a subagent
    /// acting in its name must not keep it alive.
    ///
    /// `session`: the key is a full session's id, not a subagent's. Such a session keeps the first
    /// handle it names: naming another (registering or relaying for a child) moves nothing. One that
    /// names a monitored handle is a monitored session: it lets go of what it held and is barred, so
    /// what it names after (a child's handle) is never kept alive by its calls.
    /// An in-process call cannot replace or reclassify a binding recorded as a full session,
    /// including before that session registers a native callback.
    pub fn bind_agent(&self, agent: &str, handle: &str, session: bool) -> Result<bool, Error> {
        #[derive(Deserialize)]
        struct Found {
            row: Option<SessionRow>,
            barred: bool,
            bound: Vec<String>,
            in_process: Option<bool>,
            held_in_process: Option<bool>,
            native: bool,
        }
        self.retrying(|| {
            let found: Found = self.transact(
                both(sql::BINDING, pg::binding()),
                json!({"agent": agent, "handle": handle}),
            )?;
            let Some(row) = found.row else {
                return Ok(false);
            };
            if !session
                && (found.native
                    || found.in_process == Some(false)
                    || found.held_in_process == Some(false))
            {
                return Ok(false);
            }
            if row.agent.as_deref() == Some(agent) {
                if found.in_process == Some(true) || (session && found.in_process == Some(false)) {
                    return Ok(true);
                }
                self.transact::<Value>(
                    both(sql::rebind(), pg::rebind()),
                    json!({"agent":agent, "handle":handle, "full":session,
                           "pid":row.pid, "host":row.host}),
                )?;
                return Ok(true);
            }
            if session && found.barred {
                return Ok(false);
            }
            let monitored = self.monitored(&row);
            let bound = if session && monitored {
                false
            } else if row.left_at.is_some() || monitored || (session && !found.bound.is_empty()) {
                return Ok(false);
            } else {
                true
            };
            let vars = json!({
                "agent": agent, "handle": handle, "full": session, "now": (self.clock)(),
                "pid": row.pid, "host": row.host,
            });
            let write = if bound { sql::bind() } else { sql::bar() };
            let pg = if bound { pg::bind() } else { pg::bar() };
            self.transact::<Value>(both(&write, pg), vars)?;
            Ok(bound)
        })
    }

    /// The session has a monitor that may be running: a live local pid, or one on a host this store
    /// cannot probe.
    fn monitored(&self, row: &SessionRow) -> bool {
        row.pid
            .is_some_and(|pid| pid != 0 && (row.host != self.host || (self.pid_alive)(pid)))
    }

    /// A tool call of `agent` counts as a sign of life for the handle bound to it: that handle.
    ///
    /// `busy_for`: tool call `call` may run this long, and the session reads live until then; of
    /// calls running side by side, the mark that ends latest is kept. `ended`: call `call` has
    /// returned, which clears the mark only if that call set it. A plain beat leaves the mark alone.
    pub fn agent_beat(&self, agent: &str, beat: &Beat) -> Result<Option<String>, Error> {
        self.agent_beat_in(agent, beat, None, None)
    }

    /// [`Store::agent_beat`], from the checkout the tool call runs in, if it is known: its top folder
    /// and the branch checked out there; and with the clock since boot as the call read it, if it did
    /// ([`Boottime`]). An in-process agent's binding keeps them as where it really works (a
    /// subagent's session is registered where its orchestrator ran `register`), and when it came
    /// there, by the board's clock and the clock since boot, written only when they change; a call
    /// there that reads a time before any other there did (the clock went back) lowers
    /// [`Binding::stay_from`]; and each call, there or not, moves [`Binding::last_boot`] on while
    /// preserving ordering evidence and any unreadable-clock uncertainty. A full
    /// session's binding keeps none: its terminal or launch says where it works.
    pub fn agent_beat_in(
        &self,
        agent: &str,
        beat: &Beat,
        place: Option<(&str, &str)>,
        boot: Option<&Boottime>,
    ) -> Result<Option<String>, Error> {
        self.agent_beat_spending(agent, beat, place, boot, &[])
    }

    /// [`Store::agent_beat_in`], adding `spent`, the tokens the agent's model calls took since its
    /// last beat, to the handle bound to it, in the same transaction: on the row of the task and
    /// role it works now, the model, the speed and the task's round ([`crate::usage`]). With no
    /// handle bound, nothing is added; the answer, the handle, says whether it was.
    pub fn agent_beat_spending(
        &self,
        agent: &str,
        beat: &Beat,
        place: Option<(&str, &str)>,
        boot: Option<&Boottime>,
        spent: &[Spent],
    ) -> Result<Option<String>, Error> {
        let place = place.map(|(worktree, branch)| json!({"worktree": worktree, "branch": branch}));
        let mut vars = json!({
            "agent": agent, "now": (self.clock)(), "busy_for": beat.busy_for, "call": beat.call,
            "ended": beat.ended, "place": place, "boot": boot,
        });
        let spends = !spent.is_empty();
        if spends {
            super::spending::vars(&mut vars, spent);
        }
        let body = sql::beat(spends);
        let pg = pg::beat(spends);
        self.retrying(|| self.transact(both(&body, pg.clone()), vars.clone()))
    }

    /// Take an existing handle back (a re-armed monitor), keeping every role it holds.
    pub fn reattach(&self, handle: &str, pid: Option<i64>) -> Result<Session, Error> {
        let host = if pid.is_some_and(|pid| pid != 0) {
            self.host.as_str()
        } else {
            ""
        };
        let body = sql::reattach();
        let pg = pg::reattach();
        let vars = self.acting(handle, (self.clock)(), json!({"pid": pid, "host": host}));
        self.retrying(|| self.transact::<Value>(both(&body, pg.clone()), vars.clone()))?;
        self.session(handle)
    }

    /// A session that left (its pane was closed) is back, so it can act on the board again. The roles
    /// it released stay released: it claims or joins again.
    pub fn revive(&self, handle: &str) -> Result<(), Error> {
        let vars = json!({
            "handle": handle, "now": (self.clock)(),
            "unknown": format!("{REFUSED}no session {}", repr(handle)),
        });
        self.retrying(|| self.transact::<Value>(both(sql::REVIVE, pg::revive()), vars.clone()))
            .map(|_| ())
    }

    /// The session is done: its roles are released on the record, and it can no longer act. A task
    /// left in progress with no executor is ready again. The open work its orchestrator role kept
    /// somebody's goes, in the same write, to an orchestrator that stays by rule 4, the topmost task
    /// of each tree it would leave with none ([`orchestration::orphaned`]), one line on each thread;
    /// with none, each such task is marked waiting (`Store::give_unowned`). Only a leaver holding
    /// the orchestrator role reads the whole board for it. On a task a custom workflow holds, its
    /// other roles go first, each by its position's lost-holder recovery, which may keep one for a
    /// person to recover and never moves the task to a pair it does not name.
    pub fn leave(&self, handle: &str) -> Result<(), Error> {
        // What it staffs on a task a custom workflow holds goes first, by each position's
        // lost-holder recovery (docs/WORKFLOW.md, "Recovery").
        self.recover_leaving(handle)?;
        let body = sql::leave();
        let pg = pg::leave();
        let why = format!("{handle}, who orchestrated it, left");
        self.retrying(|| {
            let read = sql::held_orchestrating();
            let held: Vec<String> = self.read(
                both(&read, pg::held_orchestrating()),
                json!({"actor": handle}),
            )?;
            let mut pass = Pass::default();
            if !held.is_empty() {
                let counted = self.slot_counters()?;
                let (board, versions) = self.board_versions()?;
                let mut pool = self.pool(&board, &[handle])?;
                let gone = |_: &crate::Task, r: &crate::Role| r.handle == handle;
                for top in orchestration::orphaned(&board.tasks, gone) {
                    pass.give(&board, &versions, top, pool.pick_for(top), &why, &[handle]);
                }
                pass.guard(&counted);
            }
            let vars = self.acting(handle, (self.clock)(), json!({"held": held}));
            self.transact::<Value>(both(&body, pg.clone()), pass.vars(handle, vars))
        })
        .map(|_| ())
    }

    /// Every session, by when it connected, then handle.
    pub fn sessions(&self) -> Result<Vec<Session>, Error> {
        #[derive(Deserialize)]
        struct Found {
            sessions: Vec<SessionRow>,
            held: Vec<String>,
        }
        let read = sql::sessions();
        let found: Found = self.retrying(|| {
            self.transact(both(&read, pg::sessions()), json!({"executing": EXECUTING}))
        })?;
        let now = (self.clock)();
        let held: HashSet<&str> = found.held.iter().map(String::as_str).collect();
        Ok(found
            .sessions
            .iter()
            .map(|row| Session {
                health: health::health(
                    row,
                    now,
                    &self.host,
                    &*self.pid_alive,
                    held.contains(row.handle.as_str()),
                ),
                row: row.clone(),
            })
            .collect())
    }

    /// The session whose tool calls beat through Claude Code session or agent id `agent`.
    pub fn handle_for_agent(&self, agent: &str) -> Result<Option<String>, Error> {
        self.retrying(|| {
            self.transact(
                both(sql::HANDLE_FOR_AGENT, pg::handle_for_agent()),
                json!({"agent": agent}),
            )
        })
    }

    /// Handle -> the Claude Code session or agent id whose tool calls beat for it, for sessions that
    /// have not left.
    pub fn bound_agents(&self) -> Result<BTreeMap<String, String>, Error> {
        #[derive(Deserialize)]
        struct Bound {
            handle: String,
            agent: String,
        }
        let bound: Vec<Bound> = self
            .retrying(|| self.transact(both(sql::BOUND_AGENTS, pg::bound_agents()), json!({})))?;
        Ok(bound.into_iter().map(|b| (b.handle, b.agent)).collect_map())
    }

    /// Handle -> the key whose tool calls beat for it, with what its binding keeps (whether it is an
    /// in-process agent's, and where and since when that agent's calls run), for sessions that have
    /// not left: what [`Store::bound_agents`] reads, in the same one request.
    pub fn agent_bindings(&self) -> Result<BTreeMap<String, Binding>, Error> {
        let bound: Vec<BoundAgent> = self.retrying(|| {
            self.transact(both(sql::AGENT_BINDINGS, pg::agent_bindings()), json!({}))
        })?;
        Ok(bound
            .into_iter()
            .map(|b| (b.handle, b.binding))
            .collect_map())
    }
}

/// A session's handle and its binding, one record of [`Store::agent_bindings`]' read: the binding
/// is read beside the handle ([`Beside`]), so its times read in every build.
#[derive(Debug, PartialEq)]
struct BoundAgent {
    handle: String,
    binding: Binding,
}

impl<'de> Deserialize<'de> for BoundAgent {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let mut record = Beside::read(from)?;
        Ok(BoundAgent {
            handle: record.take("handle")?,
            binding: record.rest()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A binding's times as a reader that keeps a number's text holds them, past their shortest
    /// form: they read beside the handle in every build.
    #[test]
    fn a_bound_agent_reads_its_binding_beside_its_handle_however_its_times_are_written() {
        let text = r#"{"handle":"amber-heron","agent":"abc","in_process":true,"worktree":"/w",
            "branch":"b","placed_at":1762912345.250,"stay_from":1.76291234525e9,
            "placed_boot":{"boot":"x","at":12.50},"last_boot":null,"boot_uncertain":true}"#;
        let want = BoundAgent {
            handle: "amber-heron".into(),
            binding: Binding {
                agent: "abc".into(),
                in_process: true,
                worktree: "/w".into(),
                branch: "b".into(),
                placed_at: 1_762_912_345.25,
                stay_from: 1_762_912_345.25,
                placed_boot: Some(Boottime {
                    boot: "x".into(),
                    at: 12.5,
                }),
                last_boot: None,
                boot_uncertain: true,
            },
        };
        assert_eq!(serde_json::from_str::<BoundAgent>(text).unwrap(), want);
        let value: Value = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::from_value::<BoundAgent>(value).unwrap(), want);
    }

    /// `placed_at ?? 0` in the read gives a whole number, and a field the read leaves out is the
    /// binding's default; a record without its handle is refused naming it.
    #[test]
    fn a_bound_agent_reads_whole_number_times_and_the_defaults_and_needs_its_handle() {
        let bound: BoundAgent =
            serde_json::from_str(r#"{"handle":"h","agent":"a","placed_at":0,"stay_from":7}"#)
                .unwrap();
        assert_eq!(
            bound,
            BoundAgent {
                handle: "h".into(),
                binding: Binding {
                    agent: "a".into(),
                    stay_from: 7.0,
                    ..Binding::default()
                },
            }
        );
        let error = serde_json::from_str::<BoundAgent>(r#"{"agent":"a"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing field `handle`"), "{error}");
    }
}
