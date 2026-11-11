//! Resume recorded agent conversations while holding their session against concurrent closes.

use super::hc::{self, Hc, text_of};
use super::{
    GONE, LaunchError, Machine, Start, Started, branch_exists, claude_args, cleanup, codex_args,
    git, gone, live_executor_in, start_confirmed, stream,
};
use crate::rows::TaskSessionRow;
use crate::{Error, Store};
use std::collections::HashSet;
use std::path::Path;

/// Reopen every closed session of `slug` (or `handle`'s), and every open one whose pane is gone,
/// where it was, in a new terminal, resuming its conversation.
pub fn restore(
    store: &Store,
    hc: &dyn Hc,
    m: &Machine,
    slug: &str,
    handle: Option<&str>,
    tab: Option<&str>,
) -> Result<Vec<Started>, Error> {
    let recorded = store.task_sessions(slug)?;
    if let Some(handle) = handle
        && !recorded.iter().any(|s| s.handle == handle)
    {
        return Err(Error::Refused(format!(
            "{handle} has no recorded session on {slug}"
        )));
    }
    if recorded.is_empty() {
        return Err(Error::Refused(format!(
            "{slug} has no recorded sessions: it was not launched with `just tasks launch`"
        )));
    }
    let running: HashSet<String> = if recorded.iter().any(|s| s.closed.is_none()) {
        live_addresses(hc)?
    } else {
        HashSet::new()
    };
    let mut restored = Vec::new();
    for s in &recorded {
        if handle.is_some_and(|h| s.handle != h) || (s.closed.is_none() && !gone(&s.pane, &running))
        {
            continue;
        }
        // Held for the whole restore, as a close holds it: neither starts while the other runs,
        // and each of its writes lands only while it still holds it.
        let hold = store.take_session(&s.handle, "a restore")?;
        let started = restore_one(store, hc, m, slug, s, tab, hold);
        let released = store.release_session(&s.handle, hold);
        restored.push(started?);
        released?;
    }
    Ok(restored)
}

/// Restore `session` of `slug`, closed or in a pane that is gone, while it holds it.
fn restore_one(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    slug: &str,
    session: &TaskSessionRow,
    tab: Option<&str>,
    hold: u64,
) -> Result<Started, Error> {
    if session.claude_session.is_empty() || session.claude_session == "codex:" {
        return Err(Error::Refused(format!(
            "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
             with `just tasks launch`)",
            session.handle
        )));
    }
    if let Some(terminal) = running_as(hc, &session.handle)? {
        return Err(Error::Refused(format!(
            "{} still runs in terminal {terminal}: close it before restoring",
            session.handle
        )));
    }
    if let Some(handle) = live_executor_in(store, Path::new(&session.worktree))?
        && handle != session.handle
    {
        return Err(Error::Refused(format!(
            "{} has live executor {handle}: cannot restore another agent there",
            session.worktree
        )));
    }
    if session.closed.is_none() {
        // A pane that is gone (a herdr pane from before hc): its closure is said first, once:
        // a close that recorded it meanwhile said it already.
        store.close_task_session_as(
            session,
            GONE,
            &cleanup::restore_line(slug, &session.handle),
            hold,
        )?;
    }
    if !Path::new(&session.worktree).is_dir() {
        if !session.branch.is_empty() && branch_exists(&machine.root, &session.branch) {
            git(
                &machine.root,
                &["worktree", "add", &session.worktree, &session.branch],
            )?;
        } else {
            git(
                &machine.root,
                &["worktree", "add", "--detach", &session.worktree, "main"],
            )?;
        }
    }
    let text = format!(
        "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /work {slug}.",
        session.handle, session.handle
    );
    let (agent, args) = if let Some(id) = session.claude_session.strip_prefix("codex:") {
        let mut args = codex_args(&session.model, &session.handle, machine);
        args.extend(["resume".into(), id.into()]);
        ("codex", args)
    } else {
        (
            "claude",
            claude_args(
                &session.model,
                &session.handle,
                &machine.mode,
                ["--resume", &session.claude_session],
            ),
        )
    };
    if !store.renew_session(&session.handle, hold)? {
        return Err(Error::Refused(format!(
            "another close or a restore of {} took its session over, so the restore starts no \
             terminal",
            session.handle
        )));
    }
    let mut terminal = String::new();
    let started = start_confirmed(
        hc,
        machine,
        &mut terminal,
        &Start {
            cwd: &session.worktree,
            handle: &session.handle,
            stream: tab
                .filter(|t| !t.is_empty())
                .unwrap_or(stream(&session.role)),
            agent,
            resumed_codex: agent == "codex",
            args,
            text: &text,
        },
    );
    let message = started.map_err(|problem| {
        if !terminal.is_empty() {
            let _ = hc.close(&terminal); // the problem is what the caller must hear
        }
        Error::from(problem)
    })?;
    // Reopened and revived only while the restore still holds the session and the record is the
    // one read: else the record says what holds, the terminal this one started goes, and it
    // records nothing.
    let reopened = store.reopen_task_session_as(session, &terminal, hold);
    if !matches!(reopened, Ok(true)) {
        let _ = hc.close(&terminal); // the problem is what the caller must hear
        let why = match reopened {
            Err(Error::Refused(why)) => why,
            Err(problem) => return Err(problem),
            Ok(_) => format!("{}'s record changed", session.handle),
        };
        return Err(Error::Refused(format!(
            "{why} while it was restored, so the restore records nothing and terminal \
             {terminal}, which it started, is killed"
        )));
    }
    if message == super::startup::Message::Working {
        let thread = session
            .claude_session
            .strip_prefix("codex:")
            .expect("a resumed Codex turn");
        let delivery = match hc.queue_codex(thread, &text) {
            Ok(()) => {
                "restore message queued for when it is idle; delivery is not yet confirmed".into()
            }
            Err(problem) => format!(
                "restore message queue was not confirmed: {problem}; do not resend without checking the conversation"
            ),
        };
        return Err(Error::Refused(format!(
            "{} resumed in terminal {terminal} and is still working; kept alive with its record reopened; {delivery}",
            session.handle
        )));
    }
    let row = store
        .task_sessions(slug)?
        .into_iter()
        .find(|x| x.handle == session.handle)
        .ok_or_else(|| {
            Error::Refused(format!(
                "{} has no recorded session on {slug}",
                session.handle
            ))
        })?;
    Ok(Started { row })
}

/// The addresses of this board's live terminals.
fn live_addresses(hc: &dyn Hc) -> Result<HashSet<String>, LaunchError> {
    Ok(hc
        .terminals()?
        .iter()
        .filter(|t| text_of(t, "state") == "live")
        .map(|t| text_of(t, "address"))
        .collect())
}

/// The live terminal `handle` still runs in, by its name or its `sf.handle` label, if any.
fn running_as(hc: &dyn Hc, handle: &str) -> Result<Option<String>, LaunchError> {
    let labelled = hc.listed(&[format!("{}={handle}", hc::HANDLE_LABEL)], false)?;
    let named = hc.live()?;
    Ok(labelled
        .iter()
        .chain(named.iter().filter(|t| text_of(t, "name") == handle))
        .map(|t| text_of(t, "address"))
        .next())
}
