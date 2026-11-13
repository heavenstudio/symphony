//! Native session channels and a durable outbox. Every write observes the board for each recipient
//! before it answers, so what a recipient hears is read against the board as that write left it.
//! A library store's write delivers too; a command line's leaves the delivery to the detached
//! dispatcher it starts, one a host at a time ([`Store::dispatching_by`],
//! [`Store::dispatch_detached`]), so the command answers without waiting on any recipient. The
//! Symphony server's clock drains retries and observes actionable PULL/STALE/LOST changes. No agent
//! keeps a monitor alive.

use super::callback_peer::{self as peer, Owner};
use super::callback_state::{State, stored_seen};
use super::launches::{HOLDS, held};
use super::pull_account::{Account, PullPlan};
use super::queued_lines::{Hearing, Queued, Rescoped};
use super::write::FENCE_RECIPIENT;
use super::{ACT, Error, Store, logged, refused};
use crate::cli::agents::{Answer, Pane};
use crate::cli::{attach, probes, pull};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

#[path = "codex_delivery.rs"]
mod codex;
mod visit;
use visit::Visit;

#[path = "pane_watch.rs"]
mod pane_watch;

#[cfg(test)]
#[path = "../../../../tools/test-support/script_process_cwd.rs"]
mod fixture_script;

/// How long one dispatcher holds a recipient: past it, another may observe and deliver for it.
pub(super) const CALLBACK_LEASE_SECONDS: u64 = 60;

/// How long a delivery may go on after it renewed its recipient's lease, by the board's clock: well
/// inside the lease, so no batch begins once another dispatcher could hold the recipient.
const DELIVERY_SECONDS: f64 = Duration::from_secs(CALLBACK_LEASE_SECONDS / 2).as_secs_f64();

/// What a dispatch does for each recipient on its host. Each observes the board for it and queues
/// what it hears in its outbox; a write whose delivery a detached dispatcher makes stops there.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    Observe,
    Deliver,
    /// A session with no channel reads what a delivery would send it (`tasks wait`): the pass
    /// plans as a delivering one does, and what it would send goes to the reader instead
    /// ([`Visit::send`]), recorded as delivered.
    Collect,
}

pub(super) struct Idle {
    marker: u64,
    health: Vec<crate::Health>,
    pull_at: f64,
    holdings: Option<pull::Holdings>,
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
    /// What a PULL says, which its acknowledgement records as the next one's baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pull_said: Option<pull::Said>,
}

/// A recipient's callback state as a dispatch reads it, with when its PULL was last checked and
/// the lines still queued for it, which a hand-over can take back ([`Hearing::rescope`]).
#[derive(Deserialize)]
struct Stored {
    state: Option<State>,
    checked_pull: f64,
    queued: Vec<Queued>,
}

/// A recipient's observation as written: whether the dispatcher still held the recipient's lease,
/// whether the PULLs it read as waiting still were, and how the write moved the board marker.
#[derive(Deserialize)]
struct Accounted {
    held: bool,
    current: bool,
    marker: Moved,
}

/// The board marker just before and just after one of a dispatcher's own writes.
#[derive(Deserialize)]
struct Moved {
    before: u64,
    after: u64,
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
    /// The hc terminal the registering agent ran in (`HC_TERMINAL_ID`), if it ran in one: a
    /// Codex thread is present while that terminal runs Codex.
    #[serde(default)]
    pub terminal: Option<String>,
    /// How many times the handle has registered since its row was made: a number a registration
    /// made after a retirement (which deletes the row) takes again.
    #[serde(default)]
    registration_revision: u64,
    /// Which registration this is: a random id each registration writes, so one made since never
    /// has it. None on a row an older build wrote, which writes none.
    #[serde(default)]
    registration_nonce: Option<String>,
    #[serde(skip)]
    live_orchestrator: bool,
}

