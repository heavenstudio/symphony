//! What reruns a red check's failures alone (RULES.md `tests/rerun-the-failure-alone`), read from
//! its log: each test target cargo said failed (`error: test failed, to rerun pass `ARGS``, a
//! doctest's and a bench's alike), with the tests libtest named `FAILED` since the target before
//! it. A target that ended naming none, as one that crashed does, is rerun whole. Nothing else in a
//! log is read, so a check whose failure is no test's (lint, a build, the docs audit) names nothing
//! narrower than itself, and a test named before the log ended with no target after it (a run
//! stopped at its time) is left to the log's tail.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// A test target cargo said failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failed {
    /// The arguments that select it, as cargo said them: `-p symphony --test nightly`.
    pub(crate) target: String,
    /// The tests libtest named failed in it, in the order it named them.
    pub(crate) tests: Vec<String>,
}

/// What follows a target's failure in cargo's line, before the arguments that rerun it.
const RERUN: &str = ", to rerun pass `";

/// The test targets the log at `log` says failed, in its order; none when it cannot be read.
pub(crate) fn failed(log: &Path) -> Vec<Failed> {
    let Ok(file) = File::open(log) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let (mut found, mut tests) = (Vec::new(), Vec::new());
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if let Some(name) = failed_test(text) {
            if !tests.contains(&name) {
                tests.push(name);
            }
        } else if let Some(target) = target(text) {
            found.push(Failed {
                target,
                tests: std::mem::take(&mut tests),
            });
        }
    }
    found
}

/// The test a libtest line names failed, `test NAME ... FAILED`, a `should panic` test's name
/// without that note.
fn failed_test(line: &str) -> Option<String> {
    let name = line.strip_prefix("test ")?.strip_suffix(" ... FAILED")?;
    let name = name.strip_suffix(" - should panic").unwrap_or(name);
    (!name.trim().is_empty()).then(|| name.to_string())
}

/// The arguments a cargo line says rerun a failed target: ARGS of
/// `error: test failed, to rerun pass `ARGS``.
fn target(line: &str) -> Option<String> {
    let (_, args) = line.strip_prefix("error: ")?.split_once(RERUN)?;
    let args = args.strip_suffix('`')?;
    (!args.trim().is_empty() && !args.contains('`')).then(|| args.to_string())
}

/// The cargo test arguments that rerun each failure of `failed` alone: `ARGS -- NAME --exact` for
/// each test it named, and `ARGS` for a target that named none.
pub(crate) fn arguments(failed: &[Failed]) -> Vec<String> {
    let mut each = Vec::new();
    for target in failed {
        if target.tests.is_empty() {
            each.push(target.target.clone());
        }
        for test in &target.tests {
            each.push(format!("{} -- {} --exact", target.target, quoted(test)));
        }
    }
    each
}

/// `word` as a shell reads it back: as it is when it holds nothing a shell reads apart, else in
/// single quotes.
fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_:./-".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    /// What cargo 1.99 printed for `cargo test --no-fail-fast` in a workspace: a library whose
    /// tests failed (one a `should panic`), an integration target with two, one that crashed
    /// naming none, a passing one and a doctest, then its summary, which repeats the targets.
    const LOG: &str = "   Compiling symphony v0.1.0 (/wt/nightly/crates/symphony)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.77s
     Running unittests src/lib.rs (.target/debug/deps/symphony_board-c55f883224b5f683)

running 3 tests
test tests::lib_passes ... ok
test tests::should_but_does_not - should panic ... FAILED
test tests::lib_fails ... FAILED

failures:

---- tests::lib_fails stdout ----
assertion `left == right` failed

failures:
    tests::lib_fails
    tests::should_but_does_not

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p symphony-board --lib`
     Running tests/nightly/main.rs (.target/debug/deps/nightly-dfaaafee719b0dda)

running 3 tests
test reds::passes ... ok
test reds::one_fails ... FAILED
test reds::two_fails ... FAILED

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p symphony --test nightly`
     Running tests/beta.rs (.target/debug/deps/beta-81cdc7807bf4ce06)

running 1 test
error: test failed, to rerun pass `-p symphony --test beta`

Caused by:
  process didn't exit successfully: `.target/debug/deps/beta-81cdc7807bf4ce06` (signal: 6, SIGABRT: process abort signal)
     Running tests/green.rs (.target/debug/deps/green-0123)

running 1 test
test fine ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests symphony

