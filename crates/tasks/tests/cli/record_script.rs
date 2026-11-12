//! `tools/cargo.sh` and `tools/record.sh`, copied into workspaces of the test's own: a build or a
//! test goes through the recorder this binary is, keeping the arguments, streams, status and
//! limits Cargo would have had; anything else, a run inside a recorded one, or no recorder that
//! answers (a hung one included) leaves Cargo exec'd as it is. A worktree records with its main
//! checkout's prepared recorder, never with a containing project's. The justfile's recipes record
//! through the script, and run as they are in a tree without it.
//!
//! What must not be recorded is told by a recorder that logs each run of its own ([`logging`]): a
//! command is recorded only by a recorder run before it, so a log read once the command ended says
//! all there is, where a work folder is written by a recorder that may still be on its way.

use crate::recorder::{MACHINE, RUNS_VAR, day_lines, git_says, kept};
use crate::{git, scratch};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use symphony_api::{End, RunLine};
use symphony_board::cli::machine_work::recorded::{self, Phase, Record};
use symphony_board::sys::{Command, Stdio, output, wait_output};

const CARGO: &str = r#"#!/bin/bash
printf 'cwd=%s\njobs=%s\nthreads=%s\ntarget=%s\n' "$PWD" "$CARGO_BUILD_JOBS" "$RUST_TEST_THREADS" "$CARGO_TARGET_DIR"
printf 'arg=<%s>\n' "$@"
cat
printf 'diagnostic\n' >&2
if [ "${SELF_TERM:-}" = yes ]; then kill -TERM $$; fi
exit 7
"#;

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// This workspace's `tools/NAME`.
fn tool(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools")
        .join(name)
}

/// A workspace at `root` holding a manifest and the two scripts.
fn workspace(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("tools")).unwrap();
    for name in ["cargo.sh", "record.sh"] {
        fs::copy(tool(name), root.join("tools").join(name)).unwrap();
    }
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
    root.canonicalize().unwrap()
}

/// A scratch folder with a work folder and a recorder log of its own, and the workspace whose name
/// has spaces and the Cargo stand-in the tests share, which none changes: the first run of a file
/// just made executable can wait seconds on the system's scan of it, so each is made once.
#[derive(Clone)]
struct Setup {
    dir: PathBuf,
    root: PathBuf,
    cargo: PathBuf,
    work: PathBuf,
    log: PathBuf,
}

impl Setup {
    fn new(name: &str) -> Setup {
        static SHARED: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();
        warmed();
        let (root, cargo) = SHARED.get_or_init(|| {
            let shared = scratch("record-script-shared");
            let cargo = shared.join("cargo stand-in");
            executable(&cargo, CARGO);
            (workspace(&shared.join("workspace with spaces")), cargo)
        });
        let dir = scratch(name);
        Setup {
            work: dir.join("work"),
            log: dir.join("recorder log"),
            dir,
            root: root.clone(),
            cargo: cargo.clone(),
        }
    }

    /// The same, in a workspace of its own whose prepared recorder (`.target/bin/tasks`) is
    /// [`logging`].
    fn prepared(&self) -> Setup {
        let root = workspace(&self.dir.join("prepared"));
        fs::create_dir_all(root.join(".target/bin")).unwrap();
        std::os::unix::fs::symlink(logging(), root.join(".target/bin/tasks")).unwrap();
        Setup {
            root,
            ..self.clone()
        }
    }

