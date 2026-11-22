//! What a run's log says of its tests (docs/workflow/RUNS.md, *Tests*), read once the run ended ([`read`]):
//! each test binary cargo ran, with its counts and time, and the tests that failed, each with the
//! first line of its failure. Stable libtest writes no machine-readable report (`--format json` is
//! nightly's), so these are its words and cargo's:
//!
//! - `     Running unittests src/lib.rs (…/deps/NAME-HASH)`, `     Running tests/a.rs (…)` and
//!   `   Doc-tests NAME`: cargo starting a binary, which is named by them, the hash cut ([`header`]);
//! - `test result: ok. 8 passed; 0 failed; 1 ignored; …; finished in 0.41s`: its counts and time,
//!   added up where it says several (merged doc-tests say two), and doc-tests' time as `all
//!   doctests ran in 0.76s` says it, which counts their build; a sum stays at its most, a count at
//!   `u32::MAX` and a time at the largest finite seconds, so its line always reads back;
//! - `failures:`, once the binary said a line ending in `FAILED` (on its own after a serial
//!   test's uncaptured output, or `test NAME ... FAILED`), which starts
//!   libtest's report of the failed tests: each one's output under `---- NAME stdout ----`, then
//!   `failures:` again and their names. A failure's first line is the first of what its panic
//!   said, else the first its output holds.
//!
//! A binary with no result line before the next one starts or the log ends (it crashed, or its run
//! was stopped) is a suite with an error, never a pass. A failed test's output is what the test
//! printed, so nothing in it starts a binary or ends one, and a report a test printed there ends
//! nothing: the list that ends the real one names the test whose output comes first. Everything
//! else is passed over.

use super::regular;
use std::fs::OpenOptions;
use std::io::{BufRead as _, BufReader};
use std::path::Path;
use symphony_api::{FailedTest, Suite};

/// How many characters of a failure's line are kept.
const WIDTH: usize = 240;

/// A log's tests: its binaries, and the tests that failed in them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tests {
    pub suites: Vec<Suite>,
    pub failed: Vec<FailedTest>,
}

/// What the log at `log` says of its tests: none when it is no regular file, cannot be read, names
/// no test binary, or says a result before cargo started any (its stdout first, then the stderr
/// where cargo names them, as two files make it). It is read through no link and never waits on a
/// FIFO.
pub fn read(log: &Path) -> Option<Tests> {
    let file = regular(log, OpenOptions::new().read(true)).ok()?;
    let mut reader = BufReader::new(file);
    let mut reading = Reading::default();
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
        let line = String::from_utf8_lossy(&bytes);
        reading.line(line.trim_end_matches(['\n', '\r']));
    }
    reading.close();
    (!reading.unordered && !reading.tests.suites.is_empty()).then_some(reading.tests)
}

/// Where the log is in a binary's output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum At {
    /// Its tests running, or between its results.
    #[default]
    Tests,
    /// libtest's report of its failed tests, their output.
    Report,
    /// The report's list of the failed tests' names.
    Listing,
}

/// A failed test's output in the report: its name, and the first line of what its panic said and
/// of what it holds.
#[derive(Debug)]
struct Block {
    name: String,
    panic: Option<String>,
    first: Option<String>,
    /// Whether the line before was its panic's, whose next is what it said.
    panicked: bool,
}

/// The binary cargo started last.
#[derive(Debug)]
struct Binary {
    suite: Suite,
    results: u32,
    /// What doc-tests say they took in all.
    doctests: Option<f64>,
    /// Whether it said a test `FAILED` since its last report: else a `failures:` line is what a
    /// test printed where libtest captured none of it.
    failing: bool,
    at: At,
    blocks: Vec<Block>,
    listed: Vec<String>,
}

#[derive(Debug, Default)]
struct Reading {
    tests: Tests,
    open: Option<Binary>,
    /// Whether a result came before any binary started.
    unordered: bool,
}