running 1 test
test src/lib.rs - f (line 1) ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

error: doctest failed, to rerun pass `-p symphony --doc`
error: 4 targets failed:
    `-p symphony-board --lib`
    `-p symphony --test nightly`
    `-p symphony --test beta`
    `-p symphony --doc`
";

    fn failed_in(text: &str) -> Vec<Failed> {
        let tree = Tree::new();
        failed(&tree.write("verify.log", text))
    }

    fn target(target: &str, tests: &[&str]) -> Failed {
        Failed {
            target: target.into(),
            tests: tests.iter().map(|test| (*test).to_string()).collect(),
        }
    }

    #[test]
    fn each_target_cargo_said_failed_is_named_once_with_the_tests_failed_in_it() {
        assert_eq!(
            failed_in(LOG),
            [
                target(
                    "-p symphony-board --lib",
                    &["tests::should_but_does_not", "tests::lib_fails"]
                ),
                target(
                    "-p symphony --test nightly",
                    &["reds::one_fails", "reds::two_fails"]
                ),
                target("-p symphony --test beta", &[]),
                target("-p symphony --doc", &["src/lib.rs - f (line 1)"]),
            ],
            "the summary's repeat of the targets adds none"
        );
    }

    #[test]
    fn each_failure_is_rerun_alone_and_a_target_that_named_none_whole() {
        assert_eq!(
            arguments(&failed_in(LOG)),
            [
                "-p symphony-board --lib -- tests::should_but_does_not --exact",
                "-p symphony-board --lib -- tests::lib_fails --exact",
                "-p symphony --test nightly -- reds::one_fails --exact",
                "-p symphony --test nightly -- reds::two_fails --exact",
                "-p symphony --test beta",
                "-p symphony --doc -- 'src/lib.rs - f (line 1)' --exact",
            ]
        );
    }

    #[test]
    fn a_log_with_no_target_cargo_said_failed_names_nothing() {
        for (text, case) in [
            ("", "an empty log"),
            (
                "error: could not compile `symphony` (lib) due to 2 previous errors\n\
                 error: Recipe `lint-rs` failed on line 21 with exit code 101\n",
                "a lint that failed",
            ),
            (
                "running 2 tests\ntest a ... FAILED\ntest b ... ok\n(stopped after 36000 s: the \
                 run's time was up)\n",
                "a test named failed and no target after it: the run stopped",
            ),
            (
                "error: test failed, to rerun pass ``\nerror: test failed, to rerun pass `-p a\n",
                "a rerun line that names no arguments, or never ends",
            ),
            (
                "    `-p symphony --lib`\ntest result: FAILED. 0 passed; 1 failed\n",
                "a summary's line alone",
            ),
        ] {
            assert_eq!(failed_in(text), [], "{case}");
        }
        assert_eq!(failed(Path::new("/no/such/verify.log")), [], "no log");
    }

    #[test]
    fn a_test_named_failed_twice_is_rerun_once_and_names_left_unclosed_go_to_no_target() {
        let text = "test a ... FAILED\ntest a ... FAILED\n\
                    error: test failed, to rerun pass `--lib`\n\
                    test b ... FAILED\n\
                    error: test failed, to rerun pass `--test t`\n\
                    test c ... FAILED\n";
        assert_eq!(
            failed_in(text),
            [target("--lib", &["a"]), target("--test t", &["b"])]
        );
    }

    #[test]
    fn a_log_that_is_not_utf8_is_read_as_far_as_it_says_something() {
        let tree = Tree::new();
        let log = tree.path("verify.log");
        std::fs::write(
            &log,
            b"\xff\xfe noise\r\ntest a ... FAILED\r\nerror: test failed, to rerun pass `--lib`\r\n",
        )
        .unwrap();
        assert_eq!(failed(&log), [target("--lib", &["a"])]);
    }

    #[test]
    fn a_name_a_shell_reads_apart_is_quoted() {
        assert_eq!(quoted("reds::a_red_check"), "reds::a_red_check");
        assert_eq!(quoted("src/lib.rs"), "src/lib.rs");
        assert_eq!(
            quoted("src/lib.rs - f (line 1)"),
            "'src/lib.rs - f (line 1)'"
        );
        assert_eq!(quoted("it's"), r"'it'\''s'");
        assert_eq!(quoted("$HOME"), "'$HOME'");
        assert_eq!(quoted(""), "''");
    }
}
