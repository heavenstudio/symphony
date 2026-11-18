//! What a red check's log says failed, read from logs cut from a release run's (run
//! 2026-11-13T0900-61777: its tests' step, its client check, its size check made red, a step and a
//! test run stopped at the run's time) and from logs of the other shapes a step's red takes.

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
/// Its tests' step stopped at the run's time before a test failed: passing tests, one whose own
/// output says a refusal and a recipe failed, a browser's lines saying a file missing.
const TESTS_STOPPED: &str = include_str!("fixtures/tests-stopped.log");
/// Its size check, made red: cargo's release build (among its crates `gix-error`), then the size
/// budget's lines, one binary past its line, and just's.
const SIZE_CHECK_RED: &str = include_str!("fixtures/size-check-red.log");
/// A tests' step run in lanes (tools/test-lanes.sh): the build, then each lane's output whole, two
/// of them red, each with its own list, one a test printed among them, then the lists as one.
const TESTS_LANES_RED: &str = include_str!("fixtures/tests-lanes-red.log");
/// A tests' step run in lanes with three red, two of which listed no target of their own (one
/// killed, one whose cargo failed before any target), each named by the runner, then the lists as
/// one.
const TESTS_LANES_DEAD: &str = include_str!("fixtures/tests-lanes-dead.log");

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
fn a_test_run_in_lanes_names_each_failed_test_then_the_lists_of_its_red_lanes_as_one() {
    assert_eq!(
        quoted_lines(TESTS_LANES_RED).unwrap(),
        [
            "test land::a ... FAILED",
            "    panicked at crates/tasks/tests/sweep/land.rs:5:9: the land never ended",
            "test cut::b ... FAILED",
            "    panicked at crates/symphony/tests/release/cut.rs:7:5: assertion failed: green",
            "error: 2 targets failed:",
            "    `-p symphony-board --test sweep`",
            "    `-p symphony-app --test release`",
        ],
        "neither a lane's own list nor one a test printed is the run's"
    );
}