    /// The runs of [`logging`] for this setup so far, a line each.
    fn logged(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// `script ARGS` in the workspace, recording with `recorder` (TASKS_RECORDER unset for none)
    /// and no session.
    fn command(&self, script: &str, args: &[&str], recorder: Option<&str>) -> Command {
        let mut command = Command::new(self.root.join("tools").join(script));
        command
            .args(args)
            .current_dir(&self.dir)
            .env("CARGO", &self.cargo)
            .env("CARGO_BUILD_JOBS", "1")
            .env("RUST_TEST_THREADS", "3")
            .env("CARGO_TARGET_DIR", "relative-target")
            .env("TASKS_WORK", &self.work)
            .env(LOG_VAR, &self.log)
            .env("SYMPHONY_MACHINE_DIR", self.dir.join("machine"))
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .env_remove("TASKS_SERVER")
            .env_remove(recorded::PARENT_VAR)
            .env_remove(recorded::PIPELINE_VAR)
            .env_remove(recorded::TRIGGER_VAR)
            .env_remove(RUNS_VAR)
            .env_remove("TASKS_RECORDER");
        if let Some(recorder) = recorder {
            command.env("TASKS_RECORDER", recorder);
        }
        command
    }

    /// What `cargo.sh ARGS` said, "caller input" on its stdin.
    fn cargo(&self, args: &[&str], recorder: Option<&str>) -> Output {
        self.run(&mut self.command("cargo.sh", args, recorder))
    }

    fn run(&self, command: &mut Command) -> Output {
        let input = self.dir.join("stdin");
        fs::write(&input, "caller input\n").unwrap();
        wait_output(
            command
                .stdin(Stdio::from(fs::File::open(input).unwrap()))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
        .unwrap()
    }

    /// What Cargo itself prints for `args`.
    fn said(&self, args: &[&str]) -> String {
        let mut said = format!(
            "cwd={}\njobs=1\nthreads=3\ntarget=relative-target\n",
            self.root.display()
        );
        for arg in args {
            said.push_str(&format!("arg=<{arg}>\n"));
        }
        said + "caller input\n"
    }

    /// The history once it keeps `count` operations.
    fn history(&self, count: usize) -> Vec<Record> {
        kept(&self.work, count)
    }
}

const RECORDER: &str = env!("CARGO_BIN_EXE_tasks");
/// The file [`logging`] logs its runs in.
const LOG_VAR: &str = "RECORDER_LOG";

/// The recorder run once, before any script probes it: the first run of a binary just built can wait
/// seconds on the system's scan of it, longer than a probe waits for its answer.
fn warmed() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let probe = output(Command::new(RECORDER).args(["run", "--probe"])).unwrap();
        assert_eq!(probe.stdout, b"run 1\nrun 2\n", "{probe:?}");
    });
}

/// The tests' recorder, named `tasks`, that writes each of its runs, its arguments on a line, to
/// the file [`LOG_VAR`] names, when one is named, then is [`RECORDER`]: made once, and run once.
fn logging() -> &'static Path {
    static LOGGING: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    LOGGING.get_or_init(|| {
        warmed();
        let path = scratch("record-script-logging").join("bin/tasks");
        executable(
            &path,
            &format!(
                "#!/bin/sh\nif [ -n \"${{{LOG_VAR}:-}}\" ]; then printf '%s\\n' \"$*\" >> \"${LOG_VAR}\"; fi\nexec '{RECORDER}' \"$@\"\n"
            ),
        );
        let probe = output(
            Command::new(&path)
                .args(["run", "--probe"])
                .env_remove(LOG_VAR),
        )
        .unwrap();
        assert_eq!(probe.stdout, b"run 1\nrun 2\n", "{probe:?}");
        path
    })
}

/// A recorder as old as the first version of `tasks run`: it answers the probe with `run 1` alone,
/// and otherwise logs its run as [`logging`] does and is [`RECORDER`].
fn first_version() -> &'static Path {
    static OLD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    OLD.get_or_init(|| {
        warmed();
        let path = scratch("record-script-first-version").join("bin/tasks");
        executable(
            &path,
            &format!(
                "#!/bin/sh\nif [ \"$*\" = 'run --probe' ]; then echo 'run 1'; exit 0; fi\nprintf '%s\\n' \"$*\" >> \"${LOG_VAR}\"\nexec '{RECORDER}' \"$@\"\n"
            ),
        );
        let probe = output(Command::new(&path).args(["run", "--probe"])).unwrap();
        assert_eq!(probe.stdout, b"run 1\n", "{probe:?}");
        path
    })
}

