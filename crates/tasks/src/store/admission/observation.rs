//! An execution attempt's ending proposed to the workflow that holds its task (docs/workflow/WORKFLOW.md,
//! "Attempts"), decided by [`crate::workflow::recover::observed`] on admission's one read and
//! written as the board's recovery service: an automatic edge through the one admission
//! ([`Store::apply_operation`]'s), any other answer as a holder's recovery is written (`pg::workflow::apply`).
//!
//! The ending is the attempt's own record, written under its worker's lease
//! (`crate::execution::Coordinator`), and it moves the task only from where the task stood when
//! the attempt was admitted, which the attempt's row keeps: a task moved, reassigned or cancelled
//! since keeps its place, and the ending stays on its attempt. An attempt's ending is answered once:
//! the transition that answers it names it, and a second observation answers that transition.
//!
//! A failure is never left unanswered by a refusal that may pass. An automatic edge admission
//! refuses the board's take of (a dependency not yet met, a slot full) gives way to the position's
//! `attempt_failure` action, decided again as if no automatic edge left the pair; and a failure
//! nothing can answer (that action refused too, or none declared) is said on its task's thread
//! once, under its key, which is then its answer: a later observation writes nothing.

use super::recovery::{applied, ended, said, vars};
use super::{Decided, Error, Failure, Host, Refusal, RefusalCode, Store, context, refusal};
use crate::execution::{Cancellation, Ending, Row};
use crate::store::messages::BOARD;
use crate::workflow::admit::word;
use crate::workflow::recover::{self as plan, Observation, Proposal, Recovery};
use crate::workflow::{Recovered, Transition, Trigger, board_service};
use serde_json::json;
use std::collections::BTreeMap;
use symphony_api::workflow::{OperationRequest, Pair, PositionSnapshot};
use symphony_plugin::execution::Principal;

/// What a decision on the board's read leaves to do.
enum Next {
    /// The transition that answers the attempt's ending, written now or before.
    Answered(Transition),
    /// No transition answers it: its thread said once that nothing could.
    Said,
    /// The automatic edge to take through admission, by this request, answering this.
    Take {
        request: OperationRequest,
        answered: Recovered,
    },
}

/// The board's take of automatic edge `edge` that admission refused, `why`, for a reason that may
/// pass.
struct Refused {
    edge: String,
    why: String,
}

