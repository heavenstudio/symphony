//! A project `symphony init` set up runs the board through the recipes it installed, once the
//! project's builds are linked where init says: register, create and list on a scratch board, from
//! the checkout, a folder in it and a linked worktree.

use super::common::{BIN, TempDir, retain_coverage_env};
use crate::board::Fixture;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A git project in a folder of its own, its commands run with a search path holding `just` and
/// the system's tools alone, and the machine's Symphony folder beside it.
struct Project {
    dir: TempDir,
    root: PathBuf,
    path: String,
}

impl Project {
    fn new() -> Project {
        let dir = TempDir::new("recipes");
        let just = symphony::builds::on_path("just", std::env::var_os("PATH").as_deref())
            .expect("`just` on PATH: the installed recipes run under it");
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::os::unix::fs::symlink(just, tools.join("just")).unwrap();
        let path = format!("{}:/usr/bin:/bin", tools.display());
        let root = dir.path().join("lamp shop");
        let project = Project { dir, root, path };
        project.ok(
            project.dir.path(),
            "git",
            &["init", "-q", "lamp shop"],
            None,
        );
        project
    }

    /// `program ARGS` in `cwd`, on `board` when one is given.
    fn run(&self, cwd: &Path, program: &str, args: &[&str], board: Option<&Fixture>) -> Output {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.dir.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("SYMPHONY_MACHINE_DIR", self.dir.path().join("machine"))
            .env("TASKS_DISPATCH", "inline");
        if let Some(board) = board {
            let client = board.other_client();
            command
                .env("TASKS_SERVER", client.addr())
                .env("TASKS_DATABASE", client.database())
                .env("TASKS_NOW", board.now().to_string());
        }
        retain_coverage_env(&mut command);
        command.output().unwrap()
    }

    /// Its stdout, once it succeeded.
    fn ok(&self, cwd: &Path, program: &str, args: &[&str], board: Option<&Fixture>) -> String {
        let done = self.run(cwd, program, args, board);
        assert!(done.status.success(), "{program} {args:?}: {done:?}");
        String::from_utf8(done.stdout).unwrap()
    }

    /// `symphony init` from the project's folder, with `args` naming (or not) its root.
    fn init(&self, args: &[&str]) -> Output {
        let args: Vec<&str> = ["init"].iter().chain(args).copied().collect();
        self.run(&self.root, BIN, &args, None)
    }
}

#[test]
fn a_fresh_project_runs_the_board_through_the_recipes_init_installed() {
    let project = Project::new();
    let root = project.root.clone();

    let first = project.init(&["--root", "."]);
    let (out, err) = (
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr),
    );
    assert!(first.status.success(), "{first:?}");
    for said in [
        "symphony.just: installed\n",
        "justfile: installed\n",
        ".gitignore: installed\n",
        "\nNext:\n- write symphony.toml",
        // `.` is resolved: the project is named after the folder it names.
        "until then it is `lamp-shop`, after its folder",
        "serves at /lamp-shop/docs/",
        "\n- link the project's builds at .target/bin/symphony and .target/bin/tasks: ",
        "\n- register the project with the machine's Symphony: ",
    ] {
        assert!(out.contains(said), "{said:?} in {out}");
    }
    assert_eq!(
        err,
        "symphony init: warning: .target/bin/symphony and .target/bin/tasks are missing: no tool \
         call counts as a beat and the memory guard refuses nothing until the project's builds \
         are linked there (see Next)\n"
    );

    // With no build to run, a recipe says which link to make.
    let unlinked = project.run(&root, "just", &["tasks", "list"], None);
    assert_eq!(unlinked.status.code(), Some(127), "{unlinked:?}");
    // It is the whole error: `just` adds no line of its own that the recipe failed.
    assert_eq!(
        String::from_utf8_lossy(&unlinked.stderr),
        "no tasks binary: link the project build of tasks at .target/bin/tasks, or put tasks on \
         PATH\n",
        "{unlinked:?}"
    );

    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    std::os::unix::fs::symlink(
        crate::support::tasks_binary(),
        root.join(".target/bin/tasks"),
    )
    .unwrap();
    std::os::unix::fs::symlink(BIN, root.join(".target/bin/symphony")).unwrap();
    // No `--root` is the current folder, named the same way.
    let again = project.init(&[]);
    let out = String::from_utf8_lossy(&again.stdout);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(String::from_utf8_lossy(&again.stderr), "");
    assert!(out.contains("symphony.just: current\n"), "{out}");
    assert!(
        out.contains("until then it is `lamp-shop`, after its folder"),
        "{out}"
    );
    assert!(
        out.contains("justfile: kept (the project's own)\n"),
        "{out}"
    );
    assert!(!out.contains(".target/bin/symphony and"), "{out}");

    let board = Fixture::empty();
    let handle = project.ok(
        &root,
        "just",
        &["tasks", "register", "--kind", "worker"],
        Some(&board),
    );
    let handle = handle.trim();
    project.ok(
        &root,
        "just",
        &[
            "tasks",
            "create",
            "lamps/first",
            "The first lamp",
            "--as",
            handle,
        ],
        Some(&board),
    );
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let listed = project.ok(&root.join("docs"), "just", &["tasks", "list"], Some(&board));
    assert!(listed.contains("lamps/first"), "{listed}");
    // A refusal is the command's own words, exit code and all.
    let mistyped = project.run(&root, "just", &["tasks", "shwo"], Some(&board));
    assert_eq!(mistyped.status.code(), Some(2), "{mistyped:?}");
    let said = String::from_utf8_lossy(&mistyped.stderr);
    assert!(
        said.ends_with("did you mean 'show'? (`just tasks help` lists them)\n"),
        "{said}"
    );

    // The review commands go through the document CLI's recipe.
    let asks = project.ok(&root, "just", &["symphony-ask", "--help"], None);
    assert!(asks.starts_with("usage: symphony ask"), "{asks}");

    // A linked worktree has no links of its own: its recipes run the main checkout's.
    let identity = [
        "-c",
        "user.name=lamps",
        "-c",
        "user.email=lamps@example.invalid",
    ];
    project.ok(&root, "git", &["add", "justfile", "symphony.just"], None);
    let commit = [
        &identity[..],
        &["commit", "-q", "--no-gpg-sign", "-m", "recipes"],
    ]
    .concat();
    project.ok(&root, "git", &commit, None);
    project.ok(
        &root,
        "git",
        &["worktree", "add", "-q", "-b", "lamp-tree", "../lamp tree"],
        None,
    );
    let tree = project.dir.path().join("lamp tree");
    assert!(!tree.join(".target").exists());
    let shown = project.ok(
        &tree,
        "just",
        &["tasks", "show", "lamps/first"],
        Some(&board),
    );
    assert!(shown.contains("The first lamp"), "{shown}");
}
