//! Close the hc terminals of agents that sit idle with their board work finished, or that have sat
//! idle at their prompt an hour, and remove the terminals whose program ended: the periodic sweep.
//!
//! A session doing nothing is closed once its work has finished, by a periodic sweep that needs no
//! reminder. A session that is done and not needed soon is closed, then resumed when it is: only
//! sessions doing active work, or that will have active work soon (within the hour), are kept
//! open. A task that becomes done or cancelled closes its own sessions at once
//! ([`super::finish`], from `just tasks move/verdict/done`); this sweep catches the rest. `just
//! tasks reap` is one sweep; Symphony (`just symphony`) runs it every [`REAP_SECONDS`]; `just tasks clean`
//! runs cleanup and one sweep at once.
//!
//! Every sweep holds `reap.json.lock` (`flock`, not waited on), so two sweeps never overlap: a
//! second app's timer, or `just tasks reap`, that comes while one runs sweeps nothing. Sweeps that
//! follow each other closely (two apps, one per worktree, or a hand run after the timer) cannot
//! shorten the wait below: `reap.json` keeps when each terminal was first seen idle, by the board's
//! clock. Both are in `symphony/tasks/` of the git common dir, beside the board's server
//! ([`state_path`]). Nothing is printed into an agent's monitor; each close is one line on a task
//! thread.
//!
//! Only this board's terminals (`symphony.board`) are swept. A live one is closed (killed, then
//! removed) when every one of these holds at two sweeps in a row, the first at least its idle wait
//! before the second (so an agent just launched, or between two steps, is never closed):
//!
//! - hc calls its agent `idle` or `done`, or `blocked` on the folder-trust prompt (a launch that
//!   never started);
//! - it is named (`claude -n HANDLE`, else its `symphony.handle` label, else its terminal name) for
//!   a board session that is not an orchestrator or a chat;
//! - it is not the calling terminal (`HC_TERMINAL_ID`) or the caller's handle;
//! - neither it nor any session under it (`parent`, at any depth, including through a session that
//!   left) has an unanswered addressed post;
//! - nothing runs under its agent but what is known to be harmless ([`procs::reap_busy_reason`]):
//!   MCP helpers (Codex's REPL helper by its configured path, the ChatGPT app's computer-use REPL by
//!   its exact node and module path, Codex's code-mode host by its standalone release's layout),
//!   caffeinate, a board command or monitor (`just tasks…`, the `tasks` binary).
//!   Anything else, whatever it is, is work. A terminal whose processes cannot be read is kept.
//! - the owner is not looking at it ([`super::screen`]): not in the tab the Herdr app shows, not
//!   the agent it shows, not held over them; when the window cannot say, it is kept.
//!
//! Its idle wait is [`REAP_SECONDS`] when its work is finished, and [`IDLE_HOUR`] while it, or a
//! session under it, holds a live reservation on an unfinished task or a role on open work: an
//! executor of a task `in_progress`, `blocked` or in `review`, a reviewer of one in `review` or
//! awaiting its re-review after its changes verdict
//! (a verdict, or a landing's changes message). The hour starts again when the role or its task's
//! status changes, and runs from its agent's last tool call when that is later (`last_tool`, the
//! beat), so work between two sweeps that each find it at its prompt counts. Its close says what
//! brings it back (`work::Hold::closing`): a reviewer launch for the next tip resumes a reviewer, a
//! changes verdict or a post addressed to it restores an executor, as the unanswered post's restore
//! does.
//!
//! A terminal whose program ended is removed at the first sweep that finds it, out of the owner's
//! sight, its open records (those naming that terminal) closed first with their restore line
//! ([`ENDED`]); one whose records are closed already goes without a line, and one whose program
//! runs again by the remove's last look stays, its records open again. A kill waits for its
//! terminal to end before the remove ([`Hc::end`]).
//!
//! Every terminal of the board a sweep leaves open is in [`Sweep::kept`], saying why. `reap.json`
//! keeps the reason last said of each idle one (`said`), so the reap command tells the Symphony's
//! log (its stderr) of each once per change of reason; a dry run lists them all.
//!
//! Native channels are checked during the sweep, independently of tool activity: a Codex thread is
//! present while hc runs Codex in its terminal ([`crate::Presence`]), and its registration is
//! retired once every terminal the board launched it in is closed or gone. That does not exempt a
//! finished idle worker's terminal from the rules above.
//!
//! Right before the close, the program, owner, status, processes, board work and screen are read
//! again: an unconfirmed or changed occupant restarts the idle wait; a prompt, work, a claim or the
//! owner's look that came during the sweep keeps the terminal. The close is recorded first, on the handle's open task
//! sessions (`closed the pane of …; restore: just tasks restore …`), else on the last task it held a
//! role on, as a session found and closed in that one line; a record that fails leaves the terminal
//! open, and a close hc refuses reopens the session. After the record, the close looks once more,
//! right before the kill ([`screen::close_unseen`]): a terminal whose agent works again (its
//! status, or work under it), that is on screen then, or that a call cannot say of, stays, its
//! session reopened and its idle wait started again. A sweep interrupted after
//! recording a close reuses that record when it retries closing the same terminal and conversation.

