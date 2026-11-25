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
//!
//! Each run names a board that cannot be reached ([`UNREACHED`]), a scratch one, so it takes the
//! recorder tests' scale of the recorder's waits ([`SCALE`]), and the script's wait for its
//! recorder's probe is long ([`PROBE_WAIT`]) but in the case of that deadline: a loaded machine
//! never decides whether it is recorded. What the long wait would hide, a recorder grown slow to
//! answer, a probe of its own shows ([`PROMPT_PROBE`]).

use crate::children::Child as _;
use crate::recorder::{MACHINE, RUNS_VAR, SCALE, SCALE_VAR, UNREACHED, day_lines, git_says, kept};
use crate::{git, scratch};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};
use symphony_api::{End, RunLine};
use symphony_board::cli::machine_work::recorded::{self, Phase, Record};
use symphony_board::stamps::read::began;
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
        let mut command = Command::of(self.root.join("tools").join(script));
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
            .envs(UNREACHED)
            .env(SCALE_VAR, SCALE)
            .env_remove(recorded::PARENT_VAR)
            .env_remove(recorded::PIPELINE_VAR)
            .env_remove(recorded::TRIGGER_VAR)
            .env_remove(RUNS_VAR)
            .env_remove("TASKS_RECORDER")
            .env(PROBE_WAIT_VAR, PROBE_WAIT);
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

    /// The history once it keeps `count` operations, in the order they began, as the board's
    /// readers order runs ([`began`]). The history's own order is the order the recorders kept
    /// them, and a recorder keeps its operation after its command ends: on a loaded machine the
    /// recorder of a command that ran first can keep its record after the next command's.
    fn history(&self, count: usize) -> Vec<Record> {
        let mut history = kept(&self.work, count);
        symphony_board::order::sort_by(&mut history, &|a, b| began(a).total_cmp(&began(b)));
        history
    }
}

const RECORDER: &str = env!("CARGO_BIN_EXE_tasks");
/// The file [`logging`] logs its runs in.
const LOG_VAR: &str = "RECORDER_LOG";
/// What sets, in seconds, how long `tools/record.sh` waits for its recorder's probe.
const PROBE_WAIT_VAR: &str = "TASKS_RECORDER_PROBE_WAIT";
/// The probe's wait in every case here but the one of it: the backstop of a test's wait, so a
/// loaded machine never leaves Cargo to run unrecorded for a probe it answered late.
const PROBE_WAIT: &str = "120";
/// The probe's own wait, the script's when [`PROBE_WAIT_VAR`] names none: the default
/// `tools/record.sh` writes, which the probe's case holds it to. A recorder that answers no later
/// than this records the builds it is asked to.
const SCRIPT_PROBE_WAIT: Duration = Duration::from_secs(2);
/// What a probe of the real recorder must answer within, from its start to its end (the end of its
/// answer, which the script reads to), once at least, for the recorder to count as prompt: half of [`SCRIPT_PROBE_WAIT`]. Every other
/// case lengthens the script's wait ([`PROBE_WAIT`]), so this is what turns a recorder grown slow
/// to answer red. Spans measured: 12 ms to 193 ms at loads of 19 to 48 on 16 cores.
const PROMPT_PROBE: Duration = Duration::from_secs(1);
/// The folder a verify writes its steps' logs and list into, when one is named (tools/verify.sh).
const STEPS_VAR: &str = "SYMPHONY_CHECK_STEPS";

/// The recorder run once, before any script probes it: the first run of a binary just built can wait
/// seconds on the system's scan of it, longer than a probe waits for its answer.
fn warmed() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let probe = output(Command::of(RECORDER).args(["run", "--probe"])).unwrap();
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
            Command::of(&path)
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
        let probe = output(Command::of(&path).args(["run", "--probe"])).unwrap();
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
        (4, ["nextest", "run"], Phase::Test, "test"),
        (5, ["check", "-q"], Phase::Build, "build"),
        (6, ["doc", "-q"], Phase::Build, "build"),
        (7, ["rustc", "-q"], Phase::Build, "build"),
        (8, ["bench", "-q"], Phase::Test, "test"),
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
    for args in [vec!["fmt", "--check"], vec!["metadata"]] {
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
    // The probe's own two seconds: the deadline this case is of.
    let done = setup.run(
        setup
            .command("cargo.sh", &["test"], Some(hung.to_str().unwrap()))
            .env_remove(PROBE_WAIT_VAR)
            .env("RELEASE", &release)
            .env("ENDED", &ended),
    );
    // Cargo ran and ended while the probe still hung.
    cargo_said(&setup, done, &["test"]);
    assert!(!ended.exists(), "cargo waited for the probe to end");
    fs::write(&release, "").unwrap();
    test_time::until_true(
        "the probe ended once released",
        test_time::backstop(std::time::Duration::from_secs(30)),
        || ended.exists(),
    )
    .unwrap();
    // It was asked, and never asked to record: its one run was the probe.
    assert_eq!(setup.logged(), "run --probe\n");
}

