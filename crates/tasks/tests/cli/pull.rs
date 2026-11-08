//! PULL digest: actionable board work without a corresponding event; red tasks are [`reds`]'.

mod reds;

use crate::common::{Fixture, NOW};
use std::collections::BTreeSet;
use symphony_board::cli::pull::{Digest, digest};
use symphony_board::health::{LIVE_SECONDS, RESERVE_SECONDS, STALE_SECONDS};
use symphony_board::sys::local_minute;
use symphony_board::{NewTask, Task};

#[path = "pull_unanswered.rs"]
mod unanswered;

struct Case {
    f: Fixture,
    landed: BTreeSet<String>,
    idle: Vec<String>,
}

impl Case {
    fn new() -> Case {
        Case {
            f: Fixture::empty(),
            landed: BTreeSet::new(),
            idle: Vec::new(),
        }
    }

    fn digest(&self) -> Digest {
        self.verifying(&[])
    }

    fn verifying(&self, verifying: &[String]) -> Digest {
        let landed = |t: &Task| self.landed.contains(&t.slug);
        digest(
            &self.f.store,
            &self.idle,
            Some(&landed),
            self.f.now(),
            verifying,
        )
        .unwrap()
    }

    fn worker(&self) -> String {
        self.f.session("worker")
    }

    /// `slug`, titled `T slug`, made ready and taken to `status` by a worker of its own.
    fn task(&self, slug: &str, status: &str) {
        self.task_with(NewTask::new(slug, &format!("T {slug}"), "owner"), status);
    }

    fn task_with(&self, new: NewTask, status: &str) {
        let slug = new.slug.clone();
        self.f.store.create(&new).unwrap();
        if status == "in_progress" {
            self.f.store.claim(&slug, &self.worker()).unwrap();
        } else if status == "review" {
            let who = self.worker();
            self.f.store.claim(&slug, &who).unwrap();
            self.f.store.move_to(&slug, "review", &who).unwrap();
        }
    }

    fn sub(&self, slug: &str, parent: &str) -> NewTask {
        NewTask {
            parent: Some(parent.into()),
            instruction: true,
            ..NewTask::new(slug, &format!("T {slug}"), "owner")
        }
    }

    fn priority(&self, slug: &str, priority: i64) -> NewTask {
        NewTask {
            priority,
            ..NewTask::new(slug, &format!("T {slug}"), "owner")
        }
    }

    /// Past stale: whoever does not beat again is lost.
    fn go_quiet(&self) {
        self.f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
    }

    fn executor(&self, slug: &str) -> String {
        self.f.store.get(slug).unwrap().holders("executor")[0].to_string()
    }
}

fn slugs(d: &Digest, name: &str) -> Vec<String> {
    d.items(name).iter().map(|t| t.slug.clone()).collect()
}

// -- unstaffed -------------------------------------------------------------------------

#[test]
fn a_ready_task_with_nobody_is_unstaffed() {
    let c = Case::new();
    c.task_with(c.priority("sim/a", 1), "ready");
    let d = c.digest();
    assert_eq!(d.count("ready_unstaffed"), 1);
    assert_eq!(slugs(&d, "ready_unstaffed"), ["sim/a"]);
}

#[test]
fn a_ready_task_with_a_live_executor_is_staffed() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.f.store.claim("sim/a", &c.worker()).unwrap(); // claiming a ready task makes it in_progress
    assert_eq!(c.digest().count("ready_unstaffed"), 0);
}

#[test]
fn a_ready_task_waiting_on_a_dependency_is_not_pullable() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.task_with(
        NewTask {
            depends: vec!["sim/a".into()],
            ..NewTask::new("sim/b", "T sim/b", "owner")
        },
        "ready",
    );
    assert_eq!(slugs(&c.digest(), "ready_unstaffed"), ["sim/a"]);
}

