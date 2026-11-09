use super::*;

fn operation(machine: &str, id: &str, kind: Kind, state: State) -> Operation {
    Operation {
        machine_id: Some(machine.into()),
        operation_id: Some(id.into()),
        parent_id: Some("driver".into()),
        handle: Some("worker".into()),
        task: Some("tools/build".into()),
        checkout: "/same/path".into(),
        recipe: "cargo test".into(),
        kind,
        state,
        resource: "process".into(),
        reason: "native child".into(),
        phase_since: Some(99_999_999_999.0),
    }
}

fn sample(machine: &str, sequence: u64, operations: Vec<Operation>) -> Sample {
    Sample {
        machine_id: machine.into(),
        label: "same hostname".into(),
        producer_id: "publisher".into(),
        generation: 1,
        sequence,
        complete: true,
        operations,
    }
}

#[test]
fn machines_are_independent_and_completion_requires_a_new_complete_inventory() {
    let mut projection = Projection::new(2, 8, 10);
    let queued = operation("a", "one", Kind::Build, State::Queued);
    let sibling = operation("a", "two", Kind::Test, State::Running);
    projection
        .observe(sample("a", 1, vec![queued.clone(), sibling.clone()]), 10)
        .unwrap();
    projection
        .observe(
            sample(
                "b",
                1,
                vec![
                    operation("b", "one", Kind::Test, State::Running),
                    operation("b", "two", Kind::Test, State::Queued),
                ],
            ),
            10,
        )
        .unwrap();
    let before = projection.snapshot(10).machines[1].clone();
    assert_eq!(
        projection.snapshot(10).machines[0]
            .counts
            .as_ref()
            .unwrap()
            .queued_builds,
        1
    );
    assert_eq!(before.counts.as_ref().unwrap().queued_tests, 1);
    let mut running = queued;
    running.state = State::Running;
    projection
        .observe(sample("a", 2, vec![running.clone(), sibling]), 11)
        .unwrap();
    let view = projection.snapshot(11);
    let counts = view.machines[0].counts.as_ref().unwrap();
    assert_eq!(
        (
            counts.running_builds,
            counts.queued_builds,
            counts.running_tests
        ),
        (1, 0, 1)
    );
    assert_eq!(
        view.machines[0].operations[0].operation_id.as_deref(),
        Some("one")
    );
    assert_eq!(view.machines[1].operations, before.operations);
    assert_eq!(view.machines[1].counts, before.counts);
    let mut partial = sample("a", 3, vec![running]);
    partial.complete = false;
    projection.observe(partial, 12).unwrap();
    let view = projection.snapshot(12);
    assert_eq!(view.machines[0].operations.len(), 2);
    assert_eq!(view.machines[0].freshness, Freshness::Incomplete);
    assert!(view.machines[0].counts.is_none());
    projection.observe(sample("a", 4, vec![]), 13).unwrap();
    let view = projection.snapshot(13);
    assert!(view.machines[0].operations.is_empty());
    assert_eq!(view.machines[0].counts, Some(Counts::default()));
    assert_eq!(view.machines[1].counts, before.counts);
    let later = projection.snapshot(21);
    assert_eq!(later.machines[0].freshness, Freshness::Fresh);
    assert_eq!(later.machines[1].freshness, Freshness::Stale);
    assert_eq!(later.machines[1].age_ms, Some(11));
    assert!(later.machines[1].counts.is_none());
}

#[test]
fn receiver_age_replay_disconnect_and_restart_never_invent_free_capacity() {
    let mut projection = Projection::new(1, 4, 10);
    assert_eq!(projection.snapshot(0).freshness, Freshness::Unknown);
    let first = sample(
        "a",
        3,
        vec![operation("a", "one", Kind::Test, State::Running)],
    );
    projection.observe(first.clone(), 10).unwrap();
    assert_eq!(
        projection.observe(first.clone(), 20),
        Err(Rejected::OutOfOrder)
    );
    assert_eq!(
        projection.snapshot(20).machines[0].freshness,
        Freshness::Fresh
    );
    let stale = projection.snapshot(21).machines.remove(0);
    assert_eq!(stale.freshness, Freshness::Stale);
    assert_eq!(stale.age_ms, Some(11));
    assert!(stale.counts.is_none());
    assert_eq!(stale.operations.len(), 1);
    assert!(projection.snapshot(9).machines[0].counts.is_none());
    assert_eq!(
        projection.observe(sample("a", 4, vec![]), 9),
        Err(Rejected::ReceiverClock)
    );
    projection.disconnect("a");
    let disconnected = projection.snapshot(20).machines.remove(0);
    assert_eq!(disconnected.freshness, Freshness::Disconnected);
    assert_eq!(disconnected.operations, stale.operations);
    assert!(disconnected.counts.is_none());
    assert_eq!(projection.observe(first, 22), Err(Rejected::OutOfOrder));
    let mut restarted = sample("a", 1, vec![]);
    restarted.generation = 2;
    restarted.producer_id = "replacement".into();
    projection.observe(restarted, 22).unwrap();
    assert_eq!(
        projection.observe(sample("a", 999, vec![]), 23),
        Err(Rejected::OutOfOrder)
    );
    assert_eq!(
        projection.snapshot(23).machines[0].counts,
        Some(Counts::default())
    );
}

