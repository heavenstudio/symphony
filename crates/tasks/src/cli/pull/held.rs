//! Where PULL lists a task a custom workflow holds (docs/workflow/WORKFLOW.md, "Operations"): by the edges
//! leaving its position and the role that position staffs, never by the status it projects to.
//!
//! - A claim edge giving the executor's role, or the reviewer's, that nobody there holds, its
//!   dependency predicate met: `ready_unstaffed` (`red_unstaffed` for red work nobody took after
//!   [`UNSTAFFED_SECONDS`]) or `review_unreviewed`, a live reservation named beside it.
//! - The staffed role held only by holders lost or gone: `in_progress_orphaned`, which the board's
//!   settling pass recovers by the position's `lost_holder` action, or which waits on a person.
//! - A prerequisite that ended without success: `dependency_blocked`, naming the position's
//!   policy. The settling pass cancels a dependant whose policy is `cancel` where a cancel edge
//!   the board may take leaves its pair; any other waits on a person.
//! - Its staffed executor there: executing.
//!
//! A terminal position lists nothing: the task is finished.

use crate::Task;
use crate::red::UNSTAFFED_SECONDS;
use crate::workflow::admit::word;
use crate::workflow::operate::{Operating, Placed};
use symphony_api::workflow::{Role, UnsuccessfulDependency};

/// One list a held task joins, and what PULL says beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Listed {
    pub category: &'static str,
    pub note: Option<String>,
}

/// What PULL makes of a held task: the lists it joins, and whether its executor is at work.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Judged {
    pub listed: Vec<Listed>,
    pub executing: bool,
}

/// `task`, standing at `placed` on `operating`, as PULL lists it at `now`; `find` reads the
/// board's other tasks.
pub(super) fn judged<'t>(
    operating: &Operating,
    placed: &Placed<'_>,
    task: &'t Task,
    find: impl Fn(&str) -> Option<&'t Task> + Copy,
    now: f64,
) -> Judged {
    let mut judged = Judged::default();
    if placed.finished() {
        return judged;
    }
    let unsuccessful = operating.unsuccessful(task, find);
    if !unsuccessful.is_empty() {
        let policy = placed.declared.unsuccessful_dependency;
        let mut note = format!(
            "prerequisites ended without success: {}; its position's policy: {}",
            unsuccessful.join(", "),
            word(&policy)
        );
        note.push_str(match policy {
            UnsuccessfulDependency::Cancel => {
                "; no cancel edge the board may take leaves it: a person cancels or moves it"
            }
            UnsuccessfulDependency::Manual => "; it waits on a person",
            UnsuccessfulDependency::Block => "",
        });
        judged.listed.push(Listed {
            category: "dependency_blocked",
            note: Some(note),
        });
        return judged;
    }
    if placed.working(task) {
        judged.executing = placed.staffing() == Some(Role::Executor);
    }
    let lost = placed.lost(task);
    if !lost.is_empty() {
        let waits = lost.iter().all(|holder| placed.waits_on_person_for(holder));
        judged.listed.push(Listed {
            category: "in_progress_orphaned",
            note: Some(if waits {
                format!(
                    "lost {}; its recovery waits on a person (`just tasks workflow actions {}`)",
                    lost.join(", "),
                    task.slug
                )
            } else {
                format!(
                    "lost {}; the board recovers it by its position's lost_holder action",
                    lost.join(", ")
                )
            }),
        });
    }
    let reserved = (task.roles.iter())
        .find(|r| r.role == "reserved" && r.health.holds())
        .map(|r| format!("reserved {}", r.handle));
    let mut wanted: Vec<Role> = Vec::new();
    for claim in placed.claims() {
        let role = word(&claim.role);
        let there = (task.roles.iter()).any(|r| r.role == role && r.health.holds());
        if there
            || wanted.contains(&claim.role)
            || !operating.dependencies_met(task, claim.edge, find)
        {
            continue;
        }
        wanted.push(claim.role);
        let category = match claim.role {
            Role::Executor if task.red() && now - task.created >= UNSTAFFED_SECONDS => {
                "red_unstaffed"
            }
            Role::Executor => "ready_unstaffed",
            Role::Reviewer => "review_unreviewed",
            Role::Orchestrator => continue,
        };
        judged.listed.push(Listed {
            category,
            note: reserved.clone(),
        });
    }
    judged
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