/// `done` is Cargo's own: its status 7, what it printed, its diagnostic.
fn cargo_said(setup: &Setup, done: Output, args: &[&str]) {
    assert_eq!(done.status.code(), Some(7), "{done:?}");
    assert_eq!(done.stderr, b"diagnostic\n");
    assert_eq!(String::from_utf8(done.stdout).unwrap(), setup.said(args));
}

#[test]
fn a_cargo_test_or_build_is_recorded_with_its_arguments_streams_status_and_limits_its_own() {
    let setup = Setup::new("record-script-recorded");
    let args = ["test", "one argument", "--", "--exact"];
    cargo_said(&setup, setup.cargo(&args, Some(RECORDER)), &args);
    let [test] = setup.history(1).try_into().unwrap();
    assert_eq!(test.phase, Phase::Test);
    assert_eq!(test.label, "cargo test one argument -- --exact");
    assert_eq!((test.exit_code, test.signal), (Some(7), None));
    assert_eq!(Path::new(&test.checkout), setup.root);
    assert_eq!(pipeline(&test), Some("test"));
    assert_eq!((test.handle, test.task), (None, None));
    let ended = setup.run(
        setup
            .command("cargo.sh", &["build"], Some(RECORDER))
            .env("SELF_TERM", "yes"),
    );
    assert_eq!(ended.status.signal(), Some(15), "{ended:?}");
    let history = setup.history(2);
    let build = history.last().unwrap();
    assert_eq!(
        (build.phase, build.label.as_str()),
        (Phase::Build, "cargo build")
    );
    assert_eq!((build.exit_code, build.signal), (None, Some(15)));
    assert_eq!(pipeline(build), Some("build"));
    // Run by itself, each is a run of the pipeline its command says.
    for (count, args, phase, run) in [
        (3, ["clippy", "-q"], Phase::Build, "lint"),
        (4, ["symphony-ui", "dist"], Phase::Build, "client"),
        (5, ["nextest", "run"], Phase::Test, "test"),
        (6, ["check", "-q"], Phase::Build, "build"),
        (7, ["doc", "-q"], Phase::Build, "build"),
        (8, ["rustc", "-q"], Phase::Build, "build"),
        (9, ["bench", "-q"], Phase::Test, "test"),
    ] {
        cargo_said(&setup, setup.cargo(&args, Some(RECORDER)), &args);
        let history = setup.history(count);
        let last = history.last().unwrap();
        assert_eq!((last.phase, pipeline(last)), (phase, Some(run)), "{args:?}");
        assert_eq!(last.run.as_ref().unwrap().step, None, "{args:?}");
    }
}

/// The pipeline `record` was recorded a run or a step of.
fn pipeline(record: &Record) -> Option<&str> {
    record.run.as_ref().map(|run| run.pipeline.as_str())
}

