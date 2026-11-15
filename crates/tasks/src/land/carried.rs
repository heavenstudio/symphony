//! An approve that stands over the one merge of the default branch made on the commit it names
//! ([TASKS](../../../../docs/TASKS.md#the-default-landing)): the merge an executor makes to
//! settle a land's conflict in documents, which no reviewer reads again.
//!
//! It stands at the tip T when all of these hold ([`over`]):
//!
//! 1. T is a merge of two parents and no more, its first the commit the approve names and its
//!    second a commit the default branch holds. T is the branch's tip, so nothing is on top.
//! 2. Every path where T differs from git's own merge of those two parents (`git merge-tree`,
//!    which leaves its conflict markers where the two conflict) is a document ([`document`]). So
//!    what nobody read is the settling of a conflict in a document and nothing else: each other
//!    path is as git merges what the reviewer read with what the default branch landed, which is
//!    what the land itself makes of a tip. A merge that changes anything else by hand, in a
//!    conflict or not, is read by a reviewer, as before.
//!
//! The land then reads the task's tests at T whatever its needs name ([`super::needs::TESTS`]): the
//! approve says the change was read, and the recorded run that it still works on the default
//! branch as merged.

use super::{git_z, run_git, short};
use crate::sweep::git::{ancestor, git};
use std::path::Path;

/// How many paths a refusal names of those a merge changes beyond git's own.
const NAMED: usize = 5;

/// The one merge an approve stands over ([`over`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    /// The merge: the branch's tip.
    pub merge: String,
    /// The commit the approve names: the merge's first parent.
    pub approved: String,
    /// The default branch's commit the merge brought in: its second parent.
    pub from: String,
    /// The documents where the merge differs from git's own merge of the two, each a conflict
    /// settled by hand or a document edited in the merge; none where git merged them alone.
    pub settled: Vec<String>,
}

impl Carried {
    /// What the land says of it after naming the approve: the merge of the branch `default` it
    /// stands over, and what that merge settles.
    pub fn said(&self, default: &str) -> String {
        let tip = &self.merge;
        let settled = if self.settled.is_empty() {
            "which is git's own merge of the two".to_string()
        } else {
            format!(
                "which differs from git's own merge of the two in documents alone ({})",
                self.settled.join(", ")
            )
        };
        format!(
            "naming {}, which stands over the one merge {} of {default} at {} into it, {settled}; \
             its tests are read at {}",
            short(&self.approved),
            short(tip),
            short(&self.from),
            short(tip)
        )
    }
}

/// Whether `path` is a document: a Markdown file, by its extension.
pub fn document(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Whether an approve naming one of the commits `named` stands at `tip` in the checkout `dir`,
/// `base` the default branch's tip: the merge it stands over. Otherwise nothing where `tip` is no
/// merge of a commit the default branch holds into a commit named, the approve's ordinary reading
/// then; or, where it is that merge, why the approve does not stand over it.
pub fn over(
    dir: &Path,
    base: &str,
    tip: &str,
    named: &[String],
) -> Result<Carried, Option<String>> {
    let parents = run_git(dir, &["rev-list", "--parents", "-n", "1", tip]).map_err(|_| None)?;
    let parents: Vec<&str> = parents.split_whitespace().skip(1).collect();
    let [approved, from] = parents[..] else {
        return Err(None);
    };
    if !named.iter().any(|sha| sha == approved) || !ancestor(dir, from, base) {
        return Err(None);
    }
    let cannot = |why: String| Some(format!("what that merge settles cannot be read ({why})"));
    let differs = merged(dir, approved, from)
        .and_then(|tree| {
            git_z(
                dir,
                &["diff", "--name-only", "--no-renames", "-z", &tree, tip],
            )
        })
        .map_err(cannot)?;
    let (settled, beyond): (Vec<String>, Vec<String>) =
        differs.into_iter().partition(|path| document(path));
    if beyond.is_empty() {
        return Ok(Carried {
            merge: tip.to_string(),
            approved: approved.to_string(),
            from: from.to_string(),
            settled,
        });
    }
    let mut said = beyond
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if beyond.len() > NAMED {
        said.push_str(&format!(" and {} more", beyond.len() - NAMED));
    }
    Err(Some(format!(
        "that merge differs from git's own merge of the two in {said}, no document: an approve \
         stands over a merge that settles documents alone"
    )))
}

/// The tree git's own merge of `ours` and `theirs` makes in `dir`, conflict markers left where the
/// two conflict, nothing checked out.
fn merged(dir: &Path, ours: &str, theirs: &str) -> Result<String, String> {
    let args = ["merge-tree", "--write-tree", "--name-only", ours, theirs];
    let done = git(dir, &args).ok_or_else(|| format!("git cannot run in {}", dir.display()))?;
    // 0 is a clean merge and 1 one with conflicts, the tree written either way.
    let tree = String::from_utf8_lossy(&done.stdout)
        .lines()
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if !matches!(done.status.code(), Some(0 | 1)) || tree.is_empty() {
        let said = String::from_utf8_lossy(&done.stderr);
        let said = said.lines().find(|l| !l.trim().is_empty());
        return Err(format!(
            "git merge-tree: {}",
            said.map_or("failed", str::trim)
        ));
    }
    Ok(tree)
}

#[cfg(test)]
mod tests;
