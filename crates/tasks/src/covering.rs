//! `just tasks done SLUG NOTE --covered-by OTHER`: a task whose work landed with another task's
//! branch, closed as covered by that landing by its executor or orchestrator, with no reviewer's
//! verdict ([`Store::close_covered`], whose module doc says why it is no approve). The board cannot
//! see the repository, so this reads it first, from a checkout of it, as a move back to review
//! does (`crate::land::reopenable`): the commit `OTHER`'s landing names is on the default branch,
//! and `SLUG` has no branch of its own with commits the default branch lacks. Such a branch is
//! work of its own, which needs its own review and land, so the close refuses, naming its commits.

use crate::cli::Env;
use crate::land::Settings;
use crate::sweep::git::{ancestor, git, rev};
use crate::sweep::trigger;
use crate::{Error, Store, Task};
use std::path::Path;

/// How many of a branch's own commits a refusal names, the rest counted.
const NAMED: usize = 5;

/// `slug` closed by `handle` as covered by `other`'s landing, with `note`, once the repository `env`
/// runs in says the landing's commit is on its default branch and `slug`'s own branches hold no
/// commit the default branch lacks ([`own_commits`]). Refused, with nothing written, from outside a
/// checkout of the repository, as the board cannot tell either there.
pub fn close(
    store: &Store,
    env: Option<&Env>,
    slug: &str,
    other: &str,
    handle: &str,
    note: &str,
) -> Result<Task, Error> {
    let named = store.covering(other)?;
    let task = store.get(slug)?;
    let unknown = || {
        Error::Refused(format!(
            "{slug} is closed as covered only from a checkout of its repository, which says \
             whether {other}'s landing is on the default branch and what {slug}'s own branch holds"
        ))
    };
    let common = env
        .and_then(|env| crate::git::common_dir(&env.cwd).ok())
        .ok_or_else(unknown)?;
    let main = trigger::main_checkout(&common).ok_or_else(unknown)?;
    let default = Settings::of(main).map_err(Error::Refused)?.default_branch;
    let tip = format!("refs/heads/{default}");
    let commit = rev(main, &format!("{named}^{{commit}}"))
        .filter(|commit| ancestor(main, commit, &tip))
        .ok_or_else(|| {
            Error::Refused(format!(
                "{other}'s landing names {named}, which {default} does not hold: {slug} is closed \
                 as covered only by a landing on {default}"
            ))
        })?;
    let mut branches = vec![slug.to_string()];
    branches.extend(task.branch.filter(|branch| branch != slug));
    for branch in &branches {
        let own = own_commits(main, branch, &tip).map_err(Error::Refused)?;
        if own.is_empty() {
            continue;
        }
        let mut said: Vec<&str> = own.iter().take(NAMED).map(String::as_str).collect();
        let more = own.len().saturating_sub(NAMED);
        let rest = format!("and {more} more");
        if more > 0 {
            said.push(&rest);
        }
        return Err(Error::Refused(format!(
            "{slug} has a branch of its own, {branch}, with {} {default} does not hold ({}): \
             that work needs its own review and land, not a close as covered by {other}",
            if own.len() == 1 {
                "1 commit".to_string()
            } else {
                format!("{} commits", own.len())
            },
            said.join(", ")
        )));
    }
    store.close_covered(slug, handle, other, &commit, note)
}

/// The commits of `branch` that `tip` (the default branch) does not hold, oldest first, each as its
/// short hash and subject; none when there is no such branch. A log git cannot give is the line
/// saying so, never taken for none.
fn own_commits(main: &Path, branch: &str, tip: &str) -> Result<Vec<String>, String> {
    let reference = format!("refs/heads/{branch}");
    if rev(main, &reference).is_none() {
        return Ok(Vec::new());
    }
    let range = format!("{tip}..{reference}");
    let args = ["log", "--reverse", "--format=%h %s", &range, "--"];
    let done = git(main, &args)
        .filter(|done| done.status.success())
        .ok_or_else(|| format!("git {} failed in {}", args.join(" "), main.display()))?;
    Ok(String::from_utf8_lossy(&done.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(String::from)
        .collect())
}
