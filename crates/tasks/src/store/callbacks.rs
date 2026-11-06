//! Native session channels and a durable outbox. Writes deliver immediately; the Symphony server's
//! clock drains retries and observes actionable PULL/STALE/LOST changes. No agent keeps a monitor alive.

use super::callback_peer::{self as peer, Owner};
use super::callback_state::{State, stored_seen};
use super::pull_account::{Account, PullPlan};
use super::queued_lines::{Hearing, Queued, Rescoped};
use super::{ACT, Error, Store, acting, logged, refused};
use crate::cli::agents::{Answer, Pane, Probe, Probes};
use crate::cli::{attach, pull};
use crate::sys::{Command, Stdio};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[path = "codex_delivery.rs"]
mod codex;

#[cfg(test)]
#[path = "../../../../../tools/gate/tests/verify/script_process_cwd.rs"]
mod fixture_script;

const CALLBACK_LEASE_SECONDS: u64 = 60;

fn sampled<'a, T: Clone + 'a>(probe: Probe<'a, T>) -> Probe<'a, T> {
    let sample = std::cell::OnceCell::new();
    Box::new(move || sample.get_or_init(&probe).clone())
}

// One machine observation per recipient dispatch, shared by planning and the leased read.
// Only machine inputs are retained: the agents view is rebuilt against the refreshed board.
struct DispatchProbes<'a> {
    env: &'a crate::cli::Env,
    sample: std::cell::OnceCell<Probes<'a>>,
}

impl<'a> DispatchProbes<'a> {
    fn get(&self) -> &Probes<'a> {
        self.sample.get_or_init(|| {
            let probes = crate::cli::probes::machine(self.env);
            Probes {
                panes: sampled(probes.panes),
                runs: sampled(probes.runs),
                branch_of: probes.branch_of,
            }
        })
    }
}

pub(super) struct Idle {
    marker: u64,
    health: Vec<crate::Health>,
    pull_at: f64,
}

#[derive(Deserialize, Serialize)]
struct Event {
    key: String,
    handle: String,
    text: String,
    slug: Option<String>,
    message: Option<i64>,
    #[serde(default)]
    attempts: u32,
    #[serde(default)]
    sequence: u64,
    #[serde(default)]
    pull_entries: Option<BTreeSet<(String, String)>>,
    #[serde(default)]
    pull_withheld: Option<BTreeSet<(String, String)>>,
}

/// A recipient's callback state as a dispatch reads it, with when its PULL was last checked and
/// the lines still queued for it, which a hand-over can take back ([`Hearing::rescope`]).
#[derive(Deserialize)]
struct Stored {
    state: Option<State>,
    checked_pull: f64,
    queued: Vec<Queued>,
}

#[derive(Clone, Deserialize)]
pub struct Callback {
    pub handle: String,
    pub address: String,
    pub host: String,
    #[serde(default)]
    pub chat: bool,
    #[serde(default)]
    chat_fallback: bool,
    #[serde(default)]
    owner: Option<Owner>,
    #[serde(default)]
    in_process: bool,
    #[serde(default)]
    confirmed_until: f64,
    session: crate::rows::SessionRow,
    #[serde(default)]
    chat_resume: bool,
    /// What the server clock's pane watch last told it; older dispatchers leave it as it is.
    #[serde(default)]
    pane_watch: attach::PaneWatch,
    #[serde(skip)]
    live_orchestrator: bool,
}

