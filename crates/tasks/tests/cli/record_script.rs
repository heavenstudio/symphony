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

use crate::recorder::kept;
use crate::{git, scratch};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
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
        assert_eq!(probe.stdout, b"run 1\n", "{probe:?}");
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
    for (count, args, phase) in [
        (3, ["clippy", "-q"], Phase::Build),
        (4, ["symphony-ui", "dist"], Phase::Build),
        (5, ["nextest", "run"], Phase::Test),
    ] {
        cargo_said(&setup, setup.cargo(&args, Some(RECORDER)), &args);
        assert_eq!(
            setup.history(count).last().unwrap().phase,
            phase,
            "{args:?}"
        );
    }
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
        runs[1].starts_with("run --phase test --label cargo test -- "),
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
            .filter(|run| run.starts_with("run --phase build --label cargo build -- "))
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
    fs::copy(tool("../justfile"), tree.join("justfile")).unwrap();
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
        runs[1].starts_with("run --phase test --label just test-rs one argument -- "),
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
