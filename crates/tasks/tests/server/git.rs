//! The git common dir. A folder of its own per test, outside this repository (the build's own
//! temporary folder is inside it).

use std::path::{Path, PathBuf};
use symphony_board::git::{checkout, checkout_common_dir, common_dir, linked};
use symphony_board::sys::Command;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let dir = std::env::temp_dir()
            .join(format!("sf-git-{}-{name}", std::process::id()))
            .canonicalize_or_make();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

trait Make {
    fn canonicalize_or_make(self) -> PathBuf;
}

impl Make for PathBuf {
    fn canonicalize_or_make(self) -> PathBuf {
        let _ = std::fs::remove_dir_all(&self);
        std::fs::create_dir_all(&self).unwrap();
        self.canonicalize().unwrap()
    }
}

fn git(args: &[&str]) {
    let done = symphony_board::sys::output(
        Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn worktrees_share_it() {
    let t = Temp::new("worktrees");
    let (main, other) = (t.0.join("main"), t.0.join("other"));
    git(&["init", "-q", path(&main)]);
    git(&[
        "-C",
        path(&main),
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "x",
    ]);
    git(&["-C", path(&main), "worktree", "add", "-q", path(&other)]);
    std::fs::create_dir(other.join("tools")).unwrap();
    for at in [&main, &other, &other.join("tools")] {
        assert_eq!(
            common_dir(at).unwrap(),
            main.join(".git"),
            "{}",
            at.display()
        );
    }
}

#[test]
fn it_is_read_from_the_files_without_running_git() {
    // A worktree's files with nothing git itself could open behind them: only reading them finds it.
    let t = Temp::new("files");
    let gitdir = t.0.join("common").join("worktrees").join("w");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    let tree = t.0.join("tree");
    std::fs::create_dir_all(tree.join("deep")).unwrap();
    std::fs::write(tree.join(".git"), "gitdir: ../common/worktrees/w\n").unwrap();
    assert_eq!(common_dir(&tree.join("deep")).unwrap(), t.0.join("common"));
}

#[test]
fn a_layout_it_cannot_read_asks_git() {
    let t = Temp::new("bare");
    let bare = t.0.join("board.git");
    git(&["init", "-q", "--bare", path(&bare)]);
    assert_eq!(common_dir(&bare).unwrap(), bare);
}

#[test]
fn outside_any_repository_is_an_error_naming_where() {
    let t = Temp::new("outside");
    let error = common_dir(&t.0).unwrap_err();
    assert!(error.to_string().contains(path(&t.0)), "{error}");
}

/// Placement and project lookup must stop at the same marker, even when an enclosing checkout
/// has a readable HEAD. A broken inner marker cannot attribute a tool call to that outer branch.
#[test]
fn checkout_placement_respects_invalid_inner_markers_and_valid_linked_heads() {
    let t = Temp::new("placement-markers");
    let outer = t.0.join("outer");
    std::fs::create_dir_all(outer.join(".git")).unwrap();
    std::fs::write(outer.join(".git/HEAD"), "ref: refs/heads/tools/outer\n").unwrap();
    assert_eq!(
        checkout(&outer),
        Some((outer.clone(), "tools/outer".into()))
    );
    assert!(!linked(&outer));

    let bad_admin = outer.join("bad-admin");
    std::fs::create_dir_all(&bad_admin).unwrap();
    std::fs::write(bad_admin.join("HEAD"), "ref: refs/heads/tools/invalid\n").unwrap();
    std::fs::write(bad_admin.join("commondir"), "missing\n").unwrap();
    for (name, marker) in [
        ("empty", "gitdir: \n"),
        ("malformed", "not a marker\n"),
        ("missing", "gitdir: ../missing-admin\n"),
        ("bad-common", "gitdir: ../bad-admin\n"),
        ("dangling", ""),
    ] {
        let inner = outer.join(name);
        let deep = inner.join("component");
        std::fs::create_dir_all(&deep).unwrap();
        if name == "dangling" {
            std::os::unix::fs::symlink("missing", inner.join(".git")).unwrap();
        } else {
            std::fs::write(inner.join(".git"), marker).unwrap();
        }
        assert_eq!(checkout_common_dir(&inner), None, "{name}");
        for at in [&inner, &deep] {
            assert_eq!(checkout(at), None, "{name}: {}", at.display());
        }
    }

    let admin = outer.join("admin");
    let inner = outer.join("linked");
    let deep = inner.join("component");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(inner.join(".git"), "gitdir: ../admin\n").unwrap();
    std::fs::write(admin.join("commondir"), "../.git\n").unwrap();
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/tools/inner\n").unwrap();
    assert_eq!(checkout_common_dir(&inner), Some(outer.join(".git")));
    assert!(linked(&inner));
    assert_eq!(checkout(&deep), Some((inner.clone(), "tools/inner".into())));
    std::fs::write(
        admin.join("HEAD"),
        "0123456789abcdef0123456789abcdef01234567\n",
    )
    .unwrap();
    assert_eq!(checkout(&deep), Some((inner, String::new())));
}
