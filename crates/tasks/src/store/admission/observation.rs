//! An execution attempt's ending proposed to the workflow that holds its task (docs/WORKFLOW.md,
//! "Attempts"), decided by [`crate::workflow::recover::observed`] on admission's one read and
//! written as the board's recovery service: an automatic edge through the one admission
//! ([`Store::apply_operation`]'s), any other answer as a holder's recovery is written (`sql::APPLY`).
//!
//! The ending is the attempt's own record, written under its worker's lease
//! (`crate::execution::Coordinator`), and it moves the task only from where the task stood when
//! the attempt was admitted, which the attempt's row keeps: a task moved, reassigned or cancelled
//! since keeps its place, and the ending stays on its attempt. An attempt's ending is answered once:
//! the transition that answers it names it, and a second observation answers that transition.

use super::recovery::{applied, said};
use super::{Decided, Error, Failure, Host, Refusal, RefusalCode, Store, Taking, context, refusal};
use crate::execution::{Cancellation, Ending, Row};
use crate::workflow::recover::{self as plan, Observation, Proposal, Recovery};
use crate::workflow::{Recovered, Transition, Trigger, board_service};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_api::workflow::{OperationRequest, Pair, PositionSnapshot};
use symphony_plugin::execution::Principal;

/// What a decision on the board's read leaves to do.
enum Next {
    /// The transition that answers the attempt's ending, written now or before.
    Answered(Transition),
    /// The automatic edge to take through admission, by this request, answering this.
    Take {
        request: OperationRequest,
        answered: Recovered,
    },
}

impl Store {
    /// Execution attempt `attempt`'s ending proposed to the workflow that holds its task, as the
    /// board's recovery service, which `host` must grant `recovery`: a failure (it ended failed or
    /// rejected, no cancel asked) takes the automatic edge leaving the task's pair within its
    /// retry bound, or the position's `attempt_failure` action; a cancel known to have taken, the
    /// position's `cancellation_acknowledged` action. Answers the transition that answers it,
    /// written now or by an earlier observation of the same ending, and none where its ending
    /// proposes nothing: it is still live, it succeeded, stopped or was superseded, a later attempt
    /// holds its lane, or no workflow held its task when it was admitted. Refused, nothing written,
    /// where its task moved since it was admitted (`stale_task`) or its position declares no action
    /// for it: its ending stays on the attempt.
    ///
    /// The coordinator calls it after each of its own writes that ends an attempt; a host calls it
    /// again for an attempt whose worker died before it did, as often as it likes.
    pub fn observe_attempt(&self, attempt: &str, host: &Host) -> Result<Option<Transition>, Error> {
        let value = self
            .execution_attempt(attempt)?
            .ok_or_else(|| Error::Refused(format!("there is no execution attempt {attempt}")))?;
        let row = Row::of(&value)?;
        let Some(observation) = self.observation(&row)? else {
            return Ok(None);
        };
        let principal = board_service("recovery");
        if !host.grants.holds(&principal, "recovery") {
            return Err(Error::Refused(
                Refusal::new(
                    RefusalCode::Unauthorized,
                    "this host grants the board's recovery service no `recovery`: no attempt's \
                     ending moves a task here",
                )
                .to_string(),
            ));
        }
        let slug = row.task.as_str();
        let next = self.retrying(|| {
            let transitions = self.workflow_transitions(slug).map_err(Failure::Stop)?;
            if let Some(answered) = answering(&transitions, attempt) {
                return Ok(Next::Answered(answered.clone()));
            }
            let decided = self.decided(slug, &principal, None)?;
            let at = self.now();
            let proposal = plan::observed(
                context(&decided),
                &decided.facts,
                &observation,
                &transitions,
                &principal,
                at,
            )
            .map_err(refusal)?;
            match proposal {
                Proposal::Edge { edge, answered } => Ok(Next::Take {
                    request: OperationRequest {
                        project: self.database().database().into(),
                        task: slug.into(),
                        edge,
                        expected: PositionSnapshot {
                            workflow: decided.recorded.revision.clone(),
                            activation_generation: decided.active.generation,
                            task_revision: observation.revision,
                            at: observation.at.clone(),
                        },
                        input_revisions: BTreeMap::new(),
                        idempotency_key: format!("attempt/{attempt}"),
                        inputs: BTreeMap::new(),
                    },
                    answered,
                }),
                Proposal::Recovery {
                    recovery,
                    exhausted,
                } => {
                    self.recovered(&decided, &recovery, exhausted.as_deref(), &principal, at)?;
                    let transition = recovery.transition.expect("an ending's recovery moves");
                    Ok(Next::Answered(transition))
                }
            }
        })?;
        match next {
            Next::Answered(transition) => Ok(Some(transition)),
            Next::Take { request, answered } => {
                // Admission decides again on its own read, fenced on the task's revision, and a
                // repeat of the request answers what the first wrote.
                self.apply(&principal, host, &request, Some(&answered))?;
                let transitions = self.workflow_transitions(slug)?;
                Ok(answering(&transitions, attempt).cloned())
            }
        }
    }