mod closing;
mod ended;
mod state;
mod work;
use state::{lock, read_said, read_seen, read_state, write_state};
pub use state::{lock_path, state_path};
use work::{Hold, Holds};

use super::hc::{self, Hc, Object, text_of};
use super::screen;
use super::{Processes, procs};
use crate::rows::TaskSessionRow;
use crate::{Callback, Error, Presence, Store};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;

/// The Symphony sweeps this often, and a terminal whose work is finished must have been idle this
/// long: 5 to 10 minutes.
pub const REAP_SECONDS: f64 = 300.0;
/// How long a terminal whose agent holds a role on open work must have sat idle at its prompt
/// before a sweep closes it: an hour, as only work due within the hour keeps a session open.
pub const IDLE_HOUR: f64 = 3600.0;
const READY: [&str; 2] = ["idle", "done"];
const TRUST_PROMPT: &str = "trust this folder";
/// The kinds of session whose terminal the board never closes while it runs: cleanup's rule too.
pub(super) const NOT_REAPED_KINDS: [&str; 2] = ["orchestrator", "chat"];
const WHY: &str = "idle for two sweeps, its work finished";
/// Why a sweep removes a terminal whose program ended, and closes the records naming it.
pub const ENDED: &str = "its program ended";
/// Why a terminal whose work is finished stays at the first sweep that finds it idle.
pub const IDLE_ONCE: &str = "idle once: it closes if still idle at the next sweep";

/// A terminal the sweep closed, or for a dry run would close.
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    pub pane: String,
    /// The board session it ran; empty for an ended terminal of no known session.
    pub handle: String,
    /// Why it closed: its work finished ([`IDLE_ONCE`]'s second sweep), idle an hour holding a
    /// role (with what brings it back), or its program ended ([`ENDED`]).
    pub why: String,
    /// Empty when there was no task to record the close on, when its records were closed already
    /// (and for a dry run of a live one).
    pub restore: String,
}

/// A terminal of the board the sweep left open, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub pane: String,
    /// The board session it was taken for; empty when it names none.
    pub handle: String,
    pub why: String,
    /// Whether its agent sat idle at its prompt.
    pub idle: bool,
    /// Whether this sweep is the first to keep it idle for this reason: what the reap command tells
    /// the Symphony's log.
    pub told: bool,
}

/// One sweep's result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sweep {
    /// Closed, or for a dry run would be.
    pub closed: Vec<Closed>,
    /// Handles idle this sweep with their work finished, waiting for a second one.
    pub idle: Vec<String>,
    /// Every other terminal of the board, live or ended, left open, in the order hc lists them.
    pub kept: Vec<Kept>,
}

/// How a sweep runs: where its state is, whether it closes anything, who calls it.
pub struct Reap<'a> {
    pub state: PathBuf,
    pub apply: bool,
    pub processes: &'a Processes,
    pub caller: Option<&'a str>,
    pub min_idle: f64,
}

