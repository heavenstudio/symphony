//! What a red check's log says failed, as its task names it before the log's last lines
//! (docs/RELEASES.md, a red's text), read from the log ([`read`]):
//!
//! - a test run's, a log in which cargo started a test target (`     Running …`, `   Doc-tests …`)
//!   or said one failed, or libtest named a test `FAILED`: each test libtest named `FAILED`, with
//!   where it panicked (its own thread's panic before one of a thread it started) and the first
//!   line of what its panic said, then cargo's list of the targets that failed (`error: N targets
//!   failed:` and its lines; with no list, as when one target failed or the run was cut short, each
//!   target's own `error: test failed, to rerun pass …`). Nothing else of such a log is named, and
//!   nothing of what a failed test printed (its block, `---- NAME stdout ----`) but its panic: the
//!   tests' own output says the errors, refusals and failed tests of the commands they run, as
//!   their cases. A test run that names no failed test or target (one stopped at the run's time,
//!   hung or ended by a signal) names only what [`other`] finds before cargo started its first
//!   target, where a build's errors are;
//! - any other log's: each line with a word that starts `error` or `refus` (any case: `error:`,
//!   `error[E0425]`, `refused`, never `thiserror`, `gix-error` or `std::error`) or the docs audit's
//!   mark `✖`, an error's with the `-->` line a compiler gives its place on, and the step's own last
//!   word, the line it printed last before just said its recipe failed (`client-check: …`), with
//!   each line its tool starts with the same `NAME: ` (`size-budget: …`, so the binary past its line
//!   is among them); never a line of cargo's progress (`   Compiling gix-error v0.3.2`).
//!
//! Each list stops at [`LISTED`] and says how many more the log holds; a line is cut at [`WIDTH`]
//! characters; a terminal's colour codes are taken out. A log that says none of these, or cannot be
//! read, names nothing, and its task quotes its last lines alone.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::rerun;

/// How many failed tests, lines of cargo's list and other lines a red task names at most.
pub(crate) const LISTED: usize = 40;
/// How many characters of a line it quotes.
const WIDTH: usize = 240;

/// A test libtest named `FAILED`, and where it panicked with the first line of what it said, when
/// the log says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Test {
    name: String,
    panic: Option<String>,
    /// Whether the panic is its own thread's, the one libtest names for it, which no panic of a
    /// thread it started replaces.
    own: bool,
}

/// What a red check's log says failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Failures {
    /// A test run's failed tests, in the log's order.
    tests: Vec<Test>,
    /// The lines in which cargo said the targets that failed, as it said them.
    targets: Vec<String>,
    /// Any other log's lines that say what failed, in its order, each once.
    lines: Vec<String>,
}

/// Where the first line of what a panic said goes: on a failed test, or on a panic of a thread
/// outside every test's own block of output.
#[derive(Clone, Copy)]
enum Said {
    Test(usize),
    Thread(usize),
}

/// The state of one reading of a test run's log, and of the step's own last word.
#[derive(Default)]
struct Reading {
    tests: Vec<Test>,
    /// Where the target cargo started last begins in `tests`.
    from: usize,
    /// The failed test whose block of output (`---- NAME stdout ----`) the log is in, and whether
    /// it is past a `failures:` line there, which starts libtest's list of the failed tests that
    /// ends the blocks, or a list a test printed.
    section: Option<usize>,
    ending: bool,
    /// The lines read, and the number of the first in which cargo started a test target, as cargo
    /// says it ([`progress`]), not a line that quotes it.
    count: usize,
    started: Option<usize>,
    /// The panic whose next line is what it said.
    said: Option<Said>,
    /// The panics of the target cargo started last outside every block, by the thread's name.
    threads: Vec<(String, String)>,
    /// Whether the log is in cargo's list, and the list's lines, the last list's.
    listing: bool,
    listed: Vec<String>,
    /// Each target's own line, once.
    each: Vec<String>,
    /// The last line neither blank nor just's, and whether the line before was just's.
    last: Option<String>,
    just: bool,
    /// The line before just's last lines saying a recipe failed.
    verdict: Option<String>,
}