impl Store {
    /// Send directly to a registered agent, without inventing a task or a terminal input channel.
    /// The callback outbox owns delivery and retries; false means not yet confirmed delivered.
    /// Another dispatcher may hold the lease and already be delivering it; it remains in the inbox.
    pub fn send_agent(&self, sender: &str, handle: &str, text: &str) -> Result<bool, Error> {
        let text = event_line(text);
        if text.is_empty() {
            return Err(refused("an empty message says nothing".into()));
        }
        let text = format!("AGENT from {sender} to {handle} — {text}");
        let board_command = self.callback_command();
        let key: String = self.retrying(|| self.transact(
            &format!("{ACT}
LET $recipient = (SELECT left_at FROM ONLY type::record('session', $handle));
IF $recipient = NONE {{ THROW string::concat('refused: no session ', $handle); }};
IF $recipient.left_at != NONE {{ THROW string::concat('refused: session ', $handle, ' has left'); }};
IF $actor = $handle {{ RETURN ''; }};
LET $channel = (SELECT address, owner FROM ONLY type::record('callback_state', $handle));
LET $address = $channel.address ?? '';
LET $agent = type::record('session', $handle).agent;
LET $in_process = IF $agent = NONE {{ false }} ELSE {{ type::record('callback_state', ['binding', $agent]).in_process ?? true }};
IF $address = '' OR $in_process OR (string::starts_with($address, 'uds:') AND $channel.owner = NONE) {{ THROW $no_channel; }};
LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
LET $key = string::concat('agent-', <string>$counted.last);
CREATE type::record('callback_event', [$handle, $key]) SET key = $key, handle = $handle,
    text = $text, sequence = $counted.last, attempts = 0, delivered = false RETURN NONE;
RETURN $key;"),
            acting(sender, self.now(), json!({"handle":handle, "text":text,
                "no_channel":format!("refused: {handle} has no native channel; have it run {board_command} callback --as {handle}, or send on a task with {board_command} say SLUG TEXT --to {handle} (it can read {board_command} inbox --as {handle})") })),
        ))?;
        if key.is_empty() {
            return Ok(true);
        }
        self.dispatch_agent(handle).map_err(|error| Error::Db(format!(
            "message {key} is queued for {handle}; callback dispatch failed: {error}; do not resend"
        )))?;
        // Delivered rows may already have been pruned by another dispatcher or the sweep.
        self.retrying(|| self.transact(
            "RETURN !record::exists(type::record('callback_event', [$handle, $key])) OR type::record('callback_event', [$handle, $key]).delivered = true;",
            json!({"handle":handle, "key":key}),
        ))
    }

    /// Register an explicit native channel, or unregister with None. Tokens never enter the board.
    /// A single chat recipient is selected transactionally; --if-free lets an orchestrator fill a
    /// vacant seat without displacing the session answering the owner.
    pub fn callback(
        &self,
        handle: &str,
        address: Option<&str>,
        chat: bool,
        if_free: bool,
    ) -> Result<(), Error> {
        if let Some(address) = address {
            validate(address).map_err(refused)?;
        } else if chat {
            return Err(refused("register a callback before receiving chat".into()));
        }
        let owner = if let Some(path) = address.and_then(|a| a.strip_prefix("uds:")) {
            Some(
                peer::connect(path)
                    .map_err(|e| refused(format!("callback {path}: {e}")))?
                    .1,
            )
        } else {
            None
        };
        // Registration probes only the requested address, never every other session's socket.
        if let Some(owner) = &owner {
            for old in self
                .stored_callbacks()?
                .into_iter()
                .filter(|c| c.host == self.host && Some(c.address.as_str()) == address)
            {
                if old.in_process || old.owner.as_ref() != Some(owner) {
                    self.clear_callback(&old, "registration listener identity changed")?;
                }
            }
        }
        let body = format!("{ACT}
UPSERT callback_state:chat SET revision = (revision ?? 0) + 1 RETURN NONE;
LET $agent = type::record('session', $actor).agent;
LET $binding = IF $agent = NONE {{ NONE }} ELSE {{ type::record('callback_state', ['binding', $agent]).in_process }};
IF $address != NONE AND $agent != NONE AND $binding = NONE {{ THROW string::concat('refused: binding provenance is not recorded yet; retry just tasks callback --as ', $actor, ' so the current hook can classify the session'); }};
IF $address != NONE AND $agent != NONE AND $binding = true {{ THROW 'refused: in-process agent binding cannot register a callback; use just tasks inbox'; }};
LET $other = (SELECT VALUE handle FROM callback_state WHERE address = $address AND handle != $actor AND type::record('session', handle).left_at = NONE ORDER BY handle LIMIT 1)[0];
IF $address != NONE AND $other != NONE {{ THROW string::concat('refused: callback address already held by ', $other, '; in-process subagents use just tasks inbox'); }};
LET $old = (SELECT VALUE address FROM ONLY type::record('callback_state', $actor));
IF $address != NONE AND ($old != $address OR ($chat AND !$if_free)) {{
    UPDATE callback_event SET attempts = 0, retry_at = 0, capacity_limited = false WHERE handle = $actor AND delivered != true RETURN NONE;
}};
LET $occupied = array::len(SELECT handle FROM callback_state WHERE chat = true AND address != NONE AND type::record('session', handle).left_at = NONE AND handle != $actor) > 0;
LET $take = $chat AND (!$if_free OR !$occupied);
IF $take {{
    UPDATE callback_state SET chat = false WHERE chat = true RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle != $actor AND string::starts_with(key, 'chat-') RETURN NONE;
}};
IF $address = NONE {{ UPDATE callback_event SET delivered = true, text = '' WHERE handle = $actor AND string::starts_with(key, 'chat-') RETURN NONE; }};
UPSERT type::record('callback_state', $actor) SET handle = $actor, address = $address, host = $host, chat_fallback = $chat AND $if_free,
    registration_revision = (registration_revision ?? 0) + 1,
    owner = $owner, confirmed_until = 0, chat_resume = false,
    chat = IF $address = NONE {{ false }} ELSE IF $take {{ true }} ELSE {{ chat ?? false }} RETURN NONE;
RETURN NONE;");
        self.retrying(|| {
            self.transact::<Value>(
                &body,
                acting(
                    handle,
                    self.now(),
                    json!({
                        "address":address, "chat":chat, "if_free":if_free, "host":self.host, "owner":owner,
                    }),
                ),
            )
        })?;
        self.dispatch_callbacks()
    }

    pub fn chat_recipient(&self) -> Result<Option<String>, Error> {
        Ok(recipient(&self.cached_callbacks()?).map(str::to_owned))
    }

    pub fn callbacks(&self) -> Result<BTreeMap<String, Callback>, Error> {
        let rows = self.cached_callbacks()?;
        let recipient = recipient(&rows).map(str::to_owned);
        Ok(rows
            .into_iter()
            .map(|mut row| {
                row.chat = recipient.as_deref() == Some(row.handle.as_str());
                (row.handle.clone(), row)
            })
            .collect())
    }

    fn stored_callbacks(&self) -> Result<Vec<Callback>, Error> {
        self.retrying(|| self.transact(
            "RETURN SELECT handle, address, host, chat ?? false AS chat, chat_fallback ?? false AS chat_fallback, owner, confirmed_until ?? 0 AS confirmed_until,
                (SELECT * OMIT id FROM ONLY type::record('session', $parent.handle)) AS session, chat_resume ?? false AS chat_resume, pane_watch ?? {} AS pane_watch,
                IF type::record('session', handle).agent = NONE { false } ELSE { type::record('callback_state', ['binding', type::record('session', handle).agent]).in_process ?? true } AS in_process
             FROM callback_state WHERE address != NONE AND type::record('session', handle).left_at = NONE;", json!({}),
        ))
    }

    /// Reads use only the last recorded evidence. They neither connect nor retire registrations.
    fn cached_callbacks(&self) -> Result<Vec<Callback>, Error> {
        Ok(self
            .stored_callbacks()?
            .into_iter()
            .filter(|c| !c.in_process && (!c.address.starts_with("uds:") || c.owner.is_some()))
            .map(|mut c| {
                c.live_orchestrator = c.session.kind == "orchestrator"
                    && !c.chat_resume
                    && (c.confirmed_until >= self.now()
                        || crate::health::health(
                            &c.session,
                            self.now(),
                            &self.host,
                            &*self.pid_alive,
                            false,
                        ) == crate::Health::Live);
                c
            })
            .collect())
    }

    pub(crate) fn clear_callback(&self, callback: &Callback, why: &str) -> Result<(), Error> {
        self.retrying(|| self.transact::<Value>(
            "LET $state = (SELECT address, owner FROM ONLY type::record('callback_state', $handle));
             IF $state.address = $address AND $state.owner = $owner {
                DELETE type::record('callback_state', $handle) RETURN NONE;
                UPDATE callback_event SET delivered = true, text = '' WHERE handle = $handle AND string::starts_with(key, 'chat-') RETURN NONE;
             }; RETURN NONE;",
            json!({"handle":callback.handle, "address":callback.address, "owner":callback.owner}),
        ))?;
        eprintln!(
            "CALLBACK {} retired: {why}; messages remain in inbox",
            callback.handle
        );
        Ok(())
    }

    /// STALE/LOST bookkeeping uses the last sweep's evidence until the next sweep plus its margin.
    pub(crate) fn confirmed_callbacks(&self) -> Result<BTreeSet<String>, Error> {
        Ok(self
            .cached_callbacks()?
            .into_iter()
            .filter(|c| c.confirmed_until >= self.now())
            .map(|c| c.handle)
            .collect())
    }

    /// The idle sweep confirms native reachability rather than treating absent tool calls as an
    /// exit. A socket must accept a connection; a Codex thread must still occupy a pane. A dry run
    /// reports the same channels without changing their heartbeat. No payload or credentials are
    /// sent by the socket probe. The inventory's boolean says whether absence is established:
    /// partial inventories can confirm known threads but cannot retire missing registrations.
    pub(crate) fn callback_presence(
        &self,
        pane_sessions: Option<(&BTreeSet<String>, bool)>,
        refresh: bool,
    ) -> Result<BTreeSet<String>, Error> {
        let mut present = BTreeSet::new();
        for callback in self
            .stored_callbacks()?
            .into_iter()
            .filter(|c| c.host == self.host)
        {
            if callback.in_process
                || (callback.address.starts_with("uds:") && callback.owner.is_none())
            {
                if refresh {
                    self.clear_callback(&callback, "invalid binding or missing listener identity")?;
                }
                continue;
            }
            let alive = if callback.address.starts_with("uds:") {
                match probe(&callback) {
                    Ok(()) => true,
                    Err(error) => {
                        if refresh {
                            if error.kind == DeliveryFailure::Dead {
                                self.clear_callback(&callback, &error.message)?;
                            } else {
                                self.callback_blocked(&callback)?;
                            }
                        }
                        continue;
                    }
                }
            } else {
                let Some((panes, complete)) = pane_sessions else {
                    continue;
                };
                let found = callback
                    .address
                    .strip_prefix("codex:")
                    .is_some_and(|id| panes.contains(id));
                if !found && !complete {
                    continue;
                }
                found
            };
            if alive && (!refresh || self.callback_seen(&callback)?) {
                present.insert(callback.handle);
            } else if !alive && refresh {
                self.clear_callback(&callback, "thread no longer occupies a pane")?;
            }
        }
        if refresh {
            self.prune_callbacks()?;
        }
        Ok(present)
    }

    fn prune_callbacks(&self) -> Result<(), Error> {
        self.retrying(|| self.transact::<Value>(
            "DELETE callback_event WHERE delivered = true OR (string::starts_with(key, 'chat-') AND type::record('chat_event', string::slice(key, 5)).delivered = true) RETURN NONE;
             LET $agents = array::distinct(SELECT VALUE agent FROM session WHERE agent != NONE AND left_at = NONE);
             DELETE callback_state WHERE in_process != NONE AND record::id(id)[1] NOT IN $agents RETURN NONE;
             RETURN NONE;", json!({}),
        )).map(|_| ())
    }

    // A liveness probe recovers transport failures, not an event already found too large on its
    // own. Keep that event's attempt budget even if another message later blocks the listener.
    fn callback_seen(&self, callback: &Callback) -> Result<bool, Error> {
        self.retrying(|| self.transact(
            "IF type::record('callback_state', $handle).address != $address OR type::record('callback_state', $handle).owner != $owner OR type::record('session', $handle).left_at != NONE { RETURN false; };
             LET $blocked = type::record('callback_state', $handle).blocked ?? false;
             LET $resume = type::record('callback_state', $handle).chat_resume ?? false;
             LET $seat = type::record('callback_state', $handle).chat_resume_seat ?? true;
             LET $occupied = array::len(SELECT handle FROM callback_state WHERE chat = true AND address != NONE AND handle != $handle AND type::record('session', handle).left_at = NONE) > 0;
             IF $resume AND (!$seat OR !$occupied) {
                UPDATE type::record('callback_state', $handle) SET chat = $seat, chat_fallback = chat_resume_fallback ?? false, chat_resume = false RETURN NONE;
             };
             UPDATE type::record('callback_state', $handle) SET confirmed_until = $until, blocked = false RETURN NONE;
             IF $blocked { UPDATE callback_event SET attempts = 0, retry_at = 0 WHERE handle = $handle AND delivered != true AND capacity_limited != true RETURN NONE; };
             UPDATE type::record('session', $handle) SET last_seen = math::max([last_seen, $now]) RETURN NONE;
             RETURN true;",
            json!({"handle":callback.handle, "address":callback.address, "owner":callback.owner, "now":self.now(),
                "until":self.now() + crate::launch::reap::REAP_SECONDS + crate::health::LIVE_SECONDS}),
        ))
    }

    fn callback_blocked(&self, callback: &Callback) -> Result<(), Error> {
        self.retrying(|| self.transact::<Value>(
            "UPDATE type::record('callback_state', $handle) SET blocked = true WHERE address = $address AND owner = $owner AND (blocked ?? false) = false RETURN NONE; RETURN NONE;",
            json!({"handle":callback.handle, "address":callback.address, "owner":callback.owner}),
        )).map(|_| ())
    }

    /// Failed or pending non-message events also remain readable through the ordinary inbox.
    pub fn callback_inbox(&self, handle: &str) -> Result<Vec<(String, String)>, Error> {
        let chat = self.chat_recipient()?.as_deref() == Some(handle);
        let events: Vec<Event> = self.retrying(|| self.transact(
            "RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND message = NONE
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;",
            json!({"handle":handle, "chat":chat}),
        ))?;
        Ok(events
            .into_iter()
            .map(|event| (event.key, event.text))
            .collect())
    }

    pub fn ack_callbacks(&self, handle: &str, keys: &[String]) -> Result<(), Error> {
        let recipient = self.chat_recipient()?;
        let chats: Vec<&str> = keys
            .iter()
            .filter(|_| recipient.as_deref() == Some(handle))
            .filter_map(|key| key.strip_prefix("chat-"))
            .collect();
        self.retrying(|| self.transact::<Value>(
            &format!("{ACT}\nLET $pull = (SELECT pull_entries, pull_withheld, sequence FROM callback_event WHERE handle = $actor AND key IN $keys AND pull_entries != NONE ORDER BY sequence DESC LIMIT 1)[0];
                IF $pull != NONE {{ UPDATE type::record('callback_state', $actor) SET pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_awaiting = NONE,
                    pull_withheld_revision = revision, pull_queued = NONE RETURN NONE; }};
                FOR $key IN $keys {{ UPDATE type::record('callback_event', [$actor, $key]) SET delivered = true RETURN NONE; }};
                FOR $key IN $chats {{ UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; }};
                UPDATE callback_event SET text = '' WHERE handle = $actor AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE; RETURN NONE;"),
            acting(handle, self.now(), json!({"keys":keys, "chats":chats})),
        )).map(|_| ())
    }

    /// The app calls this after a successful owner save. The event and any undelivered record stay
    /// in the board even when no recipient has registered yet. key is stable across save retries.
    pub fn owner_callback(&self, key: &str, text: &str) -> Result<(), Error> {
        self.retrying(|| self.transact::<Value>(
            "IF !record::exists(type::record('chat_event', $key)) {
                CREATE type::record('chat_event', $key) SET key = $key, text = $text, delivered = false RETURN NONE;
             }; RETURN NONE;",
            json!({"key":key, "text":text}),
        ))?;
        self.dispatch_callbacks()
    }

    pub fn dispatch_callbacks(&self) -> Result<(), Error> {
        let callbacks = self.callbacks()?;
        if !callbacks.is_empty() {
            self.dispatch_board(&callbacks)?;
        }
        Ok(())
    }

    /// The server clock reuses this sequenced snapshot for its read models after delivery. Only the
    /// clock watches panes: a write delivers what it caused, and a pane's prompt is no write. The
    /// watch looks first, so the one herdr sample it takes serves the tick's PULL view and the
    /// clock's agents view after it ([`Store::clock_panes`]).
    pub fn dispatch_callbacks_snapshot(&self) -> Result<Option<super::Board>, Error> {
        *self.clock_panes.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let callbacks = self.callbacks()?;
        if callbacks.is_empty() {
            return Ok(None);
        }
        let (board, _) = self.callback_board()?;
        self.watch_panes(&board, &callbacks)?;
        self.dispatch_board(&callbacks).map(Some)
    }

    /// The herdr panes this tick of the server clock sampled, when its pane watch took a sample.
    pub(crate) fn clock_panes(&self) -> Option<Answer<Vec<Pane>>> {
        self.clock_panes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn dispatch_board(
        &self,
        callbacks: &BTreeMap<String, Callback>,
    ) -> Result<super::Board, Error> {
        let (mut board, mut sequence) = self.callback_board()?;
        let health: Vec<_> = board
            .sessions
            .iter()
            .map(|session| session.health)
            .chain(
                board
                    .tasks
                    .iter()
                    .flat_map(|task| task.roles.iter().map(|role| role.health)),
            )
            .collect();
        let idle = self
            .callback_idle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|idle| {
                idle.marker == sequence
                    && idle.health == health
                    && self.now() < idle.pull_at
                    && board.sessions.iter().all(|session| {
                        matches!(session.health, crate::Health::Live | crate::Health::Left)
                    })
            });
        if idle {
            for (handle, callback) in callbacks.iter().filter(|(_, c)| c.host == self.host) {
                if self.callback_pending(handle, callback.chat)? {
                    self.dispatch_agent(handle)?;
                }
            }
            return Ok(board);
        }
        let initial_sequence = sequence;
        let mut observed = true;
        let mut pull_at = f64::INFINITY;
        for (handle, callback) in callbacks.iter().filter(|(_, c)| c.host == self.host) {
            let env = crate::cli::Env::of_process();
            let probes = DispatchProbes {
                env: &env,
                sample: std::cell::OnceCell::new(),
            };
            let (handled, due) =
                self.dispatch_one(handle, callback, &mut board, &mut sequence, &probes, None)?;
            observed &= handled;
            pull_at = pull_at.min(due);
        }
        *self.callback_idle.lock().unwrap_or_else(|e| e.into_inner()) =
            (observed && initial_sequence == sequence).then_some(Idle {
                marker: sequence,
                health,
                pull_at,
            });
        Ok(board)
    }

    /// One look of the pane watch for each recipient on this host ([`attach::pane_watch`]): herdr is
    /// asked once a tick, and only when some recipient oversees a session launched into an open
    /// pane. A look that changes nothing writes nothing. It plans from what the recipient's row said
    /// and, under its lease, looks again from what the row says now: another clock on the board may
    /// have said the same thing meanwhile.
    fn watch_panes(
        &self,
        board: &super::Board,
        callbacks: &BTreeMap<String, Callback>,
    ) -> Result<(), Error> {
        let recipients: Vec<(&Callback, attach::Overseen)> = callbacks
            .values()
            .filter(|c| c.host == self.host)
            .map(|c| (c, attach::overseen(board, &c.handle)))
            .collect();
        let open = if recipients.iter().any(|(_, overseen)| !overseen.is_empty()) {
            self.open_task_sessions()?
        } else {
            Vec::new()
        };
        let env = crate::cli::Env::of_process();
        let panes = || {
            self.clock_panes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| crate::cli::probes::herdr_panes(&env))
                .clone()
        };
        for (callback, overseen) in recipients {
            let look = |before: &attach::PaneWatch| {
                let mut lines = Vec::new();
                let after =
                    attach::pane_watch(board, &overseen, &open, &panes, before, &mut |line| {
                        lines.push(event_line(line))
                    });
                (lines, after)
            };
            let (lines, after) = look(&callback.pane_watch);
            if lines.is_empty() && after == callback.pane_watch {
                continue;
            }
            let handle = callback.handle.as_str();
            self.with_callback_lease(handle, |deadline, revision| {
                let stored: attach::PaneWatch = self.retrying(|| {
                    self.transact(
                        "RETURN (SELECT VALUE pane_watch FROM ONLY type::record('callback_state', $handle)) ?? {};",
                        json!({"handle":handle}),
                    )
                })?;
                let (lines, after) = look(&stored);
                if lines.is_empty() && after == stored {
                    return Ok(());
                }
                let events: Vec<Event> = lines
                    .iter()
                    .enumerate()
                    .map(|(i, text)| Event {
                        key: format!("pane-{revision}-{i}"),
                        handle: handle.to_string(),
                        text: text.clone(),
                        slug: session_task(board, handle),
                        message: None,
                        attempts: 0,
                        sequence: 0,
                        pull_entries: None,
                        pull_withheld: None,
                    })
                    .collect();
                self.retrying(|| self.transact::<Value>(
                    "FOR $event IN $events {
                        IF !record::exists(type::record('callback_event', [$handle, $event.key])) {
                            LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                            CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                            UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                        };
                     };
                     UPDATE type::record('callback_state', $handle) SET pane_watch = $after RETURN NONE;
                     RETURN NONE;",
                    json!({"handle":handle, "events":events, "after":after}),
                ))?;
                self.deliver_pending(handle, callback, deadline)
            })?;
        }
        Ok(())
    }