impl Store {
    /// Execution attempt `attempt`'s ending proposed to the workflow that holds its task, as the
    /// board's recovery service, which `host` must grant `recovery`: a failure (it ended failed or
    /// rejected, no cancel asked) takes the automatic edge leaving the task's pair within its
    /// retry bound, or, past it or where admission refuses the take, the position's
    /// `attempt_failure` action; a cancel known to have taken, the position's
    /// `cancellation_acknowledged` action. Answers the transition that answers it, written now or
    /// by an earlier observation of the same ending, and none where no transition does: its ending
    /// proposes nothing (it is still live, it succeeded, stopped or was superseded, a later attempt
    /// holds its lane, or no workflow held its task when it was admitted), or it is a failure
    /// nothing could answer, which its thread says once. Refused, nothing written, where its task
    /// moved since it was admitted (`stale_task`) or no workflow holds it now, and where its
    /// position declares no action for a cancel.
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
        let decide = |refused: Option<&Refused>| {
            self.retrying(|| self.next(slug, &observation, &principal, refused))
        };
        let (request, answered) = match decide(None)? {
            Next::Answered(transition) => return Ok(Some(transition)),
            Next::Said => return Ok(None),
            Next::Take { request, answered } => (request, answered),
        };
        // Admission decides again on its own read, fenced on the task's revision, and a repeat of
        // the request answers what the first wrote.
        let why = match self.apply(&principal, host, &request, &[], Some(&answered)) {
            Ok(_) => {
                let transitions = self.workflow_transitions(slug)?;
                return Ok(answering(&transitions, attempt).cloned());
            }
            Err(Error::Refused(why)) if may_pass(&why) => why,
            Err(error) => return Err(error),
        };
        let refused = Refused {
            edge: request.edge,
            why,
        };
        match decide(Some(&refused))? {
            Next::Answered(transition) => Ok(Some(transition)),
            Next::Said => Ok(None),
            Next::Take { .. } => unreachable!("a take admission refused is not proposed again"),
        }
    }

    /// What answers `observation`'s ending of `slug` on the board's read, as `principal`: the
    /// transition that answered it, the line that said nothing could, the automatic edge to take,
    /// or what [`Store::proposed`] writes now. A failure refused for a reason that may pass is
    /// said once on the thread, which is then its answer.
    fn next(
        &self,
        slug: &str,
        observation: &Observation,
        principal: &Principal,
        refused: Option<&Refused>,
    ) -> Result<Next, Failure> {
        let attempt = &observation.attempt;
        let transitions = self.workflow_transitions(slug).map_err(Failure::Stop)?;
        if let Some(answered) = answering(&transitions, attempt) {
            return Ok(Next::Answered(answered.clone()));
        }
        if self.keyed(slug, &key(attempt))? {
            return Ok(Next::Said);
        }
        match self.proposed(slug, observation, &transitions, principal, refused) {
            Err(Failure::Stop(Error::Refused(why)))
                if observation.trigger == Trigger::AttemptFailure && may_pass(&why) =>
            {
                let line = unanswered(slug, observation, refused, &why);
                self.record_once(slug, &sender(principal), &line, &key(attempt))?;
                Ok(Next::Said)
            }
            next => next,
        }
    }

    /// What `observation`'s ending of `slug`, whose recorded `transitions` these are, proposes on
    /// admission's read, as `principal`: the automatic edge to take, or a recovery, written now.
    /// With `refused`, the board's take of the automatic edge admission refused, it is answered as
    /// if no automatic edge left the pair, and the line says why.
    fn proposed(
        &self,
        slug: &str,
        observation: &Observation,
        transitions: &[Transition],
        principal: &Principal,
        refused: Option<&Refused>,
    ) -> Result<Next, Failure> {
        let decided = self.decided(slug, principal, None)?;
        let at = self.now();
        let proposal = plan::observed(
            context(&decided),
            &decided.facts,
            observation,
            transitions,
            refused.is_some(),
            principal,
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
                    idempotency_key: key(&observation.attempt),
                    inputs: BTreeMap::new(),
                },
                answered,
            }),
            Proposal::Recovery {
                recovery,
                exhausted,
            } => {
                let why = match (exhausted, refused) {
                    (Some(edge), _) => Some(format!("{edge}'s retry bound is exhausted")),
                    (None, Some(Refused { edge, why })) => {
                        Some(format!("the board could not take {edge}: {why}"))
                    }
                    (None, None) => None,
                };
                self.recovered(&decided, &recovery, why.as_deref(), principal, at)?;
                let transition = recovery.transition.expect("an ending's recovery moves");
                Ok(Next::Answered(transition))
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
    /// active workflow, the task's version and each role it lets go; its line ends with `why` the
    /// board took no automatic edge, where it says.
    fn recovered(
        &self,
        decided: &Decided,
        recovery: &Recovery,
        why: Option<&str>,
        principal: &Principal,
        at: f64,
    ) -> Result<(), Failure> {
        let slots = self.recovery_slots(decided, recovery, principal)?;
        let guarded = !slots.is_empty();
        // No holder's roles relied on, as an attempt names none, and no session to check is still
        // there: the board's write, said by its recovery service.
        let mut vars = vars(decided, recovery, "", None, Vec::new(), slots, BOARD, false, at);
        vars["sender"] = json!(sender(principal));
        if let Some(why) = why {
            vars["said"] = json!(format!("{}; {why}", said(recovery, "")));
        }
        self.written(applied(guarded), vars, &decided.task.slug)?;
        Ok(())
    }
}

/// The key of every answer to attempt `attempt`'s ending: its transition's, or its thread line's
/// where nothing could answer it.
fn key(attempt: &str) -> String {
    format!("attempt/{attempt}")
}

/// The transition that answered `attempt`'s ending, if one did.
fn answering<'a>(transitions: &'a [Transition], attempt: &str) -> Option<&'a Transition> {
    let key = key(attempt);
    (transitions.iter()).find(|t| t.recovery.is_some() && t.key.as_deref() == Some(key.as_str()))
}

/// Whether `why`, a refusal of what answers an attempt's ending, may pass: a refusal's, by its
/// code, but a pin gone stale (the task, its workflow or its activation moved since the attempt
/// was admitted) or a task no workflow holds (none is active, or it holds no such task), which
/// nothing answers however long it waits.
fn may_pass(why: &str) -> bool {
    let silent = [
        RefusalCode::StaleTask,
        RefusalCode::StaleWorkflow,
        RefusalCode::StaleActivation,
        RefusalCode::UnknownId,
        RefusalCode::UnsupportedActivation,
    ];
    (why.split_once(": ")).is_some_and(|(code, _)| !silent.iter().any(|c| word(c) == code))
}

/// The thread's line for `observation`'s failure of `slug` that nothing could answer, `why`:
/// after admission `refused` the board's take of its automatic edge, where it did.
fn unanswered(
    slug: &str,
    observation: &Observation,
    refused: Option<&Refused>,
    why: &str,
) -> String {
    let what = ended(&observation.recovered(), &observation.attempt);
    let why = match refused {
        Some(refused) => format!(
            "the board could not take {} ({}), nor its position's attempt_failure action ({why})",
            refused.edge, refused.why
        ),
        None => why.to_string(),
    };
    format!(
        "the board's recovery: {what}, and nothing answers it: {why}; a person does \
         (`just tasks workflow actions {slug}`)"
    )
}

/// Who the thread names as the board's service `principal`.
fn sender(principal: &Principal) -> String {
    format!("{}/{}", principal.issuer, principal.subject)
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
