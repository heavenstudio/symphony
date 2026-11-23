//! `claim`, and what it must get past: a live executor, a reservation, the reviewer, a dependency.

use crate::common::{
    Fixture, GONE, NOW, Seed, amber_back_meanwhile, refused, session, two_and_a_task,
};
use std::sync::{Arc, Barrier};
use symphony_board::Health;
use symphony_board::health::LIVE_SECONDS;

#[test]
fn claim_makes_the_session_executor_and_starts_the_task() {
    let f = two_and_a_task("ready");
    let task = f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(task.status, "in_progress");
    assert_eq!(task.holders("executor"), ["amber-heron"]);
    assert_eq!(f.store.get("sim/jump").unwrap(), task);
    let said = f.store.thread("sim/jump").unwrap();
    let last = said.last().unwrap();
    assert_eq!(
        (last.sender.as_str(), last.kind.as_str(), last.text.as_str()),
        ("amber-heron", "system", "amber-heron claimed it")
    );
}

#[test]
fn a_second_live_session_cannot_claim() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(
        refused(f.store.claim("sim/jump", "brisk-finch")),
        "sim/jump is held by amber-heron (live)"
    );
}

#[test]
fn claiming_twice_is_idempotent_for_the_holder() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    let again = f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(again.holders("executor"), ["amber-heron"]);
    assert_eq!(
        f.store.thread("sim/jump").unwrap().len(),
        1,
        "a second claim says nothing"
    );
}

#[test]
fn a_lost_executor_is_handed_over_on_the_record() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    f.advance(GONE);
    let task = f.store.claim("sim/jump", "brisk-finch").unwrap();
    assert_eq!(task.holders("executor"), ["brisk-finch"]);
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(last.kind, "system");
    assert_eq!(
        last.text,
        "brisk-finch claimed it, handed over from amber-heron (lost)"
    );
}

#[test]
fn a_stale_executor_is_not_taken_over() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        refused(f.store.claim("sim/jump", "brisk-finch")),
        "sim/jump is held by amber-heron (stale)"
    );
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("executor"),
        ["amber-heron"]
    );
}

#[test]
fn acting_counts_as_being_seen() {
    let f = two_and_a_task("ready");
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        f.store.session("amber-heron").unwrap().health,
        Health::Stale
    );
    f.store.claim("sim/jump", "amber-heron").unwrap();
    let me = f.store.session("amber-heron").unwrap();
    assert_eq!((me.health, me.row.last_seen), (Health::Live, f.now()));
}

#[test]
fn an_unfinished_dependency_blocks_the_claim() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/first", "ready")
            .task("sim/second", "ready")
            .task("sim/third", "done")
            .link("sim/second", "dep", "sim/first")
            .link("sim/second", "dep", "sim/third"),
    );
    assert_eq!(
        refused(f.store.claim("sim/second", "amber-heron")),
        "sim/second waits on sim/first"
    );
}

#[test]
fn only_concept_design_ready_or_in_progress_work_can_be_claimed() {
    for status in ["backlog", "review", "done", "blocked", "cancelled"] {
        let f = two_and_a_task(status);
        assert_eq!(
            refused(f.store.claim("sim/jump", "amber-heron")),
            format!("sim/jump is {status}; only concept, design or ready work can be claimed")
        );
    }
}

#[test]
fn claiming_concept_or_design_work_keeps_its_stage() {
    for status in ["concept", "design"] {
        let f = two_and_a_task(status);
        let task = f.store.claim("sim/jump", "amber-heron").unwrap();
        assert_eq!(
            (task.status.as_str(), task.holders("executor")),
            (status, vec!["amber-heron"])
        );
    }
}

#[test]
fn a_left_or_unknown_session_cannot_act() {
    let f = Fixture::new(
        Seed::default()
            .session_row(symphony_board::rows::SessionRow {
                left_at: Some(NOW),
                ..session("amber-heron")
            })
            .task("sim/jump", "ready"),
    );
    assert_eq!(
        refused(f.store.claim("sim/jump", "amber-heron")),
        "session amber-heron has left; attach again"
    );
    assert_eq!(
        refused(f.store.claim("sim/jump", "nobody")),
        f.no_session("nobody")
    );
}

#[test]
fn an_unknown_task_cannot_be_claimed() {
    let f = two_and_a_task("ready");
    assert_eq!(
        refused(f.store.claim("sim/none", "amber-heron")),
        "no task 'sim/none'"
    );
}

