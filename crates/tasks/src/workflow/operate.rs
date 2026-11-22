//! What the active custom workflow says of a task it holds, for the board's operational readers
//! (docs/workflow/WORKFLOW.md, "Operations"): PULL and `next`, the launcher and the staffing pass, the slot
//! cap, the pipeline alarm and the sweeps. Each reads the task's declared position and the edges
//! that leave it, never a state's name or the status the position projects to: which roles its
//! claim edges give, the role its position staffs and whether a session that is there holds it,
//! whether it holds a task slot, whether it is finished, and which of its prerequisites ended
//! without success. Whether a principal may take an edge is admission's own question
//! (`Store::workflow_actions`), never decided here.
//!
//! A task slot is task WIP, the cap on an orchestrator's open work ([`crate::slots`]): a task holds
//! one while its position's slot policy is `acquire` or `retain`, and none at `release`. It is not
//! an execution attempt's capacity, which the attempt service keeps apart.

use super::TaskPosition;
use crate::Task;
use symphony_api::workflow::{
    Definition, Dependency, Edge, Effect, Outcome, Pair, Position, Revision, Role, Slot,
    WorkflowRead,
};

/// The active custom workflow, read once by a command for every task it looks at.
#[derive(Clone, Debug, PartialEq)]
pub struct Operating {
    pub definition: Definition,
    pub revision: Revision,
    pub generation: u64,
    /// The accepted output standing for each task a workflow holds that a task read depends on,
    /// by its slug, with the workflow revision it stood at when read (docs/workflow/WORKFLOW.md, "Accepted
    /// outputs"): read only where an edge depends on `accepted_output`
    /// ([`crate::Store::operating_for`]), and none for a task whose last transition left none.
    pub accepted: Vec<(String, u64, String)>,
}

impl From<WorkflowRead> for Operating {
    fn from(read: WorkflowRead) -> Operating {
        Operating {
            definition: read.definition,
            revision: read.revision,
            generation: read.activation_generation,
            accepted: Vec::new(),
        }
    }
}

/// Whether `task` is finished by its recorded facts, on `operating` where a custom workflow is
/// active ([`Operating::finished`]): a task one holds at a terminal position, never by the status
/// it projects to, and none at a pair it does not declare; any other task by the board's own done
/// or cancelled.
pub fn finished(operating: Option<&Operating>, task: &Task) -> bool {
    match operating {
        Some(operating) => operating.finished(task).unwrap_or(false),
        None => {
            !crate::store::workflow::position_held(&task.fields)
                && crate::health::FINISHED.contains(&task.status.as_str())
        }
    }
}

/// A task the workflow holds, at a position the definition declares.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed<'a> {
    pub position: TaskPosition,
    pub declared: &'a Position,
    definition: &'a Definition,
}

/// An edge leaving a task's pair that gives its taker a role: a claim, by whatever operation the
/// definition names it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claim<'a> {
    pub edge: &'a Edge,
    pub role: Role,
}

impl Operating {
    /// Where `task` stands on this workflow: none for a task no workflow holds, or one at a pair
    /// the definition does not declare (which activation never leaves).
    pub fn placed(&self, task: &Task) -> Option<Placed<'_>> {
        let position = crate::store::workflow::task_position(&task.fields)?;
        let declared = position_of(&self.definition, &position.at())?;
        Some(Placed {
            position,
            declared,
            definition: &self.definition,
        })
    }

    /// Whether `task` is finished by its recorded facts: a task the workflow holds at a terminal
    /// position (one that records its outcome), else the board's own done or cancelled. None for a
    /// task the workflow holds at a pair it does not declare, which no reader may guess at.
    pub fn finished(&self, task: &Task) -> Option<bool> {
        if !crate::store::workflow::position_held(&task.fields) {
            return Some(crate::health::FINISHED.contains(&task.status.as_str()));
        }
        self.placed(task).map(|placed| placed.finished())
    }

    /// Whether `task` holds a task slot: by its position's slot policy, for a task the workflow
    /// holds; none for any other task, which the ledger counts by its status.
    pub fn holds_slot(&self, task: &Task) -> Option<bool> {
        self.placed(task).map(|placed| placed.holds_slot())
    }

    /// The prerequisites of `task` that ended without success, in the order it names them: one the
    /// workflow holds at a position whose outcome is a cancellation or a failure, or one the
    /// board's writers keep cancelled. A prerequisite not on the board is no such end.
    pub fn unsuccessful<'t>(
        &self,
        task: &'t Task,
        find: impl Fn(&str) -> Option<&'t Task>,
    ) -> Vec<&'t str> {
        (task.depends.iter())
            .filter(|slug| {
                find(slug).is_some_and(|prerequisite| match self.placed(prerequisite) {
                    Some(placed) => matches!(
                        placed.declared.outcome,
                        Some(Outcome::Cancellation | Outcome::Failure)
                    ),
                    None => {
                        !crate::store::workflow::position_held(&prerequisite.fields)
                            && prerequisite.status == "cancelled"
                    }
                })
            })
            .map(String::as_str)
            .collect()
    }

    /// Whether `edge`'s dependency predicate holds for `task` on the board `find` reads, as
    /// admission judges it: `done` is met by a prerequisite the workflow holds at a position that
    /// satisfies its dependants, or one the board's writers keep done; `accepted_output` by an
    /// accepted output standing for each, one the workflow holds at such a position whose last
    /// transition, at the revision `find` reads it at, left one ([`Operating::accepted`]). A
    /// prerequisite not on the board meets nothing, and `landed`, which this release does not
    /// check, is never met.
    pub fn dependencies_met<'t>(
        &self,
        task: &Task,
        edge: &Edge,
        find: impl Fn(&str) -> Option<&'t Task>,
    ) -> bool {
        match edge.dependencies {
            Dependency::None => true,
            Dependency::Landed => false,
            Dependency::AcceptedOutput => task.depends.iter().all(|slug| {
                (find(slug).and_then(|prerequisite| self.placed(prerequisite))).is_some_and(
                    |placed| {
                        placed.declared.satisfies_dependencies
                            && (self.accepted.iter()).any(|(standing, revision, _)| {
                                standing == slug && *revision == placed.position.revision
                            })
                    },
                )
            }),
            Dependency::Done => task.depends.iter().all(|slug| {
                find(slug).is_some_and(|prerequisite| match self.placed(prerequisite) {
                    Some(placed) => placed.declared.satisfies_dependencies,
                    None => {
                        !crate::store::workflow::position_held(&prerequisite.fields)
                            && prerequisite.status == "done"
                    }
                })
            }),
        }
    }
}