    // Direct messages need only the native outbox, never a machine/pane probe or a board digest.
    fn dispatch_agent(&self, handle: &str) -> Result<(), Error> {
        if let Some(callback) = self
            .callbacks()?
            .get(handle)
            .filter(|c| c.host == self.host)
        {
            self.with_callback_lease(handle, |deadline, _| {
                self.deliver_pending(handle, callback, deadline)
            })?;
        }
        Ok(())
    }

    fn with_callback_lease(
        &self,
        handle: &str,
        send: impl FnOnce(Instant, u64) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        // The same per-recipient lease serializes board callbacks and direct messages.
        let deadline = Instant::now() + Duration::from_secs(CALLBACK_LEASE_SECONDS / 2);
        let acquired: Option<u64> = self.retrying(|| {
                self.transact(
                    "LET $s = (SELECT address, lease, revision FROM ONLY type::record('callback_state', $handle));
                 LET $lease = (SELECT * FROM ONLY type::record('callback_lease', $handle));
                 IF $s.address = NONE OR type::record('session', $handle).left_at != NONE { RETURN NONE; };
                 IF math::max([$s.lease ?? 0, $lease.until ?? 0]) > $now { RETURN NONE; };
                 LET $revision = math::max([$lease.revision ?? 0, $s.revision ?? 0]) + 1;
                 UPSERT type::record('callback_lease', $handle) SET until = $now + $lease_seconds, revision = $revision RETURN NONE;
                 RETURN $revision;",
                    json!({"handle":handle, "now":self.now(), "lease_seconds":CALLBACK_LEASE_SECONDS}),
                )
        })?;
        if let Some(revision) = acquired {
            let delivered = send(deadline, revision);
            let released = self.retrying(|| self.transact::<Value>(
                "UPDATE type::record('callback_lease', $handle) SET until = 0 WHERE revision = $revision RETURN NONE; RETURN NONE;",
                json!({"handle":handle, "revision":revision}),
            ));
            delivered?;
            released?;
        }
        Ok(acquired.is_some())
    }

