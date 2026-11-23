//! Native session channels and a durable outbox. Every write observes the board for each recipient
//! before it answers, so what a recipient hears is read against the board as that write left it.
//! A library store's write delivers too; a command line's leaves the delivery to the detached
//! dispatcher it starts, one a host at a time ([`Store::dispatching_by`],
//! [`Store::dispatch_detached`]), so the command answers without waiting on any recipient. The
//! Symphony server's clock drains retries and observes actionable PULL/STALE/LOST changes. No agent
//! keeps a monitor alive.
//!
//! A pass sends the board as many requests for many recipients as for one, but for those it visits
//! under their lease: it reads every recipient's stored state with whether a line of its outbox is
//! due, and the pane lines queued for each, in one read apiece ([`Store::stored_states`],
//! [`Store::pending`]), and only a visit under its lease reads its recipient's again. A board
//! keeps every registered channel's recipient whatever its session's health, dozens of them, and
//! a pass a second.

use super::callback_peer::{self as peer, Owner};
use super::callback_state::{State, stored_seen};
use super::launches::held;
use super::postgres::callbacks as pg;
use super::pull_account::{Account, PullPlan};
use super::queued_lines::{Hearing, Queued, Rescoped};
use super::statement::both;
use super::surreal::callbacks as sql;
use super::{Error, Store, refused};
use crate::cli::agents::{Answer, Pane};
use crate::cli::{attach, probes, pull};
use crate::lock::Recovered as _;
use crate::maps::Collect as _;
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
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture scripts every crate's tests share start their children as a test likes, wait by no timer, and recover machine_copy.rs's and script_tests.rs's poisoned locks by std's PoisonError::into_inner, as a crate without the board's lock::Recovered must"
)]
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

impl Event {
    /// A line for `handle` under `key`, about the task `slug` or the post `message` if it names
    /// one: never attempted, and no PULL.
    fn line(
        key: String,
        handle: &str,
        text: String,
        slug: Option<String>,
        message: Option<i64>,
    ) -> Event {
        Event {
            key,
            handle: handle.to_string(),
            text,
            slug,
            message,
            attempts: 0,
            sequence: 0,
            pull_entries: None,
            pull_withheld: None,
            pull_said: None,
        }
    }
}

/// A recipient's callback state as a dispatch reads it, with when its PULL was last checked, the
/// lines still queued for it, which a hand-over can take back ([`Hearing::rescope`]), and whether a
/// line of its outbox was due when it was read. A pass's plans read every recipient's at once
/// ([`Store::stored_states`]); a visit under its lease reads its own without the due line, false
/// there, as it asks its outbox afresh ([`Store::stored_one`]).
#[derive(Deserialize)]
struct Stored {
    handle: String,
    state: Option<State>,
    checked_pull: f64,
    queued: Vec<Queued>,
    #[serde(default)]
    pending: bool,
}

/// A recipient's observation as written: whether the dispatcher still held the recipient's lease,
/// whether the PULLs it read as waiting still were, and how the write moved the board marker.
#[derive(Deserialize)]
struct Accounted {
    held: bool,
    current: bool,
    marker: Moved,
}

/// The agents view a visit took, when its recipient's look needed one, with the sessions it lists
/// idle and those running a test ([`pull::idle_and_busy`]).
struct Agents {
    view: Option<crate::cli::agents::View>,
    idle: Vec<String>,
    busy: Vec<String>,
}

/// What a PULL due for its recipient planned ([`Store::plan_pull`]): its text and what it says,
/// the account of what the recipient was told of it, and every entry of the whole digest, which
/// the observation records as older dispatchers do.
struct Pulled {
    text: String,
    said: pull::Said,
    account: Account,
    entries: BTreeSet<(String, String)>,
}

impl Pulled {
    /// The event of this PULL for `handle`, keyed by the lease `revision` it is queued under, when
    /// its account queues a new one ([`PullPlan::Queued`]).
    fn event(&self, handle: &str, board: &super::Board, revision: u64) -> Option<Event> {
        let Account {
            plan: PullPlan::Queued,
            awaiting: Some(awaiting),
            ..
        } = &self.account
        else {
            return None;
        };
        let (key, slug) = (format!("pull-{revision}"), session_task(board, handle));
        Some(Event {
            pull_entries: Some(awaiting.entries.clone()),
            pull_withheld: Some(awaiting.withheld.clone()),
            pull_said: Some(self.said.clone()),
            ..Event::line(key, handle, self.text.clone(), slug, None)
        })
    }
}

/// A recipient as one visit reads it against the board the visit holds ([`Store::look`]): its
/// stored state and the lines still queued for it, what it hears now, and whether its PULL is due.
struct Look {
    state: State,
    queued: Vec<Queued>,
    /// What it hears now, and the same compacted to the open tasks, as its record keeps it.
    seen: attach::Seen,
    heard: attach::Seen,
    /// The whole board's look older dispatchers keep, compacted to the open tasks.
    legacy: attach::Seen,
    /// What it was told, as it hears it ([`State::heard`]).
    before: Option<attach::Seen>,
    /// How what it hears narrows the whole board's look ([`attach::Narrowing`]).
    narrowing: attach::Narrowing,
    /// Whether no newer dispatcher has written for it yet; of those, whether an older one has
    /// ([`upgrades_legacy`]) or nobody has.
    initializing: bool,
    legacy_upgrade: bool,
    first_observation: bool,
    /// The message its inbox is read after.
    message_floor: i64,
    /// The open tasks' statuses an orchestrator's PULL is scheduled by; none for anyone else.
    statuses: BTreeMap<String, String>,
    rescoped: Rescoped,
    agents: Agents,
    /// Whether its PULL is due now, and when it is next due.
    due: bool,
    pull_at: f64,
}

