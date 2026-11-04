//! Close the herdr panes of Claude agents that sit idle with their board work finished: the periodic
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
//! wait below: `reap.json` keeps when each pane was first seen idle, by the board's clock. Both are in
//! `sf-tasks/` of the git common dir, beside the board's server ([`state_path`]). Nothing is printed
//! into an agent's monitor; each close is one line on a task thread.
//!
//! A pane is closed when every one of these holds at two sweeps in a row, the first at least
//! [`REAP_SECONDS`] before the second (so an agent just launched, or between two steps, is never
//! closed):
//!
//! - herdr calls its agent `idle` or `done`, or `blocked` on the folder-trust prompt (a launch that
//!   never started);
//! - it is named (`claude -n HANDLE`, else herdr's agent name) for a board session that is not an
//!   orchestrator or a chat: an unnamed pane may be the owner's;
//! - it is not the calling pane, the focused pane or the caller's handle;
//! - neither that handle nor any session under it (`parent`, at any depth, including through a
//!   session that left) holds an executor role on an `in_progress`, `blocked` or `review` task or a
//!   reviewer role on a task in `review`: an executor in review waits on its reviewers, whose
//!   changes must reach its pane, and [`super::finish`] closes it once the task is done;
//! - nothing runs under its claude process but what is known to be harmless
//!   ([`procs::reap_busy_reason`]): MCP helpers, caffeinate, a board command or monitor (`just
//!   tasks…`, the `tasks` binary). Anything else, whatever it is, is work. A pane whose processes
//!   cannot be read is kept.
//!
//! Native channels are checked during the sweep, independently of tool activity. That does not
//! exempt a finished idle worker's pane from the normal cleanup rules below.
//!
//! Right before the close, the conversation, owner, status, focus, processes and board work are read
//! again: an unconfirmed or changed occupant restarts the idle wait; focus, a prompt, work or a claim
//! that came during the sweep keeps the pane. The close is recorded
//! first, on the handle's open task sessions (`closed the pane of …; restore: just tasks restore …`),
//! else on the last task it held a role on, as a session found and closed in that one line; a record that fails
//! leaves the pane open, and a close herdr refuses reopens the session. A sweep interrupted after
//! recording a close reuses that record when it retries closing the same pane and conversation.

use super::cleanup::{restore_line, session_value};
use super::herdr::{Herdr, Object, text};
use super::{Processes, procs};
use crate::cli::Env;
use crate::sys::FileLock;
use crate::{Error, NewLaunch, Store, git};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The Symphony sweeps this often, and a pane must have been idle this long: 5 to 10 minutes idle.
pub const REAP_SECONDS: f64 = 300.0;
const READY: [&str; 2] = ["idle", "done"];
const TRUST_PROMPT: &str = "trust this folder";
const NOT_REAPED_KINDS: [&str; 2] = ["orchestrator", "chat"];
const WHY: &str = "idle for two sweeps, its work finished";

/// A pane the sweep closed, or for a dry run would close.
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
    pub current_pane: Option<&'a str>,
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

