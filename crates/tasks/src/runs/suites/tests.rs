//! The tests' reader over one recorded fixture, `tests/runs/fixtures/cargo-test.log`: what `just
//! test-rs -p symphony-args --no-fail-fast` wrote into a caller's file in this workspace (its
//! checkout's path made `/wt/symphony`), with tests added there for the recording alone: a library
//! with a passing, an ignored and two failing tests and an unused variable (the compiler's warnings
//! before the binaries), an integration binary that aborts in its second test, the package's own
//! green one, and doc-tests, one run and one ignored.

use super::*;
use std::fs;
use std::path::PathBuf;

/// The fixture, which the line's tests in `crate::runs` read too.
pub(crate) const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/runs/fixtures/cargo-test.log"
);
const LIB: &str = "unittests src/lib.rs (symphony_args)";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-runs-suites-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn suite(name: &str, (passed, failed, ignored): (u32, u32, u32), s: f64, error: bool) -> Suite {
    Suite {
        name: name.into(),
        passed,
        failed,
        ignored,
        s,
        error,
    }
}

fn failed(suite: &str, name: &str, line: &str) -> FailedTest {
    FailedTest {
        suite: Some(suite.into()),
        name: name.into(),
        line: line.into(),
    }
}

/// What `text` says of its tests, read as a log in `dir`.
fn read_text(dir: &Path, text: &str) -> Option<Tests> {
    let log = dir.join("run.log");
    fs::write(&log, text).unwrap();
    read(&log)
}

#[test]
fn a_real_cargo_test_gives_each_binary_its_counts_and_time_and_each_failed_test_its_first_line() {
    let tests = read(Path::new(FIXTURE)).unwrap();
    assert_eq!(
        tests.suites,
        [
            suite(LIB, (1, 2, 1), 0.0, false),
            // Aborted in its second test: no result line, so an error and no count.
            suite("tests/aborts.rs (aborts)", (0, 0, 0), 0.0, true),
            suite("tests/arguments.rs (arguments)", (8, 0, 0), 0.0, false),
            // Merged doc-tests say two results, and their time with their build.
            suite("Doc-tests symphony_args", (1, 0, 1), 0.76, false),
        ]
    );
    // In the order libtest lists them; what a test printed before its panic is not its failure.
    assert_eq!(
        tests.failed,
        [
            failed(
                LIB,
                "fixture::compares",
                "assertion `left == right` failed: one and one"
            ),
            failed(LIB, "fixture::panics", "it went wrong"),
        ]
    );
}

