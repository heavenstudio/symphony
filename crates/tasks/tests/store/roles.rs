//! Joining and releasing a task, moving it along, and landing it.

use crate::common::{Fixture, GONE, NOW, Seed, refused};
use std::collections::BTreeSet;
use symphony_board::Health;
use symphony_board::health::{LIVE_SECONDS, RESERVE_SECONDS};
use symphony_board::{LANDS_FROM, NewTask};

/// An orchestrator, an executor and a reviewer on `sim/jump`, in review, each joined through the store.
fn in_review() -> (Fixture, String, String, String) {
    let f = Fixture::new(Seed::default());
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
    let f = Fixture::new(Seed::default());
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

/// One reviewer a task, each later round resumed: a second is refused while the first holds the
/// role and is still there, live or stale, after a changes as before it, naming it, how it lets the
/// role go and how one joins beside it; once it is lost the second takes over, beside a holder
/// whose verdict the board refuses. The first joins again, and a red's filer joins beside a
/// reviewer to cancel the red, as before.
#[test]
fn a_second_reviewer_is_refused_while_the_first_is_still_there() {
    let (f, _, exe, rev) = in_review();
    let second = f.session("reviewer");
    let board = f.store.board_command();
    let reviewed_by = |health: &str| {
        format!(
            "sim/jump is reviewed by {rev} ({health}): a later round resumes it, never another \
                 reviewer; it lets the role go with: {board} release sim/jump --role reviewer, or one \
                 joins beside it with: {board} role sim/jump reviewer --beside WHY"
        )
    };
    let before = f.rows();
    assert_eq!(
        refused(f.store.take_role("sim/jump", &second, "reviewer")),
        reviewed_by("live")
    );
    assert_eq!(f.rows(), before);
    f.store
        .verdict("sim/jump", &rev, "changes", "again")
        .unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        refused(f.store.take_role("sim/jump", &second, "reviewer")),
        reviewed_by("stale")
    );
    let again = f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
    assert_eq!(again.holders("reviewer"), [rev.as_str()]);
    f.advance(GONE);
    assert_eq!(f.health(&rev), Health::Lost);
    // Its executor, that role let go, holds the reviewer's too: it gives no verdict, so it keeps
    // no fresh reviewer out.
    f.store.release("sim/jump", &exe, Some("executor")).unwrap();
    f.store.take_role("sim/jump", &exe, "reviewer").unwrap();
    let taken = f.store.take_role("sim/jump", &second, "reviewer").unwrap();
    assert_eq!(
        BTreeSet::from_iter(taken.holders("reviewer")),
        BTreeSet::from([rev.as_str(), exe.as_str(), second.as_str()])
    );
    let filer = f.session("filer");
    f.store.take_role("sim/jump", &filer, "reviewer").unwrap();
    let closed = f.store.move_to("sim/jump", "cancelled", &filer).unwrap();
    assert_eq!(closed.status, "cancelled");
}

