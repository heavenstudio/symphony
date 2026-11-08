//! The close of a live terminal whose idle wait is over: the last looks that confirm it
//! ([`still_idle`], then [`still_finished`] right before the kill), the record on its task thread
//! first ([`record`]), and the sessions opened again when the close does not happen.

use super::work::{HOUR, Hold, holds};
use super::{CHANGED, Closed, Due, NOT_REAPED_KINDS, Reap, Round, WHY, name, ready};
use crate::launch::cleanup::{restore_line, stored_session};
use crate::launch::hc::{self, Hc, Object, text_of};
use crate::launch::procs;
use crate::launch::screen::{self, NotClosed};
use crate::{Error, NewLaunch, Store};
use std::collections::HashSet;

impl Round<'_> {
    /// Close a terminal whose idle wait is over, once the last looks confirm it: None, or why it
    /// stays after all.
    pub(super) fn close(&mut self, due: Due) -> Result<Option<String>, Error> {
        let Due {
            address,
            handle,
            pid,
            key,
            hold,
        } = due;
        let (store, hc, how) = (self.store, self.hc, self.how);
        let Some(confirmed) = still_idle(store, hc, how, &address, &handle, pid, hold.as_ref())?
        else {
            self.idle_now.remove(&key);
            return Ok(Some(CHANGED.into()));
        };
        let why = hold
            .as_ref()
            .and_then(Hold::closing)
            .unwrap_or_else(|| WHY.into());
        if !how.apply {
            self.result.closed.push(Closed {
                pane: address,
                handle,
                why,
                restore: String::new(),
            });
            return Ok(None);
        }
        let closed_before: HashSet<String> = store
            .sessions_of(&handle)?
            .into_iter()
            .filter(|s| s.closed.is_some())
            .map(|s| s.slug)
            .collect();
        // first: a close is never left without its line
        let restore = match record(store, &handle, &confirmed, &why) {
            Ok(restore) => restore,
            Err(problem) => {
                let _ = writeln!(
                    self.warn,
                    "· idle sweep kept {address} ({handle}): the board took no record: {problem}"
                );
                let _ = self.warn.flush();
                return Ok(Some(format!(
                    "the board took no record of its close: {problem}"
                )));
            }
        };
        let finished = |now: &Object| still_finished(hc, how, &address, pid, now);
        match screen::close_unseen(hc, &address, &finished) {
            Ok(()) => {}
            Err(NotClosed::Refused(problem)) => {
                reopen(store, &handle, &closed_before, &address)?;
                return Ok(Some(format!("hc refused to close it: {problem}")));
            }
            Err(NotClosed::Kept(kept)) => {
                // Work resumed, the owner looked, or a call could not say, after the record: its
                // idle wait starts again.
                reopen(store, &handle, &closed_before, &address)?;
                self.idle_now.remove(&key);
                return Ok(Some(kept));
            }
        }
        if let Some(callback) = self.callbacks.get(&handle) {
            let said = if hold.is_some() {
                "idle terminal closed after an hour"
            } else {
                "finished idle terminal closed"
            };
            store.clear_callback(callback, said)?;
        }
        self.idle_now.remove(&key);
        self.result.closed.push(Closed {
            pane: address,
            handle,
            why,
            restore,
        });
        Ok(None)
    }
}

/// Confirm the same program and owner right before closing, then recheck the confirmed occupant's
/// eligibility: the same `hold` on it as the sweep found (none, or the same role). Any missing or
/// changed evidence keeps the terminal and resets its idle wait.
fn still_idle(
    store: &Store,
    hc: &dyn Hc,
    how: &Reap<'_>,
    address: &str,
    handle: &str,
    pid: i64,
    hold: Option<&Hold>,
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
        || holds(store)?.get(&current_handle) != hold
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

/// One line on a task thread for the close, saying `why`: the restore line, or "" when the handle
/// had no task.
fn record(store: &Store, handle: &str, terminal: &Object, why: &str) -> Result<String, Error> {
    let sessions = store.sessions_of(handle)?;
    let still_open: Vec<_> = sessions.iter().filter(|s| s.closed.is_none()).collect();
    if let Some(last) = still_open.last() {
        for s in &still_open {
            store.close_task_session(&s.slug, handle, why, &restore_line(&s.slug, handle))?;
        }
        return Ok(restore_line(&last.slug, handle));
    }
    let address = text_of(terminal, "address");
    if let Some(recorded) = sessions.iter().rev().find(|s| {
        s.closed.is_some()
            && swept(&s.closed_why)
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
    store.close_task_session(&slug, handle, why, &restore_line(&slug, handle))?;
    Ok(restore_line(&slug, handle))
}

/// Whether a record's close was an idle sweep's: its work finished, or idle an hour holding a role.
fn swept(why: &str) -> bool {
    why == WHY || why.starts_with(HOUR)
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