/// The real recorder answers a probe well within the script's own wait ([`PROMPT_PROBE`]): probed
/// until one answer comes that soon, so a loaded machine's slow probes, however many, never fail
/// it, and a recorder slow on every probe stalls the wait. A failure says each probe's span. The
/// script's wait is [`SCRIPT_PROBE_WAIT`] where none is named, as the script says it.
#[test]
fn the_recorder_answers_its_probe_well_within_the_scripts_own_wait() {
    let default = format!("${{{PROBE_WAIT_VAR}:-{}}}", SCRIPT_PROBE_WAIT.as_secs());
    let script = fs::read_to_string(tool("record.sh")).unwrap();
    assert!(
        script.contains(&format!("read -r -t \"{default}\"")),
        "tools/record.sh waits for its probe other than {default}"
    );
    warmed();
    let mut spans = Vec::new();
    let prompt = test_time::until(
        format!("a probe of the recorder answered within {PROMPT_PROBE:?}"),
        test_time::backstop(SCRIPT_PROBE_WAIT),
        || {
            let asked = Instant::now();
            let probe = output(
                Command::of(RECORDER)
                    .args(["run", "--probe"])
                    .stdin(Stdio::null()),
            )
            .unwrap();
            let span = asked.elapsed();
            assert_eq!(probe.stdout, b"run 1\nrun 2\n", "{probe:?}");
            spans.push(span.as_secs_f64());
            (span < PROMPT_PROBE).then_some(span)
        },
    );
    if let Err(stall) = prompt {
        panic!(
            "{stall}: the script waits {SCRIPT_PROBE_WAIT:?} for the answer; each probe's span, in \
             seconds: {spans:?}"
        );
    }
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
            Command::of(root.join("tools/cargo.sh"))
                .arg("build")
                .env("CARGO", &cargo)
                .env("TASKS_WORK", &work)
                .env(LOG_VAR, &log)
                .env_remove("TASKS_SESSION")
                .env_remove("TASKS_TASK")
                .envs(UNREACHED)
                .env(SCALE_VAR, SCALE)
                .env(PROBE_WAIT_VAR, PROBE_WAIT)
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
        "#!/bin/sh\nprintf 'arg=<%s>\\n' \"$@\"\nprintf 'hc=<%s>\\n' \"${HC_BIN-unset}\"\nexit 5\n",
    );
    let recipe = |task: Option<&str>, hc: Option<&str>| {
        let mut command = Command::of("just");
        command
            .args(["test-rs", "one argument"])
            .current_dir(&tree)
            .env("TASKS_WORK", &setup.work)
            .env("TASKS_RECORDER", logging())
            .env(LOG_VAR, &setup.log)
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .envs(UNREACHED)
            .env(SCALE_VAR, SCALE)
            .env(PROBE_WAIT_VAR, PROBE_WAIT)
            .env_remove(recorded::PARENT_VAR)
            .env_remove("HC_BIN");
        if let Some(task) = task {
            command.env("TASKS_TASK", task);
        }
        if let Some(hc) = hc {
            command.env("HC_BIN", hc);
        }
        let done = output(command.stdin(Stdio::null())).unwrap();
        assert_eq!(done.status.code(), Some(5), "{done:?}");
        assert_eq!(
            String::from_utf8(done.stdout).unwrap(),
            format!(
                "arg=<test>\narg=<--locked>\narg=<one argument>\nhc=<{}>\n",
                hc.unwrap_or("/usr/bin/false")
            )
        );
    };
    // No tools/record.sh here: the recipe runs as it is, its recorder never asked.
    // Its tests are named an hc that answers nothing, never this machine's, unless the caller
    // names one.
    recipe(None, None);
    recipe(None, Some("/the/callers/hc"));
    assert_eq!(setup.logged(), "", "a tree without the script recorded");
    fs::copy(tool("record.sh"), tree.join("tools/record.sh")).unwrap();
    recipe(Some("tools/recipe"), None);
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

/// `tools/rs-bin.sh`'s stand-in: red under `FAIL_PUBLISH`, the size budgets under `FAIL_SIZE` and
/// the crates check under `FAIL_CRATES`.
const RS_BIN: &str = r#"#!/bin/sh
if [ -n "${FAIL_PUBLISH:-}" ]; then exit 4; fi
if [ "$2" = size-budget ] && [ -n "${FAIL_SIZE:-}" ]; then echo 'size stand-in: past its budget'; exit 3; fi
if [ "$2" = crates-check ] && [ -n "${FAIL_CRATES:-}" ]; then echo 'crates stand-in: sources moved'; exit 5; fi
exit 0
"#;

/// The web client's checks' stand-in (`apps/symphony-web/check.sh`, which the `client-lint` recipe
/// runs last in `lint-rs`): green, as what these tests hold the recipes to is how they record, not
/// the client's own checks on its Node.
const CLIENT_LINT: &str = "#!/bin/sh\nexit 0\n";

/// The client's build's stand-in (`apps/symphony-web/build.sh`, which the `symphony-ui-dist`
/// recipe runs): green, building nothing, as the client check's stand-in writes the client.
const CLIENT_BUILD: &str = "#!/bin/sh\nif [ \"$1\" = build/verified-client ] && [ -n \"${NO_CLIENT:-}\" ]; then exit 0; fi\nmkdir -p \"$1/public\"\nprintf client > \"$1/public/index.html\"\n";