#[test]
fn a_ready_parent_whose_ready_child_has_a_live_executor_counts_as_staffed() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.task_with(c.sub("sim/a/do", "sim/a"), "ready");
    c.f.store.claim("sim/a/do", &c.worker()).unwrap();
    assert_eq!(c.digest().count("ready_unstaffed"), 0);
}

#[test]
fn a_grandchild_at_work_staffs_its_grandparent_through_a_finished_child() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.task_with(c.sub("sim/a/b", "sim/a"), "ready");
    c.task_with(c.sub("sim/a/b/c", "sim/a/b"), "ready");
    let who = c.worker();
    c.f.store.claim("sim/a/b", &who).unwrap();
    c.f.store.done("sim/a/b", &who, "split it").unwrap();
    c.f.store.claim("sim/a/b/c", &c.worker()).unwrap();
    assert_eq!(c.digest().count("ready_unstaffed"), 0);
}

#[test]
fn a_stale_executor_still_counts() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    c.f.advance(LIVE_SECONDS + 5.0);
    let d = c.digest();
    assert_eq!(
        (d.count("in_progress_orphaned"), d.count("executing")),
        (0, 1)
    );
}

#[test]
fn ready_tasks_come_by_priority_then_slug() {
    let c = Case::new();
    c.task_with(c.priority("sim/c", 2), "ready");
    c.task_with(c.priority("sim/a", 3), "ready");
    c.task_with(c.priority("sim/b", 0), "ready");
    c.task_with(c.priority("sim/d", 2), "ready");
    assert_eq!(
        slugs(&c.digest(), "ready_unstaffed"),
        ["sim/b", "sim/c", "sim/d", "sim/a"]
    );
}

#[test]
fn a_task_reserved_for_a_live_session_is_still_listed_with_its_reserver() {
    let c = Case::new();
    c.task("sim/a", "ready");
    let who = c.worker();
    c.f.store.reserve("sim/a", &who, RESERVE_SECONDS).unwrap();
    let d = c.digest();
    assert_eq!(d.count("ready_unstaffed"), 1);
    assert!(d.lines().contains(&format!(
        "  ready_unstaffed p2 sim/a — T sim/a   (reserved {who})"
    )));
}

#[test]
fn a_reservation_by_a_lost_session_is_not_named() {
    let c = Case::new();
    c.task("sim/a", "ready");
    let who = c.worker();
    c.f.store
        .reserve("sim/a", &who, RESERVE_SECONDS * 10.0)
        .unwrap();
    c.go_quiet();
    assert!(
        c.digest()
            .lines()
            .contains(&"  ready_unstaffed p2 sim/a — T sim/a".to_string())
    );
}

#[test]
fn concept_and_backlog_work_is_not_pulled() {
    let c = Case::new();
    c.task_with(
        NewTask {
            status: "backlog".into(),
            ..NewTask::new("sim/b", "B", "owner")
        },
        "backlog",
    );
    assert_eq!(c.digest().count("ready_unstaffed"), 0);
}

// -- finished subtasks -----------------------------------------------------------------

#[test]
fn a_done_subtasks_executor_does_not_staff_its_ready_parent() {
    let c = Case::new();
    c.task_with(c.priority("sim/a", 1), "ready");
    c.task_with(c.sub("sim/a/s1", "sim/a"), "ready");
    let who = c.worker();
    c.f.store.claim("sim/a/s1", &who).unwrap();
    c.f.store.done("sim/a/s1", &who, "stage 1 landed").unwrap();
    assert_eq!(slugs(&c.digest(), "ready_unstaffed"), ["sim/a"]);
}

#[test]
fn a_cancelled_subtasks_executor_does_not_staff_its_parent() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.task_with(c.sub("sim/a/s1", "sim/a"), "ready");
    let who = c.worker();
    c.f.store.claim("sim/a/s1", &who).unwrap();
    c.f.store.move_to("sim/a/s1", "cancelled", &who).unwrap();
    assert_eq!(c.digest().count("ready_unstaffed"), 1);
}

