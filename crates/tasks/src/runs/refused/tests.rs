//! The refusal reader over what the runners wrote: the two logs the board kept on 2026-11-18 of
//! commands that never ran (`tools/board-pg-lifecycle` at 22fc65d3 and
//! `tests/idle-count-discards-restored` at ecf85d8f, each a red until a new commit), the usage
//! errors of cargo 1.99, cargo-nextest 0.9.152 and just 1.59 as they wrote them, and the reds a
//! refusal must never be taken for. The recorder's own tests run the runners themselves
//! (`tests/cli/recorder.rs`).

use super::*;
use std::fs;

/// `just test-rs -p symphony-app --test hooks own_names owner_words named_recipes`: exit 1 in
/// 0.07 s, its whole log.
const LIFECYCLE: &str = "error: unexpected argument 'owner_words' found

Usage: cargo test [OPTIONS] [TESTNAME] [-- [ARGS]...]

For more information, try '--help'.
";

/// `just test-rs -p symphony-board --test store` with three test names: exit 1, after the line the
/// worktree's first build says of its target folder.
const IDLE_COUNT: &str = "symphony seed: /wt/tests-idle-count-discards-restored/.target/debug \
starts as a clone of /symphony/.target/debug (no disk until a build changes it)
error: unexpected argument 'a_failed_unanswered_scan_delivers_the_rest_and_never_repeats_a_told_post' found

Usage: cargo test [OPTIONS] [TESTNAME] [-- [ARGS]...]

For more information, try '--help'.
";

/// `cargo nextest run --nosuch`: exit 2.
const NEXTEST: &str = "error: unexpected argument '--nosuch' found

  tip: a similar argument exists: '--bench'
  tip: to pass '--nosuch' as a value, use '-- --nosuch'

Usage: cargo nextest run --bench <BENCH> [FILTERS]... [-- <FILTERS_AND_ARGS>...]

For more information, try '--help'.
";

/// `just --nosuch`: exit 2.
const JUST_OPTION: &str = "error: unexpected argument '--nosuch' found

  tip: a similar argument exists: '--no-cache'
  tip: to pass '--nosuch' as a value, use '-- --nosuch'

Usage: just --no-cache [ARGUMENTS]...

For more information, try '--help'.
";

/// `just on`, in a justfile whose one recipe is `one ARG`: exit 1.
const JUST_NEAR: &str = "error: justfile does not contain recipe `on`\nDid you mean `one`?\n";
/// `just nosuch` there: exit 1.
const JUST_NONE: &str = "error: justfile does not contain recipe `nosuch`\n";
/// `just one` there: exit 1.
const JUST_COUNT: &str = "error: recipe `one` got 0 positional arguments but takes 1
usage:
    just one ARG
";

/// `cargo test` of a library that does not compile: exit 101.
const COMPILE_ERROR: &str = "   Compiling c v0.1.0 (/wt/c)
error[E0308]: mismatched types
 --> src/lib.rs:2:5
  |
