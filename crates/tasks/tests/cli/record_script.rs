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
/// The folder a verify writes its steps' logs and list into, when one is named (tools/verify.sh).
const STEPS_VAR: &str = "SYMPHONY_CHECK_STEPS";

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

/// The client check's stand-in: it builds the client (an `index.html` in the folder it is handed)
/// unless `NO_CLIENT` is set, and is red under `FAIL_CLIENT`.
const CLIENT_CHECK: &str = r#"#!/bin/sh
if [ -z "${NO_CLIENT:-}" ]; then mkdir -p "$1" && echo built > "$1/index.html"; fi
if [ -n "${FAIL_CLIENT:-}" ]; then echo 'client-check stand-in: differs'; exit 1; fi
exit 0
"#;

/// `tools/rs-bin.sh`'s stand-in: red under `FAIL_PUBLISH`, and the size budgets under `FAIL_SIZE`.
const RS_BIN: &str = r#"#!/bin/sh
if [ -n "${FAIL_PUBLISH:-}" ]; then exit 4; fi
if [ "$2" = size-budget ] && [ -n "${FAIL_SIZE:-}" ]; then echo 'size stand-in: past its budget'; exit 3; fi
exit 0
"#;

/// Cargo's stand-in: `fmt` red under `FAIL_LINT`, and held a minute under `HOLD_LINT`, once it has
/// made the file that names; `test` saying the client it was handed and the steps folder it sees,
/// red under `FAIL_TESTS`.
const RECIPES_CARGO: &str = r#"#!/bin/sh
if [ "$1" = fmt ] && [ -n "${HOLD_LINT:-}" ]; then : > "$HOLD_LINT"; sleep 60; fi
if [ "$1" = fmt ] && [ -n "${FAIL_LINT:-}" ]; then echo 'fmt stand-in: a diff'; exit 8; fi
if [ "$1" = test ]; then
    echo "client=${SYMPHONY_UI_DIST-unset}"
    echo "steps=${SYMPHONY_CHECK_STEPS-unset}"
    if [ -n "${FAIL_TESTS:-}" ]; then exit 9; fi
fi
exit 0
"#;

