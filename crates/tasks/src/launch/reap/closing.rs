//! The close of a live terminal whose idle wait is over: the last looks that confirm it
//! ([`still_idle`], then [`still_finished`] right before the kill, each reading hc and the board
//! again), the record on its task thread first ([`record`]), and the sessions opened again when
//! the close does not happen.

use super::work::{HOUR, Hold, Holds};
use super::{CHANGED, Closed, Due, NOT_REAPED_KINDS, Reap, Round, TOOL_CALL, WHY, name, ready};
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
            beat,
        } = due;
        let (store, hc, how) = (self.store, self.hc, self.how);
        let judged = Judged {
            handle: &handle,
            pid,
            hold: hold.as_ref(),
            beat,
        };
        let confirmed = match still_idle(store, hc, how, &address, &judged)? {
            Ok(confirmed) => confirmed,
            Err(why) => {
                self.idle_now.remove(&key);
                return Ok(Some(why));
            }
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
        let finished = |now: &Object| still_finished(store, hc, how, &address, &judged, now);
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
        crate::launch::attempts::closed(store, hc, &handle, &address, &why)?;
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

/// What the sweep judged a terminal's agent by: its session `handle`, its program's `pid`, what held
/// it (`hold`: none, or a role) and when its last tool call was (`beat`), as the sweep read them.
struct Judged<'a> {
    handle: &'a str,
    pid: i64,
    hold: Option<&'a Hold>,
    beat: Option<f64>,
}

/// Confirm the same program and owner right before closing, then recheck the confirmed occupant's
/// eligibility on the board ([`board_still`]). Any missing or changed evidence keeps the terminal,
/// saying why, and resets its idle wait.
fn still_idle(
    store: &Store,
    hc: &dyn Hc,
    how: &Reap<'_>,
    address: &str,
    judged: &Judged<'_>,
) -> Result<Result<Object, String>, Error> {
    let changed = || Ok(Err(CHANGED.to_string()));
    let Ok(Some(now)) = hc.get(address) else {
        return changed();
    };
    let Some(current_handle) = name(&now) else {
        return changed();
    };
    if text_of(&now, "state") != "live"
        || hc::top_pid(&now) != Some(judged.pid)
        || hc::agent(&now).is_none()
        || current_handle != judged.handle
        || Some(current_handle.as_str()) == how.caller
        || !ready(hc, address, &hc::status(&now))
    {
        return changed();
    }
    // A board that cannot say, its unanswered lookup out of time among them, keeps the terminal.
    if let Err(why) = board_still(store, judged).unwrap_or_else(|problem| Err(unread(&problem))) {
        return Ok(Err(why));
    }
    if !(how.processes)(judged.pid)
        .is_ok_and(|commands| procs::reap_busy_reason(&commands).is_none())
        || !matches!(screen::on_screen(hc, &now), Ok(false))
    {
        return changed();
    }
    Ok(Ok(now))
}

/// Whether the board still says of the agent what the sweep judged it by: a session of a kind the
/// sweep closes, no tool call of its agent since the one the sweep read, and the same hold on it
/// (none, or the same role: a post addressed to it since holds it). Else why it stays.
fn board_still(store: &Store, judged: &Judged<'_>) -> Result<Result<(), String>, Error> {
    let sessions = store.sessions()?;
    let Some(session) = sessions.iter().find(|s| s.row.handle == judged.handle) else {
        return Ok(Err(CHANGED.into()));
    };
    if NOT_REAPED_KINDS.contains(&session.row.kind.as_str()) {
        return Ok(Err(CHANGED.into()));
    }
    if session.row.last_tool != judged.beat {
        return Ok(Err(TOOL_CALL.into()));
    }
    Ok(match Holds::read(store)?.of(judged.handle)?.as_ref() {
        now if now == judged.hold => Ok(()),
        Some(post) if post.is_post() => Err(post.why()),
        _ => Err(CHANGED.into()),
    })
}

/// Whether the terminal, as the close's own `hc get` finds it (`now`), is still the idle agent the
/// sweep confirmed: its program ended (nothing to lose), or the same agent under its pid, ready
/// ([`ready`]), with nothing under it that is work, and the board saying of it what the sweep
/// judged it by ([`board_still`]): read here again, since a tool call or a post may come while the
/// close records itself. Else why it stays.
fn still_finished(
    store: &Store,
    hc: &dyn Hc,
    how: &Reap<'_>,
    address: &str,
    judged: &Judged<'_>,
    now: &Object,
) -> Result<(), String> {
    if text_of(now, "state") != "live" {
        return Ok(());
    }
    if hc::top_pid(now) != Some(judged.pid) || hc::agent(now).is_none() {
        return Err("another program runs in it now".into());
    }
    let status = hc::status(now);
    if !ready(hc, address, &status) {
        let status = if status.is_empty() { "silent" } else { &status };
        return Err(format!("its agent is {status} now"));
    }
    let commands =
        (how.processes)(judged.pid).map_err(|problem| screen::no_information(&problem))?;
    if let Some(why) = procs::reap_busy_reason(&commands) {
        return Err(format!("work started under it: {why}"));
    }
    board_still(store, judged).unwrap_or_else(|problem| Err(unread(&problem)))
}

/// Why a terminal stays whose board could not be read: `problem`, said as no information.
fn unread(problem: &Error) -> String {
    screen::no_information(&crate::launch::LaunchError(problem.to_string()))
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
    roles.sort_unstable_by(|a, b| {
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
    recorded == running
        || (running == crate::launch::CODEX && recorded.starts_with(crate::launch::CODEX))
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
