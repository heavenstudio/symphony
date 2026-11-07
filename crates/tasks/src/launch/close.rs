//! `just tasks close SLUG HANDLE`: close one agent's terminal on its orchestrator's word, whatever
//! role it holds (a reviewer waiting for a new tip, which cleanup and the idle sweep keep), and record
//! the closure on the task with the line that restores it.
//!
//! The terminal is the one the handle's open record on SLUG names, else the live terminal of this
//! board the handle runs in (its `sf.handle` label, else its name). Only this board's terminals
//! (`sf.board`) close, and only one whose `sf.handle` label, when it has one, is the handle's. The
//! close is refused for the caller's own session or terminal (`HC_TERMINAL_ID`), for one the owner
//! may be looking at ([`screen`], the rule cleanup keeps), and for a live one whose program is not at
//! rest: hc knows no pid for it, its foreground is not its agent (or, with no agent, a shell) as
//! cleanup reads it ([`foreground_idle`]), or anything runs under it that the idle sweep counts as
//! work ([`procs::reap_busy_reason`]: a verify, a test run of any kind, a land, the Symphony, a
//! shell, any program but the agent's helpers and board commands). A call that cannot say refuses
//! as well.
//!
//! A close holds the session for its whole run ([`Store::take_session`]), as a restore does, so
//! neither starts while the other runs, nor a second close; one stopped part way holds it until its
//! lease lapses. Every write it makes carries that hold, and none lands once another close or
//! restore took the session over: a close that lost its hold after its kill records nothing more,
//! and the next close or restore finds the terminal gone. The last look before the kill
//! ([`screen::close_unseen`]) must find the program judged ([`Running`]), still this board's and the
//! handle's, no work under it, the terminal out of the owner's sight and the session still held.
//! Only once the terminal is killed is the closure written, bound to the record read (its launch and
//! its terminal), so a close kept or refused writes nothing, and a callback the agent registered
//! before the kill completes the record's thread. Then the handle's callback is retired if it was
//! registered from that terminal, or is the very registration read before the close (one from no
//! terminal); never one registered from another. What is said to it waits in its inbox until
//! `just tasks restore` brings it back.
//!
//! A recorded terminal hc no longer runs (or a herdr pane id) is recorded closed as gone, with its
//! restore line, and nothing is killed; one a close recorded closed already (stopped before it
//! retired the callback) writes nothing more. Either way the callback is retired by the same rule.

use super::cleanup::{Judged, Running, foreground_idle, restore_line, stored_session};
use super::hc::{self, BOARD_LABEL, HANDLE_LABEL, Hc, Object, text_of};
use super::screen::{self, NotClosed};
use super::{Closure, GONE, LaunchError, Processes, procs};
use crate::rows::{SessionRow, TaskSessionRow};
use crate::{Callback, Error, NewLaunch, Store, Task};
use serde_json::Value;

/// How a close's reason starts: `closed by ACTOR`.
const CLOSED_BY: &str = "closed by ";

/// Who closes which session's terminal, recorded on which task, why, and the session's hold.
struct Ask<'a> {
    store: &'a Store,
    hc: &'a dyn Hc,
    processes: &'a Processes,
    slug: &'a str,
    handle: &'a str,
    why: String,
    hold: u64,
}

/// The terminal a close works on, as the board knows it.
enum Target {
    /// The open record of the session on the task names it.
    Recorded(TaskSessionRow),
    /// The handle runs in it, and no open record on the task names it.
    Found,
    /// A close recorded the session closed, and no terminal of the handle runs.
    Closed(TaskSessionRow),
}

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
    let board = store.session(handle)?.row;
    let hold = store.take_session(handle, &format!("a close by {actor}"))?;
    let ask = Ask {
        store,
        hc,
        processes,
        slug,
        handle,
        why: format!("{CLOSED_BY}{actor}"),
        hold,
    };
    let closed = holding(&ask, &task, board);
    let released = store.release_session(handle, hold);
    let closure = closed?;
    released?;
    Ok(closure)
}

/// The close, while it holds the session.
fn holding(ask: &Ask<'_>, task: &Task, board: SessionRow) -> Result<Closure, Error> {
    let mine = ask.store.sessions_of(ask.handle)?;
    let role = role(task, &mine, ask.slug, ask.handle)?;
    let (target, terminal) = target(ask, &mine)?;
    let Some(terminal) = terminal else {
        return gone(ask, &target, &mine);
    };
    let address = text_of(&terminal, "address");
    let judged = judge(ask, &terminal)?;
    let read = ask.store.callbacks()?.remove(ask.handle);
    let finished = |now: &Object| still_closable(ask, now, &judged);
    match screen::close_unseen(ask.hc, &address, &finished) {
        Ok(()) => {}
        Err(NotClosed::Kept(kept)) => {
            return Err(refused(format!("terminal {address} stays: {kept}")));
        }
        Err(NotClosed::Refused(problem)) => return Err(problem.into()),
    }
    let written = (|| {
        record(ask, &mine, &address)?;
        if let Target::Found = target {
            record_found(ask, &terminal, role, board)?;
        }
        retire(ask, &address, read.as_ref(), &ask.why)
    })();
    match written {
        Ok(()) => Ok(closure(&address, ask, &ask.why)),
        Err(Error::Refused(why)) => Err(refused(format!(
            "terminal {address} is killed, but {why}, so this close records nothing more: the next \
             close or restore of {} finds it gone",
            ask.handle
        ))),
        Err(problem) => Err(problem),
    }
}

