//! The eight-worktree scan's wait, on events alone: the intercepted worktrees' arrivals and the
//! scan's end, with no deadline a loaded host can outrun.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::process::Output;
use std::sync::mpsc::Receiver;

/// What the scan case hears: a worktree holding its first merge-base (its path), or the scan's end.
pub(crate) enum Scan {
    Arrived(String),
    Ended(io::Result<Output>),
}

/// The worktrees in flight together, or, when the scan ends first, those that arrived and its end.
pub(crate) type InFlight = Result<BTreeSet<String>, (BTreeSet<String>, io::Result<Output>)>;

/// The worktrees after `want` arrivals. The intercept holds each worktree at its first merge-base
/// and nothing releases one before this returns, so every arrival is a worktree the scan started
/// without awaiting any: a scan that awaits one first (a serial scan) never brings the next arrival
/// and never passes, and one that ends first answers what arrived and what it said. Each arrival is
/// written to stderr as it comes, past the harness's capture, so a serial scan, which the suite's
/// budget ends as a timeout, shows how many worktrees were in flight and names the held one.
pub(crate) fn in_flight(events: &Receiver<Scan>, want: usize) -> InFlight {
    let mut active = BTreeSet::new();
    for _ in 0..want {
        match events.recv().expect("the scan's waiter sends its end") {
            Scan::Arrived(path) => {
                let shown = path.trim_end().to_string();
                active.insert(path);
                let _ = writeln!(
                    io::stderr(),
                    "scan: {} of {want} worktrees in flight, none awaited: {shown}",
                    active.len()
                );
            }
            Scan::Ended(done) => return Err((active, done)),
        }
    }
    Ok(active)
}

/// The failure of a scan that ended before the worktrees it should have had in flight together.
pub(crate) fn ended_early(
    (active, done): (BTreeSet<String>, io::Result<Output>),
) -> BTreeSet<String> {
    let done = done.expect("the scan's output reads to its end");
    panic!(
        "scan must start eight independent worktrees before awaiting any, but it ended with {} in flight: {active:?}\n{}{}",
        active.len(),
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The scan's output, the one event left once its arrivals are in.
pub(crate) fn scan_end(events: &Receiver<Scan>) -> Output {
    match events.recv() {
        Ok(Scan::Ended(done)) => done.expect("the scan's output reads to its end"),
        Ok(Scan::Arrived(path)) => panic!("an arrival after the scan's last: {path}"),
        Err(error) => panic!("the scan's waiter ended without its end: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Scan, ended_early, in_flight, scan_end};
    use crate::test_channel::channel;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn ended(code: i32) -> Scan {
        Scan::Ended(Ok(Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: b"said".to_vec(),
            stderr: Vec::new(),
        }))
    }

    #[test]
    fn eight_arrivals_answer_the_eight_and_leave_the_scan_end_unread() {
        let (send, receive) = channel();
        for n in 0..8 {
            send.send(Scan::Arrived(format!("/live{n}\n"))).unwrap();
        }
        send.send(ended(0)).unwrap();
        let active = in_flight(&receive, 8).unwrap_or_else(ended_early);
        assert_eq!(
            active.into_iter().collect::<Vec<_>>(),
            (0..8).map(|n| format!("/live{n}\n")).collect::<Vec<_>>()
        );
        let done = scan_end(&receive);
        assert!(done.status.success());
        assert_eq!(done.stdout, b"said");
    }

    #[test]
    #[should_panic(expected = "an arrival after the scan's last: /live8")]
    fn an_arrival_past_the_wanted_ones_is_not_the_scan_end() {
        let (send, receive) = channel();
        send.send(Scan::Arrived("/live8".into())).unwrap();
        scan_end(&receive);
    }

    #[test]
    #[should_panic(expected = "the scan's waiter ended without its end")]
    fn a_waiter_gone_without_the_scan_end_is_not_a_pass() {
        let (send, receive) = channel::<Scan>();
        drop(send);
        scan_end(&receive);
    }

    #[test]
    #[should_panic(expected = "ended with 3 in flight")]
    fn a_scan_that_ends_early_fails_with_its_count_and_output() {
        let (send, receive) = channel();
        for n in 0..3 {
            send.send(Scan::Arrived(format!("/live{n}\n"))).unwrap();
        }
        send.send(ended(1)).unwrap();
        in_flight(&receive, 8).unwrap_or_else(ended_early);
    }

    #[test]
    fn a_scan_that_ends_before_eight_answers_what_arrived_and_what_it_said() {
        let (send, receive) = channel();
        for n in 0..3 {
            send.send(Scan::Arrived(format!("/live{n}\n"))).unwrap();
        }
        send.send(ended(1)).unwrap();
        let Err((active, done)) = in_flight(&receive, 8) else {
            panic!("the scan ended with three in flight");
        };
        assert_eq!(active.len(), 3);
        let done = done.unwrap();
        assert_eq!(done.status.code(), Some(1));
        assert_eq!(done.stdout, b"said");
    }

    #[test]
    fn a_repeated_worktree_counts_once() {
        let (send, receive) = channel();
        for path in ["/live0\n", "/live0\n"] {
            send.send(Scan::Arrived(path.into())).unwrap();
        }
        let active = in_flight(&receive, 2).unwrap_or_else(|_| panic!("two arrivals"));
        assert_eq!(active.len(), 1);
    }
}