/// Cargo's stand-in: `fmt` red under `FAIL_LINT`, and held a minute under `HOLD_LINT`, once it has
/// made the file that names; `metadata` saying no target, so the tests' lanes are the libraries'
/// and binaries' own tests and the doc tests; `test` (the tests' build and each lane) saying the
/// client it was handed and the steps folder it sees, red under `FAIL_TESTS`; but the hosted
/// board's tests, which name its feature, say nothing and are red under `FAIL_HOSTED` alone, and
/// the board's raw-statement fence, a test target `lint-rs` runs as one of its checks, says nothing
/// and is green.
const RECIPES_CARGO: &str = r#"#!/bin/sh
if [ "$1" = metadata ]; then exit 0; fi
if [ "$1" = nextest ] && [ "$2" = list ]; then
    printf '{"rust-binaries":{}}\n'
    exit 0
fi
if [ "$1" = fmt ] && [ -n "${HOLD_LINT:-}" ]; then : > "$HOLD_LINT"; sleep 60; fi
if [ "$1" = fmt ] && [ -n "${FAIL_LINT:-}" ]; then echo 'fmt stand-in: a diff'; exit 8; fi
case " $* " in *' --features hosted-postgresql '*)
    if [ -n "${FAIL_HOSTED:-}" ]; then echo 'hosted stand-in: no PostgreSQL 16'; exit 4; fi
    exit 0 ;;
esac
case " $* " in *' --test raw_fence '*) exit 0 ;; esac
if [ "$1" = test ] || [ "$1" = nextest ]; then
    echo "client=${SYMPHONY_UI_DIST-unset}"
    echo "steps=${SYMPHONY_CHECK_STEPS-unset}"
    if [ -n "${FAIL_TESTS:-}" ]; then exit 9; fi
fi
exit 0
"#;

/// A repository of the test's own holding this workspace's justfile and the scripts its recipes
/// run, ignoring `build/` as this one does, with stand-ins for Cargo ([`RECIPES_CARGO`]), the client
/// check ([`CLIENT_CHECK`]), the web client's checks ([`CLIENT_LINT`]), its build
/// ([`CLIENT_BUILD`]) and `tools/rs-bin.sh`
/// ([`RS_BIN`]): the recipes recorded by [`RECORDER`].
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
        for name in [
            "cargo.sh",
            "record.sh",
            "nextest.sh",
            "test-lanes.sh",
            "verify.sh",
        ] {
            fs::copy(tool(name), tree.join("tools").join(name)).unwrap();
        }
        super::test_lanes::mock_nextest(&tree);
        executable(
            &tree.join(".target/debug/symphony-build"),
            "#!/bin/sh\ncat\n",
        );
        executable(&tree.join("tools/client-check.sh"), CLIENT_CHECK);
        executable(&tree.join("apps/symphony-web/check.sh"), CLIENT_LINT);
        executable(&tree.join("apps/symphony-web/build.sh"), CLIENT_BUILD);
        executable(&tree.join("tools/rs-bin.sh"), RS_BIN);
        executable(
            &tree.join("tools/semver.sh"),
            "#!/bin/sh\nif [ -n \"${FAIL_SEMVER:-}\" ]; then echo 'semver stand-in: breaking API'; exit 6; fi\nexit 0\n",
        );
        fs::write(tree.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        fs::write(tree.join(".gitignore"), "/build/\n/.target/\n").unwrap();
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
        let mut command = Command::of("just");
        command
            .args(args)
            .current_dir(&self.tree)
            .env_remove(STEPS_VAR)
            .env("CARGO", &self.cargo)
            .env("CARGO_TARGET_DIR", self.tree.join(".target"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.tree.join(".target/nextest-fixture/bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env(
                "NEXT_TOOL_ARCHIVE",
                self.tree.join(".target/nextest-fixture/tool.tar.gz"),
            )
            .env_remove("FAIL_NEXTEST_SETUP")
            .env("TASKS_WORK", &self.work)
            .env(RUNS_VAR, &self.runs)
            .env("TASKS_RECORDER", RECORDER)
            .env("SYMPHONY_MACHINE_DIR", &self.machine)
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .envs(UNREACHED)
            .env(SCALE_VAR, SCALE)
            .env(PROBE_WAIT_VAR, PROBE_WAIT)
            .env_remove(recorded::PARENT_VAR)
            .env_remove(recorded::PIPELINE_VAR)
            .env_remove(recorded::TRIGGER_VAR)
            .env_remove("CDPATH")
            .envs(vars.iter().copied());
        command
    }
}

#[test]
fn one_recipe_check_commands_refuse_added_recipes_and_trailing_options_before_execution() {
    let setup = Setup::new("record-script-one-recipe");
    let recipes = Recipes::new(&setup);
    let marker = recipes.tree.join("executed with spaces");
    let other = recipes.tree.join("other.just");
    let justfile = r#"marker := env_var("CHECK_MARKER")
lint-rs:
    @echo lint >> {{quote(marker)}}
release:
    @echo release >> {{quote(marker)}}
symphony:
    @echo server >> {{quote(marker)}}
"#;
    let vars = [("CHECK_MARKER", marker.to_str().unwrap())];
    fs::write(recipes.tree.join("justfile"), justfile).unwrap();
    fs::write(&other, justfile).unwrap();
    let other = other.to_str().unwrap();
    let unguarded = recipes.just_with(&["lint-rs", "release"], &vars);
    assert!(unguarded.status.success(), "{unguarded:?}");
    assert_eq!(fs::read_to_string(&marker).unwrap(), "lint\nrelease\n");
    fs::remove_file(&marker).unwrap();
    for tail in [
        vec!["release"],
        vec!["symphony"],
        vec!["--justfile", other],
        vec!["-f", other],
        vec!["--working-directory", "/tmp"],
        vec!["--shell", "/bin/sh"],
        vec!["--command", "touch", marker.to_str().unwrap()],
        vec!["--set", "record", "arbitrary"],
        vec!["record=arbitrary"],
        vec!["--no-deps"],
    ] {
        let mut args = vec!["--one", "--", "lint-rs"];
        args.extend_from_slice(&tail);
        let done = recipes.just_with(&args, &vars);
        assert!(!done.status.success(), "{args:?}: {done:?}");
        assert!(!marker.exists(), "{args:?} executed a command: {done:?}");
    }
    let done = recipes.just_with(&["--one", "--", "lint-rs"], &vars);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(fs::read_to_string(&marker).unwrap(), "lint\n");
}

#[test]
fn one_recipe_test_commands_keep_argument_boundaries_task_commit_and_red_status() {
    let setup = Setup::new("record-script-one-recipe-records");
    let recipes = Recipes::new(&setup);
    let commit = git_says(&recipes.tree, &["rev-parse", "HEAD"]);
    let log = setup.dir.join("cargo-arguments");
    executable(
        &recipes.cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CHECK_ARGS\"\nexit \"$CHECK_EXIT\"\n",
    );
    for code in ["0", "7"] {
        let done = recipes.just_with(
            &[
                "--one",
                "--",
                "test-rs",
                "--",
                "a filter with spaces",
                "release",
                "--justfile",
                "/tmp/other.just",
            ],
            &[
                ("CHECK_ARGS", log.to_str().unwrap()),
                ("CHECK_EXIT", code),
                ("TASKS_TASK", "tools/one-recipe"),
                ("TASKS_SESSION", "fixture-worker"),
            ],
        );
        assert_eq!(done.status.code(), Some(code.parse().unwrap()), "{done:?}");
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "test\n--locked\n--\na filter with spaces\nrelease\n--justfile\n/tmp/other.just\n"
        );
    }
    let records = setup.history(2);
    for (record, code) in records.iter().zip([0, 7]) {
        assert_eq!(record.task.as_deref(), Some("tools/one-recipe"));
        assert_eq!(record.handle.as_deref(), Some("fixture-worker"));
        assert_eq!(record.commit.as_deref(), Some(commit.as_str()));
        assert_eq!(record.exit_code, Some(code));
        assert_eq!(record.phase, Phase::Test);
    }
}

