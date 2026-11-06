//! `just tasks cleanup`: close the terminals nobody needs (the module above says which), each
//! closure recorded on its task with the line that restores it; and [`finish`], the same for the
//! sessions of one task that just became done or cancelled.

use super::hc::{self, Hc, Object, text_of};
use super::procs::{busy_reason, named};
use super::screen::{self, NotClosed};
use super::{GONE, LaunchError, Processes, READY_STATES, gone};
use crate::health::FINISHED;
use crate::rows::TaskSessionRow;
use crate::{Error, NewLaunch, Store, Task};
use std::collections::{HashMap, HashSet};

/// A terminal cleanup closed, or for a dry run would close; a recorded pane that is gone, whose
/// closure cleanup recorded; or a terminal it kept (`kept`) because a call that says whether it is
/// idle failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub pane: String,
    /// The board session it belongs to; none for an idle shell, or a kept terminal of no known
    /// session.
    pub handle: Option<String>,
    /// Why it closes; for a kept terminal, why it stays: the failed call and its error.
    pub why: String,
    /// The line that brings it back; empty when there is no task to bring it back on, and for a
    /// kept terminal.
    pub restore: String,
    /// False for a dry run, when hc refused, and for a kept terminal.
    pub closed: bool,
    /// True for a terminal left as it is because a call that says whether it is idle failed (`why`
    /// names the call and its error), or because the owner may be looking at it: on screen in the
    /// Herdr app, or the window could not say ([`super::screen`]). Never closed on no information,
    /// and never silently. Its handle is the session the board would have closed it for; none when
    /// `hc get` failed, before the terminal's session was looked for.
    pub kept: bool,
}

impl Closure {
    /// `terminal` left as it is, of no known session: `problem`, the failed call's error, said in
    /// `why`.
    fn kept(terminal: &str, problem: &LaunchError) -> Closure {
        Closure::left(terminal, screen::no_information(problem))
    }

    /// `terminal` left as it is, of no known session, for the reason `why`.
    fn left(terminal: &str, why: String) -> Closure {
        Closure {
            pane: terminal.into(),
            handle: None,
            why,
            restore: String::new(),
            closed: false,
            kept: true,
        }
    }

    /// None when the owner is not looking at `terminal`; else it stays, of `handle`, saying why: on
    /// screen, or the window could not say.
    fn on_screen(hc: &dyn Hc, terminal: &Object, handle: Option<&str>) -> Option<Closure> {
        let address = text_of(terminal, "address");
        let left = match screen::on_screen(hc, terminal) {
            Ok(false) => return None,
            Ok(true) => Closure::left(&address, screen::ON_SCREEN.into()),
            Err(problem) => Closure::kept(&address, &problem),
        };
        Some(Closure {
            handle: handle.map(str::to_string),
            ..left
        })
    }
}

/// Close the terminal `closure` is about, the owner's look judged last ([`screen::close_unseen`]):
/// whether it closed, false when hc refused; or `closure` as it stays, saying why.
fn closed(hc: &dyn Hc, address: &str, closure: &Closure) -> Result<bool, Closure> {
    match screen::close_unseen(hc, address) {
        Ok(()) => Ok(true),
        Err(NotClosed::Refused(_)) => Ok(false),
        Err(NotClosed::Kept(why)) => Err(Closure {
            handle: closure.handle.clone(),
            ..Closure::left(address, why)
        }),
    }
}

pub(super) fn restore_line(slug: &str, handle: &str) -> String {
    format!("just tasks restore {slug} {handle}")
}

