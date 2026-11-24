//! One hand of a notice to its way's program (docs/engineering/NOTICES.md, "The hand-off"): the
//! program started with no argument in a process group of its own, the message written on its
//! stdin and its stdin closed, its end waited for as the kernel's event, at most [`MOST`]. A program
//! that has not ended by then is ended, and so is whatever of its group still runs once it ends,
//! before it is reaped: the leader not yet reaped holds the group's id, so no other process's group
//! is signalled for it. The server's stop ends a hand under way the same way ([`Running::stop`]).
//!
//! What the program says on stderr is kept, with every hidden word (the address, the link)
//! replaced, so nothing a log line repeats of it holds either.

use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use symphony::notices::{Came, came, unstarted};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::Stdio;

/// The longest a program runs for one notice before it is ended: the plugins bound their own
/// requests well within it.
pub(crate) const MOST: Duration = Duration::from_secs(60);

/// The most of a program's stderr read: a program that says more is still read to its end.
const KEPT: usize = 64 * 1024;
/// The most of what it said that a line repeats.
const SAID: usize = 300;
/// What a hidden word is said as.
const HIDDEN: &str = "<hidden>";

/// What came of one hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Handed {
    Sent,
    /// Not sent this time, worth handing again, and why.
    Later(String),
    /// Not sent, handing again will not send it, and why.
    Never(String),
    /// The server's stop came first: what came of it is not known.
    Cut,
}

/// What a hand and the server's stop share: the group of the program running, and whether the
/// server stops.
#[derive(Default)]
pub(crate) struct Running {
    group: Mutex<Option<u32>>,
    stopping: AtomicBool,
}

impl Running {
    /// The server stops: no program starts any more, and the one running is ended with its group.
    pub(crate) fn stop(&self) {
        let group = self.group.lock().recover();
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(group) = *group {
            symphony_board::sys::end_group(group, true);
        }
    }

    /// Whether the server stops.
    pub(crate) fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
}

/// Hands `line` to `program`, the words `hidden` never repeated of what it says: [`Handed`].
/// `ends_within` waits for the process of the pid it is given to end, answering whether it ended
/// within its bound (in the server, [`symphony_board::sys::gone`] for [`MOST`]).
pub(crate) fn hand(
    program: &Path,
    line: &str,
    hidden: &[&str],
    running: &Running,
    ends_within: &dyn Fn(u32) -> io::Result<bool>,
) -> Handed {
    let mut command = symphony::children::command(program);
    command
        .process_group(0)
        .stdin(Stdio::Piped)
        .stdout(Stdio::Null)
        .stderr(Stdio::Piped);
    let mut child = {
        let mut group = running.group.lock().recover();
        if running.stopping() {
            return Handed::Cut;
        }
        match command.spawn() {
            Ok(child) => {
                *group = Some(child.id());
                child
            }
            Err(error) => {
                let why = format!("its program did not start: {error}");
                return match unstarted(&error) {
                    Came::Later => Handed::Later(why),
                    Came::Sent | Came::Never => Handed::Never(why),
                };
            }
        }
    };
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        // A program that ends without reading its stdin breaks the pipe: its status says what came
        // of the notice.
        let _ = stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"));
    }
    let stderr = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut stderr = stderr;
            let _ = (&mut stderr).take(KEPT as u64).read_to_end(&mut kept);
            // Read to its end, so a program that says more is never held on a full pipe.
            let _ = io::copy(&mut stderr, &mut io::sink());
            kept
        })
    });
    let ended = ends_within(pid);
    {
        // Ended, not ended within its bound, or its end not watched: the leader is not reaped yet,
        // so what of its group still runs ends now, and the wait below is never unbounded.
        let mut group = running.group.lock().recover();
        symphony_board::sys::end_group(pid, true);
        *group = None;
    }
    let status = child.wait();
    let said = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let said = hidden_in(&said, hidden);
    if running.stopping() {
        return Handed::Cut;
    }
    match (ended, status) {
        (Ok(false), _) => Handed::Later(format!(
            "it did not end within {} s, so it and its group were ended{}",
            MOST.as_secs(),
            colon(&said)
        )),
        (Err(error), _) => Handed::Later(format!(
            "its end could not be watched ({error}), so it and its group were ended{}",
            colon(&said)
        )),
        (_, Err(error)) => Handed::Later(format!("its end could not be read: {error}")),
        (_, Ok(status)) => {
            let code = status.code();
            match came(code) {
                Came::Sent => Handed::Sent,
                Came::Later => Handed::Later(nonempty(said)),
                Came::Never => Handed::Never(match code {
                    Some(code) => format!("exit {code}{}", colon(&said)),
                    None => format!("it ended with no exit status{}", colon(&said)),
                }),
            }
        }
    }
}

/// `said`, read as text, each of `hidden` replaced, on one line and cut short.
fn hidden_in(said: &[u8], hidden: &[&str]) -> String {
    let mut text = String::from_utf8_lossy(said).into_owned();
    for word in hidden.iter().filter(|word| !word.is_empty()) {
        text = text.replace(word, HIDDEN);
    }
    let line = (text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    match line.char_indices().nth(SAID) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

/// `said` after a colon, or nothing where it is empty.
fn colon(said: &str) -> String {
    if said.is_empty() {
        String::new()
    } else {
        format!(": {said}")
    }
}

/// `said`, or that the program said nothing.
fn nonempty(said: String) -> String {
    if said.is_empty() {
        "its program said nothing on stderr".into()
    } else {
        said
    }
}

#[cfg(test)]
#[path = "hand_tests.rs"]
mod tests;