#[test]
fn an_open_subtask_still_staffs_beside_a_done_one() {
    let c = Case::new();
    c.task("sim/a", "ready");
    for name in ["s1", "s2"] {
        c.task_with(c.sub(&format!("sim/a/{name}"), "sim/a"), "ready");
    }
    let (first, second) = (c.worker(), c.worker());
    c.f.store.claim("sim/a/s1", &first).unwrap();
    c.f.store.done("sim/a/s1", &first, "done").unwrap();
    c.f.store.claim("sim/a/s2", &second).unwrap();
    assert_eq!(c.digest().count("ready_unstaffed"), 0);
}

// -- verifying -------------------------------------------------------------------------

#[test]
fn a_board_lost_executor_in_its_verify_does_not_orphan_its_task() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    let executor = c.executor("sim/a");
    c.go_quiet();
    assert_eq!(c.digest().count("in_progress_orphaned"), 1);
    let d = c.verifying(&[executor]);
    assert_eq!(
        (d.count("in_progress_orphaned"), d.count("executing")),
        (0, 1)
    );
}

#[test]
fn a_verifying_reviewer_still_staffs_the_review() {
    let c = Case::new();
    c.task("sim/a", "review");
    let reviewer = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &reviewer, "reviewer").unwrap();
    c.go_quiet();
    assert_eq!(c.verifying(&[reviewer]).count("review_unreviewed"), 0);
}

// -- staffing through subtasks ---------------------------------------------------------

#[test]
fn only_a_subtask_still_to_be_done_staffs_its_parent() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.task_with(
        NewTask {
            parent: Some("sim/a".into()),
            ..NewTask::new("sim/a/work", "Work", "owner")
        },
        "ready",
    ); // a plain subtask, which can go to review
    let who = c.worker();
    c.f.store.claim("sim/a/work", &who).unwrap();
    c.f.store.move_to("sim/a/work", "review", &who).unwrap();
    assert_eq!(c.digest().count("ready_unstaffed"), 1); // its executor waits on a reviewer, not on `sim/a`
}

// -- landed, by the thread -------------------------------------------------------------

/// `land` deletes the branch, so the branch cannot say it landed; its own line on the thread does.
#[test]
fn a_landing_posted_on_the_thread_marks_an_open_task_landed() {
    let c = Case::new();
    c.task("sim/a", "review");
    let orch = c.f.session("orchestrator");
    c.f.store
        .say(
            "sim/a",
            &orch,
            "landed sim/a on main at 3e0c66192471 (tier 3 · high)",
            None,
        )
        .unwrap();
    assert_eq!(slugs(&c.digest(), "landed_open"), ["sim/a"]);
}

#[test]
fn a_landing_of_another_branch_on_the_thread_does_not() {
    let c = Case::new();
    c.task("sim/a", "review");
    let orch = c.f.session("orchestrator");
    c.f.store
        .say("sim/a", &orch, "landed sim/b on main at 3e0c66192471", None)
        .unwrap();
    assert_eq!(c.digest().count("landed_open"), 0);
}

#[test]
fn a_task_reopened_after_its_landing_line_is_not_landed_any_more() {
    let c = Case::new();
    c.task("sim/a", "review");
    let orch = c.f.session("orchestrator");
    c.f.store
        .say("sim/a", &orch, "landed sim/a on main at 3e0c66192471", None)
        .unwrap();
    let executor = c.executor("sim/a");
    c.f.store
        .move_to("sim/a", "in_progress", &executor)
        .unwrap(); // more work on it after
    assert_eq!(c.digest().count("landed_open"), 0);
}

#[test]
fn a_task_that_never_landed_is_not_listed() {
    let c = Case::new();
    c.task("sim/a", "review");
    assert_eq!(c.digest().count("landed_open"), 0);
}

// -- orphaned --------------------------------------------------------------------------

