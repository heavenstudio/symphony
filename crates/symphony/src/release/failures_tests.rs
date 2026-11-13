//! What a red check's log says failed, read from logs cut from a release run's (run
//! 2026-11-13T0900-61777: its tests' step, its client check, a step stopped at the run's time) and
//! from logs of the other shapes a step's red takes.

use super::*;
use crate::test_tree::Tree;

/// The tests' step of run 2026-11-13T0900-61777, cut: three tests failed in two targets, among
/// passing tests, a browser's lines and a test whose own output says a refusal and a recipe
/// failed; doc-tests after them, then cargo's list.
const TESTS_RED: &str = include_str!("fixtures/tests-red.log");
/// Its client check, cut: a client build's lines (a crate named `thiserror`, warnings in colour, an
/// asset copied per line), then the check's refusal and just's lines.
const CLIENT_CHECK_RED: &str = include_str!("fixtures/client-check-red.log");
/// A step stopped at the run's time while it built, which says no failure of its own.
const NO_FAILURE: &str = include_str!("fixtures/no-failure.log");

fn read_in(text: &str) -> Failures {
    let tree = Tree::new();
    read(&tree.write("step.log", text))
}

/// What the red task quotes of `text`, the fence's lines; none when it names nothing.
fn quoted_lines(text: &str) -> Option<Vec<String>> {
    let quoted = read_in(text).quoted()?;
    let fenced = quoted
        .strip_prefix("What failed, as its log says it:\n\n```\n")
        .and_then(|rest| rest.strip_suffix("\n```"))
        .unwrap_or_else(|| panic!("not one fence: {quoted}"));
    Some(fenced.lines().map(str::to_string).collect())
}

#[test]
fn a_test_runs_red_names_each_failed_test_with_where_it_panicked_then_cargos_list() {
    assert_eq!(
        quoted_lines(TESTS_RED).unwrap(),
        [
            "test tests::a_real_page_blocked_in_its_renderer_spends_nothing_and_a_stopped_browser_\
             ends_at_the_backstop ... FAILED",
            "    panicked at crates/chrome-devtools/src/tests.rs:2005:5: the blocked page spent its \
             wait in 2.008174209s",
            "test plugins::process::tests::a_stdout_that_fails_ends_its_process_killed_and_the_\
             plugin_down ... FAILED",
            "    panicked at crates/symphony/src/plugins/process_tests.rs:1020:5: assertion `left \
             == right` failed",
            "test plugins::process::tests::a_call_that_finds_its_process_ending_waits_to_be_told_\
             how_it_ended ... FAILED",
            "    panicked at crates/symphony/src/plugins/process_tests.rs:1284:42: called \
             `Result::unwrap()` on an `Err` value: \"its process did not declare: no answer in \
             10s\"",
            "error: 2 targets failed:",
            "    `-p chrome-devtools --lib`",
            "    `-p symphony --lib`",
        ],
        "the tests' own output (a refusal, a recipe failed), the doc-tests and the run's last \
         lines are not named"
    );
}

#[test]
fn a_client_checks_red_names_its_refusal_and_justs_lines_never_its_build() {
    assert_eq!(
        quoted_lines(CLIENT_CHECK_RED).unwrap(),
        [
            "client-check: rebuild and commit the client asset: index.html",
            "error: recipe `_client-check` failed on line 98 with exit code 1",
            "error: recipe `client-check` failed on line 93 with exit code 1",
        ],
        "not `thiserror`, a warning in colour or the place a warning gives, nor an asset's line"
    );
}

#[test]
fn a_log_that_says_no_failure_it_knows_names_nothing() {
    for (text, case) in [
        (
            NO_FAILURE,
            "a step stopped at the run's time while it built",
        ),
        ("", "an empty log"),
        ("\n\n  \n", "blank lines"),
        (
            "error: recipe `verify` failed on line 1 with exit code 1\n",
            "just's line alone: an error, so named",
        ),
    ] {
        let named = read_in(text).quoted();
        if case.ends_with("so named") {
            assert!(named.is_some(), "{case}");
        } else {
            assert_eq!(named, None, "{case}");
        }
    }
    assert_eq!(read(Path::new("/no/such/step.log")), Failures::default());
    assert_eq!(read(Path::new("/no/such/step.log")).quoted(), None);
}

