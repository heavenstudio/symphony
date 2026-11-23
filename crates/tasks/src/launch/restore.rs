//! Resume recorded agent conversations while holding their session against concurrent closes.

use super::attempts::Local;
use super::hc::{self, Hc, text_of};
use super::{
    GONE, LaunchError, Machine, Start, Started, agent_of, branch_exists, claude_args, cleanup,
    codex_args, git, gone, live_executor_in, prompt_text, skill, start_confirmed, stream,
};
use crate::rows::TaskSessionRow;
use crate::store::unanswered::restorable;
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
        let text = format!(
            "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /{} {slug}.",
            s.handle,
            s.handle,
            skill(&s.role)
        );
        restored.push(holding(store, hc, m, s, tab, &text)?);
    }
    Ok(restored)
}

/// The reviewers recorded on `slug` that never said `just tasks leave`, newest first: those a
/// reviewer launch goes back to ([`resume_reviewer`]). A quiet one whose task went back to
/// `in_progress` reads left by its health, and is one of them.
pub(super) fn recorded_reviewers(store: &Store, slug: &str) -> Result<Vec<TaskSessionRow>, Error> {
    let mut stayed = Vec::new();
    for record in store.task_sessions(slug)?.into_iter().rev() {
        if record.role != "reviewer" {
            continue;
        }
        let left = match store.session(&record.handle) {
            Ok(session) => session.row.left_at.is_some(),
            Err(Error::Refused(_)) => true,
            Err(problem) => return Err(problem),
        };
        if !left {
            stayed.push(record);
        }
    }
    Ok(stayed)
}

