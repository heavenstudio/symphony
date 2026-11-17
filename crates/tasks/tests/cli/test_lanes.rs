//! `tools/test-lanes.sh`, the tests step of `just verify`, copied into a workspace of the test's own
//! over a Cargo stand-in ([`CARGO`]) and a `getconf` one ([`GETCONF`]): the workspace's tests built
//! once, then run in lanes side by side, at most half the processors online at once, each lane's
//! log named before any starts and kept, each lane's output whole in the lanes' order, every red
//! lane's targets as one list at the end, a red lane that listed none named whole, and the build's
//! binaries named for the tests that would build their own.

use crate::children::Child as _;
use crate::scratch;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;
use symphony_board::sys::{Command, Stdio, output, wait_output};

/// The lanes a run in the stand-in workspace starts: all of them, as its targets fill each.
const LANES: usize = 8;

/// Cargo, standing in. `metadata` says the targets in the file `METADATA` names; `test --no-run`,
/// the build, says its arguments and exits with `FAIL_BUILD`'s status (0 unset); any other `test`
/// is a lane, which says its arguments, writes them to a file of its own in `$STAND/started` (and
/// one in `$STAND/running` while it runs), and waits until `LANES` lanes have started (one run
/// while too few others could would wait out its backstop and fail), then adds how many lanes run
/// to `$STAND/at-once` and says the binaries it was named. With `HOLD`, each lane makes the file
/// `$HOLD.PID` once all started, then waits to be ended by a signal, and says so if it never was: a
/// lane started with Ctrl-C ignored. A lane that runs a target `SAY` names as `TARGET:TEXT` writes
/// TEXT as the status of the lane named as that target (in the folder `SYMPHONY_TEST_LANE_LOGS`
/// names), where the run cannot write it again, and ends green. One `DIE` names as `TARGET:kill`
/// says a failed test, a list a test printed and a test it started, then is killed mid-line; as
/// `TARGET:STATUS`, it says a build's error and exits with that status. One `FAIL` names as
/// `TARGET:STATUS` says a failed test in it, a list a test printed, cargo's line and its own list,
/// and exits with that status.
const CARGO: &str = r#"#!/bin/bash
case "$1" in
metadata) cat "$METADATA"; exit 0 ;;
test) shift ;;
*) exit 64 ;;
esac
for arg in "$@"; do
    if [ "$arg" = --no-run ]; then
        printf 'built: %s\n' "$*"
        printf '%s\n' "$*" >"$STAND/built"
        exit "${FAIL_BUILD:-0}"
    fi
done
mkdir -p "$STAND/started" "$STAND/running"
: >"$STAND/running/$$"
trap 'rm -f "$STAND/running/$$"' EXIT
printf '%s\n' "$*" >"$STAND/started/$$"
printf '     Running lane: %s\n' "$*"
waited=0
until [ "$(ls "$STAND/started" | wc -l)" -ge "$LANES" ]; do
    sleep 0.05
    waited=$((waited + 1))
    if [ "$waited" -ge 2400 ]; then printf 'the other lanes never started\n'; exit 70; fi
done
ls "$STAND/running" | wc -l | tr -d ' ' >>"$STAND/at-once"
printf 'binaries=%s\ntasks=%s\n' "${SYMPHONY_TEST_BINARIES-unset}" "${SYMPHONY_TEST_TASKS_BINARY-unset}"
if [ -n "${HOLD:-}" ]; then
    : >"$HOLD.$$"
    sleep 600
    printf 'the signal never reached this lane\n'
    exit 0
fi
for pair in ${SAY:-}; do
    target="${pair%%:*}" said="${pair#*:}"
    case " $* " in
    *" --test $target "*)
        printf '%s' "$said" >"$SYMPHONY_TEST_LANE_LOGS/$target.status"
        chmod 444 "$SYMPHONY_TEST_LANE_LOGS/$target.status"
        ;;
    esac
done
for pair in ${DIE:-}; do
    target="${pair%%:*}" how="${pair#*:}"
    case " $* " in
    *" --test $target "*)
        if [ "$how" = kill ]; then
            printf 'test %s::dies ... FAILED\n' "$target"
            printf 'error: 1 target failed:\n    `-p a-test-printed --lib`\n'
            printf 'test %s::runs ... ' "$target"
            kill -KILL $$
        fi
        printf 'error: could not compile `p` (test "%s") due to 1 previous error\n' "$target"
        exit "$how"
        ;;
    esac
