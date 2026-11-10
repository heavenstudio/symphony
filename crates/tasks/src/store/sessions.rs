//! Sessions: registering, being seen, a Claude Code agent's tool calls beating for a handle, leaving
//! and coming back.

use super::{
    ACT, Error, Failure, Found, HELD, REFUSED, Session, Store, acting, logged, refused, repr,
    session_read,
};
use crate::health::{self, EXECUTING};
use crate::rows::SessionRow;
use crate::sys::Boottime;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};
use std::sync::{Mutex, PoisonError};

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

/// A session, its parent there when it names one, and a handle nobody took meanwhile.
const REGISTER: &str = "
IF $parent != NONE AND !record::exists(type::record('session', $parent)) { THROW $orphan };
IF record::exists(type::record('session', $handle)) { THROW 'stale' };
CREATE type::record('session', $handle) CONTENT $row RETURN NONE;";

/// What `bind_agent` decides on.
const BINDING: &str = "
LET $held = type::record('session', $handle).agent;
RETURN {
    row: (SELECT * OMIT id FROM ONLY type::record('session', $handle)),
    barred: record::exists(type::record('barred_key', $agent)),
    bound: (SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE),
    in_process: type::record('callback_state', ['binding', $agent]).in_process,
    held_in_process: IF $held = NONE { NONE } ELSE { type::record('callback_state', ['binding', $held]).in_process },
    native: type::record('callback_state', $handle).address != NONE,
};";

/// Every bind of one agent writes its counter first, so two at once meet there rather than each
/// binding a handle of its own; then the session is still as decided on.
const BIND_GUARD: &str = "
UPSERT type::record('counter', ['agent', $agent]) SET last += 1 RETURN NONE;
LET $row = (SELECT left_at, pid, host FROM ONLY type::record('session', $handle));
IF $row.pid != $pid OR $row.host != $host { THROW 'stale' };";

// Session rows stay compatible with older readers. The hook knows whether this key is a full
// session or an in-process agent; keep that provenance in callback metadata, never in session.
const CHANNEL_BIND: &str = "
LET $held = type::record('session', $handle).agent;
LET $held_in_process = IF $held = NONE { NONE } ELSE { type::record('callback_state', ['binding', $held]).in_process };
IF !$full AND (type::record('callback_state', $handle).address != NONE
    OR type::record('callback_state', ['binding', $agent]).in_process = false
    OR $held_in_process = false) { THROW 'stale' };
UPSERT type::record('callback_state', ['binding', $agent]) SET in_process = (in_process ?? false) OR !$full RETURN NONE;
IF !$full {
    DELETE type::record('callback_state', $handle) RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle = $handle AND string::starts_with(key, 'chat-') RETURN NONE;
};";

/// `agent` beats for `handle` from now on, and for no other.
const BIND: &str = "
IF $row.left_at != NONE { THROW 'stale' };
IF $full AND (record::exists(type::record('barred_key', $agent))
    OR array::len(SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE) > 0) { THROW 'stale' };
UPDATE session SET agent = NONE, busy_until = NONE, busy_call = NONE WHERE agent = $agent AND handle != $handle RETURN NONE;
UPDATE type::record('session', $handle) SET agent = $agent, last_seen = math::max([last_seen, $now]) RETURN NONE;
RETURN NONE;";

/// `agent` is a session with a monitor of its own: it beats for nobody, ever.
const BAR: &str = "
IF !record::exists(type::record('barred_key', $agent)) {
    CREATE type::record('barred_key', $agent) SET key = $agent RETURN NONE;
};
UPDATE session SET agent = NONE, busy_until = NONE, busy_call = NONE WHERE agent = $agent RETURN NONE;
RETURN NONE;";

/// The session bound to `$agent` is seen, and its busy mark kept, moved or cleared. `boot_order`
/// keeps the known watermark even when `last_boot` is unreadable; retired boot ids prevent a
/// delayed sample from moving it back across a reboot. No arrival order proves sample order across
/// an unreadable call, so `boot_uncertain` lasts for the checkout stay.
const BEAT: &str = "
LET $s = (SELECT handle, busy_until, busy_call FROM session WHERE agent = $agent AND left_at = NONE LIMIT 1)[0];
LET $mark = IF $s = NONE { NONE }
    ELSE IF $busy_for != NONE AND ($s.busy_until = NONE OR $now + $busy_for >= $s.busy_until) {
        { until: $now + $busy_for, call: $call } }
    ELSE IF $ended AND $s.busy_call = $call { { until: NONE, call: NONE } }
    ELSE { { until: $s.busy_until, call: $s.busy_call } };