impl Reading {
    fn line(&mut self, line: &str) {
        self.count += 1;
        self.own_word(line);
        if let Some(said) = self.said.take()
            && !line.trim().is_empty()
            && !line.starts_with("note: run with `RUST_BACKTRACE")
        {
            self.say(said, line.trim());
        }
        if self.section.is_some() {
            self.in_block(line);
            return;
        }
        if self.listing {
            let entry = line.trim();
            if line.starts_with(char::is_whitespace) && entry.starts_with('`') {
                self.listed.push(line.to_string());
                return;
            }
            self.listing = false;
        }
        if let Some(name) = rerun::failed_test(line) {
            if self.failed(&name).is_none() {
                self.tests.push(Test {
                    name,
                    panic: None,
                    own: false,
                });
            }
        } else if rerun::target(line).is_some() {
            if !self.each.iter().any(|each| each == line) {
                self.each.push(line.to_string());
            }
        } else if listing(line) {
            self.listing = true;
            self.listed = vec![line.to_string()];
        } else if rerun::header(line).is_some() {
            self.close();
            self.from = self.tests.len();
            if progress(line) {
                self.started.get_or_insert(self.count);
            }
        } else if let Some(name) = section(line) {
            self.section = self.failed(name);
        } else if let Some((thread, panic)) = panicked(line) {
            self.panicked(thread, panic);
        }
    }

    /// A line of a failed test's block of output, which is what the test printed: its panic, the
    /// next failed test's block, or libtest's list of the failed tests that ends the blocks
    /// (`failures:`, then the names, this test's among them), and nothing else, so a run, a test or
    /// a list the test printed is never the log's own.
    fn in_block(&mut self, line: &str) {
        if self.ending {
            match line.strip_prefix("    ") {
                _ if line.trim().is_empty() => return,
                Some(name) if self.failed(name).is_some() => {
                    (self.section, self.ending) = (None, false);
                    return;
                }
                Some(_) => return,
                None => self.ending = false,
            }
        }
        if line == "failures:" {
            self.ending = true;
        } else if let Some(at) = section(line).and_then(|name| self.failed(name)) {
            self.section = Some(at);
        } else if let Some((thread, panic)) = panicked(line) {
            self.panicked(thread, panic);
        }
    }

    /// Where the test `name` is among the failed tests of the target cargo started last.
    fn failed(&self, name: &str) -> Option<usize> {
        (self.tests[self.from..].iter())
            .position(|test| test.name == name)
            .map(|at| self.from + at)
    }

    /// The step's own last word: the line before the last lines in which just said a recipe
    /// failed.
    fn own_word(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        if rerun::recipe(line).is_some() {
            if !self.just {
                self.verdict.clone_from(&self.last);
            }
            self.just = true;
        } else {
            let last = self.last.get_or_insert_with(String::new);
            last.clear();
            last.push_str(line);
            self.just = false;
        }
    }

    /// A panic, `thread` its thread's name: the failed test's whose block the log is in, the first
    /// there or its own thread's, which libtest names for it, before any thread's it started; else
    /// the thread's, which [`Reading::close`] gives the failed test of its name.
    fn panicked(&mut self, thread: &str, panic: &str) {
        if let Some(at) = self.section {
            let test = &mut self.tests[at];
            let own = test.name == thread;
            if test.panic.is_none() || (own && !test.own) {
                (test.panic, test.own) = (Some(panic.to_string()), own);
                self.said = panic.ends_with(':').then_some(Said::Test(at));
            }
        } else if !self.threads.iter().any(|(name, _)| name == thread) {
            self.threads.push((thread.to_string(), panic.to_string()));
            self.said = panic
                .ends_with(':')
                .then_some(Said::Thread(self.threads.len() - 1));
        }
    }

    /// `line`, the first of what a panic said, after where it panicked.
    fn say(&mut self, said: Said, line: &str) {
        let panic = match said {
            Said::Test(at) => self.tests[at].panic.as_mut(),
            Said::Thread(at) => self.threads.get_mut(at).map(|(_, panic)| panic),
        };
        if let Some(panic) = panic {
            panic.push(' ');
            panic.push_str(line);
        }
    }