/// What hc's live terminals say of Codex agents: the terminals running Codex, the threads the
/// board's open launch records put in one of them, and the threads its launch records name of which
/// no open record names a live terminal ([`Presence::gone`]), all from one read of the records that
/// name a Codex thread and of only what this judges of each (`Store::codex_launches`): a record
/// the board cannot read elsewhere is no information here, and stops no sweep.
pub fn presence(store: &Store, live: &[Object]) -> Result<Presence, Error> {
    let address = |t: &Object| Some(text_of(t, "address")).filter(|a| !a.is_empty());
    let listed: BTreeSet<String> = live.iter().filter_map(address).collect();
    let terminals: BTreeSet<String> = (live.iter())
        .filter(|t| hc::agent(t) == Some("codex"))
        .filter_map(address)
        .collect();
    let (mut threads, mut held, mut launched) = Default::default();
    for record in store.codex_launches()? {
        let Some(thread) = record.claude_session.strip_prefix(crate::launch::CODEX) else {
            continue;
        };
        let insert = |set: &mut BTreeSet<String>| set.insert(thread.to_string());
        insert(&mut launched);
        if record.open && listed.contains(&record.pane) {
            insert(&mut held);
            if terminals.contains(&record.pane) {
                insert(&mut threads);
            }
        }
    }
    let gone = launched.difference(&held).cloned().collect();
    Ok(Presence {
        terminals,
        threads,
        gone,
    })
}

/// One sweep (the module doc says which terminals close); what it could not record goes to `warn`.
/// None when another sweep holds the lock.
pub fn sweep(
    store: &Store,
    hc: &dyn Hc,
    how: &Reap<'_>,
    warn: &mut dyn Write,
) -> Result<Option<Sweep>, Error> {
    let io = |e: std::io::Error| Error::Db(format!("{}: {e}", how.state.display()));
    let _lock = if how.apply {
        let Some(held) = lock(&how.state).map_err(io)? else {
            return Ok(None);
        };
        Some(held)
    } else {
        None
    };
    let sessions: HashMap<String, (String, Option<f64>)> = store
        .sessions()?
        .into_iter()
        .map(|s| (s.row.handle, (s.row.kind, s.row.last_tool)))
        .collect();
    // Probe sockets even when hc cannot answer: a native Claude inbox needs no terminal.
    let codex = match hc.live() {
        Ok(live) => Some(presence(store, &live)?),
        Err(_) => None,
    };
    store.callback_presence(codex.as_ref(), how.apply)?;
    let terminals = match hc.terminals() {
        Ok(terminals) => terminals,
        Err(error) => {
            let _ = writeln!(
                warn,
                "terminal sweep unavailable: {error}; native sockets checked"
            );
            return Ok(Some(Sweep::default()));
        }
    };
    let now = store.now();
    let saved = read_state(&how.state);
    let mut round = Round {
        store,
        hc,
        how,
        warn,
        current: hc.caller(),
        sessions,
        callbacks: store.callbacks()?,
        holds: Holds::read(store)?,
        open: store.open_task_sessions()?,
        now,
        seen: read_seen(&saved, now),
        said: read_said(&saved),
        idle_now: BTreeMap::new(),
        said_now: BTreeMap::new(),
        result: Sweep::default(),
    };
    for listed in &terminals {
        round.look(listed)?;
    }
    if how.apply {
        write_state(&how.state, round.idle_now, &round.said_now).map_err(io)?;
    }
    Ok(Some(round.result))
}

/// One sweep's look at the board's terminals: what it read before the first, and what it found.
struct Round<'a> {
    store: &'a Store,
    hc: &'a dyn Hc,
    how: &'a Reap<'a>,
    warn: &'a mut dyn Write,
    /// The calling terminal (`HC_TERMINAL_ID`).
    current: Option<String>,
    /// Handle -> its session's kind, and when a tool call of its agent last beat.
    sessions: HashMap<String, (String, Option<f64>)>,
    callbacks: BTreeMap<String, Callback>,
    /// What holds the terminals it judges, read for each as it comes.
    holds: Holds<'a>,
    /// The board's open launch records.
    open: Vec<TaskSessionRow>,
    now: f64,
    /// Terminal key -> when a sweep first saw it idle; and what this sweep keeps of it.
    seen: HashMap<String, f64>,
    idle_now: BTreeMap<String, f64>,
    /// Terminal -> why the last sweep said it kept it idle; and what this sweep keeps of it.
    said: HashMap<String, String>,
    said_now: BTreeMap<String, String>,
    result: Sweep,
}

