//! Resume recorded agent conversations through hc.

use super::hc::Hc;
use super::{
    GONE, Machine, Start, Started, branch_exists, claude_args, cleanup, codex_args, git, gone,
    live_addresses, live_executor_in, running_as, start_confirmed, stream,
};
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
        if s.claude_session.is_empty() || s.claude_session == "codex:" {
            return Err(Error::Refused(format!(
                "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
                 with `just tasks launch`)",
                s.handle
            )));
        }
        if let Some(terminal) = running_as(hc, &s.handle)? {
            return Err(Error::Refused(format!(
                "{} still runs in terminal {terminal}: close it before restoring",
                s.handle
            )));
        }
        if let Some(handle) = live_executor_in(store, Path::new(&s.worktree))?
            && handle != s.handle
        {
            return Err(Error::Refused(format!(
                "{} has live executor {handle}: cannot restore another agent there",
                s.worktree
            )));
        }
        if s.closed.is_none() {
            // A pane that is gone (a herdr pane from before hc): its closure is said first.
            store.close_task_session(
                slug,
                &s.handle,
                GONE,
                &cleanup::restore_line(slug, &s.handle),
            )?;
        }
        if !Path::new(&s.worktree).is_dir() {
            if !s.branch.is_empty() && branch_exists(&m.root, &s.branch) {
                git(&m.root, &["worktree", "add", &s.worktree, &s.branch])?;
            } else {
                git(
                    &m.root,
                    &["worktree", "add", "--detach", &s.worktree, "main"],
                )?;
            }
        }
        let text = format!(
            "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /work {slug}.",
            s.handle, s.handle
        );
        let (agent, args) = if let Some(id) = s.claude_session.strip_prefix("codex:") {
            let mut args = codex_args(&s.model, &s.handle, m);
            args.extend(["resume".into(), id.into()]);
            ("codex", args)
        } else {
            (
                "claude",
                claude_args(
                    &s.model,
                    &s.handle,
                    &m.mode,
                    ["--resume", &s.claude_session],
                ),
            )
        };
        let mut terminal = String::new();
        let started = start_confirmed(
            hc,
            m,
            &mut terminal,
            &Start {
                cwd: &s.worktree,
                handle: &s.handle,
                stream: tab.filter(|t| !t.is_empty()).unwrap_or(stream(&s.role)),
                agent,
                args,
                text: &text,
            },
        );
        if let Err(problem) = started {
            if !terminal.is_empty() {
                let _ = hc.close(&terminal); // the problem is what the caller must hear
            }
            return Err(problem.into());
        }
        // cleanup leaves handles that closed: the restored agent must be able to use the board
        store.revive(&s.handle)?;
        store.reopen_task_session(slug, &s.handle, &terminal)?;
        let row = store
            .task_sessions(slug)?
            .into_iter()
            .find(|x| x.handle == s.handle)
            .ok_or_else(|| {
                Error::Refused(format!("{} has no recorded session on {slug}", s.handle))
            })?;
        restored.push(Started { row });
    }
    Ok(restored)
}
