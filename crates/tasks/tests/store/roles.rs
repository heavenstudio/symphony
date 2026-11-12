//! Joining and releasing a task, moving it along, and landing it.

use crate::common::{Fixture, NOW, Seed, refused};
use std::collections::BTreeSet;
use symphony_board::health::RESERVE_SECONDS;
use symphony_board::{LANDS_FROM, NewTask};

/// An orchestrator, an executor and a reviewer on `sim/jump`, in review, each joined through the store.
fn in_review() -> (Fixture, String, String, String) {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    f.task("sim/jump");
    f.store
        .take_role("sim/jump", &orch, "orchestrator")
        .unwrap();
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
    (f, orch, exe, rev)
}

fn one_task() -> (Fixture, String, String) {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    (f, a, b)
}

#[test]
fn three_sessions_hold_one_task_at_once_each_on_the_record() {
    let (f, orch, exe, rev) = in_review();
    let task = f.store.get("sim/jump").unwrap();
    assert_eq!(task.status, "review");
    assert_eq!(
        (
            task.holders("orchestrator"),
            task.holders("executor"),
            task.holders("reviewer")
        ),
        (vec![orch.as_str()], vec![exe.as_str()], vec![rev.as_str()])
    );
    assert_eq!(
        f.said("sim/jump"),
        [
            "created by owner".to_string(),
            format!("{orch} joined as orchestrator"),
            format!("{exe} claimed it"),
            format!("{exe} moved it in_progress → review"),
            format!("{rev} joined as reviewer"),
        ]
    );
}

#[test]
fn joining_again_changes_nothing() {
    let (f, orch, _, _) = in_review();
    let before = f.store.get("sim/jump").unwrap();
    let again = f
        .store
        .take_role("sim/jump", &orch, "orchestrator")
        .unwrap();
    assert_eq!(again.roles, before.roles);
    assert_eq!(f.said("sim/jump").len(), 5);
}

#[test]
fn the_executor_cannot_review_its_own_work() {
    let (f, _, exe, _) = in_review();
    assert_eq!(
        refused(f.store.take_role("sim/jump", &exe, "reviewer")),
        format!("{exe} executes sim/jump and cannot review its own work")
    );
}

#[test]
fn take_role_is_not_a_back_door_to_claim_or_reserve() {
    let (f, orch, _, _) = in_review();
    for role in ["executor", "reserved", "boss"] {
        assert_eq!(
            refused(f.store.take_role("sim/jump", &orch, role)),
            format!("take_role is for orchestrator or reviewer; use claim or reserve for '{role}'")
        );
    }
}

#[test]
fn a_finished_task_takes_no_new_roles_verdicts_or_reservations() {
    let (f, orch, _, rev) = in_review();
    f.store.verdict("sim/jump", &rev, "approve", "").unwrap();
    assert_eq!(
        refused(f.store.take_role("sim/jump", &orch, "reviewer")),
        "sim/jump is done"
    );
    assert_eq!(
        refused(f.store.verdict("sim/jump", &rev, "approve", "")),
        "sim/jump is done, not in review"
    );
    assert_eq!(
        refused(f.store.reserve("sim/jump", &orch, RESERVE_SECONDS)),
        "sim/jump is done"
    );
    f.task("sim/other");
    f.store.move_to("sim/other", "cancelled", &orch).unwrap();
    assert_eq!(
        refused(f.store.take_role("sim/other", &orch, "orchestrator")),
        "sim/other is cancelled"
    );
}

