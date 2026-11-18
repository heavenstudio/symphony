//! The pinned runner's libtest-json-plus 0.1 events. Captured test output is a JSON string,
//! never another event: a fixture printing a failed test cannot become a release failure.
//! Suite-start events identify Cargo targets; test events identify a case by `SUITE$TEST`.

use std::collections::HashMap;

/// The live case the release selects explicitly; a narrow rerun must keep its ignored flag.
pub(super) const LIVE_FIXTURE: &str =
    "release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) target: String,
    pub(super) suite: String,
    pub(super) name: String,
    pub(super) timed_out: bool,
    pub(super) stall: Option<String>,
    pub(super) output: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Started,
    Failed(Failure),
    Other,
}

#[derive(Clone, Default)]
pub(super) struct Reader {
    targets: HashMap<String, String>,
}

impl Reader {
    pub(super) fn line(&mut self, line: &str) -> Option<Event> {
        // Captured human output is indented by nextest. Do not trim it into an event.
        if !line.starts_with('{') {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        let kind = value.get("type")?.as_str()?;
        let event = value.get("event")?.as_str()?;
        if kind == "suite" && event == "started" {
            let meta = value.get("nextest")?;
            let package = meta.get("crate")?.as_str()?;
            let binary = meta.get("test_binary")?.as_str()?;
            let flag = match meta.get("kind")?.as_str()? {
                "lib" => "--lib".to_string(),
                "test" => format!("--test {}", super::rerun::quoted(binary)),
                "bin" => format!("--bin {}", super::rerun::quoted(binary)),
                "bench" => format!("--bench {}", super::rerun::quoted(binary)),
                "example" => format!("--example {}", super::rerun::quoted(binary)),
                _ => return None,
            };
            self.targets.insert(
                format!("{package}::{binary}"),
                format!("-p {} {flag}", super::rerun::quoted(package)),
            );
            return Some(Event::Started);
        }
        if kind == "test" && event == "failed" {
            let (suite, name) = value.get("name")?.as_str()?.split_once('$')?;
            if name.is_empty() {
                return None;
            }
            return Some(Event::Failed(Failure {
                target: self.targets.get(suite)?.clone(),
                suite: suite.to_string(),
                name: name.to_string(),
                timed_out: value.get("reason").and_then(|v| v.as_str())
                    == Some("time limit exceeded"),
                stall: value.get("symphony-stall").and_then(|stall| {
                    let seconds = stall.get("limit_seconds")?.as_u64().filter(|s| *s > 0)?;
                    let load = stall.get("load")?.as_str().filter(|s| !s.is_empty())?;
                    Some(format!("taken as a stall: limit {seconds}s; load {load}"))
                }),
                output: value
                    .get("stdout")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }));
        }
        matches!(kind, "test" | "suite").then_some(Event::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "the release tests step selects this live runner fixture explicitly"]
    fn the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test() {
        let tree = crate::test_tree::Tree::new();
        tree.write("Cargo.toml", include_str!("fixtures/nextest/Cargo.toml"));
        tree.write("src/lib.rs", include_str!("fixtures/nextest/src/lib.rs"));
        let config = tree.write(
            ".config/nextest.toml",
            include_str!("fixtures/nextest/.config/nextest.toml"),
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = std::process::Command::new(root.join("tools/nextest.sh"))
            .arg("--where")
            .output()
            .expect("the workspace nextest launcher starts");
        assert!(
            found.status.success(),
            "{}",
            String::from_utf8_lossy(&found.stderr)
        );
        let binary = String::from_utf8(found.stdout).unwrap();
        let done = std::process::Command::new(binary.trim_end())
            .args([
                "nextest",
                "run",
                "--profile",
                "default",
                "--offline",
                "--color",
                "never",
                "--no-fail-fast",
                "--message-format",
                "libtest-json-plus",
                "--message-format-version",
                "0.1",
                "--failure-output",
                "never",
                "--success-output",
                "never",
            ])
            .arg("--manifest-path")
            .arg(tree.path("Cargo.toml"))
            .arg("--config-file")
            .arg(config)
            .current_dir(&tree.root)
            .env("CARGO_TARGET_DIR", tree.path("target"))
            .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
            // The outer runner exports its profile; this fixture owns a different config.
            .env("NEXTEST_PROFILE", "release")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .output()
            .expect("the pinned nextest is installed: just nextest-setup");
        assert_eq!(done.status.code(), Some(100), "{done:?}");
        let backstop = tree.write(
            "backstop.toml",
            "[profile.release]\nslow-timeout={period='1s',terminate-after=1}\n",
        );
        let (mut annotated, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            crate::commands::test_stalls::main(
                &[backstop.display().to_string()],
                &mut done.stdout.as_slice(),
                &mut annotated,
                &mut err,
            ),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
        let log = tree.write("run.log", &String::from_utf8(annotated).unwrap());
        let quoted = super::super::failures::read(&log).quoted().unwrap();
        assert!(
            quoted.contains("test runner-fixture::runner_fixture fails ... FAILED"),
            "{quoted}"
        );
        assert!(quoted.contains("fixture failure"), "{quoted}");
        assert!(
            quoted.contains("test runner-fixture::runner_fixture timeout ... TIMED OUT"),
            "{quoted}"
        );
        assert!(
            quoted.contains("taken as a stall: limit 1s; load "),
            "{quoted}"
        );
        let rerun = super::super::rerun::read(&log);
        let mut args = super::super::rerun::arguments(&rerun.targets);
        args.sort();
        assert_eq!(
            args,
            [
                "-p runner-fixture --lib -- fails --exact",
                "-p runner-fixture --lib -- timeout --exact"
            ]
        );
        eprintln!("Live nextest fixture reader:\n{quoted}");
        for args in args {
            eprintln!("Fixture rerun arguments: {args}");
        }
    }

    #[test]
    fn the_default_focused_run_leaves_the_live_fixture_explicitly_ignored_without_tools() {
        let tree = crate::test_tree::Tree::new();
        let target = tree.path("target-with-no-tools");
        let done = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", LIVE_FIXTURE])
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        assert!(done.status.success(), "{done:?}");
        let out = String::from_utf8(done.stdout).unwrap();
        assert!(
            out.contains("test result: ok. 0 passed; 0 failed; 1 ignored;"),
            "{out}"
        );
        assert!(!target.exists());
    }

    #[test]
    fn each_cargo_target_kind_has_an_exact_selector() {
        for (kind, flag) in [
            ("lib", "--lib"),
            ("test", "--test case"),
            ("bin", "--bin case"),
            ("bench", "--bench case"),
            ("example", "--example case"),
        ] {
            let mut reader = Reader::default();
            let start = serde_json::json!({"type":"suite", "event":"started", "nextest":{
                "crate":"p", "test_binary":"case", "kind":kind
            }});
            assert_eq!(reader.line(&start.to_string()), Some(Event::Started));
            let failed = r#"{"type":"test","event":"failed","name":"p::case$edge::case"}"#;
            assert_eq!(
                reader.line(failed),
                Some(Event::Failed(Failure {
                    target: format!("-p p {flag}"),
                    suite: "p::case".into(),
                    name: "edge::case".into(),
                    timed_out: false,
                    stall: None,
                    output: String::new(),
                }))
            );
        }
    }

    #[test]
    fn unrelated_malformed_indented_and_unidentified_events_are_not_failures() {
        let mut reader = Reader::default();
        for line in [
            "",
            "{",
            "{}",
            r#"{"type":"error","event":"failed"}"#,
            r#"{"type":"test","event":"failed","name":"p::case$unregistered"}"#,
            r#"{"type":"test","event":"failed","name":"p::case$"}"#,
            r#"{"type":"suite","event":"started","nextest":{"crate":"p","test_binary":"case","kind":"unknown"}}"#,
            r#"    {"type":"suite","event":"started","nextest":{"crate":"p","test_binary":"case","kind":"lib"}}"#,
        ] {
            assert_eq!(reader.line(line), None, "{line}");
        }
        assert_eq!(
            reader.line(r#"{"type":"test","event":"ok"}"#),
            Some(Event::Other)
        );
    }

    #[test]
    fn a_real_fixture_run_distinguishes_failed_timed_out_and_successful_tests() {
        let mut reader = Reader::default();
        let failures: Vec<_> = include_str!("fixtures/tests-nextest-red.log")
            .lines()
            .filter_map(|line| match reader.line(line) {
                Some(Event::Failed(failure)) => Some(failure),
                _ => None,
            })
            .collect();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].name, "fails");
        assert_eq!(failures[0].target, "-p runner-fixture --lib");
        assert!(failures[0].output.contains("fixture failure"));
        assert!(!failures[0].timed_out);
        assert_eq!(failures[1].name, "timeout");
        assert!(failures[1].timed_out);
    }

    #[test]
    fn captured_events_are_data_and_two_suites_can_fail_in_the_same_named_test() {
        let mut reader = Reader::default();
        for binary in ["first", "second"] {
            let start = serde_json::json!({"type":"suite", "event":"started", "nextest":{
                "crate":"p", "test_binary":binary, "kind":"test"
            }});
            assert_eq!(reader.line(&start.to_string()), Some(Event::Started));
        }
        for binary in ["second", "first"] {
            let event = serde_json::json!({"type":"test", "event":"failed",
                "name":format!("p::{binary}$same$case"),
                "stdout":"{\"type\":\"test\",\"event\":\"failed\",\"name\":\"fake\"}\n"
            });
            let Some(Event::Failed(failure)) = reader.line(&event.to_string()) else {
                panic!()
            };
            assert_eq!(failure.target, format!("-p p --test {binary}"));
            assert_eq!(failure.name, "same$case");
            assert_eq!(failure.suite, format!("p::{binary}"));
        }
    }
}
