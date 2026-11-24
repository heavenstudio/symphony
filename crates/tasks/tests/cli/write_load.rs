//! A beat's board_changed event writes its session revision independently of counter:board.
//! These real CLI writes run while 32 independent sessions continuously beat on a scratch board,
//! each read and decided once: the count of their tries again is the measure, not their time.

use super::launch::Terminals;
use std::sync::{
    Barrier,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use symphony_board::{Beat, NewSession, Store};

const WRITERS: usize = 32;
/// How long every writer has to make its first two beats: the backstop of the 30 s it once had, a
/// wait for the beats, which a loaded machine slows and never stops.
const PATIENCE: Duration = test_time::backstop(Duration::from_secs(30));

struct Stop<'a>(&'a AtomicBool);
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[test]
fn create_and_launch_finish_under_32_beats_without_the_counter_board_conflict() {
    let terminals = Terminals::new("beat-load");
    let actor = terminals.orchestrator();
    let board = terminals.board();
    let writers: Vec<_> = (0..WRITERS)
        .map(|n| {
            let handle = board
                .register(&NewSession::new("worker"))
                .unwrap()
                .row
                .handle;
            let agent = format!("load-agent-{n}");
            let bound = board.bind_agent(&agent, &handle, true).unwrap();
            assert!(bound, "binding {agent} to {handle}: {bound}");
            (terminals.board(), agent, handle)
        })
        .collect();
    let stopped = AtomicBool::new(false);
    let start = Barrier::new(WRITERS + 1);
    let (ready, began) = mpsc::channel();
    let stats = std::thread::scope(|threads| {
        let stop = Stop(&stopped);
        let workers: Vec<_> = writers
            .into_iter()
            .map(|(store, agent, handle)| {
                let (start, stopped, ready) = (&start, &stopped, ready.clone());
                threads.spawn(move || {
                    let requests = store.database().requests();
                    let mut beats = 0;
                    start.wait();
                    while !stopped.load(Ordering::Relaxed) {
                        let beat = Beat {
                            busy_for: Some(60.0),
                            call: Some(format!("call-{beats}")),
                            ended: false,
                        };
                        assert_eq!(
                            store.agent_beat(&agent, &beat).unwrap(),
                            Some(handle.clone())
                        );
                        beats += 1;
                        if beats == 2 {
                            ready.send(()).unwrap();
                        }
                    }
                    (beats, store.database().requests() - requests)
                })
            })
            .collect();
        start.wait();
        let all_active = Instant::now();
        for _ in 0..WRITERS {
            began
                .recv_timeout(PATIENCE.saturating_sub(all_active.elapsed()))
                .expect("all 32 writers must actually change their sessions");
        }
        for args in [
            vec!["create", "tools/loaded", "Created under beats"],
            vec!["launch", "tools/loaded", "--kind", "claude"],
        ] {
            let tried = Store::tried_again_here();
            let began = Instant::now();
            let ran = terminals.as_(&actor, &args);
            let elapsed = began.elapsed();
            let again = Store::tried_again_here() - tried;
            assert_eq!(ran.code, 0, "{args:?}: {} {}", ran.out, ran.err);
            // The beats move nothing it reads or writes: it never read and decided again, however
            // long a loaded machine made it take.
            assert_eq!(again, 0, "{args:?} met the beats' writes {again} times");
            eprintln!(
                "{args:?}: {elapsed:?} and {again} tries again with {WRITERS} continuous beat writers"
            );
        }
        drop(stop);
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(stats.len(), WRITERS, "{stats:?}");
    assert!(stats.iter().all(|(beats, _)| *beats >= 2), "{stats:?}");
    let beats: usize = stats.iter().map(|(beats, _)| beats).sum();
    let retries: usize = stats.iter().map(|(beats, requests)| requests - beats).sum();
    // BEAT contains no stale guard: an extra request here is an optimistic transaction conflict.
    eprintln!(
        "{beats} committed beats, {retries} transaction-conflict retries; no stale-guard path in BEAT"
    );
    assert_eq!(
        board.get("tools/loaded").unwrap().title,
        "Created under beats"
    );
    let launches = board.task_sessions("tools/loaded").unwrap();
    assert_eq!(launches.len(), 1, "{launches:?}");
    assert!(!launches[0].pane.is_empty(), "{launches:?}");
}
