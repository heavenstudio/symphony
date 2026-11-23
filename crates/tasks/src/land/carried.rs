//! An approve that stands over the one merge of the default branch made on the commit it names
//! ([TASKS](../../../../docs/workflow/TASKS.md#the-default-landing)): the merge an executor makes to
//! settle a land's conflict in documents, which no reviewer reads again.
//!
//! It stands at the tip T when all of these hold ([`over`]):
//!
//! 1. T is a merge of two parents and no more, its first the commit the approve names and its
//!    second a commit the default branch holds. T is the branch's tip, so nothing is on top.
//! 2. Every path git's own merge of those two parents leaves in conflict (`git merge-tree`, by
//!    its status and the paths it lists, whatever the conflict: both changed, one deleted, a
//!    mode) is a document ([`document`]) that T holds as a regular file ([`REGULAR`]).
//! 3. T differs from that merge's tree in conflicted paths alone. So what nobody read is the
//!    settling of a conflict in a document and nothing else: each other path, a document
//!    included, is as git merges what the reviewer read with what the default branch landed,
//!    which is what the land itself makes of a tip.
//!
//! A merge that settles a conflict anywhere else, or changes by hand a path that did not
//! conflict, is read by a reviewer, as before. The land then reads the task's tests at T whatever
//! its needs name ([`super::needs::TESTS`]): the approve says the change was read, and the
//! recorded run that it still works on the default branch as merged.

use super::{git_z, run_git, short};
use crate::sweep::git::{ancestor, git};
use std::path::Path;

/// How many paths a refusal names of those that keep a merge with its reviewer.
const NAMED: usize = 5;

/// The mode of a regular file in a tree: no executable, symbolic link or submodule.
const REGULAR: &str = "100644";

/// The files that instruct agents wherever they lie, never a document here.
const INSTRUCTIONS: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "RULES.md"];

/// The folder whose files are skills, agent instructions too, at any depth.
const SKILLS: &str = "skills";

/// The one merge an approve stands over ([`over`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    /// The merge: the branch's tip.
    pub merge: String,
    /// The commit the approve names: the merge's first parent.
    pub approved: String,
    /// The default branch's commit the merge brought in: its second parent.
    pub from: String,
    /// The documents whose conflict the merge settles; none where git merged the two alone.
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
                "which settles conflicts in documents alone ({}) and is git's own merge of the \
                 two elsewhere",
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

/// Whether `path` names a document whose conflict an executor settles unread: a Markdown file by
/// its extension that instructs no agent, so none under a `skills` folder and none named as a
/// guide or the rules ([`INSTRUCTIONS`]), where a wrong settling misleads every agent and no test
/// says so.
pub fn document(path: &str) -> bool {
    let path = Path::new(path);
    let markdown = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    let instructs = path
        .file_name()
        .is_some_and(|name| INSTRUCTIONS.iter().any(|guide| name == *guide))
        || path
            .parent()
            .is_some_and(|folder| folder.components().any(|part| part.as_os_str() == SKILLS));
    markdown && !instructs
}

/// `paths` as a refusal names them: the first few, and how many more.
fn listed(paths: &[String]) -> String {
    let mut said = paths
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > NAMED {
        said.push_str(&format!(" and {} more", paths.len() - NAMED));
    }
    said
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
    let (tree, conflicted) = merged(dir, approved, from).map_err(cannot)?;
    let regular = regular(dir, tip, &conflicted).map_err(cannot)?;
    let unread: Vec<String> = (conflicted.iter())
        .filter(|path| !document(path) || !regular.contains(path))
        .cloned()
        .collect();
    if !unread.is_empty() {
        return Err(Some(format!(
            "that merge settles a conflict in {}, no document its executor settles alone (a \
             Markdown file kept as a regular file, no skill, guide or rules): an approve stands \
             over a merge that settles conflicts in documents alone",
            listed(&unread)
        )));
    }
    let args = ["diff", "--name-only", "--no-renames", "-z", &tree, tip];
    let beside: Vec<String> = (git_z(dir, &args).map_err(cannot)?.into_iter())
        .filter(|path| !conflicted.contains(path))
        .collect();
    if !beside.is_empty() {
        return Err(Some(format!(
            "that merge changes {} where the two did not conflict: an approve stands over a \
             merge that settles conflicts in documents alone",
            listed(&beside)
        )));
    }
    Ok(Carried {
        merge: tip.to_string(),
        approved: approved.to_string(),
        from: from.to_string(),
        settled: conflicted,
    })
}

/// Those of `paths` the commit `tip` holds as a regular file ([`REGULAR`]) in `dir`.
fn regular(dir: &Path, tip: &str, paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["ls-tree", "-z", tip, "--"];
    args.extend(paths.iter().map(String::as_str));
    // Each entry is `MODE TYPE OBJECT<TAB>PATH`.
    Ok((git_z(dir, &args)?.iter())
        .filter_map(|entry| entry.split_once('\t'))
        .filter(|(meta, _)| meta.split(' ').next() == Some(REGULAR))
        .map(|(_, path)| path.to_string())
        .collect())
}

/// Git's own merge of `ours` and `theirs` in `dir`, nothing checked out: its tree, which holds
/// conflict markers where both changed a text and one side's version of any other conflict, and
/// the paths it leaves in conflict, sorted, each once.
pub(crate) fn merged(
    dir: &Path,
    ours: &str,
    theirs: &str,
) -> Result<(String, Vec<String>), String> {
    let args = [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "-z",
        ours,
        theirs,
    ];
    let done = git(dir, &args).ok_or_else(|| format!("git cannot run in {}", dir.display()))?;
    // The tree, then each conflicted path, then an empty field before git's messages.
    let out = String::from_utf8_lossy(&done.stdout);
    let mut fields = out.split('\0');
    let tree = fields.next().unwrap_or_default().trim().to_string();
    let mut conflicted: Vec<String> = fields
        .take_while(|field| !field.is_empty())
        .map(str::to_string)
        .collect();
    conflicted.sort_unstable();
    conflicted.dedup();
    // 0 is a clean merge and 1 one with conflicts, the tree written either way; a status that
    // disagrees with the paths listed is not one to go by.
    let read = match done.status.code() {
        Some(0) => conflicted.is_empty(),
        Some(1) => !conflicted.is_empty(),
        _ => false,
    };
    if !read || tree.is_empty() {
        let said = String::from_utf8_lossy(&done.stderr);
        let said = said.lines().find(|l| !l.trim().is_empty());
        return Err(format!(
            "git merge-tree: {}",
            said.map_or("its answer cannot be read", str::trim)
        ));
    }
    Ok((tree, conflicted))
}

#[cfg(test)]
mod tests;
