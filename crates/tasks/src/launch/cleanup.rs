//! `just tasks cleanup`: close the terminals nobody needs (the module above says which), each
//! closure recorded on its task with the line that restores it; and [`finish`], the same for the
//! sessions of one task that just became done or cancelled.

use super::hc::{self, Hc, Object, text, text_of};
use super::procs::{self, busy_reason, named};
use super::reap::NOT_REAPED_KINDS;
use super::screen::{self, NotClosed};
use super::{GONE, LaunchError, Processes, READY_STATES, attempts, gone};
use crate::rows::TaskSessionRow;
use crate::{Error, NewLaunch, Store, Task};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// A terminal cleanup closed, or for a dry run would close; a recorded pane that is gone, whose
/// closure cleanup recorded; or a terminal it kept (`kept`) because a call that says whether it is
/// idle failed, or a dry run naming its live reservation.
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub pane: String,
    /// The board session it belongs to; none for an idle shell, or a kept terminal of no known
    /// session.
    pub handle: Option<String>,
    /// Why it closes; for a kept terminal, the failed call, owner's look or live reservation.
    pub why: String,
    /// The line that brings it back; empty when there is no task to bring it back on, and for a
    /// kept terminal.
    pub restore: String,
    /// False for a dry run, when hc refused, and for a kept terminal.
    pub closed: bool,
    /// True for a terminal left as it is because a call that says whether it is idle failed (`why`
    /// names the call and its error), or because the owner may be looking at it: on screen in the
    /// Herdr app, or the window could not say ([`super::screen`]); also for a live reservation in
    /// a dry run. Never closed on no information,
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

/// Close the terminal `closure` is about as the last look before the kill finds it
/// ([`screen::close_unseen`]): still `finished`, then out of the owner's sight. Whether it closed,
/// false when hc refused; or `closure` as it stays, saying why.
fn closed(
    hc: &dyn Hc,
    address: &str,
    closure: &Closure,
    finished: &dyn Fn(&Object) -> Result<(), String>,
) -> Result<bool, Closure> {
    match screen::close_unseen(hc, address, finished) {
        Ok(()) => Ok(true),
        Err(NotClosed::Refused(_)) => Ok(false),
        Err(NotClosed::Kept(why)) => Err(Closure {
            handle: closure.handle.clone(),
            ..Closure::left(address, why)
        }),
    }
}

/// What runs in a live terminal by one `hc get`: the program it started (its pid), the program in
/// its foreground (pid, name and argv, or null) and the agent hc sees there. Cleanup (and `close`)
/// closes the terminal only while the close's own look finds the same, so its judgement is never
/// applied to another program: another instance, a shell `--keep` put in place of a lost one, or a
/// job a shell became by `exec` under its own pid.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Running {
    pid: Option<i64>,
    foreground: Value,
    agent: Option<&'static str>,
}

impl Running {
    pub(super) fn of(terminal: &Object) -> Running {
        let foreground = terminal.get("foreground").and_then(Value::as_object);
        Running {
            pid: hc::top_pid(terminal),
            foreground: foreground.map_or(Value::Null, |shown| {
                json!({"pid": shown.get("pid"), "name": shown.get("name"), "argv": shown.get("argv")})
            }),
            agent: hc::agent(terminal),
        }
    }

    /// The pid of what runs in the live terminal `now` (the close's own `hc get`) while it is still
    /// this; else why the terminal stays.
    pub(super) fn still(&self, now: &Object) -> Result<i64, String> {
        let Some(pid) = hc::top_pid(now) else {
            return Err("hc knows no pid for it now".into());
        };
        if Running::of(now) != *self {
            return Err("another program runs in it now".into());
        }
        Ok(pid)
    }
}

/// What a close judged closable: a terminal whose program had ended, or a live one running this.
pub(super) enum Judged {
    Ended,
    Live(Running),
}

/// Whether what holds a live terminal's foreground leaves it closable: the program the terminal
/// started (not a job a shell runs in front), and in a terminal with no agent, a shell. A program
/// is never an idle shell on its own, and the processes under a program never include it, so a job
/// a shell became by `exec` shows only here. Nothing hc shows there says nothing is idle.
pub(super) fn foreground_idle(terminal: &Object, is_agent: bool) -> bool {
    let Some(shown) = terminal.get("foreground").and_then(Value::as_object) else {
        return false;
    };
    if hc::pid(terminal) != hc::top_pid(terminal) {
        return false;
    }
    is_agent || procs::shell(&text(shown.get("name")))
}