#[test]
fn a_binary_cut_off_before_its_result_line_is_an_error_and_what_follows_never_ran() {
    let dir = scratch("cut");
    let text = fs::read_to_string(FIXTURE).unwrap();
    // A run stopped while its last binary ran: cut after one of its tests.
    let running = text
        .find("test malformed_options_keep_their_refusals ... ok\n")
        .unwrap();
    let tests = read_text(&dir, &text[..running]).unwrap();
    let names: Vec<(&str, bool)> = (tests.suites.iter())
        .map(|suite| (suite.name.as_str(), suite.error))
        .collect();
    assert_eq!(
        names,
        [
            (LIB, false),
            ("tests/aborts.rs (aborts)", true),
            ("tests/arguments.rs (arguments)", true)
        ]
    );
    assert_eq!(tests.failed.len(), 2);
    // Stopped in its report of failures: no result, so an error, and no test named failed.
    let report = text.find("---- fixture::compares stdout ----").unwrap();
    let tests = read_text(&dir, &text[..report]).unwrap();
    assert_eq!(tests.suites, [suite(LIB, (0, 0, 0), 0.0, true)]);
    assert_eq!(tests.failed, []);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_log_naming_no_test_binary_or_that_cannot_be_read_says_nothing() {
    let dir = scratch("none");
    let text = fs::read_to_string(FIXTURE).unwrap();
    // A build's output alone: its warnings and cargo's progress, a test's words outside any binary.
    let build = &text[..text.find("     Running").unwrap()];
    let words = "test result: ok. 1 passed; 0 failed; 0 ignored; finished in 1.00s\nfailures:\n";
    assert_eq!(read_text(&dir, &format!("{build}{words}")), None);
    assert_eq!(read_text(&dir, ""), None);
    // A result before cargo started any binary: stdout's lines first, as two files leave them, so
    // no binary is named by its own.
    assert_eq!(read_text(&dir, &format!("{words}{text}")), None);
    assert_eq!(read(&dir.join("missing.log")), None);
    // Never through a link.
    let link = dir.join("link.log");
    std::os::unix::fs::symlink(FIXTURE, &link).unwrap();
    assert_eq!(read(&link), None);
    assert!(read(Path::new(FIXTURE)).is_some());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn what_a_failed_test_printed_starts_and_ends_no_binary_and_one_that_printed_nothing_still_fails() {
    let dir = scratch("printed");
    // `a`'s panic quotes a whole run a command of its said, report and all; `b` failed printing
    // nothing, and `c`'s panic said nothing.
    let log = "     Running tests/x.rs (.target/debug/deps/x-0123456789abcdef)

running 4 tests
test a ... FAILED
test b ... FAILED
test c ... FAILED
test d ... ok

failures:

---- a stdout ----

thread 'a' (1234) panicked at tests/x.rs:2:2:
the run said:
     Running tests/inner.rs (.target/debug/deps/inner-0123456789abcdef)

running 1 test
test inner ... FAILED

failures:

---- inner stdout ----
thread 'inner' panicked at tests/inner.rs:1:1:
not a's

failures:
    inner

test result: FAILED. 0 passed; 1 failed; 0 ignored; finished in 9.00s

note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- c stdout ----

thread 'c' (1235) panicked at tests/x.rs:3:3:

note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    a
    b
    c

test result: FAILED. 1 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.25s

     Running tests/x.rs (.target/other/deps/x-fedcba9876543210)

running 1 test
test e ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.50s
";
    let tests = read_text(&dir, log).unwrap();
    // Two binaries of one name (two packages' `tests/x.rs`) are one suite.
    assert_eq!(
        tests.suites,
        [suite("tests/x.rs (x)", (2, 3, 0), 0.75, false)]
    );
    let x = "tests/x.rs (x)";
    assert_eq!(
        tests.failed,
        [
            failed(x, "a", "the run said:"),
            failed(x, "b", ""),
            failed(x, "c", "panicked at tests/x.rs:3:3"),
        ]
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// A log of `binaries` binaries all named `tests/x.rs (x)`, each saying `results` as its result
/// lines.
fn binaries_saying(binaries: usize, results: &[&str]) -> String {
    let binary = format!(
        "     Running tests/x.rs (.target/debug/deps/x-0123456789abcdef)\n\n{}",
        results
            .iter()
            .map(|result| format!("test result: ok. {result}\n"))
            .collect::<String>()
    );
    binary.repeat(binaries)
}

#[test]
fn counts_past_the_most_a_count_holds_stay_at_it_and_never_panic() {
    let dir = scratch("counts");
    let most = "4294967295 passed; 4294967295 failed; 4294967295 ignored; finished in 1.00s";
    // Added up in one binary's results, and over two binaries of one name.
    for log in [
        binaries_saying(1, &[most, most]),
        binaries_saying(2, &[most]),
    ] {
        let tests = read_text(&dir, &log).unwrap();
        assert_eq!(
            tests.suites,
            [suite(
                "tests/x.rs (x)",
                (u32::MAX, u32::MAX, u32::MAX),
                2.0,
                false
            )],
            "{log}"
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_time_past_the_largest_finite_seconds_stays_at_it_and_its_line_reads_back() {
    let dir = scratch("time");
    let long = "1 passed; 0 failed; finished in 1e308s";
    for log in [
        binaries_saying(1, &[long, long]),
        binaries_saying(2, &[long]),
    ] {
        let tests = read_text(&dir, &log).unwrap();
        assert_eq!(
            tests.suites,
            [suite("tests/x.rs (x)", (2, 0, 0), f64::MAX, false)],
            "{log}"
        );
        // Infinity would be written as `null`, which no reader reads back as a time.
        let text = serde_json::to_string(&tests.suites).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<Suite>>(&text).unwrap(),
            tests.suites
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failures_line_a_passing_binary_printed_starts_no_report() {
    let dir = scratch("stray");
    // A child's output that libtest did not capture, in a binary in which nothing failed.
    let log = "     Running tests/x.rs (.target/debug/deps/x-0123456789abcdef)

running 1 test
failures:
    printed
test a ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.50s

     Running tests/y.rs (.target/debug/deps/y-0123456789abcdef)

running 1 test
test b ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.25s
";
    let tests = read_text(&dir, log).unwrap();
    assert_eq!(
        tests.suites,
        [
            suite("tests/x.rs (x)", (1, 0, 0), 0.5, false),
            suite("tests/y.rs (y)", (1, 0, 0), 0.25, false),
        ]
    );
    assert_eq!(tests.failed, []);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_binary_is_named_by_cargos_own_line_alone_its_hash_cut() {
    for (line, name) in [
        (
            "     Running unittests src/main.rs (.target/debug/deps/tasks-c55f883224b5f683)",
            Some("unittests src/main.rs (tasks)"),
        ),
        (
            "     Running benches/b.rs (target/release/deps/b-c55f883224b5f683)",
            Some("benches/b.rs (b)"),
        ),
        // No hash of cargo's: the file as it is.
        (
            "     Running tests/a.rs (deps/a-b-c)",
            Some("tests/a.rs (a-b-c)"),
        ),
        (
            "   Doc-tests symphony_board",
            Some("Doc-tests symphony_board"),
        ),
        // Cargo's verbose line, a line a test printed, and cargo's other words.
        ("     Running `/t/deps/a-c55f883224b5f683`", None),
        ("Running tests/a.rs (deps/a-c55f883224b5f683)", None),
        ("   Compiling tasks v0.1.0 (/wt/crates/tasks)", None),
        ("   Doc-tests ", None),
    ] {
        assert_eq!(header(line).as_deref(), name, "{line}");
    }
    assert_eq!(
        result("test result: FAILED. 1 passed; 2 failed; 3 ignored; finished in 4.5s"),
        Some((1, 2, 3, 4.5))
    );
    assert_eq!(result("test result: ok. 1 passed"), None, "no failed count");
    assert_eq!(cut(&"x".repeat(WIDTH)), "x".repeat(WIDTH));
    assert_eq!(
        cut(&"é".repeat(WIDTH + 1)),
        format!("{}…", "é".repeat(WIDTH))
    );
}