    fn callback_command(&self) -> String {
        if self.client.database() == crate::DATABASE {
            "just tasks".to_string()
        } else {
            format!(
                "tools/rs-bin.sh tasks --database {}",
                self.client.database()
            )
        }
    }

    /// The mutation marker and snapshot are read in the same transaction. Cached inputs are
    /// reinterpreted through View on every tick so health, reservations and presence still age.
    fn callback_board(&self) -> Result<(super::Board, u64), Error> {
        let marker = self.board_marker()?;
        let mut cached = self
            .callback_snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(snapshot) = cached.as_ref()
            && snapshot.callback_sequence == marker
        {
            return Ok((
                Self::board_view(super::View::new(self, snapshot.clone())),
                marker,
            ));
        }
        let (read, vars) = Self::reading(&super::Which::All);
        let body = format!(
            "LET $callback_sequence = (SELECT VALUE last FROM ONLY counter:board) ?? 0;\n{read}"
        );
        let snapshot: super::Snapshot = self.retrying(|| self.transact(&body, vars.clone()))?;
        let sequence = snapshot.callback_sequence;
        *cached = Some(snapshot.clone());
        Ok((Self::board_view(super::View::new(self, snapshot)), sequence))
    }

    fn board_marker(&self) -> Result<u64, Error> {
        self.retrying(|| {
            self.transact(
                "RETURN (SELECT VALUE last FROM ONLY counter:board) ?? 0;",
                json!({}),
            )
        })
    }

    /// Read cached board inputs, refreshing only when a committed board mutation moved the marker.
    pub fn clock_board(&self) -> Result<super::Board, Error> {
        self.callback_board().map(|(board, _)| board)
    }