/// What [`Round::judge`] made of a live terminal whose agent is at rest.
enum Look {
    /// hc no longer knows it.
    Gone,
    /// It stays: (its handle, or "" when it names none; why; whether an agent sits idle in it).
    Stays(String, String, bool),
    /// Its idle wait is over: closed if the last looks confirm it.
    Due(Due),
}

/// A terminal whose idle wait is over: the agent `handle` under `pid`, its idle mark `key`, what
/// held it while it waited, and when the sweep read its agent's last tool call (`beat`).
struct Due {
    address: String,
    handle: String,
    pid: i64,
    key: String,
    hold: Option<Hold>,
    beat: Option<f64>,
}

impl Round<'_> {
    /// One terminal as `hc ls` lists it: removed when its program ended, closed when its idle wait
    /// is over, else kept, saying why.
    fn look(&mut self, listed: &Object) -> Result<(), Error> {
        let address = text_of(listed, "address");
        if address.is_empty() {
            return Ok(());
        }
        if text_of(listed, "state") != "live" {
            return self.ended(&address);
        }
        if Some(&address) == self.current.as_ref() {
            let why = "it is the terminal this sweep runs in".into();
            let named = self.named(&address, listed);
            self.keep(&address, &named, why, false);
            return Ok(());
        }
        let status = hc::status(listed);
        if !ready(self.hc, &address, &status) {
            let why = match (hc::agent(listed), status.as_str()) {
                (None, _) => "no agent runs in it".into(),
                (Some(_), "") => "its agent says nothing of its state".into(),
                (Some(_), status) => format!("its agent is {status}"),
            };
            let named = self.named(&address, listed);
            self.keep(&address, &named, why, false);
            return Ok(());
        }
        match self.judge(&address)? {
            Look::Gone => {}
            Look::Stays(handle, why, idle) => self.keep(&address, &handle, why, idle),
            Look::Due(due) => {
                let handle = due.handle.clone();
                if let Some(why) = self.close(due)? {
                    self.keep(&address, &handle, why, true);
                }
            }
        }
        Ok(())
    }

    /// The session a terminal the sweep looks no further at runs, to say whose it is: the board's
    /// open record of it, else its name as `hc ls` lists it, else as `hc get` finds it; "" when
    /// none says.
    fn named(&self, address: &str, listed: &Object) -> String {
        if let Some(record) = self.open.iter().rev().find(|s| s.pane == address) {
            return record.handle.clone();
        }
        name(listed)
            .or_else(|| self.hc.get(address).ok().flatten().and_then(|t| name(&t)))
            .unwrap_or_default()
    }

    /// A live terminal whose agent is at rest, by its own `hc get`: why it stays, or that its idle
    /// wait is over. A terminal that it reads idle and out of the owner's sight gets its idle mark.
    fn judge(&mut self, address: &str) -> Result<Look, Error> {
        let stays = |handle: &str, why: String| Ok(Look::Stays(handle.to_string(), why, true));
        let terminal = match self.hc.get(address) {
            Ok(Some(terminal)) => terminal,
            Ok(None) => return Ok(Look::Gone),
            Err(problem) => return stays("", screen::no_information(&problem)),
        };
        if hc::agent(&terminal).is_none() {
            let why = "no agent runs in it".into();
            return Ok(Look::Stays(name(&terminal).unwrap_or_default(), why, false));
        }
        let Some(handle) = name(&terminal) else {
            return stays("", NO_SESSION.into());
        };
        if Some(handle.as_str()) == self.how.caller {
            return stays(&handle, "it is the session this sweep runs for".into());
        }
        let last_tool = match self.sessions.get(&handle) {
            None => return stays(&handle, NO_SESSION.into()),
            Some((kind, _)) if kind == "orchestrator" => {
                return stays(&handle, "an orchestrator never closes".into());
            }
            Some((kind, _)) if kind == "chat" => {
                return stays(&handle, "a chat never closes".into());
            }
            Some((_, beat)) => *beat,
        };
        let beat = last_tool.unwrap_or(0.0);
        let hold = match self.holds.of(&handle) {
            Ok(hold) => hold,
            Err(problem) => {
                let problem = super::LaunchError(problem.to_string());
                return stays(&handle, screen::no_information(&problem));
            }
        };
        if let Some(post) = hold.as_ref().filter(|hold| hold.is_post()) {
            return stays(&handle, post.why());
        }
        let Some(pid) = hc::top_pid(&terminal) else {
            return stays(
                &handle,
                "hc knows no pid for it: what runs under it is unknown".into(),
            );
        };
        match (self.how.processes)(pid) {
            Ok(commands) => {
                if let Some(why) = procs::reap_busy_reason(&commands) {
                    return stays(&handle, format!("it {why}"));
                }
            }
            Err(problem) => return stays(&handle, screen::no_information(&problem)),
        }
        match screen::on_screen(self.hc, &terminal) {
            Ok(false) => {}
            // the owner may be looking at it: its idle wait starts again
            Ok(true) => return stays(&handle, screen::ON_SCREEN.into()),
            Err(problem) => return stays(&handle, screen::no_information(&problem)),
        }
        // A role's mark names it, so its wait starts again when the role or its task's status
        // changes, and a terminal it no longer holds waits two sweeps as finished work does.
        let key = match &hold {
            Some(role) => format!("{address} {handle} {pid} {}", role.mark()),
            None => format!("{address} {handle} {pid}"),
        };
        let since = *self.seen.get(&key).unwrap_or(&self.now);
        self.idle_now.insert(key.clone(), since);
        // A role's hour runs from its agent's last tool call too, so work done between two sweeps
        // (each finding it at its prompt) starts it again.
        let (wait, from) = if hold.is_some() {
            (IDLE_HOUR, since.max(beat))
        } else {
            (self.how.min_idle, since)
        };
        if !self.seen.contains_key(&key) || self.now - from < wait {
            return match &hold {
                Some(role) => stays(&handle, role.why()),
                None => {
                    self.result.idle.push(handle.clone());
                    stays(&handle, IDLE_ONCE.into())
                }
            };
        }
        Ok(Look::Due(Due {
            address: address.into(),
            handle,
            pid,
            key,
            hold,
            beat: last_tool,
        }))
    }

    /// Keep the terminal at `address` (of `handle`) for `why`: told when its agent is `idle` and the
    /// last sweep said otherwise of it.
    fn keep(&mut self, address: &str, handle: &str, why: String, idle: bool) {
        let said = self.said.get(address);
        let told = idle && said != Some(&why);
        if idle {
            self.said_now.insert(address.into(), why.clone());
        } else if let Some(said) = said {
            self.said_now.insert(address.into(), said.clone());
        }
        self.result.kept.push(Kept {
            pane: address.into(),
            handle: handle.into(),
            why,
            idle,
            told,
        });
    }
}