impl Reading {
    fn line(&mut self, line: &str) {
        if let Some(binary) = &mut self.open
            && binary.at != At::Tests
        {
            if let Some(failed) = binary.report(line) {
                self.tests.failed.extend(failed);
            }
            return;
        }
        if let Some(name) = header(line) {
            self.close();
            self.open = Some(Binary::new(name));
            return;
        }
        let Some(binary) = &mut self.open else {
            // Results before cargo started any binary: a log whose stdout came first, then its
            // stderr, where cargo names them.
            self.unordered |= result(line).is_some();
            return;
        };
        if line.ends_with("FAILED") {
            binary.failing = true;
        } else if line == "failures:" && binary.failing {
            binary.at = At::Report;
        } else if let Some(said) = result(line) {
            binary.said(said);
        } else if let Some(s) = line
            .strip_prefix("all doctests ran in ")
            .and_then(|rest| seconds(rest.split(';').next()?))
        {
            binary.doctests = Some(s);
        }
    }

    /// The binary cargo started last, ended: kept, with an error when it said no result; one of a
    /// name kept before adds to that one.
    fn close(&mut self) {
        let Some(binary) = self.open.take() else {
            return;
        };
        let mut suite = binary.suite;
        suite.error = binary.results == 0;
        if let Some(s) = binary.doctests {
            suite.s = s;
        }
        match (self.tests.suites.iter_mut()).find(|kept| kept.name == suite.name) {
            Some(kept) => {
                add(kept, (suite.passed, suite.failed, suite.ignored, suite.s));
                kept.error |= suite.error;
            }
            None => self.tests.suites.push(suite),
        }
    }
}

/// `said` added to `suite`, each sum held at its most: a count at `u32::MAX`, the time at the
/// largest finite seconds (both times are finite and none negative), so its line reads back.
fn add(suite: &mut Suite, (passed, failed, ignored, s): Said) {
    suite.passed = suite.passed.saturating_add(passed);
    suite.failed = suite.failed.saturating_add(failed);
    suite.ignored = suite.ignored.saturating_add(ignored);
    suite.s = (suite.s + s).min(f64::MAX);
}

impl Binary {
    fn new(name: String) -> Binary {
        Binary {
            suite: Suite {
                name,
                passed: 0,
                failed: 0,
                ignored: 0,
                s: 0.0,
                error: false,
            },
            results: 0,
            doctests: None,
            failing: false,
            at: At::Tests,
            blocks: Vec::new(),
            listed: Vec::new(),
        }
    }

    /// A result line's counts and time, added ([`add`]).
    fn said(&mut self, said: Said) {
        self.results = self.results.saturating_add(1);
        add(&mut self.suite, said);
    }

    /// A line of its report of failed tests: the tests it names failed once its list ends on its
    /// result line.
    fn report(&mut self, line: &str) -> Option<Vec<FailedTest>> {
        if let Some(name) = block(line) {
            self.at = At::Report;
            self.blocks.push(Block {
                name: name.to_string(),
                panic: None,
                first: None,
                panicked: false,
            });
            return None;
        }
        if self.at == At::Listing {
            if let Some(name) = line.strip_prefix("    ")
                && !name.is_empty()
                && !name.starts_with(' ')
            {
                self.listed.push(name.to_string());
                return None;
            }
            if line.is_empty() {
                return None;
            }
            // The list that ends the report names the test whose output it holds first; one that
            // test printed, or one after it, does not.
            if let Some(said) = result(line)
                && (self.blocks.first()).is_none_or(|first| self.listed.contains(&first.name))
            {
                self.said(said);
                (self.at, self.failing) = (At::Tests, false);
                return Some(self.failed());
            }
            self.at = At::Report;
            self.listed.clear();
        }
        if line == "failures:" {
            self.at = At::Listing;
            self.listed.clear();
        } else if let Some(block) = self.blocks.last_mut() {
            block.holds(line);
        }
        None
    }

    /// The tests its report listed, each with its failure's line, and the report left.
    fn failed(&mut self) -> Vec<FailedTest> {
        let blocks = std::mem::take(&mut self.blocks);
        (std::mem::take(&mut self.listed).into_iter())
            .map(|name| {
                let block = blocks.iter().find(|block| block.name == name);
                let line = block.and_then(|block| block.panic.as_ref().or(block.first.as_ref()));
                FailedTest {
                    suite: Some(self.suite.name.clone()),
                    line: line.map_or_else(String::new, |line| cut(line)),
                    name,
                }
            })
            .collect()
    }
}