#[test]
fn what_is_neither_a_build_nor_a_test_or_runs_inside_a_recorded_one_or_finds_no_recorder_is_cargo_as_it_is()
 {
    let setup = Setup::new("record-script-plain");
    let logging = logging();
    // Neither a build nor a test: the recorder named is not even asked.
    for args in [
        vec!["fmt", "--check"],
        vec!["symphony-ui", "dev", "0"],
        vec!["metadata"],
    ] {
        cargo_said(&setup, setup.cargo(&args, logging.to_str()), &args);
        assert_eq!(setup.logged(), "", "{args:?} ran its recorder");
    }
    // No recorder named, none prepared here, and the workspace is in no repository: a `tasks` on
    // the path is not its recorder.
    let path = format!(
        "{}:{}",
        logging.parent().unwrap().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let unprepared = setup.run(setup.command("cargo.sh", &["test"], None).env("PATH", path));
    cargo_said(&setup, unprepared, &["test"]);
    assert_eq!(setup.logged(), "", "a recorder on the path ran");
    // A workspace whose prepared recorder is the logging one: TASKS_RECORDER=off, a recorder named
    // that is missing or older than the command, or a run inside a recorded one, leaves it unrun.
    let prepared = setup.prepared();
    let missing = setup.dir.join("no recorder here");
    let older = setup.dir.join("older tasks");
    executable(
        &older,
        "#!/bin/sh\nprintf 'usage: unknown command run\\n' >&2\nexit 2\n",
    );
    for recorder in ["off", missing.to_str().unwrap(), older.to_str().unwrap()] {
        cargo_said(
            &prepared,
            prepared.cargo(&["test"], Some(recorder)),
            &["test"],
        );
        assert_eq!(prepared.logged(), "", "{recorder} left its recorder to run");
    }
    let inside = prepared.run(
        prepared
            .command("cargo.sh", &["test"], None)
            .env(recorded::PARENT_VAR, "f".repeat(32)),
    );
    cargo_said(&prepared, inside, &["test"]);
    assert_eq!(prepared.logged(), "", "a step was recorded again");
    assert!(!setup.work.exists(), "something was recorded");
    // The same workspace's test, run as it is, is recorded by that recorder.
    cargo_said(&prepared, prepared.cargo(&["test"], None), &["test"]);
    let logged = prepared.logged();
    let runs: Vec<&str> = logged.lines().collect();
    assert_eq!(runs.len(), 2, "{logged}");
    assert_eq!(runs[0], "run --probe");
    assert!(
        runs[1].starts_with("run --phase test --label cargo test --pipeline test -- "),
        "{logged}"
    );
    let [test] = prepared.history(1).try_into().unwrap();
    assert_eq!(Path::new(&test.checkout), prepared.root);
    let interrupted = output(
        setup
            .command("cargo.sh", &["test"], Some("off"))
            .env("SELF_TERM", "yes"),
    )
    .unwrap();
    assert_eq!(interrupted.status.signal(), Some(15));
}

#[test]
fn a_recorder_whose_probe_hangs_leaves_cargo_to_run_as_it_is() {
    let setup = Setup::new("record-script-hung");
    let hung = setup.dir.join("hung tasks");
    let release = setup.dir.join("release");
    let ended = setup.dir.join("probe ended");
    executable(
        &hung,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"${LOG_VAR}\"\nwhile [ ! -e \"$RELEASE\" ]; do sleep 0.05; done\n: > \"$ENDED\"\n"
        ),
    );
    let done = setup.run(
        setup
            .command("cargo.sh", &["test"], Some(hung.to_str().unwrap()))
            .env("RELEASE", &release)
            .env("ENDED", &ended),
    );
    // Cargo ran and ended while the probe still hung.
    cargo_said(&setup, done, &["test"]);
    assert!(!ended.exists(), "cargo waited for the probe to end");
    fs::write(&release, "").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !ended.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the probe never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // It was asked, and never asked to record: its one run was the probe.
    assert_eq!(setup.logged(), "run --probe\n");
}

#[test]
fn a_command_line_record_cannot_read_is_refused_and_one_it_can_keeps_its_own_label() {
    let setup = Setup::new("record-script-usage");
    for args in [
        vec![],
        vec!["build", "label"],
        vec!["lint", "label", "true"],
    ] {
        let done = output(&mut setup.command("record.sh", &args, Some(RECORDER))).unwrap();
        assert_eq!(done.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&done.stderr).starts_with("usage: tools/record.sh"));
    }
    let done = setup.run(&mut setup.command(
        "record.sh",
        &["test", "just verify", "sh", "-c", "cat; exit 4"],
        Some(RECORDER),
    ));
    assert_eq!(done.status.code(), Some(4));
    assert_eq!(done.stdout, b"caller input\n");
    let [verify] = setup.history(1).try_into().unwrap();
    assert_eq!(
        (verify.label.as_str(), verify.phase),
        ("just verify", Phase::Test)
    );
}

