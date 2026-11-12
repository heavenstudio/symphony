//! A finished worktree's evidence, set aside before the worktree goes. What git ignores under its
//! `artifacts/` (a task's pictures its thread names, a test's leftovers, a fixture repository) is
//! moved into the main checkout's [`SWEPT`] folder, under the worktree's folder name, so removing
//! the worktree loses none of it; a removal git then refuses moves it back.

use super::git;
use std::fs;
use std::path::{Path, PathBuf};

/// Where a removed worktree's artifacts go, in the main checkout.
pub const SWEPT: &str = "artifacts/swept";
/// The folder a worktree's evidence is in.
const ARTIFACTS: &str = "artifacts/";

/// Where the artifacts of worktree `tree` go in the main checkout `main`: `artifacts/swept/NAME`,
/// NAME the worktree's folder name, then `NAME-2`, `NAME-3`… while one is taken.
pub fn destination(main: &Path, tree: &Path) -> Option<PathBuf> {
    let name = tree.file_name()?.to_string_lossy().into_owned();
    let swept = main.join(SWEPT);
    (1..1000)
        .map(|n| {
            if n == 1 {
                swept.join(&name)
            } else {
                swept.join(format!("{name}-{n}"))
            }
        })
        .find(|at| fs::symlink_metadata(at).is_err())
}

/// What git ignores under `artifacts/` in worktree `tree`, each as it lists it (`artifacts/` for a
/// folder ignored whole); none when git cannot say. A worktree with no `artifacts` has nothing to
/// list, and git is not asked.
pub fn entries(tree: &Path) -> Option<Vec<String>> {
    if fs::symlink_metadata(tree.join("artifacts")).is_err() {
        return Some(Vec::new());
    }
    let done = git::read(
        tree,
        &[
            "status",
            "--porcelain",
            "-z",
            "--ignored=matching",
            "--untracked-files=all",
            "--",
            ARTIFACTS,
        ],
    )?;
    let text = String::from_utf8(done.stdout).ok()?;
    Some(
        text.split('\0')
            .filter_map(|record| record.strip_prefix("!! "))
            .filter(|name| name.starts_with(ARTIFACTS) || *name == "artifacts")
            .map(str::to_string)
            .collect(),
    )
}

/// Where `entry` of a worktree goes under `to`: the folder itself for `artifacts/`, else its path
/// below `artifacts/`.
fn target(to: &Path, entry: &str) -> PathBuf {
    let below = entry
        .strip_prefix(ARTIFACTS)
        .unwrap_or("")
        .trim_end_matches('/');
    if below.is_empty() {
        to.to_path_buf()
    } else {
        to.join(below)
    }
}

/// A worktree's artifacts, moved: what went where, to put back.
#[derive(Debug)]
pub struct Aside {
    /// The folder they went to.
    pub to: PathBuf,
    moved: Vec<(PathBuf, PathBuf)>,
}

impl Aside {
    /// Move what git ignores under worktree `tree`'s `artifacts/` into the main checkout `main`;
    /// none when there is nothing to move. Refused, with every entry put back, when git cannot
    /// list them or one cannot be moved (another disk, say): the worktree then stays.
    pub fn take(main: &Path, tree: &Path) -> Result<Option<Aside>, String> {
        let entries =
            entries(tree).ok_or_else(|| "git cannot list what its artifacts/ holds".to_string())?;
        if entries.is_empty() {
            return Ok(None);
        }
        let to = destination(main, tree)
            .ok_or_else(|| format!("no free name under {}", main.join(SWEPT).display()))?;
        let mut aside = Aside {
            to: to.clone(),
            moved: Vec::new(),
        };
        for entry in &entries {
            let from = tree.join(entry.trim_end_matches('/'));
            let into = target(&to, entry);
            let made = into.parent().map_or(Ok(()), fs::create_dir_all);
            if let Err(error) = made.and_then(|()| fs::rename(&from, &into)) {
                aside.put_back();
                return Err(format!(
                    "{} cannot be moved to {}: {error}",
                    from.display(),
                    into.display()
                ));
            }
            aside.moved.push((from, into));
        }
        Ok(Some(aside))
    }

