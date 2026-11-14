//! Where a path may lead when an agent hands it to a recipe the allow rules run unasked: into the
//! checkout the command runs in, never one of its git folders, and from a linked worktree into the
//! main checkout's artifacts folder (the ask store's), and nowhere else.
//!
//! `symphony init` allows the board and reply recipes with no prompt (`just tasks`,
//! `just symphony-reply`, `just symphony-ask`, `just symphony-status`), so no prompt sees a path an
//! agent hands them. Each of those recipes sets [`MARKER`], and under it the command that writes
//! where it is told refuses a place its [`Places`] do not admit, with one line naming it
//! ([`inside`]): the board's snapshot (`tasks export --out`), a reply's document
//! (`symphony reply PATH`), the ask store (`symphony ask --db`, `--root`) and the registers a tidy
//! rewrites (`symphony status --tidy --root`). Every link and `..` in the path is resolved first,
//! so neither leads it out. A git folder is refused wherever it stands, and a `.git` part in any
//! case of its letters (git's own on a volume that ignores case): a hook, the config or a
//! worktree's `.git` file written there would run, or misdirect, every later git command. The rest
//! of the main checkout is refused too: no documented write goes there but the ask store a
//! callback names. The binaries run by themselves, as the owner may, write wherever they are told:
//! no allow rule runs them unasked.

use std::path::{Component, Path, PathBuf};

/// What the recipes the allow rules pass set, `SYMPHONY_UNASKED=1`: under it a path a command
/// writes to stays where [`Places`] admit ([`inside`]).
pub const MARKER: &str = "SYMPHONY_UNASKED";

/// Where a path from a command's folder may lead under [`MARKER`], each folder resolved.
#[derive(Debug, PartialEq, Eq)]
pub struct Places {
    /// The top of the checkout the command runs in; with no checkout, its folder.
    pub checkout: PathBuf,
    /// From a linked worktree its repository lists as its own, the main checkout's artifacts
    /// folder (`project::artifacts`, as `Project::artifact` reads it), where the ask store is.
    pub artifacts: Option<PathBuf>,
    /// The checkout's git dir and its repository's common dir, never written, wherever they are;
    /// a `.git` part below an admitted folder, in any case, is refused as well.
    pub git: Vec<PathBuf>,
}

impl Places {
    /// Whether `path`, resolved, is somewhere these admit. A `.git` part is refused in any case of
    /// its letters, as git reads it under `core.ignorecase`: on a volume that ignores case a new
    /// `.GIT` is the checkout's `.git` to git, though no existing folder resolved it so.
    pub fn admit(&self, path: &Path) -> bool {
        self.admit_in(&self.checkout, path)
            || self
                .artifacts
                .as_deref()
                .is_some_and(|folder| self.admit_in(folder, path))
    }

    /// Whether `path`, resolved, is in `folder`, resolved, on the terms [`Places::admit`] takes
    /// these on: no `.git` part below it in any case of its letters, and in none of these git
    /// folders. What a command admits beside these (a launch, the project's worktree base) it
    /// admits on them.
    pub fn admit_in(&self, folder: &Path, path: &Path) -> bool {
        path.strip_prefix(folder).is_ok_and(|rest| {
            !rest
                .components()
                .any(|part| part.as_os_str().eq_ignore_ascii_case(".git"))
        }) && !self.git.iter().any(|git| path.starts_with(git))
    }

    /// What they admit, as a refusal says it.
    pub fn said(&self) -> String {
        let artifacts = self
            .artifacts
            .as_ref()
            .map(|folder| {
                format!(
                    ", and the main checkout's artifacts folder at {}",
                    folder.display()
                )
            })
            .unwrap_or_default();
        format!(
            "only the checkout at {}, none of its git folders{artifacts}",
            self.checkout.display()
        )
    }
}

