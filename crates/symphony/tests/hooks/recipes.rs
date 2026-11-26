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
            let client = board.other_database();
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

/// A program at `path` that says `said` and its arguments.
fn says(path: &Path, said: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\necho \"{said} $*\"\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The release `name` installed in the machine folder `machine` as an install leaves it, holding
/// the binaries `names`, and `current` pointed at it.
fn release(machine: &Path, name: &str, names: &[&str]) {
    for binary in names {
        says(
            &machine.join("releases").join(name).join("bin").join(binary),
            &format!("{name} {binary}:"),
        );
    }
    let _ = std::fs::remove_file(machine.join("current"));
    std::os::unix::fs::symlink(Path::new("releases").join(name), machine.join("current")).unwrap();
}

#[test]
fn a_project_with_no_build_runs_the_machines_release_and_one_with_a_build_runs_its_own() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    let machine = project.dir.path().join("machine");
    release(&machine, "release-1", &["tasks", "symphony", "agent-send"]);
    // No link in the checkout and none on the search path: each recipe runs the release's binary,
    // its arguments as given, from a folder inside the project too.
    assert_eq!(
        project.ok(&root, "just", &["tasks", "list", "--all"], None),
        "release-1 tasks: list --all\n"
    );
    std::fs::create_dir_all(root.join("docs")).unwrap();
    assert_eq!(
        project.ok(
            &root.join("docs"),
            "just",
            &["symphony-cli", "status"],
            None
        ),
        "release-1 symphony: status\n"
    );
    assert_eq!(
        project.ok(&root, "just", &["symphony-status", "--lint"], None),
        "release-1 symphony: status --lint\n"
    );
    assert_eq!(
        project.ok(&root, "just", &["agent-send", "a b", "c"], None),
        "release-1 agent-send: a b c\n"
    );
    // What the release does not hold is not there to run: the recipe says so.
    let server = project.run(&root, "just", &["symphony"], None);
    assert_eq!(server.status.code(), Some(127), "{server:?}");
    assert!(
        String::from_utf8_lossy(&server.stderr).starts_with("no symphony-server binary: "),
        "{server:?}"
    );
    // The link moved: the next command runs the new release, nothing else told.
    release(&machine, "release-2", &["tasks"]);
    assert_eq!(
        project.ok(&root, "just", &["tasks", "list"], None),
        "release-2 tasks: list\n"
    );
    // The search path's comes after the release's.
    says(&project.dir.path().join("tools/tasks"), "path tasks:");
    assert_eq!(
        project.ok(&root, "just", &["tasks", "list"], None),
        "release-2 tasks: list\n"
    );
    // A project that builds its own keeps running it, from a linked worktree as well.
    says(&root.join(".target/bin/tasks"), "own tasks:");
    assert_eq!(
        project.ok(&root, "just", &["tasks", "list"], None),
        "own tasks: list\n"
    );
    let identity = [
        "-c",
        "user.name=Lamp",
        "-c",
        "user.email=lamp@example.invalid",
    ];
    project.ok(&root, "git", &["add", "justfile", "symphony.just"], None);
    let commit: Vec<&str> = (identity.iter().copied())
        .chain(["commit", "-q", "-m", "recipes"])
        .collect();
    project.ok(&root, "git", &commit, None);
    let tree = project.dir.path().join("tree");
    let add = ["worktree", "add", "-q", tree.to_str().unwrap()];
    project.ok(&root, "git", &add, None);
    assert_eq!(
        project.ok(&tree, "just", &["tasks", "list"], None),
        "own tasks: list\n"
    );
    // A dangling link is no release: the search path's runs.
    std::fs::remove_file(root.join(".target/bin/tasks")).unwrap();
    std::fs::remove_dir_all(machine.join("releases")).unwrap();
    assert_eq!(
        project.ok(&root, "just", &["tasks", "list"], None),
        "path tasks: list\n"
    );
}