done
for pair in ${FAIL:-}; do
    target="${pair%%:*}" status="${pair#*:}"
    case " $* " in
    *" --test $target "*)
        printf 'test %s::fails ... FAILED\n' "$target"
        printf 'error: 2 targets failed:\n    `-p a-test-printed --lib`\n    `-p it-too --lib`\n'
        printf 'error: test failed, to rerun pass `-p p --test %s`\n' "$target"
        printf 'error: 1 target failed:\n    `-p p --test %s`\n' "$target"
        exit "$status"
        ;;
    esac
done
exit 0
"#;

/// `getconf`, standing in: `_NPROCESSORS_ONLN` is `PROCESSORS`, and fails when that is empty.
const GETCONF: &str = r#"#!/bin/sh
if [ "$1" != _NPROCESSORS_ONLN ] || [ -z "${PROCESSORS:-}" ]; then exit 1; fi
printf '%s\n' "$PROCESSORS"
"#;

/// The stand-in workspace's targets as `cargo metadata --no-deps` says them: tests of the named
/// lanes and three no lane names, one of which cargo does not test, a library, a binary, a build
/// script, and examples and benches with `test` and without.
const METADATA: &str = r#"{"packages":[
 {"name":"board","targets":[
  {"kind":["lib"],"name":"board","test":true},
  {"kind":["bin"],"name":"tasks","test":true},
  {"kind":["test"],"name":"perf","test":true},
  {"kind":["test"],"name":"sweep","test":true},
  {"kind":["test"],"name":"cli","test":true},
  {"kind":["test"],"name":"server","test":true},
  {"kind":["test"],"name":"links","test":true},
  {"kind":["test"],"name":"untested","test":false},
  {"kind":["bench"],"name":"quiet","test":false},
  {"kind":["bench"],"name":"loud","test":true},
  {"kind":["custom-build"],"name":"build-script-build","test":false}]},
 {"name":"ui","targets":[
  {"kind":["test"],"name":"browser","test":true},
  {"kind":["test"],"name":"after_reload","test":true},
  {"kind":["test"],"name":"links","test":true},
  {"kind":["test"],"name":"wasm_build","test":true},
  {"kind":["example"],"name":"size","test":true},
  {"kind":["example"],"name":"draw","test":false}]}]}"#;

/// Each lane a run in the stand-in workspace starts, its name and its targets as cargo takes them,
/// in the lanes' order: the named lanes with the targets the workspace has, `rest` with every test
/// target none names.
const LISTED: [&str; LANES] = [
    "perf --test perf",
    "sweep --test sweep",
    "browser --test browser",
    "board --test cli --test server",
    "server --test after_reload",
    "units --lib --bins --bench loud --example size",
    "rest --test links --test wasm_build",
    "doc --doc",
];

/// What the build and every lane hand cargo before their own targets.
const RUN: &str = "--config profile.dev.build-override.debug=\"line-tables-only\" --workspace \
                   --locked --no-fail-fast";

/// The processors the stand-in `getconf` says are online unless a test says otherwise: as many
/// lanes at once as there are.
const PROCESSORS: &str = "16";

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A workspace of the test's own holding this workspace's `tools/test-lanes.sh` and the scripts it
/// runs, and the folder (`stand`) of its Cargo and `getconf` stand-ins, where Cargo writes what it
/// was asked.
struct Lanes {
    root: PathBuf,
    stand: PathBuf,
}

impl Lanes {
    fn new(name: &str) -> Lanes {
        let dir = scratch(name);
        let root = dir.join("workspace");
        fs::create_dir_all(root.join("tools")).unwrap();
        for tool in ["cargo.sh", "record.sh", "test-lanes.sh"] {
            let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools");
            fs::copy(from.join(tool), root.join("tools").join(tool)).unwrap();
        }
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        let stand = dir.join("stand");
        executable(&stand.join("cargo"), CARGO);
        executable(&stand.join("bin/getconf"), GETCONF);
        fs::write(stand.join("metadata.json"), METADATA).unwrap();
        Lanes {
            root: root.canonicalize().unwrap(),
            stand: stand.canonicalize().unwrap(),
        }
    }

