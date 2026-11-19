//! PULL's lists of held tasks, without a board: each position of the review workflow, its claim
//! edges' roles, lost holders, prerequisites that ended without success and red work.

use super::*;
use crate::Health;
use crate::workflow::operate::tests::{held, holding, legacy, reviewed};

fn judge(task: &Task, board: &[Task]) -> Judged {
    let w = reviewed();
    let placed = w.placed(task).unwrap();
    judged(&w, &placed, task, |slug| board.iter().find(|t| t.slug == slug), 1e6)
}

fn listed(category: &'static str, note: Option<&str>) -> Listed {
    Listed {
        category,
        note: note.map(String::from),
    }
}

#[test]
fn work_nobody_claimed_is_unstaffed_by_the_role_its_claim_gives() {
    let available = held("sim/a", "build", "available");
    assert_eq!(
        judge(&available, &[]).listed,
        [listed("ready_unstaffed", None)]
    );
    let awaiting = held("sim/a", "verify", "awaiting");
    assert_eq!(
        judge(&awaiting, &[]).listed,
        [listed("review_unreviewed", None)]
    );
    // A reservation is named beside it, as the board's own ready work says it.
    let reserved = holding(available.clone(), "amber", "reserved", Health::Live);
    assert_eq!(
        judge(&reserved, &[]).listed,
        [listed("ready_unstaffed", Some("reserved amber"))]
    );
    let lapsed = holding(available, "amber", "reserved", Health::Lost);
    assert_eq!(judge(&lapsed, &[]).listed, [listed("ready_unstaffed", None)]);
}

#[test]
fn a_claim_whose_dependencies_are_not_met_waits_unlisted() {
    let mut task = held("sim/a", "build", "available");
    task.depends = vec!["sim/b".into()];
    for (prerequisite, listed) in [
        (held("sim/b", "build", "working"), false),
        (held("sim/b", "verify", "approved"), true),
        (legacy("sim/b", "review"), false),
        (legacy("sim/b", "done"), true),
    ] {
        let board = [prerequisite.clone()];
        assert_eq!(
            !judge(&task, &board).listed.is_empty(),
            listed,
            "{} at {:?}",
            prerequisite.status,
            prerequisite.fields
        );
    }
}

#[test]
fn work_held_by_a_session_there_is_executing_and_listed_nowhere() {
    let task = holding(held("sim/a", "build", "working"), "amber", "executor", Health::Stale);
    assert_eq!(
        judge(&task, &[]),
        Judged {
            listed: Vec::new(),
            executing: true
        }
    );
    // A review in progress is no execution, and its position offers no claim.
    let task = holding(held("sim/a", "verify", "reviewing"), "birch", "reviewer", Health::Live);
    assert_eq!(judge(&task, &[]), Judged::default());
    // A held block keeps its executor: at work, though not working the task's edges.
    let task = holding(held("sim/a", "build", "blocked"), "amber", "executor", Health::Live);
    assert_eq!(judge(&task, &[]).executing, true);
    // Work nobody holds at a staffed position is neither.
    assert_eq!(judge(&held("sim/a", "build", "working"), &[]), Judged::default());
}

#[test]
fn a_lost_holder_is_orphaned_work_the_board_recovers_or_a_person_does() {
    let task = holding(held("sim/a", "build", "working"), "amber", "executor", Health::Lost);
    assert_eq!(
        judge(&task, &[]).listed,
        [listed(
            "in_progress_orphaned",
            Some("lost amber; the board recovers it by its position's lost_holder action")
        )]
    );
    let mut task = holding(held("sim/a", "build", "blocked"), "amber", "executor", Health::Left);
    task.fields.get_mut("_board").unwrap().insert(
        "workflow".into(),
        serde_json::json!({
            "phase": "build", "state": "blocked", "generation": 1, "revision": 4,
            "digest": reviewed().revision.digest,
            "manual": {"trigger": "lost_holder", "holder": "amber", "role": "executor", "manual": true},
        }),
    );
    let judged = judge(&task, &[]);
    assert!(!judged.executing);
    assert_eq!(
        judged.listed,
        [listed(
            "in_progress_orphaned",
            Some(
                "lost amber; its recovery waits on a person (`just tasks workflow actions sim/a`)"
            )
        )]
    );
}

#[test]
fn an_unsuccessful_prerequisite_blocks_naming_the_positions_policy() {
    let board = [legacy("sim/gone", "cancelled"), held("sim/off", "build", "cancelled")];
    let needing = |phase, state| {
        let mut task = held("sim/a", phase, state);
        task.depends = vec!["sim/gone".into(), "sim/off".into()];
        task
    };
    let said = |phase, state| {
        let judged = judge(&needing(phase, state), &board);
        assert!(!judged.executing);
        let [one] = &judged.listed[..] else {
            panic!("{judged:?}");
        };
        assert_eq!(one.category, "dependency_blocked");
        one.note.clone().unwrap()
    };
    let ended = "prerequisites ended without success: sim/gone, sim/off; its position's policy";
    assert_eq!(
        said("build", "available"),
        format!(
            "{ended}: cancel; no cancel edge the board may take leaves it: a person cancels or \
             moves it"
        )
    );
    assert_eq!(said("build", "working"), format!("{ended}: block"));
    assert_eq!(
        said("build", "blocked"),
        format!("{ended}: manual; it waits on a person")
    );
}

#[test]
fn red_work_nobody_took_is_named_first_and_finished_work_is_listed_nowhere() {
    let mut red = held("sim/a", "build", "available");
    red.fields
        .entry("_board".into())
        .or_default()
        .insert(crate::red::MARK.into(), serde_json::json!("the release run's tests"));
    assert!(red.red());
    assert_eq!(judge(&red, &[]).listed, [listed("red_unstaffed", None)]);
    // Filed just now: not yet unstaffed past its wait.
    red.created = 1e6;
    assert_eq!(judge(&red, &[]).listed, [listed("ready_unstaffed", None)]);
    for (phase, state) in [("verify", "accepted"), ("build", "cancelled")] {
        let mut task = holding(held("sim/a", phase, state), "amber", "executor", Health::Lost);
        task.depends = vec!["sim/gone".into()];
        assert_eq!(
            judge(&task, &[legacy("sim/gone", "cancelled")]),
            Judged::default(),
            "{phase}/{state}"
        );
    }
}