#[test]
fn commands_reservations_and_wrappers_do_not_count_as_test_execution() {
    let mut projection = Projection::new(1, 8, 10);
    let child = operation("a", "child", Kind::Test, State::Running);
    let observations = vec![
        operation("a", "cargo", Kind::Command, State::Running),
        operation("a", "room", Kind::Reservation, State::Running),
        operation("a", "slot", Kind::Reservation, State::Queued),
        operation("a", "setup", Kind::Setup, State::Running),
        child.clone(),
        child,
    ];
    projection.observe(sample("a", 1, observations), 0).unwrap();
    let machine = projection.snapshot(0).machines.remove(0);
    assert_eq!(machine.operations.len(), 5);
    assert_eq!(
        machine.counts,
        Some(Counts {
            running_tests: 1,
            ..Counts::default()
        })
    );
}

#[test]
fn conflicting_reports_are_rejected_atomically_and_retain_unknown_last_work() {
    let mut projection = Projection::new(1, 3, 10);
    let child = operation("a", "child", Kind::Test, State::Running);
    projection
        .observe(sample("a", 1, vec![child.clone()]), 0)
        .unwrap();
    let mut other = child.clone();
    other.handle = Some("imposter".into());
    assert_eq!(
        projection.observe(sample("a", 2, vec![other.clone()]), 1),
        Err(Rejected::Ownership)
    );
    assert_eq!(
        projection.observe(sample("a", 2, vec![child.clone(), other]), 1),
        Err(Rejected::ConflictingOperation)
    );
    let mut foreign = child.clone();
    foreign.machine_id = Some("b".into());
    assert_eq!(
        projection.observe(sample("a", 2, vec![foreign]), 1),
        Err(Rejected::Ownership)
    );
    let machine = projection.snapshot(1).machines.remove(0);
    assert_eq!(machine.operations, vec![child]);
    assert_eq!(machine.freshness, Freshness::Conflict);
    assert!(machine.counts.is_none());
    projection.observe(sample("a", 2, vec![]), 2).unwrap();
    assert_eq!(
        projection.snapshot(2).machines[0].freshness,
        Freshness::Fresh
    );
}

#[test]
fn report_bounds_and_malformed_ids_do_not_mutate_the_inventory() {
    let mut projection = Projection::new(1, 1, 10);
    assert_eq!(
        projection.observe(sample("", 1, vec![]), 0),
        Err(Rejected::Identity)
    );
    let op = operation("a", "child", Kind::Build, State::Running);
    assert_eq!(
        projection.observe(sample("a", 1, vec![op.clone(), op.clone()]), 0),
        Err(Rejected::Limit)
    );
    let mut invalid = op.clone();
    invalid.parent_id = Some("child".into());
    assert_eq!(
        projection.observe(sample("a", 1, vec![invalid]), 0),
        Err(Rejected::Ownership)
    );
    let malformed: [fn(&mut Operation); 7] = [
        |o| o.operation_id = None,
        |o| o.operation_id = Some(" ".into()),
        |o| o.parent_id = Some("bad\nparent".into()),
        |o| o.handle = Some("".into()),
        |o| o.task = Some("".into()),
        |o| o.phase_since = Some(f64::NAN),
        |o| o.reason = "x".repeat(4097),
    ];
    for corrupt in malformed {
        let mut invalid = op.clone();
        corrupt(&mut invalid);
        assert_eq!(
            projection.observe(sample("a", 1, vec![invalid]), 0),
            Err(Rejected::Ownership)
        );
    }
    projection.observe(sample("a", 1, vec![op]), 0).unwrap();
    let mut invalid_publisher = sample("a", 2, vec![]);
    invalid_publisher.producer_id.clear();
    assert_eq!(
        projection.observe(invalid_publisher, 1),
        Err(Rejected::Identity)
    );
    assert!(projection.snapshot(1).machines[0].counts.is_none());
    assert_eq!(
        projection.observe(
            sample(
                "a",
                2,
                vec![
                    operation("a", "one", Kind::Build, State::Running),
                    operation("a", "two", Kind::Build, State::Running),
                ]
            ),
            1
        ),
        Err(Rejected::Limit)
    );
    assert_eq!(
        projection.observe(sample("b", 1, vec![]), 1),
        Err(Rejected::Limit)
    );
    let mut partial = sample(
        "a",
        2,
        vec![operation("a", "second", Kind::Build, State::Running)],
    );
    partial.complete = false;
    assert_eq!(projection.observe(partial, 1), Err(Rejected::Limit));
    assert_eq!(projection.snapshot(1).machines[0].sequence, 1);
    assert!(projection.snapshot(1).machines[0].counts.is_none());
}