#[test]
fn with_no_machine_folder_named_the_recipes_read_the_platforms() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    let home = project.dir.path().join("home");
    let (machine, data) = if cfg!(target_os = "macos") {
        (home.join("Library/Application Support/Symphony"), None)
    } else {
        let data = project.dir.path().join("data");
        (data.join("symphony"), Some(data))
    };
    release(&machine, "release-1", &["tasks"]);
    let run = |edit: &dyn Fn(&mut Command)| {
        let mut command = Command::new("just");
        command
            .args(["tasks", "list"])
            .current_dir(&root)
            .env_clear()
            .env("PATH", &project.path)
            .env("HOME", &home);
        if let Some(data) = &data {
            command.env("XDG_DATA_HOME", data);
        }
        edit(&mut command);
        retain_coverage_env(&mut command);
        command.output().unwrap()
    };
    let found = run(&|_| {});
    assert_eq!(
        String::from_utf8_lossy(&found.stdout),
        "release-1 tasks: list\n",
        "{found:?}"
    );
    // A machine folder that is named wins, and one named by a relative path is read nowhere.
    let named = run(&|command| {
        command.env("SYMPHONY_MACHINE_DIR", project.dir.path().join("none"));
    });
    assert_eq!(named.status.code(), Some(127), "{named:?}");
    let relative = run(&|command| {
        command.env("SYMPHONY_MACHINE_DIR", "machine");
    });
    assert_eq!(relative.status.code(), Some(127), "{relative:?}");
    // With no home there is no folder to read.
    let homeless = run(&|command| {
        command.env_remove("HOME").env_remove("XDG_DATA_HOME");
    });
    assert_eq!(homeless.status.code(), Some(127), "{homeless:?}");
}