#[test]
fn a_worktree_records_with_its_main_checkouts_recorder_and_a_nested_workspace_with_none() {
    let logging = logging();
    let dir = scratch("record-script-worktree");
    let main = workspace(&dir.join("main"));
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-q", "-m", "base"]);
    let worktree = dir.join("worktree");
    git(
        &main,
        &["worktree", "add", "-q", worktree.to_str().unwrap()],
    );
    fs::create_dir_all(main.join(".target/bin")).unwrap();
    std::os::unix::fs::symlink(logging, main.join(".target/bin/tasks")).unwrap();
    let cargo = dir.join("cargo stand-in");
    executable(&cargo, "#!/bin/sh\nexit 0\n");
    let work = dir.join("work");
    let log = dir.join("recorder log");
    let run = |root: &Path| {
        let done = output(
            Command::new(root.join("tools/cargo.sh"))
                .arg("build")
                .env("CARGO", &cargo)
                .env("TASKS_WORK", &work)
                .env(LOG_VAR, &log)
                .env_remove("TASKS_SESSION")
                .env_remove("TASKS_TASK")
                .env_remove("TASKS_SERVER")
                .env_remove(recorded::PARENT_VAR)
                .env_remove("TASKS_RECORDER"),
        )
        .unwrap();
        assert!(done.status.success(), "{done:?}");
    };
    run(&worktree);
    let [recorded] = kept(&work, 1).try_into().unwrap();
    assert_eq!(
        Path::new(&recorded.checkout),
        worktree.canonicalize().unwrap()
    );
    assert_eq!(recorded.branch.as_deref(), Some("worktree"));
    run(&main);
    kept(&work, 2);
    // Each was probed, then recorded.
    let recorded_runs = |log: &str| {
        log.lines()
            .filter(|run| {
                run.starts_with("run --phase build --label cargo build --pipeline build -- ")
            })
            .count()
    };
    let before = fs::read_to_string(&log).unwrap();
    assert_eq!(
        (before.lines().count(), recorded_runs(&before)),
        (4, 2),
        "{before}"
    );
    // A workspace inside another project's repository is not that project's worktree: the
    // project's recorder is not run for it.
    let nested = workspace(&main.join("vendored"));
    run(&nested);
    assert_eq!(fs::read_to_string(&log).unwrap(), before);
    assert_eq!(recorded::history(&work).unwrap().len(), 2);
}

#[test]
fn a_recipe_records_through_the_script_and_runs_as_it_is_in_a_tree_without_it() {
    let setup = Setup::new("record-script-recipe");
    let tree = setup.dir.join("tree with spaces");
    fs::create_dir_all(tree.join("tools")).unwrap();
    // The justfile and the Symphony's recipes it imports.
    for name in ["justfile", "symphony.just"] {
        fs::copy(tool("..").join(name), tree.join(name)).unwrap();
    }
    executable(
        &tree.join("tools/cargo.sh"),
        "#!/bin/sh\nprintf 'arg=<%s>\\n' \"$@\"\nexit 5\n",
    );
    let recipe = |task: Option<&str>| {
        let mut command = Command::new("just");
        command
            .args(["test-rs", "one argument"])
            .current_dir(&tree)
            .env("TASKS_WORK", &setup.work)
            .env("TASKS_RECORDER", logging())
            .env(LOG_VAR, &setup.log)
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .env_remove("TASKS_SERVER")
            .env_remove(recorded::PARENT_VAR);
        if let Some(task) = task {
            command.env("TASKS_TASK", task);
        }
        let done = output(command.stdin(Stdio::null())).unwrap();
        assert_eq!(done.status.code(), Some(5), "{done:?}");
        assert_eq!(
            String::from_utf8(done.stdout).unwrap(),
            "arg=<test>\narg=<--locked>\narg=<one argument>\n"
        );
    };
    // No tools/record.sh here: the recipe runs as it is, its recorder never asked.
    recipe(None);
    assert_eq!(setup.logged(), "", "a tree without the script recorded");
    fs::copy(tool("record.sh"), tree.join("tools/record.sh")).unwrap();
    recipe(Some("tools/recipe"));
    let logged = setup.logged();
    let runs: Vec<&str> = logged.lines().collect();
    assert_eq!(runs.len(), 2, "{logged}");
    assert_eq!(runs[0], "run --probe");
    assert!(
        runs[1]
            .starts_with("run --phase test --label just test-rs one argument --pipeline test -- "),
        "{logged}"
    );
    let [test] = setup.history(1).try_into().unwrap();
    assert_eq!(
        (test.label.as_str(), test.phase, test.task.as_deref()),
        (
            "just test-rs one argument",
            Phase::Test,
            Some("tools/recipe")
        )
    );
    assert_eq!(test.exit_code, Some(5));
}

