//! Where a path may lead when an agent hands it to a recipe the allow rules run unasked: into the
//! project's repository on disk, and nowhere else.
//!
//! `symphony init` allows the board and reply recipes with no prompt (`just tasks`,
//! `just symphony-reply`, `just symphony-ask`, `just symphony-status`), so no prompt sees a path an
//! agent hands them. Each of those recipes sets [`MARKER`], and under it the command that writes
//! where it is told refuses a place outside the project's [`folders`], with one line naming it
//! ([`inside`]): the board's snapshot (`tasks export --out`), a reply's document
//! (`symphony reply PATH`), the ask store (`symphony ask --db`, `--root`) and the registers a tidy
//! rewrites (`symphony status --tidy --root`). Every link and `..` in the path is resolved first,
//! so neither leads it out. The binaries run by themselves, as the owner may, write wherever they
//! are told: no allow rule runs them unasked.

use std::path::{Component, Path, PathBuf};

/// What the recipes the allow rules pass set, `SYMPHONY_UNASKED=1`: under it a path a command
/// writes to stays in the project ([`inside`]).
pub const MARKER: &str = "SYMPHONY_UNASKED";

/// The folders a path from `cwd` may lead into, each resolved: the top of the checkout `cwd` is
/// in, its repository's git common dir, and the main checkout holding that common dir (from a
/// linked worktree, where the ask store and the board are); with no checkout, `cwd` itself.
pub fn folders(cwd: &Path) -> Vec<PathBuf> {
    let here = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let Some(top) = here
        .ancestors()
        .find(|folder| folder.join(".git").symlink_metadata().is_ok())
        .map(Path::to_path_buf)
    else {
        return vec![here];
    };
    let mut folders = vec![top.clone()];
    if let Ok(common) = crate::git::common_dir(&top) {
        let main = common
            .parent()
            .filter(|_| common.file_name() == Some(".git".as_ref()))
            .map(Path::to_path_buf);
        for folder in [Some(common), main].into_iter().flatten() {
            if !folders.iter().any(|known| folder.starts_with(known)) {
                folders.push(folder);
            }
        }
    }
    folders
}

/// `path`, from `cwd` when it is relative, with each link and `..` in it resolved as the system
/// follows them when it writes there; a part that is not there yet is kept as written, and a `..`
/// after it goes back over it. A link that leads nowhere is refused: a write through it would make
/// whatever it names.
///
/// # Errors
///
/// The refusal of such a link, one line naming it.
pub fn resolve(cwd: &Path, path: &Path) -> Result<PathBuf, String> {
    let mut at = if path.is_absolute() {
        PathBuf::new()
    } else {
        cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
    };
    for part in path.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => at.push(part.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                at.pop();
            }
            Component::Normal(name) => {
                at.push(name);
                match at.canonicalize() {
                    Ok(real) => at = real,
                    Err(_) if at.is_symlink() => {
                        return Err(format!(
                            "{} goes through {}, a link that leads nowhere: a write there would \
                             make what it names, wherever that is",
                            path.display(),
                            at.display()
                        ));
                    }
                    Err(_) => {}
                }
            }
        }
    }
    Ok(at)
}

/// `path` resolved ([`resolve`]), when it leads into one of the [`folders`] of `cwd`.
///
/// # Errors
///
/// One line naming the path, where it leads and the project it left.
pub fn inside(cwd: &Path, path: &Path) -> Result<PathBuf, String> {
    let resolved = resolve(cwd, path)?;
    let folders = folders(cwd);
    if folders.iter().any(|folder| resolved.starts_with(folder)) {
        return Ok(resolved);
    }
    let place = if resolved == path {
        format!("{} is", path.display())
    } else {
        format!("{} leads to {},", path.display(), resolved.display())
    };
    Err(format!(
        "{place} outside the project at {}: a recipe an agent runs unasked writes only in the \
         project, and the binary run by itself anywhere",
        folders[0].display()
    ))
}

