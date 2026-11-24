//! A child's output as events, each line kept with the time it arrived, so a wait that runs out
//! shows how long each stage the child named took.

use super::{LIMIT, stalled};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;
use symphony_board::sys::Child;

/// Readiness and diagnostics are consumed as events. Both streams stay drained even when the
/// caller is waiting on a build/publication socket instead of a log line.
pub(super) struct Lines {
    receive: mpsc::Receiver<(Instant, String)>,
    began: Instant,
    pub(super) history: Vec<String>,
    /// When each line of `history` arrived.
    arrived: Vec<Instant>,
}

impl Lines {
    /// Both of `child`'s streams, each line also written to `path` after the seconds since now.
    pub(super) fn of(child: &mut Child, path: &Path) -> Self {
        let began = Instant::now();
        let log = Arc::new(Mutex::new(File::create(path).unwrap()));
        let (send, receive) = mpsc::channel();
        let streams: Vec<Box<dyn Read + Send>> = vec![
            Box::new(child.stdout.take().unwrap()),
            Box::new(child.stderr.take().unwrap()),
        ];
        for stream in streams {
            let log = log.clone();
            let send = send.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let line = line.unwrap();
                    let now = Instant::now();
                    writeln!(log.lock().unwrap(), "{} {line}", stamp(now, began)).unwrap();
                    let _ = send.send((now, line));
                }
            });
        }
        Self {
            receive,
            began,
            history: Vec::new(),
            arrived: Vec::new(),
        }
    }

    pub(super) fn until(&mut self, needle: &str) -> String {
        self.until_before(needle, Instant::now() + LIMIT)
    }

    pub(super) fn until_before(&mut self, needle: &str, deadline: Instant) -> String {
        self.seen_before(needle, deadline).unwrap_or_else(|e| {
            let why = match e {
                mpsc::RecvTimeoutError::Timeout => stalled(format!("a line holding {needle}")),
                mpsc::RecvTimeoutError::Disconnected => {
                    format!("waiting for {needle}: both streams closed")
                }
            };
            panic!("{why}; {:?}", self.stamped())
        })
    }

    /// The first line holding `needle` by `deadline`, or why none came: both streams closed, or
    /// the time ran out.
    pub(super) fn seen_before(
        &mut self,
        needle: &str,
        deadline: Instant,
    ) -> Result<String, mpsc::RecvTimeoutError> {
        loop {
            let (at, line) = self
                .receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            self.keep(at, line.clone());
            if line.contains(needle) {
                return Ok(line);
            }
        }
    }

    /// The lines that arrived at or after `mark`, each after the seconds since `mark`.
    pub(super) fn since(&self, mark: Instant) -> String {
        self.arrived
            .iter()
            .zip(&self.history)
            .filter(|(at, _)| **at >= mark)
            .map(|(at, line)| format!("{} {line}", stamp(*at, mark)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn keep(&mut self, at: Instant, line: String) {
        self.arrived.push(at);
        self.history.push(line);
    }

    /// The history, each line after the seconds since the child's output was first read.
    fn stamped(&self) -> Vec<String> {
        self.arrived
            .iter()
            .zip(&self.history)
            .map(|(at, line)| format!("{} {line}", stamp(*at, self.began)))
            .collect()
    }

    pub(super) fn closed(&mut self) {
        let began = Instant::now();
        loop {
            match self
                .receive
                .recv_timeout(LIMIT.saturating_sub(began.elapsed()))
            {
                Ok((at, line)) => self.keep(at, line),
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(_) => panic!(
                    "{}; {:?}",
                    stalled("the generation's pipes to close"),
                    self.stamped()
                ),
            }
        }
    }
}

/// `at` as the seconds since `mark`, to the millisecond.
fn stamp(at: Instant, mark: Instant) -> String {
    format!("+{:.3}s", at.saturating_duration_since(mark).as_secs_f64())
}