/// What hc says of the machine's Codex agents, for [`Store::callback_presence`]: the live
/// terminals running Codex, and the threads known to run in one of them (a recorded launch's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Presence {
    pub terminals: BTreeSet<String>,
    pub threads: BTreeSet<String>,
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
        let board_command = self.board_command();
        let key: String = self.retrying(|| self.transact(
            &format!("{ACT}
LET $recipient = (SELECT left_at FROM ONLY type::record('session', $handle));
IF $recipient = NONE {{ THROW string::concat('refused: no session ', $handle); }};
IF $recipient.left_at != NONE {{ THROW string::concat('refused: session ', $handle, ' has left'); }};
{FENCE_RECIPIENT}
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
            self.acting(sender, self.now(), json!({"handle":handle, "text":text,
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
        self.callback_in(handle, address, chat, if_free, None)
    }

    /// [`Store::callback`] from the hc terminal `terminal`, if the agent runs in one. A Codex
    /// thread also completes the conversation its open launch records left unknown (`codex:`), so
    /// that `restore` can resume it.
    ///
    /// A new channel starts from the whole digest: turning the channel off, registering one where
    /// none was (after `--off` or a `tasks wait`) or one another process listens on forgets which
    /// PULL the session acknowledged, and a PULL waiting for it gives way to the whole digest. The
    /// posts it was told stay told. Only the same listener moving to a new socket keeps its
    /// account, so what waited out an outage is said there as it stands.
    pub fn callback_in(
        &self,
        handle: &str,
        address: Option<&str>,
        chat: bool,
        if_free: bool,
        terminal: Option<&str>,
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
UPSERT callback_state:chat SET revision = (revision ?? 0) + 1, waiting = array::complement(waiting ?? [], [$actor]) RETURN NONE;
LET $agent = type::record('session', $actor).agent;
LET $binding = IF $agent = NONE {{ NONE }} ELSE {{ type::record('callback_state', ['binding', $agent]).in_process }};
IF $address != NONE AND $agent != NONE AND $binding = NONE {{ THROW string::concat('refused: binding provenance is not recorded yet; retry just tasks callback --as ', $actor, ' so the current hook can classify the session'); }};
IF $address != NONE AND $agent != NONE AND $binding = true {{ THROW 'refused: in-process agent binding cannot register a callback; use just tasks inbox'; }};
LET $other = (SELECT VALUE handle FROM callback_state WHERE address = $address AND handle != $actor AND type::record('session', handle).left_at = NONE ORDER BY handle LIMIT 1)[0];
IF $address != NONE AND $other != NONE {{ THROW string::concat('refused: callback address already held by ', $other, '; in-process subagents use just tasks inbox'); }};
LET $old = (SELECT VALUE address FROM ONLY type::record('callback_state', $actor));
LET $old_owner = (SELECT VALUE owner FROM ONLY type::record('callback_state', $actor));
IF $old != $address AND ($old = NONE OR $address = NONE OR $old_owner = NONE OR $old_owner != $owner) {{
    UPDATE type::record('callback_state', $actor) SET pull_entries = NONE, pull_said = NONE, pull_withheld = [], pull_awaiting = NONE, pull_queued = NONE RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle = $actor AND delivered != true AND pull_entries != NONE RETURN NONE;
}};
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
    registration_revision = (registration_revision ?? 0) + 1, registration_nonce = rand::string(32),
    owner = $owner, confirmed_until = 0, chat_resume = false, terminal = $terminal,
    chat = IF $address = NONE {{ false }} ELSE IF $take {{ true }} ELSE {{ chat ?? false }} RETURN NONE;
IF $address != NONE AND string::starts_with($address, 'codex:') {{
    UPDATE task_session SET claude_session = $address WHERE handle = $actor AND closed = NONE AND claude_session = 'codex:' RETURN NONE;
}};
RETURN NONE;");
        self.retrying(|| {
            self.transact::<Value>(
                &body,
                self.acting(
                    handle,
                    self.now(),
                    json!({
                        "address":address, "chat":chat, "if_free":if_free, "host":self.host, "owner":owner,
                        "terminal":terminal.filter(|t| !t.is_empty() && address.is_some()),
                    }),
                ),
            )
        })?;
        self.dispatch_or_hand_off()
    }

    /// Who receives the owner's chat events: a registered channel's session, as [`recipient`]
    /// chooses one; while none can, the waiting orchestrator ([`Store::waiting_orchestrator`]).
    pub fn chat_recipient(&self) -> Result<Option<String>, Error> {
        match recipient(&self.cached_callbacks()?) {
            Some(handle) => Ok(Some(handle.to_owned())),
            None => self.waiting_orchestrator(),
        }
    }

    /// The orchestrator that holds the vacant chat seat by waiting (`tasks wait`): of the sessions
    /// that read by waiting and registered no channel since, the live orchestrators', the first by
    /// handle. A waiter that stops waiting holds it while it stays live, so what the owner says
    /// waits for its next read rather than for nobody.
    fn waiting_orchestrator(&self) -> Result<Option<String>, Error> {
        let waiting: Vec<crate::rows::SessionRow> = self.retrying(|| self.read(
            "LET $waiting = (SELECT VALUE waiting FROM ONLY callback_state:chat) ?? [];
             RETURN SELECT * OMIT id FROM $waiting.map(|$h| type::record('session', $h))
                WHERE kind = 'orchestrator' AND left_at = NONE AND type::record('callback_state', handle).address = NONE;",
            json!({}),
        ))?;
        let live = |s: &&crate::rows::SessionRow| {
            crate::health::health(s, self.now(), &self.host, &*self.pid_alive, false)
                == crate::Health::Live
        };
        Ok((waiting.iter().filter(live))
            .map(|s| s.handle.clone())
            .min())
    }

    /// Whether `handle` reads the owner's chat events: it holds the seat, or nobody does and it is
    /// an orchestrator, which then reads them on demand (`tasks inbox`, `tasks pull`).
    fn reads_chat(&self, handle: &str) -> Result<bool, Error> {
        Ok(match self.chat_recipient()? {
            Some(recipient) => recipient == handle,
            None => self.session_kind(handle)?.as_deref() == Some("orchestrator"),
        })
    }

    fn session_kind(&self, handle: &str) -> Result<Option<String>, Error> {
        self.retrying(|| {
            self.read(
                "RETURN (SELECT VALUE kind FROM ONLY type::record('session', $handle));",
                json!({"handle":handle}),
            )
        })
    }

    /// The owner's chat events no session received: what an orchestrator reads on demand while
    /// nobody holds the seat ([`Store::reads_chat`]), each keyed as its callback is (`chat-N`).
    pub fn unheard_owner_events(&self, handle: &str) -> Result<Vec<(String, String)>, Error> {
        if self.chat_recipient()?.is_some()
            || self.session_kind(handle)?.as_deref() != Some("orchestrator")
        {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct Chat {
            key: String,
            text: String,
        }
        let chat: Vec<Chat> = self.retrying(|| {
            self.read(
                "RETURN SELECT key, text FROM chat_event WHERE delivered = false ORDER BY key NUMERIC;",
                json!({}),
            )
        })?;
        Ok(chat
            .into_iter()
            .map(|c| (format!("chat-{}", c.key), c.text))
            .collect())
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
        self.retrying(|| self.read(
            "RETURN SELECT handle, address, host, terminal, registration_revision ?? 0 AS registration_revision, registration_nonce, chat ?? false AS chat, chat_fallback ?? false AS chat_fallback, owner, confirmed_until ?? 0 AS confirmed_until,
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

    /// Retire the registration `callback` was read as, exactly: its address, listener, terminal (or
    /// none), `registration_revision` and `registration_nonce` (or none), so never one registered
    /// since, though from the same listener and terminal and with the same number. Whether it
    /// retired it. The one registration it cannot tell from the one read is a row an older build
    /// (which writes no nonce) made again just as it was: the same address, listener and terminal,
    /// and numbered as the one read.
    pub(crate) fn clear_callback(&self, callback: &Callback, why: &str) -> Result<bool, Error> {
        self.retire_registration(callback, why, None)
    }

    /// [`Store::clear_callback`] by the close holding the handle's session as `hold`
    /// ([`Store::take_session`]): refused, retiring nothing, once the hold is another's
    /// ([`HOLDS`]).
    pub(crate) fn clear_callback_held(
        &self,
        callback: &Callback,
        why: &str,
        hold: u64,
    ) -> Result<bool, Error> {
        self.retire_registration(callback, why, Some(hold))
    }

    fn retire_registration(
        &self,
        callback: &Callback,
        why: &str,
        hold: Option<u64>,
    ) -> Result<bool, Error> {
        let mut vars = json!({
            "handle":callback.handle, "address":callback.address, "owner":callback.owner,
            "terminal":callback.terminal, "revision":callback.registration_revision,
            "nonce":callback.registration_nonce,
        });
        let mut guard = "";
        if let Some(hold) = hold {
            held(&mut vars, &callback.handle, hold);
            guard = HOLDS;
        }
        let body = format!("{guard}
             LET $state = (SELECT address, owner, terminal, registration_revision ?? 0 AS revision, registration_nonce AS nonce FROM ONLY type::record('callback_state', $handle));
             IF $state.address != $address OR $state.owner != $owner OR $state.terminal != $terminal OR $state.revision != $revision OR $state.nonce != $nonce {{ RETURN false }};
             DELETE type::record('callback_state', $handle) RETURN NONE;
             UPDATE callback_event SET delivered = true, text = '' WHERE handle = $handle AND string::starts_with(key, 'chat-') RETURN NONE;
             RETURN true;");
        let retired = self.retrying(|| self.transact::<bool>(&body, vars.clone()))?;
        if retired {
            eprintln!(
                "CALLBACK {} retired: {why}; messages remain in inbox",
                callback.handle
            );
        }
        Ok(retired)
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
    /// exit. A socket must accept a connection; a Codex thread must still occupy a terminal: one a
    /// recorded launch puts it in, else the terminal it registered from, which must still run
    /// Codex. A thread registered from no terminal, that no launch records, is neither confirmed
    /// nor retired: hc cannot say where it runs. A dry run reports the same channels without
    /// changing their heartbeat. No payload or credentials are sent by the socket probe. Without
    /// `codex` (hc did not answer) no thread is confirmed or retired.
    pub(crate) fn callback_presence(
        &self,
        codex: Option<&Presence>,
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
                let Some(codex) = codex else {
                    continue;
                };
                let thread = callback.address.strip_prefix("codex:").unwrap_or_default();
                if codex.threads.contains(thread) {
                    true
                } else if let Some(terminal) = &callback.terminal {
                    codex.terminals.contains(terminal)
                } else {
                    continue;
                }
            };
            if alive && (!refresh || self.callback_seen(&callback)?) {
                present.insert(callback.handle);
            } else if !alive && refresh {
                self.clear_callback(&callback, "thread no longer occupies a terminal")?;
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

    /// Failed or pending non-message events also remain readable through the ordinary inbox, and,
    /// while nobody holds the chat seat, an orchestrator's lists the owner's chat events nobody
    /// received ([`Store::unheard_owner_events`]).
    pub fn callback_inbox(&self, handle: &str) -> Result<Vec<(String, String)>, Error> {
        let chat = self.reads_chat(handle)?;
        let events: Vec<Event> = self.retrying(|| self.transact(
            "RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND message = NONE
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;",
            json!({"handle":handle, "chat":chat}),
        ))?;
        let mut events: Vec<(String, String)> = (events.into_iter())
            .map(|event| (event.key, event.text))
            .collect();
        for (key, text) in self.unheard_owner_events(handle)? {
            if !events.iter().any(|(queued, _)| *queued == key) {
                events.push((key, text));
            }
        }
        Ok(events)
    }

    pub fn ack_callbacks(&self, handle: &str, keys: &[String]) -> Result<(), Error> {
        let chat = self.reads_chat(handle)?;
        let chats: Vec<&str> = keys
            .iter()
            .filter(|_| chat)
            .filter_map(|key| key.strip_prefix("chat-"))
            .collect();
        self.retrying(|| self.transact::<Value>(
            &format!("{ACT}\nLET $pull = (SELECT pull_entries, pull_withheld, pull_said, sequence FROM callback_event WHERE handle = $actor AND key IN $keys AND pull_entries != NONE ORDER BY sequence DESC LIMIT 1)[0];
                IF $pull != NONE {{ UPDATE type::record('callback_state', $actor) SET pull_unanswered = array::union(pull_unanswered ?? [], $pull.pull_entries.filter(|$entry| $entry[0] = 'unanswered' AND $entry NOT IN ($pull.pull_withheld ?? []))), pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_said = $pull.pull_said, pull_awaiting = NONE, pull_queued = NONE RETURN NONE; }};
                FOR $key IN $keys {{ UPDATE type::record('callback_event', [$actor, $key]) SET delivered = true RETURN NONE; }};
                FOR $key IN $chats {{ UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; }};
                UPDATE callback_event SET text = '' WHERE handle = $actor AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE; RETURN NONE;"),
            self.acting(handle, self.now(), json!({"keys":keys, "chats":chats})),
        )).map(|_| ())
    }

    /// One look for `handle`, a session with no channel (`tasks wait`): what a dispatch would queue
    /// and deliver to it, each text printed by `print` instead and, once printed whole, recorded as
    /// delivered, so the next look says only what changed since; how many texts it printed, and
    /// the board's marker as the look left it. It is the dispatch's own pass ([`Pass::Collect`]),
    /// so it says what a channel would carry, by the same diff and the same account, from the
    /// same state. Refused while a channel is registered, which takes those, and for a session not
    /// here.
    pub fn collect_callbacks(
        &self,
        handle: &str,
        env: &crate::cli::Env,
        print: &mut dyn FnMut(&str) -> std::io::Result<()>,
    ) -> Result<(usize, u64), Error> {
        #[derive(Deserialize)]
        struct State {
            address: Option<String>,
            #[serde(default)]
            pane_watch: Option<attach::PaneWatch>,
        }
        #[derive(Deserialize)]
        struct Reader {
            state: Option<State>,
            session: Option<crate::rows::SessionRow>,
            waiting: bool,
        }
        // A read, once the reader has its state row and is on the chat seat's waiting list.
        let read = || {
            self.read::<Reader>(
                "RETURN {
                    session: (SELECT * OMIT id FROM ONLY type::record('session', $handle)),
                    state: (SELECT address, pane_watch FROM ONLY type::record('callback_state', $handle)),
                    waiting: $handle IN ((SELECT VALUE waiting FROM ONLY callback_state:chat) ?? []),
                };",
                json!({"handle":handle}),
            )
        };
        let mut reader = self.retrying(read)?;
        let here = reader.session.as_ref().is_some_and(|s| s.left_at.is_none());
        let unaddressed = reader.state.as_ref().is_none_or(|s| s.address.is_none());
        if here && unaddressed && (reader.state.is_none() || !reader.waiting) {
            self.retrying(|| self.transact::<Value>(
                "IF type::record('session', $handle).left_at = NONE AND type::record('callback_state', $handle).address = NONE {
                    IF !record::exists(type::record('callback_state', $handle)) {
                        CREATE type::record('callback_state', $handle) SET handle = $handle RETURN NONE;
                    };
                    UPSERT callback_state:chat SET waiting = array::union(waiting ?? [], [$handle]) RETURN NONE;
                 }; RETURN NONE;",
                json!({"handle":handle}),
            ))?;
            reader = self.retrying(read)?;
        }
        let state = reader.state.unwrap_or(State {
            address: None,
            pane_watch: None,
        });
        if let Some(address) = state.address {
            return Err(refused(format!(
                "{handle} has a channel ({address}), which takes what is pushed to it; read it there, or run {} callback --off --as {handle} first",
                self.board_command()
            )));
        }
        let Some(session) = reader.session.filter(|s| s.left_at.is_none()) else {
            return Err(refused(format!("no session {handle} here to wait for")));
        };
        let callback = Callback {
            handle: handle.to_string(),
            address: String::new(),
            host: self.host.clone(),
            // The owner's chat events, while it holds the seat: no channel does, and it is the
            // waiting orchestrator ([`Store::chat_recipient`]).
            chat: self.chat_recipient()?.as_deref() == Some(handle),
            chat_fallback: false,
            owner: None,
            in_process: false,
            confirmed_until: 0.0,
            session,
            chat_resume: false,
            pane_watch: state.pane_watch.unwrap_or_default(),
            terminal: None,
            registration_revision: 0,
            registration_nonce: None,
            live_orchestrator: false,
        };
        // Each look reads the messages afresh, as each pass does.
        *self.inbox_pool.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let (mut board, mut sequence) = self.callback_board()?;
        let holdings = (callback.session.kind == "orchestrator")
            .then(|| self.slot_holdings())
            .transpose()?;
        let visit = Visit::collecting(env, self, holdings.as_ref(), print);
        // The terminals it oversees, as the server's clock watches a channel's: one sample a look.
        let open = match attach::overseen(&board, handle).is_empty() {
            true => Vec::new(),
            false => self.open_task_sessions()?,
        };
        let sample = std::cell::OnceCell::new();
        let panes = || (sample.get_or_init(|| crate::cli::probes::hc_terminals(env))).clone();
        self.watch_pane(&board, &callback, &open, &panes, &visit)?;
        self.dispatch_one(handle, &callback, &mut board, &mut sequence, &visit, None)?;
        Ok((visit.printed(), sequence))
    }

    /// The board's marker: it moves with every committed board write.
    pub fn board_mark(&self) -> Result<u64, Error> {
        self.board_marker()
    }

    /// The app calls this after a successful owner save. The event and any undelivered record stay
    /// in the board even when no recipient has registered yet. key is stable across save retries.
    pub fn owner_callback(&self, key: &str, text: &str) -> Result<(), Error> {
        self.owner_save(key, text, None)
    }

    /// [`Store::owner_callback`], and in the same write the ask the save opens or joins
    /// ([`super::asks::OWNER_SAVE`]): once per key, so a save handed over again changes nothing. A
    /// save with no words to deliver (empty `text`: a rejection with no note) delivers no event and
    /// only opens its ask.
    pub fn owner_save(
        &self,
        key: &str,
        text: &str,
        ask: Option<&super::NewAsk>,
    ) -> Result<(), Error> {
        let mut vars = match ask {
            Some(new) => self.ask_vars(new)?,
            None => json!({}),
        };
        vars["key"] = key.into();
        vars["text"] = text.into();
        vars["asking"] = ask.is_some().into();
        self.retrying(|| self.transact::<Value>(&super::asks::OWNER_SAVE, vars.clone()))?;
        self.dispatch_or_hand_off()
    }

    /// This store, its writes leaving their callback delivery to `hand_off` instead of making it
    /// before they answer: a command line's, whose hand-off starts a detached dispatcher
    /// ([`Store::dispatch_detached`]). The write still observes the board for every recipient first,
    /// so each hears it as the write left the board. A hand-off that says it started nothing leaves
    /// the delivery to the write, as a store without one does (the Symphony server's, whose clock
    /// dispatches too).
    pub fn dispatching_by(mut self, hand_off: super::HandOff) -> Store {
        self.hand_off = Some(hand_off);
        self
    }

    /// What a committed write that may concern a recipient does: observes the board for each and
    /// hands the delivery over, or makes it. Never fails the write: what went wrong goes to stderr,
    /// and the outbox keeps the rest for the next dispatch.
    pub(super) fn after_write(&self) {
        if let Err(error) = self.dispatch_or_hand_off() {
            eprintln!("CALLBACK deferred: {error}");
        }
    }

    fn dispatch_or_hand_off(&self) -> Result<(), Error> {
        let Some(hand_off) = &self.hand_off else {
            return self.dispatch_callbacks();
        };
        // What each recipient hears is queued against the board as this write left it: a later
        // write (a hand-over, a move) cannot reroute or fold it before the dispatcher delivers.
        let observed = self.dispatch_pass(Pass::Observe).map(|_| ());
        if !hand_off() {
            self.dispatch_callbacks()?;
        }
        observed
    }

    pub fn dispatch_callbacks(&self) -> Result<(), Error> {
        self.dispatch_pass(Pass::Deliver).map(|_| ())
    }

    /// One pass over the recipients on this host: whether it reached every one, none of whose
    /// lease another dispatcher held.
    pub(super) fn dispatch_pass(&self, pass: Pass) -> Result<bool, Error> {
        let callbacks = self.callbacks()?;
        if callbacks.is_empty() {
            return Ok(true);
        }
        self.dispatch_board(&callbacks, pass)
            .map(|(_, reached)| reached)
    }

    /// The server clock reuses this sequenced snapshot for its read models after delivery. Only the
    /// clock watches panes: a write delivers what it caused, and a pane's prompt is no write. The
    /// watch looks first, so the one hc sample it takes serves the tick's PULL view and the
    /// clock's agents view after it ([`Store::clock_panes`]).
    pub fn dispatch_callbacks_snapshot(&self) -> Result<Option<super::Board>, Error> {
        *self.clock_panes.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let callbacks = self.callbacks()?;
        if callbacks.is_empty() {
            return Ok(None);
        }
        let (board, _) = self.callback_board()?;
        self.watch_panes(&board, &callbacks)?;
        self.dispatch_board(&callbacks, Pass::Deliver)
            .map(|(board, _)| Some(board))
    }

    /// The hc terminals this tick of the server clock sampled, when its pane watch took a sample.
    pub(crate) fn clock_panes(&self) -> Option<Answer<Vec<Pane>>> {
        self.clock_panes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The board as the pass read it, and whether the pass reached every recipient on this host: one
    /// whose lease another dispatcher held is left to that one, and to the next pass.
    fn dispatch_board(
        &self,
        callbacks: &BTreeMap<String, Callback>,
        pass: Pass,
    ) -> Result<(super::Board, bool), Error> {
        // Each pass reads the messages afresh, once for every recipient ([`super::pool::Pool`]).
        // The pool is plain data, whole or none, so a panic elsewhere leaves nothing half-made in it.
        *self.inbox_pool.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let (mut board, mut sequence) = self.callback_board()?;
        let holdings = callbacks
            .values()
            .any(|callback| callback.host == self.host && callback.session.kind == "orchestrator")
            .then(|| self.slot_holdings())
            .transpose()?;
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
                    && idle.holdings == holdings
                    && idle.health == health
                    && self.now() < idle.pull_at
                    && board.sessions.iter().all(|session| {
                        matches!(session.health, crate::Health::Live | crate::Health::Left)
                    })
            });
        if idle {
            let mut reached = true;
            for (handle, callback) in callbacks.iter().filter(|(_, c)| c.host == self.host) {
                if pass == Pass::Deliver && self.callback_pending(handle, callback.chat)? {
                    reached &= self.dispatch_agent(handle)?;
                }
            }
            return Ok((board, reached));
        }
        let initial_sequence = sequence;
        let mut observed = true;
        let mut pull_at = f64::INFINITY;
        for (handle, callback) in callbacks.iter().filter(|(_, c)| c.host == self.host) {
            let env = crate::cli::Env::of_process();
            let visit = Visit::new(pass, &env, self, holdings.as_ref());
            let (handled, due) =
                self.dispatch_one(handle, callback, &mut board, &mut sequence, &visit, None)?;
            observed &= handled;
            pull_at = pull_at.min(due);
        }
        *self.callback_idle.lock().unwrap_or_else(|e| e.into_inner()) =
            (observed && initial_sequence == sequence).then_some(Idle {
                marker: sequence,
                health,
                pull_at,
                holdings,
            });
        Ok((board, observed))
    }

    /// Direct messages need only the native outbox, never a machine/pane probe or a board digest.
    /// Whether it reached the recipient: false while another dispatcher holds its lease.
    fn dispatch_agent(&self, handle: &str) -> Result<bool, Error> {
        match self
            .callbacks()?
            .get(handle)
            .filter(|c| c.host == self.host)
        {
            Some(callback) => self.with_callback_lease(handle, false, |revision| {
                self.deliver_pending(handle, callback, revision, &|text| deliver(callback, text))
            }),
            None => Ok(true),
        }
    }

    /// `send` under `handle`'s lease, when no other dispatcher holds it, handed the lease's
    /// revision: whether it ran and kept the lease to its end (`send` says whether it did). Only a
    /// registered channel is dispatched to, but for a collecting pass (`collect`), whose reader has
    /// none.
    fn with_callback_lease(
        &self,
        handle: &str,
        collect: bool,
        send: impl FnOnce(u64) -> Result<bool, Error>,
    ) -> Result<bool, Error> {
        // The same per-recipient lease serializes board callbacks and direct messages. What it
        // allows is timed by the delivery, which renews it ([`Store::deliver_pending`]): never
        // from here, before a digest that may take longer than the lease to build.
        let acquired: Option<u64> = self.retrying(|| {
                self.transact(
                    "LET $s = (SELECT address, lease, revision FROM ONLY type::record('callback_state', $handle));
                 LET $lease = (SELECT * FROM ONLY type::record('callback_lease', $handle));
                 IF ($s.address = NONE AND !$collect) OR type::record('session', $handle).left_at != NONE { RETURN NONE; };
                 IF math::max([$s.lease ?? 0, $lease.until ?? 0]) > $now { RETURN NONE; };
                 LET $revision = math::max([$lease.revision ?? 0, $s.revision ?? 0]) + 1;
                 UPSERT type::record('callback_lease', $handle) SET until = $now + $lease_seconds, revision = $revision RETURN NONE;
                 RETURN $revision;",
                    json!({"handle":handle, "now":self.now(), "lease_seconds":CALLBACK_LEASE_SECONDS, "collect":collect}),
                )
        })?;
        let Some(revision) = acquired else {
            return Ok(false);
        };
        let delivered = send(revision);
        let released = self.retrying(|| self.transact::<Value>(
            "UPDATE type::record('callback_lease', $handle) SET until = 0 WHERE revision = $revision RETURN NONE; RETURN NONE;",
            json!({"handle":handle, "revision":revision}),
        ));
        let held = delivered?;
        released?;
        Ok(held)
    }

    /// One recipient's visit: whether it was reached, and when its PULL is next due. `lease` is the
    /// revision of the recipient's lease this dispatcher holds, once it holds it.
    fn dispatch_one(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        lease: Option<u64>,
    ) -> Result<(bool, f64), Error> {
        // A write's observation keeps the board it read once its write committed: what the write
        // caused is read against the board as the write left it. A delivering pass reads the board
        // again under the lease, so a delayed plan cannot publish a transient from an older board.
        if lease.is_some() && visit.pass != Pass::Observe {
            (*board, *sequence) = self.callback_board()?;
        }
        let collect = visit.pass == Pass::Collect;
        let Stored {
            state,
            checked_pull,
            queued,
        } = self.retrying(|| {
            self.read(
                "RETURN { state: (SELECT * OMIT id FROM ONLY type::record('callback_state', $handle)), checked_pull: type::record('callback_lease', $handle).pull ?? 0,
                    queued: (SELECT key, text, sequence, pull_entries, pull_withheld, pull_said FROM callback_event WHERE handle = $handle AND delivered != true
                        AND (string::starts_with(key, 'pull-') OR string::starts_with(key, 'event-') OR (string::starts_with(key, 'pane-') AND string::starts_with(text, 'BLOCKED ')))) };",
                json!({"handle":handle}),
            )
        })?;
        let Some(mut state) = state else {
            return Ok((true, f64::INFINITY));
        };
        state.restore_unknown();
        let unsequenced = state.seen.is_some() && state.snapshot_sequence == 0;
        // An observation recorded at a marker at or past this board's read a board holding this
        // write's commit (each write a board read sees moves the marker, and this board was read
        // after it), so what the write caused was heard there: a write's observation leaves that
        // recipient. Reading the board again instead would cost each of many writers at once the
        // whole board for each recipient another had observed meanwhile.
        let count_changed = callback.session.kind == "orchestrator"
            && visit.holdings.is_some_and(|holdings| {
                state.heard().and_then(|before| before.open_slots())
                    != Some(holdings.get(handle).map_or(0, BTreeSet::len))
            });
        if visit.pass == Pass::Observe
            && !unsequenced
            && state.snapshot_sequence >= *sequence
            && !count_changed
        {
            return Ok((true, f64::INFINITY));
        }
        // Planning can meet an observation newer than its shared snapshot. Recheck the marker
        // before diffing it, including unsequenced legacy observations. A lease also rechecks the
        // marker above, so a delayed plan cannot publish a transient from an older board.
        if state.snapshot_sequence > *sequence || unsequenced {
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
        if orchestrator {
            seen = seen.with_holdings(
                visit.holdings.ok_or_else(|| {
                    Error::Db("orchestrator PULL has no holdings observation".into())
                })?,
                handle,
            );
        }
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
        // A PULL that falls due with time, or one still queued, which is built again before it is
        // said, waits for the pass that delivers: an observation leaves both as it found them, so
        // that pass still finds them due. What the observation would record as heard is not left.
        let delivering = visit.pass != Pass::Observe;
        let scheduled = orchestrator
            && (initializing
                || (state.pull_entries.is_none() && state.pull_queued.is_none())
                || (delivering && self.now() - last_pull >= 600.0)
                || hears(&statuses) != hears(&state.statuses)
                || (delivering && queued.iter().any(Queued::is_pull)));
        let changed = orchestrator && attach::pull_changed(before.as_ref(), &compact(&seen));
        let quiet = (seen.children.values())
            .chain(legacy.children.values())
            .any(|health| health.quiet());
        let env = crate::cli::Env::of_process();
        let view = if scheduled || changed || quiet {
            Some(crate::cli::agents::describe_board(
                self,
                visit.probes(),
                self.now(),
                false,
                board,
            )?)
        } else {
            None
        };
        let (idle, busy) = pull::idle_and_busy(view.as_ref(), &seen.children);
        // One running a test is present however quiet the board says it is.
        for agent in &busy {
            for look in [&mut seen, &mut legacy] {
                if let Some(health) = look.children.get_mut(agent) {
                    *health = crate::Health::Live;
                }
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
                .with_callback_lease(handle, collect, |revision| {
                    (self.dispatch_one(handle, callback, board, sequence, visit, Some(revision)))
                        .map(|(handled, _)| handled)
                })
                .map(|handled| (handled, pull_at));
        }
        let mut lines = Vec::new();
        if let Some(before) = &before
            && !legacy_upgrade
            && before != &compact(&seen)
        {
            attach::diff_on(self, board, before, &seen, handle, &mut |line| {
                lines.push(event_line(line))
            })?;
        }
        // The recorded entries are the whole digest's, as older dispatchers record them; what
        // the recipient was told of them is in the PULLs it acknowledged ([`Account`]).
        let mut account: Option<Account> = None;
        let (mut pull_text, mut pull_current) = (String::new(), BTreeSet::new());
        let mut pull_said: Option<pull::Said> = None;
        if due {
            let root = probes::repo_root(&env.cwd);
            let root = root.as_deref();
            let landed = |task: &crate::Task| root.is_some_and(|r| probes::landed(r, &task.slug));
            let tips = probes::tips(root);
            let holdings = visit
                .holdings
                .ok_or_else(|| Error::Db("orchestrator PULL has no holdings observation".into()))?;
            let observed = pull::Observation { board, holdings };
            let digest = pull::digest_on(
                self,
                observed,
                &idle,
                Some(&landed),
                Some(&tips),
                self.now(),
                &busy,
            )?;
            let entries = digest.entries();
            let digest = digest.only_for(board, handle);
            let heard = digest.entries();
            // Once it acknowledged a PULL this generation queued, it hears what changed since.
            let told = super::pull_account::told(&state, &entries);
            let baseline = super::pull_account::baseline(&state, &heard);
            let lines = match (baseline, &told) {
                (Some(said), Some(told)) if !legacy_upgrade => digest.changes_since(said, told),
                // The whole digest; after a new channel reset the account, the posts it was told
                // stay told.
                _ => digest.event_lines_since(
                    &(state.acknowledged()).unwrap_or_else(|| state.pull_unanswered.clone()),
                ),
            };
            pull_text = (lines.iter().map(|l| event_line(l)))
                .collect::<Vec<_>>()
                .join("\n");
            pull_said = Some(digest.said_after(baseline));
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
        let revision = lease.unwrap_or(state.revision);
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
                pull_said: None,
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
                pull_said: pull_said.clone(),
            });
        }
        let legacy = compact(&legacy);
        let narrowing = attach::Narrowing::between(&legacy, &compact(&seen));
        let unchanged =
            state.seen.as_ref() == Some(&legacy) && state.narrowing.as_ref() == Some(&narrowing);
        // Message, role and task-tree writes append a board message. Once the inbox is durably
        // copied, an unchanged observation needs only outbox retries, not another history scan.
        // A plan that would read it goes on to the lease below whatever it finds (the cursor or the
        // observation has moved), and the leased pass reads it again: the plan leaves it to that one.
        let inbox =
            if lease.is_none() || (state.inbox_cursor == Some(board.latest_message) && unchanged) {
                Vec::new()
            } else if first_observation {
                self.inbox(handle)?
            } else {
                self.inbox_pooled(handle, message_floor, board.latest_message)?
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
                    crate::cli::text::event_kind(&message.kind),
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
                pull_said: None,
            });
        }
        if callback.chat {
            #[derive(Deserialize)]
            struct Chat {
                key: String,
                text: String,
            }
            let chat: Vec<Chat> = self.retrying(|| {
                self.read(
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
                    pull_said: None,
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
        let settled = events.is_empty()
            && !initializing
            && state.registration_revision == state.observed_registration
            && state.statuses == statuses
            && state.unknown_children == unknown_children
            && (account.as_ref())
                .is_none_or(|a| a.changes_nothing(&state, &queued, &pull_text, pull_said.as_ref()))
            && rescoped.is_empty();
        let current = unchanged && state.inbox_cursor == Some(board.latest_message);
        // A write's observation leaves a recipient whose look moved by the newest message alone, and
        // whose inbox no message since could reach (the pass's pool says so, with no read of its
        // own), to the delivering pass: the write gave it nothing to hear. Its recorded look and
        // cursors stay where they were, as when another dispatcher held its lease, and the delivering
        // pass moves them. Taking every recipient's lease to read an empty inbox and move its cursor
        // was most of what one write cost.
        let unreached = !current
            && settled
            && visit.pass == Pass::Observe
            && lease.is_none()
            && state.narrowing.as_ref() == Some(&narrowing)
            && state.seen.as_ref().is_some_and(|recorded| {
                let mut recorded = recorded.clone();
                recorded.latest_message = legacy.latest_message;
                recorded == legacy
            })
            && !self.could_hear(handle, message_floor, board.latest_message)?;
        if settled && (current || unreached) {
            if due {
                // Fresh command Stores share this deadline even when the digest has no new
                // entry. Its scheduling checkpoint belongs to the small lease, not Seen.
                let revision = lease.expect("a timed checkpoint holds a lease");
                self.retrying(|| self.transact::<Value>(
                    "UPDATE type::record('callback_lease', $handle) SET pull = $now WHERE revision = $revision RETURN NONE; RETURN NONE;",
                    json!({"handle":handle, "revision":revision, "now":self.now()}),
                ))?;
            }
            if visit.pass == Pass::Observe || !self.callback_pending(handle, callback.chat)? {
                return Ok((true, pull_at));
            }
            if let Some(revision) = lease {
                return self
                    .deliver_pending(handle, callback, revision, &|text| {
                        visit.send(callback, text)
                    })
                    .map(|held| (held, pull_at));
            }
        }
        if lease.is_none() {
            return self
                .with_callback_lease(handle, collect, |revision| {
                    (self.dispatch_one(handle, callback, board, sequence, visit, Some(revision)))
                        .map(|(handled, _)| handled)
                })
                .map(|handled| (handled, pull_at));
        }
        // An inbox acknowledgement takes no lease: one of a PULL read as waiting may have come in
        // since, and what the recipient was told with it. A PULL still waits only while its event
        // is there undelivered: a presence sweep, which takes no lease either, prunes delivered
        // ones, and no key is used twice. Otherwise nothing is written, and the dispatch reads and
        // decides again. Nor is anything written once the lease lapsed and another dispatcher took
        // it (its revision moved) while this one built: the recipient is that one's.
        let waiting: Vec<&str> = (queued.iter().filter(|line| line.is_pull()))
            .map(|line| line.key.as_str())
            .collect();
        let accounted = self.retrying(|| self.transact::<Accounted>(
            &format!("LET $marker = {board_marker};
             LET $held = type::record('callback_lease', $handle).revision = $revision;
             LET $current = array::len($waiting.filter(|$key| record::exists(type::record('callback_event', [$handle, $key]))
                AND type::record('callback_event', [$handle, $key]).delivered != true)) = array::len($waiting);
             IF $held AND $current {{
             IF $pull_kept != NONE {{
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') AND key != $pull_kept RETURN NONE;
                UPDATE type::record('callback_event', [$handle, $pull_kept]) SET text = $pull_text, pull_entries = $pull_awaiting.entries,
                    pull_withheld = $pull_awaiting.withheld, pull_said = $pull_said WHERE delivered != true RETURN NONE;
             }};
             IF $replace_pull {{ DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') RETURN NONE; }};
             FOR $key IN $rescoped.dropped {{ DELETE type::record('callback_event', [$handle, $key]) WHERE delivered != true RETURN NONE; }};
             FOR $line IN $rescoped.rewritten {{ UPDATE type::record('callback_event', [$handle, $line[0]]) SET text = $line[1] WHERE delivered != true RETURN NONE; }};
             FOR $new IN $events.filter(|$event| string::starts_with($event.key, 'event-') AND string::starts_with($event.text, 'NEW ')) {{
                LET $prefix = string::concat('NEW ', string::split($new.text, ' ')[1], ' ready — ');
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'event-') AND string::starts_with(text, $prefix) RETURN NONE;
             }};
             FOR $event IN $events {{
                IF !record::exists(type::record('callback_event', [$handle, $event.key])) {{
                    LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                    CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                    UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                }};
             }};
             IF $computed {{ UPDATE type::record('callback_lease', $handle) SET pull = $pull WHERE revision = $revision RETURN NONE; }};
             LET $before = (SELECT seen, narrowing, unknown_children, statuses, pull_queued, inbox_cursor, message_cursor, delivery_version, registration_revision, observed_registration FROM ONLY type::record('callback_state', $handle));
             IF $before.seen != $seen OR $before.narrowing != $narrowing OR $before.unknown_children != $unknown_children OR $before.statuses != $statuses
                OR $before.pull_queued != $pull_queued OR $before.inbox_cursor != $inbox_cursor
                OR ($before.message_cursor ?? 0) < $message_cursor OR $before.delivery_version = NONE
                OR ($before.registration_revision ?? 0) != ($before.observed_registration ?? 0) OR $computed {{
             UPDATE type::record('callback_state', $handle) SET seen = $seen, narrowing = $narrowing, unknown_children = $unknown_children, snapshot_sequence = $snapshot_sequence, statuses = $statuses, revision = $revision, pull = $pull, pull_queued = $pull_queued,
                observed_registration = registration_revision ?? 0,
                pull_entries = IF $legacy_upgrade {{ $pull_account.recorded ?? pull_entries ?? [] }}
                    ELSE IF $computed AND pull_entries != NONE {{
                        array::union(pull_entries.filter(|$entry| $entry IN $pull_current AND $entry NOT IN $pull_account.untold), $pull_account.unheard)
                    }} ELSE {{ pull_entries }},
                pull_withheld = IF $computed {{ $pull_account.withheld }} ELSE {{ pull_withheld }},
                pull_awaiting = IF $computed {{ $pull_awaiting }} ELSE {{ pull_awaiting }},
                message_cursor = math::max([message_cursor ?? 0, $message_cursor]),
                inbox_cursor = $inbox_cursor,
                delivery_version = delivery_version ?? 1 RETURN NONE;
             }};
             }};
             RETURN {{ held: $held, current: $current, marker: {{ before: $marker, after: {board_marker} }} }};", board_marker = crate::schema::BOARD_MARKER),
            json!({"handle":handle, "waiting":waiting, "events":events, "seen":stored_seen, "narrowing":narrowing, "unknown_children":unknown_children, "snapshot_sequence":sequence, "statuses":statuses, "revision":revision,
                "pull":if due {self.now()} else {state.pull}, "legacy_upgrade":legacy_upgrade,
                "pull_queued":account.as_ref().map_or(&state.pull_queued, |a| &a.queued),
                "pull_awaiting":account.as_ref().and_then(|a| a.awaiting.as_ref()),
                "pull_account":account.as_ref().map(|a| json!({"recorded":a.recorded, "unheard":a.unheard, "untold":a.untold, "withheld":a.withheld})),
                "message_cursor":board.latest_message, "inbox_cursor":board.latest_message,
                "computed":due, "pull_current":pull_current, "rescoped":rescoped,
                "pull_kept":pull_kept, "pull_text":pull_text, "pull_said":pull_said, "replace_pull":pull_replaced}),
        ))?;
        if *sequence == accounted.marker.before {
            *sequence = accounted.marker.after;
        }
        self.adopt(&accounted.marker);
        if !accounted.held {
            return Ok((false, pull_at));
        }
        if !accounted.current {
            return self.dispatch_one(handle, callback, board, sequence, visit, lease);
        }
        if delivering {
            let revision = lease.expect("writes hold a lease");
            let held = self.deliver_pending(handle, callback, revision, &|text| {
                visit.send(callback, text)
            })?;
            return Ok((held, pull_at));
        }
        Ok((true, pull_at))
    }

    /// The board marker as one of this dispatcher's own writes moved it. When the board stood at
    /// `before` (no other write came between this dispatcher's read of the board and its write), the
    /// snapshot read there still holds at `after`: a dispatcher writes observations, outbox lines
    /// and deliveries, none of which a snapshot reads. The next recipient then reads the marker
    /// alone, not the whole board again. The cached snapshot is plain data, whole or none, so a
    /// panic that poisoned its lock left nothing half-made in it.
    fn adopt(&self, moved: &Moved) {
        let mut cached = self
            .callback_snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(snapshot) = cached.as_mut()
            && snapshot.callback_sequence == moved.before
        {
            snapshot.callback_sequence = moved.after;
        }
    }

    fn callback_pending(&self, handle: &str, chat: bool) -> Result<bool, Error> {
        self.retrying(|| self.read(
            "RETURN array::len(SELECT key FROM callback_event WHERE handle = $handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) LIMIT 1) > 0;",
            json!({"handle":handle, "chat":chat, "now":self.now()}),
        ))
    }

    /// Send what waits for `handle` by `send`, in batches, and record each batch's outcome, under
    /// the lease of `revision`: whether this dispatcher still held it. It renews the lease first,
    /// in the read of what is due, and begins no batch [`DELIVERY_SECONDS`] after, by the board's
    /// clock, which the lease's expiry is read by: what was built under the lease is delivered
    /// however long it took to build. A lease that lapsed and another dispatcher took meanwhile
    /// (its revision moved) is that one's, and nothing is sent: two never send the same line.
    fn deliver_pending(
        &self,
        handle: &str,
        callback: &Callback,
        revision: u64,
        send: &dyn Fn(&str) -> Result<(), DeliveryError>,
    ) -> Result<bool, Error> {
        let now = self.now();
        let pending: Option<Vec<Event>> = self.retrying(|| self.transact(
            "IF type::record('callback_lease', $handle).revision != $revision { RETURN NONE; };
             UPDATE type::record('callback_lease', $handle) SET until = $now + $lease_seconds RETURN NONE;
             UPDATE callback_event SET delivered = true WHERE handle = $handle AND delivered != true AND message != NONE AND record::exists(type::record('delivery', [message, $handle])) RETURN NONE;
             DELETE callback_event WHERE handle = $handle AND delivered = true RETURN NONE;
             RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;",
            json!({"handle":handle, "chat":callback.chat, "now":now, "revision":revision, "lease_seconds":CALLBACK_LEASE_SECONDS}),
        ))?;
        let Some(pending) = pending else {
            return Ok(false);
        };
        deliver_batches(
            &pending,
            now + DELIVERY_SECONDS,
            &|| self.now(),
            &mut |batch| {
                let text = batch
                    .iter()
                    .map(|event| {
                        attach::PaneWatch::delivery_text(
                            if event.message.is_none() {
                                &event.key
                            } else {
                                ""
                            },
                            &event.text,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                send(&text)
            },
            &mut |batch, result| self.record_callback_delivery(handle, callback, batch, result),
        )?;
        Ok(true)
    }

    fn record_callback_delivery(
        &self,
        handle: &str,
        callback: &Callback,
        pending: &[Event],
        result: Result<(), DeliveryError>,
    ) -> Result<(), Error> {
        let command = self.board_command();
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
        let moved = self.retrying(|| self.transact::<Moved>(
            &format!("LET $marker = {board_marker};
             FOR $key IN $keys {{
                UPDATE type::record('callback_event', [$handle, $key]) SET attempts += 1, delivered = $success, retry_at = $now + 5,
                    capacity_limited = $capacity OR (capacity_limited ?? false) RETURN NONE;
             }};
             IF $success {{
                IF $pull != NONE {{ UPDATE type::record('callback_state', $handle) SET pull_unanswered = array::union(pull_unanswered ?? [], $pull.pull_entries.filter(|$entry| $entry[0] = 'unanswered' AND $entry NOT IN ($pull.pull_withheld ?? []))), pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_said = $pull.pull_said, pull_awaiting = NONE, pull_queued = NONE RETURN NONE; }};
                FOR $message IN $messages {{ UPSERT type::record('delivery', [$message, $handle]) SET message = $message, handle = $handle, acked = $now RETURN NONE; }};
                FOR $key IN $chat {{ UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; }};
                UPDATE callback_event SET text = '' WHERE handle = $handle AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE;
             }}; RETURN {{ before: $marker, after: {board_marker} }};", board_marker = crate::schema::BOARD_MARKER),
            json!({"handle":handle, "keys":keys, "messages":messages, "chat":chat_keys, "success":success, "now":self.now(),
                "capacity":capacity, "pull":pull}),
        ))?;
        self.adopt(&moved);
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
                        self.run_transaction::<Value>(&body, json!({
                            "slug":slug, "now":self.now(), "text":format!("CALLBACK {handle} failed after 3 attempts: {why}; retained in inbox ({command} inbox --as {handle})"),
                        }), super::transaction::Run::Guarded { dispatch: false })?;
                        Ok(())
                    })?;
                }
            }
            eprintln!("CALLBACK {handle}: {why}; retained for inbox/retry");
        }
        Ok(())
    }
}

/// Capacity refusals send no bytes, so only they may split a batch during one attempt. No batch
/// begins at or past `deadline` by `now`, the board's clock.
fn deliver_batches<T>(
    pending: &[T],
    deadline: f64,
    now: &dyn Fn() -> f64,
    send: &mut impl FnMut(&[T]) -> Result<(), DeliveryError>,
    record: &mut impl FnMut(&[T], Result<(), DeliveryError>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut start = 0;
    let mut end = pending.len();
    // A large queue may need many connections. Yield well before the recipient lease expires;
    // events not yet attempted remain due for the next dispatcher, with their budgets intact.
    while start < pending.len() {
        if now() >= deadline {
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

/// `text` queued on Codex thread `thread` by `codex queue`, given 5 s (`codex_queue::patience`).
/// Started without a fork: a connection the Symphony server accepted and has not yet marked
/// close-on-exec stays out of it, so that client's read to the end of its answer never waits for
/// the queue.
fn queue(thread: &str, text: &str) -> Result<(), DeliveryError> {
    let patience = crate::codex_queue::patience(&crate::cli::Env::of_process());
    crate::codex_queue::queue(crate::sys::command("codex"), thread, text, patience)
        .map_err(DeliveryError::retry)
}

/// `queue` with its deadline handed in: `due` is asked, with the time since the spawn, whether a
/// queue still running is ended there. A test's deadline can be an event of its stand-in's rather
/// than a time a loaded machine may not even start it in.
#[cfg(test)]
fn queue_until(
    thread: &str,
    text: &str,
    due: impl Fn(Duration) -> bool,
) -> Result<(), DeliveryError> {
    crate::codex_queue::queue_until(crate::sys::command("codex"), thread, text, due)
        .map_err(DeliveryError::retry)
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
mod tests;

#[cfg(test)]
mod queue_tests;