#[test]
fn an_in_progress_task_whose_executor_is_gone_is_orphaned() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    c.go_quiet();
    let d = c.digest();
    assert_eq!(
        (d.count("in_progress_orphaned"), d.count("executing")),
        (1, 0)
    );
}

#[test]
fn an_in_progress_parent_staffed_through_a_subtask_is_not_orphaned() {
    let c = Case::new();
    c.task("sim/a", "ready");
    c.f.store.claim("sim/a", &c.worker()).unwrap();
    c.go_quiet();
    c.task_with(c.sub("sim/a/do", "sim/a"), "ready");
    c.f.store.claim("sim/a/do", &c.worker()).unwrap();
    assert_eq!(c.digest().count("in_progress_orphaned"), 0);
}

#[test]
fn blocked_work_is_nobodys_job() {
    let c = Case::new();
    c.task("sim/a", "in_progress");
    let executor = c.executor("sim/a");
    c.f.store.move_to("sim/a", "blocked", &executor).unwrap();
    c.go_quiet();
    assert_eq!(c.digest().count("in_progress_orphaned"), 0);
}

// -- review ----------------------------------------------------------------------------

#[test]
fn a_review_with_no_reviewer_is_unreviewed() {
    let c = Case::new();
    c.task("sim/a", "review");
    assert_eq!(slugs(&c.digest(), "review_unreviewed"), ["sim/a"]);
}

#[test]
fn a_live_reviewer_staffs_it() {
    let c = Case::new();
    c.task("sim/a", "review");
    let reviewer = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &reviewer, "reviewer").unwrap();
    assert_eq!(c.digest().count("review_unreviewed"), 0);
}

#[test]
fn a_lost_reviewer_does_not() {
    let c = Case::new();
    c.task("sim/a", "review");
    let reviewer = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &reviewer, "reviewer").unwrap();
    c.go_quiet();
    assert_eq!(c.digest().count("review_unreviewed"), 1);
}

#[test]
fn an_executor_on_a_review_subtask_staffs_the_review() {
    let c = Case::new();
    c.task("sim/a", "review");
    c.task_with(c.sub("sim/a/review-x", "sim/a"), "ready");
    c.f.store.claim("sim/a/review-x", &c.worker()).unwrap();
    assert_eq!(c.digest().count("review_unreviewed"), 0);
}

// -- landed and idle -------------------------------------------------------------------

#[test]
fn a_landed_branch_whose_task_is_still_open_is_listed() {
    let mut c = Case::new();
    c.task("sim/a", "review");
    c.task("sim/b", "in_progress");
    c.task("sim/c", "ready");
    c.landed.extend([
        "sim/a".to_string(),
        "sim/b".to_string(),
        "sim/c".to_string(),
    ]);
    let d = c.digest();
    assert_eq!(slugs(&d, "landed_open"), ["sim/a", "sim/b"]);
    assert!(d.lines().contains(
        &"  landed_open p2 sim/a — T sim/a   (landed on main, task still review)".to_string()
    ));
    assert!(d.lines().contains(
        &"  landed_open p2 sim/b — T sim/b   (landed on main, task still in_progress)".to_string()
    ));
}

#[test]
fn a_done_task_is_not_listed_as_landed() {
    let mut c = Case::new();
    c.task("sim/a", "review");
    let reviewer = c.f.session("reviewer");
    c.f.store.take_role("sim/a", &reviewer, "reviewer").unwrap();
    c.f.store
        .verdict("sim/a", &reviewer, "approve", "ok")
        .unwrap();
    c.landed.insert("sim/a".into());
    assert_eq!(c.digest().count("landed_open"), 0);
}

#[test]
fn without_a_landed_probe_only_the_thread_says_a_task_landed() {
    let c = Case::new();
    c.task("sim/a", "review");
    let d = digest(&c.f.store, &[], None, NOW, &[]).unwrap();
    assert_eq!(d.count("landed_open"), 0);
}