    /// `tools/test-lanes.sh ARGS` with `vars`, recording nothing, run from elsewhere.
    fn command(&self, args: &[&str], vars: &[(&str, &str)]) -> Command {
        let mut command = Command::of(self.root.join("tools/test-lanes.sh"));
        let path = std::env::var("PATH").unwrap_or_default();
        command
            .args(args)
            .current_dir(&self.stand)
            .env(
                "PATH",
                format!("{}:{path}", self.stand.join("bin").display()),
            )
            .env("PROCESSORS", PROCESSORS)
            .env("CARGO", self.stand.join("cargo"))
            .env("METADATA", self.stand.join("metadata.json"))
            .env("STAND", &self.stand)
            .env("LANES", LANES.to_string())
            .env("TASKS_RECORDER", "off")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("SYMPHONY_TEST_BINARIES")
            .env_remove("SYMPHONY_TEST_TASKS_BINARY")
            .env_remove("SYMPHONY_TEST_LANE_LOGS")
            .env_remove("FAIL")
            .env_remove("FAIL_BUILD")
            .env_remove("DIE")
            .env_remove("SAY")
            .env_remove("HOLD")
            .envs(vars.iter().copied())
            .stdin(Stdio::null());
        command
    }

    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(&mut self.command(args, vars)).unwrap()
    }

    /// A run with `vars` in a process group of its own, `signal` sent to the whole group once every
    /// lane has started and holds: what it did.
    fn stopped(&self, signal: libc::c_int, vars: &[(&str, &str)]) -> Output {
        let hold = self.stand.join("held");
        let mut vars = vars.to_vec();
        vars.push(("HOLD", hold.to_str().unwrap()));
        let child = self
            .command(&[], &vars)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let held = || {
            (fs::read_dir(&self.stand).unwrap())
                .filter(|entry| {
                    (entry.as_ref().unwrap().file_name().to_string_lossy()).starts_with("held.")
                })
                .count()
        };
        test_time::until_true(
            "every lane held",
            test_time::backstop(Duration::from_secs(120)),
            || held() == LANES,
        )
        .unwrap();
        let group = i32::try_from(child.id()).unwrap();
        // SAFETY: kill reads nothing of ours; a pid below 0 names the group the child leads.
        assert_eq!(unsafe { libc::kill(-group, signal) }, 0);
        wait_output(child).unwrap()
    }

    /// The arguments of each lane that started, sorted.
    fn started(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.stand.join("started")) else {
            return Vec::new();
        };
        let mut started: Vec<String> = entries
            .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
            .map(|said| said.trim_end().to_string())
            .collect();
        started.sort();
        started
    }

    /// The most lanes that ran at once, as each counted them once every lane it waited for had
    /// started.
    fn most_at_once(&self) -> usize {
        let counted = fs::read_to_string(self.stand.join("at-once")).unwrap();
        (counted.lines())
            .map(|count| count.parse::<usize>().unwrap())
            .max()
            .unwrap()
    }

    /// The folder the lanes' logs go to when the run is told none.
    fn logs(&self) -> PathBuf {
        self.root.join("build/test-lanes")
    }
}

/// The name of the lane [`LISTED`] says as `lane`.
fn name(lane: &str) -> &str {
    lane.split_once(' ').unwrap().0
}

/// The targets of the lane [`LISTED`] says as `lane`.
fn targets(lane: &str) -> &str {
    lane.split_once(' ').unwrap().1
}

/// What a run says before any lane starts, with [`PROCESSORS`] online and the lanes' logs in
/// `logs`: how many lanes run at once and each lane's log.
fn named(logs: &Path) -> String {
    let mut said = String::from(
        "test-lanes: lanes run at most 8 at once (16 processors online), each writing its log as \
         it runs:\n",
    );
    for lane in LISTED {
        said.push_str(&format!(
            "test-lanes:     {}/{}.log ({})\n",
            logs.display(),
            name(lane),
            targets(lane)
        ));
    }
    said
}