/// A repository of the test's own holding this workspace's justfile and the scripts its recipes
/// run, ignoring `build/` as this one does, with stand-ins for Cargo ([`RECIPES_CARGO`]), the client
/// check ([`CLIENT_CHECK`]) and `tools/rs-bin.sh` ([`RS_BIN`]): the recipes recorded by
/// [`RECORDER`].
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
        for name in ["cargo.sh", "record.sh", "verify.sh"] {
            fs::copy(tool(name), tree.join("tools").join(name)).unwrap();
        }
        executable(&tree.join("tools/client-check.sh"), CLIENT_CHECK);
        executable(&tree.join("tools/rs-bin.sh"), RS_BIN);
        fs::write(tree.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        fs::write(tree.join(".gitignore"), "/build/\n").unwrap();
        git(&tree, &["init", "-q", "-b", "main"]);
        git(&tree, &["add", "-A"]);
        git(&tree, &["commit", "-q", "-m", "base"]);
        let cargo = setup.dir.join("cargo");
        executable(&cargo, RECIPES_CARGO);
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
        self.just_with(&[recipe], vars)
    }

    /// `just ARGS` in the tree at top level, with `vars` set and no steps folder named but one
    /// `vars` names: what it did.
    fn just_with(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(self.command(args, vars).stdin(Stdio::null())).unwrap()
    }

    /// The command [`Recipes::just_with`] runs.
    fn command(&self, args: &[&str], vars: &[(&str, &str)]) -> Command {
        let mut command = Command::new("just");
        command
            .args(args)
            .current_dir(&self.tree)
            .env_remove(STEPS_VAR)
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
            .env_remove("CDPATH")
            .envs(vars.iter().copied());
        command
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

/// The steps of a verify, in the order it runs them, each with the label its record keeps.
const STEPS: [(&str, &str); 5] = [
    ("lint", "just lint-rs"),
    ("client-check", "just client-check"),
    ("size-check", "just size-check"),
    ("docs-audit", "docs audit"),
    ("tests", "tests"),
];

#[test]
fn a_verify_writes_its_run_and_a_step_each_for_lint_client_and_size_checks_docs_and_tests() {
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
    let first = symphony_api::fold(day_lines(runs, 6));
    let green = verify(false);
    assert!(green.status.success(), "{green:?}");
    let lines = day_lines(runs, 12);
    // A start line and an end line for each run and step, and none for the commands inside them.
    assert_eq!(lines.len(), 24, "{lines:#?}");
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
        assert_eq!(runs.len(), 6, "{runs:#?}");
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
        for (step, label) in STEPS {
            let line = named(runs, Some(step));
            assert_eq!(
                (line.pipeline.as_str(), line.label.as_str()),
                ("verify", label)
            );
            assert_eq!(line.parent.as_deref(), Some(verify.run.as_str()), "{step}");
            let ended = if step == "tests" { end } else { End::Green };
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
    for step in std::iter::once(None).chain(STEPS.map(|(step, _)| Some(step))) {
        assert_eq!(
            named(&second, step).rerun_of,
            Some(named(&first, step).run),
            "{step:?}"
        );
    }
    // Each step's operation is a child of its run's; the commands inside them are none.
    let history = setup.history(12);
    for record in &history {
        let line = second
            .iter()
            .chain(&first)
            .find(|line| line.run == record.operation_id)
            .unwrap();
        assert_eq!(record.parent_id, line.parent);
    }
}

/// The steps a verify listed in `folder`'s `steps.tsv`, each its name, exit status, log and
/// command, its seconds a whole number and the machine's load as it ended its last field: the load
/// averages over 1, 5 and 15 minutes and the processors.
fn listed(folder: &Path) -> Vec<(String, i32, PathBuf, String)> {
    let text = fs::read_to_string(folder.join("steps.tsv")).unwrap();
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 6, "{line:?}");
            assert!(fields[2].parse::<u64>().is_ok(), "{line:?}");
            let load: Vec<&str> = fields[5].split(' ').collect();
            assert_eq!(load.len(), 4, "{line:?}");
            assert!(
                load[..3].iter().all(|n| n.parse::<f64>().is_ok())
                    && load[3].parse::<usize>().is_ok_and(|n| n > 0),
                "{line:?}"
            );
            (
                fields[0].to_string(),
                fields[1].parse().unwrap(),
                PathBuf::from(fields[3]),
                fields[4].to_string(),
            )
        })
        .collect()
}

/// The client a tests step's log says it was handed, its real path; `unset` for none.
fn served(log: &str) -> String {
    let client = log.lines().next().unwrap().strip_prefix("client=").unwrap();
    if client == "unset" {
        return client.into();
    }
    Path::new(client)
        .canonicalize()
        .unwrap()
        .display()
        .to_string()
}