#[test]
fn a_fresh_projects_just_list_says_what_each_installed_recipe_does_in_one_line() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    let listed = project.ok(&root, "just", &["--list"], None);
    // `just --list` shows the comment line right above a recipe: each is the whole of what the
    // recipe does, never the tail of a longer comment.
    for (recipe, said) in [
        (
            "tasks",
            "The task board: `just tasks help`. Every command of the board's but its recorder.",
        ),
        (
            "agent-send",
            "A message to an agent through its registered channel: `just agent-send HANDLE TEXT \
             --as SENDER`.",
        ),
        (
            "symphony",
            "The machine's Symphony server, serving this project; `--port 0` runs one of its own.",
        ),
        (
            "symphony-cli",
            "The document CLI: `just symphony-cli help`.",
        ),
        (
            "symphony-status",
            "The review board in the terminal: `--open`, `--json`, `--tidy`, `--lint`.",
        ),
        (
            "symphony-reply",
            "An agent's line under a card: `just symphony-reply PATH SLUG TEXT [--question] [--as \
             HANDLE]`.",
        ),
        (
            "symphony-ask",
            "The owner's asks: `list --open`, `show ID`, `answer ID TEXT --as HANDLE`.",
        ),
        (
            "land",
            "Land a task's approved branch: `just land SLUG --as HANDLE`.",
        ),
    ] {
        let line = listed
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("{recipe} *ARGS ")))
            .unwrap_or_else(|| panic!("{recipe} in {listed}"));
        assert!(line.ends_with(&format!("# {said}")), "{line}");
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
        "\n- link the project's builds at .target/bin/symphony, .target/bin/tasks, \
         .target/bin/agent-send and .target/bin/symphony-server, or put them on PATH: the \
         heartbeat, the memory guard, `just symphony-cli`, `just tasks`, `just tasks-people`, \
         `just agent-send` and `just symphony` run them and build nothing",
        // Its board is not made yet, and is made on PostgreSQL, which this machine's search path
        // for the fixture does not hold: the step says how to have it.
        "\n- have PostgreSQL 18 or later for the board, which runs on the machine's own: ",
        "install PostgreSQL 18 or later with the machine's package manager (`brew install \
         postgresql@18` on a Mac); put its bin folder on PATH or set [board] postgresql in \
         symphony.toml to that absolute folder\n",
        "\n- make the board: the first board command makes it, on PostgreSQL (`just tasks list`)",
        "\n- register the project with the machine's Symphony: ",
        "\n\nClaude Code counts the allow rules only in a folder it trusts: ",
    ] {
        assert!(out.contains(said), "{said:?} in {out}");
    }
    // `just` is on the search path, so nothing asks to install it; a board not made yet is told
    // nothing of SurrealDB, which only a board still on it is (how it moves).
    assert!(!out.contains("install `just`"), "{out}");
    assert!(!out.contains("SurrealDB"), "{out}");
    // The build running init is a checkout's: the step links it from there.
    let running = std::path::Path::new(BIN).canonicalize().unwrap();
    assert!(
        out.contains(&format!(
            "(`mkdir -p .target/bin && ln -s '{}' .target/bin/symphony",
            running.display()
        )),
        "{out}"
    );
    assert_eq!(
        err,
        "symphony init: warning: symphony and tasks have no build at .target/bin/ or on PATH: no \
         tool call counts as a beat and the memory guard refuses nothing until they do (see \
         Next)\n"
    );
    // A root that is not there is refused, and nothing is made.
    let gone = project.init(&["--root", "gone"]);
    assert_eq!(gone.status.code(), Some(2), "{gone:?}");
    assert!(
        String::from_utf8_lossy(&gone.stderr).contains("error: no folder at "),
        "{gone:?}"
    );
    assert!(!root.join("gone").exists());

    // With no build to run, a recipe says which link to make.
    let unlinked = project.run(&root, "just", &["tasks", "list"], None);
    assert_eq!(unlinked.status.code(), Some(127), "{unlinked:?}");
    // It is the whole error: `just` adds no line of its own that the recipe failed.
    assert_eq!(
        String::from_utf8_lossy(&unlinked.stderr),
        "no tasks binary: link the project build of tasks at .target/bin/tasks, install a release \
         on the machine, or put tasks on PATH\n",
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
    // The justfile init wrote, unchanged since, is current, not the project's own.
    assert!(out.contains("justfile: current\n"), "{out}");
    // Its board is not made yet: the step that makes it is left.
    assert!(
        out.contains(
            "\n- make the board: the first board command makes it, on PostgreSQL (`just tasks \
             list`); the Symphony only reads a board and never makes one\n"
        ),
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

/// What the board's recipe says for the recorder, and all it says.
const NO_RECORDER: &str = "just tasks refuses run: the recorder runs any command it is handed, and \
                           an agent runs just tasks unasked. Record a build by the board binary \
                           itself (.target/bin/tasks run); just tasks help run says how\n";

/// The recorder's spellings as `tasks` reads them (its command first, after any `--database NAME`),
/// each with a command that makes `ran`, and the environment prefix an agent's rule allows too.
fn recorder_spellings(ran: &Path) -> Vec<String> {
    let touch = format!("-- touch '{}'", ran.display());
    vec![
        format!("just tasks run --phase test {touch}"),
        format!("just tasks run {touch}"),
        format!("just tasks --database scratch1 run --phase test {touch}"),
        format!("just tasks --database a --database b run --phase build {touch}"),
        format!("just tasks --database run run --phase test {touch}"),
        format!("TASKS_SESSION=lamp-1 just tasks run --phase test --as lamp-1 {touch}"),
        "just tasks run --probe".into(),
        "just tasks run --help".into(),
    ]
}

/// The board's recipe, which an agent runs unasked, runs every board command but the recorder
/// (`tasks run`), which runs any command it is handed: it refuses it in each spelling `tasks`
/// reads, and the command never starts. Its help, and a database named `run`, still pass; the
/// board's binary, which no rule allows, still records, as a build wrapper runs it.
#[test]
fn the_board_recipe_refuses_the_recorder_which_the_board_binary_still_runs() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    let tasks = root.join(".target/bin/tasks");
    std::os::unix::fs::symlink(crate::support::tasks_binary(), &tasks).unwrap();
    let ran = project.dir.path().join("ran");

    for command in recorder_spellings(&ran) {
        let refused = project.run(&root, "sh", &["-c", &command], None);
        assert_eq!(refused.status.code(), Some(2), "{command}: {refused:?}");
        assert_eq!(
            (
                String::from_utf8_lossy(&refused.stdout),
                String::from_utf8_lossy(&refused.stderr)
            ),
            ("".into(), NO_RECORDER.into()),
            "{command}"
        );
        assert!(!ran.exists(), "{command} ran its command");
    }

    // Its help is the board's `help run`, a database named `run` among the options or not.
    for args in [
        &["tasks", "help", "run"][..],
        &["tasks", "--database", "run", "help", "run"],
    ] {
        let help = project.ok(&root, "just", args, None);
        assert!(
            help.starts_with("usage: tasks run --phase build|test"),
            "{args:?}: {help}"
        );
    }
    // A word `run` that is no command passes to the board, which says what it makes of it.
    let other = project.run(&root, "just", &["tasks", "--as", "lamp-1", "run"], None);
    assert_eq!(other.status.code(), Some(2), "{other:?}");
    assert!(
        String::from_utf8_lossy(&other.stderr).contains("error: no command 'run'"),
        "{other:?}"
    );

    // The binary itself is the recorder: it runs the command.
    let recorded = project.run(
        &root,
        tasks.to_str().unwrap(),
        &[
            "run",
            "--phase",
            "test",
            "--",
            "touch",
            ran.to_str().unwrap(),
        ],
        None,
    );
    assert!(recorded.status.success(), "{recorded:?}");
    assert!(ran.is_file());
}

/// A change to a board's people or a profile runs by `just tasks-people`, which no allow rule
/// passes, so an agent's run of it asks first: `just tasks`, which an agent runs unasked, refuses
/// each change in every spelling `tasks` reads (`--as` before the command, in either form or
/// abbreviated, an option or a `--` before the action), writing nothing on the board or in the
/// machine folder, and reads the board's people unasked.
#[test]
fn a_people_change_runs_by_its_own_recipe_and_the_board_recipe_refuses_it_however_spelled() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    let tasks = root.join(".target/bin/tasks");
    std::os::unix::fs::symlink(crate::support::tasks_binary(), &tasks).unwrap();
    let board = Fixture::empty();
    let board = Some(&board);
    let me = project.ok(
        &root,
        "just",
        &["tasks", "register", "--kind", "worker"],
        board,
    );
    let me = me.trim();
    let as_me = format!("--as={me}");
    let mut spellings: Vec<Vec<&str>> = vec![
        vec!["people", "add", "ana", "--as", me],
        vec!["--as", me, "people", "add", "ana"],
        vec![&as_me, "people", "add", "ana"],
        vec!["--a", me, "people", "add", "ana"],
        vec!["people", "--display", "Ana", "add", "ana", "--as", me],
        vec!["--as", me, "people", "--", "add", "ana"],
        vec!["people", "profile", "ana", "--git", "ana-l", "--as", me],
        vec!["people", "group", "add", "staff", "--as", me],
        vec!["people", "group", "remove", "staff", "--as", me],
    ];
    for action in [
        &["remove", "ana"][..],
        &["join", "ana", "staff"],
        &["leave", "ana", "staff"],
        &["grant", "ana", "decide"],
        &["revoke", "ana", "decide"],
    ] {
        spellings.push([&["people"][..], action, &["--as", me]].concat());
    }
    for spelling in &spellings {
        let args = [&["tasks"][..], spelling].concat();
        let refused = project.run(&root, "just", &args, board);
        let said = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "{args:?}: {refused:?}");
        assert!(
            said.contains("`just tasks` refuses people ") && said.contains("`just tasks-people "),
            "{args:?}: {said}"
        );
    }
    let log = ["tasks", "people", "log", "--as", me];
    assert_eq!(
        project.ok(&root, "just", &log, board),
        "no change was made to this board's people\n"
    );
    let machine = project.dir.path().join("machine");
    assert!(!machine.join(symphony_board::people::FILE).exists());

    // Its own recipe makes each, the board's reads following it unasked.
    let added = project.ok(
        &root,
        "just",
        &["tasks-people", "add", "ana", "--power", "speak", "--as", me],
        board,
    );
    assert!(
        added.starts_with("ana is on this board (change 1)"),
        "{added}"
    );
    let granted = ["tasks-people", "grant", "ana", "decide", "--as", me];
    assert!(
        project
            .ok(&root, "just", &granted, board)
            .starts_with("ana is given decide")
    );
    let list = project.ok(
        &root,
        "just",
        &["tasks", "people", "list", "--as", me],
        board,
    );
    assert!(list.starts_with("ana  read, speak, decide"), "{list}");
    assert_eq!(project.ok(&root, "just", &log, board).lines().count(), 2);
    assert!(machine.join(symphony_board::people::FILE).is_file());
}

