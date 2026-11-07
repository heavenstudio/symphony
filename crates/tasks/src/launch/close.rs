//! `just tasks close SLUG HANDLE`: close one agent's terminal on its orchestrator's word, whatever
//! role it holds (a reviewer waiting for a new tip, which cleanup and the idle sweep keep), and record
//! the closure on the task with the line that restores it.
//!
//! The terminal is the one the handle's record on SLUG names (open, or closed by a close that may
//! not have killed it yet), else the live terminal of this board the handle runs in (its `sf.handle`
//! label, else its name). Only this board's terminals (`sf.board`) close, and only one whose
//! `sf.handle` label, when it has one, is the handle's. The close is refused for the caller's own
//! session or terminal (`HC_TERMINAL_ID`), for one the owner may be looking at ([`screen`], the rule
//! cleanup keeps), and for a live one whose program is not at rest: hc knows no pid for it, its
//! foreground is not its agent (or, with no agent, a shell) as cleanup reads it
//! ([`foreground_idle`]), or anything runs under it that the idle sweep counts as work
//! ([`procs::reap_busy_reason`]: a verify, a test run of any kind, a land, the Symphony, a shell,
//! any program but the agent's helpers and board commands). A call that cannot say refuses as well.
//!
//! The closure is recorded first, bound to the record read (its launch and its terminal): it never
//! closes a session restored into another terminal meanwhile, and a second close of the same
//! terminal writes no second line. While it stands, restore refuses that session, whose terminal
//! still runs. Then the last look before the kill ([`screen::close_unseen`]) must find the program
//! judged ([`Running`]), still this board's and the handle's, no work under it, and the terminal out
//! of the owner's sight; else the session is open again, said on the thread. A close stopped after
//! its record is finished by the next close of that session, which writes no line. A recorded
//! terminal hc no longer runs (or a herdr pane id) is recorded closed as gone, with its restore
//! line, and nothing is killed. The handle's native callback registered from that terminal (or from
//! none) is retired, never one registered since from another: what is said to it waits in its inbox
//! until `just tasks restore` brings it back.

use super::cleanup::{Judged, Running, foreground_idle, restore_line, stored_session};
use super::hc::{self, BOARD_LABEL, HANDLE_LABEL, Hc, Object, text_of};
use super::screen::{self, NotClosed};
use super::{Closure, GONE, LaunchError, Processes, procs};
use crate::rows::{SessionRow, TaskSessionRow};
use crate::{Callback, Error, NewLaunch, Store, Task};
use serde_json::Value;

/// How a close's reason starts: `closed by ACTOR`.
const CLOSED_BY: &str = "closed by ";

/// Who closes which session's terminal, recorded on which task, and why.
struct Ask<'a> {
    store: &'a Store,
    hc: &'a dyn Hc,
    processes: &'a Processes,
    slug: &'a str,
    handle: &'a str,
    why: String,
}