impl Look {
    /// Whether the look its state records is this one.
    fn unchanged(&self) -> bool {
        self.state.seen.as_ref() == Some(&self.legacy)
            && self.state.narrowing.as_ref() == Some(&self.narrowing)
    }

    /// Whether its record is current on `board`: its look unchanged, and its inbox read to the
    /// board's newest message.
    fn current(&self, board: &super::Board) -> bool {
        self.unchanged() && self.state.inbox_cursor == Some(board.latest_message)
    }

    /// Whether recording this look, with `events`, the children `unknown` whose liveness is not
    /// known and the PULL `pulled` planned, if one is due, changes nothing its state records.
    fn settled(
        &self,
        events: &[Event],
        unknown: &BTreeSet<String>,
        pulled: Option<&Pulled>,
    ) -> bool {
        let state = &self.state;
        events.is_empty()
            && !self.initializing
            && state.registration_revision == state.observed_registration
            && state.statuses == self.statuses
            && state.unknown_children == *unknown
            && pulled.is_none_or(|pulled| {
                let Pulled { text, said, .. } = pulled;
                (pulled.account).changes_nothing(state, &self.queued, text, Some(said))
            })
            && self.rescoped.is_empty()
    }
}

/// Where a leased pass reads its recipient's inbox from ([`Store::message_events`]): after `floor`,
/// where its cursor stands, or, on its `first` observation, whole, keeping what came after `floor`
/// and, of the rest, each message (`msg`) and each line addressed to it.
#[derive(Clone, Copy)]
struct Inbox {
    floor: i64,
    first: bool,
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

/// The sessions of `view` with no terminal, their listeners answering, that `heard` lists idle,
/// each with the beat it rests since, as text: its last tool call, else when it connected.
fn resting(
    view: Option<&crate::cli::agents::View>,
    board: &crate::Board,
    heard: &BTreeSet<(String, String)>,
) -> BTreeMap<String, String> {
    let agents = view.map_or(&[][..], |view| view.agents.as_slice());
    (agents.iter())
        .filter(|a| a.pane.is_none() && a.confirmed)
        .filter(|a| heard.contains(&("idle".to_string(), a.handle.clone())))
        .filter_map(|a| board.sessions.iter().find(|s| s.row.handle == a.handle))
        .map(|s| {
            let beat = s.row.last_tool.unwrap_or(s.row.connected);
            (s.row.handle.clone(), beat.to_string())
        })
        .collect()
}

impl Callback {
    /// Whether the idle sweep's last evidence still confirms this listener at `now`.
    pub(crate) fn confirmed(&self, now: f64) -> bool {
        self.confirmed_until >= now
    }
}

/// What hc says of the machine's Codex agents, for [`Store::callback_presence`]: the live
/// terminals running Codex, and the threads known to run in one of them (a recorded launch's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Presence {
    pub terminals: BTreeSet<String>,
    pub threads: BTreeSet<String>,
    /// The threads the board launched (a launch record, open or closed, names them) that no open
    /// launch record puts in a terminal hc lists live: every terminal the board ran each in is
    /// closed or gone.
    pub gone: BTreeSet<String>,
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
            both(sql::send_agent(), pg::send_agent()),
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
        self.retrying(|| {
            self.read_together(
                both(sql::AGENT_DELIVERED, pg::agent_delivered()),
                json!({"handle":handle, "key":key}),
            )
        })
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
        let body = sql::register();
        self.retrying(|| {
            self.transact::<Value>(
                both(&body, pg::register()),
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
        Ok(self.chat_seat()?.map(|(handle, _)| handle))
    }

    /// [`Store::chat_recipient`], with why it hears the owner's chat.
    pub fn chat_seat(&self) -> Result<Option<(String, ChatSeat)>, Error> {
        match seat(&self.cached_callbacks()?) {
            Some((handle, why)) => Ok(Some((handle.to_owned(), why))),
            None => Ok(self
                .waiting_orchestrator()?
                .map(|handle| (handle, ChatSeat::Waiting))),
        }
    }

    /// The orchestrator that holds the vacant chat seat by waiting (`tasks wait`): of the sessions
    /// that read by waiting and registered no channel since, the live orchestrators', the first by
    /// handle. A waiter that stops waiting holds it while it stays live, so what the owner says
    /// waits for its next read rather than for nobody.
    fn waiting_orchestrator(&self) -> Result<Option<String>, Error> {
        let waiting: Vec<crate::rows::SessionRow> = self.retrying(|| {
            self.read(
                both(sql::WAITING_ORCHESTRATORS, pg::waiting_orchestrators()),
                json!({}),
            )
        })?;
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
                both(sql::SESSION_KIND, pg::session_kind()),
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
        let chat: Vec<Chat> =
            self.retrying(|| self.read(both(sql::UNHEARD_CHAT, pg::unheard_chat()), json!({})))?;
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
            .collect_map())
    }

    fn stored_callbacks(&self) -> Result<Vec<Callback>, Error> {
        let read = || {
            super::statement::both(
                sql::REGISTRATIONS,
                super::postgres::callbacks::registrations(),
            )
        };
        self.retrying(|| self.read(read(), json!({})))
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
    /// ([`super::surreal::launches::HOLDS`]).
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
        if let Some(hold) = hold {
            held(&mut vars, &callback.handle, hold);
        }
        let body = sql::retire(hold.is_some());
        let retired = self.retrying(|| {
            self.transact::<bool>(both(&body, pg::retire(hold.is_some())), vars.clone())
        })?;
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
            .filter(|c| c.confirmed(self.now()))
            .map(|c| c.handle)
            .collect())
    }