/// This repository's own `tasks` recipe, which replaces the Symphony's here and builds the board
/// before it runs it, refuses the recorder the same way, before it builds anything; any other board
/// command reaches its build, words and all.
#[test]
fn this_repositorys_board_recipe_refuses_the_recorder_before_it_builds() {
    let tree = TempDir::new("own-tasks");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["justfile", "symphony.just"] {
        std::fs::copy(workspace.join(name), tree.path().join(name)).unwrap();
    }
    let built = tree.path().join("built");
    let stand_in = tree.path().join("tools/rs-bin.sh");
    std::fs::create_dir_all(stand_in.parent().unwrap()).unwrap();
    // It says the marker too: under it the board writes only into the project, as the
    // Symphony's recipe has it.
    std::fs::write(
        &stand_in,
        format!(
            "#!/bin/sh\nprintf '[%s]' \"${{SYMPHONY_UNASKED-unset}}\" >> '{0}'\nprintf '<%s>' \"$@\" >> '{0}'\n",
            built.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &stand_in,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let project = Project::new();
    let ran = tree.path().join("ran");
    for command in recorder_spellings(&ran) {
        let refused = project.run(tree.path(), "sh", &["-c", &command], None);
        assert_eq!(refused.status.code(), Some(2), "{command}: {refused:?}");
        assert_eq!(
            String::from_utf8_lossy(&refused.stderr),
            NO_RECORDER,
            "{command}"
        );
        assert!(
            !ran.exists() && !built.exists(),
            "{command} reached the build"
        );
    }
    project.ok(
        tree.path(),
        "just",
        &["tasks", "say", "lamps/first", "run it", "--as", "lamp-1"],
        None,
    );
    project.ok(tree.path(), "just", &["tasks", "help", "run"], None);
    // Its people recipe runs the same build with no marker: the change asks first by its rule.
    let change = ["tasks-people", "add", "ana", "--as", "lamp-1"];
    project.ok(tree.path(), "just", &change, None);
    assert_eq!(
        std::fs::read_to_string(&built).unwrap(),
        "[1]<tasks><say><lamps/first><run it><--as><lamp-1>[1]<tasks><help><run>\
         [unset]<tasks><people><add><ana><--as><lamp-1>"
    );
}

/// The board and reply recipes, which an agent runs unasked, write only into the checkout they run
/// in: a place outside it, as written, by `..` or through a link, and any git folder are refused
/// with one line naming it, and nothing is made there. The same commands inside the checkout pass,
/// the ask store a callback names in the main checkout's artifacts folder passes from a linked
/// worktree (and the rest of the main checkout does not), and the binaries run by themselves, which
/// no rule allows, still write where they are told.
#[test]
fn the_board_and_reply_recipes_write_only_into_the_project() {
    let project = Project::new();
    let root = project.root.clone();
    assert!(project.init(&["--root", "."]).status.success());
    std::fs::create_dir_all(root.join(".target/bin")).unwrap();
    let tasks = root.join(".target/bin/tasks");
    std::os::unix::fs::symlink(crate::support::tasks_binary(), &tasks).unwrap();
    let symphony = root.join(".target/bin/symphony");
    std::os::unix::fs::symlink(BIN, &symphony).unwrap();
    let card = "# Decisions\n\n- [ ] **lamp** — Which lamp?\n";
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/DECISIONS.md"), card).unwrap();
    let out = project.dir.path().canonicalize().unwrap().join("elsewhere");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("notes.txt"), card).unwrap();
    std::os::unix::fs::symlink(&out, root.join("out-link")).unwrap();
    let top = root.canonicalize().unwrap();
    let board = Fixture::empty();
    let from_main = format!(
        "only the checkout at {}, none of its git folders",
        top.display()
    );
    let refusal_from = |path: &str, leads: &Path, places: &str| {
        let place = if Path::new(path) == leads {
            format!("{path} is")
        } else {
            format!("{path} leads to {}, which is", leads.display())
        };
        format!(
            "✖ {place} not a place a recipe an agent runs unasked writes: {places}; the binary \
             run by itself writes anywhere\n"
        )
    };
    let refusal = |path: &str, leads: &Path| refusal_from(path, leads, &from_main);
    // The store `--root` names is the folder's own, by its absolute path.
    let other_store = out.join("artifacts/asks/asks.db");
    let other_store_named = other_store.display().to_string();

    let notes = out.join("notes.txt");
    for (args, path, leads) in [
        (
            vec!["tasks", "export", "--out", "../elsewhere/board.md"],
            "../elsewhere/board.md",
            out.join("board.md"),
        ),
        (
            vec!["tasks", "export", "--out", "out-link/board.md"],
            "out-link/board.md",
            out.join("board.md"),
        ),
        (
            vec![
                "symphony-reply",
                "../elsewhere/notes.txt",
                "lamp",
                "from outside",
            ],
            "../elsewhere/notes.txt",
            notes.clone(),
        ),
        (
            vec![
                "symphony-reply",
                "out-link/notes.txt",
                "lamp",
                "from outside",
            ],
            "out-link/notes.txt",
            notes.clone(),
        ),
        (
            vec!["symphony-ask", "--db", "../elsewhere/deep/asks.db", "list"],
            "../elsewhere/deep/asks.db",
            out.join("deep/asks.db"),
        ),
        (
            vec!["symphony-ask", "--root", "../elsewhere", "list"],
            other_store_named.as_str(),
            other_store.clone(),
        ),
        (
            vec!["symphony-status", "--root", "../elsewhere", "--tidy"],
            "../elsewhere",
            out.clone(),
        ),
    ] {
        let refused = project.run(&root, "just", &args, Some(&board));
        assert_eq!(
            (
                refused.status.code(),
                String::from_utf8_lossy(&refused.stdout).into_owned(),
                String::from_utf8_lossy(&refused.stderr).into_owned()
            ),
            (Some(1), String::new(), refusal(path, &leads)),
            "{args:?}"
        );
        let mut left: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        symphony_board::order::sort_by(&mut left, &Ord::cmp);
        assert_eq!(left, ["notes.txt"], "{args:?} wrote outside");
        assert_eq!(std::fs::read_to_string(&notes).unwrap(), card, "{args:?}");
    }
    // Nor into the checkout's own git folder, where a hook or the config would run or misdirect
    // every later git command.
    let config = top.join(".git/config");
    let config_before = std::fs::read_to_string(&config).unwrap();
    for (args, path, leads) in [
        (
            vec!["tasks", "export", "--out", ".git/config"],
            ".git/config",
            config.clone(),
        ),
        (
            vec!["tasks", "export", "--out", "docs/../.git/hooks/pre-commit"],
            "docs/../.git/hooks/pre-commit",
            top.join(".git/hooks/pre-commit"),
        ),
        (
            vec!["symphony-ask", "--db", ".git/asks.db", "list"],
            ".git/asks.db",
            top.join(".git/asks.db"),
        ),
    ] {
        let refused = project.run(&root, "just", &args, Some(&board));
        assert_eq!(
            (
                refused.status.code(),
                String::from_utf8_lossy(&refused.stderr).into_owned()
            ),
            (Some(1), refusal(path, &leads)),
            "{args:?}"
        );
    }
    assert_eq!(std::fs::read_to_string(&config).unwrap(), config_before);
    assert!(!top.join(".git/hooks/pre-commit").exists() && !top.join(".git/asks.db").exists());

    // Inside the project each passes: the snapshot's default place, a reply under the card, the
    // ask store in the artifacts folder, the tidy of the project's own registers.
    let wrote = project.ok(&root, "just", &["tasks", "export"], Some(&board));
    assert_eq!(wrote, "wrote docs/history/tasks.md\n");
    let replied = project.ok(
        &root,
        "just",
        &[
            "symphony-reply",
            "docs/DECISIONS.md",
            "lamp",
            "The brass one.",
        ],
        None,
    );
    assert_eq!(replied, "replied on lamp in docs/DECISIONS.md\n");
    assert!(
        std::fs::read_to_string(root.join("docs/DECISIONS.md"))
            .unwrap()
            .contains("agent — The brass one."),
    );
    assert_eq!(
        project.ok(&root, "just", &["symphony-ask", "list"], None),
        "no asks\n"
    );
    let store = top.join("artifacts/asks/asks.db");
    assert!(store.is_file());
    let tidied = project.run(&root, "just", &["symphony-status", "--tidy"], None);
    assert!(
        tidied.status.success() && tidied.stderr.is_empty(),
        "{tidied:?}"
    );

    // From a linked worktree the store a callback names, in the main checkout, is the project's.
    project.ok(&root, "git", &["add", "justfile", "symphony.just"], None);
    project.ok(
        &root,
        "git",
        &[
            "-c",
            "user.name=lamps",
            "-c",
            "user.email=lamps@example.invalid",
            "commit",
            "-q",
            "--no-gpg-sign",
            "-m",
            "recipes",
        ],
        None,
    );
    project.ok(
        &root,
        "git",
        &["worktree", "add", "-q", "-b", "lamp-tree", "../lamp tree"],
        None,
    );
    let tree = project.dir.path().join("lamp tree");
    let named = store.display().to_string();
    assert_eq!(
        project.ok(
            &tree,
            "just",
            &["symphony-ask", "--db", &named, "list"],
            None
        ),
        "no asks\n"
    );
    // The rest of the main checkout and its git folder are not the worktree's to write.
    let from_tree = format!(
        "only the checkout at {}, none of its git folders, and the main checkout's artifacts \
         folder at {}",
        tree.canonicalize().unwrap().display(),
        top.join("artifacts").display()
    );
    let justfile_before = std::fs::read_to_string(top.join("justfile")).unwrap();
    let config_before = std::fs::read_to_string(&config).unwrap();
    for leads in [
        top.join("justfile"),
        top.join(".git/hooks/pre-commit"),
        top.join(".git/config"),
    ] {
        let path = leads.display().to_string();
        let refused = project.run(
            &tree,
            "just",
            &["tasks", "export", "--out", &path],
            Some(&board),
        );
        assert_eq!(
            (
                refused.status.code(),
                String::from_utf8_lossy(&refused.stderr).into_owned()
            ),
            (Some(1), refusal_from(&path, &leads, &from_tree)),
            "{path}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(top.join("justfile")).unwrap(),
        justfile_before
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), config_before);

    // The binaries run by themselves, as the owner may, write where they are told.
    let board_md = out.join("board.md").display().to_string();
    let exported = project.run(
        &root,
        tasks.to_str().unwrap(),
        &["export", "--out", &board_md],
        Some(&board),
    );
    assert!(exported.status.success(), "{exported:?}");
    let notes_path = notes.display().to_string();
    let reply = ["reply", notes_path.as_str(), "lamp", "from the owner"];
    let direct = project.run(&root, symphony.to_str().unwrap(), &reply, None);
    assert!(direct.status.success(), "{direct:?}");
    assert!(
        std::fs::read_to_string(&notes)
            .unwrap()
            .contains("agent — from the owner")
    );
    assert!(out.join("board.md").is_file());
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

    // The board's recipe never lands, the handle before `land` or after it, an approve standing or
    // not: only `just land` does, whose recipe sets the marker.
    let before = project.ok(&root, "git", &["rev-parse", "main"], None);
    store
        .verdict("lamps/first", &rev, "approve", "good")
        .unwrap();
    let orch_handle = orch.as_str();
    for args in [
        ["tasks", "land", "lamps/first", "--as", orch_handle],
        ["tasks", "--as", orch_handle, "land", "lamps/first"],
    ] {
        let unasked = project.run(&root, "just", &args, Some(&board));
        assert!(!unasked.status.success(), "{args:?}: {unasked:?}");
        assert!(
            String::from_utf8_lossy(&unasked.stderr).contains(
                "`tasks land` runs only as `just land SLUG`, the recipe that moves and pushes the \
                 default branch"
            ),
            "{args:?}: {unasked:?}"
        );
        assert_eq!(
            project.ok(&root, "git", &["rev-parse", "main"], None),
            before
        );
    }
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
            "`tasks land` runs only as `just land SLUG`, the recipe that moves and pushes the \
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

/// A project an older init set up asked before a land: its ask rules for the marker and for
/// `just tasks land` go, the project's own stays, `just land` is allowed, and a second run changes
/// nothing. `tasks land` run by itself still refuses (`a_fresh_projects_just_land_lands_an_approved_branch`).
#[test]
fn an_init_over_the_old_ask_rules_lets_just_land_run_unasked() {
    let project = Project::new();
    let root = project.root.clone();
    let old = [
        "Bash(just bless:*)",
        "Bash(*SYMPHONY_LAND*)",
        "Bash(just tasks land)",
        "Bash(just tasks land *)",
        "Bash(just tasks -* land)",
        "Bash(just tasks -* land *)",
        "Bash(TASKS_SESSION=* just tasks land)",
        "Bash(TASKS_SESSION=* just tasks land *)",
        "Bash(TASKS_SESSION=* just tasks -* land)",
        "Bash(TASKS_SESSION=* just tasks -* land *)",
    ];
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        serde_json::json!({"permissions": {"allow": ["Bash(just tasks:*)"], "ask": old}})
            .to_string(),
    )
    .unwrap();
    let first = project.init(&["--root", "."]);
    assert!(first.status.success(), "{first:?}");
    assert!(
        String::from_utf8_lossy(&first.stdout).contains("claude settings: updated\n"),
        "{first:?}"
    );
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["permissions"]["ask"],
        serde_json::json!(["Bash(just bless:*)"])
    );
    let allow = settings["permissions"]["allow"].as_array().unwrap();
    assert!(allow.contains(&"Bash(just land:*)".into()), "{allow:?}");
    let again = project.init(&[]);
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("claude settings: current\n"),
        "{again:?}"
    );
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