#[test]
fn releasing_one_role_keeps_the_others_on_the_record() {
    let (f, orch, _, _) = in_review();
    f.store.take_role("sim/jump", &orch, "reviewer").unwrap();
    let task = f
        .store
        .release("sim/jump", &orch, Some("reviewer"))
        .unwrap();
    assert_eq!(task.holders("orchestrator"), [orch.as_str()]);
    assert!(!task.holders("reviewer").contains(&orch.as_str()));
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("{orch} released reviewer (released)")
    );
    // A role it does not hold is refused naming the ones it does, and how to let them go, on
    // this board.
    let board = f.store.board_command();
    assert_eq!(
        refused(f.store.release("sim/jump", &orch, Some("reviewer"))),
        format!(
            "{orch} holds no reviewer on sim/jump, but orchestrator: {board} release sim/jump \
             --role orchestrator"
        )
    );
    f.store.take_role("sim/jump", &orch, "reviewer").unwrap();
    assert_eq!(
        refused(f.store.release("sim/jump", &orch, Some("executor"))),
        format!(
            "{orch} holds no executor on sim/jump, but orchestrator and reviewer: {board} release \
             sim/jump --role ROLE lets one go, {board} release sim/jump every one"
        )
    );
    let stranger = f.session("worker");
    for role in [None, Some("reviewer")] {
        assert_eq!(
            refused(f.store.release("sim/jump", &stranger, role)),
            format!("{stranger} holds no {} on sim/jump", role.unwrap_or("role"))
        );
    }
}

#[test]
fn release_frees_the_task_back_to_ready() {
    let (f, a, b) = one_task();
    f.store.claim("sim/jump", &a).unwrap();
    f.advance(5.0);
    let task = f.store.release("sim/jump", &a, None).unwrap();
    assert_eq!(
        (task.status.as_str(), task.updated, task.roles.len()),
        ("ready", f.now(), 0)
    );
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("{a} released every role (released)")
    );
    f.store.claim("sim/jump", &b).unwrap();
}

#[test]
fn releasing_work_in_review_keeps_its_status() {
    let (f, _, exe, _) = in_review();
    let task = f.store.release("sim/jump", &exe, None).unwrap();
    assert_eq!(
        (task.status.as_str(), task.holders("executor").len()),
        ("review", 0)
    );
}

#[test]
fn claiming_concept_or_design_work_keeps_its_stage() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task_with(NewTask {
        status: "design".into(),
        ..NewTask::new("sim/jump", "Jump departure", "owner")
    });
    assert_eq!(f.store.claim("sim/jump", &a).unwrap().status, "design");
    assert_eq!(
        refused(f.store.move_to("sim/jump", "ready", &b)),
        format!("{b} holds no role on sim/jump, which is design")
    );
    f.store.move_to("sim/jump", "ready", &a).unwrap();
    f.store.release("sim/jump", &a, None).unwrap();
    assert_eq!(f.store.get("sim/jump").unwrap().status, "ready");
}

#[test]
fn a_move_follows_the_pipeline_on_the_record() {
    let (f, a, _) = one_task();
    f.store.claim("sim/jump", &a).unwrap();
    f.advance(5.0);
    let task = f.store.move_to("sim/jump", "review", &a).unwrap();
    assert_eq!((task.status.as_str(), task.updated), ("review", f.now()));
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (last.sender, last.kind, last.text),
        (
            a.clone(),
            "system".into(),
            format!("{a} moved it in_progress → review")
        )
    );
}

#[test]
fn a_move_off_the_pipeline_is_refused() {
    let (f, a, _) = one_task();
    // Each refusal says where the task goes from where it is.
    for status in ["review", "done", "doing"] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", status, &a)),
            format!(
                "sim/jump cannot go from ready to {status}; from ready it goes to backlog, \
                 concept, design, in_progress, blocked or cancelled"
            )
        );
    }
    assert_eq!(
        refused(f.store.move_to("sim/jump", "blocked", "ghost-owl")),
        "no session 'ghost-owl'; register first (`just tasks register`)"
    );
    assert_eq!(
        refused(f.store.move_to("sim/none", "blocked", &a)),
        "no task 'sim/none'"
    );
    assert_eq!(f.store.get("sim/jump").unwrap().updated, NOW);
}

