//! The release test runner over local Cargo/nextest stand-ins: one build, explicit shared-fixture
//! exceptions, the client build before concurrent readers, no retries, pinned setup, and live logs.

use crate::children::Child as _;
use crate::scratch;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;
use symphony_board::sys::{Command, Stdio, group_alive, output, wait_output};

const LANES: usize = 9;
const VERSION: &str = "0.9.152";
const CARGO: &str = r#"#!/bin/bash
if [ "$1" = nextest ] && [ "$2" = list ]; then
    printf '%s\n' "$*" >>"$STAND/built"
    if [ -n "${FAIL_BUILD:-}" ]; then exit "$FAIL_BUILD"; fi
    cat "$METADATA"
    exit 0
fi
if [ "$1" = nextest ]; then
    name=nextest
    printf '<%s>\n' "$@" >"$STAND/nextest-args"
else
    case " $* " in
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
        let stand = dir.join("stand");
        executable(&stand.join("cargo"), CARGO);
        executable(
            &stand.join("bin/getconf"),
            "#!/bin/sh\nprintf '%s\\n' \"${PROCESSORS-32}\"\n",
        );
        executable(
            &root.join(format!("tools/.nextest/{VERSION}/bin/cargo-nextest")),
            &format!("#!/bin/sh\nprintf 'cargo-nextest {VERSION}\\n'\n"),
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
                    "{}:{}",
                    self.stand.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("CARGO", self.stand.join("cargo"))
            .env("METADATA", self.stand.join("metadata.json"))
            .env("STAND", &self.stand)
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
            .env_remove("UNSAID")
            .env_remove("HOLD")
            .envs(vars.iter().copied())
            .stdin(Stdio::null());
        command
    }
    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(&mut self.command(args, vars)).unwrap()
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
        let mut child = self
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

    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        output(&mut self.command(args, vars)).unwrap()
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
        let mut child = self
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
            {"binary":"/build/deps/stateless-exact", "shared":false}
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
        .join(format!("tools/.nextest/{VERSION}/bin/cargo-nextest"));
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
            .join(format!("tools/.nextest/{VERSION}/bin/cargo-nextest")),
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
    for args in [&["unknown"][..], &["--list", "--list"]] {
        let done = lanes.run(args, &[]);
        assert_eq!(done.status.code(), Some(2), "{done:?}");
        assert_eq!(done.stderr, b"usage: tools/test-lanes.sh [--list]\n");
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
            said.ends_with(&format!("error: 1 target failed:\n    `{target}`\n")),
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
        said.ends_with("error: 1 target failed:\n    `--workspace --test build`\n"),
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
            assert!(fs::read_to_string(log).unwrap().starts_with("running "));
        }
    }
}

#[test]
fn setup_can_seed_the_pinned_runner_from_a_matching_local_install_without_a_build() {
    let lanes = Lanes::new("nextest-setup");
    let binary = lanes
        .root
        .join(format!("tools/.nextest/{VERSION}/bin/cargo-nextest"));
    fs::remove_file(&binary).unwrap();
    let seed = lanes.stand.join("bin/cargo-nextest");
    executable(&seed, &format!("#!/bin/sh\necho cargo-nextest {VERSION}\n"));
    let path = format!(
        "{}:{}",
        lanes.stand.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let done = output(
        Command::of(lanes.root.join("tools/nextest.sh"))
            .arg("--setup")
            .env("PATH", path)
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    assert_eq!(fs::read(&binary).unwrap(), fs::read(seed).unwrap());
    assert!(!lanes.stand.join("built").exists());
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
fn a_relative_target_folder_names_its_built_binaries_for_every_test_batch() {
    let lanes = Lanes::new("nextest-target-folder");
    let debug = lanes.root.join("other-target/debug");
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
