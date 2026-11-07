//! `just tasks close SLUG HANDLE`: close one agent's terminal on its orchestrator's word, whatever
//! it holds (a reviewer waiting for a new tip, which cleanup and the idle sweep keep), and record the
//! closure on the task with the line that restores it.
//!
//! The terminal is the one the handle's open record on SLUG names, else the live terminal of this
//! board the handle runs in (its `sf.handle` label, else its name). Only this board's terminals
//! (`sf.board`) close, and only one whose `sf.handle` label, when it has one, is the handle's. The
//! close is refused for the caller's own session or terminal (`HC_TERMINAL_ID`), for a terminal
//! running a verify, a test run, a land or the Symphony ([`procs::protected`], read over the program
//! it started and every process under it), and for one the owner may be looking at ([`screen`], the
//! rule cleanup keeps); a call that cannot say refuses as well. The last look before the kill
//! ([`screen::close_unseen`]) must still find the same program, no such work started in it, and the
//! terminal out of the owner's sight. A recorded terminal hc no longer runs (or a herdr pane id) is
//! recorded closed as gone, with its restore line, and nothing is killed.
//!
//! The closure goes on every open record of the handle in that terminal, and on SLUG as a session
//! found there when SLUG has no open record of it. The handle's native callback is retired: what is
//! said to it waits in its inbox until `just tasks restore` brings it back.

use super::cleanup::{restore_line, stored_session};
use super::hc::{self, BOARD_LABEL, HANDLE_LABEL, Hc, Object, text_of};
use super::screen::{self, NotClosed};
use super::{Closure, GONE, LaunchError, Processes, procs};
use crate::{Error, NewLaunch, Store};

/// Close `handle`'s terminal for `actor`, recording it on `slug` (the module says when it is
/// refused): the closure, `closed`, with its restore line.
pub fn close(
    store: &Store,
    hc: &dyn Hc,
    processes: &Processes,
    actor: &str,
    slug: &str,
    handle: &str,
) -> Result<Closure, Error> {
    if handle == actor {
        return Err(refused(format!(
            "{handle} is you: a session never closes its own terminal"
        )));
    }
    let task = store.get(slug)?;
    let session = store.session(handle)?.row;
    let open: Vec<_> = store
        .sessions_of(handle)?
        .into_iter()
        .filter(|s| s.closed.is_none())
        .collect();
    let recorded = open.iter().find(|s| s.slug == slug && !s.pane.is_empty());
    let past = store
        .task_sessions(slug)?
        .into_iter()
        .find(|s| s.handle == handle);
    let role = if task.holders("reviewer").contains(&handle) {
        "reviewer".to_string()
    } else if task.holders("executor").contains(&handle) {
        "executor".to_string()
    } else if let Some(past) = &past {
        past.role.clone()
    } else {
        return Err(refused(format!(
            "{handle} has no recorded session or role on {slug}"
        )));
    };
    let restore = restore_line(slug, handle);
    let terminal = match recorded {
        Some(s) => match found(hc, &s.pane)? {
            Some(terminal) => terminal,
            None => {
                // Closed outside the board: said, with its restore line; nothing left to kill.
                for gone in open.iter().filter(|o| o.pane == s.pane) {
                    let line = restore_line(&gone.slug, handle);
                    store.close_task_session(&gone.slug, handle, GONE, &line)?;
                }
                return Ok(closure(&s.pane, handle, GONE, restore));
            }
        },
        None => running(hc, handle)?.ok_or_else(|| {
            refused(format!(
                "{handle} has no open session on {slug} and runs in no terminal of this board"
            ))
        })?,
    };
    let address = text_of(&terminal, "address");
    let board = hc::label(&terminal, BOARD_LABEL);
    if board != hc.board() {
        return Err(refused(format!(
            "terminal {address} is not this board's ({}): the board closes only its own",
            if board.is_empty() { "no board" } else { &board }
        )));
    }
    let labelled = hc::label(&terminal, HANDLE_LABEL);
    if !labelled.is_empty() && labelled != handle {
        return Err(refused(format!(
            "terminal {address} runs {labelled}, not {handle}"
        )));
    }
    if hc.caller().as_deref() == Some(address.as_str()) {
        return Err(refused(format!(
            "terminal {address} is the one you run in: a session never closes its own terminal"
        )));
    }
    let live = text_of(&terminal, "state") == "live";
    let pid = if live {
        let Some(pid) = hc::top_pid(&terminal) else {
            return Err(refused(format!(
                "terminal {address} stays: hc knows no pid for it, so what runs there cannot be read"
            )));
        };
        work(&terminal, processes, pid)
            .map_err(|why| refused(format!("terminal {address} stays: {why}")))?;
        Some(pid)
    } else {
        None
    };
    match screen::on_screen(hc, &terminal) {
        Ok(false) => {}
        Ok(true) => {
            return Err(refused(format!(
                "terminal {address} stays: {}",
                screen::ON_SCREEN
            )));
        }
        Err(problem) => {
            return Err(refused(format!(
                "terminal {address} stays: {}",
                screen::no_information(&problem)
            )));
        }
    }
    let finished = |now: &Object| still_closable(now, processes, pid);
    match screen::close_unseen(hc, &address, &finished) {
        Ok(()) => {}
        Err(NotClosed::Kept(why)) => {
            return Err(refused(format!("terminal {address} stays: {why}")));
        }
        Err(NotClosed::Refused(problem)) => return Err(problem.into()),
    }
    let why = format!("closed by {actor}");
    let mut on_slug = false;
    for s in open.iter().filter(|s| s.pane == address) {
        store.close_task_session(&s.slug, handle, &why, &restore_line(&s.slug, handle))?;
        on_slug |= s.slug == slug;
    }
    if !on_slug {
        let cwd = text_of(&terminal, "cwd");
        store.record_launch(&NewLaunch {
            slug: slug.into(),
            handle: handle.into(),
            role,
            claude_session: stored_session(&terminal),
            worktree: if cwd.is_empty() {
                session.worktree.clone()
            } else {
                cwd
            },
            branch: session.branch.clone(),
            pane: address.clone(),
            model: session.model.clone(),
            how: "found".into(),
            log: false,
        })?;
        store.close_task_session(slug, handle, &why, &restore)?;
    }
    if let Some(callback) = store.callbacks()?.get(handle) {
        store.clear_callback(callback, &format!("its terminal was {why}"))?;
    }
    Ok(closure(&address, handle, &why, restore))
}

