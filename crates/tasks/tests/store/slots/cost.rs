//! What the cap costs on a board the size of the live one (2026-11-07: 2,170 tasks and 4,300
//! roles, where a capped claim that read every task and scanned the roles twice took 328 ms against
//! 118 ms uncapped). As `tools/board-query-cost`'s plan check does, deterministically: each
//! statement the cap sends is planned by the server, and none reads a table whole or runs a subquery
//! for each row a predicate tests; and a capped claim adds one round trip for each level of tasks
//! above the deepest open one, never one for each task.

use super::{EIGHT, K, O, capped, sessions, with};
use crate::common::{Fixture, session, task};
use serde_json::json;
use symphony_board::SLOT_READS;
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

/// Whether a plan's line holds a subquery in a predicate, which EXPLAIN shows as `(Operator)`.
fn subquery_in_predicate(line: &str) -> bool {
    let Some((_, predicate)) = line.split_once("predicate: ") else {
        return false;
    };
    predicate.split('(').skip(1).any(|inner| {
        let name: String = inner
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        name.starts_with(|c: char| c.is_ascii_uppercase()) && inner[name.len()..].starts_with(')')
    })
}

// On SurrealDB alone: it counts the round trips SurrealDB's client makes.
#[test]
fn the_caps_reads_are_planned_on_an_index_and_a_capped_claim_adds_a_round_trip_a_level() {
    let f = big(Backend::SurrealDb);
    let client = f.store.client();
    let vars = json!({
        "slot_statuses": ["in_progress", "review"],
        "slot_wanted": ["work/a", "deep", "old/7", "review/work/a"],
        "slug": "work/next",
        "slot_holds": [["work/a", 1800000000.0], ["old/7", 1.0]],
    });
    for statement in SLOT_READS {
        let mut answers = (client.query(&format!("EXPLAIN {statement};"), vars.clone()))
            .unwrap_or_else(|why| panic!("{statement}: {why:?}"));
        let plan = (answers.pop())
            .and_then(|answer| answer.as_str().map(String::from))
            .unwrap_or_default();
        eprintln!("--- {statement}\n{plan}");
        assert!(!plan.is_empty(), "{statement}: no plan");
        for line in plan.lines() {
            assert!(
                !line.trim_start().starts_with("TableScan"),
                "{statement} reads a table whole:\n{plan}"
            );
            assert!(
                !subquery_in_predicate(line),
                "{statement} runs a subquery for each row:\n{plan}"
            );
        }
    }
    for (statement, index) in [
        (SLOT_READS[0], "task_status"),
        (SLOT_READS[2], "role_slug"),
        (SLOT_READS[7], "task_session_slug"),
        (SLOT_READS[8], "task_session_branch"),
    ] {
        let plan = format!(
            "{:?}",
            client.query(&format!("EXPLAIN {statement};"), vars.clone())
        );
        assert!(plan.contains(index), "{statement} through {index}: {plan}");
    }
    let uncapped = f.other();
    let before = uncapped.client().requests();
    uncapped.claim("plain/next", K).unwrap();
    let plain = uncapped.client().requests() - before;
    let store = capped(&f, 9);
    let before = store.client().requests();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let held = store.client().requests() - before;
    eprintln!("perf: a claim took {plain} round trips uncapped, {held} capped");
    assert!(
        held <= plain + DEEPEST,
        "{held} round trips capped, {plain} uncapped: one more for each of {DEEPEST} levels at most"
    );
    let observation = f.other();
    let before = observation.client().requests();
    let began = std::time::Instant::now();
    let holdings = observation.slot_holdings().unwrap();
    eprintln!(
        "perf: one whole-board holdings observation took {:?}",
        began.elapsed()
    );
    assert_eq!(observation.client().requests() - before, 1);
    assert_eq!(holdings.len(), 1, "finished history holds no slots");
    assert_eq!(holdings[O].len(), 9);
    assert!(
        holdings[O].contains("deep"),
        "nested work uses its feature key"
    );
}