/// Why a terminal that names no board session stays.
pub const NO_SESSION: &str = "it names no board session: it may be the owner's";

/// Why a terminal stays whose last looks before its close found it changed.
pub const CHANGED: &str = "it changed while this sweep looked: its idle wait starts again";

/// Why a terminal stays whose agent made a tool call after the sweep read its last one, found by
/// the last looks before its close.
pub const TOOL_CALL: &str =
    "its agent made a tool call while this sweep looked: its idle wait starts again";

/// Its handle: claude's `-n` (or `--name`), else its `symphony.handle` label, else its terminal's
/// name.
fn name(terminal: &Object) -> Option<String> {
    let argv = hc::argv(terminal);
    let run = hc::program(&argv);
    let flagged = run.split_last().map_or(&[][..], |(_, before)| before);
    for flag in ["-n", "--name"] {
        if let Some(at) = flagged.iter().position(|a| a == flag) {
            return Some(run[at + 1].clone());
        }
    }
    [
        hc::label(terminal, hc::HANDLE_LABEL),
        text_of(terminal, "name"),
    ]
    .into_iter()
    .find(|n| !n.is_empty())
}

fn ready(hc: &dyn Hc, address: &str, status: &str) -> bool {
    if READY.contains(&status) {
        return true;
    }
    if status != "blocked" {
        return false;
    }
    hc.read(address, 30)
        .is_ok_and(|screen| screen.to_lowercase().contains(TRUST_PROMPT))
}