#[test]
fn a_red_lane_that_listed_no_target_of_its_own_is_named_in_the_runs_list() {
    // The sweep's lane killed after a test failed, the browser's cargo failing before any target,
    // the board's red in a target it listed.
    assert_eq!(
        quoted_lines(TESTS_LANES_DEAD).unwrap(),
        [
            "test land::a ... FAILED",
            "error: 3 targets failed:",
            "    `--workspace --test sweep`",
            "    `--workspace --test browser`",
            "    `-p symphony-board --test cli`",
        ]
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
        (
            TESTS_STOPPED,
            "a test run stopped at the run's time before a test failed",
        ),
        (
            &TESTS_RED[..TESTS_RED.find(" ... FAILED").unwrap()],
            "a test run cut before its first failed test",
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
fn a_size_checks_red_names_its_tools_lines_and_never_a_crate_cargo_built() {
    let lines = quoted_lines(SIZE_CHECK_RED).unwrap();
    assert_eq!(
        lines[..4],
        [
            "size-budget: agent-send       3.2 MiB of 5.0 MiB; its line comes down to 3,540,000 \
             (3,404,752 bytes plus 4 %)",
            "size-budget: symphony         13.6 MiB of 13.9 MiB",
            "size-budget: symphony-server  17.3 MiB of 17.7 MiB (36.6 MiB with the embedded \
             client)",
            "size-budget: embedded client  19.4 MiB of 19.8 MiB (111 files)",
        ],
        "{lines:#?}"
    );
    assert!(
        lines[4].starts_with(
            "size-budget: tasks is 23.7 MiB (24,851,000 bytes), 0.1 MiB past its budget of 23.6 \
             MiB (24,746,000 bytes) in tools/size-budget.toml."
        ) && lines[4].ends_with('…'),
        "the binary past its line, cut at the width: {lines:#?}"
    );
    assert_eq!(
        lines[5..],
        [
            "error: recipe `_size-check` failed on line 108 with exit code 1",
            "error: recipe `size-check` failed on line 104 with exit code 1",
        ],
        "never `   Compiling gix-error v0.3.2`, nor another line of cargo's: {lines:#?}"
    );
}

#[test]
fn a_line_of_cargos_progress_is_its_word_right_aligned_in_twelve_columns() {
    for (line, said) in [
        ("   Compiling gix-error v0.3.2", true),
        ("    Checking gix-error v0.3.2", true),
        (
            "    Finished `release` profile [optimized] target(s) in 18m 51s",
            true,
        ),
        ("  Downloaded gix-error v0.3.2", true),
        (" Documenting symphony v0.1.0 (/w/crates/symphony)", true),
        (
            "     Running unittests src/lib.rs (.target/debug/deps/p-01)",
            true,
        ),
        ("   Doc-tests symphony", true),
        (
            "    Blocking waiting for file lock on build directory",
            true,
        ),
        ("Compiling gix-error v0.3.2", false),
        ("  Compiling gix-error v0.3.2", false),
        ("   Compiling", false),
        ("       Error refused the push", false),
        ("error: could not compile `gix-error`", false),
        ("", false),
        ("é", false),
    ] {
        assert_eq!(progress(line), said, "{line:?}");
    }
    // So a lint run's `Checking` of such a crate is not named, its error is.
    let log = "    Checking gix-error v0.3.2\n    Checking my-error v0.1.0 (/w/crates/my-error)\n\
               error: unused variable: `x`\nerror: recipe `lint-rs` failed on line 1\n";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "error: unused variable: `x`",
            "error: recipe `lint-rs` failed on line 1"
        ]
    );
}

#[test]
fn a_test_run_that_names_no_failure_names_only_what_its_build_said_before_its_first_target() {
    let log = "   Compiling x v0.1.0 (/w/x)
error[E0425]: cannot find value `y` in this scope
  --> src/lib.rs:3:5
error: could not compile `x` (lib test) due to 1 previous error
     Running unittests src/a.rs (.target/debug/deps/a-01)

running 1 test
the server refused: error 3
test t ... ok
error: recipe `test-rs` failed on line 1 with exit code 101
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "error[E0425]: cannot find value `y` in this scope",
            "  --> src/lib.rs:3:5",
            "error: could not compile `x` (lib test) due to 1 previous error",
        ]
    );
    // A log that quotes cargo's line, indented, as a check quotes its step's tail, is no test run.
    let log = "verify: tests: red, exit 101 (4741 s); its last lines:
       Doc-tests test_time
    error: 2 targets failed:
verify: red in tests
error: recipe `_verify` failed on line 120 with exit code 1
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "verify: tests: red, exit 101 (4741 s); its last lines:",
            "    error: 2 targets failed:",
            "verify: red in tests",
            "error: recipe `_verify` failed on line 120 with exit code 1",
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
        ("  0.00s ERROR vite failed", "error", true),
        ("(error)", "error", true),
        ("level=ERROR", "error", true),
        ("\"error\": \"x\"", "error", true),
        ("Compiled [ 49/167]: thiserror", "error", false),
        ("thiserror_impl", "error", false),
        ("my_error", "error", false),
        ("errno", "error", false),
        // A word joined to another by `-`, `.`, `/` or `:` is part of a name.
        ("   Compiling gix-error v0.3.2", "error", false),
        ("an-error here", "error", false),
        ("rustc --error-format=json", "error", false),
        ("  --> src/error.rs:3:5", "error", false),
        ("self.error", "error", false),
        (
            "warning: unused import: `std::error::Error`",
            "error",
            false,
        ),
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
        ("symphony_web: x", Some("symphony_web")),
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
    assert_eq!(lines[LISTED], "… and 5 more failed tests in the log");
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