/// Close the terminals nobody needs; with `only` slugs, only those of those tasks' open sessions.
/// The caller's terminal is never closed. A record whose pane is gone is recorded closed first. A
/// terminal whose `hc get` or process table could not be read stays, as a [`Closure`] that is
/// `kept`, naming the call that failed; so does one the owner may be looking at ([`screen`]).
pub fn cleanup(
    store: &Store,
    hc: &dyn Hc,
    processes: &Processes,
    dry_run: bool,
    only: &[String],
) -> Result<Vec<Closure>, Error> {
    let current = hc.caller();
    let handles: HashSet<String> = store
        .sessions()?
        .into_iter()
        .map(|s| s.row.handle)
        .collect();
    let terminals = hc.terminals()?;
    let running: HashSet<String> = terminals
        .iter()
        .filter(|t| text_of(t, "state") == "live")
        .map(|t| text_of(t, "address"))
        .collect();
    let mut result = Vec::new();
    let open = store.open_task_sessions()?;
    for s in &open {
        if (!only.is_empty() && !only.contains(&s.slug)) || !gone(&s.pane, &running) {
            continue;
        }
        let restore = restore_line(&s.slug, &s.handle);
        if !dry_run {
            store.close_task_session(&s.slug, &s.handle, GONE, &restore)?;
        }
        result.push(Closure {
            pane: s.pane.clone(),
            handle: Some(s.handle.clone()),
            why: GONE.into(),
            restore,
            closed: !dry_run,
            kept: false,
        });
    }
    let by_pane: HashMap<&str, &str> = open
        .iter()
        .filter(|s| !s.pane.is_empty())
        .map(|s| (s.pane.as_str(), s.handle.as_str()))
        .collect();
    let by_session: HashMap<&str, &str> = open
        .iter()
        .filter(|s| !s.claude_session.is_empty() && s.claude_session != "codex:")
        .map(|s| (s.claude_session.as_str(), s.handle.as_str()))
        .collect();
    let wanted: Option<HashSet<&str>> = (!only.is_empty()).then(|| {
        open.iter()
            .filter(|s| only.contains(&s.slug))
            .map(|s| s.pane.as_str())
            .collect()
    });
    for listed in &terminals {
        let address = text_of(listed, "address");
        if address.is_empty()
            || Some(&address) == current.as_ref()
            || wanted
                .as_ref()
                .is_some_and(|w| !w.contains(address.as_str()))
        {
            continue;
        }
        let terminal = match hc.get(&address) {
            Ok(Some(terminal)) => terminal,
            Ok(None) => continue, // gone since it was listed: nothing to close
            Err(problem) => {
                result.push(Closure::kept(&address, &problem));
                continue;
            }
        };
        let ended = text_of(&terminal, "state") != "live";
        let is_agent = hc::agent(&terminal).is_some();
        let handle = handle_of(&terminal, store, &handles, &by_pane, &by_session)?;
        if ended {
            // Its program ended: nothing in it to lose, and any record of it was closed above.
            if let Some(kept) = Closure::on_screen(hc, &terminal, handle.as_deref()) {
                result.push(kept);
                continue;
            }
            let mut closure = Closure {
                pane: address.clone(),
                handle,
                why: "its program ended".into(),
                restore: String::new(),
                closed: false,
                kept: false,
            };
            if !dry_run {
                match closed(hc, &address, &closure) {
                    Ok(done) => closure.closed = done,
                    Err(kept) => closure = kept,
                }
            }
            result.push(closure);
            continue;
        }
        if is_agent && !READY_STATES.contains(&hc::status(&terminal).as_str()) {
            continue;
        }
        let Some(pid) = terminal
            .get("pid")
            .and_then(serde_json::Value::as_i64)
            .filter(|p| *p > 0)
        else {
            continue; // hc knows no pid: nothing to look under, not known to be idle
        };
        // What the board says first, so a table that cannot be read is said only of a terminal it
        // would otherwise close.
        let (handle, why, mine) = if is_agent {
            let Some(handle) = handle else {
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
        match processes(pid) {
            Ok(commands) if busy_reason(&commands).is_none() => {}
            Ok(_) => continue, // work
            Err(problem) => {
                result.push(Closure {
                    handle,
                    ..Closure::kept(&address, &problem)
                });
                continue;
            }
        }
        if let Some(kept) = Closure::on_screen(hc, &terminal, handle.as_deref()) {
            result.push(kept);
            continue;
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
            pane: address.clone(),
            handle: handle.clone(),
            why: why.into(),
            restore,
            closed: false,
            kept: false,
        };
        if !dry_run {
            match closed(hc, &address, &closure) {
                Ok(true) => closure.closed = true,
                Ok(false) => {
                    result.push(closure);
                    continue;
                }
                Err(kept) => {
                    result.push(kept);
                    continue;
                }
            }
            if let Some(handle) = &handle {
                record_closure(store, handle, &terminal, why)?;
            }
        }
        result.push(closure);
    }
    Ok(result)
}

/// The board session a terminal belongs to: its recorded terminal, its `sf.handle` label, the
/// conversation it runs (recorded, or the one an agent's beats are bound to), a `-n HANDLE` flag of
/// what runs in it, its name.
fn handle_of(
    terminal: &Object,
    store: &Store,
    handles: &HashSet<String>,
    by_pane: &HashMap<&str, &str>,
    by_session: &HashMap<&str, &str>,
) -> Result<Option<String>, Error> {
    if let Some(handle) = by_pane.get(text_of(terminal, "address").as_str()) {
        return Ok(Some(handle.to_string()));
    }
    let labelled = hc::label(terminal, hc::HANDLE_LABEL);
    if handles.contains(&labelled) {
        return Ok(Some(labelled));
    }
    let argv = hc::argv(terminal);
    if let Some(conversation) = hc::conversation(&argv) {
        if let Some(handle) = by_session.get(conversation.as_str()) {
            return Ok(Some(handle.to_string()));
        }
        let bare = conversation.strip_prefix("codex:").unwrap_or(&conversation);
        if let Some(bound) = store.handle_for_agent(bare)? {
            return Ok(Some(bound));
        }
    }
    if let Some(name) = named(&argv.join(" "))
        && handles.contains(name)
    {
        return Ok(Some(name.to_string()));
    }
    let name = text_of(terminal, "name");
    Ok(handles.contains(&name).then_some(name))
}

/// The conversation a terminal runs, as a record stores it (`codex:` and a thread for Codex), or "".
pub(super) fn stored_session(terminal: &Object) -> String {
    hc::conversation(&hc::argv(terminal)).unwrap_or_else(|| {
        if hc::agent(terminal) == Some("codex") {
            "codex:".into()
        } else {
            String::new()
        }
    })
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
fn record_closure(store: &Store, handle: &str, terminal: &Object, why: &str) -> Result<(), Error> {
    let mut mine: Vec<TaskSessionRow> = store.sessions_of(handle)?;
    if mine.is_empty() {
        let board = store.session(handle)?.row;
        let cwd = text_of(terminal, "cwd");
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
                claude_session: stored_session(terminal),
                worktree: if cwd.is_empty() {
                    board.worktree.clone()
                } else {
                    cwd.clone()
                },
                branch: board.branch.clone(),
                pane: text_of(terminal, "address"),
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

/// `slug` became done or cancelled: close the terminals of the sessions launched on it or below it
/// (an agent works a subtask, and closes it from its terminal). Never fails: it runs after a status
/// change that must stand. The lines to print; what went wrong is one of them.
pub fn finish(store: &Store, hc: &dyn Hc, processes: &Processes, slug: &str) -> Vec<String> {
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
        cleanup(store, hc, processes, false, &slugs)
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
