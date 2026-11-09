//! Command preparation happens before connecting to the board.

use super::{Finished, Options, coalesce, git, sweep, trigger};
use crate::{Store, cli::Env, sys};
use std::borrow::Borrow;

/// What `tasks sweep` was asked.
#[derive(Debug, Clone, Default)]
pub struct Request<'a> {
    pub dry_run: bool,
    pub hours: Option<f64>,
    pub tasks: Vec<&'a str>,
    pub background: Option<&'a str>,
}

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
        }
        let only = scoped.then(|| finished.worktrees.clone());
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