failures:
    older
    same

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
               thread 't' panicked at src/a.rs:2:2:\n\nfailures:\n    t\n\n\
               error: test failed, to rerun pass `--lib`\n";
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
fn what_a_failed_test_printed_is_never_the_runs_and_its_own_threads_panic_is_its() {
    // A test that printed a run of its own (a nested cargo test, a red task's body it filed): that
    // run's target, failed test, list and panic are not the log's; the test's own thread's panic,
    // said after a thread's it started, is the test's.
    let log = "     Running tests/a.rs (.target/debug/deps/a-01)
test nests ... FAILED
test plain ... FAILED

failures:

---- nests stdout ----
     Running unittests src/lib.rs (/tmp/fx/target/debug/deps/fx-02)
test inner ... FAILED

failures:

---- inner stdout ----
thread 'inner' panicked at src/lib.rs:1:1:
the inner one's

failures:
    inner

test result: FAILED. 0 passed; 1 failed
error: test failed, to rerun pass `-p fx --lib`
error: 1 target failed:
    `-p fx --lib`
thread 'nests' panicked at tests/a.rs:5:5:
the nested run failed

---- plain stdout ----
thread '<unnamed>' panicked at tests/a.rs:8:8:
a thread it started
thread 'plain' (9) panicked at tests/a.rs:9:9:
its own
thread '<unnamed>' panicked at tests/a.rs:10:10:
another after it

failures:
    nests
    plain

test result: FAILED. 0 passed; 2 failed

error: test failed, to rerun pass `-p p --test a`
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "test nests ... FAILED",
            "    panicked at tests/a.rs:5:5: the nested run failed",
            "test plain ... FAILED",
            "    panicked at tests/a.rs:9:9: its own",
            "error: test failed, to rerun pass `-p p --test a`",
        ]
    );
    // A list the test printed that names none of the failed tests does not end its block, which
    // libtest's own list ends.
    let log = "test t ... FAILED

failures:

---- t stdout ----
failures:
    inner
error: test failed, to rerun pass `-p fx --lib`
thread 't' panicked at src/a.rs:1:1:
its own

failures:
    t

error: test failed, to rerun pass `-p p --lib`
";
    assert_eq!(
        quoted_lines(log).unwrap(),
        [
            "test t ... FAILED",
            "    panicked at src/a.rs:1:1: its own",
            "error: test failed, to rerun pass `-p p --lib`",
        ]
    );
}

/// Three targets that failed, shaped as a `cargo test --no-fail-fast` log is: each failed test's
/// block, then libtest's list of them, its result and cargo's line, and cargo's list at the end.
const THREE_TARGETS: &str = "     Running tests/golden_card.rs (target/debug/deps/golden_card-1a2b)

running 2 tests
test discovers_its_card ... FAILED
test draws_its_card ... ok

failures:

---- discovers_its_card stdout ----
thread 'discovers_its_card' (31) panicked at tools/art/tests/golden_card.rs:209:5:
assertion `left == right` failed


failures:
    discovers_its_card

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.10s

error: test failed, to rerun pass `-p art --test golden_card`
     Running tests/after_tool_runner.rs (target/debug/deps/after_tool_runner-3c4d)

running 1 test
test the_catalogue_agrees_with_the_budget ... FAILED

failures:

---- the_catalogue_agrees_with_the_budget stdout ----
thread 'the_catalogue_agrees_with_the_budget' (32) panicked at tools/gate/tests/after_tool_runner.rs:898:5:
✖ a target discovered with no entry in the budget


failures:
    the_catalogue_agrees_with_the_budget

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s

error: test failed, to rerun pass `-p gate --test after_tool_runner`
     Running tests/scope/main.rs (target/debug/deps/scope-5e6f)

running 3 tests
test quoting::forwards_each_word ... FAILED
test quoting::splits_packed_flags ... FAILED
test quoting::keeps_empty_words ... ok

failures:

---- quoting::forwards_each_word stdout ----
thread 'quoting::forwards_each_word' (33) panicked at tools/gate/tests/scope/quoting.rs:92:71:
called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound }

---- quoting::splits_packed_flags stdout ----
thread 'quoting::splits_packed_flags' (34) panicked at tools/gate/tests/scope/quoting.rs:96:9:
the flags were not split


failures:
    quoting::forwards_each_word
    quoting::splits_packed_flags

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s

error: test failed, to rerun pass `-p gate --test scope`
error: 3 targets failed:
    `-p art --test golden_card`
    `-p gate --test after_tool_runner`
    `-p gate --test scope`
";

