//! A tidy where the project delivers by pull request (`[land] delivery`): its checkout on the
//! default branch is left as it was, the tidy refused when it would change a file and named the
//! way that writes it; a task's worktree, and a project that does not deliver, are written as
//! before.

use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use crate::common::{Tree, status};

const DELIVERS: &str = "[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n";
const REGISTER: &str =
    "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
const NOTES: &str = "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n\nrest\n";

/// `git ARGS…` in `root`, by no one's own git settings, asserting it succeeds: what it printed.
fn git(root: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim_end().into()
}

/// A checkout on `main` whose one commit holds `manifest` as its `symphony.toml`, a register
/// with a plain approval and a page with an answered comment: a tidy changes both and starts the
/// ledger.
fn checkout(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write("symphony.toml", manifest);
    tree.write("docs/DECISIONS.md", REGISTER);
    tree.write("docs/NOTES.md", NOTES);
    git(&tree.root, &["init", "-q", "-b", "main"]);
    commit(&tree.root, "the project");
    tree
}

/// Everything in the checkout at `root` committed on its branch.
fn commit(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Tidy test",
            "-c",
            "user.email=tidy-test@example.invalid",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// The file at `path` by its inode: a file replaced whole is another.
fn inode(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().ino()
}

/// What `git status` says of the checkout at `root`, untracked files and all.
fn dirt(root: &Path) -> String {
    git(root, &["status", "--porcelain", "--untracked-files=all"])
}

#[test]
fn a_tidy_on_the_default_branch_of_a_project_that_delivers_writes_nothing_and_names_the_way() {
    let tree = checkout(DELIVERS);
    let register = inode(&tree.path("docs/DECISIONS.md"));
    let (code, out, err) = status(&tree.root, &["--tidy", "--now", "2026-11-22 21:00"]);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(out, "");
    assert_eq!(
        err,
        "✖ not tidied: this project delivers by pull request ([land] delivery), so a tidy \
         writes nothing in a checkout on main, and this one would change docs/DECISIONS.md, \
         docs/NOTES.md, docs/history/review-ledger.md. Run `just symphony-status --tidy` in a \
         task's worktree instead: its pull request delivers what the tidy writes.\n"
    );
    assert_eq!(tree.read("docs/DECISIONS.md"), REGISTER);
    assert_eq!(tree.read("docs/NOTES.md"), NOTES);
    assert_eq!(tree.names("docs"), ["DECISIONS.md", "NOTES.md"]);
    // Not even replaced by the same text: the register is the file it was.
    let after = inode(&tree.path("docs/DECISIONS.md"));
    assert_eq!(after, register, "the register was replaced");
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn a_tidy_there_with_nothing_to_change_says_what_it_always_says_and_replaces_no_file() {
    // The checkout as a tidy delivered leaves it: tidied, committed, and then delivering.
    let tree = checkout("");
    let (code, _, err) = status(&tree.root, &["--tidy", "--now", "2026-11-22 21:00"]);
    assert_eq!((code, err.as_str()), (0, ""));
    tree.write("symphony.toml", DELIVERS);
    commit(&tree.root, "tidied, and delivering");
    let register = inode(&tree.path("docs/DECISIONS.md"));
    let (code, out, err) = status(&tree.root, &["--tidy", "--now", "2026-11-22 22:00"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "resolved: (none)\nregrouped docs/DECISIONS.md\n");
    let after = inode(&tree.path("docs/DECISIONS.md"));
    assert_eq!(after, register, "the register was replaced");
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn the_tidy_in_a_tasks_worktree_writes_it_and_leaves_the_checkout_on_main_as_it_was() {
    let tree = checkout(DELIVERS);
    let worktree = tree.path("../").join(format!(
        "{}-task",
        tree.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&worktree);
    git(
        &tree.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "docs/tidy",
            worktree.to_str().unwrap(),
        ],
    );
    let _gone = Removed(&worktree);
    let (code, out, err) = status(&worktree, &["--tidy", "--now", "2026-11-22 21:00"]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let register = std::fs::read_to_string(worktree.join("docs/DECISIONS.md")).unwrap();
    assert!(
        register.contains("- [r] **done** — plain.\n  [2026-11-22 21:00] agent · resolved"),
        "{register}"
    );
    assert!(worktree.join("docs/history/review-ledger.md").is_file());
    assert_eq!(
        dirt(&worktree),
        " M docs/DECISIONS.md\n M docs/NOTES.md\n?? docs/history/review-ledger.md"
    );
    assert_eq!(tree.read("docs/DECISIONS.md"), REGISTER);
    assert_eq!(dirt(&tree.root), "");
}

/// A linked worktree outside the tree, removed with its folder once the test is done.
struct Removed<'a>(&'a Path);

impl Drop for Removed<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

#[test]
fn a_tidy_of_a_project_that_does_not_deliver_writes_its_checkout_on_main_as_before() {
    let tree = checkout("[land]\ncheck = false\n");
    let (code, out, err) = status(&tree.root, &["--tidy", "--now", "2026-11-22 21:00"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "resolved: done\nregrouped docs/DECISIONS.md\narchived: docs/NOTES.md (1)\n"
    );
    assert!(
        tree.read("docs/DECISIONS.md")
            .contains("- [r] **done** — plain.\n  [2026-11-22 21:00] agent · resolved")
    );
    assert_eq!(tree.read("docs/NOTES.md"), "# N\n\nrest\n");
    assert_eq!(
        dirt(&tree.root),
        " M docs/DECISIONS.md\n M docs/NOTES.md\n?? docs/history/review-ledger.md"
    );
}

#[test]
fn a_delivery_the_default_branch_does_not_commit_holds_nothing_back() {
    // What delivers is the setting the default branch holds, as for an owner's save: one only
    // written in the checkout is not the project's yet.
    let tree = checkout("");
    tree.write("symphony.toml", DELIVERS);
    let (code, _, err) = status(&tree.root, &["--tidy", "--now", "2026-11-22 21:00"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(tree.read("docs/DECISIONS.md").contains("- [r] **done**"));
}
