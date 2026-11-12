//! `verdict`, on a task in review and on an instruction subtask, and whose it is: never one that
//! executed the task or a work slice under it, while the executor of an instruction under it, its
//! reviewer's own review subtask say, gives it.

use std::collections::BTreeSet;

use crate::common::{Fixture, NOW, Seed, refused};
use symphony_board::NewTask;
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
    assert_eq!(
        refused(f.store.verdict("sim/jump", "ochre-owl", "approve", "")),
        "ochre-owl is not a reviewer of sim/jump"
    );
    assert_eq!(
        refused(f.store.verdict("sim/jump", "amber-heron", "approve", "")),
        "amber-heron executed sim/jump: its verdict is a fresh reviewer's, one that did not write \
         or execute it"
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
}

/// RULES.md `workflow/reviewers-not-the-owner`: the session that executed a task never judges it,
/// though it released the executor's role and took the reviewer's.
#[test]
fn a_session_that_executed_the_task_gives_it_no_verdict_whatever_role_it_holds_now() {
    let f = Fixture::empty();
    let exe = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.release("sim/jump", &exe, Some("executor")).unwrap();
    let task = f.store.take_role("sim/jump", &exe, "reviewer").unwrap();
    assert_eq!(task.holders("reviewer"), [exe.as_str()]);
    for verdict in ["approve", "changes"] {
        assert_eq!(
            refused(f.store.verdict("sim/jump", &exe, verdict, "mine is fine")),
            format!(
                "{exe} executed sim/jump: its verdict is a fresh reviewer's, one that did not \
                 write or execute it"
            )
        );
    }
    assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
    let fresh = f.session("reviewer");
    f.store.take_role("sim/jump", &fresh, "reviewer").unwrap();
    f.store.verdict("sim/jump", &fresh, "approve", "").unwrap();
}

/// `sim/jump` moved to review on a fresh board: its orchestrator and its executor.
fn moved_to_review() -> (Fixture, String, String) {
    let f = Fixture::empty();
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    f.task("sim/jump");
    f.store
        .take_role("sim/jump", &orch, "orchestrator")
        .unwrap();
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    (f, orch, exe)
}

/// `slug` under `parent`, an instruction or a work slice, filed by `by` and reserved for `whom`.
fn under(f: &Fixture, slug: &str, parent: &str, instruction: bool, by: &str, whom: Option<&str>) {
    f.task_with(NewTask {
        parent: Some(parent.into()),
        instruction,
        assignee: whom.map(str::to_string),
        ..NewTask::new(slug, "A step", by)
    });
}

fn executed(handle: &str) -> String {
    format!(
        "{handle} executed sim/jump: its verdict is a fresh reviewer's, one that did not write or \
         execute it"
    )
}

/// A reviewer staffed as the skills say: an instruction subtask `sim/jump/review` reserved for it,
/// which it claims, and the reviewer's role on `sim/jump`. Its claim of the instruction is no
/// execution of `sim/jump`, whose branch it never wrote: its verdict stands, before or after it
/// closes the instruction with `done`, and so does the instruction's orchestrator's.
#[test]
fn the_reviewer_that_claimed_its_review_instruction_gives_the_verdict() {
    for close_first in [false, true] {
        let (f, orch, exe) = moved_to_review();
        let rev = f.session("reviewer");
        under(&f, "sim/jump/review", "sim/jump", true, &orch, Some(&rev));
        f.store.claim("sim/jump/review", &rev).unwrap();
        // One that orchestrates only the instruction is not the task's orchestrator either.
        let errand = f.session("orchestrator");
        f.store
            .take_role("sim/jump/review", &errand, "orchestrator")
            .unwrap();
        f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
        let held = f.store.ever_held("sim/jump").unwrap();
        assert_eq!(held.executed, BTreeSet::from([exe.clone()]));
        assert_eq!(held.orchestrated, BTreeSet::from([orch.clone()]));
        // The instruction's own executor is its reviewer, as any task's is.
        assert_eq!(
            f.store.ever_executed("sim/jump/review").unwrap(),
            BTreeSet::from([rev.clone()])
        );
        if close_first {
            f.store
                .done("sim/jump/review", &rev, "approved, its tests rerun")
                .unwrap();
        }
        let task = f
            .store
            .verdict("sim/jump", &rev, "approve", "3e0c66192471: its tests pass")
            .unwrap();
        assert_eq!(task.status, "done", "closed first: {close_first}");
    }
}

/// Whoever executed a work slice of the task, at any depth, wrote its branch and gives it no
/// verdict, the instruction it holds besides or not; the task's own executor neither, though it
/// claimed the review instruction too. An instruction's executor is not counted wherever the
/// instruction stands, under a slice or under another instruction, and a slice under an
/// instruction is work all the same.
#[test]
fn whoever_executed_the_task_or_a_slice_at_any_depth_is_refused_and_an_instructions_executor_is_not()
 {
    let (f, orch, exe) = moved_to_review();
    let tree = [
        ("sim/jump/part", "sim/jump", false),
        ("sim/jump/part/deep", "sim/jump/part", false),
        ("sim/jump/part/check", "sim/jump/part", true),
        ("sim/jump/review", "sim/jump", true),
        ("sim/jump/review/ask", "sim/jump/review", true),
        ("sim/jump/review/fix", "sim/jump/review", false),
    ];
    let mut by = std::collections::BTreeMap::new();
    for (slug, parent, instruction) in tree {
        under(&f, slug, parent, instruction, &orch, None);
        let who = f.session("worker");
        f.store.claim(slug, &who).unwrap();
        by.insert(slug, who);
    }
    // The task's executor, and a slice's, claim a review instruction besides.
    let slice = by["sim/jump/part"].clone();
    for (slug, who) in [("sim/jump/again", &exe), ("sim/jump/also", &slice)] {
        under(&f, slug, "sim/jump", true, &orch, None);
        f.store.claim(slug, who).unwrap();
    }
    let work = ["sim/jump/part", "sim/jump/part/deep", "sim/jump/review/fix"];
    assert_eq!(
        f.store.ever_held("sim/jump").unwrap().executed,
        BTreeSet::from_iter(work.iter().map(|s| by[s].clone()).chain([exe.clone()]))
    );
    for slug in work {
        let who = &by[slug];
        f.store.take_role("sim/jump", who, "reviewer").unwrap();
        for verdict in ["approve", "changes"] {
            assert_eq!(
                refused(f.store.verdict("sim/jump", who, verdict, "")),
                executed(who),
                "{slug}"
            );
        }
    }
    f.store.release("sim/jump", &exe, Some("executor")).unwrap();
    f.store.take_role("sim/jump", &exe, "reviewer").unwrap();
    assert_eq!(
        refused(f.store.verdict("sim/jump", &exe, "approve", "")),
        executed(&exe)
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
    // An instruction's executor, here one under a slice, gives it.
    let check = &by["sim/jump/part/check"];
    f.store.take_role("sim/jump", check, "reviewer").unwrap();
    let task = f.store.verdict("sim/jump", check, "approve", "").unwrap();
    assert_eq!(task.status, "done");
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
