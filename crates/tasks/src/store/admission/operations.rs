//! The board's operations on tasks a custom workflow holds (docs/WORKFLOW.md, "Operations"): the
//! active workflow read once for a command's readers ([`Store::workflow_operating`]), the pass that
//! settles what no session asked for ([`Store::settle_held`]: a lost holder's recovery, and a
//! dependant's policy when a prerequisite ended without success), and a claim named by the role it
//! gives rather than by its edge ([`Store::claim_by_workflow`]). Each goes through the board's one
//! admission ([`Store::apply_operation`]) or its recovery ([`Store::recover_holder`]): nothing here
//! writes a position of its own.

use super::{Error, Host, Refusal, RefusalCode, Store};
use crate::Board;
use crate::store::messages::BOARD;
use crate::workflow::operate::{Operating, Placed};
use crate::workflow::{admit::word, board_service};
use std::collections::BTreeMap;
use symphony_api::workflow::{
    Authority, NextActions, OperationRequest, OperationResult, Role, UnsuccessfulDependency,
};
use symphony_plugin::execution::Principal;

/// What one pass of [`Store::settle_held`] did, each line as the pass says it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settled {
    pub lines: Vec<String>,
}

impl Store {
    /// The active custom workflow, read once for every task a command's readers look at; none on a
    /// board that never activated one.
    pub fn workflow_operating(&self) -> Result<Option<Operating>, Error> {
        Ok(self.workflow_read()?.map(Operating::from))
    }

    /// [`Store::workflow_operating`] for a reader of `tasks`: none, and nothing read, where none of
    /// them is a task a workflow holds, so a board with no workflow reads as it always did.
    pub fn operating_for(&self, tasks: &[crate::Task]) -> Result<Option<Operating>, Error> {
        if !(tasks.iter()).any(|t| crate::store::workflow::position_held(&t.fields)) {
            return Ok(None);
        }
        self.workflow_operating()
    }

    /// Whether `task` is finished by its recorded facts ([`Operating::finished`]): a task a
    /// custom workflow holds when its position is terminal, never by the status it projects to;
    /// any other when the board's writers keep it done or cancelled. Reads the workflow only for a
    /// task one holds.
    pub fn finished(&self, task: &crate::Task) -> Result<bool, Error> {
        let operating = self.operating_for(std::slice::from_ref(task))?;
        Ok(crate::workflow::operate::finished(operating.as_ref(), task))
    }

    /// [`Store::settle_held`] where `board` holds anything to settle, its failure said on stderr:
    /// what a PULL and an orchestrator's monitor run beside the board's gives. Whether it wrote.
    pub fn settle_held_if_due(&self, board: &Board) -> bool {
        match self.settle_held(board) {
            Ok(settled) => !settled.lines.is_empty(),
            Err(error) => {
                eprintln!("settling the tasks a workflow holds deferred: {error}");
                false
            }
        }
    }

    /// What the board does by itself for the tasks a custom workflow holds on `board`, each by the
    /// position's own configuration and never a target it invents:
    ///
    /// - a holder of the role a position staffs whose session is lost or left is recovered by the
    ///   position's `lost_holder` action, as the board's recovery service
    ///   ([`Store::recover_holder`]), once: a stay that waits on a person for that holder is not
    ///   recorded again;
    /// - a task with a prerequisite that ended without success, at a position whose
    ///   `unsuccessful_dependency` is `cancel`, takes the one cancel edge leaving its pair whose
    ///   authority the recovery service holds. `block` and `manual` move nothing: admission's
    ///   dependency predicates refuse the edges that read them, and PULL lists the task.
    ///
    /// Each is fenced as the operation it is, so a holder that comes back, or a session that moves
    /// the task first, wins; a refusal is said in the pass's lines, and the pass goes on.
    pub fn settle_held(&self, board: &Board) -> Result<Settled, Error> {
        let Some(operating) = self.operating_for(&board.tasks)? else {
            return Ok(Settled::default());
        };
        let host = Host::local();
        let mut settled = Settled::default();
        let find = |slug: &str| board.tasks.iter().find(|t| t.slug == slug);
        for task in &board.tasks {
            let Some(placed) = operating.placed(task) else {
                continue;
            };
            if placed.finished() {
                continue;
            }
            for holder in placed.lost(task) {
                if placed.waits_on_person_for(holder) {
                    continue;
                }
                let line = match self.recover_holder(&task.slug, holder, BOARD, &host) {
                    Ok(_) => format!("recovered {holder} on {}", task.slug),
                    Err(error) => format!("{holder} on {} not recovered: {error}", task.slug),
                };
                settled.lines.push(line);
            }
            if placed.declared.unsuccessful_dependency == UnsuccessfulDependency::Cancel
                && !operating.unsuccessful(task, find).is_empty()
                && let Some(line) = self.cancel_dependant(&operating, &placed, task, &host)
            {
                settled.lines.push(line);
            }
        }
        Ok(settled)
    }