/// Whether `terminal`, as the close's own `hc get` finds it, still closes, else why it stays. One
/// whose program has ended has nothing to lose. One cleanup judged ended stays if its program runs
/// again; a live one stays if hc knows no pid for it, if another program runs in it now
/// ([`Running`]), if its agent is no longer idle or done, or if what runs under it now is work
/// ([`busy_reason`]), or if that cannot be read.
fn still_finished(terminal: &Object, processes: &Processes, judged: &Judged) -> Result<(), String> {
    if text_of(terminal, "state") != "live" {
        return Ok(());
    }
    let Judged::Live(judged) = judged else {
        return Err("its program runs again".into());
    };
    let pid = judged.still(terminal)?;
    let status = hc::status(terminal);
    if judged.agent.is_some() && !READY_STATES.contains(&status.as_str()) {
        let status = if status.is_empty() { "silent" } else { &status };
        return Err(format!("its agent is {status} now"));
    }
    match processes(pid) {
        Ok(commands) => busy_reason(&commands)
            .map_or(Ok(()), |why| Err(format!("work started under it: {why}"))),
        Err(problem) => Err(screen::no_information(&problem)),
    }
}

pub(super) fn restore_line(slug: &str, handle: &str) -> String {
    format!("just tasks restore {slug} {handle}")
}

/// Close the terminals nobody needs; with `only` slugs, only those of those tasks' open sessions.
/// The caller's terminal is never closed, nor an orchestrator's or a chat's. A record whose pane is
/// gone is recorded closed first. A terminal whose `hc get` or process table could not be read
/// stays, as a [`Closure`] that is `kept`, naming the call that failed; so does one the owner may
/// be looking at ([`screen`]).
pub fn cleanup(
    store: &Store,
    hc: &dyn Hc,
    processes: &Processes,
    dry_run: bool,
    only: &[String],
) -> Result<Vec<Closure>, Error> {
    cleanup_as(store, hc, processes, dry_run, only, None)
}

/// [`cleanup`] for the session `caller`, whose own terminal is never closed either, wherever it is.
pub fn cleanup_as(
    store: &Store,
    hc: &dyn Hc,
    processes: &Processes,
    dry_run: bool,
    only: &[String],
    caller: Option<&str>,
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
            attempts::closed(store, hc, &s.handle, &s.pane, GONE)?;
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
        .filter(|s| super::resumable(&s.claude_session))
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
                match closed(hc, &address, &closure, &|t| {
                    still_finished(t, processes, &Judged::Ended)
                }) {
                    Ok(done) => closure.closed = done,
                    Err(kept) => closure = kept,
                }
                if let (true, Some(handle)) = (closure.closed, &closure.handle) {
                    attempts::closed(store, hc, handle, &address, &closure.why)?;
                }
            }
            result.push(closure);
            continue;
        }
        if is_agent && !READY_STATES.contains(&hc::status(&terminal).as_str()) {
            continue;
        }
        let Some(pid) = hc::top_pid(&terminal) else {
            continue; // hc knows no pid: nothing to look under, not known to be idle
        };
        if !foreground_idle(&terminal, is_agent) {
            continue; // work in front, or nothing known to be idle there
        }
        let judged = Judged::Live(Running::of(&terminal));
        // What the board says first, so a table that cannot be read is said only of a terminal it
        // would otherwise close.
        let (handle, why, mine) = if is_agent {
            let Some(handle) = handle else {
                continue;
            };
            if Some(handle.as_str()) == caller
                || NOT_REAPED_KINDS.contains(&store.session(&handle)?.row.kind.as_str())
            {
                continue; // the caller's own, an orchestrator's or a chat's
            }
            let (live, mine) = held(store, &handle)?;
            if dry_run
                && let Some(task) = live.iter().find(|task| {
                    task.roles
                        .iter()
                        .any(|role| role.handle == handle && role.role == "reserved")
                })
            {
                result.push(Closure {
                    handle: Some(handle.clone()),
                    ..Closure::left(
                        &address,
                        format!("it holds a live reservation on {}", task.slug),
                    )
                });
            }
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
            match closed(hc, &address, &closure, &|t| {
                still_finished(t, processes, &judged)
            }) {
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
                attempts::closed(store, hc, handle, &address, why)?;
            }
        }
        result.push(closure);
    }
    Ok(result)
}

/// The board session a terminal belongs to: its recorded terminal, its `symphony.handle` label, the
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
        let bare = conversation
            .strip_prefix(crate::launch::CODEX)
            .unwrap_or(&conversation);
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
            crate::launch::CODEX.into()
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
    let operating = store.operating_for(&mine)?;
    let live = mine
        .iter()
        .filter(|t| !crate::workflow::operate::finished(operating.as_ref(), t))
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

/// `slug` finished ([`Store::finished`]: done or cancelled, or at a terminal position of the custom
/// workflow that holds it): close the terminals of the sessions launched on it or below it (an
/// agent works a subtask, and closes it from its terminal). Never fails: it runs after a status
/// change that must stand. The lines to print; what went wrong is one of them.
pub fn finish(store: &Store, hc: &dyn Hc, processes: &Processes, slug: &str) -> Vec<String> {
    let closures = || -> Result<Vec<Closure>, Error> {
        if !store.finished(&store.get(slug)?)? {
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
