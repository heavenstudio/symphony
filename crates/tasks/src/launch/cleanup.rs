//! `just tasks cleanup`: close the panes nobody needs (the module above says which), each closure
//! recorded on its task with the line that restores it; and [`finish`], the same for the sessions of
//! one task that just became done or cancelled.

use super::herdr::{Herdr, Object, text, truthy};
use super::procs::{busy_reason, named};
use super::{LaunchError, Processes, READY_STATES};
use crate::health::FINISHED;
use crate::rows::TaskSessionRow;
use crate::{Error, NewLaunch, Store, Task};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// A pane cleanup closed, or for a dry run would close.
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub pane: String,
    /// The board session it belongs to; none for an idle shell.
    pub handle: Option<String>,
    pub why: String,
    /// The line that brings it back; empty when there is no task to bring it back on.
    pub restore: String,
    /// False for a dry run, or when herdr refused.
    pub closed: bool,
    /// True for a pane left as it is because a call that says whether it is idle failed: `why`
    /// names the call and its error. Never closed on no information, and never silently. Its
    /// handle is the session the board would have closed it for; none when herdr's process-info
    /// failed, before the pane's session was looked for.
    pub kept: bool,
}

impl Closure {
    /// `pane` left as it is, of no known session: `problem`, the failed call's error, said in `why`.
    fn kept(pane: &str, problem: &LaunchError) -> Closure {
        Closure {
            pane: pane.into(),
            handle: None,
            why: format!("never closed on no information: {problem}"),
            restore: String::new(),
            closed: false,
            kept: true,
        }
    }
}

pub(super) fn restore_line(slug: &str, handle: &str) -> String {
    format!("just tasks restore {slug} {handle}")
}

/// Close the panes nobody needs; with `only` slugs, only the panes of those tasks' open sessions.
/// `current_pane` (the caller's) is never closed. A pane whose herdr process-info or process table
/// could not be read stays, as a [`Closure`] that is `kept`, naming the call that failed.
pub fn cleanup(
    store: &Store,
    herdr: &dyn Herdr,
    processes: &Processes,
    dry_run: bool,
    current_pane: Option<&str>,
    only: &[String],
) -> Result<Vec<Closure>, Error> {
    let handles: HashSet<String> = store
        .sessions()?
        .into_iter()
        .map(|s| s.row.handle)
        .collect();
    let open = store.open_task_sessions()?;
    let by_pane: HashMap<&str, &str> = open
        .iter()
        .filter(|s| !s.pane.is_empty())
        .map(|s| (s.pane.as_str(), s.handle.as_str()))
        .collect();
    let by_session: HashMap<&str, &str> = open
        .iter()
        .map(|s| {
            (
                s.claude_session
                    .strip_prefix("codex:")
                    .unwrap_or(&s.claude_session),
                s.handle.as_str(),
            )
        })
        .collect();
    let wanted: Option<HashSet<&str>> = (!only.is_empty()).then(|| {
        open.iter()
            .filter(|s| only.contains(&s.slug))
            .map(|s| s.pane.as_str())
            .collect()
    });
    let mut result = Vec::new();
    for pane in herdr.panes()? {
        let pane_id = text(pane.get("pane_id"));
        if Some(pane_id.as_str()) == current_pane
            || truthy(pane.get("focused"))
            || wanted
                .as_ref()
                .is_some_and(|w| !w.contains(pane_id.as_str()))
        {
            continue;
        }
        let is_agent = truthy(pane.get("agent"));
        if is_agent && !READY_STATES.contains(&text(pane.get("agent_status")).as_str()) {
            continue;
        }
        let info = match herdr.process_info(&pane_id) {
            Ok(info) => info,
            Err(problem) => {
                result.push(Closure::kept(&pane_id, &problem));
                continue;
            }
        };
        let Some(shell_pid) = pid(info.get("shell_pid")) else {
            continue; // herdr knows no shell: nothing to look under, not known to be idle
        };
        // What the board says first, so a table that cannot be read is said only of a pane it
        // would otherwise close.
        let (handle, why, mine) = if is_agent {
            let Some(handle) = handle_of(&pane, &info, store, &handles, &by_pane, &by_session)?
            else {
                continue;
            };
            if store.session(&handle)?.row.kind == "orchestrator" {
                continue;
            }
            let (live, mine) = held(store, &handle)?;
            if !live.is_empty() || (mine.is_empty() && store.sessions_of(&handle)?.is_empty()) {
                continue; // live work, or no task to record the closure and its restore line on
            }
            let why = if mine.is_empty() {
                "it holds nothing live"
            } else {
                "its tasks are finished"
            };
            (Some(handle), why, mine)
        } else {
            (None, "an idle shell", Vec::new())
        };
        match processes(shell_pid) {
            Ok(commands) if busy_reason(&commands).is_none() => {}
            Ok(_) => continue, // work
            Err(problem) => {
                result.push(Closure {
                    handle,
                    ..Closure::kept(&pane_id, &problem)
                });
                continue;
            }
        }
        let slug = open
            .iter()
            .find(|s| Some(s.handle.as_str()) == handle.as_deref())
            .map(|s| s.slug.clone())
            .or_else(|| mine.first().map(|t| t.slug.clone()));
        let restore = match (&slug, &handle) {
            (Some(slug), Some(handle)) => restore_line(slug, handle),
            _ => String::new(),
        };
        let mut closure = Closure {
            pane: pane_id.clone(),
            handle: handle.clone(),
            why: why.into(),
            restore,
            closed: false,
            kept: false,
        };
        if !dry_run {
            if herdr.close(&pane_id).is_err() {
                result.push(closure);
                continue;
            }
            closure.closed = true;
            if let Some(handle) = &handle {
                record_closure(store, handle, &pane, why)?;
            }
        }
        result.push(closure);
    }
    Ok(result)
}