fn refused(why: String) -> Error {
    Error::Refused(why)
}

fn closure(pane: &str, ask: &Ask<'_>, why: &str) -> Closure {
    Closure {
        pane: pane.into(),
        handle: Some(ask.handle.into()),
        why: why.into(),
        restore: restore_line(ask.slug, ask.handle),
        closed: true,
        kept: false,
    }
}

/// The role `handle` holds on `task`, else the one its last record there had; a handle with
/// neither is refused: there is nothing to record the closure on.
fn role(task: &Task, mine: &[TaskSessionRow], slug: &str, handle: &str) -> Result<String, Error> {
    let held = ["reviewer", "executor"]
        .into_iter()
        .find(|role| task.holders(role).contains(&handle));
    let recorded = mine.iter().rev().find(|record| record.slug == slug);
    match (held, recorded) {
        (Some(role), _) => Ok(role.into()),
        (None, Some(record)) => Ok(record.role.clone()),
        (None, None) => Err(refused(format!(
            "{handle} has no recorded session or role on {slug}"
        ))),
    }
}

/// The terminal to close, and what `hc get` shows of it (None: it is gone).
fn target(ask: &Ask<'_>, mine: &[TaskSessionRow]) -> Result<(Target, Option<Object>), Error> {
    let recorded = mine
        .iter()
        .find(|record| record.slug == ask.slug && !record.pane.is_empty());
    if let Some(record) = recorded
        && record.closed.is_none()
    {
        let terminal = found(ask.hc, &record.pane)?;
        return Ok((Target::Recorded(record.clone()), terminal));
    }
    if let Some(address) = running(ask.hc, ask.handle)? {
        return Ok((Target::Found, found(ask.hc, &address)?));
    }
    match recorded {
        Some(record) if record.closed_why.starts_with(CLOSED_BY) => {
            Ok((Target::Closed(record.clone()), None))
        }
        _ => Err(nowhere(ask)),
    }
}

fn nowhere(ask: &Ask<'_>) -> Error {
    refused(format!(
        "{} has no open session on {} and runs in no terminal of this board",
        ask.handle, ask.slug
    ))
}