#[test]
fn a_move_the_pipeline_refuses_names_the_command_that_takes_the_task_on() {
    let (f, _, exe, rev) = in_review();
    let before = f.store.dump().unwrap();
    // On this board: a test's is a database of its own, which the command names.
    let board = f.store.board_command();
    assert!(board.starts_with("just tasks --database "), "{board}");
    for who in [&exe, &rev] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", "done", who)),
            format!(
                "sim/jump cannot go from review to done; its reviewer closes it with: {board} \
                 verdict sim/jump approve"
            )
        );
    }
    assert_eq!(f.store.dump().unwrap(), before);
    for status in ["done", "cancelled"] {
        let f = Fixture::new(Seed::default().task("sim/jump", status));
        let a = f.session("worker");
        assert_eq!(
            refused(f.store.move_to("sim/jump", "ready", &a)),
            format!("sim/jump cannot go from {status} to ready; a {status} task moves no further")
        );
    }
}

#[test]
fn owner_cancellation_without_a_session_preserves_other_fields_and_records_history() {
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "blocked",
        "review",
        "done",
        "cancelled",
    ] {
        let f = Fixture::new(Seed::default().task("sim/jump", status));
        let before = f.store.dump().unwrap();
        let mut expected = f.store.get("sim/jump").unwrap();
        f.advance(1.0);
        if matches!(status, "done" | "cancelled") {
            assert_eq!(
                refused(f.store.move_to("sim/jump", "cancelled", "owner")),
                format!("sim/jump cannot go from {status} to cancelled")
            );
            assert_eq!(f.store.dump().unwrap(), before);
        } else {
            expected.status = "cancelled".into();
            expected.updated = f.now();
            assert_eq!(
                f.store.move_to("sim/jump", "cancelled", "owner").unwrap(),
                expected
            );
            assert_eq!(f.store.get("sim/jump").unwrap(), expected);
            assert!(f.store.dump().unwrap().sessions.is_empty());
            let history = f.store.thread("sim/jump").unwrap();
            assert_eq!(history.len(), 1);
            let line = &history[0];
            assert_eq!(line.sender, "owner");
            assert_eq!(line.kind, "system");
            assert_eq!(line.recipient, None);
            assert_eq!(line.text, format!("owner moved it {status} → cancelled"));
        }
    }
}

#[test]
fn owner_cancellation_grants_no_other_moves_and_refuses_missing_tasks() {
    let f = Fixture::new(Seed::default().task("sim/jump", "ready"));
    let before = f.store.dump().unwrap();
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "blocked",
        "done",
        "unknown",
    ] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", status, "owner")),
            "no session 'owner'; register first (`just tasks register`)"
        );
        assert_eq!(f.store.dump().unwrap(), before);
    }
    // The owner's other move, back to review, starts only from blocked work.
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", "owner")),
        "sim/jump cannot go from ready to review"
    );
    assert_eq!(f.store.dump().unwrap(), before);
    assert_eq!(
        refused(f.store.move_to("sim/missing", "cancelled", "owner")),
        "no task 'sim/missing'"
    );
    assert_eq!(f.store.dump().unwrap(), before);
}

#[test]
fn the_owner_and_a_role_holder_return_blocked_work_to_review_but_never_an_instruction() {
    let mut instruction = crate::common::task("sim/jump/step", "blocked");
    instruction.id = 2;
    instruction.parent = Some("sim/jump".into());
    instruction.instruction = true;
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "blocked")
            .task_row(instruction)
            .task("sim/held", "blocked")
            .role("sim/held", "amber-heron", "executor"),
    );
    let before = f.store.dump().unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/jump/step", "review", "owner")),
        "sim/jump/step is a subtask: integrate into its parent and close it with `done`, not review"
    );
    assert_eq!(f.store.dump().unwrap(), before);
    f.advance(1.0);
    let back = f.store.move_to("sim/jump", "review", "owner").unwrap();
    assert_eq!((back.status.as_str(), back.updated), ("review", f.now()));
    assert_eq!(f.said("sim/jump"), ["owner moved it blocked → review"]);
    assert_eq!(f.store.dump().unwrap().sessions, before.sessions);
    assert_eq!(
        f.store
            .move_to("sim/held", "review", "amber-heron")
            .unwrap()
            .status,
        "review"
    );
    assert_eq!(
        f.said("sim/held"),
        ["amber-heron moved it blocked → review"]
    );
}