/// A pid as herdr gives it: a number, or digits in a string; none for nothing, zero or anything else.
fn pid(value: Option<&Value>) -> Option<i64> {
    let pid = match value? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (pid != 0).then_some(pid)
}

/// The board session a pane belongs to: its recorded pane, its Claude session id (recorded, or the
/// one an agent's beats are bound to), a `-n HANDLE` flag of what runs in it, its title.
fn handle_of(
    pane: &Object,
    info: &Object,
    store: &Store,
    handles: &HashSet<String>,
    by_pane: &HashMap<&str, &str>,
    by_session: &HashMap<&str, &str>,
) -> Result<Option<String>, Error> {
    if let Some(handle) = by_pane.get(text(pane.get("pane_id")).as_str()) {
        return Ok(Some(handle.to_string()));
    }
    let value = session_value(pane);
    if !value.is_empty() {
        if let Some(handle) = by_session.get(value.as_str()) {
            return Ok(Some(handle.to_string()));
        }
        if let Some(bound) = store.handle_for_agent(&value)? {
            return Ok(Some(bound));
        }
    }
    let processes = match info.get("foreground_processes") {
        Some(Value::Array(all)) => all.as_slice(),
        _ => &[],
    };
    for process in processes {
        let cmdline = text(process.get("cmdline"));
        if let Some(name) = named(&cmdline)
            && handles.contains(name)
        {
            return Ok(Some(name.to_string()));
        }
    }
    let title = text(pane.get("terminal_title_stripped"));
    Ok(handles.contains(&title).then_some(title))
}

/// The Claude session id herdr read in a pane (`agent_session.value`), or "".
pub(super) fn session_value(pane: &Object) -> String {
    text(pane.get("agent_session").and_then(|s| s.get("value")))
}

pub(super) fn stored_session(pane: &Object) -> String {
    let id = session_value(pane);
    if text(pane.get("agent")) == "codex" || text(pane.get("kind")) == "codex" {
        format!("codex:{id}")
    } else {
        id
    }
}

/// (the tasks `handle` holds a role on that are live, all tasks it holds a role on).
fn held(store: &Store, handle: &str) -> Result<(Vec<Task>, Vec<Task>), Error> {
    let mine: Vec<Task> = store
        .tasks(None)?
        .into_iter()
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let live = mine
        .iter()
        .filter(|t| !FINISHED.contains(&t.status.as_str()))
        .cloned()
        .collect();
    Ok((live, mine))
}

/// Put the closure on every task the session ran: its recorded sessions, else one per task it holds
/// a role on, recorded as found.
fn record_closure(store: &Store, handle: &str, pane: &Object, why: &str) -> Result<(), Error> {
    let mut mine: Vec<TaskSessionRow> = store.sessions_of(handle)?;
    if mine.is_empty() {
        let board = store.session(handle)?.row;
        let cwd = text(pane.get("cwd"));
        for task in held(store, handle)?.1 {
            let role = if task.holders("reviewer").contains(&handle) {
                "reviewer"
            } else {
                "executor"
            };
            mine.push(store.record_launch(&NewLaunch {
                slug: task.slug.clone(),
                handle: handle.into(),
                role: role.into(),
                claude_session: stored_session(pane),
                worktree: if cwd.is_empty() {
                    board.worktree.clone()
                } else {
                    cwd.clone()
                },
                branch: board.branch.clone(),
                pane: text(pane.get("pane_id")),
                model: board.model.clone(),
                how: "found".into(),
                ..NewLaunch::default()
            })?);
        }
    }
    for s in mine.iter().filter(|s| s.closed.is_none()) {
        store.close_task_session(&s.slug, handle, why, &restore_line(&s.slug, handle))?;
    }
    Ok(())
}

/// `slug` became done or cancelled: close the panes of the sessions launched on it or below it (an
/// agent works a subtask, and closes it from its pane). Never fails: it runs after a status change
/// that must stand. The lines to print; what went wrong is one of them.
pub fn finish(
    store: &Store,
    herdr: &dyn Herdr,
    processes: &Processes,
    slug: &str,
    current_pane: Option<&str>,
) -> Vec<String> {
    let closures = || -> Result<Vec<Closure>, Error> {
        if !FINISHED.contains(&store.get(slug)?.status.as_str()) {
            return Ok(Vec::new());
        }
        let mut slugs = vec![slug.to_string()];
        slugs.extend(store.descendants(slug)?.into_iter().map(|t| t.slug));
        let mut sessions = Vec::new();
        for x in &slugs {
            sessions.extend(store.task_sessions(x)?);
        }
        if !sessions.iter().any(|s| s.closed.is_none()) {
            return Ok(Vec::new());
        }
        cleanup(store, herdr, processes, false, current_pane, &slugs)
    };
    match closures() {
        Err(problem) => vec![format!(
            "could not clean up the sessions of {slug}: {problem}"
        )],
        Ok(closures) => closures
            .iter()
            .filter_map(|c| {
                if c.kept {
                    let of = c
                        .handle
                        .as_ref()
                        .map_or(String::new(), |h| format!(" of {h}"));
                    Some(format!("kept pane {}{of} ({})", c.pane, c.why))
                } else if c.closed {
                    Some(format!(
                        "closed pane {} of {} ({}); restore: {}",
                        c.pane,
                        c.handle.as_deref().unwrap_or("None"),
                        c.why,
                        c.restore
                    ))
                } else {
                    None
                }
            })
            .collect(),
    }
}