#[test]
fn a_verify_runs_every_step_after_red_ones_and_names_each_red_with_its_own_log() {
    let setup = Setup::new("record-script-verify-every-step");
    let recipes = Recipes::new(&setup);
    let tree = &recipes.tree;
    // A client an earlier check left behind, which no later verify serves.
    let stale = tree.join("build/verified-client/public");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("index.html"), "stale").unwrap();
    let red = recipes.just(
        "verify",
        &[
            ("FAIL_LINT", "yes"),
            ("FAIL_CLIENT", "yes"),
            ("NO_CLIENT", "yes"),
            ("FAIL_SIZE", "yes"),
            ("FAIL_TESTS", "yes"),
        ],
    );
    assert_eq!(red.status.code(), Some(1), "{red:?}");
    // Every step ran past the reds before it, each recorded as its own command ended.
    let lines = symphony_api::fold(day_lines(&recipes.runs, 6));
    let ended: Vec<_> = STEPS
        .iter()
        .map(|(step, _)| {
            let line = (lines.iter())
                .find(|line| line.step.as_deref() == Some(*step))
                .unwrap_or_else(|| panic!("no step {step}: {lines:#?}"));
            (*step, line.end, line.exit)
        })
        .collect();
    assert_eq!(
        ended,
        [
            ("lint", Some(End::Red), Some(8)),
            ("client-check", Some(End::Red), Some(1)),
            ("size-check", Some(End::Red), Some(3)),
            ("docs-audit", Some(End::Green), Some(0)),
            ("tests", Some(End::Red), Some(9)),
        ]
    );
    // Each step's output is in a log of its own, which the list names with its status.
    let folder = tree.join("build/verify");
    let steps = listed(&folder);
    let said: Vec<(&str, i32, &str)> = (steps.iter())
        .map(|(name, code, _, command)| (name.as_str(), *code, command.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("lint", 8, "just lint-rs"),
            ("client-check", 1, "just client-check build/verified-client"),
            ("size-check", 3, "just size-check"),
            ("docs-audit", 0, "just symphony-cli docs-audit --check"),
            (
                "tests",
                9,
                "tools/cargo.sh test --workspace --locked --no-fail-fast"
            ),
        ]
    );
    let path = |name: &str| steps.iter().find(|step| step.0 == name).unwrap().2.clone();
    let log = |name: &str| {
        assert_eq!(
            path(name).canonicalize().unwrap(),
            folder.join(format!("{name}.log")).canonicalize().unwrap()
        );
        fs::read_to_string(path(name)).unwrap()
    };
    assert!(log("lint").starts_with("fmt stand-in: a diff\n"));
    assert!(log("client-check").contains("client-check stand-in: differs\n"));
    assert!(log("size-check").starts_with("size stand-in: past its budget\n"));
    // The stale client is gone, and the tests say they were served none.
    assert!(!stale.exists(), "the client check's folder was cleared");
    let tests = log("tests");
    let none = "the client check built no client, so SYMPHONY_UI_DIST was unset: the browser \
                tests built the client themselves (build/symphony-ui), red if that build failed";
    assert_eq!(served(&tests), "unset", "{tests}");
    assert!(tests.contains("\nsteps=unset\n"), "{tests}");
    assert!(tests.ends_with(&format!("\nverify: {none}\n")), "{tests}");
    // What it said names each red step and its log, then the reds together.
    let out = String::from_utf8(red.stdout).unwrap();
    for (name, code) in [
        ("lint", 8),
        ("client-check", 1),
        ("size-check", 3),
        ("tests", 9),
    ] {
        let head = format!("verify: {name} red, exit {code} (");
        let tail = format!(" s); log {}", path(name).display());
        assert!(
            out.lines()
                .any(|line| line.starts_with(&head) && line.contains(&tail)),
            "{name} in:\n{out}"
        );
    }
    assert!(out.contains("\nverify: docs-audit green ("), "{out}");
    assert!(out.contains(&format!("; {none}\n")), "{out}");
    assert!(
        out.ends_with("\nverify: red in lint, client-check, size-check, tests\n"),
        "{out}"
    );
}

