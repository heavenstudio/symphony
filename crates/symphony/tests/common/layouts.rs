//! The git layouts a checkout can have, made under a temporary folder for the tests that tell a
//! linked worktree from a main checkout: a main checkout and its linked worktree, and the checkouts
//! whose `.git` is a file without being a linked worktree's.
#![allow(dead_code, reason = "each test binary uses its own share of these")]
use std::path::{Path, PathBuf};

/// `git ARGS` in `at`, which must succeed, with no configuration of the machine's to sign or hook.
pub fn git(at: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "init.defaultBranch=main"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(at)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// `text` written at `root`/`rel`, its folders made.
fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A repository with one commit under `root`/`name`, its files written first.
fn committed(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    for (rel, text) in files {
        write(root, &format!("{name}/{rel}"), text);
    }
    let top = root.join(name);
    git(&top, &["init", "-q"]);
    git(&top, &["add", "."]);
    git(&top, &["commit", "-qm", name]);
    top
}

/// A repository under `root` with one commit, the project `lantern`: its main checkout `main/` and
/// a linked worktree `wt/` on a branch, each with a `docs/` folder: the two checkouts, canonical.
pub fn repository(root: &Path) -> (PathBuf, PathBuf) {
    let main = committed(
        root,
        "main",
        &[
            ("docs/README.md", "# Docs\n"),
            (
                "symphony.toml",
                "[project]\nid = \"lantern\"\nname = \"Lantern\"\n",
            ),
        ],
    );
    let linked = root.join("wt");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/x",
            linked.to_str().unwrap(),
        ],
    );
    (main.canonicalize().unwrap(), linked.canonicalize().unwrap())
}

/// The checkouts [`others`] makes, canonical.
pub struct Others {
    /// The submodule `outer/sub`: its `.git` file names `../.git/modules/sub`, its repository's
    /// gitdir, which has no `commondir`. It is its repository's main checkout.
    pub submodule: PathBuf,
    /// `sep`, made by `git init --separate-git-dir sep.gitdir`: its `.git` file names its
    /// repository's gitdir, as a submodule's does. It is its repository's main checkout.
    pub separate: PathBuf,
    /// The bare repository `bare.git`, which has no main checkout.
    pub bare: PathBuf,
    /// `bare-wt`, a linked worktree of `bare.git` on a branch: its gitdir has a `commondir`.
    pub bare_worktree: PathBuf,
}

/// Under `root`, a checkout of each layout whose `.git` is a file other than a main checkout's
/// linked worktree, each with a `docs/` folder and a `symphony.toml` of its own project.
pub fn others(root: &Path) -> Others {
    let project = |id: &str| format!("[project]\nid = \"{id}\"\nname = \"{id}\"\n");
    let lib = committed(
        root,
        "lib",
        &[
            ("docs/README.md", "# Lib\n"),
            ("symphony.toml", &project("lib")),
        ],
    );
    let outer = committed(root, "outer", &[("README.md", "# Outer\n")]);
    git(
        &outer,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            lib.to_str().unwrap(),
            "sub",
        ],
    );
    write(root, "sep/docs/README.md", "# Sep\n");
    write(root, "sep/symphony.toml", &project("sep"));
    let separate = root.join("sep");
    let gitdir = root.join("sep.gitdir");
    git(
        &separate,
        &["init", "-q", "--separate-git-dir", gitdir.to_str().unwrap()],
    );
    git(&separate, &["add", "."]);
    git(&separate, &["commit", "-qm", "sep"]);
    let bare = root.join("bare.git");
    git(
        root,
        &[
            "clone",
            "-q",
            "--bare",
            lib.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let bare_worktree = root.join("bare-wt");
    git(
        &bare,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/x",
            bare_worktree.to_str().unwrap(),
        ],
    );
    Others {
        submodule: outer.join("sub").canonicalize().unwrap(),
        separate: separate.canonicalize().unwrap(),
        bare: bare.canonicalize().unwrap(),
        bare_worktree: bare_worktree.canonicalize().unwrap(),
    }
}
