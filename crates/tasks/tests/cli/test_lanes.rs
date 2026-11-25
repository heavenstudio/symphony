//! The release test runner over local Cargo/nextest stand-ins: one build, explicit shared-fixture
//! exceptions, the client build before concurrent readers, no retries, pinned setup, and live logs.

use crate::children::Child as _;
use crate::{git, scratch};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;
use symphony_board::sys::{Command, Stdio, group_alive, output, wait_output};

const LANES: usize = 9;
const VERSION: &str = "0.9.152";
pub(super) const MOCK_TOOL: &str =
    "#!/bin/sh\nprintf 'cargo-nextest 0.9.152 (fixture 2026-11-13)\\nrelease: 0.9.152\\n'\n";

/// Real launcher and SHA-256 checks over a local archive. Only the pin's fixture bytes and fetch
/// transport change, so corrupt archives and executable copies can be tested without a network.
pub(super) fn mock_nextest(root: &Path) {
    let fixture = root.join(".target/nextest-fixture");
    executable(&fixture.join("cargo-nextest"), MOCK_TOOL);
    let archive = fixture.join("tool.tar.gz");
    let done = output(
        Command::of("tar")
            .args(["-czf", archive.to_str().unwrap(), "-C"])
            .arg(&fixture)
            .arg("cargo-nextest"),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let path = root.join("tools/nextest.sh");
    let script = fs::read_to_string(&path).unwrap();
    let guard = "if [ \"${BASH_SOURCE[0]}\" = \"$0\" ]; then main \"$@\"; fi";
    assert!(script.contains(guard));
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    fs::write(
        path,
        script.replace(
            guard,
            &format!(
                "binary_sha256() {{ echo {}; }}\narchive_sha256() {{ echo {}; }}\nmain \"$@\"",
                digest(MOCK_TOOL.as_bytes()),
                digest(&fs::read(&archive).unwrap())
            ),
        ),
    )
    .unwrap();
    executable(
        &root.join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest")),
        MOCK_TOOL,
    );
    executable(
        &fixture.join("bin/curl"),
        "#!/bin/sh\nif [ -n \"${FAIL_NEXTEST_SETUP:-}\" ]; then exit 7; fi\nwhile [ $# -gt 0 ]; do\nif [ \"$1\" = -o ]; then cp \"$NEXT_TOOL_ARCHIVE\" \"$2\"; exit; fi\nshift\ndone\nexit 64\n",
    );
}
const CARGO: &str = r#"#!/bin/bash
if [ "$1" = nextest ] && [ "$2" = list ]; then
    printf '%s\n' "$*" >>"$STAND/built"
    if [ -n "${FAIL_BUILD:-}" ]; then exit "$FAIL_BUILD"; fi
    mkdir -p "$CARGO_TARGET_DIR/debug" || exit 2
    cp "$STAND/stall-reporter" "$CARGO_TARGET_DIR/debug/symphony-build" || exit 2
    cat "$METADATA"
    exit 0
fi
if [ "$1" = nextest ]; then
    name=nextest
    printf '<%s>\n' "$@" >"$STAND/nextest-args"
else
    # As Cargo does at every call: each binary's copy (BUILD=COPY) made again, unless it is its build.
    for pair in ${UPLIFT-}; do
        if [ "${pair%%=*}" -ef "${pair#*=}" ]; then
            printf 'kept %s\n' "${pair#*=}"
        else
            rm -f "${pair#*=}" && cp "${pair%%=*}" "${pair#*=}" && printf 'copied %s\n' "${pair#*=}"
        fi >>"$STAND/uplifts"
    done
    case " $* " in
        *' --ignored --exact release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test '*)
            printf '%s\n' "$*" >"$STAND/runner-fixture-args"
            printf 'explicit runner fixture\n'
            exit "${FAIL_LIVE_FIXTURE:-0}" ;;
        *' --test build '*)
            : >"$STAND/serial-done"
            printf 'serial build test\n'
            exit "${FAIL_SERIAL:-0}" ;;
        *' --test perf '*) name=perf ;;
        *' --test sweep '*) name=sweep ;;
        *' --test cli '*) name=cli ;;
        *' --test server '*) name=server ;;
        *' --test store '*) name=store ;;
        *' --test launch '*) name=launch ;;
        *' --lib '*) name=libraries ;;
        *' --doc '*) name=doc ;;
        *) exit 64 ;;
    esac
fi
printf 'running %s\nbinaries=%s\ntasks=%s\nclient=%s\nsteps=%s\n' "$name" "${SYMPHONY_TEST_BINARIES-unset}" "${SYMPHONY_TEST_TASKS_BINARY-unset}" "${SYMPHONY_UI_DIST-unset}" "${SYMPHONY_CHECK_STEPS-unset}"
printf 'hc=%s\n' "${HC_BIN-unset}"
mkdir -p "$STAND/started"
printf '%s\n' "$*" >"$STAND/started/$name"
if [ -n "${EXPECT_SERIAL:-}" ] && [ ! -f "$STAND/serial-done" ]; then exit 65; fi
if [ -n "${HOLD:-}" ]; then
    exec 3<>"$HOLD"
    : >"$HOLD.$$"
    read -r held <&3
    printf 'the signal never reached this batch\n'
    exit 0
fi
if [ "${DIE:-}" = "$name" ]; then kill -KILL $$; fi
if [ "${UNSAID:-}" = "$name" ]; then kill -KILL $PPID; fi
if [ "${SAY:-}" = "$name" ]; then
    printf %s "${SAY_STATUS-x}" >"$SYMPHONY_TEST_LANE_LOGS/$name.status"
    chmod 444 "$SYMPHONY_TEST_LANE_LOGS/$name.status"
fi
if [ "${FAIL:-}" = "$name" ]; then
    if [ "$name" = nextest ]; then
        cat "$STAND/nextest-red.log"
        exit 100
    fi
    printf 'test case::fails ... FAILED\n'
    printf 'error: test failed, to rerun pass `-p p --test %s`\n' "$name"
    printf 'error: 1 target failed:\n    `-p p --test %s`\n' "$name"
    exit 101
fi
exit 0
"#;