#[test]
fn a_step_inside_a_run_is_recorded_as_one_and_a_first_version_recorder_records_no_run_or_step() {
    let setup = Setup::new("record-script-steps");
    let prepared = setup.prepared();
    let parent = "f".repeat(32);
    let step = |recorder: Option<&Path>, pipeline: Option<&str>, step: Option<&str>| {
        let mut args = vec!["--pipeline", "lint"];
        if let Some(step) = step {
            args.extend(["--step", step]);
        }
        args.extend(["build", "just lint-rs", "sh", "-c", "cat; exit 6"]);
        let mut command = prepared.command("record.sh", &args, recorder.and_then(Path::to_str));
        command.env(recorded::PARENT_VAR, &parent);
        if let Some(pipeline) = pipeline {
            command.env(recorded::PIPELINE_VAR, pipeline);
        }
        let done = prepared.run(&mut command);
        assert_eq!(done.status.code(), Some(6), "{done:?}");
        assert_eq!(done.stdout, b"caller input\n");
    };
    // Inside a run, a step it names is recorded as a step of that run.
    step(None, Some("verify"), Some("lint"));
    let logged = prepared.logged();
    assert_eq!(
        logged.lines().collect::<Vec<_>>(),
        [
            "run --probe",
            "run --phase build --label just lint-rs --pipeline lint --step lint -- sh -c cat; exit 6"
        ],
        "{logged}"
    );
    let [lint] = prepared.history(1).try_into().unwrap();
    assert_eq!(lint.parent_id.as_deref(), Some(parent.as_str()));
    let run = lint.run.unwrap();
    assert_eq!(
        (run.pipeline.as_str(), run.step.as_deref()),
        ("verify", Some("lint"))
    );
    // Inside a recorded command that is no run, a command it names no step of, or a recorder of
    // the first version: as it is.
    step(None, None, Some("lint"));
    step(None, Some("verify"), None);
    step(Some(first_version()), Some("verify"), Some("lint"));
    assert_eq!(
        prepared.logged(),
        logged,
        "a command inside a run was recorded"
    );
    // At top level, the first version records it as it did, with no pipeline.
    let old = first_version().to_str();
    cargo_said(&prepared, prepared.cargo(&["test"], old), &["test"]);
    let now = prepared.logged();
    let last = now.lines().last().unwrap();
    assert!(
        last.starts_with("run --phase test --label cargo test -- "),
        "{now}"
    );
    assert_eq!(prepared.history(2)[1].run, None);
}

/// A repository of the test's own holding this workspace's justfile and the scripts its recipes
/// run, with stand-ins for Cargo (which fails the tests under `FAIL_TESTS`), the client check and
/// `tools/rs-bin.sh` (which fails under `FAIL_PUBLISH`): the recipes recorded by [`RECORDER`].
struct Recipes {
    tree: PathBuf,
    cargo: PathBuf,
    machine: PathBuf,
    runs: PathBuf,
    work: PathBuf,
}

