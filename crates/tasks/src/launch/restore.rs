//! Resume recorded agent conversations in their worktrees.

use super::herdr::Herdr;
use super::{
    Machine, Started, branch_exists, claude_args, codex_args, confirmed, git, live_executor_in,
    pane_for, recover, start, stream,
};
use crate::{Error, Store};
use std::path::Path;

/// Reopen every closed session of `slug` (or `handle`'s) where it was and resume its Claude
/// conversation.
pub fn restore(
    store: &Store,
    herdr: &dyn Herdr,
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
    let mut restored = Vec::new();
    for s in &recorded {
        if handle.is_some_and(|h| s.handle != h) {
            continue;
        }
        if s.closed.is_none() {
            let tasks = store.tasks(None)?;
            if !store.pending_posts(&tasks)?.iter().any(|p| {
                p.handle == s.handle
                    && p.overdue(store.now())
                    && p.launch
                        .as_ref()
                        .is_some_and(|launch| launch.slug == s.slug)
            }) || recover::present(s, &herdr.all_agents()?)
            {
                continue;
            }
        }
        if s.claude_session.is_empty() || s.claude_session == "codex:" {
            return Err(Error::Refused(format!(
                "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
                 with `just tasks launch`)",
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
        let tab = tab.filter(|t| !t.is_empty()).unwrap_or(stream(&s.role));
        let pane = pane_for(herdr, &s.worktree, &s.handle, tab)?;
        let text = format!(
            "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /work {slug}.",
            s.handle, s.handle
        );
        let (agent, args) = if let Some(id) = s.claude_session.strip_prefix("codex:") {
            let mut args = codex_args(&s.model, &s.handle);
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
        let prompted = start(herdr, &s.handle, &pane, agent, &args, &*m.sleep)
            .and_then(|()| herdr.call(&["agent", "prompt", &s.handle, &text]))
            .and_then(|_| confirmed(herdr, &pane, &text, &*m.sleep));
        if let Err(problem) = prompted {
            herdr.close(&pane)?;
            return Err(problem.into());
        }
        // cleanup leaves handles that closed: the restored agent must be able to use the board
        store.revive(&s.handle)?;
        store.reopen_task_session(slug, &s.handle, &pane)?;
        let row = store
            .task_sessions(slug)?
            .into_iter()
            .find(|x| x.handle == s.handle)
            .ok_or_else(|| {
                Error::Refused(format!("{} has no recorded session on {slug}", s.handle))
            })?;
        restored.push(Started { row, stuck: false });
    }
    Ok(restored)
}