/// The terminal is gone: a recorded one closed outside the board is recorded closed, with its
/// restore line; one a close recorded already needs no other line. Its callback is retired.
fn gone(ask: &Ask<'_>, target: &Target, mine: &[TaskSessionRow]) -> Result<Closure, Error> {
    let (pane, why) = match target {
        Target::Found => return Err(nowhere(ask)),
        Target::Closed(record) => (&record.pane, record.closed_why.as_str()),
        Target::Recorded(record) => (&record.pane, GONE),
    };
    let read = ask.store.callbacks()?.remove(ask.handle);
    let open_there = mine
        .iter()
        .filter(|record| record.closed.is_none() && &record.pane == pane);
    for record in open_there {
        let restore = restore_line(&record.slug, ask.handle);
        ask.store
            .close_task_session_as(record, GONE, &restore, ask.hold)?;
    }
    retire(ask, pane, read.as_ref(), why)?;
    Ok(closure(pane, ask, why))
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
fn running(hc: &dyn Hc, handle: &str) -> Result<Option<String>, LaunchError> {
    let board = format!("{BOARD_LABEL}={}", hc.board());
    let labelled = hc.listed(&[board.clone(), format!("{HANDLE_LABEL}={handle}")], false)?;
    let named = hc.listed(&[board], false)?;
    let by_name = named
        .iter()
        .filter(|terminal| text_of(terminal, "name") == handle);
    Ok(labelled
        .iter()
        .chain(by_name)
        .map(|terminal| text_of(terminal, "address"))
        .find(|address| !address.is_empty()))
}

/// What the close judges closable in `terminal`, at the first look; else it is refused.
fn judge(ask: &Ask<'_>, terminal: &Object) -> Result<Judged, Error> {
    let address = text_of(terminal, "address");
    let stays = |why: String| refused(format!("terminal {address} stays: {why}"));
    owned(ask, terminal).map_err(stays)?;
    if ask.hc.caller().as_deref() == Some(address.as_str()) {
        return Err(refused(format!(
            "terminal {address} is the one you run in: a session never closes its own terminal"
        )));
    }
    let judged = at_rest(terminal, ask.processes).map_err(stays)?;
    match screen::on_screen(ask.hc, terminal) {
        Ok(false) => Ok(judged),
        Ok(true) => Err(stays(screen::ON_SCREEN.into())),
        Err(problem) => Err(stays(screen::no_information(&problem))),
    }
}

/// Whether `terminal` is this board's, and the handle's when it is labelled for one; else why not.
fn owned(ask: &Ask<'_>, terminal: &Object) -> Result<(), String> {
    let board = hc::label(terminal, BOARD_LABEL);
    if board != ask.hc.board() {
        let board = if board.is_empty() { "no board" } else { &board };
        return Err(format!(
            "it is not this board's ({board}): the board closes only its own"
        ));
    }
    let labelled = hc::label(terminal, HANDLE_LABEL);
    if !labelled.is_empty() && labelled != ask.handle {
        return Err(format!("it runs {labelled}, not {}", ask.handle));
    }
    Ok(())
}

/// What runs in `terminal`, when it may close: its program ended, or the live program is at rest
/// (cleanup's foreground rule, the idle sweep's rule for what runs under an agent). Else why not.
fn at_rest(terminal: &Object, processes: &Processes) -> Result<Judged, String> {
    if text_of(terminal, "state") != "live" {
        return Ok(Judged::Ended);
    }
    let Some(pid) = hc::top_pid(terminal) else {
        return Err("hc knows no pid for it, so what runs there cannot be read".into());
    };
    if !foreground_idle(terminal, hc::agent(terminal).is_some()) {
        let shown = terminal
            .get("foreground")
            .and_then(|shown| shown.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(if shown.is_empty() {
            "hc shows nothing in its foreground, so it is not known to be at rest".into()
        } else {
            format!("{shown} holds its foreground, not its agent or a shell at rest")
        });
    }
    idle_under(processes, pid)?;
    Ok(Judged::Live(Running::of(terminal)))
}

/// Whether nothing under `pid` is work, as the idle sweep reads an agent's processes; else why not.
fn idle_under(processes: &Processes, pid: i64) -> Result<(), String> {
    let commands = processes(pid).map_err(|problem| screen::no_information(&problem))?;
    match procs::reap_busy_reason(&commands) {
        Some(why) => Err(format!("it {why}")),
        None => Ok(()),
    }
}

/// Whether the terminal, as the close's own `hc get` finds it (`now`), still closes: still this
/// board's and the handle's, the session still this close's to hold (held anew for the kill), and
/// its program ended, or the program `judged` still runs there at rest. Else why it stays.
fn still_closable(ask: &Ask<'_>, now: &Object, judged: &Judged) -> Result<(), String> {
    owned(ask, now)?;
    let held = ask
        .store
        .renew_session(ask.handle, ask.hold)
        .map_err(|problem| format!("the board cannot say it still holds its session: {problem}"))?;
    if !held {
        return Err("another close or a restore took its session over".into());
    }
    if text_of(now, "state") != "live" {
        return Ok(());
    }
    let Judged::Live(judged) = judged else {
        return Err("its program runs again".into());
    };
    idle_under(ask.processes, judged.still(now)?)
}

/// Record the closure on every open record of the handle in `terminal`, now killed, each only while
/// it is still the one read.
fn record(ask: &Ask<'_>, mine: &[TaskSessionRow], terminal: &str) -> Result<(), Error> {
    let open_there = mine
        .iter()
        .filter(|record| record.closed.is_none() && record.pane == terminal);
    for record in open_there {
        let restore = restore_line(&record.slug, ask.handle);
        ask.store
            .close_task_session_as(record, &ask.why, &restore, ask.hold)?;
    }
    Ok(())
}

/// Record the session found in `terminal`, now killed, on the task, closed there in one line,
/// unless the record of the task names that terminal already.
fn record_found(
    ask: &Ask<'_>,
    terminal: &Object,
    role: String,
    board: SessionRow,
) -> Result<(), Error> {
    let cwd = text_of(terminal, "cwd");
    let launch = NewLaunch {
        slug: ask.slug.into(),
        handle: ask.handle.into(),
        role,
        claude_session: stored_session(terminal),
        worktree: if cwd.is_empty() { board.worktree } else { cwd },
        branch: board.branch,
        pane: text_of(terminal, "address"),
        model: board.model,
        how: "found".into(),
        log: false,
    };
    let restore = restore_line(ask.slug, ask.handle);
    ask.store
        .record_found_closed(&launch, &ask.why, &restore, ask.hold)
        .map(|_| ())
}

/// Retire the handle's callback: the registration `read` before the close from no terminal, only
/// as it was read, and the one registered now from `terminal` (the one closed); never one
/// registered from another terminal, nor one from none registered since.
fn retire(ask: &Ask<'_>, terminal: &str, read: Option<&Callback>, why: &str) -> Result<(), Error> {
    let said = format!("its terminal {terminal} closed ({why})");
    if let Some(unplaced) = read.filter(|read| read.terminal.is_none()) {
        ask.store.clear_callback_held(unplaced, &said, ask.hold)?;
    }
    if let Some(now) = ask.store.callbacks()?.remove(ask.handle)
        && now.terminal.as_deref() == Some(terminal)
    {
        ask.store.clear_callback_held(&now, &said, ask.hold)?;
    }
    Ok(())
}