    fn dispatch_one(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        probes: &DispatchProbes<'_>,
        lease: Option<(Instant, u64)>,
    ) -> Result<(bool, f64), Error> {
        if lease.is_some() {
            (*board, *sequence) = self.callback_board()?;
        }
        let Stored {
            state,
            checked_pull,
            queued,
        } = self.retrying(|| {
            self.transact(
                "RETURN { state: (SELECT * OMIT id FROM ONLY type::record('callback_state', $handle)), checked_pull: type::record('callback_lease', $handle).pull ?? 0,
                    queued: (SELECT key, text, sequence, pull_entries, pull_withheld FROM callback_event WHERE handle = $handle AND delivered != true
                        AND (string::starts_with(key, 'pull-') OR string::starts_with(key, 'event-') OR (string::starts_with(key, 'pane-') AND string::starts_with(text, 'BLOCKED ')))) };",
                json!({"handle":handle}),
            )
        })?;
        let Some(mut state) = state else {
            return Ok((true, f64::INFINITY));
        };
        state.restore_unknown();
        // Planning can meet an observation newer than its shared snapshot. Recheck the marker
        // before diffing it, including unsequenced legacy observations. A lease also rechecks the
        // marker above, so a delayed plan cannot publish a transient from an older board.
        if state.snapshot_sequence > *sequence
            || (state.seen.is_some() && state.snapshot_sequence == 0)
        {
            (*board, *sequence) = self.callback_board()?;
        }
        let Some(session) = board
            .sessions
            .iter()
            .find(|s| s.row.handle == handle && s.row.left_at.is_none())
        else {
            return Ok((true, f64::INFINITY));
        };
        let orchestrator = session.row.kind == "orchestrator";
        // What the recipient hears, and the whole board's look older dispatchers keep.
        let mut seen = attach::seen(board, handle, &session.row.kind);
        let mut legacy = attach::legacy_seen(board, handle, &session.row.kind);
        let active: BTreeSet<&str> = board
            .tasks
            .iter()
            .filter(|task| !crate::health::FINISHED.contains(&task.status.as_str()))
            .map(|task| task.slug.as_str())
            .collect();
        let compact = |seen: &attach::Seen| {
            let mut seen = seen.clone();
            seen.statuses
                .retain(|slug, _| active.contains(slug.as_str()));
            seen.roles
                .retain(|(slug, _, _)| active.contains(slug.as_str()));
            seen.verdict_tasks
                .retain(|slug| active.contains(slug.as_str()));
            seen
        };
        // Baseline a legacy observer once. Its writer can later erase Seen's newer fields, but
        // cannot reset this marker or cursor and turn a new addressed message into old history.
        let initializing = state.delivery_version.is_none();
        let legacy_upgrade = initializing && state.seen.is_some();
        let first_observation = initializing && state.seen.is_none();
        let cursor = state
            .message_cursor
            .or_else(|| state.seen.as_ref().and_then(|before| before.latest_message));
        if let Some(before) = &mut state.seen {
            before.latest_message = cursor;
        }
        let message_floor = if legacy_upgrade {
            board.latest_message
        } else {
            cursor.unwrap_or(board.latest_message)
        };
        let before = state.heard();
        // Only an orchestrator uses the global status map to schedule its PULL digest. Workers
        // need their own seen state, not a copy of every task in each callback read and write.
        let statuses: BTreeMap<String, String> = if orchestrator {
            crate::orchestration::open_statuses(board)
        } else {
            BTreeMap::new()
        };
        let owners = crate::orchestration::Owners::new(&board.tasks);
        let hears = |statuses| crate::orchestration::heard_statuses(&owners, statuses, handle);
        // A line queued before a hand-over can name work the recipient no longer hears.
        let rescoped = if orchestrator && queued.iter().any(|line| !line.is_pull()) {
            Hearing::of(board, handle, &seen).rescope(&queued)
        } else {
            Rescoped::default()
        };
        let last_pull = state.pull.max(checked_pull);
        // A PULL still queued is never said as it was written, but built again (`pull_account`).
        let scheduled = orchestrator
            && (initializing
                || (state.pull_entries.is_none() && state.pull_queued.is_none())
                || self.now() - last_pull >= 600.0
                || hears(&statuses) != hears(&state.statuses)
                || queued.iter().any(Queued::is_pull));
        let changed = orchestrator && attach::pull_changed(before.as_ref(), &compact(&seen));
        let quiet = (seen.children.values())
            .chain(legacy.children.values())
            .any(|health| {
                matches!(
                    health,
                    crate::Health::Stale | crate::Health::Lost | crate::Health::Unknown
                )
            });
        let env = crate::cli::Env::of_process();
        let view = if scheduled || changed || quiet {
            Some(crate::cli::agents::describe_board(
                self,
                probes.get(),
                self.now(),
                false,
                board,
            )?)
        } else {
            None
        };
        let mut idle = Vec::new();
        let mut busy = Vec::new();
        for agent in view.as_ref().map_or(&[][..], |view| view.agents.as_slice()) {
            if agent.state == "running_test" {
                busy.push(agent.handle.clone());
                for look in [&mut seen, &mut legacy] {
                    if let Some(health) = look.children.get_mut(&agent.handle) {
                        *health = crate::Health::Live;
                    }
                }
            } else if agent.state == "idle"
                && !matches!(agent.kind.as_str(), "orchestrator" | "chat")
            {
                idle.push(agent.handle.clone());
            }
        }
        // Compare the same effective child health that observation checkpoints persist.
        let due =
            scheduled || (orchestrator && attach::pull_changed(before.as_ref(), &compact(&seen)));
        let pull_at = if !orchestrator {
            f64::INFINITY
        } else if due {
            self.now() + 600.0
        } else {
            last_pull + 600.0
        };
        if due && lease.is_none() {
            return self
                .with_callback_lease(handle, |deadline, revision| {
                    self.dispatch_one(
                        handle,
                        callback,
                        board,
                        sequence,
                        probes,
                        Some((deadline, revision)),
                    )
                    .map(|_| ())
                })
                .map(|handled| (handled, pull_at));
        }
        let mut lines = Vec::new();
        if let Some(before) = &before
            && !legacy_upgrade
            && before != &compact(&seen)
        {
            attach::diff(self, before, &seen, handle, &mut |line| {
                lines.push(event_line(line))
            })?;
        }
        // The recorded entries are the whole digest's, as older dispatchers record them; what
        // the recipient was told of them is in the PULLs it acknowledged ([`Account`]).
        let mut account: Option<Account> = None;
        let (mut pull_text, mut pull_current) = (String::new(), BTreeSet::new());
        if due {
            let root = crate::cli::probes::repo_root(&env.cwd);
            let landed = |task: &crate::Task| {
                root.as_ref()
                    .is_some_and(|root| crate::cli::probes::landed(root, &task.slug))
            };
            let digest = pull::digest(self, &idle, Some(&landed), self.now(), &busy)?;
            let entries = digest.entries();
            let digest = digest.only_for(board, handle);
            let heard = digest.entries();
            let said: Vec<String> = digest.event_lines().iter().map(|l| event_line(l)).collect();
            pull_text = said.join("\n");
            account = Some(match legacy_upgrade {
                true => Account::upgrade(&state, &entries, &heard),
                false => Account::of(&state, &queued, &entries, &heard),
            });
            pull_current = entries;
        }
        let plan = account.as_ref().map(|account| &account.plan);
        let pull_kept = match plan {
            Some(PullPlan::Kept(key)) => Some(key),
            _ => None,
        };
        let pull_replaced = matches!(plan, Some(PullPlan::Dropped | PullPlan::Queued));
        let revision = lease.map_or(state.revision, |(_, revision)| revision);
        let mut events: Vec<Event> = lines
            .into_iter()
            .enumerate()
            .map(|(i, text)| Event {
                key: format!("event-{revision}-{i}"),
                handle: handle.to_string(),
                text,
                slug: session_task(board, handle),
                message: None,
                attempts: 0,
                sequence: 0,
                pull_entries: None,
                pull_withheld: None,
            })
            .collect();
        if let Some(Account {
            plan: PullPlan::Queued,
            awaiting: Some(awaiting),
            ..
        }) = &account
        {
            events.push(Event {
                key: format!("pull-{revision}"),
                handle: handle.to_string(),
                text: pull_text.clone(),
                slug: session_task(board, handle),
                message: None,
                attempts: 0,
                sequence: 0,
                pull_entries: Some(awaiting.entries.clone()),
                pull_withheld: Some(awaiting.withheld.clone()),
            });
        }
        let legacy = compact(&legacy);
        let narrowing = attach::Narrowing::between(&legacy, &compact(&seen));
        let unchanged =
            state.seen.as_ref() == Some(&legacy) && state.narrowing.as_ref() == Some(&narrowing);
        // Message, role and task-tree writes append a board message. Once the inbox is durably
        // copied, an unchanged observation needs only outbox retries, not another history scan.
        let inbox = if state.inbox_cursor == Some(board.latest_message) && unchanged {
            Vec::new()
        } else if first_observation {
            self.inbox(handle)?
        } else {
            self.inbox_after(handle, message_floor)?
        };
        for message in inbox.into_iter().filter(|message| {
            message.id > message_floor
                || (first_observation
                    && (message.kind == "msg" || message.recipient.as_deref() == Some(handle)))
        }) {
            events.push(Event {
                key: format!("message-{}", message.id),
                handle: handle.to_string(),
                text: format!(
                    "{} #{} {} from {} — {}",
                    message.kind.to_uppercase(),
                    message.id,
                    message.slug,
                    message.sender,
                    event_line(&message.text)
                ),
                slug: Some(message.slug),
                message: Some(message.id),
                attempts: 0,
                sequence: 0,
                pull_entries: None,
                pull_withheld: None,
            });
        }
        if callback.chat {
            #[derive(Deserialize)]
            struct Chat {
                key: String,
                text: String,
            }
            let chat: Vec<Chat> = self.retrying(|| {
                self.transact(
                    "RETURN SELECT key, text FROM chat_event WHERE delivered = false
                        AND !record::exists(type::record('callback_event', [$handle, string::concat('chat-', key)])) ORDER BY key NUMERIC;",
                    json!({"handle":handle}),
                )
            })?;
            for event in chat {
                events.push(Event {
                    key: format!("chat-{}", event.key),
                    handle: handle.to_string(),
                    text: event.text,
                    slug: session_task(board, handle),
                    message: None,
                    attempts: 0,
                    sequence: 0,
                    pull_entries: None,
                    pull_withheld: None,
                });
            }
        }
        let (stored_seen, unknown_children) = stored_seen(&legacy, |handle| {
            let Some(child) = board.sessions.iter().find(|s| s.row.handle == handle) else {
                return crate::Health::Left;
            };
            let held = board.tasks.iter().any(|task| {
                task.roles.iter().any(|role| {
                    role.handle == handle && crate::health::claimable(&role.role, &task.status)
                })
            });
            crate::health::health(&child.row, self.now(), &self.host, &*self.pid_alive, held)
        });
        if events.is_empty()
            && !initializing
            && state.registration_revision == state.observed_registration
            && unchanged
            && state.statuses == statuses
            && state.inbox_cursor == Some(board.latest_message)
            && state.unknown_children == unknown_children
            && (account.as_ref()).is_none_or(|a| a.changes_nothing(&state, &queued, &pull_text))
            && rescoped.is_empty()
        {
            if due {
                // Fresh command Stores share this deadline even when the digest has no new
                // entry. Its scheduling checkpoint belongs to the small lease, not Seen.
                let revision = lease.expect("a timed checkpoint holds a lease").1;
                self.retrying(|| self.transact::<Value>(
                    "UPDATE type::record('callback_lease', $handle) SET pull = $now WHERE revision = $revision RETURN NONE; RETURN NONE;",
                    json!({"handle":handle, "revision":revision, "now":self.now()}),
                ))?;
            }
            if !self.callback_pending(handle, callback.chat)? {
                return Ok((true, pull_at));
            }
            if let Some((deadline, _)) = lease {
                return self
                    .deliver_pending(handle, callback, deadline)
                    .map(|()| (true, pull_at));
            }
        }
        if lease.is_none() {
            return self
                .with_callback_lease(handle, |deadline, revision| {
                    self.dispatch_one(
                        handle,
                        callback,
                        board,
                        sequence,
                        probes,
                        Some((deadline, revision)),
                    )
                    .map(|_| ())
                })
                .map(|handled| (handled, pull_at));
        }
        self.retrying(|| self.transact::<Value>(
            "IF $pull_kept != NONE {
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') AND key != $pull_kept RETURN NONE;
                UPDATE type::record('callback_event', [$handle, $pull_kept]) SET text = $pull_text, pull_entries = $pull_awaiting.entries,
                    pull_withheld = $pull_awaiting.withheld WHERE delivered != true RETURN NONE;
             };
             IF $replace_pull { DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') RETURN NONE; };
             FOR $key IN $rescoped.dropped { DELETE type::record('callback_event', [$handle, $key]) WHERE delivered != true RETURN NONE; };
             FOR $line IN $rescoped.rewritten { UPDATE type::record('callback_event', [$handle, $line[0]]) SET text = $line[1] WHERE delivered != true RETURN NONE; };
             FOR $new IN $events.filter(|$event| string::starts_with($event.key, 'event-') AND string::starts_with($event.text, 'NEW ')) {
                LET $prefix = string::concat('NEW ', string::split($new.text, ' ')[1], ' ready — ');
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'event-') AND string::starts_with(text, $prefix) RETURN NONE;
             };
             FOR $event IN $events {
                IF !record::exists(type::record('callback_event', [$handle, $event.key])) {
                    LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                    CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                    UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                };
             };
             IF $computed { UPDATE type::record('callback_lease', $handle) SET pull = $pull WHERE revision = $revision RETURN NONE; };
             LET $before = (SELECT seen, narrowing, unknown_children, statuses, pull_queued, inbox_cursor, message_cursor, delivery_version, registration_revision, observed_registration, revision FROM ONLY type::record('callback_state', $handle));
             IF $before.seen != $seen OR $before.narrowing != $narrowing OR $before.unknown_children != $unknown_children OR $before.statuses != $statuses
                OR $before.pull_queued != $pull_queued OR $before.inbox_cursor != $inbox_cursor
                OR ($before.message_cursor ?? 0) < $message_cursor OR $before.delivery_version = NONE
                OR ($before.registration_revision ?? 0) != ($before.observed_registration ?? 0) OR $computed {
             UPDATE type::record('callback_state', $handle) SET seen = $seen, narrowing = $narrowing, unknown_children = $unknown_children, snapshot_sequence = $snapshot_sequence, statuses = $statuses, revision = $revision, pull = $pull, pull_queued = $pull_queued,
                observed_registration = registration_revision ?? 0,
                pull_entries = IF $legacy_upgrade { $pull_account.recorded ?? pull_entries ?? [] }
                    ELSE IF $computed AND pull_entries != NONE { array::union(pull_entries.filter(|$entry| $entry IN $pull_current), $pull_account.unheard) } ELSE { pull_entries },
                pull_withheld = IF $computed { $pull_account.withheld } ELSE { pull_withheld },
                pull_awaiting = IF $computed { $pull_awaiting } ELSE { pull_awaiting },
                pull_withheld_revision = IF $computed OR (pull_withheld_revision != NONE AND pull_withheld_revision = $before.revision) { $revision } ELSE { pull_withheld_revision },
                message_cursor = math::max([message_cursor ?? 0, $message_cursor]),
                inbox_cursor = $inbox_cursor,
                delivery_version = delivery_version ?? 1 RETURN NONE;
             };
             RETURN NONE;",
            json!({"handle":handle, "events":events, "seen":stored_seen, "narrowing":narrowing, "unknown_children":unknown_children, "snapshot_sequence":sequence, "statuses":statuses, "revision":revision,
                "pull":if due {self.now()} else {state.pull}, "legacy_upgrade":legacy_upgrade,
                "pull_queued":account.as_ref().map_or(&state.pull_queued, |a| &a.queued),
                "pull_awaiting":account.as_ref().and_then(|a| a.awaiting.as_ref()),
                "pull_account":account.as_ref().map(|a| json!({"recorded":a.recorded, "unheard":a.unheard, "withheld":a.withheld})),
                "message_cursor":board.latest_message, "inbox_cursor":board.latest_message,
                "computed":due, "pull_current":pull_current, "rescoped":rescoped,
                "pull_kept":pull_kept, "pull_text":pull_text, "replace_pull":pull_replaced}),
        ))?;
        self.deliver_pending(handle, callback, lease.expect("writes hold a lease").0)
            .map(|()| (true, pull_at))
    }

    fn callback_pending(&self, handle: &str, chat: bool) -> Result<bool, Error> {
        self.retrying(|| self.transact(
            "RETURN array::len(SELECT key FROM callback_event WHERE handle = $handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) LIMIT 1) > 0;",
            json!({"handle":handle, "chat":chat, "now":self.now()}),
        ))
    }

    fn deliver_pending(
        &self,
        handle: &str,
        callback: &Callback,
        deadline: Instant,
    ) -> Result<(), Error> {
        let pending: Vec<Event> = self.retrying(|| self.transact(
            "UPDATE callback_event SET delivered = true WHERE handle = $handle AND delivered != true AND message != NONE AND record::exists(type::record('delivery', [message, $handle])) RETURN NONE;
             DELETE callback_event WHERE handle = $handle AND delivered = true RETURN NONE;
             RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;",
            json!({"handle":handle, "chat":callback.chat, "now":self.now()}),
        ))?;
        deliver_batches(
            &pending,
            deadline,
            &mut |batch| {
                let text = batch
                    .iter()
                    .map(|event| event.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                deliver(callback, &text)
            },
            &mut |batch, result| self.record_callback_delivery(handle, callback, batch, result),
        )
    }

    fn record_callback_delivery(
        &self,
        handle: &str,
        callback: &Callback,
        pending: &[Event],
        result: Result<(), DeliveryError>,
    ) -> Result<(), Error> {
        let command = self.callback_command();
        let success = result.is_ok();
        let capacity = result
            .as_ref()
            .is_err_and(|why| why.kind == DeliveryFailure::Capacity);
        let keys: Vec<&str> = pending.iter().map(|e| e.key.as_str()).collect();
        let chat_keys: Vec<&str> = pending
            .iter()
            .filter_map(|e| e.key.strip_prefix("chat-"))
            .collect();
        let messages: Vec<i64> = pending.iter().filter_map(|e| e.message).collect();
        let pull = pending
            .iter()
            .rev()
            .find(|event| event.pull_entries.is_some());
        self.retrying(|| self.transact::<Value>(
            "FOR $key IN $keys {
                UPDATE type::record('callback_event', [$handle, $key]) SET attempts += 1, delivered = $success, retry_at = $now + 5,
                    capacity_limited = $capacity OR (capacity_limited ?? false) RETURN NONE;
             };
             IF $success {
                IF $pull != NONE { UPDATE type::record('callback_state', $handle) SET pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_awaiting = NONE,
                    pull_withheld_revision = revision, pull_queued = NONE RETURN NONE; };
                FOR $message IN $messages { UPSERT type::record('delivery', [$message, $handle]) SET message = $message, handle = $handle, acked = $now RETURN NONE; };
                FOR $key IN $chat { UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; };
                UPDATE callback_event SET text = '' WHERE handle = $handle AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE;
             }; RETURN NONE;",
            json!({"handle":handle, "keys":keys, "messages":messages, "chat":chat_keys, "success":success, "now":self.now(),
                "capacity":capacity, "pull":pull}),
        ))?;
        if let Err(why) = result {
            if why.kind == DeliveryFailure::Dead {
                self.clear_callback(callback, &why.message)?;
            } else if why.kind == DeliveryFailure::Retry {
                self.callback_blocked(callback)?;
            }
            if why.kind != DeliveryFailure::Capacity
                && callback.chat
                && pending
                    .iter()
                    .any(|event| event.attempts == 2 && event.key.starts_with("chat-"))
            {
                // An unreachable recipient must not keep the chat seat indefinitely. A registered
                // orchestrator fallback can take over; otherwise the app explicitly says queued.
                self.retrying(|| self.transact::<Value>(
                    "LET $state = (SELECT address, owner, chat, chat_fallback, chat_resume FROM ONLY type::record('callback_state', $handle));
                     IF $state.address = $address AND $state.owner = $owner AND !($state.chat_resume ?? false) {
                        UPDATE type::record('callback_state', $handle) SET chat = false, chat_fallback = false, chat_resume = true,
                            chat_resume_seat = $state.chat ?? false, chat_resume_fallback = $state.chat_fallback ?? false RETURN NONE;
                     }; RETURN NONE;",
                    json!({"handle":handle, "address":callback.address, "owner":callback.owner}),
                ))?;
            }
            for event in pending.iter().filter(|event| event.attempts == 2) {
                if let Some(slug) = &event.slug {
                    let body = format!(
                        "{}\nRETURN NONE;",
                        logged("$slug", "'callbacks'", "'system'", "$text", "NONE")
                    );
                    // No reentrant dispatch from a delivery diagnostic.
                    self.retrying(|| {
                        self.client.query(&format!("BEGIN TRANSACTION; {body} COMMIT TRANSACTION;"), json!({
                            "slug":slug, "now":self.now(), "text":format!("CALLBACK {handle} failed after 3 attempts: {why}; retained in inbox ({command} inbox --as {handle})"),
                        }))?;
                        Ok(())
                    })?;
                }
            }
            eprintln!("CALLBACK {handle}: {why}; retained for inbox/retry");
        }
        Ok(())
    }
}