/// The terminal a close works on, as the board knows it.
enum Target {
    /// The open record of the session on the task names it.
    Recorded(TaskSessionRow),
    /// A close recorded the session closed in it, and may have stopped before the kill.
    Pending(TaskSessionRow),
    /// The handle runs in it, and no record on the task names it.
    Found,
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
    let mine = store.sessions_of(handle)?;
    let role = role(&task, &mine, slug, handle)?;
    let ask = Ask {
        store,
        hc,
        processes,
        slug,
        handle,
        why: format!("{CLOSED_BY}{actor}"),
    };
    let (target, terminal) = target(&ask, &mine)?;
    let Some(terminal) = terminal else {
        return gone(&ask, &target, &mine);
    };
    let address = text_of(&terminal, "address");
    let judged = judge(&ask, &terminal)?;
    let callback = registered_from(store, handle, &address)?;
    let (why, written) = match target {
        Target::Pending(record) => (record.closed_why, Vec::new()),
        Target::Recorded(_) => (ask.why.clone(), record(&ask, &mine, &terminal)?),
        Target::Found => {
            let mut written = record(&ask, &mine, &terminal)?;
            written.extend(record_found(&ask, &terminal, role, board)?);
            (ask.why.clone(), written)
        }
    };
    let finished = |now: &Object| still_closable(&ask, now, &judged);
    match screen::close_unseen(hc, &address, &finished) {
        Ok(()) => {}
        Err(NotClosed::Kept(kept)) => {
            keep(&ask, &written, &kept)?;
            return Err(refused(format!("terminal {address} stays: {kept}")));
        }
        Err(NotClosed::Refused(problem)) => {
            keep(&ask, &written, &problem.0)?;
            return Err(problem.into());
        }
    }
    retire(&ask, callback, &address, &why)?;
    Ok(closure(&address, &ask, &why))
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
    let pending = match recorded {
        Some(record) if record.closed.is_none() => {
            let terminal = found(ask.hc, &record.pane)?;
            return Ok((Target::Recorded(record.clone()), terminal));
        }
        Some(record) if record.closed_why.starts_with(CLOSED_BY) => Some(record),
        _ => None,
    };
    if let Some(record) = pending
        && let Some(terminal) = found(ask.hc, &record.pane)?
    {
        return Ok((Target::Pending(record.clone()), Some(terminal)));
    }
    if let Some(address) = running(ask.hc, ask.handle)? {
        return Ok((Target::Found, found(ask.hc, &address)?));
    }
    match pending {
        Some(record) => Ok((Target::Pending(record.clone()), None)),
        None => Err(nowhere(ask)),
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
        Target::Pending(record) => (&record.pane, record.closed_why.as_str()),
        Target::Recorded(record) => (&record.pane, GONE),
    };
    let callback = registered_from(ask.store, ask.handle, pane)?;
    let open_there = mine
        .iter()
        .filter(|record| record.closed.is_none() && &record.pane == pane);
    for record in open_there {
        let restore = restore_line(&record.slug, ask.handle);
        ask.store.close_task_session_as(record, GONE, &restore)?;
    }
    retire(ask, callback, pane, why)?;
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
/// board's and the handle's, and its program ended, or the program `judged` still runs there at
/// rest. Else why it stays.
fn still_closable(ask: &Ask<'_>, now: &Object, judged: &Judged) -> Result<(), String> {
    owned(ask, now)?;
    if text_of(now, "state") != "live" {
        return Ok(());
    }
    let Judged::Live(judged) = judged else {
        return Err("its program runs again".into());
    };
    idle_under(ask.processes, judged.still(now)?)
}

/// Record the closure on every open record of the handle in `terminal`, each only while it is
/// still the one read: the records it closed.
fn record(
    ask: &Ask<'_>,
    mine: &[TaskSessionRow],
    terminal: &Object,
) -> Result<Vec<TaskSessionRow>, Error> {
    let address = text_of(terminal, "address");
    let open_there = mine
        .iter()
        .filter(|record| record.closed.is_none() && record.pane == address);
    let mut written = Vec::new();
    for record in open_there {
        let restore = restore_line(&record.slug, ask.handle);
        if ask
            .store
            .close_task_session_as(record, &ask.why, &restore)?
        {
            written.push(record.clone());
        }
    }
    Ok(written)
}

/// Record the session found in `terminal` on the task, closed there in one line, unless the record
/// of the task names that terminal already.
fn record_found(
    ask: &Ask<'_>,
    terminal: &Object,
    role: String,
    board: SessionRow,
) -> Result<Option<TaskSessionRow>, Error> {
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
    ask.store.record_found_closed(&launch, &ask.why, &restore)
}

/// The last look kept the terminal (`kept`): the records this close wrote are open again.
fn keep(ask: &Ask<'_>, written: &[TaskSessionRow], kept: &str) -> Result<(), Error> {
    for record in written {
        ask.store.keep_task_session(record, &ask.why, kept)?;
    }
    Ok(())
}

/// The handle's native callback, if it was registered from `terminal` or from no terminal hc knows.
fn registered_from(store: &Store, handle: &str, terminal: &str) -> Result<Option<Callback>, Error> {
    Ok(store.callbacks()?.remove(handle).filter(|callback| {
        callback
            .terminal
            .as_deref()
            .is_none_or(|from| from == terminal)
    }))
}

/// Retire `callback`, the registration read before the close, and no other.
fn retire(
    ask: &Ask<'_>,
    callback: Option<Callback>,
    terminal: &str,
    why: &str,
) -> Result<(), Error> {
    match callback {
        Some(callback) => ask.store.clear_callback(
            &callback,
            &format!("its terminal {terminal} closed ({why})"),
        ),
        None => Ok(()),
    }
}
