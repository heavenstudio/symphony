//! Worktree listings, commit ancestry and repository-root discovery.

use super::{git, succeeds};
use std::path::{Path, PathBuf};

/// One worktree as `git worktree list --porcelain` lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listed {
    pub path: PathBuf,
    /// "" when it has none (a bare repository).
    pub head: String,
    /// `refs/heads/NAME`; none when detached.
    pub branch: Option<String>,
    /// The lock's reason, "" for a lock with none; none when it is not locked.
    pub locked: Option<String>,
    pub bare: bool,
    pub prunable: bool,
}

/// Whether `rev` is `main` or behind it.
pub fn ancestor(root: &Path, rev: &str, main: &str) -> bool {
    succeeds(root, &["merge-base", "--is-ancestor", rev, main])
}

/// The top of the worktree `cwd` is in, or git's own words for why there is none.
pub fn toplevel(cwd: &Path) -> Result<PathBuf, String> {
    let args = ["rev-parse", "--show-toplevel"];
    let done = git(cwd, &args)
        .ok_or_else(|| format!("git {}: {} cannot be read", args.join(" "), cwd.display()))?;
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr).trim().to_string();
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            if said.is_empty() { "failed" } else { &said }
        ));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&done.stdout).trim().to_string(),
    ))
}

/// Every worktree of `root`'s repository, the main checkout first, as git lists them.
pub fn worktrees(root: &Path) -> Result<Vec<Listed>, String> {
    let done = git(root, &["worktree", "list", "--porcelain"])
        .filter(|done| done.status.success())
        .ok_or("git worktree list --porcelain failed")?;
    Ok(parse_worktrees(&String::from_utf8_lossy(&done.stdout)))
}

/// `git worktree list --porcelain` read: one block per worktree, a line per attribute.
pub fn parse_worktrees(text: &str) -> Vec<Listed> {
    let mut found = Vec::new();
    for block in text.trim().split("\n\n") {
        let mut entry = Listed::default();
        let mut named = false;
        for line in block.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => {
                    entry.path = PathBuf::from(value);
                    named = true;
                }
                "HEAD" => entry.head = value.to_string(),
                "branch" => entry.branch = Some(value.to_string()),
                "locked" => entry.locked = Some(value.to_string()),
                "bare" => entry.bare = true,
                "prunable" => entry.prunable = true,
                _ => {}
            }
        }
        if named {
            found.push(entry);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_reads_each_worktrees_attributes() {
        let listed = parse_worktrees(
            "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\nworktree /w\nHEAD bbb\ndetached\nlocked claude agent x (pid 1 start y)\n\nworktree /g\nHEAD ccc\ndetached\nlocked\nprunable gitdir file points to non-existent location\n\nworktree /b\nbare\n",
        );
        assert_eq!(listed.len(), 4);
        assert_eq!(listed[0].branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(
            listed[1].locked.as_deref(),
            Some("claude agent x (pid 1 start y)")
        );
        assert_eq!(listed[1].branch, None);
        assert_eq!(
            (listed[2].locked.as_deref(), listed[2].prunable),
            (Some(""), true)
        );
        assert!(listed[3].bare && listed[3].head.is_empty());
    }
}