/// Capacity refusals send no bytes, so only they may split a batch during one attempt.
fn deliver_batches<T>(
    pending: &[T],
    deadline: Instant,
    send: &mut impl FnMut(&[T]) -> Result<(), DeliveryError>,
    record: &mut impl FnMut(&[T], Result<(), DeliveryError>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut start = 0;
    let mut end = pending.len();
    // A large queue may need many connections. Yield well before the recipient lease expires;
    // events not yet attempted remain due for the next dispatcher, with their budgets intact.
    while start < pending.len() {
        if Instant::now() >= deadline {
            break;
        }
        let result = send(&pending[start..end]);
        let capacity = result
            .as_ref()
            .is_err_and(|why| why.kind == DeliveryFailure::Capacity);
        if capacity && end - start > 1 {
            // A capacity refusal sent no bytes. Split only that failure; a short write or a
            // transport failure must retain the unsent events without trying them again here.
            end = start + (end - start) / 2;
            continue;
        }
        let transport_failed = result.is_err() && !capacity;
        if transport_failed {
            end = pending.len();
        }
        record(&pending[start..end], result)?;
        if transport_failed {
            break;
        }
        // A single oversized event stays in the inbox; later events still get their turn.
        start = end;
        end = pending.len();
    }
    Ok(())
}

fn event_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn recipient(callbacks: &[Callback]) -> Option<&str> {
    callbacks
        .iter()
        .find(|c| c.chat)
        .or_else(|| callbacks.iter().find(|c| c.chat_fallback))
        // Handles are stable; database row order and registration order do not choose chat.
        .or_else(|| {
            callbacks
                .iter()
                .filter(|c| c.live_orchestrator)
                .min_by(|a, b| a.handle.cmp(&b.handle))
        })
        .map(|c| c.handle.as_str())
}

fn session_task(board: &crate::Board, handle: &str) -> Option<String> {
    board
        .tasks
        .iter()
        .find(|task| task.roles.iter().any(|role| role.handle == handle))
        .map(|task| task.slug.clone())
}

fn validate(address: &str) -> Result<(), String> {
    if let Some(path) = address.strip_prefix("uds:") {
        if PathBuf::from(path).is_absolute() && !path.contains('\n') {
            return Ok(());
        }
    } else if let Some(id) = address.strip_prefix("codex:")
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    {
        return Ok(());
    }
    Err("callback must be uds:/absolute/socket or codex:THREAD-UUID".into())
}

#[derive(PartialEq, Eq)]
enum DeliveryFailure {
    Dead,
    Retry,
    Capacity,
}

struct DeliveryError {
    kind: DeliveryFailure,
    message: String,
}

impl DeliveryError {
    fn dead(message: impl std::fmt::Display) -> Self {
        Self {
            kind: DeliveryFailure::Dead,
            message: message.to_string(),
        }
    }
    fn retry(message: impl std::fmt::Display) -> Self {
        Self {
            kind: DeliveryFailure::Retry,
            message: message.to_string(),
        }
    }
    fn capacity(message: impl std::fmt::Display) -> Self {
        Self {
            kind: DeliveryFailure::Capacity,
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn deliver(callback: &Callback, text: &str) -> Result<(), DeliveryError> {
    let address = &callback.address;
    validate(address).map_err(DeliveryError::dead)?;
    if let Some(path) = address.strip_prefix("uds:") {
        let (mut socket, owner) =
            peer::connect(path).map_err(|error| connection_failure(callback, error))?;
        if callback.owner.as_ref() != Some(&owner) {
            return Err(DeliveryError::dead("socket owner changed before delivery"));
        }
        socket
            .set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(DeliveryError::retry)?;
        // Claude Code 2.1.308's installed socket handler accepts the SDK user envelope. This is
        // deliberately plain text; no claimed sender permission class and no configuration bypass.
        let mut payload =
            serde_json::to_vec(&json!({"type":"user", "message":{"role":"user", "content":text}}))
                .map_err(DeliveryError::retry)?;
        payload.push(b'\n');
        peer::reserve_send_buffer(&socket, payload.len()).map_err(|error| {
            DeliveryError::capacity(format!(
                "cannot reserve send buffer for {} envelope bytes; no bytes sent: {error}",
                payload.len()
            ))
        })?;
        // Only the complete newline-terminated envelope counts as delivered. Capacity is reserved
        // above so a paused peer can receive the whole frame. Never continue an unexpected short
        // write on this connection; close both directions and retain the envelope for retry.
        let result = match socket.write(&payload) {
            Ok(written) if written == payload.len() => Ok(()),
            Ok(written) => Err(DeliveryError::retry(format!(
                "socket wrote {written} of {} envelope bytes; closing incomplete frame",
                payload.len()
            ))),
            Err(error) => Err(DeliveryError::retry(error)),
        };
        if result.is_err() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        return result;
    }
    let thread = address.strip_prefix("codex:").unwrap_or("");
    if codex::steer(thread, text)? {
        return Ok(());
    }
    queue(thread, text)
}

/// `text` queued on Codex thread `thread` by `codex queue`, given 5 s. Started without a fork: a
/// connection the Symphony server accepted and has not yet marked close-on-exec stays out of it, so
/// that client's read to the end of its answer never waits for the queue.
fn queue(thread: &str, text: &str) -> Result<(), DeliveryError> {
    let mut child = Command::new("codex")
        .args(["queue", "--thread", thread, "--message", text])
        .env_remove("CLAUDE_CODE_MESSAGING_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(DeliveryError::retry)?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(DeliveryError::retry)? {
            Some(status) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(DeliveryError::retry(format!("codex queue exited {status}")))
                };
            }
            None if started.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DeliveryError::retry("codex queue timed out"));
            }
        }
    }
}

