//! The release run's unit tests' waits on what a child they start leaves (RULES.md
//! `tests/no-test-fails-on-load`): a pid a shell wrote, and the process it names gone, each a wait
//! on its condition whose only time is a wall-clock backstop that only a stall reaches.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::group::Bound;

/// A bound no step a test runs reaches unless it stalls: the backstop of a minute's allowance.
pub(crate) fn far<'a>() -> Bound<'a> {
    Bound::until(std::time::Instant::now() + test_time::backstop(Duration::from_secs(60)))
}

/// The pid in `file` once a shell has written it whole, as `echo $! > FILE` does: a line.
pub(crate) fn written_pid(file: &Path) -> i64 {
    test_time::until(
        format!("a pid written in {}", file.display()),
        test_time::FLOOR,
        || {
            let text = fs::read_to_string(file).ok()?;
            text.strip_suffix('\n')?.trim().parse().ok()
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}"))
}

/// Whether the pid in `file` is written whole yet.
pub(crate) fn pid_written(file: &Path) -> bool {
    fs::read_to_string(file).is_ok_and(|text| text.ends_with('\n'))
}

/// `said` with each run of digits as `N`: the words a message says, however long what it times
/// took on a loaded machine.
pub(crate) fn untimed(said: &str) -> String {
    let mut words = String::with_capacity(said.len());
    let mut chars = said.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            words.push('N');
        } else {
            words.push(c);
        }
    }
    words
}

/// Waits until the process `pid` is gone, reaped by whoever inherited it. Past the backstop it is
/// killed, so a failed test leaves nothing running, and the test fails with the stall.
pub(crate) fn gone(pid: i64, what: &str) {
    let ended = test_time::until_true(
        format!("{what} (process {pid}) gone"),
        test_time::FLOOR,
        || !symphony_board::sys::pid_alive(pid),
    );
    if let Err(stall) = ended {
        symphony_board::sys::terminate(pid, true);
        panic!("{stall}");
    }
}
