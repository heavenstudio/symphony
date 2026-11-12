//! `just tasks done SLUG NOTE --covered-by OTHER`: a task whose work landed with another task's
//! branch, closed as covered by that landing by its executor or orchestrator, with no reviewer's
//! verdict ([`Store::close_covered`], whose module doc says why it is no approve). The board cannot
//! see the repository, so this reads it first, from a checkout of it, as a move back to review
//! does (`crate::land::reopenable`): the commit `OTHER`'s landing names is on the default branch,
//! and `SLUG` has no work of its own the default branch lacks where the board records its executors
//! working ([`places`]): commits on its branches, at a worktree's HEAD (a detached one too), or
//! changes in a worktree no commit holds. Such work needs its own review and land, so the close
//! refuses, naming it. What the board records nothing of, it cannot read: the closer checks that.

use crate::cli::Env;
use crate::land::Settings;
use crate::sweep::git::{ancestor, git, rev, stdout};
use crate::sweep::trigger;
use crate::{Error, Store, Task};
use std::collections::BTreeSet;
use std::path::Path;

/// How many of a branch's own commits, or of a worktree's changed paths, a refusal names, the rest
/// counted.
const NAMED: usize = 5;

/// Where the board records an executor of a task working: a worktree's top folder and the branch
/// checked out there ("" when detached).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The executor's session.
    pub handle: String,
    pub worktree: String,
    pub branch: String,
}

/// The places the board records for `slug`'s executors: each executor launch of it (`tasks launch`,
/// `restore`), and the in-process binding of each session that executed it or a work subtask under
/// it ([`Store::ever_executed`]), where the beat hook keeps the checkout its last tool call ran in,
/// as `just tasks agents` shows it. A session that left keeps no binding to read.
pub fn places(store: &Store, slug: &str) -> Result<Vec<Place>, Error> {
    let mut places: Vec<Place> = (store.task_sessions(slug)?.into_iter())
        .filter(|launch| launch.role == "executor")
        .map(|launch| Place {
            handle: launch.handle,
            worktree: launch.worktree,
            branch: launch.branch,
        })
        .collect();
    let executed = store.ever_executed(slug)?;
    places.extend(
        (store.agent_bindings()?.into_iter())
            .filter(|(handle, bound)| {
                bound.in_process && !bound.worktree.is_empty() && executed.contains(handle)
            })
            .map(|(handle, bound)| Place {
                handle,
                worktree: bound.worktree,
                branch: bound.branch,
            }),
    );
    Ok(places)
}

/// `slug` closed by `handle` as covered by `other`'s landing, with `note`, once the repository `env`
/// runs in says the landing's commit is on its default branch and `slug` has no work of its own
/// there: no commit the default branch lacks on the branch named as its slug, on the branches
/// recorded for its executors ([`places`]), or at the HEAD of a linked worktree of theirs, and no
/// change in such a worktree that no commit holds. Refused, with nothing written, from outside a
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
    let places = places(store, slug)?;
    let own = |what: String| {
        Error::Refused(format!(
            "{what}: that work needs its own review and land, not a close as covered by {other}"
        ))
    };
    let mut branches = vec![slug.to_string()];
    for branch in task.branch.iter().chain(places.iter().map(|p| &p.branch)) {
        if !branch.is_empty() && !branches.contains(branch) {
            branches.push(branch.clone());
        }
    }
    for branch in &branches {
        let reference = format!("refs/heads/{branch}");
        if rev(main, &reference).is_none() {
            continue;
        }
        let commits = commits(main, &tip, &reference).map_err(Error::Refused)?;
        if !commits.is_empty() {
            return Err(own(format!(
                "{slug} has a branch of its own, {branch}, with {} {default} does not hold ({})",
                counted(commits.len(), "commit"),
                listed(&commits)
            )));
        }
    }
    let common = common.canonicalize().unwrap_or(common);
    let mut seen = BTreeSet::new();
    for place in &places {
        let worktree = Path::new(&place.worktree);
        let ours = crate::git::linked(worktree)
            && crate::git::checkout_common_dir(worktree).as_ref() == Some(&common);
        if !ours || !seen.insert(place.worktree.as_str()) {
            continue;
        }
        let at = format!(
            "{slug}'s executor {} works in {}",
            place.handle, place.worktree
        );
        let commits = commits(worktree, &tip, "HEAD").map_err(Error::Refused)?;
        if !commits.is_empty() {
            let on = stdout(worktree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                .map(|branch| branch.trim().to_string())
                .filter(|branch| !branch.is_empty())
                .unwrap_or_else(|| "a detached HEAD".to_string());
            return Err(own(format!(
                "{at}, on {on}, with {} {default} does not hold ({})",
                counted(commits.len(), "commit"),
                listed(&commits)
            )));
        }
        let changed = uncommitted(worktree).map_err(Error::Refused)?;
        if !changed.is_empty() {
            return Err(Error::Refused(format!(
                "{at}, which holds changes no commit has ({}: {}): commit them for their own \
                 review and land, or drop them, before a close as covered by {other}",
                counted(changed.len(), "path"),
                listed(&changed)
            )));
        }
    }
    store.close_covered(slug, handle, other, &commit, note)
}

/// The commits of `reference` that `tip` (the default branch) does not hold, read in `dir`, oldest
/// first, each as its short hash and subject. A log git cannot give is the line saying so, never
/// taken for none.
fn commits(dir: &Path, tip: &str, reference: &str) -> Result<Vec<String>, String> {
    let range = format!("{tip}..{reference}");
    let args = ["log", "--reverse", "--format=%h %s", &range, "--"];
    let done = git(dir, &args)
        .filter(|done| done.status.success())
        .ok_or_else(|| format!("git {} failed in {}", args.join(" "), dir.display()))?;
    Ok(lines(&String::from_utf8_lossy(&done.stdout)))
}

/// The paths worktree `dir` holds changes of that no commit has: tracked changes and untracked
/// files git does not ignore, read without refreshing its index, which another agent may be
/// writing. A status git cannot give is the line saying so.
fn uncommitted(dir: &Path) -> Result<Vec<String>, String> {
    let args = ["status", "--porcelain", "--untracked-files=normal"];
    let text = stdout(dir, &args)
        .ok_or_else(|| format!("git {} failed in {}", args.join(" "), dir.display()))?;
    Ok(lines(&text)
        .iter()
        .map(|line| line.get(3..).unwrap_or(line).to_string())
        .collect())
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(String::from)
        .collect()
}

/// `n` `what`s, in words: "1 commit", "2 commits".
fn counted(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// The first [`NAMED`] of `items`, and how many more.
fn listed(items: &[String]) -> String {
    let mut said: Vec<&str> = items.iter().take(NAMED).map(String::as_str).collect();
    let more = items.len().saturating_sub(NAMED);
    let rest = format!("and {more} more");
    if more > 0 {
        said.push(&rest);
    }
    said.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_five_and_counts_the_rest() {
        let items: Vec<String> = (1..=7).map(|n| format!("c{n}")).collect();
        assert_eq!(listed(&items[..1]), "c1");
        assert_eq!(listed(&items[..5]), "c1, c2, c3, c4, c5");
        assert_eq!(listed(&items), "c1, c2, c3, c4, c5, and 2 more");
        assert_eq!(counted(1, "commit"), "1 commit");
        assert_eq!(counted(7, "path"), "7 paths");
    }
}