/// [`inside`] when `marked` (the command ran under [`MARKER`]), else `path` from `cwd` as it is.
///
/// # Errors
///
/// As [`inside`].
pub fn checked(marked: bool, cwd: &Path, path: &Path) -> Result<PathBuf, String> {
    if marked {
        inside(cwd, path)
    } else {
        Ok(cwd.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A folder of its own under the temp folder, resolved, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("confine-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir.canonicalize().unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let done = crate::sys::output(
            crate::sys::command("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
                .arg(dir)
                .args(args),
        )
        .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
    }

    /// A repository at `dir/shop` with one commit, and a linked worktree of it at `dir/tree`.
    fn repository(dir: &Path) -> (PathBuf, PathBuf) {
        let main = dir.join("shop");
        std::fs::create_dir_all(main.join("docs")).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "first"]);
        git(&main, &["worktree", "add", "-q", "-b", "tree", "../tree"]);
        (main, dir.join("tree"))
    }

    #[test]
    fn a_path_keeps_what_is_missing_and_resolves_links_and_dot_dots() {
        let scratch = Scratch::new("resolve");
        let at = &scratch.0;
        std::fs::create_dir_all(at.join("real/deep")).unwrap();
        symlink(at.join("real"), at.join("link")).unwrap();
        for (path, leads) in [
            ("real/deep/x.md", at.join("real/deep/x.md")),
            ("./real/./x.md", at.join("real/x.md")),
            ("link/deep/x.md", at.join("real/deep/x.md")),
            ("new/newer/x.md", at.join("new/newer/x.md")),
            ("new/../x.md", at.join("x.md")),
            ("real/deep/../../..", at.parent().unwrap().to_path_buf()),
            ("link/..", at.clone()),
        ] {
            assert_eq!(resolve(at, Path::new(path)).unwrap(), leads, "{path}");
        }
        // An absolute path is its own, wherever the command runs.
        assert_eq!(
            resolve(Path::new("/"), &at.join("link/x.md")).unwrap(),
            at.join("real/x.md")
        );
        // A `..` past the root stays at the root, as the system reads it.
        assert_eq!(resolve(at, Path::new("/../../x")).unwrap(), Path::new("/x"));
    }

    #[test]
    fn a_link_that_leads_nowhere_is_refused_as_it_would_make_its_target() {
        let scratch = Scratch::new("dangling");
        let at = &scratch.0;
        symlink(at.join("gone/far"), at.join("dangling")).unwrap();
        for path in ["dangling", "dangling/x.md", "./dangling"] {
            let refused = resolve(at, Path::new(path)).unwrap_err();
            assert_eq!(
                refused,
                format!(
                    "{path} goes through {}, a link that leads nowhere: a write there would make \
                     what it names, wherever that is",
                    at.join("dangling").display()
                ),
                "{path}"
            );
        }
    }

    #[test]
    fn a_checkout_reaches_its_own_top_its_main_checkout_and_its_git_dir() {
        let scratch = Scratch::new("folders");
        let (main, tree) = repository(&scratch.0);
        assert_eq!(folders(&main.join("docs")), std::slice::from_ref(&main));
        assert_eq!(
            folders(&tree),
            [tree.clone(), main.join(".git"), main.clone()]
        );
        // A folder in no repository is its own.
        let alone = scratch.0.join("alone");
        std::fs::create_dir_all(&alone).unwrap();
        assert_eq!(folders(&alone), std::slice::from_ref(&alone));
    }

    #[test]
    fn a_path_inside_the_project_passes_and_one_leading_out_is_named() {
        let scratch = Scratch::new("inside");
        let (main, tree) = repository(&scratch.0);
        let out = scratch.0.join("elsewhere");
        std::fs::create_dir_all(&out).unwrap();
        symlink(&out, tree.join("out-link")).unwrap();
        // The worktree, the main checkout's artifacts (the ask store) and the git dir pass.
        for (path, leads) in [
            (
                "docs/history/tasks.md".to_string(),
                tree.join("docs/history/tasks.md"),
            ),
            (
                main.join("artifacts/asks/asks.db").display().to_string(),
                main.join("artifacts/asks/asks.db"),
            ),
            (
                "../shop/.git/sf-tasks/x".to_string(),
                main.join(".git/sf-tasks/x"),
            ),
        ] {
            assert_eq!(inside(&tree, Path::new(&path)).unwrap(), leads, "{path}");
        }
        for (path, leads) in [
            ("../elsewhere/board.md".to_string(), out.join("board.md")),
            ("out-link/board.md".to_string(), out.join("board.md")),
            ("docs/../../x.md".to_string(), scratch.0.join("x.md")),
            // `/etc` is itself a link on some systems: what it leads to is named.
            (
                "/etc/x.md".to_string(),
                Path::new("/etc").canonicalize().unwrap().join("x.md"),
            ),
            // A sibling whose name only starts as the project's does is not in it.
            (
                "../tree-not/x.md".to_string(),
                scratch.0.join("tree-not/x.md"),
            ),
        ] {
            // Where `/etc` is no link, that path is already where it leads (below).
            let place = if Path::new(&path) == leads {
                format!("{path} is")
            } else {
                format!("{path} leads to {},", leads.display())
            };
            assert_eq!(
                inside(&tree, Path::new(&path)).unwrap_err(),
                format!(
                    "{place} outside the project at {}: a recipe an agent runs unasked writes \
                     only in the project, and the binary run by itself anywhere",
                    tree.display()
                ),
                "{path}"
            );
        }
        // A path that is already where it leads is named once.
        let plain = out.join("board.md");
        assert_eq!(
            inside(&tree, &plain).unwrap_err(),
            format!(
                "{} is outside the project at {}: a recipe an agent runs unasked writes only in \
                 the project, and the binary run by itself anywhere",
                plain.display(),
                tree.display()
            )
        );
        // From the main checkout a worktree outside it is another place.
        assert!(inside(&main, &tree.join("docs/x.md")).is_err());
    }

    #[test]
    fn unmarked_a_path_is_taken_as_it_is() {
        let scratch = Scratch::new("unmarked");
        assert_eq!(
            checked(false, &scratch.0, Path::new("../anywhere/x.md")).unwrap(),
            scratch.0.join("../anywhere/x.md")
        );
        assert!(checked(true, &scratch.0, Path::new("../anywhere/x.md")).is_err());
        assert_eq!(
            checked(true, &scratch.0, Path::new("x.md")).unwrap(),
            scratch.0.join("x.md")
        );
    }
}