#[test]
fn one_recipe_document_checks_keep_fixed_arguments_and_both_records() {
    let setup = Setup::new("record-script-one-recipe-docs");
    let recipes = Recipes::new(&setup);
    let log = setup.dir.join("document-arguments");
    executable(
        &recipes.tree.join("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf '%s|' \"$@\" >> \"$DOC_ARGS\"\nprintf '\\n' >> \"$DOC_ARGS\"\n",
    );
    let vars = [("DOC_ARGS", log.to_str().unwrap())];
    for tail in ["--tidy", "release", "--justfile"] {
        let done = recipes.just_with(&["--one", "--", "docs-check", tail], &vars);
        assert!(!done.status.success(), "{tail}: {done:?}");
        assert!(!log.exists(), "{tail}: a check ran before the refusal");
    }
    let done = recipes.just_with(&["--one", "--", "docs-check"], &vars);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "symphony|docs-audit|--check|\nsymphony|status|--lint|\n"
    );
    let labels: Vec<_> = setup
        .history(2)
        .into_iter()
        .map(|record| {
            assert_eq!(record.phase, Phase::Test);
            assert_eq!(record.exit_code, Some(0));
            record.label
        })
        .collect();
    assert_eq!(
        labels,
        [
            "just symphony-cli docs-audit --check",
            "just symphony-cli status --lint"
        ]
    );
}

