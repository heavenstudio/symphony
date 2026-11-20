//! What an attempt's failure nothing answers says, and which refusals of its answer may pass.

use super::*;

/// Attempt `ab12`'s failure at verify/checking, run as `handle`'s checker, or as no session's.
fn failure(handle: Option<&str>) -> Observation {
    Observation {
        attempt: "ab12".into(),
        trigger: Trigger::AttemptFailure,
        role: "checker".into(),
        handle: handle.map(str::to_string),
        revision: 6,
        at: Pair {
            phase: "verify".into(),
            state: "checking".into(),
        },
    }
}

#[test]
fn a_refusal_may_pass_unless_its_pin_went_stale_or_no_workflow_holds_the_task() {
    for passing in [
        "dependency: sim/a depends on sim/b, which is at build/available",
        "capacity: sim/a cannot move to build/working by its recovery",
        "explicit_action_required: verify/checking declares no attempt_failure recovery",
        "unauthorized: no authority of edge rework is met",
        "unsupported_adapter: edge rework needs the adapter git",
    ] {
        assert!(may_pass(passing), "{passing}");
    }
    for silent in [
        "stale_task: sim/a is at build/working (workflow revision 7), not build/working (revision 6)",
        "stale_workflow: the active workflow is d2, not d1",
        "stale_activation: the active generation is 3, not 2",
        "unknown_id: no custom workflow holds sim/a",
        "unsupported_activation: no custom workflow is active on this board: the board's own \
         writers keep its tasks",
        // What carries no refusal's code is no refusal of an answer: it is the caller's.
        "there is no execution attempt ab12",
        "stale_task",
    ] {
        assert!(!may_pass(silent), "{silent}");
    }
}

#[test]
fn a_failure_nothing_answers_names_its_attempt_and_every_refusal() {
    let undeclared = "explicit_action_required: verify/checking declares no attempt_failure \
                      recovery: sim/a stays as it was";
    assert_eq!(
        unanswered("sim/a", &failure(None), None, undeclared),
        "the board's recovery: checker attempt ab12 failed, and nothing answers it: \
         explicit_action_required: verify/checking declares no attempt_failure recovery: sim/a \
         stays as it was; a person does (`just tasks workflow actions sim/a`)"
    );
    let refused = Refused {
        edge: "rework".into(),
        why: "dependency: sim/a depends on sim/b, which is at build/available".into(),
    };
    assert_eq!(
        unanswered(
            "sim/a",
            &failure(Some("amber")),
            Some(&refused),
            "capacity: full"
        ),
        "the board's recovery: attempt ab12 of amber's checker failed, and nothing answers it: \
         the board could not take rework (dependency: sim/a depends on sim/b, which is at \
         build/available), nor its position's attempt_failure action (capacity: full); a person \
         does (`just tasks workflow actions sim/a`)"
    );
}

#[test]
fn every_answer_to_an_attempt_is_keyed_by_it() {
    assert_eq!(key("ab12"), "attempt/ab12");
    let keyed = |key: Option<&str>, recovered: bool| Transition {
        key: key.map(str::to_string),
        recovery: recovered.then(|| failure(None).recovered()),
        ..crate::workflow::recover::attempt_tests::past(3, None, None)
    };
    let transitions = [
        keyed(Some("attempt/ab12"), false),
        keyed(Some("attempt/ab1"), true),
        keyed(Some("attempt/ab12"), true),
    ];
    // A caller's request under the same key answers nothing: only what the board recovered does.
    assert_eq!(answering(&transitions, "ab12"), Some(&transitions[2]));
    assert_eq!(answering(&transitions[..2], "ab12"), None);
}