    /// The end of the target cargo started last: each of its failed tests with no panic of its own
    /// block takes its thread's.
    fn close(&mut self) {
        for test in &mut self.tests[self.from..] {
            if test.panic.is_none() {
                test.panic = (self.threads.iter())
                    .find(|(thread, _)| *thread == test.name)
                    .map(|(_, panic)| panic.clone());
                test.own = test.panic.is_some();
            }
        }
        self.threads.clear();
    }
}

/// What the log at `log` says failed; nothing when it cannot be read or says nothing it knows.
pub(crate) fn read(log: &Path) -> Failures {
    let mut reading = Reading::default();
    if each_line(log, |line| reading.line(line)).is_none() {
        return Failures::default();
    }
    reading.close();
    let targets = if reading.listed.is_empty() {
        reading.each
    } else {
        reading.listed
    };
    if !reading.tests.is_empty() || !targets.is_empty() {
        return Failures {
            tests: reading.tests,
            targets,
            lines: Vec::new(),
        };
    }
    // A test run that names no failure, stopped, hung or ended by a signal: what its build said
    // before cargo started its first target, never what its tests printed nor its last line.
    let lines = match reading.started {
        Some(first) => other(log, None, first),
        None => other(log, reading.verdict.as_deref(), usize::MAX),
    };
    Failures {
        lines,
        ..Failures::default()
    }
}

/// The lines of a log that is no test run's that say what failed, each once, of its lines before
/// the line numbered `before`: an error, a refusal or the mark `✖` ([`says`]), the `-->` line
/// after an error's, the step's own last word `verdict` and each line its tool starts the same way
/// ([`tool`]); never a line of cargo's progress ([`progress`]).
fn other(log: &Path, verdict: Option<&str>, before: usize) -> Vec<String> {
    let tool = verdict.and_then(tool).map(|name| format!("{name}: "));
    let (mut lines, mut seen) = (Vec::new(), HashSet::new());
    let (mut count, mut after_error) = (0, false);
    each_line(log, |line| {
        count += 1;
        if count >= before || progress(line) {
            return;
        }
        let error = says(line, "error");
        let named = error
            || says(line, "refus")
            || line.contains('✖')
            || verdict == Some(line)
            || tool.as_deref().is_some_and(|tool| line.starts_with(tool))
            || (after_error && line.trim_start().starts_with("--> "));
        after_error = error;
        if named && seen.insert(line.to_string()) {
            lines.push(line.to_string());
        }
    });
    lines
}

/// Each line of the log at `log`, read as far as it goes, its bytes lossily, without its line end
/// or a terminal's colour codes; none when it cannot be opened.
fn each_line(log: &Path, mut each: impl FnMut(&str)) -> Option<()> {
    let mut reader = BufReader::new(File::open(log).ok()?);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        each(&plain(text.trim_end_matches(['\n', '\r'])));
    }
    Some(())
}

/// Whether `line` begins cargo's list of the targets that failed: `error: N targets failed:`, `1
/// target failed` for one.
fn listing(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("error: ") else {
        return false;
    };
    let count =
        (rest.strip_suffix(" targets failed:")).or_else(|| rest.strip_suffix(" target failed:"));
    count.is_some_and(|count| !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit()))
}

/// The test whose block of output `line` starts: NAME of `---- NAME stdout ----`.
fn section(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("---- ")?;
    let name = (rest.strip_suffix(" stdout ----")).or_else(|| rest.strip_suffix(" stderr ----"))?;
    (!name.is_empty()).then_some(name)
}

/// The thread a panic's line names, and where it panicked as the line says it: `thread 'NAME'
/// panicked at FILE:LINE:COL:` (`thread 'NAME' (ID) panicked at …` once threads have ids).
fn panicked(line: &str) -> Option<(&str, &str)> {
    let (thread, rest) = line.strip_prefix("thread '")?.split_once('\'')?;
    let at = rest.find("panicked at ")?;
    Some((thread, &rest[at..]))
}