/// A reviewer joins beside one still there when it says why: its join line is the board's record as
/// ever, and its why a post of its own after it, naming those it joins beside. Refused with no
/// words, or for the orchestrator's role; with nobody there it joins with no post.
#[test]
fn a_reviewer_joins_beside_another_saying_why() {
    let (f, orch, _, rev) = in_review();
    let second = f.session("reviewer");
    let before = f.rows();
    assert_eq!(
        refused(
            f.store
                .take_role_beside("sim/jump", &second, "reviewer", Some("  "))
        ),
        "a reviewer joining beside another says why, in words"
    );
    assert_eq!(
        refused(
            f.store
                .take_role_beside("sim/jump", &orch, "orchestrator", Some("why"))
        ),
        "only a reviewer joins beside another, not 'orchestrator'"
    );
    assert_eq!(f.rows(), before);
    let said = f.store.thread("sim/jump").unwrap().len();
    let task = f
        .store
        .take_role_beside(
            "sim/jump",
            &second,
            "reviewer",
            Some(" the adversarial stamp "),
        )
        .unwrap();
    assert_eq!(
        BTreeSet::from_iter(task.holders("reviewer")),
        BTreeSet::from([rev.as_str(), second.as_str()])
    );
    let lines: Vec<(String, String, String)> = f.store.thread("sim/jump").unwrap()[said..]
        .iter()
        .map(|m| (m.sender.clone(), m.kind.clone(), m.text.clone()))
        .collect();
    assert_eq!(
        lines,
        [
            (
                second.clone(),
                "system".into(),
                format!("{second} joined as reviewer")
            ),
            (
                second.clone(),
                "msg".into(),
                format!("reviewer beside {rev} (live): the adversarial stamp")
            ),
        ]
    );
    // A third names both, as the board lists them; with nobody there, a reason joins as a plain
    // take does.
    let third = f.session("reviewer");
    f.store
        .take_role_beside("sim/jump", &third, "reviewer", Some("a third look"))
        .unwrap();
    let mut both = [&rev, &second];
    both.sort();
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!(
            "reviewer beside {} (live), {} (live): a third look",
            both[0], both[1]
        )
    );
    f.task("sim/other");
    let alone = f
        .store
        .take_role_beside("sim/other", &third, "reviewer", Some("why"))
        .unwrap();
    assert_eq!(alone.holders("reviewer"), [third.as_str()]);
    assert_eq!(
        f.said("sim/other").last().unwrap(),
        &format!("{third} joined as reviewer")
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
    // One session holding two roles: the reviewer's an older board gave the orchestrator, which
    // the board refuses it now.
    let f = Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .task("sim/jump", "review")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "ochre-owl", "reviewer"),
    );
    let orch = "ochre-owl";
    // A role it does not hold is refused naming the ones it does, and how to let them go, on
    // this board.
    let board = f.store.board_command();
    assert_eq!(
        refused(f.store.release("sim/jump", orch, Some("executor"))),
        format!(
            "{orch} holds no executor on sim/jump, but orchestrator and reviewer: {board} release \
                 sim/jump --role ROLE lets one go, {board} release sim/jump every one"
        )
    );
    let task = f.store.release("sim/jump", orch, Some("reviewer")).unwrap();
    assert_eq!(task.holders("orchestrator"), [orch]);
    assert!(!task.holders("reviewer").contains(&orch));
    assert_eq!(
        f.said("sim/jump").last().unwrap(),
        &format!("{orch} released reviewer (released)")
    );
    assert_eq!(
        refused(f.store.release("sim/jump", orch, Some("reviewer"))),
        format!(
            "{orch} holds no reviewer on sim/jump, but orchestrator: {board} release sim/jump \
                 --role orchestrator"
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
    let f = Fixture::new(Seed::default());
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
        f.no_session("ghost-owl")
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
    let before = f.rows();
    // On this board: a test's is a database of its own, which the command names.
    let board = f.store.board_command();
    assert!(board.starts_with("just tasks --database "), "{board}");
    for who in [&exe, &rev] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", "done", who)),
            format!(
                "sim/jump cannot go from review to done; its reviewer closes it with: {board} \
                     verdict sim/jump approve; or, when its work landed with another task's, \
                     {board} done sim/jump NOTE --covered-by OTHER"
            )
        );
    }
    assert_eq!(f.rows(), before);
    // A cancelled task goes nowhere; a done one only back to review while an approve stands on it,
    // which none does here: what it needs is a new task.
    for (status, next) in [
        (
            "done",
            "a done task no approve stands on, as a landing leaves it, moves no further: what it \
                 still needs goes in a new task, BOARD create SLUG TITLE",
        ),
        ("cancelled", "a cancelled task moves no further"),
    ] {
        let f = Fixture::new(Seed::default().task("sim/jump", status));
        let a = f.session("worker");
        let next = next.replace("BOARD", &f.store.board_command());
        assert_eq!(
            refused(f.store.move_to("sim/jump", "ready", &a)),
            format!("sim/jump cannot go from {status} to ready; {next}")
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
        let before = f.rows();
        let mut expected = f.store.get("sim/jump").unwrap();
        f.advance(1.0);
        if matches!(status, "done" | "cancelled") {
            assert_eq!(
                refused(f.store.move_to("sim/jump", "cancelled", "owner")),
                format!("sim/jump cannot go from {status} to cancelled")
            );
            assert_eq!(f.rows(), before);
        } else {
            expected.status = "cancelled".into();
            expected.updated = f.now();
            assert_eq!(
                f.store.move_to("sim/jump", "cancelled", "owner").unwrap(),
                expected
            );
            assert_eq!(f.store.get("sim/jump").unwrap(), expected);
            assert_eq!(f.store.raw().table("session").count().unwrap(), 0);
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
    let before = f.rows();
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
            f.no_session("owner")
        );
        assert_eq!(f.rows(), before);
    }
    // The owner's other move, back to review, starts only from blocked work.
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", "owner")),
        "sim/jump cannot go from ready to review"
    );
    assert_eq!(f.rows(), before);
    assert_eq!(
        refused(f.store.move_to("sim/missing", "cancelled", "owner")),
        "no task 'sim/missing'"
    );
    assert_eq!(f.rows(), before);
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
    let before = f.rows();
    assert_eq!(
        refused(f.store.move_to("sim/jump/step", "review", "owner")),
        "sim/jump/step is a subtask: integrate into its parent and close it with `done`, not review"
    );
    assert_eq!(f.rows(), before);
    f.advance(1.0);
    let back = f.store.move_to("sim/jump", "review", "owner").unwrap();
    assert_eq!((back.status.as_str(), back.updated), ("review", f.now()));
    assert_eq!(f.said("sim/jump"), ["owner moved it blocked → review"]);
    assert_eq!(f.rows()["session"], before["session"]);
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
    let f = Fixture::new(Seed::default());
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
        f.no_session("nobody-at-all")
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
            "sim/other cannot go from in_progress to done; it goes to review first: {board} move \
                 sim/other review; or, when its work landed with another task's, {board} done \
                 sim/other NOTE --covered-by OTHER",
            board = f.store.board_command()
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