#[test]
fn the_lanes_take_the_targets_the_workspace_has_and_the_rest_every_test_target_none_names() {
    let lanes = Lanes::new("test-lanes-list");
    let listed = lanes.run(&["--list"], &[]);
    assert!(listed.status.success(), "{listed:?}");
    let said = String::from_utf8(listed.stdout).unwrap();
    assert_eq!(said.lines().collect::<Vec<_>>(), LISTED);
    // It lists them, and builds, runs and writes nothing.
    assert!(!lanes.stand.join("built").exists());
    assert!(lanes.started().is_empty());
    assert!(!lanes.root.join("build").exists());
    // A lane none of whose targets the workspace has is not started.
    fs::write(
        lanes.stand.join("metadata.json"),
        r#"{"packages":[{"name":"p","targets":[{"kind":["test"],"name":"links","test":true}]}]}"#,
    )
    .unwrap();
    let listed = lanes.run(&["--list"], &[]);
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        "units --lib --bins\nrest --test links\ndoc --doc\n"
    );
    // An argument it does not know runs nothing.
    for args in [&["--lanes"][..], &["--list", "--list"]] {
        let unknown = lanes.run(args, &[]);
        assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
        assert_eq!(
            String::from_utf8_lossy(&unknown.stderr),
            "usage: tools/test-lanes.sh [--list]\n"
        );
    }
}

#[test]
fn the_tests_are_built_once_then_every_lane_runs_beside_the_others_and_says_its_output_whole() {
    let lanes = Lanes::new("test-lanes-green");
    // An earlier run's logs and statuses, which this one replaces.
    fs::create_dir_all(lanes.logs()).unwrap();
    for file in ["perf.log", "perf.status", "doc.log", "doc.status"] {
        fs::write(lanes.logs().join(file), "an earlier run's\n").unwrap();
    }
    let done = lanes.run(&[], &[]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        fs::read_to_string(lanes.stand.join("built")).unwrap(),
        format!("{RUN} --no-run\n")
    );
    // Every lane started before any ended: none would have ended, waiting for the others.
    let mut asked: Vec<String> = (LISTED.iter())
        .map(|lane| format!("{RUN} {}", targets(lane)))
        .collect();
    asked.sort();
    assert_eq!(lanes.started(), asked);
    assert_eq!(lanes.most_at_once(), LANES);
    // After the build's output, each lane's log is named, then each lane's output follows the one
    // before it whole, in the lanes' order; the build folder holds no `tasks`, so none is named.
    let debug = lanes.root.join(".target/debug");
    let mut expected = format!("built: {RUN} --no-run\n{}", named(&lanes.logs()));
    for lane in LISTED {
        let output = format!(
            "     Running lane: {RUN} {}\nbinaries={}\ntasks=unset\n",
            targets(lane),
            debug.display()
        );
        // Its log keeps it, and no status is left beside it.
        let log = lanes.logs().join(format!("{}.log", name(lane)));
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            output,
            "{}",
            log.display()
        );
        expected.push_str(&output);
    }
    assert_eq!(String::from_utf8(done.stdout).unwrap(), expected);
    assert_eq!(String::from_utf8(done.stderr).unwrap(), "");
    let mut kept: Vec<String> = (fs::read_dir(lanes.logs()).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    kept.sort();
    let mut logs: Vec<String> = LISTED.map(|lane| format!("{}.log", name(lane))).to_vec();
    logs.sort();
    assert_eq!(kept, logs);
}

#[test]
fn the_lanes_are_named_the_builds_binaries_and_a_callers_names_win() {
    let lanes = Lanes::new("test-lanes-binaries");
    let debug = lanes.root.join("elsewhere/debug");
    fs::create_dir_all(&debug).unwrap();
    fs::write(debug.join("tasks"), "the build's").unwrap();
    let done = lanes.run(&[], &[("CARGO_TARGET_DIR", "elsewhere")]);
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    let named = format!(
        "binaries={}\ntasks={}\n",
        debug.display(),
        debug.join("tasks").display()
    );
    assert_eq!(said.matches(&named).count(), LANES, "{said}");
    let lanes = Lanes::new("test-lanes-binaries-named");
    let done = lanes.run(
        &[],
        &[
            ("SYMPHONY_TEST_BINARIES", "/the/callers"),
            ("SYMPHONY_TEST_TASKS_BINARY", "/the/callers/tasks"),
        ],
    );
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    let named = "binaries=/the/callers\ntasks=/the/callers/tasks\n";
    assert_eq!(said.matches(named).count(), LANES, "{said}");
}