#[test]
fn a_size_budgets_red_names_every_line_of_its_tool_among_them_the_binary_past_its_line() {
    let log = "   Compiling symphony-server v0.1.0 (/w/apps/symphony-server)
    Finished `release` profile [optimized] target(s) in 17m 36s
size-budget: agent-send       4.9 MiB of 5.0 MiB
size-budget: symphony is 18.8 MiB (19,719,136 bytes), 0.5 MiB past its budget of 18.3 MiB (19,200,000 bytes) in tools/size-budget.toml.
size-budget: tasks            23.2 MiB of 23.6 MiB
error: recipe `_size-check` failed on line 101 with exit code 1
error: recipe `size-check` failed on line 96 with exit code 1
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "size-budget: agent-send       4.9 MiB of 5.0 MiB",
            "size-budget: symphony is 18.8 MiB (19,719,136 bytes), 0.5 MiB past its budget of 18.3 \
             MiB (19,200,000 bytes) in tools/size-budget.toml.",
            "size-budget: tasks            23.2 MiB of 23.6 MiB",
            "error: recipe `_size-check` failed on line 101 with exit code 1",
            "error: recipe `size-check` failed on line 96 with exit code 1",
        ]
    );
}

#[test]
fn a_lints_red_names_each_error_with_its_place_and_not_a_warning() {
    let log = "    Checking symphony v0.1.0 (/w/crates/symphony)
warning: unused import: `std::fmt`
 --> crates/symphony/src/a.rs:1:5
error: this expression creates a reference which is immediately dereferenced by the compiler
  --> crates/symphony/src/b.rs:10:9
   |
10 |     f(&x);
   = help: for further information visit https://rust-lang.github.io/rust-clippy
error: could not compile `symphony` (lib) due to 1 previous error
error: recipe `_lint-rs` failed on line 21 with exit code 101
error: recipe `lint-rs` failed on line 20 with exit code 101
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "error: this expression creates a reference which is immediately dereferenced by the \
             compiler",
            "  --> crates/symphony/src/b.rs:10:9",
            "error: could not compile `symphony` (lib) due to 1 previous error",
            "error: recipe `_lint-rs` failed on line 21 with exit code 101",
            "error: recipe `lint-rs` failed on line 20 with exit code 101",
        ]
    );
}

#[test]
fn a_docs_audits_red_names_each_line_it_marks_and_a_refusal_in_any_case() {
    // The audit's recipe says nothing when it fails, so no recipe line follows it.
    let log = "⚠ 1 warning of a file this machine or a clone lacks
✖ docs/A.md:5: names `just gate`, which no project has
✖ docs/B.md: a broken link to C.md
Refused: the board is behind its schema
the same error twice
the same error twice
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "✖ docs/A.md:5: names `just gate`, which no project has",
            "✖ docs/B.md: a broken link to C.md",
            "Refused: the board is behind its schema",
            "the same error twice",
        ],
        "a line said twice is named once"
    );
}

#[test]
fn a_word_counts_when_it_starts_with_the_stem_never_inside_another() {
    for (line, stem, said) in [
        ("error: x", "error", true),
        ("error[E0425]: cannot find value", "error", true),
        ("2 Errors", "error", true),
        ("  0.00s ERROR dx failed", "error", true),
        ("an-error here", "error", true),
        ("(error)", "error", true),
        ("Compiled [ 49/167]: thiserror", "error", false),
        ("thiserror_impl", "error", false),
        ("my_error", "error", false),
        ("errno", "error", false),
        ("the push was refused", "refus", true),
        ("a refusal", "refus", true),
        ("unrefused", "refus", false),
        ("", "error", false),
    ] {
        assert_eq!(says(line, stem), said, "{line:?} {stem}");
    }
}

#[test]
fn a_tool_is_the_word_a_line_starts_with_before_its_colon_never_a_compilers() {
    for (line, tool_said) in [
        ("client-check: rebuild", Some("client-check")),
        ("size-budget: tasks 1 MiB", Some("size-budget")),
        ("symphony_ui: x", Some("symphony_ui")),
        ("docs audit: every document renders", None),
        ("error: recipe `x` failed", None),
        ("Warning: x", None),
        ("note: x", None),
        ("help: x", None),
        ("  indented: x", None),
        ("a:b", None),
        (": x", None),
        ("no colon", None),
    ] {
        assert_eq!(tool(line), tool_said, "{line:?}");
    }
    // A last word the compiler said is named, and groups nothing beside it.
    let log = "warning: a\nerror: b\nwarning: c\nerror: recipe `x` failed on line 1\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "error: b",
            "warning: c",
            "error: recipe `x` failed on line 1"
        ]
    );
    // Nor does one with no word before its colon.
    let log = "docs audit: a\ndocs audit: one page is stale\nerror: recipe `x` failed on line 1\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "docs audit: one page is stale",
            "error: recipe `x` failed on line 1"
        ]
    );
}