    /// What `row`'s ending proposes, none where it proposes nothing.
    fn observation(&self, row: &Row) -> Result<Option<Observation>, Error> {
        let Some(pinned) = &row.workflow else {
            return Ok(None);
        };
        let state = &row.state;
        if state.live() || state.successor.is_some() {
            return Ok(None);
        }
        let trigger = match (state.cancellation, state.ending) {
            (Cancellation::Confirmed, _) => Trigger::CancellationAcknowledged,
            (Cancellation::None, Some(Ending::Failed | Ending::Rejected)) => {
                Trigger::AttemptFailure
            }
            _ => return Ok(None),
        };
        let identity = &row.identity;
        let lane = self.execution_fence(json!(["lane", row.task, identity.lane]))?;
        if lane.as_ref().and_then(|fence| fence["attempt"].as_str()) != Some(row.attempt.as_str()) {
            return Ok(None);
        }
        Ok(Some(Observation {
            attempt: row.attempt.clone(),
            trigger,
            role: identity.role.clone(),
            handle: identity.handle.clone(),
            revision: pinned.revision,
            at: Pair {
                phase: pinned.phase.clone(),
                state: pinned.state.clone(),
            },
        }))
    }

    /// `recovery` written as the board's recovery service on what `decided` read, fenced on the
    /// active workflow, the task's version and each role it lets go.
    fn recovered(
        &self,
        decided: &Decided,
        recovery: &Recovery,
        exhausted: Option<&str>,
        principal: &Principal,
        at: f64,
    ) -> Result<(), Failure> {
        let transition = recovery
            .transition
            .as_ref()
            .expect("an ending's recovery moves");
        let slug = &decided.task.slug;
        let taking = Taking {
            status: &transition.status,
            acquires: recovery.acquires,
            held: decided.holds_now(),
            holds: decided.holds_at(&transition.to),
            add: &[],
            what: format!(
                "{slug} cannot move to {}/{} by its recovery",
                transition.to.phase, transition.to.state
            ),
        };
        let slots = self.taken(decided, taking, principal)?;
        let guarded = !slots.is_empty();
        let pair = |(h, r): &(String, String)| json!({"handle": h, "role": r});
        let dropped: Vec<Value> = recovery.drop.iter().map(pair).collect();
        let vars = json!({
            "slug": slug,
            "version": decided.task.version,
            "generation": decided.active.generation,
            "digest": decided.active.digest,
            "dependencies": [],
            "holding": dropped,
            "add": [],
            "drop": dropped,
            "seen": [],
            "receipt": null,
            "receipt_body": null,
            "actor": null,
            "transition": transition,
            "position": transition.position(),
            "moved": transition.status != decided.task.status,
            "sender": format!("{}/{}", principal.issuer, principal.subject),
            "said": match exhausted {
                Some(edge) => format!("{}; {edge}'s retry bound is exhausted", said(recovery, "")),
                None => said(recovery, ""),
            },
            "now": at,
            "slots": slots,
        });
        self.written(applied(guarded), vars, slug)?;
        Ok(())
    }
}

/// The transition that answered `attempt`'s ending, if one did.
fn answering<'a>(transitions: &'a [Transition], attempt: &str) -> Option<&'a Transition> {
    let key = format!("attempt/{attempt}");
    (transitions.iter()).find(|t| t.recovery.is_some() && t.key.as_deref() == Some(key.as_str()))
}
