//! What reached a project's default branch in each hour of a day file, from two cheap reads of the
//! branch since the roll-up's window opened ([`read`]):
//!
//! - its line of first parents, from its tip, with the lines each commit there changed, a merge by
//!   what it brought against its first parent: a merge there is a landing, and those lines are the
//!   lines that reached the branch;
//! - every commit it holds, by its parents, commit time and subject alone, no diff.
//!
//! One read of every commit with its lines would diff each commit of each branch merged, only to
//! count those lines again with the merge: 15.7 s of git's CPU over two days of this repository
//! (770 commits, 2026-11-12, at load 120), 2.3 s with the paths below left to git, where the two
//! reads take 0.24 s. A commit of a branch it merged counts as a commit in the hour it was made; a
//! merge off the first parents' line (a branch taking the default branch in) counts as neither a
//! commit nor a landing. What [`excluded`] names is left out of the lines, by git itself: each
//! generator's outputs, which the land's regenerator commits (the client distribution here), and
//! lock files by name. A binary file's change has no lines. A commit made in a day whose file was
//! final before the commit reached the branch is in no day file: the files are read forward, never
//! rewritten.

use crate::project::Project;
use std::path::Path;
use symphony_api::reports::{GitCounts, GitRead, ReportDay};
use symphony_board::land::owns;

/// The lock files whose lines are not counted, by file name, wherever they are.
pub const LOCKS: [&str; 13] = [
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "Gemfile.lock",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
];

/// The record of each commit: `hash\x1fparents\x1fcommit time\x1fsubject`, after a `\x1e`.
const FORMAT: &str = "--format=%x1e%H%x1f%P%x1f%ct%x1f%s";

/// The read of the line of first parents: every commit on it, those whose lines the pathspecs leave
/// out too (`--full-history --sparse`, which a pathspec would otherwise drop), each with its lines.
const LINE: &[&str] = &[
    "--first-parent",
    "--full-history",
    "--sparse",
    "--no-renames",
    "--diff-merges=first-parent",
    "--numstat",
];

/// One commit of a read, its lines those outside [`excluded`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    /// Its commit time.
    pub at: f64,
    pub subject: String,
    pub added: u64,
    pub removed: u64,
}

/// The two reads ([`read`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    /// The branch's line of first parents, its tip first, each with its lines.
    pub line: Vec<Commit>,
    /// Every commit the branch holds, with no lines.
    pub commits: Vec<Commit>,
}

/// The paths whose lines are not counted in `project`: each generator's outputs, as the project file
/// names them, then [`LOCKS`].
pub fn excluded(project: &Project) -> Vec<String> {
    let mut outputs: Vec<String> = project
        .land
        .generators
        .iter()
        .chain(&project.verdicts.generators)
        .map(|generator| generator.outputs.clone())
        .filter(|outputs| !outputs.is_empty())
        .collect();
    outputs.sort();
    outputs.dedup();
    outputs.extend(LOCKS.iter().map(|name| (*name).to_string()));
    outputs
}

/// Whether `path`'s lines are left out by `excluded` ([`excluded`]): inside a generator's outputs,
/// or a lock file by its name.
fn left_out(excluded: &[String], path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    excluded
        .iter()
        .any(|out| (LOCKS.contains(&out.as_str()) && out == name) || owns(out, path))
}

/// The pathspecs that keep git from diffing what `excluded` names: the whole tree but each output,
/// as written, and each lock file at any depth.
fn pathspecs(excluded: &[String]) -> Vec<String> {
    let mut specs = vec![".".to_string()];
    specs.extend(excluded.iter().map(|out| {
        if LOCKS.contains(&out.as_str()) {
            format!(":(exclude,glob)**/{out}")
        } else {
            format!(":(exclude,literal){out}")
        }
    }));
    specs
}

/// `git log` of `branch` in the repository at `root` since `since`, `args` before the branch and
/// `paths` after it.
fn log(
    root: &Path,
    branch: &str,
    since: f64,
    args: &[&str],
    paths: &[String],
) -> Result<String, String> {
    let mut git = crate::children::command("git");
    // The repository is the one at `root`, whatever the server was started from.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
    ] {
        git.env_remove(name);
    }
    let done = symphony_board::sys::output(
        git.env("GIT_OPTIONAL_LOCKS", "0")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "core.quotePath=false",
                "-c",
                "log.showSignature=false",
                "log",
                "--no-color",
                FORMAT,
            ])
            .arg(format!("--since=@{}", since.floor()))
            .args(args)
            .arg("--end-of-options")
            .arg(format!("refs/heads/{branch}"))
            .arg("--")
            .args(paths),
    )
    .map_err(|e| format!("git log could not start: {e}"))?;
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr);
        return Err(format!("git log of {branch} failed: {}", said.trim()));
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// The two reads of `branch` since `since`, the lines of what `excluded` names left out.
pub fn read(root: &Path, branch: &str, since: f64, excluded: &[String]) -> Result<Read, String> {
    let line = log(root, branch, since, LINE, &pathspecs(excluded))?;
    let commits = log(root, branch, since, &[], &[])?;
    Ok(Read {
        line: parse(&line, excluded),
        commits: parse(&commits, excluded),
    })
}

/// The commits of a read's `text`, in its order, their lines outside `excluded`.
pub fn parse(text: &str, excluded: &[String]) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|record| {
            let mut lines = record.lines();
            let mut head = lines.next()?.splitn(4, '\x1f');
            let hash = head.next()?.trim().to_string();
            let parents = head
                .next()?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let at = head.next()?.trim().parse::<f64>().ok()?;
            let subject = head.next().unwrap_or("").to_string();
            let mut commit = Commit {
                hash,
                parents,
                at,
                subject,
                ..Commit::default()
            };
            for line in lines {
                let mut fields = line.splitn(3, '\t');
                let (Some(added), Some(removed), Some(path)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    continue;
                };
                let (Ok(added), Ok(removed)) = (added.parse::<u64>(), removed.parse::<u64>())
                else {
                    continue; // a binary file: `-` lines
                };
                if !left_out(excluded, path) {
                    commit.added += added;
                    commit.removed += removed;
                }
            }
            (!commit.hash.is_empty()).then_some(commit)
        })
        .collect()
}

/// Fill `day`'s hours from `read` ([`read`]), `from` naming what it was read from.
pub fn fill(day: &mut ReportDay, read: &Read, from: GitRead) {
    let until = day.until;
    for hour in &mut day.hours {
        let to = (hour.start + crate::reports::HOUR).min(until);
        let within = |commit: &&Commit| hour.start <= commit.at && commit.at < to;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let line: Vec<&Commit> = read.line.iter().filter(within).collect();
        let made: Vec<&Commit> = read.commits.iter().filter(within).collect();
        hour.git = Some(GitCounts {
            commits: count(made.iter().filter(|c| c.parents.len() < 2).count()),
            merges: count(line.iter().filter(|c| c.parents.len() > 1).count()),
            added: line.iter().map(|c| c.added).sum(),
            removed: line.iter().map(|c| c.removed).sum(),
            reverts: count(
                made.iter()
                    .filter(|c| c.subject.starts_with("Revert \""))
                    .count(),
            ),
        });
    }
    day.git = Some(from);
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