/// The default `just land` lands a task its reviewer approved: done stays done, the landing's line
/// is written as its verdict, and only a landing that names done closes it from there.
#[test]
fn landing_an_approved_task_writes_its_line_and_leaves_it_done() {
    let (f, _, _, rev) = in_review();
    f.store
        .verdict("sim/jump", &rev, "approve", "good")
        .unwrap();
    let asked = f.session("orchestrator");
    assert_eq!(
        refused(
            f.store
                .landed("sim/jump", &asked, "x", true, &["review", "blocked"])
        ),
        "sim/jump is done, which this landing does not close"
    );
    let landed = f
        .store
        .landed(
            "sim/jump",
            &asked,
            "landed sim/jump on main at 3e0c66192471",
            true,
            &LANDS_FROM,
        )
        .unwrap();
    assert_eq!(
        (landed.task.status.as_str(), landed.from.as_str()),
        ("done", "done")
    );
    let thread = f.store.thread("sim/jump").unwrap();
    let texts: Vec<(&str, &str)> = thread
        .iter()
        .rev()
        .take(2)
        .map(|m| (m.kind.as_str(), m.text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            ("verdict", "landed sim/jump on main at 3e0c66192471"),
            ("verdict", "approve: good")
        ]
    );
    // No move: the stages keep the approval as the task's close.
    assert!(
        !thread.iter().any(|m| m.text.contains("done → done")),
        "{thread:?}"
    );
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
        "a landing closes a task in review, blocked, in_progress or done, not ready"
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

/// The notice a move from done back to review posts to `@executor` and `@reviewer`, by `handle`.
fn back_from_done(handle: &str) -> String {
    format!(
        "sim/jump is back in review from done, by {handle}: the approve standing does not cover \
         what is to land (the thread says why), so its reviewer approves the tip again, naming it"
    )
}

/// `slug`'s lines from the `from`th on: their sender, text and recipient.
fn lines_from(f: &Fixture, slug: &str, from: usize) -> Vec<(String, String, Option<String>)> {
    f.store.thread(slug).unwrap()[from..]
        .iter()
        .map(|m| (m.sender.clone(), m.text.clone(), m.recipient.clone()))
        .collect()
}

/// What the board said to `handle`, as its inbox has it.
fn told(f: &Fixture, handle: &str) -> Vec<String> {
    f.store
        .inbox(handle)
        .unwrap()
        .into_iter()
        .filter(|m| m.sender == "board")
        .map(|m| m.text)
        .collect()
}

/// A branch that gained commits after its approve, a merge of main to clear a conflict the landing
/// refused, say, goes back to review for a fresh approve: by its executor, reviewer or
/// orchestrator, and only while the approve stands, so never once a landing has closed it. Its
/// executor and reviewer are told, where their inbox and PULL digest find it.
#[test]
fn an_approved_task_no_landing_closed_goes_back_to_review_for_a_fresh_approve() {
    let (f, orch, exe, rev) = in_review();
    f.store
        .verdict("sim/jump", &rev, "approve", "good")
        .unwrap();
    let stranger = f.session("worker");
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", &stranger)),
        format!(
            "{stranger} is not sim/jump's executor, reviewer or orchestrator, which alone send it \
             back to review"
        )
    );
    let board = f.store.board_command();
    for status in ["in_progress", "ready", "blocked"] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", status, &exe)),
            format!(
                "sim/jump cannot go from done to {status}; a done task goes only back to review, \
                 and only one approved that no landing has closed: {board} move sim/jump review"
            )
        );
    }
    let said = f.said("sim/jump").len();
    let task = f.store.move_to("sim/jump", "review", &exe).unwrap();
    assert_eq!(task.status, "review");
    let notice = back_from_done(&exe);
    assert_eq!(
        lines_from(&f, "sim/jump", said),
        [
            (exe.clone(), format!("{exe} moved it done → review"), None),
            ("board".into(), notice.clone(), Some("@executor".into())),
            ("board".into(), notice.clone(), Some("@reviewer".into())),
        ]
    );
    assert!(told(&f, &rev).contains(&notice), "{:?}", told(&f, &rev));
    f.store
        .verdict("sim/jump", &rev, "approve", "4b5e7b0a, the merge of main")
        .unwrap();
    // The orchestrator may send it back too, and the reviewer.
    f.store.move_to("sim/jump", "review", &orch).unwrap();
    assert!(told(&f, &rev).contains(&back_from_done(&orch)));
    f.store
        .verdict("sim/jump", &rev, "approve", "again")
        .unwrap();
    f.store.move_to("sim/jump", "review", &rev).unwrap();
    f.store
        .verdict("sim/jump", &rev, "approve", "once more")
        .unwrap();
    f.store
        .landed(
            "sim/jump",
            &orch,
            "landed sim/jump on main at 3e0c66192471",
            true,
            &["done"],
        )
        .unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", &exe)),
        "sim/jump is done: only an approved task no landing has closed goes back to review, for \
         a fresh approve of its new tip"
    );
    // Its landing closed it: a refused move names no move back to review, which is refused too.
    for status in ["in_progress", "ready", "blocked"] {
        assert_eq!(
            refused(f.store.move_to("sim/jump", status, &exe)),
            closed_by_its_landing(&f, status)
        );
    }
}

