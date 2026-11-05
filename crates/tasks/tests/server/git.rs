//! The git common dir. A folder of its own per test, outside this repository (the build's own
//! temporary folder is inside it).

use std::path::{Path, PathBuf};
use symphony_board::git::common_dir;
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