#[test]
fn the_last_word_is_the_line_before_justs_last_recipe_lines() {
    // Recipes that failed earlier in the log (a step's own retries) do not make its last word.
    let log = "first try\nerror: recipe `a` failed on line 1\n\nbuilding\nclient-check: the \
               committed client lacks assets/x.js\n\nerror: recipe `_client-check` failed on line \
               98\nerror: recipe `client-check` failed on line 93\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "error: recipe `a` failed on line 1",
            "client-check: the committed client lacks assets/x.js",
            "error: recipe `_client-check` failed on line 98",
            "error: recipe `client-check` failed on line 93",
        ]
    );
}

#[test]
fn many_failed_tests_are_cut_at_the_listed_and_counted() {
    let mut log = String::from("     Running unittests src/lib.rs (.target/debug/deps/p-01)\n");
    for n in 0..LISTED + 5 {
        log.push_str(&format!("test case_{n} ... FAILED\n"));
    }
    log.push_str("error: test failed, to rerun pass `-p p --lib`\n");
    let lines = quoted_lines(&log).unwrap();
    assert_eq!(lines.len(), LISTED + 2, "{lines:#?}");
    assert_eq!(lines[0], "test case_0 ... FAILED");
    assert_eq!(
        lines[LISTED - 1],
        format!("test case_{} ... FAILED", LISTED - 1)
    );
    assert_eq!(lines[LISTED], "… and 5 more tests named FAILED in the log");
    assert_eq!(
        lines[LISTED + 1],
        "error: test failed, to rerun pass `-p p --lib`",
        "with no list of cargo's, each target's own line"
    );
    // Lines of another log are cut the same way.
    let errors: String = (0..LISTED + 2).map(|n| format!("error: e{n}\n")).collect();
    let lines = quoted_lines(&errors).unwrap();
    assert_eq!(lines.len(), LISTED + 1);
    assert_eq!(lines[LISTED], "… and 2 more such lines in the log");
}

#[test]
fn a_long_line_is_cut_at_its_width() {
    let long = "x".repeat(WIDTH + 10);
    let lines = quoted_lines(&format!("error: {long}\n")).unwrap();
    assert_eq!(lines, [format!("error: {}…", &long[..WIDTH - 7])]);
    let panic = "y".repeat(WIDTH);
    let log = format!(
        "test t ... FAILED\n---- t stdout ----\nthread 't' panicked at src/a.rs:1:2:\n{panic}\n\
         error: test failed, to rerun pass `--lib`\n"
    );
    let lines = quoted_lines(&log).unwrap();
    assert_eq!(lines[1].chars().count(), 4 + WIDTH + 1, "{}", lines[1]);
    assert!(lines[1].ends_with("y…"), "{}", lines[1]);
    assert_eq!(
        cut("é".repeat(WIDTH).as_str()),
        "é".repeat(WIDTH),
        "no cut at the width"
    );
}

#[test]
fn a_panic_is_its_tests_by_its_block_or_by_its_threads_name_within_its_target() {
    let log = "     Running tests/a.rs (.target/debug/deps/a-01)
thread 'early' (7) panicked at tests/a.rs:3:5:
said before its line, as without capture
test early ... FAILED
test spawned ... FAILED
test should_but_does_not - should panic ... FAILED
test same ... FAILED

failures:

---- spawned stdout ----
thread '<unnamed>' panicked at tests/a.rs:9:9:
in a thread it started

---- should_but_does_not stdout ----
note: test did not panic as expected

---- same stdout ----
thread 'same' panicked at tests/a.rs:20:1:
the first target's

failures:
    early
test result: FAILED. 0 passed; 4 failed

error: test failed, to rerun pass `-p p --test a`
     Running tests/b.rs (.target/debug/deps/b-02)
test same ... FAILED
test older ... FAILED

failures:

---- same stdout ----
thread 'same' panicked at tests/b.rs:4:4:
the second target's
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- older stdout ----
thread 'older' panicked at 'boom', tests/b.rs:8:3
test result: FAILED. 0 passed; 2 failed

error: test failed, to rerun pass `-p p --test b`
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "test early ... FAILED",
            "    panicked at tests/a.rs:3:5: said before its line, as without capture",
            "test spawned ... FAILED",
            "    panicked at tests/a.rs:9:9: in a thread it started",
            "test should_but_does_not ... FAILED",
            "test same ... FAILED",
            "    panicked at tests/a.rs:20:1: the first target's",
            "test same ... FAILED",
            "    panicked at tests/b.rs:4:4: the second target's",
            "test older ... FAILED",
            "    panicked at 'boom', tests/b.rs:8:3",
            "error: test failed, to rerun pass `-p p --test a`",
            "error: test failed, to rerun pass `-p p --test b`",
        ]
    );
    // A panic that says nothing on its next line has nothing added, and one of a thread no failed
    // test is named for goes to none.
    let log = "thread 'other' panicked at src/a.rs:1:1:\n\ntest t ... FAILED\n---- t stdout ----\n\
               thread 't' panicked at src/a.rs:2:2:\n\nerror: test failed, to rerun pass `--lib`\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "test t ... FAILED",
            "    panicked at src/a.rs:2:2:",
            "error: test failed, to rerun pass `--lib`"
        ]
    );
}

