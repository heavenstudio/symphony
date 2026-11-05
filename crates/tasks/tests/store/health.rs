//! Health as the store reads it: the rule itself is `src/health.rs`; these hold what the store gives
//! it (the clock, the process table, the work a session holds).

use crate::common::{Fixture, GONE, HOST, NOW, Seed, refused, session};
use symphony_board::Health;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::rows::SessionRow;

#[test]
fn health_ages_from_live_to_stale_to_lost() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Live);
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        f.store.session("amber-heron").unwrap().health,
        Health::Stale
    );
    f.advance(STALE_SECONDS);
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Lost);
}

#[test]
fn a_dead_local_pid_is_stale_at_once_but_only_time_makes_it_lost() {
    let monitored = SessionRow {
        pid: Some(41),
        host: HOST.into(),
        ..session("amber-heron")
    };
    let f = Fixture::new(
        Seed::default()
            .session_row(monitored)
            .task("sim/jump", "ready"),
    );
    f.alive(&[41]);
    f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Live);
    f.alive(&[]);
    assert_eq!(
        f.store.session("amber-heron").unwrap().health,
        Health::Stale
    );
    f.advance(GONE);
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Lost);
}

#[test]
fn a_pid_on_another_host_is_not_probed() {
    let elsewhere = SessionRow {
        pid: Some(41),
        host: "some-other-machine".into(),
        ..session("amber-heron")
    };
    let f = Fixture::new(Seed::default().session_row(elsewhere));
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Live);
}

#[test]
fn leaving_is_its_own_state() {
    let f = Fixture::new(Seed::default().session_row(SessionRow {
        left_at: Some(NOW),
        ..session("amber-heron")
    }));
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Left);
}

#[test]
fn gone_holding_nothing_reads_left() {
    let f = Fixture::new(Seed::default().session("amber-heron"));
    f.advance(GONE);
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Left);
}

#[test]
fn a_busy_mark_nobody_clears_lapses_to_lost() {
    let busy = SessionRow {
        busy_until: Some(NOW + 600.0),
        ..session("amber-heron")
    };
    let f = Fixture::new(
        Seed::default()
            .session_row(busy)
            .task("sim/jump", "in_progress")
            .role("sim/jump", "amber-heron", "executor"),
    );
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        f.store.session("amber-heron").unwrap().health,
        Health::Live,
        "past the live line, still inside its long call"
    );
    f.advance(GONE);
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Lost);
}

#[test]
fn gone_holding_only_finished_work_reads_left() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    );
    f.store
        .verdict("sim/jump", "brisk-finch", "approve", "good")
        .unwrap();
    f.advance(GONE);
    let board = f.store.board().unwrap();
    let health: Vec<(&str, Health)> = board
        .sessions
        .iter()
        .map(|s| (s.row.handle.as_str(), s.health))
        .collect();
    assert_eq!(
        health,
        [("amber-heron", Health::Left), ("brisk-finch", Health::Left)]
    );
}

#[test]
fn an_executor_whose_work_waits_in_review_ends_quietly_until_changes_are_asked() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    );
    f.advance(GONE);
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Left);
    f.store
        .verdict("sim/jump", "brisk-finch", "changes", "the hash moved")
        .unwrap();
    assert_eq!(f.store.session("amber-heron").unwrap().health, Health::Lost);
}

#[test]
fn a_reviewer_gone_before_its_verdict_is_lost() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    );
    f.advance(GONE);
    assert_eq!(f.store.session("brisk-finch").unwrap().health, Health::Lost);
    assert_eq!(f.store.claimable("brisk-finch").unwrap(), ["sim/jump"]);
}

#[test]
fn claimable_is_only_work_another_session_could_take() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/a", "in_progress")
            .role("sim/a", "amber-heron", "executor") // in progress: claimable
            .task("sim/b", "review")
            .role("sim/b", "amber-heron", "executor") // waits for its reviewer: not claimable
            .task("sim/c", "review")
            .role("sim/c", "brisk-finch", "executor")
            .role("sim/c", "amber-heron", "reviewer") // a review with no verdict: claimable
            .task("sim/d", "done")
            .role("sim/d", "amber-heron", "reviewer") // done: not claimable
            .task("sim/e", "ready")
            .role("sim/e", "amber-heron", "orchestrator") // coordinating is not work to take
            .task("sim/f", "blocked")
            .role("sim/f", "amber-heron", "executor") // blocked cannot be claimed
            .task("sim/g", "design")
            .role("sim/g", "amber-heron", "executor"), // design work is claimable
    );
    assert_eq!(
        f.store.claimable("amber-heron").unwrap(),
        ["sim/a", "sim/c", "sim/g"]
    );
    assert!(f.store.claimable("nobody").unwrap().is_empty());
}

#[test]
fn an_unknown_session_is_refused() {
    let f = Fixture::new(Seed::default());
    assert_eq!(refused(f.store.session("nobody")), "no session 'nobody'");
}

#[test]
fn a_role_held_by_a_session_the_board_does_not_know_reads_lost() {
    let f = Fixture::new(Seed::default().task("sim/jump", "in_progress").role(
        "sim/jump",
        "ghost-wren",
        "executor",
    ));
    let task = f.store.get("sim/jump").unwrap();
    assert_eq!(task.roles[0].health, Health::Lost);
}