#[test]
fn a_verify_writes_its_steps_where_it_is_told_and_runs_the_steps_it_is_named() {
    let setup = Setup::new("record-script-verify-told");
    let recipes = Recipes::new(&setup);
    let tree = &recipes.tree;
    let folder = setup.dir.join("steps of a check");
    // The client check red on a client it built: the tests are served that one, and say so.
    let red = recipes.just(
        "verify",
        &[
            (STEPS_VAR, folder.to_str().unwrap()),
            ("FAIL_CLIENT", "yes"),
        ],
    );
    assert_eq!(red.status.code(), Some(1), "{red:?}");
    let steps = listed(&folder);
    let codes: Vec<(&str, i32)> = (steps.iter())
        .map(|(name, code, _, _)| (name.as_str(), *code))
        .collect();
    assert_eq!(
        codes,
        [
            ("lint", 0),
            ("client-check", 1),
            ("size-check", 0),
            ("docs-audit", 0),
            ("tests", 0)
        ]
    );
    assert!(!tree.join("build/verify").exists(), "nothing in the tree");
    let tests = fs::read_to_string(folder.join("tests.log")).unwrap();
    let built = tree.join("build/verified-client/public").canonicalize();
    assert_eq!(served(&tests), built.unwrap().display().to_string());
    // What a step runs is not told the folder: a verify it runs lists its steps elsewhere.
    assert!(tests.contains("\nsteps=unset\n"), "{tests}");
    assert!(
        tests.contains(
            "\nverify: the client check is red: the tests ran against the client it built from \
             this tree, "
        ) && tests.ends_with(", not the committed one\n"),
        "{tests}"
    );
    assert_eq!(
        steps[4].3,
        "SYMPHONY_UI_DIST=\"$PWD/build/verified-client/public\" tools/cargo.sh test \
         --workspace --locked --no-fail-fast"
    );
    assert!(
        String::from_utf8(red.stdout)
            .unwrap()
            .ends_with("\nverify: red in client-check\n")
    );

    // Named steps run alone, in the verify's order; with no client check, the tests build their
    // own client.
    let some = recipes.just_with(&["verify", "tests", "docs-audit"], &[]);
    assert!(some.status.success(), "{some:?}");
    let steps = listed(&tree.join("build/verify"));
    let names: Vec<&str> = steps.iter().map(|step| step.0.as_str()).collect();
    assert_eq!(names, ["docs-audit", "tests"]);
    let tests = fs::read_to_string(tree.join("build/verify/tests.log")).unwrap();
    assert_eq!(served(&tests), "unset", "{tests}");
    assert!(
        tests.contains("\nverify: the client check did not run in this verify, so "),
        "{tests}"
    );
    let mut lines = symphony_api::fold(day_lines(&recipes.runs, 9));
    lines.sort_by(|one, other| one.queued.total_cmp(&other.queued));
    let labels: Vec<&str> = (lines.iter())
        .filter(|line| line.step.is_none())
        .map(|line| line.label.as_str())
        .collect();
    assert_eq!(labels, ["just verify", "just verify tests docs-audit"]);
    assert_eq!(
        String::from_utf8(some.stdout).unwrap().lines().last(),
        Some("verify: green: docs-audit tests")
    );

    // A step it does not know runs nothing.
    let unknown = recipes.just_with(&["verify", "tests", "nothing"], &[]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(
        String::from_utf8_lossy(&unknown.stderr).starts_with(
            "verify: no step nothing; the steps are lint client-check size-check docs-audit tests\n"
        ),
        "{unknown:?}"
    );
    assert_eq!(listed(&tree.join("build/verify")).len(), 2, "as it was");
}

#[test]
fn a_verify_finds_its_steps_folder_whatever_cdpath_says() {
    let setup = Setup::new("record-script-verify-cdpath");
    let recipes = Recipes::new(&setup);
    // A CDPATH that leads `cd build/verify` there, so cd prints the folder it went to.
    let done = recipes.just_with(&["verify", "docs-audit"], &[("CDPATH", ".")]);
    assert!(done.status.success(), "{done:?}");
    let steps = listed(&recipes.tree.join("build/verify"));
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(
        (steps[0].0.as_str(), steps[0].1),
        ("docs-audit", 0),
        "{steps:?}"
    );
}

#[test]
fn an_interrupt_stops_a_verify_once_the_step_it_reached_ends() {
    let setup = Setup::new("record-script-verify-interrupt");
    let recipes = Recipes::new(&setup);
    let held = setup.dir.join("lint held");
    let mut command = recipes.command(&["verify"], &[("HOLD_LINT", held.to_str().unwrap())]);
    let child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !held.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the lint never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Ctrl-C: the interrupt a terminal sends the whole foreground group.
    let group = i32::try_from(child.id()).unwrap();
    // SAFETY: kill reads nothing of ours; a pid below 0 names the group the child leads.
    assert_eq!(unsafe { libc::kill(-group, libc::SIGINT) }, 0);
    let done = wait_output(child).unwrap();
    assert_eq!(done.status.code(), Some(130), "{done:?}");
    let steps = listed(&recipes.tree.join("build/verify"));
    let names: Vec<&str> = steps.iter().map(|step| step.0.as_str()).collect();
    assert_eq!(names, ["lint"], "no step after the one it reached");
    assert!(
        String::from_utf8(done.stdout)
            .unwrap()
            .ends_with("\nverify: interrupted in lint: no step after it ran\n"),
        "{:?}",
        String::from_utf8_lossy(&done.stderr)
    );
}