#[test]
fn a_target_that_crashed_naming_no_test_is_named_by_cargos_lines() {
    let log = "     Running tests/beta.rs (.target/debug/deps/beta-81cd)

running 1 test
error: test failed, to rerun pass `-p symphony --test beta`

Caused by:
  process didn't exit successfully: `.target/debug/deps/beta-81cd` (signal: 6, SIGABRT: process abort signal)
error: test failed, to rerun pass `-p symphony --test beta`
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        ["error: test failed, to rerun pass `-p symphony --test beta`"],
        "said twice, named once"
    );
    // Cargo's list of one target, and the last list when a test's own output printed another.
    let log = "error: 3 targets failed:\n    `-p x --lib`\n    `-p y --lib`\n    `-p z --lib`\n\
               test t ... FAILED\nerror: test failed, to rerun pass `-p p --lib`\n\
               error: 1 target failed:\n    `-p p --lib`\n\nwarning: after it\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "test t ... FAILED",
            "error: 1 target failed:",
            "    `-p p --lib`"
        ]
    );
}

#[test]
fn a_list_of_cargos_is_its_count_of_targets_failed() {
    for (line, list) in [
        ("error: 2 targets failed:", true),
        ("error: 1 target failed:", true),
        ("error: 12 targets failed:", true),
        ("error: two targets failed:", false),
        ("error:  targets failed:", false),
        ("error: 2 targets failed", false),
        ("  error: 2 targets failed:", false),
    ] {
        assert_eq!(listing(line), list, "{line:?}");
    }
    for (line, name) in [
        ("---- a::b stdout ----", Some("a::b")),
        ("---- a stderr ----", Some("a")),
        ("----  stdout ----", None),
        ("---- a ----", None),
    ] {
        assert_eq!(section(line), name, "{line:?}");
    }
    assert_eq!(
        panicked("thread 'a' (12) panicked at src/a.rs:1:2:"),
        Some(("a", "panicked at src/a.rs:1:2:"))
    );
    assert_eq!(panicked("thread 'a' finished"), None);
    assert_eq!(panicked("the thread 'a' panicked at x"), None);
}

#[test]
fn a_terminals_colour_codes_are_taken_out_before_a_line_is_read() {
    assert_eq!(plain("\x1b[1m\x1b[31merror\x1b[0m: boom"), "error: boom");
    assert_eq!(plain("\x1b[38;5;196mred\x1b[m"), "red");
    assert_eq!(plain("a\x1b7b"), "ab", "an escape of one character");
    assert_eq!(plain("plain"), "plain");
    assert!(matches!(plain("plain"), Cow::Borrowed(_)));
    assert_eq!(plain("cut\x1b[1"), "cut", "a sequence the line ends inside");
    // So a coloured error is named, and without its colour.
    assert_eq!(
        quoted_lines("\x1b[1m\x1b[31merror\x1b[0m\x1b[1m: could not compile `x`\x1b[0m\n").unwrap(),
        ["error: could not compile `x`"]
    );
}

#[test]
fn a_log_that_is_not_utf8_is_read_as_far_as_it_says_something() {
    let tree = Tree::new();
    let log = tree.path("step.log");
    std::fs::write(
        &log,
        b"\xff\xfe noise\r\nclient-check: refused \xff\r\nerror: recipe `x` failed on line 1\r\n",
    )
    .unwrap();
    assert_eq!(
        read(&log).lines,
        [
            "client-check: refused \u{fffd}",
            "error: recipe `x` failed on line 1"
        ]
    );
}
