//! Whether a run's log says its runner refused the command line and did nothing
//! (docs/workflow/MACHINE_WORK.md, *A command its runner refused*), read once the run ended ([`read`]), as
//! its tests are ([`super::suites`]). Such a run built and ran nothing, so it says nothing about
//! the commit it was typed at: its record keeps the refusal's line ([`recorded::Run::refused`]),
//! and a stamp's reader passes it over, no red and no green (`crate::stamps::read`).
//!
//! It is told by the runner's own words, never by how long the run took or how it exited: a test
//! that fails at once is a red. The log ends in one of these, each a refusal of the words typed,
//! made before the runner read the tree:
//!
//! - clap's usage error, as `cargo test`, `cargo build`, `cargo nextest run` and `just` say it:
//!   `error: …`, any `  tip: …` lines, `Usage: …`, `For more information, try '--help'.`;
//! - just's `error: justfile does not contain recipe …`, alone or with its `Did you mean …?`;
//! - just's `error: recipe … got N arguments but takes M`, with its `usage:` and the recipe's line.
//!
//! and no line of it says work was done ([`worked`]): one of Cargo's or nextest's status lines
//! (`   Compiling`, `    Finished`, `     Running`, `        FAIL`), or libtest's `running N tests`
//! and `test result:`. So a compile error, a failing test, a test binary given an option it does
//! not take (it was built first) and a command that printed such a block and went on are reds, as
//! is every run that keeps no log (its caller at a terminal) or whose log is longer than [`MOST`].
//! A refusal the list does not hold (Cargo's `no test target named`, a package it cannot find)
//! stays a red too: those read the tree.
//!
//! What it cannot see: a command that did its work in silence, or in words no line above knows,
//! and then ended on a refused runner's block reads as refused.
//!
//! [`recorded::Run::refused`]: crate::cli::machine_work::recorded::Run::refused

use super::regular;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::path::Path;

/// The longest log read: a runner that refused its command line wrote a few lines.
pub const MOST: u64 = 64 * 1024;
/// How many characters of the refusal's line are kept.
const WIDTH: usize = 240;
/// The column Cargo's and nextest's status words end at, a space after it.
const STATUS: usize = 12;

/// The refusal the log at `log` ends in, its `error:` line: none when it is no regular file, cannot
/// be read, is longer than [`MOST`] or says anything else ([`said`]). It is read through no link
/// and never waits on a FIFO.
pub fn read(log: &Path) -> Option<String> {
    let file = regular(log, OpenOptions::new().read(true)).ok()?;
    if file.metadata().ok()?.len() > MOST {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MOST + 1).read_to_end(&mut bytes).ok()?;
    said(&String::from_utf8_lossy(&bytes))
}

/// The refusal the log `text` ends in, its `error:` line cut to [`WIDTH`] characters: none unless
/// its last `error:` line starts one of the module's blocks, nothing follows that block, and no
/// line says work was done.
pub fn said(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let at = lines.iter().rposition(|line| line.starts_with("error: "))?;
    let after: Vec<&str> = (lines[at + 1..].iter().copied())
        .filter(|line| !line.trim().is_empty())
        .collect();
    let error = lines[at];
    let refusal = usage(&after) || no_recipe(error, &after) || arguments(error, &after);
    (refusal && !lines.iter().any(|line| worked(line))).then(|| error.chars().take(WIDTH).collect())
}

/// clap's usage error after its `error:` line: its tips, its usage, its pointer to the help.
fn usage(after: &[&str]) -> bool {
    let tips = after.iter().take_while(|line| line.starts_with("  tip: "));
    matches!(
        &after[tips.count()..],
        [usage, "For more information, try '--help'."] if usage.starts_with("Usage: ")
    )
}

/// just's refusal of a recipe its justfile does not hold, with the one it may suggest.
fn no_recipe(error: &str, after: &[&str]) -> bool {
    let said = error.starts_with("error: justfile does not contain recipe `");
    said && match after {
        [] => true,
        [mean] => mean.starts_with("Did you mean `"),
        _ => false,
    }
}

/// just's refusal of a recipe's arguments by their count, with the recipe's usage.
fn arguments(error: &str, after: &[&str]) -> bool {
    error.starts_with("error: recipe `")
        && error.contains("` got ")
        && error.contains(" but takes ")
        && matches!(after, ["usage:", recipe] if recipe.starts_with("    just "))
}

/// Whether `line` says its runner built or ran something: a status line of Cargo's or nextest's (a
/// word of letters and hyphens, its first a capital, ending at column [`STATUS`] with a space
/// after it), or libtest's count of the tests it runs or its result.
fn worked(line: &str) -> bool {
    let status = line.get(..STATUS).is_some_and(|start| {
        let word = start.trim_start_matches(' ');
        line[STATUS..].starts_with(' ')
            && word.starts_with(|c: char| c.is_ascii_uppercase())
            && word.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
    });
    let counted = line.strip_prefix("running ").is_some_and(|rest| {
        let digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        digits.len() < rest.len() && digits.starts_with(" test")
    });
    status || counted || line.starts_with("test result: ")
}

#[cfg(test)]
mod tests;