#[test]
fn concurrent_owner_cancellations_record_exactly_one_transition() {
    let f = Fixture::new(Seed::default().task("sim/jump", "in_progress"));
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let first = f.other();
        let second = f.other();
        let barrier = &barrier;
        let a = scope.spawn(move || {
            barrier.wait();
            first.move_to("sim/jump", "cancelled", "owner")
        });
        let b = scope.spawn(move || {
            barrier.wait();
            second.move_to("sim/jump", "cancelled", "owner")
        });
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    for result in results {
        if let Err(error) = result {
            assert_eq!(
                refused::<symphony_board::Task>(Err(error)),
                "sim/jump cannot go from cancelled to cancelled"
            );
        }
    }
    assert_eq!(f.store.get("sim/jump").unwrap().status, "cancelled");
    assert_eq!(
        f.said("sim/jump"),
        ["owner moved it in_progress → cancelled"]
    );
}

#[test]
fn unstarted_work_nobody_holds_is_moved_by_anyone() {
    let f = Fixture::empty();
    let stranger = f.session("worker");
    f.task_with(NewTask {
        status: "backlog".into(),
        ..NewTask::new("sim/jump", "Jump departure", "owner")
    });
    assert_eq!(
        f.store
            .move_to("sim/jump", "ready", &stranger)
            .unwrap()
            .status,
        "ready"
    );
}

#[test]
fn moving_a_started_task_needs_a_role_on_it() {
    let (f, a, b) = one_task();
    f.store.claim("sim/jump", &a).unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/jump", "blocked", &b)),
        format!("{b} holds no role on sim/jump, which is in_progress")
    );
}

#[test]
fn landing_finishes_a_task_in_review_for_a_role_holder() {
    let (f, _, exe, _) = in_review();
    f.advance(5.0);
    let landed = f
        .store
        .landed("sim/jump", &exe, "  landed abc123 ", false, &["review"])
        .unwrap();
    assert_eq!(
        (
            landed.task.status.as_str(),
            landed.task.updated,
            landed.from.as_str()
        ),
        ("done", f.now(), "review")
    );
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (last.sender, last.kind, last.text, last.recipient),
        (exe, "verdict".into(), "landed abc123".into(), None)
    );
}

#[test]
fn landing_is_refused_to_a_session_without_a_role_or_off_review() {
    let (f, _, exe, _) = in_review();
    let stranger = f.session("worker");
    assert_eq!(
        refused(
            f.store
                .landed("sim/jump", &stranger, "x", false, &LANDS_FROM)
        ),
        format!("{stranger} holds no role on sim/jump")
    );
    f.store.release("sim/jump", &exe, None).unwrap();
    f.task("sim/other");
    assert_eq!(
        refused(f.store.landed("sim/other", &exe, "x", false, &LANDS_FROM)),
        "sim/other is ready, which this landing does not close"
    );
}

#[test]
fn landing_is_also_taken_by_the_session_that_asked_to_land_it() {
    let (f, _, _, _) = in_review();
    let asked = f.session("orchestrator");
    assert_eq!(
        refused(f.store.landed("sim/jump", &asked, "x", false, &LANDS_FROM)),
        format!("{asked} holds no role on sim/jump")
    );
    assert_eq!(
        refused(
            f.store
                .landed("sim/jump", "nobody-at-all", "x", true, &LANDS_FROM)
        ),
        "no session 'nobody-at-all'; register first (`just tasks register`)"
    );
    f.store
        .landed("sim/jump", &asked, "landed abc123", true, &LANDS_FROM)
        .unwrap();
    assert_eq!(f.store.get("sim/jump").unwrap().status, "done");
}