    /// The idle sweep confirms native reachability rather than treating absent tool calls as an
    /// exit. A socket must accept a connection; a Codex thread must still occupy a terminal: one a
    /// recorded launch puts it in, else the terminal it registered from, which must still run
    /// Codex. A thread registered from no terminal (Codex's shell sees no `HC_TERMINAL_ID` when its
    /// environment policy keeps only the core variables) is retired once every terminal the board
    /// launched it in is closed or gone ([`Presence::gone`]);
    /// one whose open launch record names a live terminal that does not run Codex (yet) is kept
    /// unconfirmed, and one no launch records is neither confirmed nor retired: hc cannot say
    /// where it runs. A session's health never decides: a Codex waiting between turns calls no
    /// tool, but its terminal runs Codex. A dry run reports the same channels without changing
    /// their heartbeat. No payload or credentials are sent by the socket probe. Without `codex`
    /// (hc did not answer) no thread is confirmed or retired.
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
                let thread = callback
                    .address
                    .strip_prefix(crate::launch::CODEX)
                    .unwrap_or_default();
                if codex.threads.contains(thread) {
                    true
                } else if let Some(terminal) = &callback.terminal {
                    codex.terminals.contains(terminal)
                } else if codex.gone.contains(thread) {
                    false
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
        self.retrying(|| self.transact::<Value>(both(sql::PRUNE, pg::prune()), json!({})))
            .map(|_| ())
    }

    // A liveness probe recovers transport failures, not an event already found too large on its
    // own. Keep that event's attempt budget even if another message later blocks the listener.
    fn callback_seen(&self, callback: &Callback) -> Result<bool, Error> {
        self.retrying(|| self.transact(
            both(sql::SEEN, pg::seen()),
            json!({"handle":callback.handle, "address":callback.address, "owner":callback.owner, "now":self.now(),
                "until":self.now() + crate::launch::reap::REAP_SECONDS + crate::health::LIVE_SECONDS}),
        ))
    }

    fn callback_blocked(&self, callback: &Callback) -> Result<(), Error> {
        self.retrying(|| {
            self.transact::<Value>(
            both(sql::BLOCKED, pg::blocked()),
            json!({"handle":callback.handle, "address":callback.address, "owner":callback.owner}),
        )
        })
        .map(|_| ())
    }