const METADATA: &str = r#"{"rust-build-meta":{"platforms":{"host":{"platform":{"triple":"fixture-host"}}}},"rust-binaries":{
 "symphony-board":{"binary-name":"symphony_board","kind":"lib","binary-path":"/build/deps/board-exact"},
 "stateless":{"binary-name":"stateless","kind":"lib","binary-path":"/build/deps/stateless-exact"},
 "symphony-build":{"binary-name":"symphony_build","kind":"lib","binary-path":"/build/deps/build-exact"},
 "perf":{"binary-name":"perf","kind":"test"},
 "sweep":{"binary-name":"sweep","kind":"test"},
 "cli":{"binary-name":"cli","kind":"test"},
 "server":{"binary-name":"server","kind":"test"},
 "store":{"binary-name":"store","kind":"test"},
 "launch":{"binary-name":"launch","kind":"test"},
 "build":{"binary-name":"build","kind":"test"},
 "new-target":{"binary-name":"new-target","kind":"test"}}}"#;

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Lanes {
    root: PathBuf,
    stand: PathBuf,
}
impl Lanes {
    fn new(name: &str) -> Self {
        let dir = scratch(name);
        let root = dir.join("workspace");
        fs::create_dir_all(root.join("tools")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools");
        for name in ["cargo.sh", "record.sh", "nextest.sh", "test-lanes.sh"] {
            fs::copy(source.join(name), root.join("tools").join(name)).unwrap();
        }
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        git(&root, &["init", "-q"]);
        mock_nextest(&root);
        let stand = dir.join("stand");
        executable(&stand.join("cargo"), CARGO);
        executable(
            &stand.join("stall-reporter"),
            "#!/bin/sh\nprintf '<%s>\\n' \"$@\" >\"$STAND/reporter-args\"\nif [ -n \"${FAIL_REPORTER:-}\" ]; then cat >/dev/null; echo 'test-stalls: error: fixture reporter failed' >&2; exit 2; fi\ncat\n",
        );
        executable(
            &stand.join("bin/getconf"),
            "#!/bin/sh\nprintf '%s\\n' \"${PROCESSORS-32}\"\n",
        );
        executable(
            &root.join("apps/symphony-web/build.sh"),
            "#!/bin/sh\n: > \"$STAND/client-built\"\nmkdir -p \"$1/public\"\n",
        );
        fs::write(stand.join("metadata.json"), METADATA).unwrap();
        fs::copy(
            source.join("../crates/symphony-build/src/release/fixtures/tests-nextest-red.log"),
            stand.join("nextest-red.log"),
        )
        .unwrap();
        Self {
            root: root.canonicalize().unwrap(),
            stand: stand.canonicalize().unwrap(),
        }
    }
    fn command(&self, args: &[&str], vars: &[(&str, &str)]) -> Command {
        let mut command = Command::of(self.root.join("tools/test-lanes.sh"));
        command
            .args(args)
            .current_dir(&self.stand)
            .env(
                "PATH",
                format!(
                    "{}:{}:{}",
                    self.stand.join("bin").display(),
                    self.root.join(".target/nextest-fixture/bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("CARGO", self.stand.join("cargo"))
            .env("METADATA", self.stand.join("metadata.json"))
            .env("STAND", &self.stand)
            .env(
                "NEXT_TOOL_ARCHIVE",
                self.root.join(".target/nextest-fixture/tool.tar.gz"),
            )
            .env("TASKS_RECORDER", "off")
            .env("PROCESSORS", "32")
            .env("EXPECT_SERIAL", "1")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("SYMPHONY_UI_DIST")
            .env_remove("SYMPHONY_TEST_BINARIES")
            .env_remove("SYMPHONY_TEST_TASKS_BINARY")
            .env_remove("SYMPHONY_TEST_LANE_LOGS")
            .env_remove("HC_BIN")
            .env_remove("FAIL_SERIAL")
            .env_remove("UNSAID")
            .env_remove("FAIL")
            .env_remove("DIE")
            .env_remove("SAY")
            .env_remove("SAY_STATUS")
            .env_remove("FAIL_BUILD")
            .env_remove("FAIL_REPORTER")
            .env_remove("FAIL_LIVE_FIXTURE")
            .env_remove("HOLD")
            .env_remove("UPLIFT")
            .envs(vars.iter().copied())
            .stdin(Stdio::null());
        command
    }
    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(&mut self.command(args, vars)).unwrap()
    }
    fn tool(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(
            Command::of(self.root.join("tools/nextest.sh"))
                .args(args)
                .current_dir(&self.stand)
                .env(
                    "PATH",
                    format!(
                        "{}:{}:{}",
                        self.stand.join("bin").display(),
                        self.root.join(".target/nextest-fixture/bin").display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .env(
                    "NEXT_TOOL_ARCHIVE",
                    self.root.join(".target/nextest-fixture/tool.tar.gz"),
                )
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("FAIL_NEXTEST_SETUP")
                .envs(vars.iter().copied())
                .stdin(Stdio::null()),
        )
        .unwrap()
    }
    fn logs(&self) -> PathBuf {
        self.root.join("build/test-lanes")
    }
    /// A run with `vars` in a process group of its own, `signal` sent to the whole group once every
    /// lane has started and holds: what it did.
    fn stopped(&self, signal: libc::c_int, vars: &[(&str, &str)]) -> Output {
        let hold = self.stand.join("held");
        assert!(
            output(Command::of("mkfifo").arg(&hold))
                .unwrap()
                .status
                .success()
        );
        let mut vars = vars.to_vec();
        vars.push(("HOLD", hold.to_str().unwrap()));
        let logs = vars
            .iter()
            .find(|(name, _)| *name == "SYMPHONY_TEST_LANE_LOGS")
            .map_or_else(|| self.logs(), |(_, path)| PathBuf::from(path));
        let held = || {
            (fs::read_dir(&self.stand).unwrap())
                .filter(|entry| {
                    (entry.as_ref().unwrap().file_name().to_string_lossy()).starts_with("held.")
                })
                .count()
        };
        self.signalled(
            signal,
            &vars,
            "every lane held and its streamed output reached the live log",
            || {
                held() == LANES
                    && fs::read_to_string(logs.join("nextest.log"))
                        .is_ok_and(|text| text.starts_with("running "))
            },
        )
    }
    /// A run with `vars` in a process group of its own, `signal` sent to the whole group once
    /// `ready` (`what`) holds: what it did.
    fn signalled(
        &self,
        signal: libc::c_int,
        vars: &[(&str, &str)],
        what: &str,
        ready: impl FnMut() -> bool,
    ) -> Output {
        let mut child = self
            .command(&[], vars)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        test_time::until_true(what, test_time::backstop(Duration::from_secs(120)), ready).unwrap();
        let group = i32::try_from(child.id()).unwrap();
        // SAFETY: kill reads nothing of ours; a pid below 0 names the group the child leads.
        assert_eq!(unsafe { libc::kill(-group, signal) }, 0);
        // Reap the leader while waiting, so its zombie cannot keep the group alive. A leader's
        // exit alone says nothing about its lanes: no member may still write a log we read next.
        test_time::until_true(
            format!("lane process group ended after signal {signal}"),
            test_time::backstop(Duration::ZERO),
            || child.try_wait().unwrap().is_some() && !group_alive(child.id()),
        )
        .unwrap();
        wait_output(child).unwrap()
    }
}

#[test]
fn one_build_feeds_nextest_and_only_shared_fixtures_and_docs_run_on_cargo() {
    let lanes = Lanes::new("nextest-one-build");
    let done = lanes.run(&[], &[("RUST_TEST_THREADS", "3")]);
    assert!(done.status.success(), "{done:?}");
    let built = fs::read_to_string(lanes.stand.join("built")).unwrap();
    assert_eq!(built.lines().count(), 1, "{built}");
    assert!(built.contains("nextest list --config profile.dev.build-override.debug=\"line-tables-only\" --workspace --locked --list-type binaries-only --message-format json"), "{built}");
    let args = fs::read_to_string(lanes.stand.join("nextest-args")).unwrap();
    for flag in [
        "<--binaries-metadata>",
        "<--profile>\n<release>",
        "<--retries>\n<0>",
        "<--test-threads>\n<3>",
        "<--no-fail-fast>",
        "<libtest-json-plus>",
        "<0.1>",
    ] {
        assert!(args.contains(flag), "{flag}: {args}");
    }
    assert!(
        !args.contains("<--workspace>"),
        "reuse never builds: {args}"
    );
    assert!(args.contains("not (kind(test) & binary(=build))"), "{args}");
    assert!(args.contains("binary(=build)"), "{args}");
    assert!(
        !args.contains("new-target"),
        "new targets are included automatically: {args}"
    );
    for name in [
        "perf",
        "sweep",
        "cli",
        "server",
        "store",
        "launch",
        "libraries",
        "doc",
        "nextest",
    ] {
        assert!(lanes.stand.join("started").join(name).is_file(), "{name}");
        assert!(lanes.logs().join(format!("{name}.log")).is_file(), "{name}");
        assert!(
            !lanes.logs().join(format!("{name}.status")).exists(),
            "{name}"
        );
    }
    let selection: serde_json::Value =
        serde_json::from_slice(&fs::read(lanes.logs().join("shared-libraries.json")).unwrap())
            .unwrap();
    assert_eq!(
        selection,
        serde_json::json!([
            {"binary":"/build/deps/board-exact", "shared":true},
            {"binary":"/build/deps/stateless-exact", "shared":false},
            {"binary":"/build/deps/build-exact", "shared":false}
        ])
    );
    let libraries = fs::read_to_string(lanes.stand.join("started/libraries")).unwrap();
    assert!(
        libraries.contains("target.fixture-host.runner="),
        "{libraries}"
    );
    assert!(libraries.contains("exec-test-binary"), "{libraries}");
    assert!(
        !args.contains("exec-test-binary"),
        "the adapter is scoped to Cargo libraries: {args}"
    );
    let docs = fs::read_to_string(lanes.stand.join("started/doc")).unwrap();
    assert!(!docs.contains("exec-test-binary"), "{docs}");
    let live = fs::read_to_string(lanes.stand.join("runner-fixture-args")).unwrap();
    assert!(live.contains("--workspace --locked --no-fail-fast --lib -- --ignored --exact release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test"), "{live}");
    assert!(!live.contains("exec-test-binary"), "{live}");
    assert!(args.contains("not (kind(lib) & package(=symphony-build) & test(=release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test))"), "{args}");
    assert!(lanes.stand.join("client-built").exists());
    let shared = fs::read_to_string(lanes.stand.join("started/cli")).unwrap();
    assert!(shared.contains("--test cli"), "{shared}");
    let log = fs::read_to_string(lanes.logs().join("nextest.log")).unwrap();
    assert!(
        log.contains(&format!(
            "client={}\n",
            lanes.root.join("build/symphony-ui/public").display()
        )),
        "{log}"
    );
}

#[test]
fn absent_or_wrong_pinned_runner_refuses_before_a_build_and_names_setup() {
    let lanes = Lanes::new("nextest-pin");
    let binary = lanes
        .root
        .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest"));
    fs::remove_file(&binary).unwrap();
    for wrong in [false, true] {
        if wrong {
            executable(&binary, "#!/bin/sh\necho cargo-nextest 0.9.1\n");
        }
        let done = lanes.run(&[], &[]);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
        assert_eq!(
            String::from_utf8(done.stderr).unwrap(),
            format!(
                "nextest: cargo-nextest {VERSION} is missing or differs; run just nextest-setup\n"
            )
        );
        assert!(!lanes.stand.join("built").exists());
    }
}

#[test]
fn a_callers_client_and_binary_paths_are_kept_and_no_client_build_runs() {
    let lanes = Lanes::new("nextest-inputs");
    let done = lanes.run(
        &[],
        &[
            ("SYMPHONY_UI_DIST", "/supplied/client"),
            ("SYMPHONY_TEST_BINARIES", "/supplied/bin"),
            ("SYMPHONY_TEST_TASKS_BINARY", "/supplied/tasks"),
        ],
    );
    assert!(done.status.success(), "{done:?}");
    assert!(!lanes.stand.join("client-built").exists());
    for name in [
        "nextest",
        "perf",
        "sweep",
        "cli",
        "server",
        "store",
        "launch",
        "libraries",
        "doc",
    ] {
        let log = fs::read_to_string(lanes.logs().join(format!("{name}.log"))).unwrap();
        assert!(
            log.contains(
                "binaries=/supplied/bin\ntasks=/supplied/tasks\nclient=/supplied/client\n"
            ),
            "{log}"
        );
    }
}

#[test]
fn listing_needs_no_install_or_build_and_bad_arguments_run_nothing() {
    let lanes = Lanes::new("nextest-list");
    fs::remove_file(
        lanes
            .root
            .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest")),
    )
    .unwrap();
    let done = lanes.run(&["--list"], &[]);
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(
        said.starts_with("nextest not (kind(test) & binary(=build))"),
        "{said}"
    );
    assert!(
        said.contains("cargo --test build (before scheduling)\n"),
        "{said}"
    );
    assert!(said.ends_with("doc --doc\n"), "{said}");
    for args in [
        &["--list", "--list"][..],
        &["--unknown"],
        &["doc", "--list"],
    ] {
        let done = lanes.run(args, &[]);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
        assert_eq!(
            done.stderr,
            b"usage: tools/test-lanes.sh [--list | --inputs | --alone LANES ARGS... | LANE...]\n"
        );
    }
    let done = lanes.run(&["doc", "unknown"], &[]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(
        String::from_utf8_lossy(&done.stderr)
            .starts_with("test-lanes: no lane unknown; the lanes are build fixture nextest perf "),
        "{done:?}"
    );
    assert!(!lanes.stand.join("built").exists());
}

/// The lanes a run listed in its `lanes.tsv`, each its name and exit status, its seconds a whole
/// number.
fn listed(lanes: &Lanes) -> Vec<(String, i32)> {
    let text = fs::read_to_string(lanes.logs().join("lanes.tsv")).unwrap();
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line:?}");
            assert!(fields[2].parse::<u64>().is_ok(), "{line:?}");
            (fields[0].to_string(), fields[1].parse().unwrap())
        })
        .collect()
}

#[test]
fn every_lane_that_ran_is_listed_with_how_it_ended_and_named_lanes_run_alone() {
    let lanes = Lanes::new("nextest-lanes-listed");
    let done = lanes.run(&[], &[("FAIL", "sweep")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let all = listed(&lanes);
    let names: Vec<&str> = all.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "build",
            "fixture",
            "nextest",
            "perf",
            "sweep",
            "cli",
            "server",
            "store",
            "launch",
            "libraries",
            "doc"
        ]
    );
    for (name, code) in &all {
        assert_eq!(*code, if name == "sweep" { 101 } else { 0 }, "{name}");
    }

    // Named lanes run alone, over the one build and the client; the rest neither run nor are
    // listed, so a list is of this run alone.
    fs::remove_dir_all(lanes.stand.join("started")).unwrap();
    fs::remove_file(lanes.stand.join("runner-fixture-args")).unwrap();
    let done = lanes.run(&["sweep", "doc"], &[("EXPECT_SERIAL", "")]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        listed(&lanes),
        [("sweep".to_string(), 0), ("doc".to_string(), 0)]
    );
    let mut started: Vec<String> = (fs::read_dir(lanes.stand.join("started")).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    symphony_board::order::sort_by(&mut started, &Ord::cmp);
    assert_eq!(started, ["doc", "sweep"]);
    assert!(!lanes.stand.join("runner-fixture-args").exists());
    assert_eq!(
        fs::read_to_string(lanes.stand.join("built"))
            .unwrap()
            .lines()
            .count(),
        2,
        "each run builds once"
    );

    // The lanes that run before the scheduler are named the same way.
    fs::remove_dir_all(lanes.stand.join("started")).unwrap();
    let done = lanes.run(&["build", "fixture"], &[("FAIL_LIVE_FIXTURE", "7")]);
    assert_eq!(done.status.code(), Some(7), "{done:?}");
    assert_eq!(
        listed(&lanes),
        [("build".to_string(), 0), ("fixture".to_string(), 7)]
    );
    assert!(
        !lanes.stand.join("started").exists(),
        "no scheduled lane ran"
    );
}

#[test]
fn what_each_lane_reads_is_said_without_an_install_or_a_build() {
    let lanes = Lanes::new("nextest-inputs");
    let done = lanes.run(&["--inputs"], &[]);
    assert!(done.status.success(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(
        said.lines().all(|line| line.split('\t').count() == 3),
        "{said}"
    );
    // Every lane a run can list says what it reads: one left out could never be carried.
    let named: std::collections::BTreeSet<&str> = said
        .lines()
        .map(|line| line.split('\t').next().unwrap())
        .collect();
    for lane in [
        "build",
        "fixture",
        "nextest",
        "perf",
        "sweep",
        "cli",
        "server",
        "store",
        "launch",
        "release",
        "after_commit",
        "after_publication",
        "after_worktrees",
        "hooks",
        "picks",
        "project",
        "after_http",
        "browser",
        "libraries",
        "doc",
    ] {
        assert!(named.contains(lane), "{lane} in:\n{said}");
    }
    for line in [
        "nextest\ttree\t\n",
        "hooks\ttree\t\n",
        "doc\ttree\t\n",
        "perf\ttest\tperf\n",
        "fixture\tpackage\tsymphony-build\n",
        "browser\ttest\tbrowser\n",
        "browser\tpackage\tsymphony-server\n",
        "browser\tpath\tapps/symphony-web\n",
        "libraries\tpackage\tsymphony-board\n",
    ] {
        assert!(said.contains(line), "{line:?} in:\n{said}");
    }
    assert!(!lanes.stand.join("built").exists());
}

#[test]
fn failed_build_starts_no_tests_and_nextest_red_stays_red_without_a_cargo_wide_rerun() {
    let lanes = Lanes::new("nextest-build-red");
    let done = lanes.run(&[], &[("FAIL_BUILD", "101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    assert!(!lanes.stand.join("started").exists());
    let lanes = Lanes::new("nextest-red");
    let done = lanes.run(&[], &[("FAIL", "nextest")]);
    assert_eq!(done.status.code(), Some(100), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(said.contains("time limit exceeded"), "{said}");
    assert!(!said.contains("to rerun pass `--workspace"), "{said}");
    assert_eq!(
        fs::read_dir(lanes.stand.join("started")).unwrap().count(),
        LANES
    );
}

#[test]
fn native_failures_keep_their_narrow_target_and_missing_status_is_red() {
    for (var, status, expected) in [
        ("FAIL", "", 101),
        ("DIE", "", 137),
        ("UNSAID", "", 1),
        ("SAY", "x", 1),
        ("SAY", "", 1),
        ("SAY", "00", 1),
    ] {
        let lanes = Lanes::new("nextest-native-red");
        let logs = lanes.logs();
        let done = lanes.run(
            &[],
            &[
                (var, "sweep"),
                ("SAY_STATUS", status),
                ("SYMPHONY_TEST_LANE_LOGS", logs.to_str().unwrap()),
            ],
        );
        assert_eq!(done.status.code(), Some(expected), "{var}: {done:?}");
        let said = String::from_utf8(done.stdout).unwrap();
        let target = if var == "FAIL" {
            "-p p --test sweep"
        } else {
            "--workspace --test sweep"
        };
        assert!(
            said.ends_with(&format!(
                "error: 1 target failed:\n    `{target}`\ntest-lanes: ended: every lane asked for ran\n"
            )),
            "{var}: {said}"
        );
    }
}

#[test]
fn a_serial_build_test_failure_does_not_hide_the_rest_of_the_workspace() {
    let lanes = Lanes::new("nextest-serial-red");
    let done = lanes.run(&[], &[("FAIL_SERIAL", "101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    assert_eq!(
        fs::read_dir(lanes.stand.join("started")).unwrap().count(),
        LANES
    );
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(
        said.ends_with(
            "error: 1 target failed:\n    `--workspace --test build`\n\
             test-lanes: ended: every lane asked for ran\n"
        ),
        "{said}"
    );
}

#[test]
fn signals_end_all_batches_and_leave_their_named_live_logs() {
    for (signal, code) in [
        (libc::SIGINT, 130),
        (libc::SIGTERM, 143),
        (libc::SIGHUP, 129),
        (libc::SIGKILL, 0),
    ] {
        let lanes = Lanes::new("nextest-signals");
        let logs = lanes.root.join("steps with spaces/tests-lanes");
        let done = lanes.stopped(
            signal,
            &[("SYMPHONY_TEST_LANE_LOGS", logs.to_str().unwrap())],
        );
        if signal == libc::SIGKILL {
            assert_eq!(done.status.signal(), Some(signal));
        } else {
            assert_eq!(done.status.code(), Some(code), "{done:?}");
        }
        let said = String::from_utf8(done.stdout).unwrap();
        assert!(!said.contains("the signal never reached"), "{said}");
        for name in [
            "nextest",
            "perf",
            "sweep",
            "cli",
            "server",
            "store",
            "launch",
            "libraries",
            "doc",
        ] {
            let log = logs.join(format!("{name}.log"));
            assert!(said.contains(&log.display().to_string()), "{said}");
            let text = fs::read_to_string(&log).unwrap();
            assert!(text.starts_with("running "), "{}: {text:?}", log.display());
        }
    }
}

#[test]
fn setup_fetches_one_verified_archive_without_a_cargo_install_and_reuses_it_offline() {
    let lanes = Lanes::new("nextest-setup");
    let binary = lanes
        .root
        .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest"));
    fs::remove_file(&binary).unwrap();
    executable(&lanes.stand.join("bin/cargo"), "#!/bin/sh\nexit 64\n");
    let done = lanes.tool(&["--setup"], &[]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(fs::read_to_string(&binary).unwrap(), MOCK_TOOL);
    let done = lanes.tool(&["--setup"], &[("FAIL_NEXTEST_SETUP", "1")]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        fs::read_dir(binary.parent().unwrap().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert!(!lanes.stand.join("built").exists());
}

#[test]
fn the_release_token_accepts_upstream_build_metadata_but_no_other_version() {
    let dir = scratch("nextest-version-output");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/nextest.sh");
    for (index, (version, valid)) in [
        ("cargo-nextest 0.9.152", true),
        (
            "cargo-nextest 0.9.152 (26ebeda79 2026-11-13)\nrelease: 0.9.152",
            true,
        ),
        ("cargo-nextest 0.9.1521", false),
        ("cargo-nextest 0.9.152-b.1", false),
        ("cargo-nextest 0.9.151", false),
        ("another-tool 0.9.152", false),
    ]
    .into_iter()
    .enumerate()
    {
        let probe = dir.join(format!("probe-{index}"));
        executable(&probe, &format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"));
        // Only the hash is mocked here, to isolate the version parser. Other cases exercise real
        // SHA-256 over cached copies and archives and prove that changed bytes never execute.
        let done = output(Command::of("bash").args(["-c", "source \"$1\"; platform=universal-apple-darwin; sha256() { binary_sha256 \"$platform\"; }; matches \"$2\"", "fixture"]).arg(&script).arg(&probe)).unwrap();
        assert_eq!(
            done.status.code(),
            Some(if valid { 0 } else { 1 }),
            "{version}: {done:?}"
        );
    }
}

#[test]
fn the_launcher_and_scheduler_share_the_pin_and_only_supported_hosts_are_selected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = root.join("tools/nextest.sh");
    assert!(
        fs::read_to_string(&script)
            .unwrap()
            .lines()
            .any(|line| line == format!("version={VERSION}"))
    );
    assert!(
        fs::read_to_string(root.join(".config/nextest.toml"))
            .unwrap()
            .lines()
            .any(|line| line == format!("nextest-version = \"{VERSION}\""))
    );
    for (system, machine, expected) in [
        ("Darwin", "arm64", Some("universal-apple-darwin")),
        ("Darwin", "x86_64", Some("universal-apple-darwin")),
        ("Linux", "aarch64", Some("aarch64-unknown-linux-gnu")),
        ("Linux", "x86_64", Some("x86_64-unknown-linux-gnu")),
        ("Other", "x86_64", None),
        ("Linux", "other", None),
    ] {
        let done = output(
            Command::of("bash")
                .args([
                    "-c",
                    "source \"$1\"; platform_name \"$2\" \"$3\"",
                    "fixture",
                ])
                .arg(&script)
                .args([system, machine]),
        )
        .unwrap();
        assert_eq!(
            done.status.code(),
            Some(if expected.is_some() { 0 } else { 2 }),
            "{done:?}"
        );
        if let Some(expected) = expected {
            assert_eq!(String::from_utf8(done.stdout).unwrap().trim(), expected);
        } else {
            assert!(
                String::from_utf8(done.stderr)
                    .unwrap()
                    .contains("no pinned cargo-nextest")
            );
        }
    }
}

#[test]
fn linked_worktrees_reuse_and_provision_only_the_main_cache() {
    let main = Lanes::new("nextest-main-cache");
    git(&main.root, &["add", "tools"]);
    git(&main.root, &["commit", "-q", "-m", "runner"]);
    let linked = main.stand.join("linked workspace");
    git(
        &main.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let linked = Lanes {
        root: linked.canonicalize().unwrap(),
        stand: main.stand.clone(),
    };
    let cached = main
        .root
        .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest"));
    let done = linked.tool(&["--where"], &[("FAIL_NEXTEST_SETUP", "1")]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap().trim(),
        cached.to_str().unwrap()
    );
    assert!(!linked.root.join(".target").exists());

    fs::remove_file(&cached).unwrap();
    let done = linked.tool(&["--check"], &[]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    let path = format!(
        "{}:{}",
        main.root.join(".target/nextest-fixture/bin").display(),
        std::env::var("PATH").unwrap()
    );
    let archive = main.root.join(".target/nextest-fixture/tool.tar.gz");
    let done = linked.tool(
        &["--setup"],
        &[
            ("PATH", &path),
            ("NEXT_TOOL_ARCHIVE", archive.to_str().unwrap()),
        ],
    );
    assert!(done.status.success(), "{done:?}");
    assert_eq!(fs::read_to_string(&cached).unwrap(), MOCK_TOOL);
    assert!(!linked.root.join(".target").exists());

    let own = linked
        .root
        .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest"));
    executable(&own, MOCK_TOOL);
    let done = linked.tool(&["--where"], &[]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap().trim(),
        own.to_str().unwrap()
    );
    // A corrupt local copy falls back without executing the corrupt bytes or downloading.
    executable(&own, "#!/bin/sh\nexit 79\n");
    let done = linked.tool(&["--where"], &[("FAIL_NEXTEST_SETUP", "1")]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap().trim(),
        cached.to_str().unwrap()
    );
}

#[test]
fn setup_refuses_a_failed_download_or_wrong_hash_and_leaves_no_partial_tool() {
    for corrupt in [false, true] {
        let lanes = Lanes::new("nextest-failed-setup");
        let folder = lanes.root.join(format!(".target/tools/nextest/{VERSION}"));
        fs::remove_file(folder.join("bin/cargo-nextest")).unwrap();
        let vars = if corrupt {
            fs::write(
                lanes.root.join(".target/nextest-fixture/tool.tar.gz"),
                "not the pinned archive",
            )
            .unwrap();
            vec![]
        } else {
            vec![("FAIL_NEXTEST_SETUP", "1")]
        };
        let done = lanes.tool(&["--setup"], &vars);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
        let err = String::from_utf8(done.stderr).unwrap();
        assert!(
            err.contains(if corrupt {
                "archive SHA-256"
            } else {
                "cannot fetch"
            }),
            "{err}"
        );
        assert!(!folder.join("bin/cargo-nextest").exists());
        assert_eq!(
            fs::read_dir(&folder).unwrap().count(),
            1,
            "only the empty bin folder survives"
        );
        let done = lanes.tool(&["--check"], &[]);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
    }
}

#[test]
fn the_runtime_refuses_changed_bytes_even_when_the_version_string_matches() {
    let lanes = Lanes::new("nextest-binary-hash");
    let binary = lanes
        .root
        .join(format!(".target/tools/nextest/{VERSION}/bin/cargo-nextest"));
    executable(&binary, &format!("{MOCK_TOOL}# changed bytes\n"));
    let done = lanes.tool(&["--check"], &[]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(
        String::from_utf8(done.stderr)
            .unwrap()
            .contains("missing or differs")
    );
    assert!(!lanes.stand.join("built").exists());
}

#[test]
fn the_named_live_fixture_is_not_retried_or_hidden_when_it_fails() {
    let lanes = Lanes::new("nextest-live-red");
    let done = lanes.run(&[], &[("FAIL_LIVE_FIXTURE", "101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert_eq!(
        said.matches("explicit runner fixture\n").count(),
        1,
        "{said}"
    );
    assert!(said.contains("the explicit runner fixture is red (exit 101), to rerun pass `-p symphony-build --lib -- --ignored release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test --exact`"), "{said}");
    assert_eq!(
        fs::read_dir(lanes.stand.join("started")).unwrap().count(),
        LANES
    );
}

#[test]
fn setup_and_runtime_use_the_default_relative_or_absolute_target_cache() {
    for variant in 0..3 {
        let lanes = Lanes::new(&format!("nextest-target-cache-{variant}"));
        let selected = match variant {
            0 => None,
            1 => Some("relative target".to_string()),
            _ => Some(lanes.stand.join("absolute target").display().to_string()),
        };
        let target = selected.as_ref().map_or_else(
            || lanes.root.join(".target"),
            |path| {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    lanes.root.join(path)
                }
            },
        );
        let binary = target.join(format!("tools/nextest/{VERSION}/bin/cargo-nextest"));
        if binary.exists() {
            fs::remove_file(&binary).unwrap();
        }
        executable(&lanes.stand.join("bin/cargo"), "#!/bin/sh\nexit 64\n");
        let vars: Vec<_> = selected
            .as_ref()
            .map(|path| ("CARGO_TARGET_DIR", path.as_str()))
            .into_iter()
            .collect();
        let done = lanes.tool(&["--setup"], &vars);
        assert!(done.status.success(), "{done:?}");
        assert_eq!(fs::read_to_string(&binary).unwrap(), MOCK_TOOL);
        assert!(!lanes.root.join("tools/.nextest").exists());
        let done = lanes.run(&[], &vars);
        assert!(done.status.success(), "{done:?}");
    }
}

#[test]
fn native_batch_slots_keep_the_processor_bound_and_fallback_when_no_number_is_available() {
    for (processors, at_once) in [("16", 8), ("5", 2), ("1", 1), ("", 1), ("many", 1)] {
        let lanes = Lanes::new("nextest-slots");
        let done = lanes.run(&[], &[("PROCESSORS", processors)]);
        assert!(done.status.success(), "{processors}: {done:?}");
        let said = String::from_utf8(done.stdout).unwrap();
        assert!(
            said.contains(&format!("lanes run at most {at_once} at once")),
            "{said}"
        );
        assert_eq!(
            fs::read_dir(lanes.stand.join("started")).unwrap().count(),
            LANES
        );
    }
}

#[test]
fn a_stall_reporter_failure_is_red_even_when_nextest_passes() {
    let lanes = Lanes::new("nextest-reporter-red");
    let done = lanes.run(&[], &[("FAIL_REPORTER", "1")]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    let log = fs::read_to_string(lanes.logs().join("nextest.log")).unwrap();
    assert!(
        log.contains("test-stalls: error: fixture reporter failed"),
        "{log}"
    );
    let args = fs::read_to_string(lanes.stand.join("reporter-args")).unwrap();
    assert_eq!(
        args,
        format!(
            "<test-stalls>\n<{}>\n",
            lanes.root.join(".config/nextest.toml").display()
        )
    );
    assert_eq!(
        fs::read_dir(lanes.stand.join("started")).unwrap().count(),
        LANES
    );
}

#[test]
fn a_relative_target_folder_names_its_built_binaries_for_every_test_batch() {
    let lanes = Lanes::new("nextest-target-folder");
    let debug = lanes.root.join("other-target/debug");
    executable(
        &lanes.root.join(format!(
            "other-target/tools/nextest/{VERSION}/bin/cargo-nextest"
        )),
        MOCK_TOOL,
    );
    fs::create_dir_all(&debug).unwrap();
    fs::write(debug.join("tasks"), "built tasks").unwrap();
    let done = lanes.run(&[], &[("CARGO_TARGET_DIR", "other-target")]);
    assert!(done.status.success(), "{done:?}");
    for name in [
        "nextest",
        "perf",
        "sweep",
        "cli",
        "server",
        "store",
        "launch",
        "libraries",
        "doc",
    ] {
        let log = fs::read_to_string(lanes.logs().join(format!("{name}.log"))).unwrap();
        assert!(
            log.contains(&format!(
                "binaries={}\ntasks={}\n",
                debug.display(),
                debug.join("tasks").display()
            )),
            "{log}"
        );
    }
}

#[test]
fn all_test_batches_get_an_isolated_hc_and_a_callers_override_wins() {
    for (name, vars, expected) in [
        ("nextest-hc", &[][..], "/usr/bin/false"),
        ("nextest-hc-empty", &[("HC_BIN", "")][..], "/usr/bin/false"),
        (
            "nextest-hc-named",
            &[("HC_BIN", "/callers/hc")][..],
            "/callers/hc",
        ),
    ] {
        let lanes = Lanes::new(name);
        let done = lanes.run(&[], vars);
        assert!(done.status.success(), "{done:?}");
        for name in [
            "nextest",
            "perf",
            "sweep",
            "cli",
            "server",
            "store",
            "launch",
            "libraries",
            "doc",
        ] {
            let log = fs::read_to_string(lanes.logs().join(format!("{name}.log"))).unwrap();
            assert_eq!(
                log.lines().find_map(|line| line.strip_prefix("hc=")),
                Some(expected),
                "{name}: {log}"
            );
        }
    }
}

/// A tests run of the stand-ins, its lanes' logs in `build/test-lanes`, then what it left there
/// for a rerun alone cleared of the stand-ins' own marks: the folder a failure is rerun over.
fn ran_for_alone(lanes: &Lanes) -> PathBuf {
    let done = lanes.run(&[], &[]);
    assert!(done.status.success(), "{done:?}");
    for mark in [
        "built",
        "nextest-args",
        "client-built",
        "runner-fixture-args",
    ] {
        let _ = fs::remove_file(lanes.stand.join(mark));
    }
    fs::remove_dir_all(lanes.stand.join("started")).unwrap();
    lanes.logs()
}

/// The lines `--alone` handed the stand-in nextest, one argument each, `<ARG>`.
fn nextest_args(lanes: &Lanes) -> String {
    fs::read_to_string(lanes.stand.join("nextest-args")).unwrap()
}

#[test]
fn a_run_says_its_end_last_and_keeps_the_client_it_served_and_one_cut_short_says_no_end() {
    let lanes = Lanes::new("alone-ended");
    let done = lanes.run(&[], &[("FAIL", "sweep")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let out = String::from_utf8(done.stdout).unwrap();
    assert_eq!(
        out.lines().last(),
        Some("test-lanes: ended: every lane asked for ran"),
        "{out}"
    );
    // The folder a failure is rerun over is said first, before any test runs.
    assert_eq!(
        out.lines().next(),
        Some(format!("test-lanes: logs in {}", lanes.logs().display()).as_str()),
        "{out}"
    );
    assert_eq!(
        fs::read_to_string(lanes.logs().join("client")).unwrap(),
        format!(
            "{}\n",
            lanes.root.join("build/symphony-ui/public").display()
        )
    );
    // A client the caller gives is the one kept.
    let done = lanes.run(&["sweep"], &[("SYMPHONY_UI_DIST", "/given/client")]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        fs::read_to_string(lanes.logs().join("client")).unwrap(),
        "/given/client\n"
    );
    // A build that failed ran no lane, and never says the end.
    let done = lanes.run(&[], &[("FAIL_BUILD", "101")]);
    assert_eq!(done.status.code(), Some(101), "{done:?}");
    let out = String::from_utf8(done.stdout).unwrap();
    assert!(!out.contains("test-lanes: ended"), "{out}");
}

#[test]
fn a_test_rerun_alone_runs_by_nextest_from_the_runs_own_build_and_builds_nothing() {
    let lanes = Lanes::new("alone-nextest");
    let logs = ran_for_alone(&lanes);
    let folder = logs.to_str().unwrap();
    let done = lanes.run(
        &[
            "--alone",
            folder,
            "-p",
            "symphony",
            "--test",
            "release",
            "--",
            "reds::one_fails",
            "--exact",
        ],
        &[],
    );
    assert!(done.status.success(), "{done:?}");
    assert!(
        !lanes.stand.join("built").exists(),
        "a rerun alone builds nothing"
    );
    let args = nextest_args(&lanes);
    for flag in [
        "<run>\n<--profile>\n<release>".to_string(),
        format!("<--binaries-metadata>\n<{folder}/binaries.json>"),
        "<--retries>\n<0>".into(),
        "<--no-tests>\n<fail>".into(),
        "<-E>\n<package(=symphony) & kind(test) & binary(=release) & test(=reds::one_fails)>"
            .into(),
    ] {
        assert!(args.contains(&flag), "{flag}: {args}");
    }
    assert!(!args.contains("<--workspace>"), "{args}");
    let out = String::from_utf8(done.stdout).unwrap();
    for said in [
        format!(
            "client={}\n",
            lanes.root.join("build/symphony-ui/public").display()
        ),
        "hc=/usr/bin/false\n".into(),
        format!("binaries={}\n", lanes.root.join(".target/debug").display()),
    ] {
        assert!(out.contains(&said), "{said:?} in {out}");
    }
    assert!(!lanes.stand.join("client-built").exists(), "{out}");
    assert!(
        !logs.join("alone").exists(),
        "a test alone writes nothing among the run's logs"
    );

    // Its status is the rerun's.
    let done = lanes.run(
        &[
            "--alone",
            folder,
            "-p",
            "p",
            "--lib",
            "--",
            "case::fails",
            "--exact",
        ],
        &[("FAIL", "nextest")],
    );
    assert_eq!(done.status.code(), Some(100), "{done:?}");
    assert!(
        nextest_args(&lanes).contains("<package(=p) & kind(lib) & test(=case::fails)>"),
        "{}",
        nextest_args(&lanes)
    );

    // A target that named no failed test is rerun whole: every test of that binary, no other.
    for (args, expression) in [
        (
            &["-p", "symphony-board", "--lib"][..],
            "package(=symphony-board) & kind(lib)",
        ),
        (
            &["-p", "symphony", "--bin", "symphony"][..],
            "package(=symphony) & kind(bin) & binary(=symphony)",
        ),
    ] {
        let mut line = vec!["--alone", folder];
        line.extend(args);
        let done = lanes.run(&line, &[]);
        assert!(done.status.success(), "{done:?}");
        let said = nextest_args(&lanes);
        assert!(said.contains(&format!("<-E>\n<{expression}>\n")), "{said}");
    }
}

#[test]
fn a_doctest_or_the_ignored_runner_fixture_rerun_alone_runs_as_its_lane_runs_it_by_cargo() {
    let lanes = Lanes::new("alone-cargo");
    let logs = ran_for_alone(&lanes);
    let folder = logs.to_str().unwrap();
    let done = lanes.run(
        &[
            "--alone",
            folder,
            "-p",
            "symphony",
            "--doc",
            "--",
            "src/lib.rs - f (line 1)",
            "--exact",
        ],
        &[],
    );
    assert!(done.status.success(), "{done:?}");
    let doc = fs::read_to_string(lanes.stand.join("started/doc")).unwrap();
    assert_eq!(
        doc,
        "test --config profile.dev.build-override.debug=\"line-tables-only\" --workspace --locked \
         --no-fail-fast --doc -- src/lib.rs - f (line 1) --exact\n"
    );
    let out = String::from_utf8(done.stdout).unwrap();
    assert!(
        out.contains(&format!(
            "client={}\n",
            lanes.root.join("build/symphony-ui/public").display()
        )),
        "{out}"
    );
    assert!(!lanes.stand.join("nextest-args").exists());
    assert!(!lanes.stand.join("built").exists());

    let done = lanes.run(
        &[
            "--alone",
            folder,
            "-p",
            "symphony-build",
            "--lib",
            "--",
            "--ignored",
            "release::nextest::tests::the_live_fixture",
            "--exact",
        ],
        &[],
    );
    assert!(done.status.success(), "{done:?}");
    let live = fs::read_to_string(lanes.stand.join("started/libraries")).unwrap();
    assert!(
        live.ends_with(
            "--workspace --locked --no-fail-fast --lib -- --ignored \
             release::nextest::tests::the_live_fixture --exact\n"
        ),
        "{live}"
    );
    assert!(!lanes.stand.join("nextest-args").exists());
}

#[test]
fn a_lanes_own_line_or_a_shared_target_whole_reruns_that_lane_alone_apart_from_the_runs_logs() {
    let lanes = Lanes::new("alone-lane");
    let logs = ran_for_alone(&lanes);
    let folder = logs.to_str().unwrap();
    let run_listed = fs::read_to_string(logs.join("lanes.tsv")).unwrap();
    for (args, lane) in [
        (&["--workspace", "--test", "perf"][..], "perf"),
        (&["-p", "symphony-board", "--test", "store"][..], "store"),
        (&["--workspace", "--lib"][..], "libraries"),
        (&["--workspace", "--doc"][..], "doc"),
    ] {
        let mut line = vec!["--alone", folder];
        line.extend(args);
        let done = lanes.run(&line, &[("FAIL", lane)]);
        assert_eq!(done.status.code(), Some(101), "{args:?}: {done:?}");
        let mut started: Vec<String> = (fs::read_dir(lanes.stand.join("started")).unwrap())
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        symphony_board::order::sort_by(&mut started, &Ord::cmp);
        assert_eq!(started, [lane], "{args:?}");
        fs::remove_dir_all(lanes.stand.join("started")).unwrap();
        let apart = logs.join("alone");
        let listed = fs::read_to_string(apart.join("lanes.tsv")).unwrap();
        assert!(
            listed.starts_with(&format!("{lane}\t101\t")) && listed.lines().count() == 1,
            "{args:?}: {listed}"
        );
        let log = fs::read_to_string(apart.join(format!("{lane}.log"))).unwrap();
        assert!(
            log.contains(&format!(
                "client={}\n",
                lanes.root.join("build/symphony-ui/public").display()
            )),
            "the client the run served: {log}"
        );
    }
    assert!(
        !lanes.stand.join("client-built").exists(),
        "no client is built for a lane alone"
    );
    assert_eq!(
        fs::read_to_string(logs.join("lanes.tsv")).unwrap(),
        run_listed,
        "the run's own list is left as it was"
    );
}

#[test]
fn a_rerun_alone_refuses_words_it_cannot_read_and_a_folder_with_no_run_and_runs_nothing() {
    let lanes = Lanes::new("alone-refused");
    let logs = ran_for_alone(&lanes);
    let folder = logs.to_str().unwrap();
    for args in [
        &[][..],
        &["-p", "p"],
        &["-p"],
        &["--lib"],
        &["-p", "p", "-p", "q", "--lib"],
        &["--workspace", "-p", "p", "--lib"],
        &["-p", "p", "--lib", "--doc"],
        &["-p", "p", "--test"],
        &["-p", "p", "--test", ""],
        &["-p", "p", "--lib", "--", "name"],
        &["-p", "p", "--lib", "--", "a", "b", "--exact"],
        &["-p", "p", "--lib", "--", "", "--exact"],
        &["-p", "p", "--lib", "--", "--ignored", "--exact"],
        &["--workspace", "--test", "perf", "--", "name", "--exact"],
        &["--workspace", "--test", "no-such-lane"],
        &["--workspace", "--bin", "perf"],
        &["-p", "p", "--frobnicate"],
        &["-p", "p", "--lib", "stray"],
    ] {
        let mut line = vec!["--alone", folder];
        line.extend(args);
        let done = lanes.run(&line, &[]);
        assert_eq!(done.status.code(), Some(2), "{args:?}: {done:?}");
        assert!(
            String::from_utf8_lossy(&done.stderr).starts_with("usage: tools/test-lanes.sh "),
            "{args:?}: {done:?}"
        );
    }
    let done = lanes.run(&["--alone", ""], &[]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    // A folder that holds no run's build and client.
    for missing in ["client", "binaries.json"] {
        let copy = lanes.stand.join(format!("without-{missing}"));
        fs::create_dir_all(&copy).unwrap();
        for kept in ["client", "binaries.json"] {
            if kept != missing {
                fs::copy(logs.join(kept), copy.join(kept)).unwrap();
            }
        }
        let done = lanes.run(
            &["--alone", copy.to_str().unwrap(), "-p", "p", "--lib"],
            &[],
        );
        assert_eq!(done.status.code(), Some(2), "{missing}: {done:?}");
        assert!(
            String::from_utf8_lossy(&done.stderr).contains("no tests run to rerun alone over"),
            "{done:?}"
        );
    }
    assert!(!lanes.stand.join("started").exists());
    assert!(!lanes.stand.join("nextest-args").exists());
    assert!(!lanes.stand.join("built").exists());
}

/// The file `path` is, as the system names it: two names of one file have the same.
fn identity(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::symlink_metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

#[test]
fn every_binarys_copy_is_its_build_before_a_batch_starts_so_no_cargo_call_removes_it() {
    let lanes = Lanes::new("nextest-binary-links");
    let debug = lanes.root.join(".target/debug");
    let deps = debug.join("deps");
    let file = |path: &Path, text: &str| {
        executable(path, text);
        path.to_path_buf()
    };
    // Cargo's copy of the newest build, beside an older build and a dep-info file that says the
    // same bytes: only the build itself, by name and bytes, is its source.
    file(
        &deps.join("tool_one-0000000000000000"),
        "tool one, an older build",
    );
    let info = file(&deps.join("tool_one-0000000000000000.d"), "tool one");
    let one = file(&deps.join("tool_one-ffffffffffffffff"), "tool one");
    let one_copy = file(&debug.join("tool-one"), "tool one");
    // A copy that is its build already, as Cargo links one where it can.
    let two = file(&deps.join("tool_two-1111111111111111"), "tool two");
    let two_copy = debug.join("tool-two");
    fs::hard_link(&two, &two_copy).unwrap();
    // A copy no build under deps/ is.
    file(&deps.join("stray-2222222222222222"), "stray, its build");
    let stray = file(&debug.join("stray"), "stray, another");
    let before = [&info, &two_copy, &stray].map(|path| identity(path));
    let mut metadata: serde_json::Value = serde_json::from_str(METADATA).unwrap();
    metadata["rust-build-meta"]["non-test-binaries"] = serde_json::json!({
        "first-package": [
            {"name": "tool-one", "kind": "bin-exe", "path": "debug/tool-one"},
            {"name": "tool-two", "kind": "bin-exe", "path": "debug/tool-two"}
        ],
        "second-package": [
            {"name": "stray", "kind": "bin-exe", "path": "debug/stray"},
            {"name": "gone", "kind": "bin-exe", "path": "debug/gone"}
        ]
    });
    fs::write(lanes.stand.join("metadata.json"), metadata.to_string()).unwrap();
    let uplift = format!(
        "{}={} {}={}",
        one.display(),
        one_copy.display(),
        two.display(),
        two_copy.display()
    );
    let done = lanes.run(&[], &[("UPLIFT", &uplift)]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(identity(&one_copy), identity(&one));
    assert_eq!(fs::read_to_string(&one_copy).unwrap(), "tool one");
    assert_eq!(
        before,
        [&info, &two_copy, &stray].map(|path| identity(path))
    );
    assert_ne!(
        identity(&stray),
        identity(&deps.join("stray-2222222222222222"))
    );
    assert!(!debug.join("gone").exists());
    let err = String::from_utf8_lossy(&done.stderr);
    assert_eq!(
        err.lines()
            .filter(|line| line.contains("no link to its build"))
            .collect::<Vec<_>>(),
        [format!(
            "test-lanes: {} is no link to its build under deps/: a batch starting while a test runs it may leave it gone",
            stray.display()
        )],
        "{err}"
    );
    let left: Vec<_> = (fs::read_dir(&debug).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".test-lanes"))
        .collect();
    assert_eq!(left, Vec::<String>::new());
    // The serial build test and the runner fixture copy it again, before any batch starts; each of
    // the eight batches' Cargo calls (perf, sweep, cli, server, store, launch, libraries, doc) finds
    // its build there, and the one already linked is never copied.
    let uplifts = fs::read_to_string(lanes.stand.join("uplifts")).unwrap();
    let said = |copy: &Path| {
        (uplifts.lines())
            .filter_map(|line| line.strip_suffix(&format!(" {}", copy.display())))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said(&one_copy),
        [["copied"; 2].as_slice(), &["kept"; 8]].concat(),
        "{uplifts}"
    );
    assert_eq!(said(&two_copy), ["kept"; 10], "{uplifts}");
}

#[test]
fn an_interrupt_while_the_binaries_are_linked_starts_no_lane() {
    let lanes = Lanes::new("nextest-link-interrupt");
    let debug = lanes.root.join(".target/debug");
    executable(&debug.join("deps/tool_one-ffffffffffffffff"), "tool one");
    let copy = debug.join("tool-one");
    executable(&copy, "tool one");
    let mut metadata: serde_json::Value = serde_json::from_str(METADATA).unwrap();
    metadata["rust-build-meta"]["non-test-binaries"] = serde_json::json!({
        "first-package": [{"name": "tool-one", "kind": "bin-exe", "path": "debug/tool-one"}]
    });
    fs::write(lanes.stand.join("metadata.json"), metadata.to_string()).unwrap();
    // The linking alone compares a copy with its build: a comparison that holds until the signal.
    let comparing = lanes.stand.join("comparing");
    assert!(
        output(Command::of("mkfifo").arg(&comparing))
            .unwrap()
            .status
            .success()
    );
    executable(
        &lanes.stand.join("bin/cmp"),
        "#!/bin/sh\nexec 3<>\"$STAND/comparing\"\n: >\"$STAND/comparing.held\"\nread -r held <&3\nexit 1\n",
    );
    let done = lanes.signalled(
        libc::SIGINT,
        &[],
        "the linking held in its comparison",
        || lanes.stand.join("comparing.held").exists(),
    );
    assert_eq!(done.status.code(), Some(130), "{done:?}");
    assert!(!lanes.stand.join("started").exists(), "{done:?}");
    let said = String::from_utf8(done.stdout).unwrap();
    assert!(!said.contains("test-lanes: lanes run at most"), "{said}");
    assert!(!said.contains("test-lanes: ended"), "{said}");
    // The serial steps ran before it, and are listed as they ended; no batch is.
    assert_eq!(
        listed(&lanes),
        [("build".to_string(), 0), ("fixture".to_string(), 0)]
    );
    // The comparison cut short left the copy as Cargo made it, and no link half made.
    assert_eq!(fs::read_to_string(&copy).unwrap(), "tool one");
    assert!(!debug.join("tool-one.test-lanes").exists());
}