/// What [`THREE_TARGETS`] names: every failed test with its own panic, then cargo's list.
const THREE_NAMED: [&str; 12] = [
    "test discovers_its_card ... FAILED",
    "    panicked at tools/art/tests/golden_card.rs:209:5: assertion `left == right` failed",
    "test the_catalogue_agrees_with_the_budget ... FAILED",
    "    panicked at tools/gate/tests/after_tool_runner.rs:898:5: ✖ a target discovered with no \
     entry in the budget",
    "test quoting::forwards_each_word ... FAILED",
    "    panicked at tools/gate/tests/scope/quoting.rs:92:71: called `Result::unwrap()` on an `Err` \
     value: Os { code: 2, kind: NotFound }",
    "test quoting::splits_packed_flags ... FAILED",
    "    panicked at tools/gate/tests/scope/quoting.rs:96:9: the flags were not split",
    "error: 3 targets failed:",
    "    `-p art --test golden_card`",
    "    `-p gate --test after_tool_runner`",
    "    `-p gate --test scope`",
];

/// [`THREE_TARGETS`] with `from` put as `to`, which it holds once.
fn three_targets_with(from: &str, to: &str) -> String {
    assert_eq!(THREE_TARGETS.matches(from).count(), 1, "{from:?}");
    THREE_TARGETS.replacen(from, to, 1)
}

/// [`THREE_NAMED`] with no panic for the tests `without`.
fn three_named_without(without: &[&str]) -> Vec<String> {
    let mut named: Vec<String> = THREE_NAMED.iter().map(|line| line.to_string()).collect();
    for test in without {
        let at = (named.iter())
            .position(|line| *line == format!("test {test} ... FAILED"))
            .unwrap();
        named.remove(at + 1);
    }
    named
}

#[test]
fn a_test_binary_that_died_in_a_block_takes_no_panic_and_hides_no_later_failure() {
    assert_eq!(
        quoted_lines(THREE_TARGETS).unwrap(),
        three_named_without(&[])
    );
    // The first target's binary killed while it printed its failures, under `--no-fail-fast`:
    // cargo says so and goes on to the next target, whose failures are all named, each with its
    // own panic, and the killed test with none.
    let killed = "error: test failed, to rerun pass `-p art --test golden_card`

Caused by:
  process didn't exit successfully: `target/debug/deps/golden_card-1a2b` (signal: 9, SIGKILL: kill)
";
    let log = three_targets_with(
        "thread 'discovers_its_card' (31) panicked at tools/art/tests/golden_card.rs:209:5:
assertion `left == right` failed


failures:
    discovers_its_card

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.10s

error: test failed, to rerun pass `-p art --test golden_card`
",
        killed,
    );
    assert_eq!(
        quoted_lines(&log).unwrap(),
        three_named_without(&["discovers_its_card"])
    );
    // The last target's killed in its first block: cargo's list after it is still named.
    let log = three_targets_with(
        "thread 'quoting::forwards_each_word' (33) panicked at tools/gate/tests/scope/quoting.rs:92:71:
called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound }

---- quoting::splits_packed_flags stdout ----
thread 'quoting::splits_packed_flags' (34) panicked at tools/gate/tests/scope/quoting.rs:96:9:
the flags were not split


failures:
    quoting::forwards_each_word
    quoting::splits_packed_flags

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s

error: test failed, to rerun pass `-p gate --test scope`
",
        "error: test failed, to rerun pass `-p gate --test scope`

Caused by:
  process didn't exit successfully: `target/debug/deps/scope-5e6f` (signal: 9, SIGKILL: kill)
",
    );
    assert_eq!(
        quoted_lines(&log).unwrap(),
        three_named_without(&[
            "quoting::forwards_each_word",
            "quoting::splits_packed_flags"
        ])
    );
}

#[test]
fn a_line_another_thread_printed_into_libtests_list_hides_no_later_failure() {
    let stray = "Chrome cleanup /tmp/symphony-chrome-501/20111-0/3: its group: Not a directory (os error 20)";
    for (from, to, case) in [
        (
            "failures:\n    discovers_its_card\n".to_string(),
            format!("failures:\n{stray}\n    discovers_its_card\n"),
            "between the list's `failures:` and its first name",
        ),
        (
            "    quoting::forwards_each_word\n    quoting::splits_packed_flags\n".to_string(),
            format!("    quoting::forwards_each_word\n{stray}\n    quoting::splits_packed_flags\n"),
            "between two names",
        ),
        (
            "failures:\n    discovers_its_card\n".to_string(),
            format!("failures:\n    discovers_its{stray}\n_card\n"),
            "inside the list's one name, which no name then ends: its block is read again",
        ),
    ] {
        let log = three_targets_with(&from, &to);
        assert_eq!(
            quoted_lines(&log).unwrap(),
            three_named_without(&[]),
            "{case}"
        );
    }
}