1 | pub fn one() -> u32 {
  |                 --- expected `u32` because of return type
2 |     \"a\"
  |     ^^^ expected `u32`, found `&str`

For more information about this error, try `rustc --explain E0308`.
error: could not compile `c` (lib) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
error: could not compile `c` (lib test) due to 1 previous error
";

/// `cargo test -- --nosuch`: the test binary was built, and refused the option. Exit 101.
const HARNESS_OPTION: &str = "   Compiling c v0.1.0 (/wt/c)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s
     Running unittests src/lib.rs (t/debug/deps/c-41ac83fb2aa53e09)
error: Unrecognized option: 'nosuch'
error: test failed, to rerun pass `--lib`
";

/// `cargo nextest run` of a failing test, its ends: exit 100.
const NEXTEST_FAILED: &str =
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.00s
────────────
    Starting 1 test across 1 binary
        FAIL [   0.012s] (1/1) c t
  Cancelling due to test failure
────────────
     Summary [   0.015s] 1 test run: 0 passed, 1 failed, 0 skipped
        FAIL [   0.012s] (1/1) c t
error: test run failed
";

#[test]
fn a_usage_error_of_cargo_nextest_or_just_is_a_refusal_named_by_its_error_line() {
    for (log, line) in [
        (LIFECYCLE, "error: unexpected argument 'owner_words' found"),
        (
            IDLE_COUNT,
            "error: unexpected argument \
             'a_failed_unanswered_scan_delivers_the_rest_and_never_repeats_a_told_post' found",
        ),
        (NEXTEST, "error: unexpected argument '--nosuch' found"),
        (JUST_OPTION, "error: unexpected argument '--nosuch' found"),
        (JUST_NEAR, "error: justfile does not contain recipe `on`"),
        (
            JUST_NONE,
            "error: justfile does not contain recipe `nosuch`",
        ),
        (
            JUST_COUNT,
            "error: recipe `one` got 0 positional arguments but takes 1",
        ),
    ] {
        assert_eq!(said(log).as_deref(), Some(line), "{log}");
        // As a terminal's line ends say it, and with nothing after its last line.
        assert_eq!(said(&log.replace('\n', "\r\n")).as_deref(), Some(line));
        assert_eq!(said(log.trim_end()).as_deref(), Some(line));
    }
}

#[test]
fn a_compile_error_a_failing_test_and_a_refused_test_binary_are_no_refusal() {
    for log in [COMPILE_ERROR, HARNESS_OPTION, NEXTEST_FAILED] {
        assert_eq!(said(log), None, "{log}");
    }
    // A real `cargo test` with failing tests and a crashed binary (`suites::tests`).
    let failing = fs::read_to_string(crate::runs::suites::tests::FIXTURE).unwrap();
    assert!(failing.contains("test result: FAILED"));
    assert_eq!(said(&failing), None);
    // Other errors that end a log: none is a refusal of the command line.
    for log in [
        "",
        "error: test failed, to rerun pass `--lib`\n",
        "error: no test target named `nosuch` in default-run packages\n",
        "error: package ID specification `nosuch` did not match any packages\n",
        "error: could not find `Cargo.toml` in `/wt` or any parent directory\n",
        "error: No justfile found\n",
        "error: recipe `test-rs` failed on line 49 with exit code 1\n",
        "assertion `left == right` failed\n",
        "Usage: cargo test [OPTIONS]\n\nFor more information, try '--help'.\n",
    ] {
        assert_eq!(said(log), None, "{log}");
    }
}

#[test]
fn a_refusal_with_work_said_before_it_or_anything_after_it_is_a_red() {
    // Each line that says its runner built or ran something, before a refusal a later command
    // of the same script made.
    for work in [
        "   Compiling c v0.1.0 (/wt/c)",
        "    Checking c v0.1.0 (/wt/c)",
        "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s",
        "     Running unittests src/lib.rs (t/debug/deps/c-41ac83fb2aa53e09)",
        "   Doc-tests c",
        "    Starting 1 test across 1 binary",
        "        FAIL [   0.012s] (1/1) c t",
        "        PASS [   0.012s] (1/1) c t",
        "running 1 test",
        "running 12 tests",
        "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
    ] {
        assert!(worked(work), "{work}");
        assert_eq!(said(&format!("{work}\n{LIFECYCLE}")), None, "{work}");
        assert_eq!(said(&format!("{work}\n{JUST_NONE}")), None, "{work}");
    }
    // What says no work: a tool's own words, a word short of or past the status column, one that
    // is no word, and `running` with no count of tests.
    for idle in [
        "symphony seed: /wt/.target/debug starts as a clone",
        "Compiling c v0.1.0",
        "       Compiling c v0.1.0",
        "     warning: unused variable",
        "    error[E0 x",
        "   Finished`test`",
        "running the checks",
        "running  tests",
        "tests result: ok",
    ] {
        assert!(!worked(idle), "{idle}");
        assert!(said(&format!("{idle}\n{LIFECYCLE}")).is_some(), "{idle}");
    }
    // Anything after the block, or a block cut short, and it is no refusal: the command went on,
    // or said something else.
    for log in [
        format!("{LIFECYCLE}error: recipe `test-rs` failed on line 49 with exit code 1\n"),
        format!("{LIFECYCLE}test t ... FAILED\n"),
        format!("{JUST_NONE}make: *** [all] Error 1\n"),
        format!("{JUST_NEAR}Did you mean `two`?\n"),
        format!("{JUST_COUNT}    just two ARG\n"),
        LIFECYCLE.replace("For more information, try '--help'.\n", ""),
        LIFECYCLE.replace("Usage: ", "usage: "),
        JUST_COUNT.replace("usage:\n", ""),
        JUST_COUNT.replace(" but takes 1", ""),
        JUST_NEAR.replace("Did you mean", "Do you mean"),
    ] {
        assert_eq!(said(&log), None, "{log}");
    }
    // The last `error:` line is the one read: a real error before a refusal's block is not it.
    assert_eq!(
        said(&format!("error: something else\n{LIFECYCLE}")).as_deref(),
        Some("error: unexpected argument 'owner_words' found")
    );
}

#[test]
fn a_log_is_read_whole_only_when_it_is_a_short_regular_file_and_its_line_is_cut() {
    let dir = std::env::temp_dir().join(format!("symphony-runs-refused-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let log = dir.join("a.log");
    assert_eq!(read(&log), None, "no log");
    fs::write(&log, LIFECYCLE).unwrap();
    assert_eq!(
        read(&log).as_deref(),
        Some("error: unexpected argument 'owner_words' found")
    );
    // Not through a link, and not a folder.
    let link = dir.join("link.log");
    std::os::unix::fs::symlink(&log, &link).unwrap();
    assert_eq!(read(&link), None);
    assert_eq!(read(&dir), None);
    // Bytes that are no UTF-8 before it are read past.
    let mut bytes = b"\xff\xfe seed\n".to_vec();
    bytes.extend_from_slice(LIFECYCLE.as_bytes());
    fs::write(&log, &bytes).unwrap();
    assert!(read(&log).is_some());
    // At most MOST bytes: a longer log did more than refuse a command line.
    let filler = "x".repeat(79) + "\n";
    let most = usize::try_from(MOST).unwrap();
    let padded = |len: usize| {
        let mut text = filler.repeat(len / filler.len() + 1);
        text.truncate(len - LIFECYCLE.len());
        text + "\n" + LIFECYCLE.trim_start_matches('\n')
    };
    fs::write(&log, padded(most - 1)).unwrap();
    assert_eq!(fs::metadata(&log).unwrap().len(), MOST);
    assert!(read(&log).is_some());
    fs::write(&log, padded(most)).unwrap();
    assert_eq!(fs::metadata(&log).unwrap().len(), MOST + 1);
    assert_eq!(read(&log), None);
    // A long argument's line is kept to WIDTH characters, at a character's edge.
    let long = LIFECYCLE.replace("owner_words", &"é".repeat(400));
    let line = said(&long).unwrap();
    assert_eq!(line.chars().count(), WIDTH);
    assert!(line.starts_with("error: unexpected argument 'éé"));
    fs::remove_dir_all(&dir).unwrap();
}
