//! `verdict`, on a task in review and on an instruction subtask.

use crate::common::{Fixture, NOW, Seed, refused};
use symphony_board::health::LIVE_SECONDS;

/// `sim/jump` in review: `ochre-owl` orchestrates, `amber-heron` executed it, `brisk-finch` reviews it.
fn in_review() -> Fixture {
    Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "review")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    )
}

#[test]
fn three_sessions_hold_one_task_at_once() {
    let f = in_review();
    let task = f.store.get("sim/jump").unwrap();
    assert_eq!(task.status, "review");
    assert_eq!(
        (
            task.holders("orchestrator"),
            task.holders("executor"),
            task.holders("reviewer")
        ),
        (vec!["ochre-owl"], vec!["amber-heron"], vec!["brisk-finch"])
    );
}

#[test]
fn approve_finishes_the_task_on_the_record() {
    let f = in_review();
    f.advance(5.0);
    let task = f
        .store
        .verdict("sim/jump", "brisk-finch", "approve", "  looks right ")
        .unwrap();
    assert_eq!((task.status.as_str(), task.updated), ("done", NOW + 5.0));
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (
            last.sender.as_str(),
            last.kind.as_str(),
            last.text.as_str(),
            last.recipient
        ),
        ("brisk-finch", "verdict", "approve: looks right", None)
    );
}

#[test]
fn changes_sends_it_back_to_the_executor() {
    let f = in_review();
    let task = f
        .store
        .verdict("sim/jump", "brisk-finch", "changes", "the hash moved")
        .unwrap();
    assert_eq!(task.status, "in_progress");
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (
            last.kind.as_str(),
            last.text.as_str(),
            last.recipient.as_deref()
        ),
        ("verdict", "changes: the hash moved", Some("@executor"))
    );
}

#[test]
fn changes_for_an_executor_gone_quiet_tell_the_orchestrator_and_an_approve_does_not() {
    for verdict in ["approve", "changes"] {
        let f = in_review();
        f.advance(LIVE_SECONDS + 1.0);
        f.store
            .verdict("sim/jump", "brisk-finch", verdict, "the hash moved")
            .unwrap();
        let told: Vec<(String, Option<String>, String)> = f
            .store
            .thread("sim/jump")
            .unwrap()
            .into_iter()
            .filter(|m| m.sender == "board")
            .map(|m| (m.kind, m.recipient, m.text))
            .collect();
        let expected = (verdict == "changes").then(|| {
            (
                "msg".to_string(),
                Some("@orchestrator".to_string()),
                "the changes verdict from brisk-finch to @executor reaches no live executor: amber-heron is stale with no callback".to_string(),
            )
        });
        assert_eq!(told, Vec::from_iter(expected), "{verdict}");
    }
}

#[test]
fn a_verdict_without_words_is_only_the_verdict() {
    let f = in_review();
    f.store
        .verdict("sim/jump", "brisk-finch", "approve", "")
        .unwrap();
    assert_eq!(
        f.store.thread("sim/jump").unwrap().pop().unwrap().text,
        "approve"
    );
}

#[test]
fn only_the_reviewer_gives_a_verdict() {
    let f = in_review();
    for who in ["amber-heron", "ochre-owl"] {
        assert_eq!(
            refused(f.store.verdict("sim/jump", who, "approve", "")),
            format!("{who} is not a reviewer of sim/jump")
        );
    }
    assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
}

#[test]
fn a_verdict_must_be_approve_or_changes() {
    let f = in_review();
    assert_eq!(
        refused(f.store.verdict("sim/jump", "brisk-finch", "maybe", "")),
        "a verdict is approve or changes, not 'maybe'"
    );
}

#[test]
fn a_finished_task_takes_no_more_verdicts() {
    let f = in_review();
    f.store
        .verdict("sim/jump", "brisk-finch", "approve", "")
        .unwrap();
    assert_eq!(
        refused(f.store.verdict("sim/jump", "brisk-finch", "approve", "")),
        "sim/jump is done, not in review"
    );
}

#[test]
fn an_instruction_subtask_never_in_review_has_nothing_to_approve() {
    let mut subtask = crate::common::task("a/m", "in_progress");
    subtask.parent = Some("a".into());
    subtask.instruction = true;
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("a", "ready")
            .task_row(symphony_board::rows::TaskRow { id: 2, ..subtask })
            .role("a/m", "amber-heron", "executor")
            .role("a/m", "brisk-finch", "reviewer"),
    );
    assert_eq!(
        refused(f.store.verdict("a/m", "brisk-finch", "approve", "ok")),
        "a/m is in_progress, not in review"
    );
}

#[test]
fn concurrent_verdicts_land_once() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .session("coral-hare")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer")
            .role("sim/jump", "coral-hare", "reviewer"),
    );
    let barrier = std::sync::Barrier::new(2);
    let outcomes: Vec<Result<symphony_board::Task, symphony_board::Error>> =
        std::thread::scope(|scope| {
            let reviews: Vec<_> = [("brisk-finch", "approve"), ("coral-hare", "changes")]
                .into_iter()
                .map(|(who, verdict)| {
                    let (store, barrier) = (f.other(), &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        store.verdict("sim/jump", who, verdict, "")
                    })
                })
                .collect();
            reviews.into_iter().map(|r| r.join().unwrap()).collect()
        });
    assert_eq!(
        outcomes.iter().filter(|o| o.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    let verdicts = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "verdict")
        .count();
    assert_eq!(verdicts, 1);
}