#[test]
fn a_quote_is_fenced_longer_than_any_run_of_backticks_in_it() {
    assert_eq!(fenced("a"), "```\na\n```");
    assert_eq!(fenced("a `b` ``c``"), "```\na `b` ``c``\n```");
    assert_eq!(fenced("```\nx"), "````\n```\nx\n````");
    assert_eq!(fenced("a ````` b"), "``````\na ````` b\n``````");
    let named = read_in("error: a line of ```\n```\n").quoted().unwrap();
    assert_eq!(
        named,
        "What failed, as its log says it:\n\n````\nerror: a line of ```\n````"
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

#[test]
fn nextest_names_each_failure_and_timeout_with_the_panics_own_location() {
    let text = include_str!("fixtures/tests-nextest-red.log");
    assert_eq!(
        quoted_lines(text).unwrap(),
        [
            "test runner-fixture::runner_fixture fails ... FAILED",
            "    panicked at src/lib.rs:2:22: fixture failure",
            "test runner-fixture::runner_fixture timeout ... TIMED OUT",
        ]
    );
    let tree = Tree::new();
    let log = rerun::read(&tree.write("nextest.log", text));
    assert_eq!(
        rerun::arguments(&log.targets),
        [
            "-p runner-fixture --lib -- fails --exact",
            "-p runner-fixture --lib -- timeout --exact",
        ]
    );
}

#[test]
fn an_annotated_timeout_says_stall_limit_and_observed_load_without_changing_its_rerun() {
    let fixture = include_str!("fixtures/tests-nextest-red.log");
    for (context, recorded) in [
        (
            serde_json::json!({"limit_seconds":2610,"load":"124.30, 142.61, 111.32 on 16 processors"}),
            true,
        ),
        (serde_json::json!({"limit_seconds":0,"load":"123"}), false),
        (serde_json::json!({"limit_seconds":-1,"load":"123"}), false),
        (serde_json::json!({"limit_seconds":2610,"load":""}), false),
        (
            serde_json::json!({"limit_seconds":"2610","load":"123"}),
            false,
        ),
    ] {
        let text = fixture
            .lines()
            .map(|line| {
                let Ok(mut event) = serde_json::from_str::<serde_json::Value>(line) else {
                    return line.to_string();
                };
                if event.get("reason").and_then(|v| v.as_str()) == Some("time limit exceeded") {
                    event["symphony-stall"] = context.clone();
                }
                event.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let quoted = read_in(&text).quoted().unwrap();
        assert_eq!(quoted.contains("taken as a stall:"), recorded, "{quoted}");
        if recorded {
            assert!(
                quoted.contains("limit 2610s; load 124.30, 142.61, 111.32 on 16 processors"),
                "{quoted}"
            );
        }
        let tree = Tree::new();
        let log = rerun::read(&tree.write("log", &text));
        assert_eq!(
            rerun::arguments(&log.targets),
            [
                "-p runner-fixture --lib -- fails --exact",
                "-p runner-fixture --lib -- timeout --exact"
            ]
        );
    }
}

#[test]
fn nextest_escaped_test_output_is_read_only_for_its_own_panic() {
    let start = r#"{"type":"suite","event":"started","nextest":{"crate":"p","test_binary":"case","kind":"test"}}"#;
    let failed = serde_json::json!({"type":"test", "event":"failed", "name":"p::case$own",
        "stdout":"test fake ... FAILED\nerror: test failed, to rerun pass `-p fake --lib`\nthread 'worker' panicked at worker.rs:1:1:\nworker failure\nthread 'own' panicked at own.rs:2:3:\nown failure\n"
    });
    assert_eq!(
        quoted_lines(&format!("{start}\n{failed}\n")).unwrap(),
        [
            "test p::case own ... FAILED",
            "    panicked at own.rs:2:3: own failure",
        ]
    );
}

#[test]
fn nextest_and_cargo_failures_share_one_log_without_losing_either_targets_cases() {
    let nextest = include_str!("fixtures/tests-nextest-red.log");
    let cargo = "     Running tests/shared.rs (.target/debug/deps/shared-123)\ntest shared_case ... FAILED\nerror: test failed, to rerun pass `-p board --test shared`\n";
    for text in [format!("{nextest}{cargo}"), format!("{cargo}{nextest}")] {
        let quoted = read_in(&text).quoted().unwrap();
        assert!(
            quoted.contains("test runner-fixture::runner_fixture timeout ... TIMED OUT"),
            "{quoted}"
        );
        assert!(quoted.contains("test shared_case ... FAILED"), "{quoted}");
        let tree = Tree::new();
        let log = rerun::read(&tree.write("mixed.log", &text));
        let mut args = rerun::arguments(&log.targets);
        args.sort();
        assert_eq!(
            args,
            [
                "-p board --test shared -- shared_case --exact",
                "-p runner-fixture --lib -- fails --exact",
                "-p runner-fixture --lib -- timeout --exact"
            ]
        );
    }
}

#[test]
fn duplicate_nextest_failure_events_are_named_and_rerun_once() {
    let fixture = include_str!("fixtures/tests-nextest-red.log");
    let repeats: String = fixture
        .lines()
        .filter(|line| line.contains("\"event\":\"failed\""))
        .map(|line| format!("{line}\n"))
        .collect();
    let text = format!("{fixture}{repeats}");
    assert_eq!(quoted_lines(&text), quoted_lines(fixture));
    let tree = Tree::new();
    let log = rerun::read(&tree.write("duplicate.log", &text));
    assert_eq!(rerun::arguments(&log.targets).len(), 2);
}

#[test]
fn a_native_failure_printing_nextest_fixture_events_reruns_only_its_own_case() {
    let fixture = include_str!("fixtures/tests-nextest-red.log");
    let text = format!(
        "     Running tests/native.rs (.target/debug/deps/native-1)\ntest native_case ... FAILED\nfailures:\n\n---- native_case stdout ----\n{fixture}\nthread 'native_case' panicked at native.rs:1:2:\nouter failure\n\nfailures:\n    native_case\n\ntest result: FAILED. 0 passed; 1 failed\nerror: test failed, to rerun pass `-p p --test native`\n"
    );
    assert_eq!(
        quoted_lines(&text).unwrap(),
        [
            "test native_case ... FAILED",
            "    panicked at native.rs:1:2: outer failure",
            "error: test failed, to rerun pass `-p p --test native`",
        ]
    );
    let tree = Tree::new();
    let log = rerun::read(&tree.write("captured.log", &text));
    assert_eq!(
        rerun::arguments(&log.targets),
        ["-p p --test native -- native_case --exact"]
    );
}

#[test]
fn nextest_after_a_native_binary_died_mid_block_is_recovered_at_the_logs_end() {
    let fixture = include_str!("fixtures/tests-nextest-red.log");
    let text = format!(
        "     Running tests/native.rs (.target/debug/deps/native-1)\ntest native_case ... FAILED\n---- native_case stdout ----\nthread 'native_case' panicked at native.rs:1:2:\nouter failure\nerror: test failed, to rerun pass `-p p --test native`\n{fixture}"
    );
    let tree = Tree::new();
    let log = rerun::read(&tree.write("cut-block.log", &text));
    assert_eq!(
        rerun::arguments(&log.targets),
        [
            "-p p --test native -- native_case --exact",
            "-p runner-fixture --lib -- fails --exact",
            "-p runner-fixture --lib -- timeout --exact",
        ]
    );
    let quote = read_in(&text).quoted().unwrap();
    assert!(quote.contains("test native_case ... FAILED"), "{quote}");
    assert!(
        quote.contains("test runner-fixture::runner_fixture timeout ... TIMED OUT"),
        "{quote}"
    );
}