/// What a move of `sim/jump` to `status` is told once its landing closed it: it moves no further,
/// and what it needs is a new task, on this board.
fn closed_by_its_landing(f: &Fixture, status: &str) -> String {
    format!(
        "sim/jump cannot go from done to {status}; a done task no approve stands on, as a landing \
         leaves it, moves no further: what it still needs goes in a new task, {} create SLUG TITLE",
        f.store.board_command()
    )
}

#[test]
fn a_task_closed_with_no_approve_standing_stays_done() {
    let (f, orch, exe, _) = in_review();
    // Landed from review, its landing its verdict.
    f.store
        .landed(
            "sim/jump",
            &orch,
            "landed sim/jump on main at 3e0c66192471",
            true,
            &["review"],
        )
        .unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", &exe)),
        "sim/jump is done: only an approved task no landing has closed goes back to review, for \
             a fresh approve of its new tip"
    );
    assert_eq!(
        refused(f.store.move_to("sim/jump", "ready", &orch)),
        closed_by_its_landing(&f, "ready")
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "done");
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

/// Whoever executed work under the task (a work subtask, or one under that) wrote what its branch
/// holds: it gives the task no verdict (an instruction's executor: `verdict.rs`). Whoever orchestrated the task, now or before it handed the
/// role over, is remembered too; a subtask's orchestrator is not the task's.
#[test]
fn whoever_executed_work_under_the_task_or_ever_orchestrated_it_is_remembered() {
    let (f, orch, exe, rev) = in_review();
    let under = |slug: &str, parent: &str| {
        f.task_with(NewTask {
            parent: Some(parent.into()),
            ..NewTask::new(slug, "A step", &orch)
        });
    };
    under("sim/jump/step", "sim/jump");
    under("sim/jump/step/one", "sim/jump/step");
    let (step, one) = (f.session("worker"), f.session("worker"));
    f.store.claim("sim/jump/step", &step).unwrap();
    f.store.claim("sim/jump/step/one", &one).unwrap();
    let step_orch = f.session("orchestrator");
    f.store
        .take_role("sim/jump/step", &step_orch, "orchestrator")
        .unwrap();
    // Its orchestrator hands the role on.
    let next = f.session("orchestrator");
    f.store.hand_over("sim/jump", &orch, &next).unwrap();
    let held = f.store.ever_held("sim/jump").unwrap();
    assert_eq!(
        held.executed,
        BTreeSet::from([exe.clone(), step.clone(), one.clone()])
    );
    assert_eq!(held.orchestrated, BTreeSet::from([orch.clone(), next]));
    assert_eq!(
        f.store.ever_executed("sim/jump").unwrap(),
        held.executed,
        "the executors are the same read"
    );
    // A subtask's own: its executors below it, its orchestrator, and none above it.
    let below = f.store.ever_held("sim/jump/step").unwrap();
    assert_eq!(below.executed, BTreeSet::from([step.clone(), one.clone()]));
    assert_eq!(below.orchestrated, BTreeSet::from([step_orch]));
    // Who executed work under it gives the task no verdict, a reviewer's role or not, once the
    // fresh reviewer, the one a task, has let it go; holding it, they keep no other out.
    f.store.release("sim/jump", &rev, Some("reviewer")).unwrap();
    for who in [&step, &one] {
        f.store.take_role("sim/jump", who, "reviewer").unwrap();
        assert_eq!(
            refused(f.store.verdict("sim/jump", who, "approve", "")),
            format!(
                "{who} executed sim/jump: its verdict is a fresh reviewer's, one that did not \
                     write or execute it"
            )
        );
    }
    assert!(!held.executed.contains(&rev) && !held.orchestrated.contains(&rev));
    assert_eq!(refused(f.store.ever_held("sim/none")), "no task 'sim/none'");
}

