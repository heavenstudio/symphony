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
    // `just` says after it that the recipe failed.
    assert_eq!(
        String::from_utf8_lossy(&unlinked.stderr).lines().next(),
        Some(
            "no tasks binary: link the project build of tasks at .target/bin/tasks, or put tasks \
             on PATH"
        ),
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

/// A fresh project's first commit on `main`, committed under an identity of the repository's own:
/// the land's merge commits under it too.
fn committed(project: &Project) {
    let root = &project.root;
    project.ok(
        root,
        "git",
        &["symbolic-ref", "HEAD", "refs/heads/main"],
        None,
    );
    for (key, value) in [
        ("user.name", "lamps"),
        ("user.email", "lamps@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        project.ok(root, "git", &["config", key, value], None);
    }
    project.ok(root, "git", &["add", "-A"], None);
    project.ok(root, "git", &["commit", "-q", "-m", "recipes"], None);
}

#[test]
fn a_fresh_projects_just_land_lands_an_approved_branch() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    std::os::unix::fs::symlink(
        crate::support::tasks_binary(),
        root.join(".target/bin/tasks"),
    )
    .unwrap();
    committed(&project);
    project.ok(
        &root,
        "git",
        &["worktree", "add", "-q", "-b", "lamps/first", "../first"],
        None,
    );
    let tree = project.dir.path().join("first");
    std::fs::write(tree.join("lamp.txt"), "lit\n").unwrap();
    project.ok(&tree, "git", &["add", "lamp.txt"], None);
    project.ok(
        &tree,
        "git",
        &["commit", "-q", "-m", "the first lamp"],
        None,
    );

    let board = Fixture::empty();
    let store = &board.store;
    let (orch, exe, rev) = (
        board.session("orchestrator"),
        board.session("worker"),
        board.session("reviewer"),
    );
    board.task("lamps/first");
    store
        .take_role("lamps/first", &orch, "orchestrator")
        .unwrap();
    store.claim("lamps/first", &exe).unwrap();
    store.move_to("lamps/first", "review", &exe).unwrap();
    store.take_role("lamps/first", &rev, "reviewer").unwrap();

    // The board's recipe, which an agent runs unasked, never lands: only `just land` does.
    let before = project.ok(&root, "git", &["rev-parse", "main"], None);
    store
        .verdict("lamps/first", &rev, "approve", "good")
        .unwrap();
    let unasked = project.run(
        &root,
        "just",
        &["tasks", "land", "lamps/first", "--as", &orch],
        Some(&board),
    );
    assert!(!unasked.status.success(), "{unasked:?}");
    assert!(
        String::from_utf8_lossy(&unasked.stderr).contains(
            "`tasks land` runs as `just land SLUG`, which asks before it moves and pushes the \
             default branch"
        ),
        "{unasked:?}"
    );
    assert_eq!(
        project.ok(&root, "git", &["rev-parse", "main"], None),
        before
    );
    store.move_to("lamps/first", "review", &exe).unwrap();

    // Not approved yet: refused, main where it was.
    let refused = project.run(
        &root,
        "just",
        &["land", "lamps/first", "--as", &orch],
        Some(&board),
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("lamps/first is review, with no approving verdict standing"),
        "{refused:?}"
    );
    assert_eq!(
        project.ok(&root, "git", &["rev-parse", "main"], None),
        before
    );

    store
        .verdict("lamps/first", &rev, "approve", "good")
        .unwrap();
    let out = project.ok(
        &tree,
        "just",
        &["land", "lamps/first", "--as", &orch],
        Some(&board),
    );
    let merge = project.ok(&root, "git", &["rev-parse", "main"], None);
    assert!(
        out.ends_with(&format!(
            "landed lamps/first on main at {}; not pushed: no origin remote\n",
            &merge[..12]
        )),
        "{out}"
    );
    assert_eq!(
        project.ok(&root, "git", &["log", "-1", "--format=%s", "main"], None),
        "Merge branch 'lamps/first'\n"
    );
    assert!(root.join("lamp.txt").is_file());
    assert_eq!(store.get("lamps/first").unwrap().status, "done");
}

#[test]
fn a_land_of_the_projects_own_replaces_the_symphonys_wherever_the_import_stands() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    // The Symphony's land runs the project's `tasks`, which is not linked yet.
    let theirs = project.run(&root, "just", &["land", "lamps/first"], None);
    assert_eq!(theirs.status.code(), Some(127), "{theirs:?}");
    assert!(
        String::from_utf8_lossy(&theirs.stderr).starts_with("no tasks binary"),
        "{theirs:?}"
    );

    let installed = std::fs::read_to_string(root.join("justfile")).unwrap();
    let own = "land *ARGS:\n    @echo our land {{ ARGS }}\n";
    for justfile in [format!("{installed}\n{own}"), format!("{own}\n{installed}")] {
        std::fs::write(root.join("justfile"), &justfile).unwrap();
        assert_eq!(
            project.ok(&root, "just", &["land", "lamps/first"], None),
            "our land lamps/first\n",
            "{justfile}"
        );
        // The rest of the Symphony's recipes stay.
        let tasks = project.run(&root, "just", &["tasks", "list"], None);
        assert_eq!(tasks.status.code(), Some(127), "{tasks:?}");
        let again = project.init(&[]);
        assert!(
            String::from_utf8_lossy(&again.stdout).contains("justfile: kept (the project's own)\n"),
            "{again:?}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("justfile")).unwrap(),
            justfile
        );
    }

    // Nor does the Symphony's land run by any other route: `tasks land` stands down, marker or not.
    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    std::os::unix::fs::symlink(
        crate::support::tasks_binary(),
        root.join(".target/bin/tasks"),
    )
    .unwrap();
    committed(&project);
    let board = Fixture::empty();
    let actor = board.session("orchestrator");
    for (command, why) in [
        (
            format!("just tasks land lamps/first --as {actor}"),
            "`tasks land` runs as `just land SLUG`, which asks before it moves and pushes the \
             default branch",
        ),
        (
            format!("SYMPHONY_LAND=1 just tasks land lamps/first --as {actor}"),
            "justfile lands with a `land` recipe of its own, which replaces the Symphony's \
             everywhere: run `just land SLUG` there",
        ),
    ] {
        let refused = project.run(&root, "sh", &["-c", &command], Some(&board));
        assert!(!refused.status.success(), "{command}: {refused:?}");
        let said = String::from_utf8_lossy(&refused.stderr);
        assert!(said.contains(why), "{command}: {said}");
    }
}

