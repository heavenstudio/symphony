//! Command preparation happens before connecting to the board.

use super::{Finished, Options, canonical, coalesce, git, sweep, trigger};
use crate::{Store, cli::Env, sys};
use std::borrow::Borrow;
use std::collections::BTreeSet;

/// What `tasks sweep` was asked.
#[derive(Debug, Clone, Default)]
pub struct Request<'a> {
    /// Report decisions without changing the board, caches or worktrees.
    pub dry_run: bool,
    /// Override the recent-activity window in hours.
    pub hours: Option<f64>,
    /// Tasks just done or cancelled.
    pub tasks: Vec<&'a str>,
    /// Why work finishing started it; printed before the pass's lines.
    pub background: Option<&'a str>,
}

/// Run a sweep on an already connected board, from where `env` runs. Busy full passes
/// record another request without waiting; callers without a connection use `command_connect`.
pub fn command(store: &Store, env: &Env, request: &Request<'_>) -> Result<Vec<String>, String> {
    command_connect(env, request, || Ok(store))
}

/// A busy full sweep records the request and returns before `connect` is called. Explicit task
/// cleanup visits only its finished worktrees; it never waits behind the full repository pass.
pub fn command_connect<S: Borrow<Store>>(
    env: &Env,
    request: &Request<'_>,
    connect: impl FnOnce() -> Result<S, String>,
) -> Result<Vec<String>, String> {
    let recent = super::recent_hours(&env.vars, request.hours)?;
    let here = git::toplevel(&env.cwd)?;
    let common = crate::git::common_dir(&here).map_err(|e| e.to_string())?;
    let scoped = !request.tasks.is_empty() && request.background.is_none();
    let mut pass = if request.dry_run || scoped {
        None
    } else {
        let Some(pass) = coalesce::request(&common, &request.tasks)? else {
            return Ok(vec![
                "worktree sweep already running; another pass requested".into(),
            ]);
        };
        Some(pass)
    };
    let connected = connect()?;
    let store = connected.borrow();
    let mut lines = Vec::new();
    loop {
        let tasks: Vec<&str> = pass.as_ref().map_or_else(
            || request.tasks.clone(),
            |pass| pass.tasks.iter().map(String::as_str).collect(),
        );
        let mut finished = Finished::default();
        for slug in tasks {
            let of = Finished::of(store, slug).map_err(|e| e.to_string())?;
            finished.worktrees.extend(of.worktrees);
            finished.handles.extend(of.handles);
            finished.agents.extend(of.agents);
            finished.branches.extend(of.branches);
        }
        // The worktrees its sessions worked in, its agents' own `agent-ID` worktrees and those on
        // its branches.
        let only = if scoped {
            let listed = git::worktrees(&here)?;
            let mut covered = BTreeSet::new();
            covered.extend(listed.iter().filter_map(|tree| {
                let path = canonical(&tree.path);
                let branch = tree
                    .branch
                    .as_deref()
                    .and_then(|b| b.strip_prefix("refs/heads/"));
                finished.covers(&path, branch).then_some(path)
            }));
            Some(covered)
        } else {
            None
        };
        let options = Options {
            dry_run: request.dry_run,
            recent_hours: recent,
            finished,
            only,
            ..Options::machine(&env.vars)
        };
        lines.extend(sweep(&here, &here, store, &options)?.lines());
        let Some(current) = pass.take() else { break };
        pass = current.next()?;
        if pass.is_none() {
            break;
        }
    }
    if scoped && !request.dry_run {
        trigger::trigger(env, &here, "after task cleanup", None);
    }
    if let Some(why) = request.background {
        lines.insert(0, format!("[{}] {why}", sys::local_minute(trigger::now())));
    }
    Ok(lines)
}