/// Whether `line` holds a word that starts with `stem` (lower case), in any case: `error:`,
/// `error[E0425]` and `Errors` for `error`; never one inside a name, after a letter, a digit, `_`,
/// `-`, `.`, `/` or `:` (`thiserror`, `my_error`, `gix-error`, `--error-format`, `self.error`,
/// `src/error.rs`, `std::error::Error`).
fn says(line: &str, stem: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.match_indices(stem).any(|(at, _)| {
        (lower[..at].chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric() && !"_-./:".contains(c))
    })
}

/// The words cargo says its progress with, right-aligned in its first [`PROGRESS_WIDTH`] columns.
const PROGRESS: [&str; 24] = [
    "Adding",
    "Blocking",
    "Building",
    "Checking",
    "Compiling",
    "Dirty",
    "Doc-tests",
    "Documenting",
    "Downloaded",
    "Downloading",
    "Finished",
    "Fresh",
    "Installed",
    "Installing",
    "Locking",
    "Packaging",
    "Removing",
    "Replacing",
    "Running",
    "Scraping",
    "Unpacking",
    "Updating",
    "Verifying",
    "Waiting",
];
const PROGRESS_WIDTH: usize = 12;

/// Whether cargo said `line` as its progress, one of its words ([`PROGRESS`]) right-aligned in its
/// first [`PROGRESS_WIDTH`] columns, then what it is at: `   Compiling gix-error v0.3.2`,
/// `    Checking …`, `    Finished …`. What it names there is a crate, a profile or a file, never
/// what failed.
fn progress(line: &str) -> bool {
    line.split_at_checked(PROGRESS_WIDTH)
        .is_some_and(|(word, rest)| {
            rest.starts_with(' ') && PROGRESS.contains(&word.trim_start_matches(' '))
        })
}

/// The tool that printed `line`, NAME of `NAME: …`, a word of letters, digits, `-` and `_` from the
/// line's start; none for what a compiler or cargo says (`error`, `warning`, `note`, `help`).
fn tool(line: &str) -> Option<&str> {
    let (name, _) = line.split_once(": ")?;
    let word = !name.is_empty()
        && (name.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let compiler =
        ["error", "warning", "note", "help"].contains(&name.to_ascii_lowercase().as_str());
    (word && !compiler).then_some(name)
}

/// `line` without a terminal's escape sequences: a control sequence (`ESC [`, its parameters, up to
/// its final character) or any other escape and the character after it.
fn plain(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    Cow::Owned(out)
}

/// `line` cut at [`WIDTH`] characters, saying it was.
fn cut(line: &str) -> String {
    match line.char_indices().nth(WIDTH) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

/// `lines` up to [`LISTED`] of them, then a line saying how many more of `what` the log holds.
fn capped<'a>(lines: impl ExactSizeIterator<Item = &'a String>, what: &str) -> Vec<String> {
    let count = lines.len();
    let mut said: Vec<String> = lines.take(LISTED).map(|line| cut(line)).collect();
    if count > LISTED {
        said.push(format!("… and {} more {what} in the log", count - LISTED));
    }
    said
}

impl Failures {
    /// What a red task says of them, a paragraph that quotes them; none when the log said nothing
    /// it knows.
    pub(crate) fn quoted(&self) -> Option<String> {
        let mut said = Vec::new();
        for test in self.tests.iter().take(LISTED) {
            said.push(cut(&format!("test {} ... FAILED", test.name)));
            if let Some(panic) = &test.panic {
                said.push(format!("    {}", cut(panic)));
            }
        }
        if self.tests.len() > LISTED {
            said.push(format!(
                "… and {} more tests named FAILED in the log",
                self.tests.len() - LISTED
            ));
        }
        said.extend(capped(self.targets.iter(), "lines of cargo's list"));
        said.extend(capped(self.lines.iter(), "such lines"));
        if said.is_empty() {
            return None;
        }
        Some(format!(
            "What failed, as its log says it:\n\n{}",
            fenced(&said.join("\n"))
        ))
    }
}

/// `text` as a block of code that no line of it closes: fenced with one backtick more than the
/// longest run of them in it, three at least.
pub(super) fn fenced(text: &str) -> String {
    let longest = (text.split(|c| c != '`')).map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}\n{text}\n{fence}")
}

#[cfg(test)]
#[path = "failures_tests.rs"]
mod tests;
