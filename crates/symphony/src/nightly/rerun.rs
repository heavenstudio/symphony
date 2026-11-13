//! What reruns a red check's failures alone (RULES.md `tests/rerun-the-failure-alone`), read from
//! its log in one pass ([`read`]): each test target cargo said failed (`error: test failed, to
//! rerun pass `ARGS``, a doctest's and a bench's alike), with the tests libtest named `FAILED`
//! since the target before it, and each recipe just said failed (`error: recipe `NAME` failed …`).
//! A target that ended naming none, as one that crashed does, is rerun whole. A `just verify` that
//! listed no steps (one older than its steps' list) and failed in no test is rerun by the step its
//! recipes name ([`step`]): its lint, its client check or its size budget, each by its own recipe;
//! its docs audit's recipe says nothing when it fails (`[no-exit-message]`), so no line names that
//! one. A step's red is read from its own log. A test named before the log ended with no target
//! after it (a run stopped at its time) is left to the log's tail.

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

/// What a red check's log says failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Log {
    /// Each test target cargo said failed, in the log's order.
    pub(crate) targets: Vec<Failed>,
    /// Each recipe just said failed, in the log's order: the innermost first, as just says them.
    pub(crate) recipes: Vec<String>,
}

/// What follows a target's failure in cargo's line, before the arguments that rerun it.
const RERUN: &str = ", to rerun pass `";

/// What the log at `log` says failed; nothing when it cannot be read.
pub(crate) fn read(log: &Path) -> Log {
    let Ok(file) = File::open(log) else {
        return Log::default();
    };
    let mut reader = BufReader::new(file);
    let (mut found, mut tests) = (Log::default(), Vec::new());
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
            found.targets.push(Failed {
                target,
                tests: std::mem::take(&mut tests),
            });
        } else if let Some(recipe) = recipe(text) {
            found.recipes.push(recipe);
        }
    }
    found
}

/// The recipe a just line says failed: NAME of `error: recipe `NAME` failed …`, `Recipe` as an
/// older just writes it.
fn recipe(line: &str) -> Option<String> {
    let rest = line.strip_prefix("error: ")?;
    let rest = (rest.strip_prefix("recipe `")).or_else(|| rest.strip_prefix("Recipe `"))?;
    let (name, after) = rest.split_once('`')?;
    (after.starts_with(" failed") && !name.is_empty()).then(|| name.to_string())
}

/// The command that reruns alone the step of a `just verify` that lists none that `recipes`
/// ([`Log::recipes`]) say failed: `just NAME` for the last of them, the one nearest `verify` as
/// just names them, that is neither verify's own (`verify`, `_verify`) nor private (`_NAME`), so a
/// step is named by the recipe the justfile gives it and by no list of its own here; none when no
/// such recipe is there.
pub(crate) fn step(recipes: &[String]) -> Option<String> {
    (recipes.iter().rev())
        .find(|recipe| recipe.as_str() != "verify" && !recipe.starts_with('_'))
        .map(|recipe| format!("just {recipe}"))
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

    fn read_in(text: &str) -> Log {
        let tree = Tree::new();
        read(&tree.write("verify.log", text))
    }

    fn failed_in(text: &str) -> Vec<Failed> {
        read_in(text).targets
    }

    fn recipes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    /// The end of the nightly's verify log of 2026-11-12, a verify that listed no steps, red in its
    /// size budget: the step's own recipe, the public one it runs through, and verify.
    const SIZE: &str = "    Finished `release` profile [optimized] target(s) in 17m 36s
size-budget: agent-send       4.9 MiB of 5.0 MiB
size-budget: symphony is 18.8 MiB (19,719,136 bytes), 0.5 MiB past its budget of 18.3 MiB (19,200,000 bytes) in tools/size-budget.toml.
size-budget: tasks            23.2 MiB of 23.6 MiB
error: recipe `_size-check` failed on line 101 with exit code 1
error: recipe `size-check` failed on line 96 with exit code 1
error: recipe `verify` failed on line 105 with exit code 1
";

    #[test]
    fn a_red_step_of_verify_is_rerun_by_its_own_command() {
        let size = read_in(SIZE);
        assert_eq!(
            size,
            Log {
                targets: Vec::new(),
                recipes: recipes(&["_size-check", "size-check", "verify"]),
            }
        );
        assert_eq!(step(&size.recipes).as_deref(), Some("just size-check"));
        for (said, command) in [
            (&["_lint-rs", "lint-rs", "verify"][..], "just lint-rs"),
            (
                &["_client-check", "client-check", "_verify", "verify"],
                "just client-check",
            ),
        ] {
            assert_eq!(step(&recipes(said)).as_deref(), Some(command), "{said:?}");
        }
        // The tests' step and the docs audit (whose recipe says nothing), verify itself and no
        // recipe at all name no step.
        for said in [&["_verify", "verify"][..], &["verify"], &[]] {
            assert_eq!(step(&recipes(said)), None, "{said:?}");
        }
        // An older just's capital, and lines that only look like one.
        assert_eq!(
            read_in("error: Recipe `lint-rs` failed on line 21 with exit code 101\n").recipes,
            ["lint-rs"]
        );
        for line in [
            "error: recipe `` failed on line 1",
            "error: recipe `lint-rs` succeeded",
            "warning: recipe `lint-rs` failed on line 1",
            "  error: recipe `lint-rs` failed on line 1",
        ] {
            assert_eq!(read_in(line).recipes, Vec::<String>::new(), "{line}");
        }
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
        assert_eq!(
            read(Path::new("/no/such/verify.log")),
            Log::default(),
            "no log"
        );
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
        assert_eq!(read(&log).targets, [target("--lib", &["a"])]);
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