IF $s != NONE {
    UPDATE type::record('session', $s.handle)
        SET last_seen = math::max([last_seen, $now]), last_tool = math::max([last_tool ?? $now, $now]), busy_until = $mark.until, busy_call = $mark.call RETURN NONE;
};
LET $bound = type::record('callback_state', ['binding', $agent]);
IF $s != NONE AND $bound.in_process = true {
    IF $place != NONE AND [$bound.worktree, $bound.branch] != [$place.worktree, $place.branch] {
        UPDATE $bound SET worktree = $place.worktree, branch = $place.branch, placed_at = $now,
            stay_from = $now, placed_boot = $boot, last_boot = $boot, boot_order = $boot,
            boot_previous = [], boot_uncertain = $boot = NONE RETURN NONE;
    } ELSE {
        IF $place != NONE AND $now < ($bound.stay_from ?? $bound.placed_at) {
            UPDATE $bound SET stay_from = $now RETURN NONE;
        };
        LET $last = $bound.boot_order ?? $bound.last_boot ?? $bound.placed_boot;
        IF $boot = NONE AND ($bound.last_boot != NONE OR $bound.boot_uncertain != true) {
            UPDATE $bound SET last_boot = NONE, boot_order = $last, boot_uncertain = true RETURN NONE;
        } ELSE IF $boot != NONE AND !array::includes($bound.boot_previous ?? [], $boot.boot)
            AND ($last = NONE OR $boot.boot != $last.boot OR $boot.at > $last.at) {
            UPDATE $bound SET last_boot = $boot, boot_order = $boot,
                boot_previous = IF $last != NONE AND $boot.boot != $last.boot {
                    array::append(boot_previous ?? [], $last.boot)
                } ELSE { boot_previous ?? [] } RETURN NONE;
        };
    };
};
RETURN $s.handle;";

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
            let taken: Vec<String> =
                self.transact("RETURN SELECT VALUE handle FROM session;", json!({}))?;
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
            let found: Found = self.transact(&format!("{REGISTER}\n{}", session_read()), vars)?;
            Ok((handle, found))
        })?;
        self.found(&handle, found)
    }

    fn new_handle(&self, taken: &HashSet<String>) -> Result<String, Error> {
        let mut pick = self.pick.lock().unwrap_or_else(PoisonError::into_inner);
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
        let body = format!("{ACT}\nRETURN NONE;");
        let vars = acting(handle, (self.clock)(), json!({}));
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))
            .map(|_| ())
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
            let found: Found = self.transact(BINDING, json!({"agent": agent, "handle": handle}))?;
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
                    &format!(
                        "{BIND_GUARD}
IF type::record('session', $handle).agent != $agent {{ THROW 'stale' }};
{CHANNEL_BIND} RETURN NONE;"
                    ),
                    json!({"agent":agent, "handle":handle, "full":session,
                           "pid":row.pid, "host":row.host}),
                )?;
                return Ok(true);
            }
            if session && found.barred {
                return Ok(false);
            }
            let monitored = self.monitored(&row);
            let (write, bound) = if session && monitored {
                (BAR, false)
            } else if row.left_at.is_some() || monitored || (session && !found.bound.is_empty()) {
                return Ok(false);
            } else {
                (BIND, true)
            };
            let vars = json!({
                "agent": agent, "handle": handle, "full": session, "now": (self.clock)(),
                "pid": row.pid, "host": row.host,
            });
            let channel = if bound { CHANNEL_BIND } else { "" };
            self.transact::<Value>(&format!("{BIND_GUARD}{channel}{write}"), vars)?;
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
        let place = place.map(|(worktree, branch)| json!({"worktree": worktree, "branch": branch}));
        let vars = json!({
            "agent": agent, "now": (self.clock)(), "busy_for": beat.busy_for, "call": beat.call,
            "ended": beat.ended, "place": place, "boot": boot,
        });
        self.retrying(|| self.transact(BEAT, vars.clone()))
    }

    /// Take an existing handle back (a re-armed monitor), keeping every role it holds.
    pub fn reattach(&self, handle: &str, pid: Option<i64>) -> Result<Session, Error> {
        let host = if pid.is_some_and(|pid| pid != 0) {
            self.host.as_str()
        } else {
            ""
        };
        let body = format!(
            "{ACT}\nUPDATE type::record('session', $actor) SET pid = $pid, host = $host RETURN NONE;\nRETURN NONE;"
        );
        let vars = acting(handle, (self.clock)(), json!({"pid": pid, "host": host}));
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))?;
        self.session(handle)
    }

    /// A session that left (its pane was closed) is back, so it can act on the board again. The roles
    /// it released stay released: it claims or joins again.
    pub fn revive(&self, handle: &str) -> Result<(), Error> {
        let body = "IF !record::exists(type::record('session', $handle)) { THROW $unknown };
            UPDATE type::record('session', $handle) SET left_at = NONE, last_seen = math::max([last_seen, $now]) RETURN NONE;
            RETURN NONE;";
        let vars = json!({
            "handle": handle, "now": (self.clock)(),
            "unknown": format!("{REFUSED}no session {}", repr(handle)),
        });
        self.retrying(|| self.transact::<Value>(body, vars.clone()))
            .map(|_| ())
    }

    /// The session is done: its roles are released on the record, and it can no longer act. A task
    /// left in progress with no executor is ready again.
    pub fn leave(&self, handle: &str) -> Result<(), Error> {
        let body = format!(
            "{ACT}
FOR $slug IN array::sort(array::distinct(SELECT VALUE slug FROM role WHERE handle = $actor)) {{
    DELETE role WHERE slug = $slug AND handle = $actor;
    LET $ready = (SELECT VALUE status FROM ONLY type::record('task', $slug)) = 'in_progress'
        AND array::len(SELECT VALUE handle FROM role WHERE slug = $slug AND role = 'executor') = 0;
    UPDATE type::record('task', $slug) SET version += 1, status = IF $ready {{ 'ready' }} ELSE {{ status }},
        updated = IF $ready {{ $now }} ELSE {{ updated }} RETURN NONE;
    {}
}};
UPDATE type::record('session', $actor) SET left_at = $now RETURN NONE;
DELETE type::record('callback_state', $actor) RETURN NONE;
RETURN NONE;",
            logged(
                "$slug",
                "$actor",
                "'system'",
                "$actor + ' released every role (left)'",
                "NONE"
            )
        );
        let vars = acting(handle, (self.clock)(), json!({}));
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))
            .map(|_| ())
    }

    /// Every session, by when it connected, then handle.
    pub fn sessions(&self) -> Result<Vec<Session>, Error> {
        #[derive(Deserialize)]
        struct Found {
            sessions: Vec<SessionRow>,
            held: Vec<String>,
        }
        let read = format!(
            "{HELD}\nRETURN {{ sessions: (SELECT * OMIT id FROM session ORDER BY connected, handle), held: $held }};"
        );
        let found: Found =
            self.retrying(|| self.transact(&read, json!({"executing": EXECUTING})))?;
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
        let read = "RETURN (SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE LIMIT 1)[0];";
        self.retrying(|| self.transact(read, json!({"agent": agent})))
    }

    /// Handle -> the Claude Code session or agent id whose tool calls beat for it, for sessions that
    /// have not left.
    pub fn bound_agents(&self) -> Result<BTreeMap<String, String>, Error> {
        #[derive(Deserialize)]
        struct Bound {
            handle: String,
            agent: String,
        }
        let read =
            "RETURN SELECT handle, agent FROM session WHERE agent != NONE AND left_at = NONE;";
        let bound: Vec<Bound> = self.retrying(|| self.transact(read, json!({})))?;
        Ok(bound.into_iter().map(|b| (b.handle, b.agent)).collect())
    }

    /// Handle -> the key whose tool calls beat for it, with what its binding keeps (whether it is an
    /// in-process agent's, and where and since when that agent's calls run), for sessions that have
    /// not left: what [`Store::bound_agents`] reads, in the same one request.
    pub fn agent_bindings(&self) -> Result<BTreeMap<String, Binding>, Error> {
        #[derive(Deserialize)]
        struct Bound {
            handle: String,
            #[serde(flatten)]
            binding: Binding,
        }
        let read = "RETURN SELECT handle, agent,
                type::record('callback_state', ['binding', agent]).in_process ?? false AS in_process,
                type::record('callback_state', ['binding', agent]).worktree ?? '' AS worktree,
                type::record('callback_state', ['binding', agent]).branch ?? '' AS branch,
                type::record('callback_state', ['binding', agent]).placed_at ?? 0 AS placed_at,
                type::record('callback_state', ['binding', agent]).stay_from
                    ?? type::record('callback_state', ['binding', agent]).placed_at ?? 0 AS stay_from,
                type::record('callback_state', ['binding', agent]).placed_boot AS placed_boot,
                type::record('callback_state', ['binding', agent]).last_boot AS last_boot,
                type::record('callback_state', ['binding', agent]).boot_uncertain ?? false AS boot_uncertain
            FROM session WHERE agent != NONE AND left_at = NONE;";
        let bound: Vec<Bound> = self.retrying(|| self.transact(read, json!({})))?;
        Ok(bound.into_iter().map(|b| (b.handle, b.binding)).collect())
    }
}
