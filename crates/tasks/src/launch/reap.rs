//! Close the hc terminals of agents that sit idle with their board work finished: the periodic
//! sweep.
//!
//! Owner, 2026-10-07: "there seems to be a ton of claude sessions doing nothing. Kill them if the work
//! has finished (and this should happen periodically, not something that needs a reminder)". A task
//! that becomes done or cancelled closes its own sessions at once ([`super::finish`], from `just
//! tasks move/verdict/done`); this sweep catches the rest. `just tasks reap` is one sweep;
//! Symphony (`just symphony`) runs it every [`REAP_SECONDS`].
//!
//! Every sweep holds `reap.json.lock` (`flock`, not waited on), so two sweeps never overlap: a second
//! app's timer, or `just tasks reap`, that comes while one runs sweeps nothing. Sweeps that follow
//! each other closely (two apps, one per worktree, or a hand run after the timer) cannot shorten the
//! wait below: `reap.json` keeps when each terminal was first seen idle, by the board's clock. Both
//! are in `sf-tasks/` of the git common dir, beside the board's server ([`state_path`]). Nothing is
//! printed into an agent's monitor; each close is one line on a task thread.
//!
//! Only this board's terminals (`sf.board`) are swept. One is closed (killed, then removed) when
//! every one of these holds at two sweeps in a row, the first at least [`REAP_SECONDS`] before the
//! second (so an agent just launched, or between two steps, is never closed):
//!
//! - hc calls its agent `idle` or `done`, or `blocked` on the folder-trust prompt (a launch that
//!   never started);
//! - it is named (`claude -n HANDLE`, else its `sf.handle` label, else its terminal name) for a
//!   board session that is not an orchestrator or a chat;
//! - it is not the calling terminal (`HC_TERMINAL_ID`) or the caller's handle;
//! - neither that handle nor any session under it (`parent`, at any depth, including through a
//!   session that left) holds an executor role on an `in_progress`, `blocked` or `review` task or a
//!   reviewer role on a task in `review`: an executor in review waits on its reviewers, whose
//!   changes must reach it, and [`super::finish`] closes it once the task is done;
//! - neither it nor its descendants has an unanswered addressed post, and no reviewer is waiting
//!   for re-review after changes (a verdict or a landing changes message);
//! - nothing runs under its agent but what is known to be harmless ([`procs::reap_busy_reason`]):
//!   MCP helpers (Codex's REPL helper by its configured path), caffeinate, a board command or
//!   monitor (`just tasks…`, the `tasks` binary).
//!   Anything else, whatever it is, is work. A terminal whose processes cannot be read is kept.
//! - the owner is not looking at it ([`super::screen`]): not in the tab the Herdr app shows, not
//!   the agent it shows, not held over them; when the window cannot say, it is kept.
//!
//! Native channels are checked during the sweep, independently of tool activity: a Codex thread is
//! present while hc runs Codex in its terminal ([`crate::Presence`]). That does not exempt a
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

mod work;
use work::live_work;

use super::cleanup::{restore_line, stored_session};
use super::hc::{self, Hc, Object, text_of};
use super::{Processes, procs, screen};
use crate::cli::Env;
use crate::sys::FileLock;
use crate::{Error, NewLaunch, Presence, Store, git};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The Symphony sweeps this often, and a terminal must have been idle this long: 5 to 10 minutes.
pub const REAP_SECONDS: f64 = 300.0;
const READY: [&str; 2] = ["idle", "done"];
const TRUST_PROMPT: &str = "trust this folder";
const NOT_REAPED_KINDS: [&str; 2] = ["orchestrator", "chat"];
const WHY: &str = "idle for two sweeps, its work finished";

/// A terminal the sweep closed, or for a dry run would close.
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    pub pane: String,
    pub handle: String,
    /// Empty when the handle had no task to record the close on (and for a dry run).
    pub restore: String,
}

/// One sweep's result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sweep {
    /// Closed, or for a dry run would be.
    pub closed: Vec<Closed>,
    /// Handles idle this sweep, waiting for a second one.
    pub idle: Vec<String>,
}

/// How a sweep runs: where its state is, whether it closes anything, who calls it.
pub struct Reap<'a> {
    pub state: PathBuf,
    pub apply: bool,
    pub processes: &'a Processes,
    pub caller: Option<&'a str>,
    pub min_idle: f64,
}