#[test]
fn idle_agents_are_reported_as_given() {
    let mut c = Case::new();
    c.idle = vec!["calm-vole".into(), "jade-merlin".into()];
    let d = c.digest();
    assert_eq!(d.count("idle"), 2);
    assert_eq!(d.idle, ["calm-vole", "jade-merlin"]);
    assert_eq!(&d.lines()[1..], ["  idle calm-vole", "  idle jade-merlin"]);
}

// -- the lines -------------------------------------------------------------------------

fn header(time: &str, counts: [usize; 6]) -> String {
    format!(
        "PULL {time} executing={} ready_unstaffed={} in_progress_orphaned={} review_unreviewed={} landed_open={} idle={}",
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]
    )
}

#[test]
fn the_header_names_every_count_at_the_given_time_in_local_time() {
    let c = Case::new();
    let d = c.digest();
    assert_eq!(d.lines()[0], header(&local_minute(NOW)[11..], [0; 6]));
    assert_eq!(d.now, NOW);
}

#[test]
fn under_a_cap_the_orchestrators_header_counts_its_open_tasks_against_it() {
    let c = Case::new();
    let orch = c.f.session("orchestrator");
    for (slug, status) in [
        ("sim/a", "in_progress"),
        ("sim/b", "review"),
        ("sim/c", "ready"),
    ] {
        c.task(slug, status);
        c.f.store.take_role(slug, &orch, "orchestrator").unwrap();
    }
    c.task("sim/b/review", "review");
    let board = c.f.store.board().unwrap();
    let capped = c.f.other().capping(Some(8));
    let first_line = |store: &symphony_board::Store| {
        digest(store, &[], None, NOW, &[])
            .unwrap()
            .only_for(&board, &orch)
            .lines()[0]
            .clone()
    };
    // sim/a and sim/b, its review mirror counted with it: two slots.
    let plain = header(&local_minute(NOW)[11..], [1, 1, 0, 2, 0, 0]);
    assert_eq!(first_line(&capped), format!("{plain} slots=2/8"));
    assert_eq!(first_line(&c.f.store), plain, "no cap, no count");
}

#[test]
fn a_quiet_board_is_the_header_alone() {
    let c = Case::new();
    assert_eq!(c.digest().lines().len(), 1);
}

#[test]
fn each_category_lists_six_at_most_and_says_how_many_more() {
    let c = Case::new();
    for n in 0..9 {
        c.task_with(c.priority(&format!("sim/t{n}"), n % 3), "ready");
    }
    let lines = c.digest().lines();
    let listed: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("  ready_unstaffed p"))
        .collect();
    assert_eq!(listed.len(), 6);
    assert_eq!(lines.last().unwrap(), "  ready_unstaffed … and 3 more");
}

#[test]
fn a_line_names_priority_slug_and_title() {
    let c = Case::new();
    c.task_with(
        NewTask {
            priority: 1,
            ..NewTask::new("sim/a", "Jump departure", "owner")
        },
        "ready",
    );
    assert!(
        c.digest()
            .lines()
            .contains(&"  ready_unstaffed p1 sim/a — Jump departure".to_string())
    );
}

#[test]
fn the_categories_come_in_their_order_each_under_the_header() {
    let mut c = Case::new();
    c.task("sim/r", "review");
    c.task("sim/o", "in_progress");
    c.task("sim/u", "ready");
    c.go_quiet();
    c.landed.insert("sim/o".into());
    c.idle = vec!["calm-vole".into()];
    assert_eq!(
        c.digest().lines(),
        [
            header(&local_minute(c.f.now())[11..], [0, 1, 1, 1, 1, 1]),
            "  ready_unstaffed p2 sim/u — T sim/u".into(),
            "  in_progress_orphaned p2 sim/o — T sim/o   (landed on main, task still in_progress)"
                .into(),
            "  review_unreviewed p2 sim/r — T sim/r".into(),
            "  landed_open p2 sim/o — T sim/o   (landed on main, task still in_progress)".into(),
            "  idle calm-vole".into(),
        ]
    );
}