impl Block {
    /// A line of the test's output: the first of what its first panic said, else where it
    /// panicked, and its first line.
    fn holds(&mut self, line: &str) {
        let text = line.trim();
        if std::mem::take(&mut self.panicked) {
            if text.is_empty() {
                self.panicked = true;
                return;
            }
            // A panic that said nothing is followed by libtest's note: where it panicked stands.
            if !text.starts_with("note: run with `RUST_BACKTRACE") {
                self.panic = Some(text.to_string());
                return;
            }
        }
        if self.panic.is_none()
            && let Some(at) = panicked(line)
        {
            // `thread 'NAME' panicked at FILE:LINE:COL:`, what it said on the lines after.
            self.panic = Some(format!("panicked at {}", at.trim_end_matches(':')));
            self.panicked = at.ends_with(':');
            return;
        }
        if self.first.is_none() && !text.is_empty() {
            self.first = Some(text.to_string());
        }
    }
}

/// The binary cargo starts with `line`, named by its words, the executable's folder and hash cut:
/// `unittests src/lib.rs (symphony_board)` of `     Running unittests src/lib.rs
/// (.target/debug/deps/symphony_board-c55f883224b5f683)`, `Doc-tests symphony_board` of
/// `   Doc-tests symphony_board`. Only cargo's own line, its word right-aligned in twelve columns.
fn header(line: &str) -> Option<String> {
    if let Some(name) = line.strip_prefix("   Doc-tests ") {
        return (!name.is_empty() && !name.contains(' ')).then(|| format!("Doc-tests {name}"));
    }
    let rest = line.strip_prefix("     Running ")?;
    let (what, executable) = rest.strip_suffix(')')?.rsplit_once(" (")?;
    let file = executable.rsplit('/').next()?;
    let file = match file.rsplit_once('-') {
        Some((stem, hash))
            if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            stem
        }
        _ => file,
    };
    (!what.is_empty() && !what.starts_with('`') && !file.is_empty())
        .then(|| format!("{what} ({file})"))
}

/// What a result line says: passed, failed, ignored and its seconds.
type Said = (u32, u32, u32, f64);

/// The counts and time of libtest's `test result: ok. 8 passed; 0 failed; 1 ignored; 0 measured; 0
/// filtered out; finished in 0.41s`.
fn result(line: &str) -> Option<Said> {
    let (_, counts) = line.strip_prefix("test result: ")?.split_once(". ")?;
    let mut said: Said = (0, 0, 0, 0.0);
    let mut known = 0;
    for part in counts.split("; ") {
        let count = |word: &str| part.strip_suffix(word)?.parse::<u32>().ok();
        if let Some(n) = count(" passed") {
            (said.0, known) = (n, known + 1);
        } else if let Some(n) = count(" failed") {
            (said.1, known) = (n, known + 1);
        } else if let Some(n) = count(" ignored") {
            said.2 = n;
        } else if let Some(s) = part.strip_prefix("finished in ").and_then(seconds) {
            said.3 = s;
        }
    }
    (known == 2).then_some(said)
}

/// The seconds of `0.41s`.
fn seconds(text: &str) -> Option<f64> {
    let s: f64 = text.trim().strip_suffix('s')?.parse().ok()?;
    (s.is_finite() && s >= 0.0).then_some(s)
}

/// The failed test whose output `line` starts: NAME of `---- NAME stdout ----`.
fn block(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("---- ")?;
    let name = (rest.strip_suffix(" stdout ----")).or_else(|| rest.strip_suffix(" stderr ----"))?;
    (!name.is_empty()).then_some(name)
}

/// Where a panic's line says it panicked: `panicked at …` of `thread 'NAME' panicked at …` (`thread
/// 'NAME' (ID) panicked at …` once threads have ids).
fn panicked(line: &str) -> Option<&str> {
    let (_, rest) = line.strip_prefix("thread '")?.split_once('\'')?;
    let at = rest.find("panicked at ")?;
    Some(rest[at + "panicked at ".len()..].trim_end())
}

/// `line` cut at [`WIDTH`] characters, saying it was.
fn cut(line: &str) -> String {
    match line.char_indices().nth(WIDTH) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests;