fn refused(why: String) -> Error {
    Error::Refused(why)
}

fn closure(pane: &str, handle: &str, why: &str, restore: String) -> Closure {
    Closure {
        pane: pane.into(),
        handle: Some(handle.into()),
        why: why.into(),
        restore,
        closed: true,
        kept: false,
    }
}

/// The recorded terminal as `hc get` shows it; None when it is gone: a herdr pane id (herdr is no
/// more), or a terminal hc no longer knows.
fn found(hc: &dyn Hc, pane: &str) -> Result<Option<Object>, LaunchError> {
    if pane.contains(':') {
        return Ok(None);
    }
    hc.get(pane)
}

/// The live terminal of this board `handle` runs in, by its `sf.handle` label, else its name.
fn running(hc: &dyn Hc, handle: &str) -> Result<Option<Object>, LaunchError> {
    let board = format!("{BOARD_LABEL}={}", hc.board());
    let labelled = hc.listed(&[board.clone(), format!("{HANDLE_LABEL}={handle}")], false)?;
    let named = hc.listed(&[board], false)?;
    let address = labelled
        .iter()
        .chain(named.iter().filter(|t| text_of(t, "name") == handle))
        .map(|t| text_of(t, "address"))
        .find(|a| !a.is_empty());
    match address {
        Some(address) => hc.get(&address),
        None => Ok(None),
    }
}

/// Why the live terminal must stay, read over the program it started and every process under
/// `pid`: a verify, a test run, a land or the Symphony there, or a process table that cannot say.
fn work(terminal: &Object, processes: &Processes, pid: i64) -> Result<(), String> {
    let mut commands = vec![hc::argv(terminal).join(" ")];
    commands.extend(processes(pid).map_err(|problem| screen::no_information(&problem))?);
    match procs::protected(&commands) {
        Some(why) => Err(format!("it {why}")),
        None => Ok(()),
    }
}

/// Whether the terminal, as the close's own `hc get` finds it (`now`), still closes: its program
/// ended, or the same program (`pid`) runs with no verify, test run, land or Symphony started in it.
/// Else why it stays.
fn still_closable(now: &Object, processes: &Processes, pid: Option<i64>) -> Result<(), String> {
    if text_of(now, "state") != "live" {
        return Ok(());
    }
    match pid.filter(|pid| hc::top_pid(now) == Some(*pid)) {
        Some(pid) => work(now, processes, pid),
        None => Err("another program runs in it now".into()),
    }
}