    /// `task` cancelled by the one cancel edge leaving its pair whose authority the board's
    /// recovery service holds, as that service; none where no such edge leaves it, which PULL
    /// then lists for a person.
    fn cancel_dependant(
        &self,
        operating: &Operating,
        placed: &Placed<'_>,
        task: &crate::Task,
        host: &Host,
    ) -> Option<String> {
        let service = board_service("recovery");
        let edges: Vec<_> = (placed.cancels().into_iter())
            .filter(|edge| {
                (edge.authority.iter()).any(|a| match a {
                    Authority::SystemCapability { capability }
                    | Authority::ProjectCapability { capability } => {
                        host.grants.holds(&service, capability)
                    }
                    Authority::TaskRole { .. } => false,
                })
            })
            .collect();
        let [edge] = edges[..] else {
            return None;
        };
        let snapshot = crate::workflow::admit::snapshot(
            crate::workflow::admit::Context {
                definition: &operating.definition,
                revision: &operating.revision,
                generation: operating.generation,
            },
            &placed.position,
        );
        let request = OperationRequest {
            project: self.database().database().to_string(),
            task: task.slug.clone(),
            edge: edge.id.clone(),
            input_revisions: BTreeMap::new(),
            idempotency_key: format!(
                "{}@{}:{}:unsuccessful_dependency",
                task.slug, snapshot.task_revision, edge.id
            ),
            inputs: BTreeMap::new(),
            expected: snapshot,
        };
        Some(match self.apply_operation(&service, host, &request) {
            Ok(result) => format!(
                "cancelled {} by {} ({}): a prerequisite ended without success",
                task.slug, edge.id, result.transition
            ),
            Err(error) => format!("{} not cancelled by {}: {error}", task.slug, edge.id),
        })
    }

    /// `role` taken on `slug` by `principal` through the one claim edge leaving its pair that
    /// gives it, at the snapshot read now: the workflow-aware form of the board's claim (the
    /// analysis's legacy alias), which reads the task's next actions and sends the revision it read
    /// there. Refused as `explicit_action_required`, naming the edges, where no edge or several
    /// open ones give the role; admission refuses the one edge for what stops it.
    pub fn claim_by_workflow(
        &self,
        slug: &str,
        principal: &Principal,
        host: &Host,
        role: Role,
    ) -> Result<OperationResult, Error> {
        let Some(operating) = self.workflow_operating()? else {
            return Err(refused(
                RefusalCode::UnsupportedActivation,
                "no custom workflow is active on this board: the board's own claim keeps its tasks",
            ));
        };
        let next = self.workflow_actions(slug, principal, host)?;
        let giving: Vec<&str> = (operating.definition.edges.iter())
            .filter(|edge| edge.from == next.snapshot.at && !edge.automatic)
            .filter(|edge| {
                (edge.effects.iter()).any(|e| {
                    matches!(e, symphony_api::workflow::Effect::AcquireRole { role: r } if *r == role)
                })
            })
            .map(|edge| edge.id.as_str())
            .collect();
        let edge = match &giving[..] {
            [] => {
                return Err(refused(
                    RefusalCode::ExplicitActionRequired,
                    format!(
                        "no edge leaving {slug}'s {} gives the {} role: read its edges with `just \
                         tasks workflow actions {slug}`",
                        named(&next.snapshot.at),
                        word(&role)
                    ),
                ));
            }
            [one] => *one,
            several => match &open(&next, several)[..] {
                [one] => *one,
                _ => {
                    return Err(refused(
                        RefusalCode::ExplicitActionRequired,
                        format!(
                            "{} edges leaving {slug}'s {} give the {} role ({}): take one with \
                             `just tasks workflow apply {slug} EDGE --revision {}`",
                            several.len(),
                            named(&next.snapshot.at),
                            word(&role),
                            several.join(", "),
                            next.snapshot.task_revision
                        ),
                    ));
                }
            },
        };
        let request = OperationRequest {
            project: self.database().database().to_string(),
            task: slug.to_string(),
            edge: edge.to_string(),
            input_revisions: BTreeMap::new(),
            idempotency_key: format!("{slug}@{}:{edge}", next.snapshot.task_revision),
            inputs: BTreeMap::new(),
            expected: next.snapshot,
        };
        self.apply_operation(principal, host, &request)
    }
}

/// Of `edges`, those `next` shows nothing stopping now.
fn open<'a>(next: &NextActions, edges: &[&'a str]) -> Vec<&'a str> {
    (edges.iter())
        .filter(|edge| {
            (next.actions.iter()).any(|a| a.edge == **edge && a.unavailable.is_empty())
        })
        .copied()
        .collect()
}

fn refused(code: RefusalCode, message: impl Into<String>) -> Error {
    Error::Refused(Refusal::new(code, message).to_string())
}

fn named(pair: &symphony_api::workflow::Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