/// Where the sweeps of the board `env` reaches from `cwd` keep their state: `TASKS_REAP_STATE`; for a
/// board `TASKS_SERVER` names (a test's), a file of its own in the temp dir, never the live board's;
/// else `sf-tasks/reap.json` in the git common dir, beside the board's server.
pub fn state_path(env: &Env, cwd: &Path) -> Result<PathBuf, String> {
    if let Some(named) = env.var("TASKS_REAP_STATE") {
        return Ok(PathBuf::from(named));
    }
    if env.var("TASKS_SERVER").is_some()
        || env
            .var("TASKS_DATABASE")
            .is_some_and(|db| db != crate::DATABASE)
    {
        let board = format!(
            "{} {}",
            env.var("TASKS_SERVER").unwrap_or("local"),
            env.var("TASKS_DATABASE").unwrap_or("")
        );
        return Ok(std::env::temp_dir()
            .join("sf-tasks-reap")
            .join(format!("reap-{:016x}.json", fnv1a(board.as_bytes()))));
    }
    let common = git::common_dir(cwd)
        .map_err(|why| format!("not in a git repository ({why}): no task board to sweep"))?;
    Ok(common.join("sf-tasks").join("reap.json"))
}

/// FNV-1a, 64 bits: a name of its own for each test board's state file.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn lock_path(state: &Path) -> PathBuf {
    let mut name = state.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    state.with_file_name(name)
}

