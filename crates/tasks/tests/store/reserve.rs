//! `reserve`: a task held for a session until the reservation lapses.

use crate::common::{GONE, NOW, amber_back_meanwhile, refused, two_and_a_task};

#[test]
fn a_reservation_is_held_until_it_lapses_and_said_on_the_record() {
    let f = two_and_a_task("ready");
    let task = f.store.reserve("sim/jump", "amber-heron", 60.0).unwrap();
    assert_eq!(task.holders("reserved"), ["amber-heron"]);
    let held = task.roles.iter().find(|r| r.role == "reserved").unwrap();
    assert_eq!((held.since, held.expires), (NOW, Some(NOW + 60.0)));
    assert_eq!(task.status, "ready", "a reservation starts nothing");
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.text.as_str()),
        ("system", "amber-heron reserved it")
    );
}

#[test]
fn two_sessions_cannot_reserve_the_same_task() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 3600.0).unwrap();
    assert_eq!(
        refused(f.store.reserve("sim/jump", "brisk-finch", 3600.0)),
        "sim/jump is reserved by amber-heron"
    );
}

#[test]
fn reserving_again_renews_the_holders_own_reservation() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 60.0).unwrap();
    f.advance(30.0);
    let task = f.store.reserve("sim/jump", "amber-heron", 60.0).unwrap();
    let held: Vec<_> = task.roles.iter().filter(|r| r.role == "reserved").collect();
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].since, held[0].expires),
        (NOW + 30.0, Some(NOW + 90.0))
    );
}

#[test]
fn a_lapsed_or_lost_reservation_gives_way_to_another() {
    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 60.0).unwrap();
    f.advance(61.0);
    assert_eq!(
        f.store
            .reserve("sim/jump", "brisk-finch", 3600.0)
            .unwrap()
            .holders("reserved"),
        ["brisk-finch"]
    );

    let f = two_and_a_task("ready");
    f.store.reserve("sim/jump", "amber-heron", 1e6).unwrap();
    f.advance(GONE);
    assert_eq!(
        f.store
            .reserve("sim/jump", "brisk-finch", 3600.0)
            .unwrap()
            .holders("reserved"),
        ["brisk-finch"]
    );
}

#[test]
fn a_task_claimed_by_a_live_session_cannot_be_reserved() {
    let f = two_and_a_task("ready");
    f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(
        refused(f.store.reserve("sim/jump", "brisk-finch", 3600.0)),
        "sim/jump is already claimed by amber-heron"
    );
    f.advance(GONE);
    assert_eq!(
        f.store
            .reserve("sim/jump", "brisk-finch", 3600.0)
            .unwrap()
            .holders("reserved"),
        ["brisk-finch"]
    );
}

#[test]
fn a_reservation_decided_on_an_executor_that_came_back_is_decided_again() {
    let (f, late) = amber_back_meanwhile();
    f.store.claim("sim/jump", "amber-heron").unwrap();
    f.advance(GONE);
    assert_eq!(
        refused(late.reserve("sim/jump", "brisk-finch", 3600.0)),
        "sim/jump is already claimed by amber-heron"
    );
}

#[test]
fn a_reservation_given_up_on_a_holder_that_came_back_is_decided_again() {
    let (f, late) = amber_back_meanwhile();
    f.store.reserve("sim/jump", "amber-heron", 1e6).unwrap();
    f.advance(GONE);
    assert_eq!(
        refused(late.reserve("sim/jump", "brisk-finch", 3600.0)),
        "sim/jump is reserved by amber-heron"
    );
}

#[test]
fn a_finished_task_takes_no_reservation() {
    for status in ["done", "cancelled"] {
        let f = two_and_a_task(status);
        assert_eq!(
            refused(f.store.reserve("sim/jump", "amber-heron", 3600.0)),
            format!("sim/jump is {status}")
        );
    }
}

#[test]
fn a_reservation_needs_a_session_that_may_act_and_a_task() {
    let f = two_and_a_task("ready");
    assert_eq!(
        refused(f.store.reserve("sim/jump", "nobody", 3600.0)),
        f.no_session("nobody")
    );
    assert_eq!(
        refused(f.store.reserve("sim/none", "amber-heron", 3600.0)),
        "no task 'sim/none'"
    );
}