#[test]
fn at_most_half_the_processors_online_run_lanes_at_once_and_one_when_the_system_says_no_number() {
    for (processors, at_once, why) in [
        ("4", 2, "4 processors online"),
        ("5", 2, "5 processors online"),
        ("3", 1, "3 processors online"),
        ("1", 1, "1 processors online"),
        ("", 1, "no number of processors online"),
        ("many", 1, "no number of processors online"),
    ] {
        let lanes = Lanes::new("test-lanes-at-once");
        // Each lane waits for as many as may run at once: fewer would wait out their backstop.
        let count = at_once.to_string();
        let done = lanes.run(&[], &[("PROCESSORS", processors), ("LANES", &count)]);
        assert!(done.status.success(), "{processors}: {done:?}");
        let said = String::from_utf8(done.stdout).unwrap();
        assert!(
            said.contains(&format!(
                "\ntest-lanes: lanes run at most {at_once} at once ({why}), each writing its log \
                 as it runs:\n"
            )),
            "{processors}: {said}"
        );
        assert_eq!(lanes.started().len(), LANES, "{processors}: every lane ran");
        assert_eq!(lanes.most_at_once(), at_once, "{processors}");
    }
}

#[test]
fn a_run_exits_as_its_first_red_lane_and_ends_with_every_red_lanes_own_list() {
    let lanes = Lanes::new("test-lanes-red");
    let done = lanes.run(&[], &[("FAIL", "wasm_build:101 sweep:101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    // A red lane's lines are in its own output, before the next lane's.
    let at = |line: &str| {
        said.find(line)
            .unwrap_or_else(|| panic!("no {line:?} in {said}"))
    };
    assert!(
        at(&format!("     Running lane: {RUN} --test sweep\n"))
            < at("error: test failed, to rerun pass `-p p --test sweep`\n")
            && at("error: test failed, to rerun pass `-p p --test sweep`\n")
                < at(&format!("     Running lane: {RUN} --test browser\n")),
        "{said}"
    );
    // The last list is each red lane's own, the one ending its log, in the lanes' order (the
    // sweep's before the rest's, whichever ended first), and never one a test printed.
    assert!(
        said.ends_with(
            "error: 2 targets failed:\n    `-p p --test sweep`\n    `-p p --test wasm_build`\n"
        ),
        "{said}"
    );
    let lanes = Lanes::new("test-lanes-red-one");
    let done = lanes.run(&[], &[("FAIL", "perf:101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(
        said.ends_with(&format!(
            "     Running lane: {RUN} --doc\nbinaries={}\ntasks=unset\n\
             error: 1 target failed:\n    `-p p --test perf`\n",
            lanes.root.join(".target/debug").display()
        )),
        "{said}"
    );
}

#[test]
fn a_red_lane_that_listed_no_target_of_its_own_is_named_whole_and_in_the_last_list() {
    let lanes = Lanes::new("test-lanes-unlisted");
    // The sweep's lane is killed after a test printed a list, mid-line, the browser's cargo fails
    // before any target, and the rest fails in a target, listing it.
    let done = lanes.run(
        &[],
        &[
            ("DIE", "sweep:kill browser:101"),
            ("FAIL", "wasm_build:101"),
        ],
    );
    assert_eq!(done.status.code(), Some(137), "the sweep's: {done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    let debug = lanes.root.join(".target/debug");
    // Each is named by a line of its own after its output, which a red's reader takes for a target
    // that failed: the lane's, which reruns it whole.
    let sweep = format!(
        "     Running lane: {RUN} --test sweep\nbinaries={}\ntasks=unset\n\
         test sweep::dies ... FAILED\nerror: 1 target failed:\n    `-p a-test-printed --lib`\n\
         test sweep::runs ... \nerror: the lane sweep is red (exit status 137) and listed no target that failed, to rerun \
         pass `--workspace --test sweep`\n     Running lane: {RUN} --test browser\n",
        debug.display()
    );
    assert!(said.contains(&sweep), "{said}");
    let browser = format!(
        "     Running lane: {RUN} --test browser\nbinaries={}\ntasks=unset\n\
         error: could not compile `p` (test \"browser\") due to 1 previous error\n\
         error: the lane browser is red (exit status 101) and listed no target that failed, to \
         rerun pass `--workspace --test browser`\n     Running lane: {RUN} --test cli",
        debug.display()
    );
    assert!(said.contains(&browser), "{said}");
    assert!(
        said.ends_with(
            "error: 3 targets failed:\n    `--workspace --test sweep`\n    `--workspace --test \
             browser`\n    `-p p --test wasm_build`\n"
        ),
        "{said}"
    );
}

#[test]
fn a_lane_that_says_no_exit_status_or_one_that_is_none_is_red_and_named() {
    for (status, line) in [
        (
            "x",
            "test-lanes: the lane browser said its exit status as 'x', which is none",
        ),
        (
            "",
            "test-lanes: the lane browser said its exit status as '', which is none",
        ),
        (
            "00",
            "test-lanes: the lane browser said its exit status as '00', which is none",
        ),
    ] {
        let lanes = Lanes::new("test-lanes-said-none");
        let logs = lanes.root.join("logs");
        let say = format!("browser:{status}");
        let done = lanes.run(
            &[],
            &[
                ("SAY", &say),
                ("SYMPHONY_TEST_LANE_LOGS", logs.to_str().unwrap()),
            ],
        );
        assert_eq!(done.status.code(), Some(1), "{status:?}: {done:?}");
        let said = String::from_utf8(done.stdout).unwrap();
        // Named after its own output, before the next lane's.
        assert!(
            said.contains(&format!(
                "\n{line}\nerror: the lane browser is red (no exit status it could read) and \
                 listed no target that failed, to rerun pass `--workspace --test browser`\n     \
                 Running lane: {RUN} --test cli --test server\n"
            )),
            "{status:?}: {said}"
        );
        assert!(
            said.ends_with("error: 1 target failed:\n    `--workspace --test browser`\n"),
            "{status:?}: {said}"
        );
    }
}

#[test]
fn a_lane_that_ends_saying_no_status_is_red_and_named() {
    let lanes = Lanes::new("test-lanes-unsaid");
    // The browser's lane is killed, the shell that runs it, before it could write how it ended.
    let anchor = "for pair in ${FAIL:-}; do";
    let stand_in = CARGO.replace(
        anchor,
        &format!("case \" $* \" in *\" --test browser \"*) kill -KILL $PPID ;; esac\n{anchor}"),
    );
    assert_ne!(stand_in, CARGO);
    executable(&lanes.stand.join("cargo"), &stand_in);
    let done = lanes.run(&[], &[]);
    assert_eq!(done.status.code(), Some(1), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(
        said.contains(
            "\ntest-lanes: the lane browser ended before it could say how\nerror: the lane \
             browser is red (no exit status it could read) and listed no target that failed, to \
             rerun pass `--workspace --test browser`\n"
        ),
        "{said}"
    );
    assert!(
        said.ends_with("error: 1 target failed:\n    `--workspace --test browser`\n"),
        "{said}"
    );
    // The slot that ran it went on: every lane ran.
    assert_eq!(lanes.started().len(), LANES);
}

#[test]
fn a_build_that_fails_runs_no_lane_and_exits_as_it_did() {
    let lanes = Lanes::new("test-lanes-unbuilt");
    let done = lanes.run(&[], &[("FAIL_BUILD", "101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!("built: {RUN} --no-run\n")
    );
    assert!(lanes.started().is_empty());
}

#[test]
fn an_interrupt_ends_the_lanes_and_the_run_says_what_they_wrote_and_exits_130() {
    let lanes = Lanes::new("test-lanes-interrupt");
    // Ctrl-C: the interrupt a terminal sends the whole foreground group.
    let done = lanes.stopped(libc::SIGINT, &[]);
    assert_eq!(done.status.code(), Some(130), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    for lane in LISTED {
        let running = format!("     Running lane: {RUN} {}\n", targets(lane));
        assert_eq!(said.matches(&running).count(), 1, "{said}");
    }
    // The interrupt reached every lane, which ended on it, and none is named red for it.
    assert!(!said.contains("the signal never reached"), "{said}");
    assert!(!said.contains("is red"), "{said}");
    assert!(!said.contains("failed:"), "{said}");
    assert!(
        said.ends_with(
            "test-lanes: interrupted (exit 130): a lane it cut short ended there, its log as far \
             as it got\n"
        ),
        "{said}"
    );
}
