//! A child keeps none of its starter's session but what its caller sets after.

use super::{board_env, command, git, output, sessionless};
use std::path::Path;
use symphony_board::sys::Command;

/// The five variables as the child sees them, `|` between.
const SAW: &str = "printf '%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \"${HC_TERMINAL_ID-unset}\" \
                   \"${HC_TERMINAL_SOCKET-unset}\" \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\"";

fn saw(command: &mut Command) -> String {
    let done = symphony_board::sys::output(command.args(["-c", SAW])).unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout).unwrap()
}

#[test]
fn a_child_runs_without_the_session_and_terminal_of_its_starter_but_with_its_actor() {
    // The variables are this test's own process's, set when it is started: set here, they would
    // reach every other test's children too.
    const CASE: &str = "SYMPHONY_CHILDREN_CASE";
    if std::env::var_os(CASE).is_some() {
        let all = "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope";
        assert_eq!(saw(&mut Command::new("/bin/sh")), all, "the starter's own");
        assert_eq!(
            saw(&mut command("/bin/sh")),
            "unset|unset|unset|unset|unset"
        );
        assert_eq!(
            saw(command("/bin/sh").env("TASKS_SESSION", "an-actor")),
            "an-actor|unset|unset|unset|unset"
        );
        // A launcher handed a command drops even what its caller set.
        let mut handed = Command::new("/bin/sh");
        handed.env("HC_TERMINAL_ID", "t8");
        assert_eq!(
            saw(sessionless(&mut handed)),
            "unset|unset|unset|unset|unset"
        );
        // So does the runner of a command the board builds and hands back.
        let ran = output(Command::new("/bin/sh").args(["-c", SAW])).unwrap();
        assert_eq!(ran.stdout, b"unset|unset|unset|unset|unset");
        // The board's environment keeps all of this process's but the session.
        let env = board_env();
        for (name, _) in STARTED {
            assert!(!env.vars.contains_key(name), "{name}");
        }
        assert_eq!(env.vars.get(CASE).map(String::as_str), Some("1"));
        return;
    }
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "children::tests::a_child_runs_without_the_session_and_terminal_of_its_starter_but_with_its_actor",
            ])
            .env(CASE, "1")
            .envs(STARTED),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// A git hook hands what it runs the repository and index of the commit it is in (every commit hook
/// `GIT_INDEX_FILE`, one in a linked worktree `GIT_DIR` too, its reference-transaction
/// `GIT_WORK_TREE`): the branch scan, a card check's private worktree and the ask store's place
/// are the folder's own all the same, and that repository's index and tree stay as they were.
#[test]
fn a_git_acts_on_the_repository_of_its_folder_whatever_repository_its_starter_names() {
    // Set in this process, the variables would move every other test's git: a re-run of this test
    // alone is started in them, with the scratch folder in this one.
    const CASE: &str = "SYMPHONY_CHILDREN_GIT_CASE";
    let said = |command: &mut Command| {
        let done = symphony_board::sys::output(command).unwrap();
        assert!(done.status.success(), "{done:?}");
        String::from_utf8(done.stdout).unwrap()
    };
    if let Some(root) = std::env::var_os(CASE) {
        let root = Path::new(&root).canonicalize().unwrap();
        let (ours, theirs) = (root.join("ours"), root.join("theirs"));
        let sub = ours.join("sub");
        let index = std::fs::read(theirs.join(".git/index")).unwrap();
        let subject = ["log", "-1", "--format=%s"];
        let kept = said(command("git").args(subject).current_dir(&sub));
        assert_eq!(kept, "theirs\n", "a git that keeps them");
        // The branch scan's reads (the docs audit's too), from a subfolder.
        assert_eq!(
            crate::branches::git(&sub, &subject).as_deref(),
            Some("ours\n")
        );
        assert_eq!(
            crate::branches::git(&sub, &["ls-files"]).as_deref(),
            Some("f\n")
        );
        let blobs = crate::branches::blobs(&sub, &["HEAD:sub/f".to_string()]);
        assert_eq!(blobs, Some(vec!["ours\n".to_string()]));
        // A card check's private worktree: made in ours with an index of its own, gone with it.
        let made = crate::landing::PrivateWorktree::new(&ours, "HEAD").unwrap();
        assert_eq!(
            std::fs::read_to_string(made.path.join("sub/f")).unwrap(),
            "ours\n"
        );
        let status = said(
            git()
                .args(["status", "--porcelain"])
                .current_dir(&made.path),
        );
        assert_eq!(status, "", "its own index holds its files");
        drop(made);
        let listed = said(
            git()
                .args(["worktree", "list", "--porcelain"])
                .current_dir(&ours),
        );
        assert_eq!(listed.matches("worktree ").count(), 1, "{listed}");
        assert_eq!(std::fs::read(theirs.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(theirs.join("f")).unwrap(),
            "changed\n"
        );
        // The ask store is ours' project's.
        let store = crate::asks::default_path(&sub, None);
        assert_eq!(store, ours.join("artifacts/asks/asks.db"));
        return;
    }
    let root = std::env::temp_dir().join(format!("symphony-children-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (ours, theirs) = (root.join("ours"), root.join("theirs"));
    for (repo, file) in [(&ours, "sub/f"), (&theirs, "f")] {
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        let name = repo.file_name().unwrap().display();
        std::fs::write(repo.join(file), format!("{name}\n")).unwrap();
        let words = ["-c", "user.email=t@t", "-c", "user.name=t"];
        let run = |args: &[&str]| said(git().args(words).args(args).current_dir(repo));
        run(&["init", "-q", "-b", "main"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", &name.to_string()]);
    }
    // Theirs has a change of its own that no git of ours may check out over.
    std::fs::write(theirs.join("f"), "changed\n").unwrap();
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "children::tests::a_git_acts_on_the_repository_of_its_folder_whatever_repository_its_starter_names",
            ])
            .env(CASE, &root)
            .env("GIT_DIR", theirs.join(".git"))
            .env("GIT_WORK_TREE", &theirs)
            .env("GIT_INDEX_FILE", theirs.join(".git/index")),
    )
    .unwrap();
    let out = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && out.contains("1 passed"),
        "{out}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// The session this test's own process is started in.
const STARTED: [(&str, &str); 5] = [
    ("TASKS_SESSION", "fixture-session"),
    ("HC_TERMINAL_ID", "t7"),
    ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
    ("HC_SESSION", "fixture-core"),
    ("HC_SCOPE", "fixture-scope"),
];