#[test]
fn the_semver_recipe_checks_all_kit_and_helper_packages_and_records_a_break_as_red() {
    let setup = Setup::new("record-script-semver-check");
    let recipes = Recipes::new(&setup);
    executable(
        &recipes.tree.join("tools/semver.sh"),
        "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\nexit \"${SEMVER_EXIT:-0}\"\n",
    );
    git(&recipes.tree, &["add", "tools/semver.sh"]);
    git(&recipes.tree, &["commit", "-q", "-m", "semver stand-in"]);
    let commit = git_says(&recipes.tree, &["rev-parse", "HEAD"]);
    for code in ["0", "1"] {
        let done = recipes.just("semver-check", &[("SEMVER_EXIT", code)]);
        assert_eq!(done.status.code(), Some(code.parse().unwrap()), "{done:?}");
        assert_eq!(
            String::from_utf8(done.stdout).unwrap(),
            "<--all-features>\n<-p>\n<symphony-plugin>\n<-p>\n<symphony-blocks>\n\
             <-p>\n<symphony-docs>\n<-p>\n<symphony-env>\n<-p>\n<symphony-args>\n\
             <-p>\n<symphony-schema>\n<-p>\n<process-cwd>\n<-p>\n<repository-read>\n"
        );
    }
    let lines = symphony_api::fold(day_lines(&recipes.work, &recipes.runs, 2));
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].end, Some(End::Green));
    assert_eq!(lines[1].end, Some(End::Red));
    for line in lines {
        assert_eq!(line.pipeline, "semver");
        assert_eq!(line.label, "just semver-check");
        assert_eq!(line.commit.as_deref(), Some(commit.as_str()));
    }
}

