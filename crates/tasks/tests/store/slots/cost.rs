//! What the cap costs on a board the size of the live one (2026-11-07: 2,170 tasks and 4,300
//! roles, where a capped claim that read every task and scanned the roles twice took 328 ms against
//! 118 ms uncapped), deterministically: a capped claim adds one round trip for each level of tasks
//! above the deepest open one, never one for each task, and the whole board's holdings are one
//! read.

use super::{EIGHT, K, O, capped, sessions, with};
use crate::common::{Fixture, session, task};
use symphony_board::rows::TaskRow;

/// Finished tasks, each with an orchestrator and an executor: the board's history.
const DONE: usize = 2_100;
/// The levels of tasks above the deepest open task.
const DEEPEST: usize = 3;

/// The board: [`DONE`] finished tasks under twenty orchestrators and forty workers, six of O's
/// open, one more open three levels below a task of O's, and two ready to claim.
fn big() -> Fixture {
    let mut seed = sessions();
    for n in 0..20 {
        seed = seed.session(&format!("orch-{n}"));
    }
    for n in 0..40 {
        seed = seed.session(&format!("worker-{n}"));
    }
    for i in 0..DONE {
        let slug = format!("old/{i}");
        let id = seed.dump.tasks.len() as i64 + 1;
        seed = seed
            .task_row(TaskRow {
                id,
                ..task(&slug, "done")
            })
            .role(&slug, &format!("orch-{}", i % 20), "orchestrator")
            .role(&slug, &format!("worker-{}", i % 40), "executor");
    }
    for (slug, status) in &EIGHT[..6] {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed = with(seed, "deep", "ready", None, Some(O));
    seed = with(seed, "deep/one", "ready", Some("deep"), None);
    seed = with(seed, "deep/one/two", "ready", Some("deep/one"), None);
    seed = with(
        seed,
        "deep/one/two/three",
        "in_progress",
        Some("deep/one/two"),
        None,
    );
    seed = with(seed, "work/next", "ready", None, Some(O));
    seed = with(seed, "plain/next", "ready", None, Some(O));
    Fixture::new(seed.session_row(session("unused")))
}

#[test]
fn a_capped_claim_adds_a_round_trip_a_level_and_the_holdings_are_one_read() {
    let f = big();
    let uncapped = f.other();
    let before = uncapped.database().requests();
    uncapped.claim("plain/next", K).unwrap();
    let plain = uncapped.database().requests() - before;
    let store = capped(&f, 9);
    let before = store.database().requests();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let held = store.database().requests() - before;
    eprintln!("perf: a claim took {plain} round trips uncapped, {held} capped");
    assert!(
        held <= plain + DEEPEST,
        "{held} round trips capped, {plain} uncapped: one more for each of {DEEPEST} levels at most"
    );
    let observation = f.other();
    let before = observation.database().requests();
    let began = std::time::Instant::now();
    let holdings = observation.slot_holdings().unwrap();
    eprintln!(
        "perf: one whole-board holdings observation took {:?}",
        began.elapsed()
    );
    assert_eq!(observation.database().requests() - before, 1);
    assert_eq!(holdings.len(), 1, "finished history holds no slots");
    assert_eq!(holdings[O].len(), 9);
    assert!(
        holdings[O].contains("deep"),
        "nested work uses its feature key"
    );
}