/// An executor that joined as one, as a board's older line says, and left: named all the same,
/// where a reviewer's or an orchestrator's join, or a line naming another, names no one.
#[test]
fn an_executor_that_joined_and_left_is_remembered_too() {
    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch")
        .session("coral-hare")
        .task("sim/jump", "review")
        .role("sim/jump", "brisk-finch", "reviewer");
    for (id, sender, text) in [
        (1, "amber-heron", "amber-heron joined as executor"),
        (2, "brisk-finch", "brisk-finch joined as reviewer"),
        (3, "coral-hare", "coral-hare joined as orchestrator"),
        (4, "coral-hare", "amber-heron joined as executor"),
    ] {
        seed.dump.messages.push(symphony_board::rows::MessageRow {
            id,
            slug: "sim/jump".into(),
            sender: sender.into(),
            recipient: None,
            kind: "system".into(),
            text: text.into(),
            ts: NOW,
        });
    }
    let f = Fixture::new(seed);
    assert_eq!(
        f.store.ever_executed("sim/jump").unwrap(),
        BTreeSet::from(["amber-heron".to_string()])
    );
    assert_eq!(
        refused(f.store.verdict("sim/jump", "amber-heron", "approve", "")),
        "amber-heron executed sim/jump: its verdict is a fresh reviewer's, one that did not write \
             or execute it"
    );
}

/// A role on the task that is not one of the three sends it nowhere; with no executor to read
/// the notice, its orchestrator hears it, and restaffs.
#[test]
fn a_reopen_no_executor_reads_is_said_to_the_orchestrator_and_a_reservation_reopens_nothing() {
    let (f, orch, exe, rev) = in_review();
    // Its executor gone, a session reserves it while it is in review.
    f.store.release("sim/jump", &exe, Some("executor")).unwrap();
    let reserver = f.session("worker");
    f.store
        .reserve("sim/jump", &reserver, RESERVE_SECONDS)
        .unwrap();
    f.store
        .verdict("sim/jump", &rev, "approve", "good")
        .unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/jump", "review", &reserver)),
        format!(
            "{reserver} is not sim/jump's executor, reviewer or orchestrator, which alone send it \
             back to review"
        )
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "done");
    let said = f.said("sim/jump").len();
    f.store.move_to("sim/jump", "review", &rev).unwrap();
    let notice = back_from_done(&rev);
    let to_executor = f.store.thread("sim/jump").unwrap()[said + 1].id;
    let unread = format!(
        "#{to_executor} from board to @executor reaches no live executor: no session holds the role"
    );
    assert_eq!(
        lines_from(&f, "sim/jump", said),
        [
            (rev.clone(), format!("{rev} moved it done → review"), None),
            ("board".into(), notice.clone(), Some("@executor".into())),
            ("board".into(), unread.clone(), Some("@orchestrator".into())),
            ("board".into(), notice.clone(), Some("@reviewer".into())),
        ]
    );
    assert!(told(&f, &orch).contains(&unread), "{:?}", told(&f, &orch));
}

#[test]
fn a_subtask_made_for_a_handle_is_reserved_without_a_lapse() {
    let f = Fixture::new(Seed::default());
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
