//! One hand of a notice to its way's program (docs/engineering/NOTICES.md, "The hand-off"): the
//! program started with no argument in a process group of its own, the message written on its
//! stdin and its stdin closed, its end waited for as the kernel's event, at most its bound
//! ([`MOST`] in the server). A program that has not ended by then is ended, and so is whatever of
//! its group still runs once it ends, before it is reaped: the leader not yet reaped holds the
//! group's id, so no other process's group is signalled for it. The server's stop ends a hand under
//! way the same way ([`Running::stop`]).
//!
//! What the program says on stderr is kept, with every hidden word (the address, the link)
//! replaced, so nothing a log line repeats of it holds either. It is heard until the pipe closes,
//! and at most its bound ([`HEARD_AFTER`] in the server) once the program and its group have ended:
//! a process that left the group and holds the pipe never holds the hand.

use std::io::{self, Read, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use symphony::notices::{Came, came, unstarted};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::Stdio;

/// The longest a program runs for one notice before it is ended: the plugins bound their own
/// requests well within it.
pub(crate) const MOST: Duration = Duration::from_secs(60);

/// The longest what a program said is still heard for once it and its group have ended: what it
/// wrote is in the pipe already, so only a process that left the group and holds the pipe is
/// waited for, and not past this.
pub(crate) const HEARD_AFTER: Duration = Duration::from_secs(5);

/// How long a hand waits: for its program's end, and for what it said once it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bounds {
    pub ends: Duration,
    pub heard: Duration,
}

impl Bounds {
    /// The server's own: [`MOST`] and [`HEARD_AFTER`].
    pub(crate) const SERVER: Bounds = Bounds {
        ends: MOST,
        heard: HEARD_AFTER,
    };
}

/// The most of a program's stderr kept: a program that says more is still read to its end.
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
/// `ends_within` waits for the process of the pid it is given to end within the bound it is given,
/// `bounds.ends`, answering whether it did (in the server, [`symphony_board::sys::gone`]), and what
/// it said is heard at most `bounds.heard` once it and its group ended ([`Bounds::SERVER`] in the
/// server).
pub(crate) fn hand(
    program: &Path,
    line: &str,
    hidden: &[&str],
    running: &Running,
    ends_within: &dyn Fn(u32, Duration) -> io::Result<bool>,
    bounds: Bounds,
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
    let heard = child.stderr.take().map(Heard::listen);
    let ended = ends_within(pid, bounds.ends);
    {
        // Ended, not ended within its bound, or its end not watched: the leader is not reaped yet,
        // so what of its group still runs ends now, and the wait below is never unbounded.
        let mut group = running.group.lock().recover();
        symphony_board::sys::end_group(pid, true);
        *group = None;
    }
    let status = child.wait();
    let said = heard
        .map(|heard| heard.within(bounds.heard))
        .unwrap_or_default();
    let said = hidden_in(&said, hidden);
    if running.stopping() {
        return Handed::Cut;
    }
    match (ended, status) {
        (Ok(false), _) => Handed::Later(format!(
            "it did not end within {} s, so it and its group were ended{}",
            bounds.ends.as_secs(),
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

/// What a program says on a pipe, read on a thread of its own as it says it, so it is never held on
/// a full pipe.
struct Heard {
    kept: Arc<Mutex<Vec<u8>>>,
    /// Sent once the pipe closed and all of it was read.
    closed: mpsc::Receiver<()>,
}

impl Heard {
    /// Read `pipe` to its end, its first [`KEPT`] bytes kept.
    fn listen(mut pipe: impl Read + Send + 'static) -> Heard {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (closes, closed) = mpsc::channel();
        let keeping = Arc::clone(&kept);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        let mut kept = keeping.lock().recover();
                        let room = KEPT.saturating_sub(kept.len()).min(read);
                        kept.extend_from_slice(&chunk[..room]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = closes.send(());
        });
        Heard { kept, closed }
    }

    /// What it heard so far.
    fn so_far(&self) -> Vec<u8> {
        self.kept.lock().recover().clone()
    }

    /// What it heard once the pipe closed, or once `bound` passed with it open still: a deadline
    /// on the wait for the pipe's end, which leaves the reader to end with it.
    fn within(self, bound: Duration) -> Vec<u8> {
        let _ = self.closed.recv_timeout(bound);
        self.so_far()
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
