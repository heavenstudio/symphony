//! Oversized stale headers cannot poison accepted nonce history or renew its receipt.

use super::{operation, sample};
use crate::cli::machine_work::{Counts, Freshness, Kind, Operation, Projection, Rejected, State};

#[derive(Clone, Copy, Debug)]
enum Case {
    RetiredGeneration,
    ReplayedSequence,
    OlderSequence,
    RegressedReceipt,
    OtherProducer,
    CurrentOverflow,
}

#[derive(Clone, Copy)]
enum Moment {
    Rejected,
    Next,
}

fn assert_current(receiver: &Projection, retained: &Operation, case: Case, moment: Moment) {
    let exhausted = matches!(case, Case::CurrentOverflow);
    let recovered = matches!(moment, Moment::Next) && !exhausted;
    let now = match moment {
        Moment::Rejected => 21,
        Moment::Next => 22,
    };
    let view = receiver.snapshot(now);
    assert_eq!(view.machines.len(), 1, "{case:?}");
    let machine = &view.machines[0];
    assert_eq!(
        machine.operations,
        if recovered {
            vec![]
        } else {
            vec![retained.clone()]
        },
        "{case:?}"
    );
    assert_eq!(machine.sequence, if recovered { 6 } else { 5 }, "{case:?}");
    assert_eq!(machine.producer_id, "current", "{case:?}");
    assert_eq!(
        machine.age_ms,
        Some(if recovered { 0 } else { now - 20 }),
        "{case:?}"
    );
    assert_eq!(
        machine.freshness,
        if exhausted {
            Freshness::Conflict
        } else {
            Freshness::Fresh
        },
        "{case:?}"
    );
    assert_eq!(
        machine.counts,
        if exhausted {
            None
        } else {
            Some(Counts {
                running_tests: if recovered { 0 } else { 1 },
                ..Counts::default()
            })
        },
        "{case:?}"
    );
}

#[test]
fn oversized_headers_are_fenced_before_the_current_publishers_history_can_be_exhausted() {
    for case in [
        Case::RetiredGeneration,
        Case::ReplayedSequence,
        Case::OlderSequence,
        Case::RegressedReceipt,
        Case::OtherProducer,
        Case::CurrentOverflow,
    ] {
        let mut receiver = Projection::new(1, 1, 10);
        let retained = operation("a", "accepted", Kind::Test, State::Running);
        let mut current = sample("a", 5, vec![retained.clone()]);
        current.generation = 2;
        current.producer_id = "current".into();
        receiver.observe(current.clone(), 20).unwrap();
        let mut oversized = current.clone();
        oversized.sequence = 6;
        oversized
            .operations
            .push(operation("a", "unseen", Kind::Test, State::Running));
        let receipt = match case {
            Case::RetiredGeneration => {
                oversized.generation = 1;
                oversized.producer_id = "retired".into();
                21
            }
            Case::ReplayedSequence => {
                oversized.sequence = 5;
                21
            }
            Case::OlderSequence => {
                oversized.sequence = 4;
                21
            }
            Case::OtherProducer => {
                oversized.producer_id = "other".into();
                21
            }
            Case::RegressedReceipt => 19,
            Case::CurrentOverflow => 21,
        };
        let rejected = match case {
            Case::RegressedReceipt => Rejected::ReceiverClock,
            Case::CurrentOverflow => Rejected::Limit,
            _ => Rejected::OutOfOrder,
        };
        assert_eq!(
            receiver.observe(oversized, receipt),
            Err(rejected),
            "{case:?}"
        );
        assert_current(&receiver, &retained, case, Moment::Rejected);
        current.sequence = 6;
        current.operations.clear();
        assert_eq!(
            receiver.observe(current, 22),
            if matches!(case, Case::CurrentOverflow) {
                Err(Rejected::Limit)
            } else {
                Ok(())
            },
            "{case:?}"
        );
        assert_current(&receiver, &retained, case, Moment::Next);
    }
}