impl<'a> Placed<'a> {
    /// Its pair.
    pub fn at(&self) -> Pair {
        self.position.at()
    }

    /// The edges leaving its pair that give their taker a role, in the definition's order: what
    /// staffing, `next` and PULL offer. An automatic edge is the board's to take, never a claim.
    pub fn claims(&self) -> Vec<Claim<'a>> {
        let at = self.at();
        (self.definition.edges.iter())
            .filter(|edge| edge.from == at && !edge.automatic)
            .flat_map(|edge| {
                edge.effects.iter().filter_map(move |effect| match effect {
                    Effect::AcquireRole { role } => Some(Claim { edge, role: *role }),
                    _ => None,
                })
            })
            .collect()
    }

    /// The edges leaving its pair that cancel it (a `cancel` operation): what the board takes
    /// where its dependency policy is `cancel`.
    pub fn cancels(&self) -> Vec<&'a Edge> {
        let at = self.at();
        (self.definition.edges.iter())
            .filter(|edge| {
                edge.from == at
                    && !edge.automatic
                    && edge.operation == symphony_api::workflow::Operation::Cancel
            })
            .collect()
    }

    /// The role its position staffs, none for a position nobody works at.
    pub fn staffing(&self) -> Option<Role> {
        self.declared.staffing
    }

    /// Whether a session that is there ([`crate::Health::holds`]) holds the role its position
    /// staffs: the work it stands for is being done.
    pub fn working(&self, task: &Task) -> bool {
        self.staffing()
            .is_some_and(|role| present(task, super::admit::word(&role).as_str()))
    }

    /// The holders of the role its position staffs whose sessions are lost or have left, none
    /// while a session that is there holds it beside them: the holders its `lost_holder` recovery
    /// answers.
    pub fn lost(&self, task: &'a Task) -> Vec<&'a str> {
        let Some(role) = self.staffing() else {
            return Vec::new();
        };
        let role = super::admit::word(&role);
        if present(task, &role) {
            return Vec::new();
        }
        (task.roles.iter())
            .filter(|r| r.role == role && !r.health.holds())
            .map(|r| r.handle.as_str())
            .collect()
    }

    /// Whether a recovery already answered `holder`'s exit here and left the task waiting on a
    /// person: recovering it again would record the same stay once more.
    pub fn waits_on_person_for(&self, holder: &str) -> bool {
        (self.position.manual.as_ref()).is_some_and(|manual| manual.holder == holder)
    }

    /// Whether it holds a task slot: its position's policy is `acquire` or `retain`.
    pub fn holds_slot(&self) -> bool {
        self.declared.slot != Slot::Release
    }

    /// Whether it is finished: its position is terminal, recording its outcome.
    pub fn finished(&self) -> bool {
        self.declared.terminal
    }
}

/// Whether a session that is there holds `role` on `task`.
fn present(task: &Task, role: &str) -> bool {
    (task.roles.iter()).any(|r| r.role == role && r.health.holds())
}

fn position_of<'a>(definition: &'a Definition, pair: &Pair) -> Option<&'a Position> {
    definition.positions.iter().find(|p| p.at == *pair)
}

#[cfg(test)]
#[path = "operate_tests.rs"]
pub(crate) mod tests;