#[test]
fn the_crates_check_recipe_records_its_task_tip_streams_and_status_or_runs_without_a_recorder() {
    let setup = Setup::new("record-script-crates-check");
    let recipes = Recipes::new(&setup);
    // Keep Just's separate error writer out of the checker's stream contract: it races the
    // recorder's pipe forwarding after the checker exits. The real recipe body stays intact.
    let justfile = recipes.tree.join("justfile");
    fs::write(
        &justfile,
        fs::read_to_string(&justfile)
            .unwrap()
            .replace("\ncrates-check:", "\n[no-exit-message]\ncrates-check:"),
    )
    .unwrap();
    executable(
        &recipes.tree.join("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf 'arg=<%s>\\n' \"$@\"\nprintf 'crates diagnostic\\n' >&2\nexit \"${CRATES_EXIT:-0}\"\n",
    );
    git(&recipes.tree, &["add", "tools/rs-bin.sh", "justfile"]);
    git(&recipes.tree, &["commit", "-q", "-m", "checker stand-in"]);
    let commit = git_says(&recipes.tree, &["rev-parse", "HEAD"]);
    let task = "tools/crates-check";
    let recorder = logging().to_str().unwrap();
    let log = setup.log.to_str().unwrap();
    let mut expected = Vec::new();
    for code in ["0", "5"] {
        let done = recipes.just(
            "crates-check",
            &[
                ("TASKS_TASK", task),
                ("CRATES_EXIT", code),
                ("TASKS_RECORDER", recorder),
                (LOG_VAR, log),
            ],
        );
        assert_eq!(done.status.code(), Some(code.parse().unwrap()), "{done:?}");
        assert_eq!(
            done.stdout,
            b"arg=<symphony-build>\narg=<crates-check>\narg=<.>\n"
        );
        assert_eq!(done.stderr, b"crates diagnostic\n", "{done:?}");
        expected.push(done);
    }
    let logged = setup.logged();
    assert_eq!(logged.lines().count(), 4, "{logged}");
    let history = setup.history(2);
    for (record, code) in history.iter().zip([0, 5]) {
        assert_eq!(
            (record.label.as_str(), record.phase, record.task.as_deref()),
            ("just crates-check", Phase::Test, Some(task))
        );
        assert_eq!(record.commit.as_deref(), Some(commit.as_str()));
        assert_eq!(record.exit_code, Some(code));
        assert_eq!(pipeline(record), Some("crates"));
        assert!(record.parent_id.is_none(), "{record:?}");
    }
    let lines = symphony_api::fold(day_lines(&recipes.work, &recipes.runs, 2));
    assert_eq!(lines.len(), 2, "{lines:?}");
    for line in &lines {
        assert_eq!(line.commit.as_deref(), Some(commit.as_str()));
        assert_eq!(line.dirty, Some(false), "{line:?}");
        assert!(line.step.is_none(), "{line:?}");
    }

    // Disabled or absent recorder, then no script: the same checker and streams, no record.
    let missing = setup.dir.join("no recorder");
    for fallback in ["off", missing.to_str().unwrap(), recorder] {
        if fallback == recorder {
            fs::remove_file(recipes.tree.join("tools/record.sh")).unwrap();
        }
        for (code, expected) in ["0", "5"].into_iter().zip(&expected) {
            let done = recipes.just(
                "crates-check",
                &[
                    ("TASKS_RECORDER", fallback),
                    ("CRATES_EXIT", code),
                    (LOG_VAR, log),
                ],
            );
            assert_eq!(done.status, expected.status, "{done:?}");
            assert_eq!(done.stdout, expected.stdout, "{done:?}");
            assert_eq!(done.stderr, expected.stderr, "{done:?}");
        }
    }
    assert_eq!(setup.logged(), logged, "a fallback ran the recorder");
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
    let mut lines = symphony_api::fold(day_lines(&recipes.work, &recipes.runs, 6));
    symphony_board::order::sort_by(&mut lines, &|one, other| {
        one.queued.total_cmp(&other.queued)
    });
    // Each line's parent by its label.
    let label = |run: &str| {
        (lines.iter())
            .find(|line| line.run == run)
            .unwrap()
            .label
            .as_str()
    };
    let said: Vec<_> = lines
        .iter()
        .map(|line| {
            (
                line.label.as_str(),
                line.pipeline.as_str(),
                line.step.as_deref(),
                line.parent.as_deref().map(label),
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
            // The web client's checks, which lint-rs runs last: a step of its run.
            (
                "just client-lint",
                "lint",
                Some("client-lint"),
                Some("just lint-rs"),
                Some(End::Green)
            ),
            ("just client-check", "client", None, None, Some(End::Green)),
            ("just size-check", "size", None, None, Some(End::Green)),
        ]
    );
    // A client check on the tree a client update failed on repeats no update; the next client
    // check repeats the one before.
    assert_eq!(
        (
            lines[1].rerun_of.as_deref(),
            lines[2].rerun_of.as_deref(),
            lines[3].rerun_of.as_deref()
        ),
        (None, None, None)
    );
    assert_eq!(lines[4].rerun_of.as_deref(), Some(lines[1].run.as_str()));
}

/// The steps of a verify, in the order it runs them, each with the label its record keeps.
const STEPS: [(&str, &str); 9] = [
    ("lint", "just lint-rs"),
    ("client-check", "just client-check"),
    ("size-check", "just size-check"),
    ("docs-audit", "docs audit"),
    ("crates-check", "crates check"),
    ("semver-check", "semver check"),
    ("hosted", "hosted check"),
    ("nextest-setup", "nextest setup"),
    ("tests", "tests"),
];

#[test]
fn a_cold_release_target_is_provisioned_before_tests_and_a_setup_red_starts_no_tests() {
    for fails in [false, true] {
        // Separate repositories: the failed cold run must not share the successful run's logs.
        let setup = Setup::new(&format!("record-script-nextest-setup-{fails}"));
        let recipes = Recipes::new(&setup);
        assert!(
            !recipes.tree.join("build/verify/tests.log").exists(),
            "the fixture starts without a prior tests log"
        );
        fs::remove_dir_all(recipes.tree.join(".target/tools")).unwrap();
        let vars = if fails {
            vec![("FAIL_NEXTEST_SETUP", "1")]
        } else {
            vec![]
        };
        let done = recipes.just_with(&["verify", "tests"], &vars);
        assert_eq!(done.status.code(), Some(i32::from(fails)), "{done:?}");
        let steps = listed(&recipes.tree.join("build/verify"));
        let names: Vec<_> = steps.iter().map(|step| (step.0.as_str(), step.1)).collect();
        if fails {
            assert_eq!(
                names,
                [("nextest-setup", 2), ("tests", -1)],
                "the tests are listed as not run, never left out: a rerun must know of them"
            );
            assert!(steps[1].2.ends_with("nextest-setup.log"), "{steps:?}");
            assert!(
                !recipes.tree.join("build/verify/tests.log").exists(),
                "tests log: {:?}; verify: {done:?}",
                fs::read_to_string(recipes.tree.join("build/verify/tests.log"))
            );
            let log =
                fs::read_to_string(recipes.tree.join("build/verify/nextest-setup.log")).unwrap();
            assert!(log.contains("nextest: cannot fetch"), "{log}");
        } else {
            assert_eq!(names, [("nextest-setup", 0), ("tests", 0)]);
            assert!(
                recipes
                    .tree
                    .join(".target/tools/nextest/0.9.152/bin/cargo-nextest")
                    .is_file()
            );
        }
        let lines = symphony_api::fold(day_lines(
            &recipes.work,
            &recipes.runs,
            if fails { 2 } else { 3 },
        ));
        let provision = lines
            .iter()
            .find(|line| line.step.as_deref() == Some("nextest-setup"))
            .unwrap();
        assert_eq!(provision.label, "nextest setup");
        assert_eq!(
            provision.end,
            Some(if fails { End::Red } else { End::Green })
        );
        assert_eq!(
            lines
                .iter()
                .any(|line| line.step.as_deref() == Some("tests")),
            !fails
        );
    }
}

#[test]
fn a_verify_writes_its_run_and_a_step_each_for_lint_client_size_and_crates_checks_docs_and_tests() {
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
    let first = symphony_api::fold(day_lines(&recipes.work, runs, 11));
    let green = verify(false);
    assert!(green.status.success(), "{green:?}");
    let lines = day_lines(&recipes.work, runs, 22);
    // A start line and an end line for each run and step, and none for the commands inside them.
    assert_eq!(lines.len(), 44, "{lines:#?}");
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
        assert_eq!(runs.len(), 11, "{runs:#?}");
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
        // The web client's checks, which lint-rs runs last: a step of the lint step.
        let client_lint = named(runs, Some("client-lint"));
        assert_eq!(
            (
                client_lint.pipeline.as_str(),
                client_lint.label.as_str(),
                client_lint.parent.as_deref(),
                client_lint.end
            ),
            (
                "verify",
                "just client-lint",
                Some(named(runs, Some("lint")).run.as_str()),
                Some(End::Green)
            )
        );
        for line in runs {
            assert_eq!(line.commit.as_deref(), Some(commit.as_str()), "{line:#?}");
            // The tree and the checkout's cleanliness are git's answers, which the recorder asks
            // for no longer than its bound at the tests' scale ([`SCALE`]: 12 s), so a line
            // without them is a stall, said with the machine's load.
            let stalled = || format!("{line:#?}; {}", test_time::Load::now());
            assert_eq!(
                line.tree.as_deref(),
                Some(checked.as_str()),
                "{}",
                stalled()
            );
            assert_eq!(
                (line.dirty, line.branch.as_deref()),
                (Some(false), Some("main")),
                "{}",
                stalled()
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
    let mut steps = vec![None, Some("client-lint")];
    steps.extend(STEPS.map(|(step, _)| Some(step)));
    for step in steps {
        assert_eq!(
            named(&second, step).rerun_of,
            Some(named(&first, step).run),
            "{step:?}"
        );
    }
    // Each step's operation is a child of its run's; the commands inside them are none.
    let history = setup.history(22);
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
    let (steps, _) = listed_with_parts(folder);
    steps
}

/// The lanes of its tests a verify listed in `folder`, each `tests/LANE`, after the step they are
/// parts of: its name and exit status.
fn lanes(folder: &Path) -> Vec<(String, i32)> {
    let (_, parts) = listed_with_parts(folder);
    (parts.into_iter())
        .map(|(name, code, _, _)| (name, code))
        .collect()
}

type Listed = Vec<(String, i32, PathBuf, String)>;

/// [`listed`], and the parts of a step listed after it (`STEP/PART`), each line of the one shape.
fn listed_with_parts(folder: &Path) -> (Listed, Listed) {
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
                // A step listed as not run (`-`) reads as -1 here.
                if fields[1] == "-" {
                    -1
                } else {
                    fields[1].parse().unwrap()
                },
                PathBuf::from(fields[3]),
                fields[4].to_string(),
            )
        })
        .partition(|step| !step.0.contains('/'))
}

/// The client a tests step's log says it was handed, its real path; `unset` for none.
fn served(log: &str) -> String {
    let client = log
        .lines()
        .find_map(|line| line.strip_prefix("client="))
        .unwrap();
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
            ("FAIL_CRATES", "yes"),
            ("FAIL_SEMVER", "yes"),
            ("FAIL_HOSTED", "yes"),
            ("FAIL_TESTS", "yes"),
        ],
    );
    assert_eq!(red.status.code(), Some(1), "{red:?}");
    // Every step ran past the reds before it, each recorded as its own command ended.
    let lines = symphony_api::fold(day_lines(&recipes.work, &recipes.runs, 10));
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
            ("crates-check", Some(End::Red), Some(5)),
            ("semver-check", Some(End::Red), Some(6)),
            ("hosted", Some(End::Red), Some(4)),
            ("nextest-setup", Some(End::Green), Some(0)),
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
            ("crates-check", 5, "just crates-check"),
            ("semver-check", 6, "just semver-check"),
            ("hosted", 4, "just hosted-check"),
            ("nextest-setup", 0, "just nextest-setup"),
            ("tests", 9, "tools/test-lanes.sh"),
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
    assert!(log("crates-check").starts_with("crates stand-in: sources moved\n"));
    assert!(log("semver-check").starts_with("semver stand-in: breaking API\n"));
    assert!(log("hosted").starts_with("hosted stand-in: no PostgreSQL 16\n"));
    // The stale client is gone, and the tests say they were served none.
    assert!(!stale.exists(), "the client check's folder was cleared");
    let tests = log("tests");
    let none = "the client check built no client, so SYMPHONY_UI_DIST was unset: the \
                tests runner published one client before scheduling (build/symphony-ui), red if that build failed";
    assert_eq!(
        served(&tests),
        tree.join("build/symphony-ui/public")
            .canonicalize()
            .unwrap()
            .display()
            .to_string(),
        "{tests}"
    );
    assert!(tests.contains("\nsteps=unset\n"), "{tests}");
    assert!(tests.ends_with(&format!("\nverify: {none}\n")), "{tests}");
    // What it said names each red step and its log, then the reds together.
    let out = String::from_utf8(red.stdout).unwrap();
    for (name, code) in [
        ("lint", 8),
        ("client-check", 1),
        ("size-check", 3),
        ("crates-check", 5),
        ("semver-check", 6),
        ("hosted", 4),
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
        out.ends_with(
            "\nverify: red in lint, client-check, size-check, crates-check, semver-check, hosted, \
             tests\n"
        ),
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
            ("crates-check", 0),
            ("semver-check", 0),
            ("hosted", 0),
            ("nextest-setup", 0),
            ("tests", 0)
        ]
    );
    assert!(!tree.join("build/verify").exists(), "nothing in the tree");
    assert!(
        !tree.join("build/test-lanes").exists(),
        "nothing in the tree"
    );
    let tests = fs::read_to_string(folder.join("tests.log")).unwrap();
    let built = tree.join("build/verified-client/public").canonicalize();
    let built = built.unwrap().display().to_string();
    assert_eq!(served(&tests), built);
    // The nextest batch and doctests keep their live logs beside the recorded step.
    for batch in ["nextest", "doc"] {
        assert!(
            tests.contains(&format!("/steps of a check/tests-lanes/{batch}.log (")),
            "{tests}"
        );
        let log = fs::read_to_string(folder.join(format!("tests-lanes/{batch}.log"))).unwrap();
        assert_eq!(log, format!("client={built}\nsteps=unset\n"), "{batch}");
    }
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
        steps[8].3,
        "SYMPHONY_UI_DIST=\"$PWD/build/verified-client/public\" tools/test-lanes.sh"
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
    assert_eq!(names, ["docs-audit", "nextest-setup", "tests"]);
    let tests = fs::read_to_string(tree.join("build/verify/tests.log")).unwrap();
    assert_eq!(
        served(&tests),
        tree.join("build/symphony-ui/public")
            .canonicalize()
            .unwrap()
            .display()
            .to_string(),
        "{tests}"
    );
    assert!(tree.join("build/verify/tests-lanes/doc.log").is_file());
    assert!(
        tests.contains("\nverify: the client check did not run in this verify, so "),
        "{tests}"
    );
    let mut lines = symphony_api::fold(day_lines(&recipes.work, &recipes.runs, 15));
    symphony_board::order::sort_by(&mut lines, &|one, other| {
        one.queued.total_cmp(&other.queued)
    });
    let labels: Vec<&str> = (lines.iter())
        .filter(|line| line.step.is_none())
        .map(|line| line.label.as_str())
        .collect();
    assert_eq!(labels, ["just verify", "just verify tests docs-audit"]);
    assert_eq!(
        String::from_utf8(some.stdout).unwrap().lines().last(),
        Some("verify: green: docs-audit nextest-setup tests")
    );

    // A step it does not know runs nothing.
    let unknown = recipes.just_with(&["verify", "tests", "nothing"], &[]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(
        String::from_utf8_lossy(&unknown.stderr).starts_with(
            "verify: no step nothing; the steps are lint client-check size-check docs-audit \
             crates-check semver-check hosted nextest-setup tests\n"
        ),
        "{unknown:?}"
    );
    assert_eq!(listed(&tree.join("build/verify")).len(), 3, "as it was");
    // Each lane that ran is listed after the tests step, as a part of it.
    assert_eq!(
        lanes(&tree.join("build/verify")),
        [
            ("tests/nextest".to_string(), 0),
            ("tests/doc".to_string(), 0)
        ]
    );

    // A lane named as `tests/LANE` runs alone, after the runner's setup, and is the one listed.
    let one = recipes.just_with(&["verify", "tests/doc"], &[("EXPECT_SERIAL", "")]);
    assert!(one.status.success(), "{one:?}");
    let steps = listed(&tree.join("build/verify"));
    let names: Vec<&str> = steps.iter().map(|step| step.0.as_str()).collect();
    assert_eq!(names, ["nextest-setup", "tests"]);
    assert_eq!(steps[1].3, "tools/test-lanes.sh doc");
    assert_eq!(
        lanes(&tree.join("build/verify")),
        [("tests/doc".to_string(), 0)]
    );
    // The step named beside one of its lanes runs whole.
    let whole = recipes.just_with(&["verify", "tests/doc", "tests"], &[]);
    assert!(whole.status.success(), "{whole:?}");
    assert_eq!(
        listed(&tree.join("build/verify"))[1].3,
        "tools/test-lanes.sh"
    );
    assert_eq!(lanes(&tree.join("build/verify")).len(), 2);
    // A lane it does not know is the tests step's red, and none is listed.
    let unknown = recipes.just_with(&["verify", "tests/nothing"], &[]);
    assert_eq!(unknown.status.code(), Some(1), "{unknown:?}");
    assert_eq!(lanes(&tree.join("build/verify")), []);
    let log = fs::read_to_string(tree.join("build/verify/tests.log")).unwrap();
    assert!(
        log.contains("test-lanes: no lane nothing; the lanes are build fixture nextest "),
        "{log}"
    );

    // What each step and lane reads is said, and nothing runs.
    let before = fs::read_to_string(tree.join("build/verify/steps.tsv")).unwrap();
    let said = output(
        Command::of("bash")
            .args(["tools/verify.sh", "justfile", "--inputs"])
            .current_dir(tree)
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(said.status.success(), "{said:?}");
    let said = String::from_utf8(said.stdout).unwrap();
    for line in [
        "lint\ttree\t\n",
        "client-check\tpath\tapps/symphony-web\n",
        "size-check\tpackage\tsymphony-server\n",
        "nextest-setup\t\t\n",
        "tests/hooks\ttree\t\n",
        "tests/perf\ttest\tperf\n",
        "tests/browser\tpackage\tsymphony-server\n",
        "tests/browser\tpath\tapps/symphony-web\n",
    ] {
        assert!(said.contains(line), "{line:?} in:\n{said}");
    }
    assert!(
        said.lines().all(|line| line.split('\t').count() == 3),
        "{said}"
    );
    assert!(
        !said.contains("\ntests\t"),
        "the tests step reads by its lanes:\n{said}"
    );
    assert_eq!(
        fs::read_to_string(tree.join("build/verify/steps.tsv")).unwrap(),
        before
    );
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
    test_time::until_true(
        "the verify's lint started",
        test_time::backstop(std::time::Duration::from_secs(120)),
        || held.exists(),
    )
    .unwrap();
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