/// A project whose justfile makes `set allow-duplicate-recipes` itself, as a justfile init wrote
/// before the recipes made it does: init writes the recipes without it, and every recipe runs, its
/// own `land` among them; once it takes the setting out, init writes them with it.
#[test]
fn a_justfile_setting_allow_duplicate_recipes_itself_still_runs_every_recipe() {
    let project = Project::new();
    let root = project.root.clone();
    let own = "set allow-duplicate-recipes\n\nimport 'symphony.just'\n\nland *ARGS:\n    @echo our land {{ ARGS }}\n";
    std::fs::write(root.join("justfile"), own).unwrap();
    assert!(project.init(&["--root", "."]).status.success());
    assert_eq!(std::fs::read_to_string(root.join("justfile")).unwrap(), own);
    assert_eq!(
        project.ok(&root, "just", &["land", "lamps/first"], None),
        "our land lamps/first\n"
    );
    let asks = project.run(&root, "just", &["symphony-ask", "--help"], None);
    // It parses and runs: the document CLI is not linked, which its runner says.
    assert_eq!(asks.status.code(), Some(127), "{asks:?}");
    assert!(
        String::from_utf8_lossy(&asks.stderr).starts_with("no symphony binary"),
        "{asks:?}"
    );

    let without = "import 'symphony.just'\n\nland *ARGS:\n    @echo our land {{ ARGS }}\n";
    std::fs::write(root.join("justfile"), without).unwrap();
    assert!(project.init(&[]).status.success());
    assert_eq!(
        project.ok(&root, "just", &["land", "lamps/first"], None),
        "our land lamps/first\n"
    );
}