#[test]
fn landing_finishes_work_held_blocked_or_taken_back_in_progress_but_never_work_not_started() {
    let (f, _, exe, _) = in_review();
    let asked = f.session("orchestrator");
    f.store.move_to("sim/jump", "blocked", &exe).unwrap();
    let landed = f
        .store
        .landed("sim/jump", &asked, "landed abc123", true, &LANDS_FROM)
        .unwrap();
    assert_eq!(
        (landed.task.status.as_str(), landed.from.as_str()),
        ("done", "blocked")
    );
    // Taken back from review as its branch landed: land closes it, as no other command does.
    f.task("sim/other");
    f.store.claim("sim/other", &exe).unwrap();
    f.store.move_to("sim/other", "review", &exe).unwrap();
    f.store.move_to("sim/other", "in_progress", &exe).unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/other", "done", &exe)),
        format!(
            "sim/other cannot go from in_progress to done; it goes to review first: {} move \
             sim/other review",
            f.store.board_command()
        )
    );
    let landed = f
        .store
        .landed("sim/other", &asked, "landed def456", true, &LANDS_FROM)
        .unwrap();
    assert_eq!(
        (landed.task.status.as_str(), landed.from.as_str()),
        ("done", "in_progress")
    );
    for status in ["backlog", "concept", "design"] {
        let slug = format!("sim/{status}");
        f.task(&slug);
        f.store.move_to(&slug, status, &exe).unwrap();
        assert_eq!(
            refused(f.store.landed(&slug, &asked, "x", true, &LANDS_FROM)),
            format!("{slug} is {status}, which this landing does not close")
        );
    }
}

/// Land names what this landing closes, and the board checks it against the status its write reads:
/// a task that moved after land's read is closed only if it moved to one land named.
#[test]
fn landing_closes_only_from_the_statuses_named_and_never_names_one_it_cannot_close() {
    let (f, _, exe, _) = in_review();
    let asked = f.session("orchestrator");
    assert_eq!(
        refused(
            f.store
                .landed("sim/jump", &asked, "x", true, &["review", "ready"])
        ),
        "a landing closes a task in review, blocked or in_progress, not ready"
    );
    f.store.move_to("sim/jump", "in_progress", &exe).unwrap();
    for from in [&["review", "blocked"][..], &[][..]] {
        assert_eq!(
            refused(f.store.landed("sim/jump", &asked, "x", true, from)),
            "sim/jump is in_progress, which this landing does not close"
        );
    }
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    assert_eq!(
        refused(
            f.store
                .landed("sim/jump", &asked, "x", true, &["in_progress"])
        ),
        "sim/jump is review, which this landing does not close"
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
}

#[test]
fn every_session_that_ever_executed_is_remembered() {
    let (f, _, exe, rev) = in_review();
    f.store
        .verdict("sim/jump", &rev, "changes", "again")
        .unwrap();
    f.store.release("sim/jump", &exe, None).unwrap();
    let again = f.session("worker");
    f.store.claim("sim/jump", &again).unwrap();
    assert_eq!(
        f.store.ever_executed("sim/jump").unwrap(),
        BTreeSet::from([exe, again])
    );
    assert_eq!(
        refused(f.store.ever_executed("sim/none")),
        "no task 'sim/none'"
    );
}

#[test]
fn a_subtask_made_for_a_handle_is_reserved_without_a_lapse() {
    let f = Fixture::empty();
    let (agent, other) = (f.session("worker"), f.session("worker"));
    f.task("a");
    let made = f.task_with(NewTask {
        parent: Some("a".into()),
        assignee: Some(agent.clone()),
        ..NewTask::new("a/review", "Review", "owner")
    });
    let reserved: Vec<_> = made.roles.iter().filter(|r| r.role == "reserved").collect();
    assert_eq!(reserved.len(), 1);
    assert_eq!(
        (reserved[0].handle.as_str(), reserved[0].expires),
        (agent.as_str(), None)
    );
    assert_eq!(
        f.said("a/review"),
        [format!("created by owner for {agent}")]
    );
    f.advance(RESERVE_SECONDS * 3.0);
    f.store.touch(&agent).unwrap();
    f.store.touch(&other).unwrap();
    assert_eq!(
        refused(f.store.claim("a/review", &other)),
        format!("a/review is reserved by {agent}")
    );
    assert_eq!(
        f.store
            .claim("a/review", &agent)
            .unwrap()
            .holders("executor"),
        [agent.as_str()]
    );
}