fn connection_failure(callback: &Callback, error: std::io::Error) -> DeliveryError {
    if callback.owner.as_ref().and_then(Owner::alive) == Some(false) {
        DeliveryError::dead(format!("registered listener process is gone: {error}"))
    } else {
        DeliveryError::retry(error)
    }
}

fn probe(callback: &Callback) -> Result<(), DeliveryError> {
    let path = callback
        .address
        .strip_prefix("uds:")
        .expect("only UDS channels are probed");
    let (_, owner) = peer::connect(path).map_err(|error| connection_failure(callback, error))?;
    if callback.owner.as_ref() == Some(&owner) {
        Ok(())
    } else {
        Err(DeliveryError::dead("socket owner changed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_orchestrator_tie_break_does_not_depend_on_callback_row_order() {
        let callback = |handle: &str| {
            let mut row: Callback = serde_json::from_value(json!({
                "handle":handle, "address":"codex:abcdef01", "host":"test",
                "session":crate::rows::SessionRow::default()
            }))
            .unwrap();
            row.live_orchestrator = true;
            row
        };
        for rows in [
            vec![callback("zinc-otter"), callback("amber-heron")],
            vec![callback("amber-heron"), callback("zinc-otter")],
        ] {
            assert_eq!(recipient(&rows), Some("amber-heron"));
        }
    }

    #[test]
    fn capacity_splits_between_events_and_never_loses_or_reorders_them() {
        let mut attempts = Vec::new();
        let mut delivered = Vec::new();
        deliver_batches(
            &[1, 2, 3, 4, 5],
            Instant::now() + Duration::from_secs(1),
            &mut |batch| {
                attempts.push(batch.to_vec());
                if batch.len() > 2 {
                    Err(DeliveryError::capacity("full"))
                } else {
                    Ok(())
                }
            },
            &mut |batch, result| {
                assert!(result.is_ok());
                delivered.extend_from_slice(batch);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            attempts,
            [
                vec![1, 2, 3, 4, 5],
                vec![1, 2],
                vec![3, 4, 5],
                vec![3],
                vec![4, 5]
            ]
        );
        assert_eq!(delivered, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn an_oversized_event_is_retained_but_does_not_block_smaller_events() {
        let mut recorded = Vec::new();
        deliver_batches(
            &[1, 2],
            Instant::now() + Duration::from_secs(1),
            &mut |batch| {
                if batch.contains(&1) {
                    Err(DeliveryError::capacity("large"))
                } else {
                    Ok(())
                }
            },
            &mut |batch, result| {
                recorded.push((batch.to_vec(), result.is_ok()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(recorded, [(vec![1], false), (vec![2], true)]);
    }

    #[test]
    fn a_transport_failure_retains_the_whole_batch_without_splitting_or_retrying_it() {
        let mut attempts = 0;
        let mut recorded = Vec::new();
        deliver_batches(
            &[1, 2, 3],
            Instant::now() + Duration::from_secs(1),
            &mut |_| {
                attempts += 1;
                Err(DeliveryError::retry("short write"))
            },
            &mut |batch, result| {
                recorded.push((batch.to_vec(), result.is_ok()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(attempts, 1);
        assert_eq!(recorded, [(vec![1, 2, 3], false)]);
    }

    /// Where a re-run of a queue test below finds its folder, whose stand-in is the only codex on
    /// its `PATH`.
    const QUEUE_CASE: &str = "SF_CODEX_QUEUE_CASE";

    /// The codex queue stand-in. In mode `check FD` it says whether it holds descriptor FD, then
    /// waits for its release; otherwise it says what it was given and ends as its mode says.
    const CODEX: &str = r#"#!/bin/sh
here=${0%/*}
read -r mode value < "$here/mode"
if [ "$mode" = check ]; then
    # Its own stderr, which it does hold, shows the shell can see a descriptor it has.
    if test ! -e /dev/fd/2; then held=blind; elif test -e "/dev/fd/$value"; then held=held;
    else held=free; fi
    printf %s "$held" > "$here/checked"
    read -r line < "$here/release"
    exit 0
fi
printf '%s\0' "$@" > "$here/argv"
input=other; output=other; error=other
test /dev/fd/0 -ef /dev/null && input=null
test /dev/fd/1 -ef /dev/null && output=null
test /dev/fd/2 -ef /dev/null && error=null
printf '%s|' "${CLAUDE_CODE_MESSAGING_TOKEN-unset}" "${SF_CODEX_QUEUE_KEPT-unset}" \
    "$input" "$output" "$error" > "$here/saw"
printf %s "$$" > "$here/pid"
case $mode in
    exit) exit "$value" ;;
    killed) kill -KILL $$ ;;
    hang) exec sleep 60 ;;
esac
"#;

    /// A folder of its own under the temporary one, holding the stand-in.
    fn stand_in(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sf-codex-queue-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fixture_script::write(&dir.join("codex"), CODEX, false);
        dir
    }

    /// Test `name` run again in a process of its own, with `dir`'s stand-in the only codex on its
    /// `PATH` and a messaging token set: set here, they would reach every other test's children too.
    /// None of its streams is `/dev/null`, so a queue that inherited one would not read null.
    fn rerun(name: &str, dir: &std::path::Path) {
        let case = crate::sys::spawn_piped(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &format!("store::callbacks::tests::{name}")])
                .env(QUEUE_CASE, dir)
                .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
                .env("CLAUDE_CODE_MESSAGING_TOKEN", "a-token")
                .env("SF_CODEX_QUEUE_KEPT", "kept"),
        )
        .unwrap();
        let done = crate::sys::wait_output(case).unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        assert!(done.status.success(), "{said}");
        assert!(said.contains("test result: ok. 1 passed"), "{said}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// How a delivery ended: a refusal is always one the outbox retries.
    fn answered(result: Result<(), DeliveryError>) -> Result<(), String> {
        result.map_err(|error| {
            assert!(error.kind == DeliveryFailure::Retry, "{error}");
            error.message
        })
    }

    #[test]
    fn codex_queue_takes_the_message_whole_without_the_token_and_answers_how_it_ended() {
        let Some(dir) = std::env::var_os(QUEUE_CASE).map(PathBuf::from) else {
            let dir = stand_in("contract");
            rerun(
                "codex_queue_takes_the_message_whole_without_the_token_and_answers_how_it_ended",
                &dir,
            );
            return;
        };
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        let queued = |mode: &str| {
            std::fs::write(dir.join("mode"), mode).unwrap();
            answered(queue("abcdef01", "AGENT from a to b — one\n-- 'two' $HOME"))
        };
        assert_eq!(queued("exit 0"), Ok(()));
        assert_eq!(
            read("argv"),
            "queue\0--thread\0abcdef01\0--message\0AGENT from a to b — one\n-- 'two' $HOME\0"
        );
        assert_eq!(read("saw"), "unset|kept|null|null|null|");
        assert_eq!(
            queued("exit 3"),
            Err("codex queue exited exit status: 3".into())
        );
        assert_eq!(
            queued("killed"),
            Err("codex queue exited signal: 9 (SIGKILL)".into())
        );
        let began = Instant::now();
        assert_eq!(queued("hang"), Err("codex queue timed out".into()));
        // Well short of the stand-in's own 60 s: the kill at 5 s is what ended it.
        let took = began.elapsed();
        assert!(
            took >= Duration::from_secs(5) && took < Duration::from_secs(30),
            "{took:?}"
        );
        let pid: i64 = read("pid").parse().unwrap();
        assert!(
            !crate::sys::pid_alive(pid),
            "the timed-out queue is ended and reaped"
        );
        std::fs::remove_file(dir.join("codex")).unwrap();
        assert_eq!(
            queued("exit 0"),
            Err(std::io::Error::from_raw_os_error(libc::ENOENT).to_string())
        );
    }

    /// The Symphony server's case: a connection accepted on macOS is not close-on-exec until a second
    /// call marks it, and a delivery started between the two must neither hold it nor keep its client
    /// waiting for the end of its answer.
    #[test]
    fn an_accepted_connection_not_yet_close_on_exec_reaches_no_codex_queue() {
        use std::fs::File;
        use std::io::Read;
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        let Some(dir) = std::env::var_os(QUEUE_CASE).map(PathBuf::from) else {
            let dir = stand_in("descriptor");
            rerun(
                "an_accepted_connection_not_yet_close_on_exec_reaches_no_codex_queue",
                &dir,
            );
            return;
        };
        let release = dir.join("release");
        let name = std::ffi::CString::new(release.as_os_str().as_bytes()).unwrap();
        // SAFETY: the path is NUL-terminated and outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        // SAFETY: this test owns the descriptor; only its close-on-exec flag changes.
        assert_eq!(
            unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_SETFD, 0) },
            0
        );
        std::fs::write(dir.join("mode"), format!("check {}", accepted.as_raw_fd())).unwrap();
        std::thread::scope(|scope| {
            let queued = scope.spawn(|| {
                let queued = answered(queue("abcdef01", "a message"));
                // A queue that never opened its release still lets the wait for it below end.
                let reader = File::options()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&release);
                (queued, reader)
            });
            // This opens once the queue waits on its release: it started while the connection was
            // open here. Dropped however the test ends, it lets the queue end.
            let mut go = File::options().write(true).open(&release).unwrap();
            assert_eq!(
                std::fs::read_to_string(dir.join("checked")).unwrap(),
                "free",
                "codex queue holds the connection"
            );
            // The server answers and closes its end: the client reads to the answer's end while
            // the queue runs.
            (&accepted).write_all(b"answer").unwrap();
            drop(accepted);
            let mut said = Vec::new();
            client.read_to_end(&mut said).unwrap();
            assert_eq!(said, b"answer");
            writeln!(go, "go").unwrap();
            drop(go);
            let (queued, _reader) = queued.join().unwrap();
            assert_eq!(queued, Ok(()));
        });
    }
}