    /// Move every entry back where it was: the removal it was set aside for did not happen. One that
    /// cannot go back stays where it went, kept all the same.
    pub fn put_back(self) {
        for (from, into) in self.moved.iter().rev() {
            let _ = fs::rename(into, from);
        }
        // The folders made for them, now empty, go; one that is not empty stays.
        let _ = fs::remove_dir(&self.to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_taken_gets_the_next_number() {
        let main = std::env::temp_dir().join(format!("sf-sweep-aside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&main);
        let tree = Path::new("/wt/agent-a5a6");
        assert_eq!(
            destination(&main, tree),
            Some(main.join("artifacts/swept/agent-a5a6"))
        );
        fs::create_dir_all(main.join("artifacts/swept/agent-a5a6")).unwrap();
        fs::write(main.join("artifacts/swept/agent-a5a6-2"), "a file\n").unwrap();
        assert_eq!(
            destination(&main, tree),
            Some(main.join("artifacts/swept/agent-a5a6-3"))
        );
        assert_eq!(destination(&main, Path::new("/")), None);
        fs::remove_dir_all(&main).unwrap();
    }

    /// A scratch repository at `root` that ignores `/artifacts/` and tracks `artifacts/kept.md`.
    fn repo(root: &Path) {
        let _ = fs::remove_dir_all(root);
        fs::create_dir_all(root.join("artifacts")).unwrap();
        fs::write(
            root.join(".gitignore"),
            "/artifacts/*\n!/artifacts/kept.md\n",
        )
        .unwrap();
        fs::write(root.join("artifacts/kept.md"), "tracked\n").unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        ] {
            let done = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(done.status.success(), "git {args:?}: {done:?}");
        }
    }

    #[test]
    fn nothing_ignored_under_artifacts_sets_nothing_aside() {
        let base = std::env::temp_dir().join(format!("sf-sweep-aside-none-{}", std::process::id()));
        let (main, tree) = (base.join("main"), base.join("tree"));
        repo(&tree);
        assert_eq!(entries(&tree), Some(vec![]));
        assert!(Aside::take(&main, &tree).unwrap().is_none());
        assert!(!main.exists(), "nothing was made in the main checkout");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_git_cannot_list_refuses_the_set_aside() {
        let base =
            std::env::temp_dir().join(format!("sf-sweep-aside-nogit-{}", std::process::id()));
        let tree = base.join("tree");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(tree.join("artifacts")).unwrap();
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        assert_eq!(entries(&tree), None);
        assert_eq!(
            Aside::take(&base.join("main"), &tree).unwrap_err(),
            "git cannot list what its artifacts/ holds"
        );
        assert!(tree.join("artifacts/a.png").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_entry_that_cannot_move_stays_where_it_was() {
        let base =
            std::env::temp_dir().join(format!("sf-sweep-aside-stuck-{}", std::process::id()));
        let (main, tree) = (base.join("main"), base.join("tree"));
        repo(&tree);
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        fs::write(tree.join("artifacts/b.png"), "b\n").unwrap();
        assert_eq!(
            entries(&tree),
            Some(vec![
                "artifacts/a.png".to_string(),
                "artifacts/b.png".to_string()
            ])
        );
        // The main checkout's swept folder is a file: no folder can be made below it.
        fs::create_dir_all(main.join("artifacts")).unwrap();
        fs::write(main.join(SWEPT), "not a folder\n").unwrap();
        let refused = Aside::take(&main, &tree).unwrap_err();
        assert!(
            refused.starts_with(&format!(
                "{} cannot be moved to {}: ",
                tree.join("artifacts/a.png").display(),
                main.join(SWEPT).join("tree/a.png").display()
            )),
            "{refused}"
        );
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/a.png")).unwrap(),
            "a\n"
        );
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/b.png")).unwrap(),
            "b\n"
        );
        assert_eq!(
            fs::read_to_string(main.join(SWEPT)).unwrap(),
            "not a folder\n"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn what_was_set_aside_goes_back() {
        let base = std::env::temp_dir().join(format!("sf-sweep-aside-back-{}", std::process::id()));
        let (main, tree) = (base.join("main"), base.join("tree"));
        repo(&tree);
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        fs::create_dir_all(tree.join("artifacts/shots")).unwrap();
        fs::write(tree.join("artifacts/shots/b.png"), "b\n").unwrap();
        let aside = Aside::take(&main, &tree).unwrap().unwrap();
        assert_eq!(aside.to, main.join(SWEPT).join("tree"));
        assert_eq!(fs::read_to_string(aside.to.join("a.png")).unwrap(), "a\n");
        assert_eq!(
            fs::read_to_string(aside.to.join("shots/b.png")).unwrap(),
            "b\n"
        );
        assert!(!tree.join("artifacts/a.png").exists());
        assert!(
            tree.join("artifacts/kept.md").exists(),
            "a tracked file stays"
        );
        aside.put_back();
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/a.png")).unwrap(),
            "a\n"
        );
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/shots/b.png")).unwrap(),
            "b\n"
        );
        assert!(
            !main.join(SWEPT).join("tree").exists(),
            "the folder made for them goes"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_entry_goes_below_the_folder_it_is_set_aside_in() {
        let to = Path::new("/main/artifacts/swept/wt");
        assert_eq!(target(to, "artifacts/"), to);
        assert_eq!(target(to, "artifacts"), to);
        assert_eq!(target(to, "artifacts/solar-swift/"), to.join("solar-swift"));
        assert_eq!(target(to, "artifacts/a/b.png"), to.join("a/b.png"));
    }
}