    /// Failed or pending non-message events also remain readable through the ordinary inbox, and,
    /// while nobody holds the chat seat, an orchestrator's lists the owner's chat events nobody
    /// received ([`Store::unheard_owner_events`]).
    pub fn callback_inbox(&self, handle: &str) -> Result<Vec<(String, String)>, Error> {
        let chat = self.reads_chat(handle)?;
        let events: Vec<Event> = self.retrying(|| {
            self.read_together(
                both(sql::INBOX, pg::inbox()),
                json!({"handle":handle, "chat":chat}),
            )
        })?;
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
        self.retrying(|| {
            self.transact::<Value>(
                both(sql::ack(), pg::ack()),
                self.acting(handle, self.now(), json!({"keys":keys, "chats":chats})),
            )
        })
        .map(|_| ())
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
        let read =
            || self.read::<Reader>(both(sql::READER, pg::reader()), json!({"handle":handle}));
        let mut reader = self.retrying(read)?;
        let here = reader.session.as_ref().is_some_and(|s| s.left_at.is_none());
        let unaddressed = reader.state.as_ref().is_none_or(|s| s.address.is_none());
        if here && unaddressed && (reader.state.is_none() || !reader.waiting) {
            self.retrying(|| {
                self.transact::<Value>(both(sql::WAIT, pg::wait()), json!({"handle":handle}))
            })?;
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
        let (mut board, mut sequence) = self.fresh_board()?;
        // Each look gives what no orchestrator holds, when one that stays could take it (the
        // reader among them, on the waiting list now), and then reads the board the give left.
        if self.give_if_due(&board) {
            (board, sequence) = self.fresh_board()?;
        }
        // Then what the tasks a workflow holds need of the board itself, as a PULL's reader settles
        // it ([`Store::settle_held_if_due`]): a look is `tasks wait`'s, never `agent-send`'s.
        if self.settle_held_if_due(&board) {
            (board, sequence) = self.fresh_board()?;
        }
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
    /// ([`super::surreal::asks::OWNER_SAVE`]): once per key, so a save handed over again changes nothing. A
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
        self.retrying(|| {
            self.transact::<Value>(
                super::statement::both(
                    super::surreal::asks::OWNER_SAVE.as_str(),
                    super::postgres::asks::owner_save(),
                ),
                vars.clone(),
            )
        })?;
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

    /// This store, its delivering passes first giving the open tasks no orchestrator holds, when
    /// one that stays could take them ([`Store::give_if_due`]): a detached dispatcher's, which
    /// every command-line write starts while a channel is registered, so a board whose
    /// orchestrators listen on channels alone gives at the next write's dispatch.
    ///
    /// Its passes then settle what the tasks a custom workflow holds need of the board itself
    /// ([`Store::settle_held_if_due`]: a lost holder recovered, a dependant cancelled by its
    /// position's policy), so the PULL they push says what is true. That pass is a pointer set
    /// here, never a call in the pass: `agent-send` delivers what it sends through the same pass
    /// (on a store never built with this), and a call there would link the board's admission and
    /// its write into it, which nothing it does needs (tools/size-budget.toml). Named only here,
    /// it links only into the programs that build a giving store, `tasks dispatch`'s.
    pub fn giving(mut self) -> Store {
        self.giving = true;
        self.settling = Some(Store::settle_held_if_due);
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
        self.resume_task_blocks()?;
        let callbacks = self.callbacks()?;
        if callbacks.is_empty() {
            return Ok(true);
        }
        self.dispatch_board(&callbacks, pass, None)
            .map(|(_, reached)| reached)
    }

    /// The server clock reuses this sequenced snapshot for its read models after delivery. Only the
    /// clock watches panes: a write delivers what it caused, and a pane's prompt is no write. The
    /// watch looks first, so the one hc sample it takes serves the tick's PULL view and the
    /// clock's agents view after it ([`Store::clock_panes`]).
    pub fn dispatch_callbacks_snapshot(&self) -> Result<Option<super::Board>, Error> {
        self.resume_task_blocks()?;
        *self.clock_panes.lock().recover() = None;
        let callbacks = self.callbacks()?;
        if callbacks.is_empty() {
            return Ok(None);
        }
        let read = self.callback_board()?;
        // The board the watch looked at serves the dispatch too, once the watch wrote nothing.
        let watched = self.watch_panes(&read.0, &callbacks)?;
        self.dispatch_board(&callbacks, Pass::Deliver, (!watched).then_some(read))
            .map(|(board, _)| Some(board))
    }

    /// The hc terminals this tick of the server clock sampled, when its pane watch took a sample.
    pub(crate) fn clock_panes(&self) -> Option<Answer<Vec<Pane>>> {
        self.clock_panes.lock().recover().clone()
    }

    /// The board as the pass read it, and whether the pass reached every recipient on this host: one
    /// whose lease another dispatcher held is left to that one, and to the next pass. The pass reads
    /// the board, or takes `read`, a board and its marker the caller read just before and wrote
    /// nothing since.
    fn dispatch_board(
        &self,
        callbacks: &BTreeMap<String, Callback>,
        pass: Pass,
        read: Option<(super::Board, u64)>,
    ) -> Result<(super::Board, bool), Error> {
        // Each pass reads the messages afresh, once for every recipient ([`Store::fresh_board`]).
        let (mut board, mut sequence) = match read {
            Some(read) => {
                *self.inbox_pool.lock().recover() = None;
                read
            }
            None => self.fresh_board()?,
        };
        // A giving store's delivering pass (a write's detached dispatcher) gives what no
        // orchestrator holds first, so its recipients hear the give; an observing one keeps the
        // board as its write left it.
        if pass == Pass::Deliver && self.giving && self.give_if_due(&board) {
            (board, sequence) = self.fresh_board()?;
        }
        // Then what the tasks a workflow holds need of the board itself ([`Store::giving`]).
        if pass == Pass::Deliver
            && let Some(settle) = self.settling
            && settle(self, &board)
        {
            (board, sequence) = self.fresh_board()?;
        }
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
            .recover()
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
        let here: Vec<&Callback> = (callbacks.values())
            .filter(|c| c.host == self.host)
            .collect();
        if idle {
            let mut reached = true;
            if pass == Pass::Deliver {
                for handle in self.pending(&here)? {
                    reached &= self.dispatch_agent(&handle)?;
                }
            }
            return Ok((board, reached));
        }
        let initial_sequence = sequence;
        let mut observed = true;
        let mut pull_at = f64::INFINITY;
        // Every recipient's stored state in one read: each one's plan reads it from there.
        let mut stored = self.stored_states(&here)?;
        for callback in here {
            let env = crate::cli::Env::of_process();
            let visit = Visit::new(pass, &env, self, holdings.as_ref());
            let handle = callback.handle.as_str();
            let ahead = stored.remove(handle);
            let (handled, due) =
                self.dispatch_one(handle, callback, &mut board, &mut sequence, &visit, ahead)?;
            observed &= handled;
            pull_at = pull_at.min(due);
        }
        *self.callback_idle.lock().recover() = (observed && initial_sequence == sequence)
            .then_some(Idle {
                marker: sequence,
                health,
                pull_at,
                holdings,
            });
        Ok((board, observed))
    }

    /// The board read again, with the messages read afresh: the inbox pool a pass reads every
    /// recipient's inbox from ([`super::pool::Pool`]) is dropped first, so the next read of an
    /// inbox reads the messages as this board has them. The pool is plain data, whole or none, so
    /// a panic elsewhere leaves nothing half-made in it.
    fn fresh_board(&self) -> Result<(super::Board, u64), Error> {
        *self.inbox_pool.lock().recover() = None;
        self.callback_board()
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
                    both(sql::TAKE_LEASE, pg::take_lease()),
                    json!({"handle":handle, "now":self.now(), "lease_seconds":CALLBACK_LEASE_SECONDS, "collect":collect}),
                )
        })?;
        let Some(revision) = acquired else {
            return Ok(false);
        };
        let delivered = send(revision);
        let released = self.retrying(|| {
            self.transact::<Value>(
                both(sql::RELEASE_LEASE, pg::release_lease()),
                json!({"handle":handle, "revision":revision}),
            )
        });
        let held = delivered?;
        released?;
        Ok(held)
    }

    /// One recipient's visit, planned without its lease from its stored state, `ahead` when the pass
    /// read it with every other recipient's ([`Store::stored_states`]): whether it was reached, and
    /// when its PULL is next due. One whose PULL is due is made again under the lease at once
    /// ([`Store::leased`]), and one the plan finds anything to record, queue or deliver for, once it
    /// has planned.
    fn dispatch_one(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        ahead: Option<Stored>,
    ) -> Result<(bool, f64), Error> {
        let stored = match ahead {
            Some(stored) => stored,
            None => self.stored_one(callback, true)?,
        };
        let pending = stored.pending;
        let Some(look) = self.look(handle, callback, board, sequence, visit, stored)? else {
            return Ok((true, f64::INFINITY));
        };
        if look.due {
            return self.leased(handle, callback, board, sequence, visit, look.pull_at);
        }
        let lines = self.changed_lines(handle, board, &look)?;
        let mut events = line_events(handle, board, lines, look.state.revision);
        // Message, role and task-tree writes append a board message. A plan that would read the
        // inbox goes on to the lease below whatever it finds (the cursor or the observation has
        // moved), and the leased pass reads it again: the plan leaves it to that one.
        events.extend(self.message_events(handle, callback, board, None)?);
        let (_, unknown_children) = self.stored_look(board, &look.legacy);
        let settled = look.settled(&events, &unknown_children, None);
        let current = look.current(board);
        // A write's observation leaves a recipient whose look moved by the newest message alone, and
        // whose inbox no message since could reach (the pass's pool says so, with no read of its
        // own), to the delivering pass: the write gave it nothing to hear. Its recorded look and
        // cursors stay where they were, as when another dispatcher held its lease, and the delivering
        // pass moves them. Taking every recipient's lease to read an empty inbox and move its cursor
        // was most of what one write cost.
        let unreached = !current
            && settled
            && visit.pass == Pass::Observe
            && look.state.narrowing.as_ref() == Some(&look.narrowing)
            && look.state.seen.as_ref().is_some_and(|recorded| {
                let mut recorded = recorded.clone();
                recorded.latest_message = look.legacy.latest_message;
                recorded == look.legacy
            })
            && !self.could_hear(handle, look.message_floor, board.latest_message)?;
        if settled && (current || unreached) && (visit.pass == Pass::Observe || !pending) {
            return Ok((true, look.pull_at));
        }
        self.leased(handle, callback, board, sequence, visit, look.pull_at)
    }

    /// One recipient's visit under its lease, `lease` the revision of it this dispatcher holds:
    /// whether it was reached, and when its PULL is next due. It records the recipient's look with
    /// the events it queued, and a delivering pass delivers them.
    fn dispatch_leased(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        lease: u64,
    ) -> Result<(bool, f64), Error> {
        // A write's observation keeps the board it read once its write committed: what the write
        // caused is read against the board as the write left it. A delivering pass reads the board
        // again under the lease, so a delayed plan cannot publish a transient from an older board.
        let delivering = visit.pass != Pass::Observe;
        if delivering {
            (*board, *sequence) = self.callback_board()?;
        }
        // Under the lease the recipient's stored state is read again, alone, and its outbox asked
        // afresh below: the read takes no due line, nor the clock one needs.
        let stored = self.stored_one(callback, false)?;
        let Some(mut look) = self.look(handle, callback, board, sequence, visit, stored)? else {
            return Ok((true, f64::INFINITY));
        };
        let lines = self.changed_lines(handle, board, &look)?;
        // The recorded entries are the whole digest's, as older dispatchers record them; what
        // the recipient was told of them is in the PULLs it acknowledged ([`Account`]).
        let pulled = match look.due {
            true => Some(self.plan_pull(handle, board, visit, &mut look)?),
            false => None,
        };
        let mut events = line_events(handle, board, lines, lease);
        events.extend((pulled.as_ref()).and_then(|pulled| pulled.event(handle, board, lease)));
        // Message, role and task-tree writes append a board message. Once the inbox is durably
        // copied, an unchanged observation needs only outbox retries, not another history scan.
        let inbox = (!look.current(board)).then_some(Inbox {
            floor: look.message_floor,
            first: look.first_observation,
        });
        events.extend(self.message_events(handle, callback, board, inbox)?);
        let (stored_seen, unknown_children) = self.stored_look(board, &look.legacy);
        if look.settled(&events, &unknown_children, pulled.as_ref()) && look.current(board) {
            if look.due {
                // Fresh command Stores share this deadline even when the digest has no new
                // entry. Its scheduling checkpoint belongs to the small lease, not Seen.
                self.retrying(|| {
                    self.transact::<Value>(
                        both(sql::PULL_CHECKED, pg::pull_checked()),
                        json!({"handle":handle, "revision":lease, "now":self.now()}),
                    )
                })?;
            }
            if !delivering || !self.callback_pending(callback)? {
                return Ok((true, look.pull_at));
            }
            return self
                .deliver_pending(handle, callback, lease, &|text| visit.send(callback, text))
                .map(|held| (held, look.pull_at));
        }
        let account = pulled.as_ref().map(|pulled| &pulled.account);
        let plan = account.map(|account| &account.plan);
        let pull_kept = match plan {
            Some(PullPlan::Kept(key)) => Some(key),
            _ => None,
        };
        let pull_replaced = matches!(plan, Some(PullPlan::Dropped | PullPlan::Queued));
        let no_entries = BTreeSet::new();
        let pull_current = (pulled.as_ref()).map_or(&no_entries, |pulled| &pulled.entries);
        let pull_text = (pulled.as_ref()).map_or("", |pulled| pulled.text.as_str());
        let pull_said = pulled.as_ref().map(|pulled| &pulled.said);
        let Look { state, queued, .. } = &look;
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
            both(sql::account(), pg::account()),
            json!({"handle":handle, "waiting":waiting, "events":events, "seen":stored_seen, "narrowing":look.narrowing, "unknown_children":unknown_children, "snapshot_sequence":sequence, "statuses":look.statuses, "revision":lease,
                "pull":if look.due {self.now()} else {state.pull}, "legacy_upgrade":look.legacy_upgrade,
                "pull_queued":account.map_or(&state.pull_queued, |a| &a.queued),
                "pull_awaiting":account.and_then(|a| a.awaiting.as_ref()),
                "pull_account":account.map(|a| json!({"recorded":a.recorded, "unheard":a.unheard, "untold":a.untold, "withheld":a.withheld})),
                "message_cursor":board.latest_message, "inbox_cursor":board.latest_message,
                "computed":look.due, "pull_current":pull_current, "rescoped":look.rescoped,
                "pull_kept":pull_kept, "pull_text":pull_text, "pull_said":pull_said, "replace_pull":pull_replaced}),
        ))?;
        if *sequence == accounted.marker.before {
            *sequence = accounted.marker.after;
        }
        self.adopt(&accounted.marker);
        if !accounted.held {
            return Ok((false, look.pull_at));
        }
        if !accounted.current {
            return self.dispatch_leased(handle, callback, board, sequence, visit, lease);
        }
        if delivering {
            let held =
                self.deliver_pending(handle, callback, lease, &|text| visit.send(callback, text))?;
            return Ok((held, look.pull_at));
        }
        Ok((true, look.pull_at))
    }

    /// `handle` as a visit reads it against `board` from its `stored` state ([`Look`]): none when
    /// there is nothing to observe of it ([`Store::stored_state`]), or it is no session here.
    fn look(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        stored: Stored,
    ) -> Result<Option<Look>, Error> {
        let Some((mut state, checked_pull, queued)) =
            self.stored_state(handle, callback, board, sequence, visit, stored)?
        else {
            return Ok(None);
        };
        let Some(session) = board
            .sessions
            .iter()
            .find(|s| s.row.handle == handle && s.row.left_at.is_none())
        else {
            return Ok(None);
        };
        let orchestrator = session.row.kind == "orchestrator";
        // What the recipient hears, and the whole board's look older dispatchers keep.
        let mut seen = attach::seen(board, handle, &session.row.kind);
        if orchestrator {
            seen = seen.with_holdings(holdings(visit)?, handle);
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
        let legacy_upgrade = upgrades_legacy(&state);
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
        let agents = Agents { view, idle, busy };
        // One running a test is present however quiet the board says it is.
        for agent in &agents.busy {
            for look in [&mut seen, &mut legacy] {
                if let Some(health) = look.children.get_mut(agent) {
                    *health = crate::Health::Live;
                }
            }
        }
        // Compare the same effective child health that observation checkpoints persist.
        let heard = compact(&seen);
        let due = scheduled || (orchestrator && attach::pull_changed(before.as_ref(), &heard));
        let pull_at = if !orchestrator {
            f64::INFINITY
        } else if due {
            self.now() + 600.0
        } else {
            last_pull + 600.0
        };
        let legacy = compact(&legacy);
        let narrowing = attach::Narrowing::between(&legacy, &heard);
        Ok(Some(Look {
            state,
            queued,
            seen,
            heard,
            legacy,
            before,
            narrowing,
            initializing,
            legacy_upgrade,
            first_observation,
            message_floor,
            statuses,
            rescoped,
            agents,
            due,
            pull_at,
        }))
    }

    /// `handle`'s `stored` state, as a visit reads it against `board`: its callback state, when its
    /// PULL was last checked, and the lines still queued for it. None when there is nothing to
    /// observe: no state, or, for a write's observation, one recorded at or past this board. A
    /// state recorded past the board, or before observations were sequenced, reads the board again.
    fn stored_state(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        stored: Stored,
    ) -> Result<Option<(State, f64, Vec<Queued>)>, Error> {
        let Stored {
            state,
            checked_pull,
            queued,
            ..
        } = stored;
        let Some(mut state) = state else {
            return Ok(None);
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
            return Ok(None);
        }
        // Planning can meet an observation newer than its shared snapshot. Recheck the marker
        // before diffing it, including unsequenced legacy observations. A lease also rechecks the
        // marker above, so a delayed plan cannot publish a transient from an older board.
        if state.snapshot_sequence > *sequence || unsequenced {
            (*board, *sequence) = self.callback_board()?;
        }
        Ok(Some((state, checked_pull, queued)))
    }

    /// The lines `handle` hears of what changed since it was last told, as `look` reads it: none
    /// before its first look, nor for an observer upgraded from a legacy one, which is baselined.
    fn changed_lines(
        &self,
        handle: &str,
        board: &super::Board,
        look: &Look,
    ) -> Result<Vec<String>, Error> {
        let mut lines = Vec::new();
        if let Some(before) = &look.before
            && !look.legacy_upgrade
            && before != &look.heard
        {
            attach::diff_on(self, board, before, &look.seen, handle, &mut |line| {
                lines.push(event_line(line))
            })?;
        }
        Ok(lines)
    }

    /// `legacy`, the whole board's look, as it is stored for older dispatchers, each child's health
    /// as the board says it, with the children whose liveness is unknown ([`stored_seen`]).
    fn stored_look(
        &self,
        board: &super::Board,
        legacy: &attach::Seen,
    ) -> (attach::Seen, BTreeSet<String>) {
        stored_seen(legacy, |handle| {
            let Some(child) = board.sessions.iter().find(|s| s.row.handle == handle) else {
                return crate::Health::Left;
            };
            let held = board.tasks.iter().any(|task| {
                task.roles.iter().any(|role| {
                    role.handle == handle && crate::health::claimable(&role.role, &task.status)
                })
            });
            crate::health::health(&child.row, self.now(), &self.host, &*self.pid_alive, held)
        })
    }

    /// What `handle` was sent, as events: each message of its inbox after where `inbox` says it is
    /// read from (none when it says nothing), then, while it holds the chat seat, each of the
    /// owner's chat events its outbox holds no line of yet.
    fn message_events(
        &self,
        handle: &str,
        callback: &Callback,
        board: &super::Board,
        inbox: Option<Inbox>,
    ) -> Result<Vec<Event>, Error> {
        let mut events = Vec::new();
        if let Some(Inbox { floor, first }) = inbox {
            let latest = board.latest_message;
            let messages = match first {
                true => self.inbox(handle)?,
                // The pass's pool says no message since the cursor could reach it: its inbox holds
                // nothing after the cursor, and is not read ([`Store::could_hear`]).
                false if !self.could_hear(handle, floor, latest)? => Vec::new(),
                false => self.inbox_pooled(handle, floor, latest)?,
            };
            for message in messages.into_iter().filter(|message| {
                message.id > floor
                    || (first
                        && (message.kind == "msg" || message.recipient.as_deref() == Some(handle)))
            }) {
                let text = format!(
                    "{} #{} {} from {} — {}",
                    crate::cli::text::event_kind(&message.kind),
                    message.id,
                    message.slug,
                    message.sender,
                    event_line(&message.text)
                );
                let key = format!("message-{}", message.id);
                let slug = Some(message.slug);
                events.push(Event::line(key, handle, text, slug, Some(message.id)));
            }
        }
        if callback.chat {
            #[derive(Deserialize)]
            struct Chat {
                key: String,
                text: String,
            }
            let chat: Vec<Chat> = self.retrying(|| {
                self.read(
                    both(sql::UNQUEUED_CHAT, pg::unqueued_chat()),
                    json!({"handle":handle}),
                )
            })?;
            for event in chat {
                let key = format!("chat-{}", event.key);
                let slug = session_task(board, handle);
                events.push(Event::line(key, handle, event.text, slug, None));
            }
        }
        Ok(events)
    }

    /// The PULL due for `handle`, built again from `board` with the agents its `look` saw: what it
    /// says, and the account of what the recipient in the look's state was told of it, with the
    /// lines still queued for it ([`Account`]). An observer the look upgrades from a legacy one
    /// hears the whole digest, and has its account upgraded. Each session with no terminal it was
    /// told is idle is read back into the look's state ([`State::recall_idle`]).
    fn plan_pull(
        &self,
        handle: &str,
        board: &super::Board,
        visit: &Visit<'_>,
        look: &mut Look,
    ) -> Result<Pulled, Error> {
        let legacy_upgrade = look.legacy_upgrade;
        let (state, queued, agents) = (&mut look.state, &look.queued, &look.agents);
        let env = crate::cli::Env::of_process();
        let root = probes::repo_root(&env.cwd);
        let root = root.as_deref();
        let landed = |task: &crate::Task| root.is_some_and(|r| probes::landed(r, &task.slug));
        let tips = probes::tips(root);
        let observed = pull::Observation {
            board,
            holdings: holdings(visit)?,
        };
        let digest = pull::digest_on(
            self,
            observed,
            &agents.idle,
            Some(&landed),
            Some(&tips),
            self.now(),
            &agents.busy,
        )?;
        let entries = digest.entries();
        let digest = digest.only_for(board, handle);
        let heard = digest.entries();
        // A session with no terminal told idle stays told while it rests on the same beat,
        // whatever a dispatcher that does not list it recorded since.
        let mut idle_told = state.recall_idle(&resting(agents.view.as_ref(), board, &heard));
        idle_told.retain(|child, _| {
            (board.sessions.iter()).any(|s| s.row.handle == *child && s.row.left_at.is_none())
        });
        // Once it acknowledged a PULL this generation queued, it hears what changed since.
        let told = super::pull_account::told(state, &entries);
        let baseline = super::pull_account::baseline(state, &heard);
        let lines = match (baseline, &told) {
            (Some(said), Some(told)) if !legacy_upgrade => digest.changes_since(said, told),
            // The whole digest; after a new channel reset the account, the posts it was told
            // stay told.
            _ => digest.event_lines_since(
                &(state.acknowledged()).unwrap_or_else(|| state.pull_unanswered.clone()),
            ),
        };
        let text = (lines.iter().map(|l| event_line(l)))
            .collect::<Vec<_>>()
            .join("\n");
        let said = pull::Said {
            idle: idle_told,
            ..digest.said_after(baseline)
        };
        let account = match legacy_upgrade {
            true => Account::upgrade(state, &entries, &heard),
            false => Account::of(state, queued, &entries, &heard),
        };
        Ok(Pulled {
            text,
            said,
            account,
            entries,
        })
    }

    /// A plan's visit made again under the recipient's lease, once no other dispatcher holds it:
    /// whether it was reached, with `pull_at`, when its PULL is next due by the plan. A collecting
    /// pass takes a lease for a reader with no channel ([`Store::with_callback_lease`]).
    fn leased(
        &self,
        handle: &str,
        callback: &Callback,
        board: &mut super::Board,
        sequence: &mut u64,
        visit: &Visit<'_>,
        pull_at: f64,
    ) -> Result<(bool, f64), Error> {
        let collect = visit.pass == Pass::Collect;
        self.with_callback_lease(handle, collect, |revision| {
            (self.dispatch_leased(handle, callback, board, sequence, visit, revision))
                .map(|(handled, _)| handled)
        })
        .map(|handled| (handled, pull_at))
    }

    /// The board marker as one of this dispatcher's own writes moved it. When the board stood at
    /// `before` (no other write came between this dispatcher's read of the board and its write), the
    /// snapshot read there still holds at `after`: a dispatcher writes observations, outbox lines
    /// and deliveries, none of which a snapshot reads. The next recipient then reads the marker
    /// alone, not the whole board again. The cached snapshot is plain data, whole or none, so a
    /// panic that poisoned its lock left nothing half-made in it.
    fn adopt(&self, moved: &Moved) {
        let mut cached = self.callback_snapshot.lock().recover();
        if let Some(snapshot) = cached.as_mut()
            && snapshot.callback_sequence == moved.before
        {
            snapshot.callback_sequence = moved.after;
        }
    }

    /// Each of `recipients`' stored state ([`Stored`]) with its due line, by handle, all of them in
    /// one read: the stored state of a pass's every recipient, which their plans read, as only a
    /// visit under its lease reads its recipient's again ([`Store::stored_one`]).
    fn stored_states(&self, recipients: &[&Callback]) -> Result<BTreeMap<String, Stored>, Error> {
        self.read_stored(recipients, true)
    }

    /// `callback`'s recipient's stored state, read alone ([`Store::stored_states`]), with its due
    /// line when `due`, and with no read of the clock when not.
    fn stored_one(&self, callback: &Callback, due: bool) -> Result<Stored, Error> {
        let handle = callback.handle.as_str();
        (self.read_stored(&[callback], due)?.remove(handle))
            .ok_or_else(|| Error::Db(format!("the board read no callback state for {handle}")))
    }

    /// Each of `recipients`' stored state by handle, in one read, with whether a line of its outbox
    /// is due now when `due` ([`Stored::pending`]).
    fn read_stored(
        &self,
        recipients: &[&Callback],
        due: bool,
    ) -> Result<BTreeMap<String, Stored>, Error> {
        if recipients.is_empty() {
            return Ok(BTreeMap::new());
        }
        let mut vars = json!({"recipients": addressed(recipients)});
        if due {
            vars["now"] = json!(self.now());
        }
        let stored: Vec<Stored> = self.retrying(|| {
            let statement = match due {
                true => both(sql::STORED_DUE.as_str(), pg::stored_due()),
                false => both(sql::STORED.as_str(), pg::stored()),
            };
            self.read(statement, vars.clone())
        })?;
        Ok(stored
            .into_iter()
            .map(|stored| (stored.handle.clone(), stored))
            .collect_map())
    }

    /// The handles of `recipients` with a line of their outbox due now, all of them in one read: the
    /// owner's chat events only for the one holding the seat ([`Callback::chat`]).
    fn pending(&self, recipients: &[&Callback]) -> Result<BTreeSet<String>, Error> {
        #[derive(Deserialize)]
        struct Due {
            handle: String,
            pending: bool,
        }
        if recipients.is_empty() {
            return Ok(BTreeSet::new());
        }
        let vars = json!({"recipients": addressed(recipients), "now": self.now()});
        let due: Vec<Due> =
            self.retrying(|| self.read(both(sql::PENDING.as_str(), pg::pending()), vars.clone()))?;
        Ok((due.into_iter())
            .filter(|due| due.pending)
            .map(|due| due.handle)
            .collect())
    }

    /// Whether a line of `callback`'s outbox is due now ([`Store::pending`]).
    fn callback_pending(&self, callback: &Callback) -> Result<bool, Error> {
        Ok(self.pending(&[callback])?.contains(&callback.handle))
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
            both(sql::DUE, pg::due()),
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
            both(sql::record_delivery(), pg::record_delivery()),
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
                self.retrying(|| {
                    self.transact::<Value>(
                    both(sql::YIELD_CHAT_SEAT, pg::yield_chat_seat()),
                    json!({"handle":handle, "address":callback.address, "owner":callback.owner}),
                )
                })?;
            }
            for event in pending.iter().filter(|event| event.attempts == 2) {
                if let Some(slug) = &event.slug {
                    let body = sql::failure_logged();
                    // No reentrant dispatch from a delivery diagnostic.
                    self.retrying(|| {
                        self.run_transaction::<Value>(both(&body, pg::failure_logged()).posting(), json!({
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

/// `recipients` as a read of their outboxes names them: each one's handle, and whether it holds the
/// owner's chat seat.
fn addressed(recipients: &[&Callback]) -> Value {
    (recipients.iter())
        .map(|callback| json!({"handle": callback.handle, "chat": callback.chat}))
        .collect()
}

/// The slot holdings `visit` observed, which an orchestrator's look and its PULL are read with.
fn holdings<'v>(visit: &Visit<'v>) -> Result<&'v pull::Holdings, Error> {
    (visit.holdings)
        .ok_or_else(|| Error::Db("orchestrator PULL has no holdings observation".into()))
}

/// Whether a visit to the recipient in `state` upgrades a legacy observer: one an older dispatcher
/// recorded a look for and no newer one has written for since, which it baselines once.
fn upgrades_legacy(state: &State) -> bool {
    state.delivery_version.is_none() && state.seen.is_some()
}

/// The events of `lines`, what `handle` hears changed, keyed by the lease `revision` they are
/// queued under.
fn line_events(
    handle: &str,
    board: &super::Board,
    lines: Vec<String>,
    revision: u64,
) -> Vec<Event> {
    (lines.into_iter().enumerate())
        .map(|(i, text)| {
            let key = format!("event-{revision}-{i}");
            Event::line(key, handle, text, session_task(board, handle), None)
        })
        .collect()
}

fn event_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Why a session hears the owner's chat ([`Store::chat_seat`]): it holds the seat, or, while
/// nobody does, it is a fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatSeat {
    /// It took the seat (`callback --chat`).
    Held,
    /// It stands next in line for the seat (`callback --chat --if-free`).
    NextInLine,
    /// A live orchestrator with a native callback, the first by handle.
    Orchestrator,
    /// An orchestrator reading by `tasks wait`, with no channel.
    Waiting,
}

fn recipient(callbacks: &[Callback]) -> Option<&str> {
    seat(callbacks).map(|(handle, _)| handle)
}

fn seat(callbacks: &[Callback]) -> Option<(&str, ChatSeat)> {
    let held = callbacks.iter().find(|c| c.chat);
    let next = || callbacks.iter().find(|c| c.chat_fallback);
    // Handles are stable; database row order and registration order do not choose chat.
    let orchestrator = || {
        callbacks
            .iter()
            .filter(|c| c.live_orchestrator)
            .min_by(|a, b| a.handle.cmp(&b.handle))
    };
    (held.map(|c| (c, ChatSeat::Held)))
        .or_else(|| next().map(|c| (c, ChatSeat::NextInLine)))
        .or_else(|| orchestrator().map(|c| (c, ChatSeat::Orchestrator)))
        .map(|(c, why)| (c.handle.as_str(), why))
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
    } else if let Some(id) = address.strip_prefix(crate::launch::CODEX)
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
    let thread = address.strip_prefix(crate::launch::CODEX).unwrap_or("");
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