/// The sweeps' lock beside `state`, not waited for: None while another sweep holds it.
fn lock(state: &Path) -> std::io::Result<Option<FileLock>> {
    if let Some(folder) = state.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let file = File::options()
        .append(true)
        .create(true)
        .open(lock_path(state))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// What hc's live terminals say of Codex agents: the terminals running Codex, and the threads the
/// board's open launch records put in one of them.
pub fn presence(store: &Store, live: &[Object]) -> Result<Presence, Error> {
    let terminals: std::collections::BTreeSet<String> = live
        .iter()
        .filter(|t| hc::agent(t) == Some("codex"))
        .map(|t| text_of(t, "address"))
        .filter(|a| !a.is_empty())
        .collect();
    let threads = store
        .open_task_sessions()?
        .into_iter()
        .filter(|s| terminals.contains(&s.pane))
        .filter_map(|s| {
            let thread = s.claude_session.strip_prefix("codex:")?;
            (!thread.is_empty()).then(|| thread.to_string())
        })
        .collect();
    Ok(Presence { terminals, threads })
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
    let sessions: HashMap<String, String> = store
        .sessions()?
        .into_iter()
        .map(|s| (s.row.handle, s.row.kind))
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
    let current = hc.caller();
    let callbacks = store.callbacks()?;
    let held = live_work(store)?;
    let now = store.now();
    let seen = read_seen(&how.state, now);
    let mut idle_now: BTreeMap<String, f64> = BTreeMap::new();
    let mut result = Sweep::default();
    for listed in terminals {
        let address = text_of(&listed, "address");
        if address.is_empty()
            || text_of(&listed, "state") != "live"
            || Some(&address) == current.as_ref()
            || !ready(hc, &address, &hc::status(&listed))
        {
            continue;
        }
        let Ok(Some(terminal)) = hc.get(&address) else {
            continue;
        };
        if hc::agent(&terminal).is_none() {
            continue;
        }
        let Some(handle) = name(&terminal) else {
            continue;
        };
        if Some(handle.as_str()) == how.caller
            || sessions
                .get(&handle)
                .is_none_or(|kind| NOT_REAPED_KINDS.contains(&kind.as_str()))
            || held.contains(&handle)
        {
            continue;
        }
        let Some(pid) = hc::top_pid(&terminal) else {
            continue;
        };
        match (how.processes)(pid) {
            Ok(commands) if procs::reap_busy_reason(&commands).is_none() => {}
            _ => continue, // work, or `ps` could not say: never closed on no information
        }
        if !matches!(screen::on_screen(hc, &terminal), Ok(false)) {
            continue; // the owner may be looking at it: its idle wait starts again
        }
        let key = format!("{address} {handle} {pid}");
        let since = *seen.get(&key).unwrap_or(&now);
        idle_now.insert(key.clone(), since);
        if !seen.contains_key(&key) || now - since < how.min_idle {
            result.idle.push(handle);
            continue;
        }
        let Some(confirmed) = still_idle(store, hc, how, &address, &handle, pid)? else {
            idle_now.remove(&key);
            continue;
        };
        if !how.apply {
            result.closed.push(Closed {
                pane: address,
                handle,
                restore: String::new(),
            });
            continue;
        }
        let closed_before: HashSet<String> = store
            .sessions_of(&handle)?
            .into_iter()
            .filter(|s| s.closed.is_some())
            .map(|s| s.slug)
            .collect();
        // first: a close is never left without its line
        let restore = match record(store, &handle, &confirmed) {
            Ok(restore) => restore,
            Err(problem) => {
                let _ = writeln!(
                    warn,
                    "· idle sweep kept {address} ({handle}): the board took no record: {problem}"
                );
                let _ = warn.flush();
                continue;
            }
        };
        let finished = |now: &Object| still_finished(hc, how, &address, pid, now);
        match screen::close_unseen(hc, &address, &finished) {
            Ok(()) => {}
            Err(screen::NotClosed::Refused(_)) => {
                reopen(store, &handle, &closed_before, &address)?;
                continue;
            }
            Err(screen::NotClosed::Kept(_)) => {
                // Work resumed, the owner looked, or a call could not say, after the record: its
                // idle wait starts again.
                reopen(store, &handle, &closed_before, &address)?;
                idle_now.remove(&key);
                continue;
            }
        }
        if let Some(callback) = callbacks.get(&handle) {
            store.clear_callback(callback, "finished idle terminal closed")?;
        }
        idle_now.remove(&key);
        result.closed.push(Closed {
            pane: address,
            handle,
            restore,
        });
    }
    if how.apply {
        let idle: Map<String, Value> = idle_now
            .into_iter()
            .map(|(key, since)| (key, json!(since)))
            .collect();
        std::fs::write(&how.state, json!({"idle": idle}).to_string()).map_err(io)?;
    }
    Ok(Some(result))
}

/// Terminal key -> when a sweep first saw it idle (the board's clock); nothing for a state it cannot
/// read.
fn read_seen(state: &Path, now: f64) -> HashMap<String, f64> {
    let Ok(text) = std::fs::read_to_string(state) else {
        return HashMap::new();
    };
    let Ok(Value::Object(read)) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    let Some(Value::Object(idle)) = read.get("idle") else {
        return HashMap::new();
    };
    let mut seen = HashMap::new();
    for (key, since) in idle {
        let Some(since) = since
            .as_f64()
            .filter(|since| since.is_finite() && *since > 0.0 && *since <= now)
        else {
            continue; // a malformed or impossible mark starts a fresh idle wait
        };
        seen.insert(key.clone(), since);
    }
    seen
}

/// Its handle: claude's `-n` (or `--name`), else its `sf.handle` label, else its terminal's name.
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

/// Confirm the same program and owner right before closing, then recheck the confirmed occupant's
/// eligibility. Any missing or changed evidence keeps the terminal and resets its idle wait.
fn still_idle(
    store: &Store,
    hc: &dyn Hc,
    how: &Reap<'_>,
    address: &str,
    handle: &str,
    pid: i64,
) -> Result<Option<Object>, Error> {
    let Ok(Some(now)) = hc.get(address) else {
        return Ok(None);
    };
    let Some(current_handle) = name(&now) else {
        return Ok(None);
    };
    if text_of(&now, "state") != "live"
        || hc::top_pid(&now) != Some(pid)
        || hc::agent(&now).is_none()
        || current_handle != handle
        || Some(current_handle.as_str()) == how.caller
        || !ready(hc, address, &hc::status(&now))
    {
        return Ok(None);
    }
    let sessions = store.sessions()?;
    if sessions
        .iter()
        .find(|s| s.row.handle == current_handle)
        .is_none_or(|s| NOT_REAPED_KINDS.contains(&s.row.kind.as_str()))
        || live_work(store)?.contains(&current_handle)
        || !(how.processes)(pid).is_ok_and(|commands| procs::reap_busy_reason(&commands).is_none())
        || !matches!(screen::on_screen(hc, &now), Ok(false))
    {
        return Ok(None);
    }
    Ok(Some(now))
}

/// Whether the terminal, as the close's own `hc get` finds it (`now`), is still the idle agent the
/// sweep confirmed: its program ended (nothing to lose), or the same agent under `pid`, ready
/// ([`ready`]), with nothing under it that is work. Else why it stays.
fn still_finished(
    hc: &dyn Hc,
    how: &Reap<'_>,
    address: &str,
    pid: i64,
    now: &Object,
) -> Result<(), String> {
    if text_of(now, "state") != "live" {
        return Ok(());
    }
    if hc::top_pid(now) != Some(pid) || hc::agent(now).is_none() {
        return Err("another program runs in it now".into());
    }
    let status = hc::status(now);
    if !ready(hc, address, &status) {
        let status = if status.is_empty() { "silent" } else { &status };
        return Err(format!("its agent is {status} now"));
    }
    match (how.processes)(pid) {
        Ok(commands) => procs::reap_busy_reason(&commands)
            .map_or(Ok(()), |why| Err(format!("work started under it: {why}"))),
        Err(problem) => Err(screen::no_information(&problem)),
    }
}

/// One line on a task thread for the close: the restore line, or "" when the handle had no task.
fn record(store: &Store, handle: &str, terminal: &Object) -> Result<String, Error> {
    let sessions = store.sessions_of(handle)?;
    let still_open: Vec<_> = sessions.iter().filter(|s| s.closed.is_none()).collect();
    if let Some(last) = still_open.last() {
        for s in &still_open {
            store.close_task_session(&s.slug, handle, WHY, &restore_line(&s.slug, handle))?;
        }
        return Ok(restore_line(&last.slug, handle));
    }
    let address = text_of(terminal, "address");
    if let Some(recorded) = sessions.iter().rev().find(|s| {
        s.closed.is_some()
            && s.closed_why == WHY
            && s.pane == address
            && same_conversation(&s.claude_session, &stored_session(terminal))
    }) {
        // A previous sweep may have stopped after the board transaction but before hc closed the
        // terminal. The durable record already carries its line; do not overwrite and log it again.
        return Ok(recorded.restore.clone());
    }
    let mut roles: Vec<(f64, String, String)> = store
        .tasks(None)?
        .into_iter()
        .flat_map(|t| {
            t.roles
                .into_iter()
                .filter(|r| r.handle == handle && (r.role == "executor" || r.role == "reviewer"))
                .map(move |r| (r.since, t.slug.clone(), r.role))
        })
        .collect();
    roles.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| (&a.1, &a.2).cmp(&(&b.1, &b.2)))
    });
    let Some((_, slug, role)) = roles.pop() else {
        return Ok(String::new());
    };
    let board = store.session(handle)?.row;
    let cwd = text_of(terminal, "cwd");
    store.record_launch(&NewLaunch {
        slug: slug.clone(),
        handle: handle.into(),
        role,
        claude_session: stored_session(terminal),
        worktree: if cwd.is_empty() { board.worktree } else { cwd },
        branch: board.branch,
        pane: address,
        model: board.model,
        how: "found".into(),
        log: false,
    })?;
    store.close_task_session(&slug, handle, WHY, &restore_line(&slug, handle))?;
    Ok(restore_line(&slug, handle))
}

/// Whether a record's conversation is the one a terminal runs: the same, or a Codex thread the record
/// took from the agent's callback where the terminal's command line names none (`codex:`).
fn same_conversation(recorded: &str, running: &str) -> bool {
    recorded == running || (running == "codex:" && recorded.starts_with("codex:"))
}

/// hc refused a close already recorded: the sessions it closed are open again.
fn reopen(store: &Store, handle: &str, before: &HashSet<String>, pane: &str) -> Result<(), Error> {
    for s in store.sessions_of(handle)? {
        if s.closed.is_some() && !before.contains(&s.slug) {
            store.reopen_task_session(&s.slug, handle, pane)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ended_sweep_frees_its_lock_while_a_forked_child_still_holds_the_descriptor() {
        let dir = std::env::temp_dir().join(format!("sf-reap-release-{}", std::process::id()));
        let state = dir.join("reap.json");
        let held = lock(&state).unwrap().unwrap();
        // What fork gives a child until it execs: the same open file, so the same lock.
        let inherited = held.0.try_clone().unwrap();
        drop(held);
        let next = lock(&state).unwrap();
        assert!(next.is_some(), "the ended sweep still holds its lock");
        assert!(lock(&state).unwrap().is_none());
        drop((inherited, next));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
