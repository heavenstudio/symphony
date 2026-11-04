//! Where a start runs from: the repository's git common dir (where the one server's lock and record
//! live), its main checkout (the root the one server serves, whatever checkout started it) and, from
//! a branch's worktree, the tree id its pages have under `/@<tree id>/` (the branch with `/` as
//! `~`).

use std::path::{Path, PathBuf};

/// The checkout a start runs in.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkout {
    pub common: PathBuf,
    pub top: PathBuf,
    pub main: PathBuf,
    pub tree_id: Option<String>,
}

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// The entries of `git worktree list --porcelain`, the main checkout first.
pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    porcelain
        .split("\n\n")
        .filter_map(|block| {
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(found) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(found));
                } else if let Some(found) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(found.to_string());
                }
            }
            Some(Worktree {
                path: path?,
                branch,
            })
        })
        .collect()
}

/// The tree id of the worktree at `top` among `trees`: none for the main checkout, a detached
/// worktree or one on its project's default branch (main), which the app serves at `/`.
pub fn tree_id(top: &Path, trees: &[Worktree]) -> Option<String> {
    let (main, others) = trees.split_first()?;
    if main.path == top {
        return None;
    }
    let branch = others.iter().find(|t| t.path == top)?.branch.as_deref()?;
    (branch != symphony::project::Project::of(&main.path).default_branch)
        .then(|| branch.replace('/', "~"))
}

/// `git ARGS` in `dir`, on the repository there whatever the environment names (a hook's GIT_DIR).
/// Read through [`tasks::sys::output`]: a census child spawned meanwhile never holds it open.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let done = crate::cli::startup(&format!("repository {}", args.join(" ")), || {
        tasks::sys::output(
            tasks::sys::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_COMMON_DIR"),
        )
        .map_err(|e| format!("git {}: {e}", args.join(" ")))
    })?;
    if !done.status.success() {
        return Err(format!(
            "{} is in no repository: {}",
            dir.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// The path as the file system resolves it (macOS's `/var` is `/private/var`), else as it is.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The checkout `dir` is in, as git says; an error when it is in none.
pub fn checkout(dir: &Path) -> Result<Checkout, String> {
    let said = git(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--show-toplevel",
        ],
    )?;
    let mut lines = said.lines();
    let (Some(common), Some(top)) = (lines.next(), lines.next()) else {
        return Err(format!("{} is in no checkout", dir.display()));
    };
    let trees: Vec<Worktree> = parse_worktrees(&git(dir, &["worktree", "list", "--porcelain"])?)
        .into_iter()
        .map(|tree| Worktree {
            path: canonical(&tree.path),
            ..tree
        })
        .collect();
    let main = trees
        .first()
        .map(|tree| tree.path.clone())
        .ok_or("git lists no worktree")?;
    let top = canonical(Path::new(top));
    Ok(Checkout {
        common: canonical(Path::new(common)),
        tree_id: tree_id(&top, &trees),
        top,
        main,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "worktree /repo\nHEAD 1111\nbranch refs/heads/main\n\n\
                        worktree /wt/one\nHEAD 2222\nbranch refs/heads/feat/one\n\n\
                        worktree /wt/loose\nHEAD 3333\ndetached\n\n\
                        worktree /wt/main-too\nHEAD 1111\nbranch refs/heads/main\n\n\
                        worktree /wt/gone\nHEAD 4444\nbranch refs/heads/old\nprunable gitdir file points to non-existent location\n";

    #[test]
    fn the_listing_is_read_in_order_with_each_branch() {
        let trees = parse_worktrees(LIST);
        assert_eq!(trees.len(), 5);
        assert_eq!(trees[0].path, PathBuf::from("/repo"));
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].branch.as_deref(), Some("feat/one"));
        assert_eq!(trees[2].branch, None);
    }

    #[test]
    fn a_branch_worktree_has_its_branch_as_tree_id_and_the_rest_have_none() {
        let trees = parse_worktrees(LIST);
        assert_eq!(
            tree_id(Path::new("/wt/one"), &trees).as_deref(),
            Some("feat~one")
        );
        assert_eq!(tree_id(Path::new("/repo"), &trees), None);
        assert_eq!(tree_id(Path::new("/wt/loose"), &trees), None);
        assert_eq!(tree_id(Path::new("/wt/main-too"), &trees), None);
        assert_eq!(tree_id(Path::new("/elsewhere"), &trees), None);
    }

    #[test]
    fn a_directory_outside_any_repository_is_no_checkout() {
        assert!(checkout(Path::new("/")).is_err());
    }
}
