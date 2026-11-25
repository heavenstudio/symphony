//! [`super::passes`], the output worker's loop: a pass for the message that wakes it, and one
//! more, never one each, for the wakes sent while a pass is pending or running.

use super::passes;
use crate::channel::channel;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[test]
fn refreshes_in_a_row_while_a_pass_runs_ask_for_one_more_pass() {
    let (wake, messages) = channel::<Option<u32>>();
    let (release, held) = channel::<()>();
    let began = Arc::new(AtomicUsize::new(0));
    let worker = std::thread::spawn({
        let began = began.clone();
        move || {
            let mut handed = Vec::new();
            passes(&messages, |jobs| {
                // The first pass runs until the test lets it end.
                if began.fetch_add(1, Ordering::SeqCst) == 0 {
                    held.recv().unwrap();
                }
                handed.push(jobs);
            });
            handed
        }
    });
    wake.send(None).unwrap();
    test_time::wait::until_true(
        "the first pass to begin",
        test_time::wait::backstop(Duration::ZERO),
        || began.load(Ordering::SeqCst) == 1,
    )
    .unwrap();
    // Five refreshes, a save and one more refresh, all while that pass runs.
    for _ in 0..5 {
        wake.send(None).unwrap();
    }
    wake.send(Some(7)).unwrap();
    wake.send(None).unwrap();
    release.send(()).unwrap();
    drop(wake);
    // One more pass, handed the save; the loop ends with its last sender.
    assert_eq!(worker.join().unwrap(), [vec![], vec![7]]);
    assert_eq!(began.load(Ordering::SeqCst), 2);
}

#[test]
fn a_pending_pass_takes_every_job_sent_before_it_in_order() {
    let (send, messages) = channel();
    for message in [Some(1), None, Some(2), None, None, Some(3)] {
        send.send(message).unwrap();
    }
    drop(send);
    let mut handed = Vec::new();
    passes(&messages, |jobs| handed.push(jobs));
    assert_eq!(handed, [vec![1, 2, 3]]);
}

#[test]
fn no_message_runs_no_pass() {
    let (send, messages) = channel::<Option<u32>>();
    drop(send);
    passes(&messages, |jobs| panic!("a pass with no message: {jobs:?}"));
}