#[test]
fn concurrent_claims_have_exactly_one_winner() {
    let mut seed = Seed::default().task("sim/jump", "ready");
    let handles: Vec<String> = (0..8).map(|i| format!("racer-{i}")).collect();
    for h in &handles {
        seed = seed.session(h);
    }
    let f = Fixture::new(seed);
    let barrier = Arc::new(Barrier::new(handles.len()));
    let outcomes: Vec<(String, Result<symphony_board::Task, symphony_board::Error>)> =
        std::thread::scope(|scope| {
            let racers: Vec<_> = handles
                .iter()
                .map(|h| {
                    let (store, barrier) = (f.other(), Arc::clone(&barrier));
                    scope.spawn(move || {
                        barrier.wait();
                        (h.clone(), store.claim("sim/jump", h))
                    })
                })
                .collect();
            racers.into_iter().map(|r| r.join().unwrap()).collect()
        });
    let wins: Vec<&str> = outcomes
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(h, _)| h.as_str())
        .collect();
    assert_eq!(wins.len(), 1, "{outcomes:?}");
    for (_, outcome) in outcomes.iter().filter(|(_, r)| r.is_err()) {
        assert_eq!(
            outcome,
            &Err(symphony_board::Error::Refused(format!(
                "sim/jump is held by {} (live)",
                wins[0]
            )))
        );
    }
    assert_eq!(f.store.get("sim/jump").unwrap().holders("executor"), wins);
    assert_eq!(
        f.store.thread("sim/jump").unwrap().len(),
        1,
        "one claim on the record"
    );
}

#[test]
fn a_reservation_keeps_others_off_and_its_holders_claim_clears_it() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 3600.0).unwrap();
    assert_eq!(
        refused(f.store.claim("sim/jump", "brisk-finch")),
        "sim/jump is reserved by amber-heron"
    );
    let task = f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(task.holders("executor"), ["amber-heron"]);
    assert!(task.holders("reserved").is_empty());
}

#[test]
fn a_reservation_expires() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 60.0).unwrap();
    f.advance(61.0);
    assert!(
        f.store
            .get("sim/jump")
            .unwrap()
            .holders("reserved")
            .is_empty(),
        "a lapsed reservation is not shown"
    );
    assert_eq!(
        f.store
            .claim("sim/jump", "brisk-finch")
            .unwrap()
            .holders("executor"),
        ["brisk-finch"]
    );
}

#[test]
fn a_lost_sessions_reservation_does_not_hold() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 1e6).unwrap();
    f.advance(GONE);
    assert_eq!(
        f.store
            .claim("sim/jump", "brisk-finch")
            .unwrap()
            .holders("executor"),
        ["brisk-finch"]
    );
}

#[test]
fn the_reviewer_cannot_also_claim() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "brisk-finch", "reviewer"),
    );
    assert_eq!(
        refused(f.store.claim("sim/jump", "brisk-finch")),
        "brisk-finch reviews sim/jump and cannot also execute it"
    );
}

#[test]
fn a_refused_claim_writes_nothing() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    let before = f.rows();
    f.advance(1.0);
    refused(f.store.claim("sim/jump", "brisk-finch"));
    assert_eq!(f.rows(), before, "not even the claimer's last_seen");
}

#[test]
fn a_claim_decided_on_a_board_that_moved_is_decided_again() {
    let f = two_and_a_task("ready");
    let rival = f.other();
    let late = f.late(move || {
        rival.claim("sim/jump", "amber-heron").unwrap();
    });
    assert_eq!(
        refused(late.claim("sim/jump", "brisk-finch")),
        "sim/jump is held by amber-heron (live)"
    );
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("executor"),
        ["amber-heron"]
    );
}

#[test]
fn a_handover_decided_on_a_holder_that_came_back_is_decided_again() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "ready")
            .task("sim/other", "ready"),
    );
    f.store.claim("sim/jump", "amber-heron").unwrap();
    f.advance(GONE);
    let back = f.other();
    let late = f.late(move || {
        back.claim("sim/other", "amber-heron").unwrap(); // acting is being seen: live again
    });
    assert_eq!(
        refused(late.claim("sim/jump", "brisk-finch")),
        "sim/jump is held by amber-heron (live)"
    );
}

#[test]
fn a_claim_past_a_reserver_that_came_back_is_decided_again() {
    let (f, late) = amber_back_meanwhile();
    f.store.reserve("sim/jump", "amber-heron", 1e6).unwrap();
    f.advance(GONE);
    assert_eq!(
        refused(late.claim("sim/jump", "brisk-finch")),
        "sim/jump is reserved by amber-heron"
    );
}