/// The places of a command run in `cwd` ([`Places`]).
pub fn places(cwd: &Path) -> Places {
    let here = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let Some(top) = here
        .ancestors()
        .find(|folder| folder.join(".git").symlink_metadata().is_ok())
        .map(Path::to_path_buf)
    else {
        return Places {
            checkout: here,
            artifacts: None,
            git: Vec::new(),
        };
    };
    // The checkout's own marker alone: an unreadable one never falls through to an enclosing
    // repository, nor asks git.
    let common = crate::git::checkout_common_dir(&top);
    let artifacts = common
        .as_deref()
        .and_then(|common| main_of(&top, common))
        .and_then(|main| {
            let folder = crate::project::artifacts(&main)?;
            resolve(Path::new("/"), &main.join(folder)).ok()
        });
    let mut git: Vec<PathBuf> = [gitdir(&top), common].into_iter().flatten().collect();
    git.dedup();
    Places {
        checkout: top,
        artifacts,
        git,
    }
}

/// The git dir of the checkout at `top`: its `.git` folder, or the folder its `.git` file names.
fn gitdir(top: &Path) -> Option<PathBuf> {
    let dot = top.join(".git");
    if dot.is_dir() {
        return dot.canonicalize().ok();
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    top.join(text.trim().strip_prefix("gitdir:")?.trim())
        .canonicalize()
        .ok()
}

/// The main checkout of the linked worktree at `top` whose repository's common dir is `common`:
/// the folder holding that common dir, taken only when the repository lists the worktree as its
/// own (its git dir is `common/worktrees/NAME`, whose `gitdir` file names `top/.git`). The
/// worktree's `.git` file is untracked, and a rewritten one would name another repository; that
/// repository does not list this worktree. None for the main checkout itself, or any other layout.
fn main_of(top: &Path, common: &Path) -> Option<PathBuf> {
    let dot = top.join(".git");
    if !crate::git::linked(top) || common.file_name()? != ".git" {
        return None;
    }
    let gitdir = gitdir(top)?;
    if gitdir.parent()? != common.join("worktrees") {
        return None;
    }
    // Git writes the back link absolute, or relative to the git dir under
    // `worktree.useRelativePaths`.
    let listed = std::fs::read_to_string(gitdir.join("gitdir")).ok()?;
    if gitdir.join(listed.trim()).canonicalize().ok()? != dot.canonicalize().ok()? {
        return None;
    }
    common.parent().map(Path::to_path_buf)
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

/// `path` resolved ([`resolve`]), when the [`places`] of `cwd` admit it.
///
/// # Errors
///
/// One line naming the path, where it leads and what a recipe may write instead.
pub fn inside(cwd: &Path, path: &Path) -> Result<PathBuf, String> {
    let resolved = resolve(cwd, path)?;
    let places = places(cwd);
    if places.admit(&resolved) {
        return Ok(resolved);
    }
    let place = if resolved == path {
        format!("{} is", path.display())
    } else {
        format!(
            "{} leads to {}, which is",
            path.display(),
            resolved.display()
        )
    };
    Err(format!(
        "{place} not a place a recipe an agent runs unasked writes: {}; the binary run by itself \
         writes anywhere",
        places.said()
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

    /// What [`inside`] says of `path`, which leads to `leads`, from a command whose places say
    /// `places`.
    fn refusal(path: &str, leads: &Path, places: &str) -> String {
        let place = if Path::new(path) == leads {
            format!("{path} is")
        } else {
            format!("{path} leads to {}, which is", leads.display())
        };
        format!(
            "{place} not a place a recipe an agent runs unasked writes: {places}; the binary run \
             by itself writes anywhere"
        )
    }

    /// Writes `text` over the file at `path`.
    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_checkout_reaches_its_own_top_and_from_a_worktree_the_main_artifacts() {
        let scratch = Scratch::new("places");
        let (main, tree) = repository(&scratch.0);
        assert_eq!(
            places(&main.join("docs")),
            Places {
                checkout: main.clone(),
                artifacts: None,
                git: vec![main.join(".git")],
            }
        );
        assert_eq!(
            places(&tree),
            Places {
                checkout: tree.clone(),
                artifacts: Some(main.join("artifacts")),
                git: vec![main.join(".git/worktrees/tree"), main.join(".git")],
            }
        );
        // The artifacts folder the main checkout's project file names, as the Symphony reads it.
        write(
            &main.join("symphony.toml"),
            "[project]\nartifacts = \"out/./kept\"\n",
        );
        assert_eq!(places(&tree).artifacts, Some(main.join("out/kept")));
        // One that leaves the project the Symphony refuses, and so does this.
        write(
            &main.join("symphony.toml"),
            "[project]\nartifacts = \"../x\"\n",
        );
        assert_eq!(places(&tree).artifacts, None);
        std::fs::remove_file(main.join("symphony.toml")).unwrap();
        // A folder in no repository is its own, with no git folder.
        let alone = scratch.0.join("alone");
        std::fs::create_dir_all(&alone).unwrap();
        assert_eq!(
            places(&alone),
            Places {
                checkout: alone.clone(),
                artifacts: None,
                git: Vec::new(),
            }
        );
    }

    #[test]
    fn a_worktree_reaches_the_main_checkout_only_when_its_repository_lists_it() {
        let scratch = Scratch::new("listed");
        let (main, tree) = repository(&scratch.0);
        git(&main, &["worktree", "add", "-q", "-b", "lamp", "../lamp"]);
        let other = scratch.0.join("other");
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q", "-b", "main"]);
        // The back link git writes relative to the worktree's git dir is read as well.
        write(
            &main.join(".git/worktrees/tree/gitdir"),
            "../../../../tree/.git\n",
        );
        assert_eq!(places(&tree).artifacts, Some(main.join("artifacts")));
        // A `.git` file rewritten to name another repository reaches nothing of it, nor of its own.
        write(
            &tree.join(".git"),
            &format!("gitdir: {}\n", other.join(".git").display()),
        );
        let named = places(&tree);
        assert_eq!(named.artifacts, None);
        assert_eq!(named.git, vec![other.join(".git")]);
        for path in [
            main.join("artifacts/board.md"),
            other.join("artifacts/board.md"),
            other.join(".git/hooks/pre-commit"),
        ] {
            assert!(inside(&tree, &path).is_err(), "{}", path.display());
        }
        // Nor does one naming another worktree's git dir, whose back link names that worktree.
        write(
            &tree.join(".git"),
            &format!("gitdir: {}\n", main.join(".git/worktrees/lamp").display()),
        );
        assert_eq!(places(&tree).artifacts, None);
        assert!(inside(&tree, &main.join("artifacts/board.md")).is_err());
    }

    #[test]
    fn a_path_in_the_checkout_passes_and_any_other_place_is_named() {
        let scratch = Scratch::new("inside");
        let (main, tree) = repository(&scratch.0);
        let out = scratch.0.join("elsewhere");
        std::fs::create_dir_all(&out).unwrap();
        symlink(&out, tree.join("out-link")).unwrap();
        std::fs::create_dir_all(tree.join("sub/.git")).unwrap();
        // The worktree and the main checkout's artifacts folder (the ask store's) pass.
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
                "../shop/artifacts/board.md".to_string(),
                main.join("artifacts/board.md"),
            ),
        ] {
            assert_eq!(inside(&tree, Path::new(&path)).unwrap(), leads, "{path}");
        }
        let from_tree = format!(
            "only the checkout at {}, none of its git folders, and the main checkout's artifacts \
             folder at {}",
            tree.display(),
            main.join("artifacts").display()
        );
        for (path, leads) in [
            // Every git folder: the common dir's hooks and config, the worktree's git dir and its
            // `.git` file, a `.git` below the artifacts folder or in a repository in the checkout.
            (
                "../shop/.git/hooks/pre-commit".to_string(),
                main.join(".git/hooks/pre-commit"),
            ),
            ("../shop/.git/config".to_string(), main.join(".git/config")),
            (
                "../shop/.git/symphony/tasks/x".to_string(),
                main.join(".git/symphony/tasks/x"),
            ),
            (
                "../shop/.git/worktrees/tree/HEAD".to_string(),
                main.join(".git/worktrees/tree/HEAD"),
            ),
            (".git".to_string(), tree.join(".git")),
            (
                "../shop/artifacts/.git/x".to_string(),
                main.join("artifacts/.git/x"),
            ),
            ("sub/.git/config".to_string(), tree.join("sub/.git/config")),
            // A new `.git` in any case, which a volume that ignores case gives git as its own.
            ("docs/.GIT".to_string(), tree.join("docs/.GIT")),
            (
                "../shop/artifacts/.Git/x".to_string(),
                main.join("artifacts/.Git/x"),
            ),
            // The rest of the main checkout, however it is reached.
            ("../shop/docs/x.md".to_string(), main.join("docs/x.md")),
            ("../shop/justfile".to_string(), main.join("justfile")),
            (
                "../shop/artifacts/../docs/x.md".to_string(),
                main.join("docs/x.md"),
            ),
            // Anywhere else.
            ("../elsewhere/board.md".to_string(), out.join("board.md")),
            ("out-link/board.md".to_string(), out.join("board.md")),
            ("docs/../../x.md".to_string(), scratch.0.join("x.md")),
            // `/etc` is itself a link on some systems: what it leads to is named.
            (
                "/etc/x.md".to_string(),
                Path::new("/etc").canonicalize().unwrap().join("x.md"),
            ),
            // A sibling whose name only starts as the checkout's does is not in it.
            (
                "../tree-not/x.md".to_string(),
                scratch.0.join("tree-not/x.md"),
            ),
        ] {
            assert_eq!(
                inside(&tree, Path::new(&path)).unwrap_err(),
                refusal(&path, &leads, &from_tree),
                "{path}"
            );
        }
        // A path that is already where it leads is named once.
        let plain = out.join("board.md");
        let said = plain.display().to_string();
        assert_eq!(
            inside(&tree, &plain).unwrap_err(),
            refusal(&said, &plain, &from_tree)
        );
        // The main checkout writes in itself, its artifacts folder too, but never in its `.git`,
        // nor in a worktree beside it.
        let from_main = format!(
            "only the checkout at {}, none of its git folders",
            main.display()
        );
        for path in ["docs/x.md", "artifacts/board.md"] {
            assert_eq!(inside(&main, Path::new(path)).unwrap(), main.join(path));
        }
        for (path, leads) in [
            (".git/config".to_string(), main.join(".git/config")),
            ("docs/.GIT".to_string(), main.join("docs/.GIT")),
            (
                "docs/../.git/hooks/pre-commit".to_string(),
                main.join(".git/hooks/pre-commit"),
            ),
            ("../tree/docs/x.md".to_string(), tree.join("docs/x.md")),
        ] {
            assert_eq!(
                inside(&main, Path::new(&path)).unwrap_err(),
                refusal(&path, &leads, &from_main),
                "{path}"
            );
        }
    }

    #[test]
    fn a_folder_admitted_beside_the_places_is_on_their_terms() {
        let scratch = Scratch::new("beside");
        let (main, tree) = repository(&scratch.0);
        let base = scratch.0.join("shop-wt");
        let places = places(&tree);
        for (path, admitted) in [
            (base.join("lamp"), true),
            (base.join("lamp/docs/x.md"), true),
            (base.clone(), true),
            // Its git folders, and a `.git` part below it in any case, as for the places' own.
            (base.join("lamp/.git"), false),
            (base.join("lamp/.GIT/config"), false),
            (base.join(".git"), false),
            // Beside it, above it, or a sibling whose name only starts as its does.
            (scratch.0.join("shop-wt-not/x"), false),
            (scratch.0.clone(), false),
        ] {
            assert_eq!(
                places.admit_in(&base, &path),
                admitted,
                "{}",
                path.display()
            );
        }
        // A folder admitted beside them never reaches their git folders, wherever it is.
        let around = scratch.0.clone();
        for git in [main.join(".git/hooks"), main.join(".git/worktrees/tree")] {
            assert!(!places.admit_in(&around, &git), "{}", git.display());
        }
        assert!(places.admit_in(&around, &main.join("docs")));
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