impl Recipes {
    fn new(setup: &Setup) -> Recipes {
        let tree = setup.dir.join("repo");
        fs::create_dir_all(tree.join("tools")).unwrap();
        for name in ["justfile", "symphony.just"] {
            fs::copy(tool("..").join(name), tree.join(name)).unwrap();
        }
        for name in ["cargo.sh", "record.sh"] {
            fs::copy(tool(name), tree.join("tools").join(name)).unwrap();
        }
        executable(&tree.join("tools/client-check.sh"), "#!/bin/sh\nexit 0\n");
        executable(
            &tree.join("tools/rs-bin.sh"),
            "#!/bin/sh\nif [ -n \"${FAIL_PUBLISH:-}\" ]; then exit 4; fi\nexit 0\n",
        );
        fs::write(tree.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        git(&tree, &["init", "-q", "-b", "main"]);
        git(&tree, &["add", "-A"]);
        git(&tree, &["commit", "-q", "-m", "base"]);
        let cargo = setup.dir.join("cargo");
        executable(
            &cargo,
            "#!/bin/sh\nif [ \"$1\" = test ] && [ -n \"${FAIL_TESTS:-}\" ]; then exit 9; fi\nexit 0\n",
        );
        let machine = setup.dir.join("machine");
        fs::create_dir_all(&machine).unwrap();
        fs::write(machine.join("machine-id"), format!("1\t{MACHINE}\n")).unwrap();
        Recipes {
            tree,
            cargo,
            machine,
            runs: setup.dir.join("runs"),
            work: setup.work.clone(),
        }
    }

    /// `just RECIPE` in the tree at top level, with `vars` set: what it did.
    fn just(&self, recipe: &str, vars: &[(&str, &str)]) -> Output {
        let mut command = Command::new("just");
        command
            .arg(recipe)
            .current_dir(&self.tree)
            .env("CARGO", &self.cargo)
            .env("TASKS_WORK", &self.work)
            .env(RUNS_VAR, &self.runs)
            .env("TASKS_RECORDER", RECORDER)
            .env("SYMPHONY_MACHINE_DIR", &self.machine)
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .env_remove("TASKS_SERVER")
            .env_remove(recorded::PARENT_VAR)
            .env_remove(recorded::PIPELINE_VAR)
            .env_remove(recorded::TRIGGER_VAR)
            .envs(vars.iter().copied());
        output(command.stdin(Stdio::null())).unwrap()
    }
}

#[test]
fn the_lint_client_and_size_recipes_are_runs_of_their_own_pipelines_and_repeat_only_themselves() {
    let setup = Setup::new("record-script-recipe-runs");
    let recipes = Recipes::new(&setup);
    let update = recipes.just("client-update", &[("FAIL_PUBLISH", "yes")]);
    assert!(!update.status.success(), "{update:?}");
    for recipe in ["client-check", "lint-rs", "client-check", "size-check"] {
        let done = recipes.just(recipe, &[]);
        assert!(done.status.success(), "{recipe}: {done:?}");
    }
    let mut lines = symphony_api::fold(day_lines(&recipes.runs, 5));
    lines.sort_by(|one, other| one.queued.total_cmp(&other.queued));
    let said: Vec<_> = lines
        .iter()
        .map(|line| {
            (
                line.label.as_str(),
                line.pipeline.as_str(),
                line.step.as_deref(),
                line.parent.as_deref(),
                line.end,
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("just client-update", "client", None, None, Some(End::Red)),
            ("just client-check", "client", None, None, Some(End::Green)),
            ("just lint-rs", "lint", None, None, Some(End::Green)),
            ("just client-check", "client", None, None, Some(End::Green)),
            ("just size-check", "size", None, None, Some(End::Green)),
        ]
    );
    // A client check on the tree a client update failed on repeats no update; the next client
    // check repeats the one before.
    assert_eq!(
        (lines[1].rerun_of.as_deref(), lines[2].rerun_of.as_deref()),
        (None, None)
    );
    assert_eq!(lines[3].rerun_of.as_deref(), Some(lines[1].run.as_str()));
}

#[test]
fn a_verify_writes_its_run_and_a_step_each_for_lint_client_and_size_checks_and_tests() {
    let setup = Setup::new("record-script-verify");
    let recipes = Recipes::new(&setup);
    let (tree, runs) = (&recipes.tree, &recipes.runs);
    let verify = |failing: bool| {
        let failing: &[(&str, &str)] = if failing {
            &[("FAIL_TESTS", "yes")]
        } else {
            &[]
        };
        recipes.just("verify", failing)
    };
    let red = verify(true);
    assert!(!red.status.success(), "{red:?}");
    let first = symphony_api::fold(day_lines(runs, 5));
    let green = verify(false);
    assert!(green.status.success(), "{green:?}");
    let lines = day_lines(runs, 10);
    // A start line and an end line for each run and step, and none for the commands inside them.
    assert_eq!(lines.len(), 20, "{lines:#?}");
    let second: Vec<RunLine> = symphony_api::fold(lines)
        .into_iter()
        .filter(|line| first.iter().all(|earlier| earlier.run != line.run))
        .collect();
    let named = |runs: &[RunLine], step: Option<&str>| -> RunLine {
        runs.iter()
            .find(|line| line.step.as_deref() == step)
            .unwrap_or_else(|| panic!("no {step:?}: {runs:#?}"))
            .clone()
    };
    let commit = git_says(tree, &["rev-parse", "HEAD"]);
    let checked = git_says(tree, &["rev-parse", "HEAD^{tree}"]);
    for (runs, end) in [(&first, End::Red), (&second, End::Green)] {
        assert_eq!(runs.len(), 5, "{runs:#?}");
        let verify = named(runs, None);
        assert_eq!(
            (
                verify.pipeline.as_str(),
                verify.label.as_str(),
                verify.parent.as_deref()
            ),
            ("verify", "just verify", None)
        );
        assert_eq!(verify.end, Some(end));
        for (step, label, ended) in [
            ("lint", "just lint-rs", End::Green),
            ("client-check", "just client-check", End::Green),
            ("size-check", "just size-check", End::Green),
            ("tests", "tests", end),
        ] {
            let line = named(runs, Some(step));
            assert_eq!(
                (line.pipeline.as_str(), line.label.as_str()),
                ("verify", label)
            );
            assert_eq!(line.parent.as_deref(), Some(verify.run.as_str()), "{step}");
            assert_eq!(line.end, Some(ended), "{step}");
        }
        for line in runs {
            assert_eq!(line.commit.as_deref(), Some(commit.as_str()));
            assert_eq!(line.tree.as_deref(), Some(checked.as_str()));
            assert_eq!(
                (line.dirty, line.branch.as_deref()),
                (Some(false), Some("main"))
            );
            assert_eq!(
                (line.machine.as_deref(), line.project.as_deref()),
                (Some(MACHINE), Some("repo"))
            );
            assert_eq!(line.trigger, "owner");
            let (started, ended) = (line.started.unwrap(), line.ended.unwrap());
            assert!(line.queued <= started && started <= ended, "{line:?}");
            assert!(line.cpu_s.is_some_and(|cpu| cpu >= 0.0), "{line:?}");
        }
    }
    let red_tests = named(&first, Some("tests"));
    assert_eq!(red_tests.exit, Some(9));
    assert_eq!(named(&first, None).exit, red.status.code());
    // The second verify, on the same tree after a red, repeats the first, and each step its own.
    for step in [
        None,
        Some("lint"),
        Some("client-check"),
        Some("size-check"),
        Some("tests"),
    ] {
        assert_eq!(
            named(&second, step).rerun_of,
            Some(named(&first, step).run),
            "{step:?}"
        );
    }
    // Each step's operation is a child of its run's; the commands inside them are none.
    let history = setup.history(10);
    for record in &history {
        let line = second
            .iter()
            .chain(&first)
            .find(|line| line.run == record.operation_id)
            .unwrap();
        assert_eq!(record.parent_id, line.parent);
    }
}