/// One sweep (the module doc says which panes close); what it could not record goes to `warn`. None
/// when another sweep holds the lock.
pub fn sweep(
    store: &Store,
    herdr: &dyn Herdr,
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
    let all_agents = herdr.all_agents();
    let pane_sessions = all_agents
        .as_ref()
        .ok()
        .map(|agents| native_sessions(agents));
    // Probe sockets even when Herdr cannot answer: a native Claude inbox needs no pane wrapper.
    store.callback_presence(
        pane_sessions
            .as_ref()
            .map(|(sessions, complete)| (sessions, *complete)),
        how.apply,
    )?;
    if pane_sessions
        .as_ref()
        .is_some_and(|(_, complete)| !*complete)
    {
        let _ = writeln!(
            warn,
            "native presence incomplete: a Codex pane has no thread or pane identity; known threads confirmed, missing registrations retained"
        );
    }
    // Only panes in this sweep's workspace lose their old idle marks. Other workspaces' marks
    // must survive so alternating scoped sweeps do not keep resetting each other's idle wait.
    let workspace = herdr.workspace();
    let outside: HashSet<String> = all_agents
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|agent| match (&workspace, agent.get("workspace_id")) {
            (Some(ours), Some(theirs)) if !theirs.is_null() => {
                theirs.as_str() != Some(ours.as_str())
            }
            _ => false,
        })
        .map(|agent| text(agent.get("pane_id")))
        .collect();
    let agents = match all_agents.and_then(|_| herdr.agents()) {
        Ok(agents) => agents,
        Err(error) => {
            let _ = writeln!(
                warn,
                "pane sweep unavailable: {error}; native sockets checked"
            );
            return Ok(Some(Sweep::default()));
        }
    };
    let callbacks = store.callbacks()?;
    let held = live_work(store)?;
    let now = store.now();
    let seen = read_seen(&how.state, now);
    let mut idle_now: BTreeMap<String, f64> = seen
        .iter()
        .filter(|(key, _)| {
            key.split_once(' ')
                .is_some_and(|(pane, _)| outside.contains(pane))
        })
        .map(|(key, since)| (key.clone(), *since))
        .collect();
    let mut result = Sweep::default();
    for agent in agents {
        let pane = text(agent.get("pane_id"));
        if pane.is_empty()
            || Some(pane.as_str()) == how.current_pane
            || super::herdr::truthy(agent.get("focused"))
            || !ready(herdr, &agent)
        {
            continue;
        }
        let Ok(info) = herdr.process_info(&pane) else {
            continue;
        };
        let claude = claude(&info);
        let Some(handle) = name(&agent, claude.as_ref()) else {
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
        let Some(pid) = claude.as_ref().and_then(|c| c.pid) else {
            continue;
        };
        match (how.processes)(pid) {
            Ok(commands) if procs::reap_busy_reason(&commands).is_none() => {}
            _ => continue, // work, or `ps` could not say: never closed on no information
        }
        let Some(conversation) = conversation(&agent) else {
            continue;
        };
        let key = format!("{pane} {handle} {conversation}");
        let since = *seen.get(&key).unwrap_or(&now);
        idle_now.insert(key.clone(), since);
        if !seen.contains_key(&key) || now - since < how.min_idle {
            result.idle.push(handle);
            continue;
        }
        let Some(confirmed) = still_idle(store, herdr, how, &agent, &handle)? else {
            idle_now.remove(&key);
            continue;
        };
        if !how.apply {
            result.closed.push(Closed {
                pane,
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
                    "· idle sweep kept {pane} ({handle}): the board took no record: {problem}"
                );
                let _ = warn.flush();
                continue;
            }
        };
        if herdr.close(&pane).is_err() {
            reopen(store, &handle, &closed_before, &pane)?;
            continue;
        }
        if let Some(callback) = callbacks.get(&handle) {
            store.clear_callback(callback, "finished idle pane closed")?;
        }
        idle_now.remove(&key);
        result.closed.push(Closed {
            pane,
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

/// Pane key -> when a sweep first saw it idle (the board's clock); nothing for a state it cannot read.
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

/// Handles that hold live work, themselves or through a session under them (a subagent): an
/// executor on a task `in_progress`, `blocked` or in `review`, a reviewer on one in `review`. A
/// departed middle session still links its live descendants to their ancestors; leaving releases
/// its own roles.
fn live_work(store: &Store) -> Result<HashSet<String>, Error> {
    let mut own: Vec<String> = Vec::new();
    for task in store.tasks(None)? {
        for role in &task.roles {
            let live = match role.role.as_str() {
                "executor" => matches!(task.status.as_str(), "in_progress" | "blocked" | "review"),
                "reviewer" => task.status == "review",
                _ => false,
            };
            if live {
                own.push(role.handle.clone());
            }
        }
    }
    let parent: HashMap<String, String> = store
        .sessions()?
        .into_iter()
        .filter_map(|s| Some((s.row.handle, s.row.parent?)))
        .collect();
    let mut held = HashSet::new();
    for handle in own {
        let mut seen = HashSet::new();
        let mut at = Some(handle);
        while let Some(here) = at.filter(|h| seen.insert(h.clone())) {
            // the holder and every session above it
            held.insert(here.clone());
            at = parent.get(&here).cloned();
        }
    }
    Ok(held)
}

/// The claude process in a pane: its pid and its argv.
struct Claude {
    pid: Option<i64>,
    argv: Vec<String>,
}

fn claude(info: &Object) -> Option<Claude> {
    let Some(Value::Array(processes)) = info.get("foreground_processes") else {
        return None;
    };
    processes.iter().find_map(|process| {
        let argv: Vec<String> = match process.get("argv") {
            Some(Value::Array(argv)) if !argv.is_empty() => {
                argv.iter().map(|a| text(Some(a))).collect()
            }
            _ => text(process.get("cmdline"))
                .split(procs::space)
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect(),
        };
        let base = argv
            .first()
            .map(|first| first.rsplit('/').next().unwrap_or(first));
        let is_claude = ["claude", "codex"].contains(&text(process.get("name")).as_str())
            || base.is_some_and(|name| ["claude", "codex"].contains(&name));
        is_claude.then(|| Claude {
            pid: process
                .get("pid")
                .and_then(Value::as_i64)
                .filter(|p| *p != 0),
            argv,
        })
    })
}

/// Its handle: claude's `-n` (or `--name`), else herdr's name for the agent.
fn name(agent: &Object, claude: Option<&Claude>) -> Option<String> {
    let argv: &[String] = claude.map_or(&[], |c| &c.argv);
    let flagged = argv.split_last().map_or(&[][..], |(_, before)| before);
    for flag in ["-n", "--name"] {
        if let Some(at) = flagged.iter().position(|a| a == flag) {
            return Some(argv[at + 1].clone());
        }
    }
    Some(text(agent.get("name"))).filter(|n| !n.is_empty())
}

fn ready(herdr: &dyn Herdr, agent: &Object) -> bool {
    let status = text(agent.get("agent_status"));
    if READY.contains(&status.as_str()) {
        return true;
    }
    if status != "blocked" {
        return false;
    }
    herdr
        .read(&text(agent.get("pane_id")), 30)
        .is_ok_and(|screen| screen.to_lowercase().contains(TRUST_PROMPT))
}

/// A known conversation, including its agent kind: an unidentified or malformed id cannot confirm
/// that a pane still holds the same occupant.
fn conversation(agent: &Object) -> Option<String> {
    agent
        .get("agent_session")?
        .get("value")?
        .as_str()
        .filter(|id| !id.is_empty())?;
    Some(super::cleanup::stored_session(agent))
}

/// Confirm the same conversation and owner right before closing, then recheck the confirmed
/// occupant's eligibility. Any missing or changed evidence keeps the pane and resets its idle wait.
fn still_idle(
    store: &Store,
    herdr: &dyn Herdr,
    how: &Reap<'_>,
    candidate: &Object,
    handle: &str,
) -> Result<Option<Object>, Error> {
    let pane = text(candidate.get("pane_id"));
    let Ok(info) = herdr.process_info(&pane) else {
        return Ok(None);
    };
    let Some(process) = claude(&info) else {
        return Ok(None);
    };
    let Some(pid) = process.pid else {
        return Ok(None);
    };
    // Read the conversation, focus and status after process-info, so changes during that read are
    // included in the final pane snapshot too.
    let Ok(agents) = herdr.agents() else {
        return Ok(None);
    };
    let Some(now) = agents.iter().find(|a| text(a.get("pane_id")) == pane) else {
        return Ok(None);
    };
    let Some(current_conversation) = conversation(now) else {
        return Ok(None);
    };
    let Some(current_handle) = name(now, Some(&process)) else {
        return Ok(None);
    };
    if Some(current_conversation) != conversation(candidate)
        || current_handle != handle
        || Some(pane.as_str()) == how.current_pane
        || Some(current_handle.as_str()) == how.caller
        || super::herdr::truthy(now.get("focused"))
        || !ready(herdr, now)
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
    {
        return Ok(None);
    }
    Ok(Some(now.clone()))
}

fn native_sessions(agents: &[Object]) -> (BTreeSet<String>, bool) {
    let mut sessions = BTreeSet::new();
    let mut complete = true;
    for agent in agents {
        let pane = text(agent.get("pane_id"));
        let session = session_value(agent);
        let codex = ["agent", "kind"]
            .iter()
            .any(|key| text(agent.get(*key)) == "codex");
        // Herdr may see the pane before its thread identity. That is incomplete evidence, not
        // evidence that every previously registered thread has disappeared.
        if codex && (pane.is_empty() || session.is_empty()) {
            complete = false;
        }
        if !pane.is_empty() && !session.is_empty() {
            sessions.insert(session);
        }
    }
    (sessions, complete)
}

/// One line on a task thread for the close: the restore line, or "" when the handle had no task.
fn record(store: &Store, handle: &str, agent: &Object) -> Result<String, Error> {
    let sessions = store.sessions_of(handle)?;
    let still_open: Vec<_> = sessions.iter().filter(|s| s.closed.is_none()).collect();
    if let Some(last) = still_open.last() {
        for s in &still_open {
            store.close_task_session(&s.slug, handle, WHY, &restore_line(&s.slug, handle))?;
        }
        return Ok(restore_line(&last.slug, handle));
    }
    if let Some(recorded) = sessions.iter().rev().find(|s| {
        s.closed.is_some()
            && s.closed_why == WHY
            && s.pane == text(agent.get("pane_id"))
            && s.claude_session == super::cleanup::stored_session(agent)
    }) {
        // A previous sweep may have stopped after the board transaction but before herdr closed
        // the pane. The durable record already carries its line; do not overwrite and log it again.
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
    let cwd = text(agent.get("cwd"));
    store.record_launch(&NewLaunch {
        slug: slug.clone(),
        handle: handle.into(),
        role,
        claude_session: super::cleanup::stored_session(agent),
        worktree: if cwd.is_empty() { board.worktree } else { cwd },
        branch: board.branch,
        pane: text(agent.get("pane_id")),
        model: board.model,
        how: "found".into(),
        log: false,
    })?;
    store.close_task_session(&slug, handle, WHY, &restore_line(&slug, handle))?;
    Ok(restore_line(&slug, handle))
}

/// Herdr refused a close already recorded: the sessions it closed are open again.
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