/// What a reviewer launch on `slug` does with its recorded reviewers ([`recorded_reviewers`]):
/// refused while one still runs in a terminal (its handle's, or the one its open record names),
/// naming it and the launch's `--beside WHY`; the newest with a conversation a restore can resume
/// ([`restorable`]) is resumed ([`resume_reviewer`]); none, and a new reviewer starts.
pub(super) fn reviewer_to_resume(
    store: &Store,
    hc: &dyn Hc,
    slug: &str,
) -> Result<Option<TaskSessionRow>, Error> {
    let recorded = recorded_reviewers(store, slug)?;
    let live = if recorded.iter().any(|r| r.closed.is_none()) {
        live_addresses(hc)?
    } else {
        HashSet::new()
    };
    for record in recorded {
        let open_in =
            (record.closed.is_none() && live.contains(&record.pane)).then(|| record.pane.clone());
        if let Some(terminal) = running_as(hc, &record.handle)?.or(open_in) {
            let (board, handle) = (store.board_command(), &record.handle);
            return Err(Error::Refused(format!(
                "{handle} reviews {slug} and still runs in terminal {terminal}: give it the new tip \
                 on the thread ({board} say {slug} TEXT --to @reviewer), or close it ({board} close \
                 {slug} {handle}) and launch again to resume it, or launch another beside it \
                 saying why ({board} launch {slug} --role reviewer --beside WHY)"
            )));
        }
        if restorable(&record) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

/// Resume `record`, the reviewer a launch on its task goes back to ([`reviewer_to_resume`]): its
/// own conversation, under its own handle, in its recorded worktree and on its model, in a new
/// terminal, as a restore resumes it, told `/review-task SLUG HANDLE` (then `prompt`): its skill
/// judges the new tip from there and takes the role again unless it holds it.
pub(super) fn resume_reviewer(
    store: &Store,
    hc: &dyn Hc,
    m: &Machine,
    record: &TaskSessionRow,
    prompt: &str,
    tab: Option<&str>,
) -> Result<Started, Error> {
    let agent = agent_of(&record.claude_session);
    let text = prompt_text(&record.slug, &record.handle, prompt, agent, &record.role);
    holding(store, hc, m, record, tab, &text)
}

/// Restore `session`, telling it `text`, held for the whole restore as a close holds it: neither
/// starts while the other runs, and each of its writes lands only while it still holds it.
fn holding(
    store: &Store,
    hc: &dyn Hc,
    m: &Machine,
    session: &TaskSessionRow,
    tab: Option<&str>,
    text: &str,
) -> Result<Started, Error> {
    let hold = store.take_session(&session.handle, "a restore")?;
    let started = restore_one(store, hc, m, session, tab, hold, text);
    let released = store.release_session(&session.handle, hold);
    let started = started?;
    released?;
    Ok(started)
}

/// Restore `session`, closed or in a pane that is gone, while it holds it, its first message
/// `text`.
fn restore_one(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    session: &TaskSessionRow,
    tab: Option<&str>,
    hold: u64,
    text: &str,
) -> Result<Started, Error> {
    let slug = session.slug.as_str();
    if !restorable(session) {
        return Err(Error::Refused(format!(
            "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
             with `just tasks launch`)",
            session.handle
        )));
    }
    // An agent turned off, or whose program this machine lacks, is not started again: its record
    // stays closed ([`super::off`]).
    let agent = agent_of(&session.claude_session);
    if let Some(why) = machine.refusing(store, agent)? {
        return Err(Error::Refused(format!(
            "no restore of {}: {why}",
            session.handle
        )));
    }
    // Through a recipe an agent runs unasked, a record names a folder it makes again and starts an
    // agent in: only one in the project ([`super::Bounds`]), whenever the record was written, and
    // used as the check resolved it, not resolved again.
    let worktree = match &machine.bounds {
        Some(bounds) => bounds
            .admit(
                &format!("{}'s recorded worktree", session.handle),
                Path::new(&session.worktree),
            )
            .map_err(Error::Refused)?
            .to_string_lossy()
            .into_owned(),
        None => session.worktree.clone(),
    };
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
    if !Path::new(&worktree).is_dir() {
        if !session.branch.is_empty() && branch_exists(&machine.root, &session.branch) {
            git(
                &machine.root,
                &["worktree", "add", &worktree, &session.branch],
            )?;
        } else {
            git(
                &machine.root,
                &["worktree", "add", "--detach", &worktree, "main"],
            )?;
        }
    }
    let (agent, args) = if let Some(id) = session.claude_session.strip_prefix(crate::launch::CODEX)
    {
        let mut args = codex_args(&session.model, None, &session.handle, machine);
        args.extend(["resume".into(), id.into()]);
        ("codex", args)
    } else {
        (
            "claude",
            claude_args(
                &session.model,
                None,
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
    let local = Local::new(store, hc, crate::workflow::board_service("restore"));
    let conversation = session
        .claude_session
        .strip_prefix(crate::launch::CODEX)
        .unwrap_or(&session.claude_session);
    let task = store.get(slug)?;
    let attempt = local
        .follow(
            &task,
            &session.role,
            &session.handle,
            &worktree,
            text,
            conversation,
        )?
        .id;
    let mut terminal = String::new();
    // The terminal this restore started and cannot use goes, and its attempt ends cancelled.
    let abandon = |terminal: &str, why: &str| {
        let _ = local.abandon(&attempt, hc, terminal, why); // the problem is what the caller must hear
    };
    let started = start_confirmed(
        hc,
        machine,
        &mut terminal,
        &Start {
            cwd: &worktree,
            handle: &session.handle,
            stream: tab
                .filter(|t| !t.is_empty())
                .unwrap_or(stream(&session.role)),
            agent,
            resumed_codex: agent == "codex",
            args,
            text,
        },
        (&local, &attempt),
    );
    let message = started
        .and_then(|message| {
            local.running(&attempt, Some(conversation))?;
            Ok(message)
        })
        .inspect_err(|problem| abandon(&terminal, &problem.to_string()))?;
    // Reopened and revived only while the restore still holds the session and the record is the
    // one read: else the record says what holds, the terminal this one started goes, and it
    // records nothing.
    let reopened = store.reopen_task_session_as(session, &terminal, hold);
    if !matches!(reopened, Ok(true)) {
        let why = match reopened {
            Err(Error::Refused(why)) => why,
            Err(problem) => {
                abandon(&terminal, &problem.to_string());
                return Err(problem);
            }
            Ok(_) => format!("{}'s record changed", session.handle),
        };
        abandon(&terminal, &format!("{why} while it was restored"));
        return Err(Error::Refused(format!(
            "{why} while it was restored, so the restore records nothing and terminal \
             {terminal}, which it started, is killed"
        )));
    }
    if message == super::startup::Message::Working {
        let thread = session
            .claude_session
            .strip_prefix(crate::launch::CODEX)
            .expect("a resumed Codex turn");
        let delivery = match hc.queue_codex(thread, text) {
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
    Ok(Started { row, resumed: true })
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

/// The live terminal `handle` still runs in, by its name or its `symphony.handle` label, if any.
fn running_as(hc: &dyn Hc, handle: &str) -> Result<Option<String>, LaunchError> {
    let labelled = hc.listed(&[format!("{}={handle}", hc::HANDLE_LABEL)], false)?;
    let named = hc.live()?;
    Ok(labelled
        .iter()
        .chain(named.iter().filter(|t| text_of(t, "name") == handle))
        .map(|t| text_of(t, "address"))
        .next())
}
