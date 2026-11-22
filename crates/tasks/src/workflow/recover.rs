//! Recovery of a holder's exit on a task an active custom workflow holds (docs/workflow/WORKFLOW.md,
//! "Recovery"): an agent's own release of its role there, or a holder lost, answered by the
//! position's configured action (`release`, `lost_holder`), never by a target the board invents.
//! What the store read decides which role rows go, whether the task stays, waiting on a person
//! when the action says manual, or moves to the action's target, and its slot. Nothing here
//! writes: the store fences the write on everything read here, as it fences an admitted operation.
//!
//! A role the task's position does not staff (an executor's left after its work went to review, a
//! reservation) is the holder's alone to let go: it goes, and the position stays as it was. The
//! orchestrator's role is the board's coordination, never a workflow's.
//!
//! An execution attempt's ending is answered the same way ([`observed`]): its failure by the
//! automatic edge leaving the task's pair, while that edge's retry bound lasts and admission takes
//! it, else by the position's `attempt_failure` action; an acknowledged cancel by its
//! `cancellation_acknowledged` action. Only from where the task stood when the attempt was
//! admitted: a task moved since is another's work, and the ending stays on its attempt.

use super::admit::{Context, Facts, Refusal, word};
use super::{Recovered, TaskPosition, Transition, Trigger};
use symphony_api::workflow::{
    Definition, Edge, Effect, Operation, Pair, Position, RecoveryAction, RefusalCode, Slot,
};
use symphony_plugin::execution::Principal;

/// What a recovery writes: the move or stay it records, none when the holder held no role the
/// position staffs, the role rows it drops, and whether its move takes a task slot under the cap.
#[derive(Clone, Debug, PartialEq)]
pub struct Recovery {
    pub transition: Option<Transition>,
    pub drop: Vec<(String, String)>,
    pub acquires: bool,
}

/// `holder`'s exit from `role` on the task `facts` read (every role it holds there but the
/// orchestrator's, for none), answered for `trigger` as `principal` at `at`, or refused: a task
/// no workflow holds, a holder holding none of it, and an agent's own release of the role a
/// position keeps when released (its action's `release_role` false), which an edge takes it out of.
pub fn recover(
    context: Context,
    facts: &Facts,
    holder: &str,
    role: Option<&str>,
    trigger: Trigger,
    principal: &Principal,
    at: f64,
) -> Result<Recovery, Refusal> {
    let slug = &facts.slug;
    let position = facts.position.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::UnknownId,
            format!("no custom workflow holds {slug}"),
        )
    })?;
    let here = position.at();
    let declared = position_of(context.definition, &here).ok_or_else(|| {
        Refusal::new(
            RefusalCode::StaleWorkflow,
            format!(
                "{slug} is at {}, which the active workflow does not declare",
                named(&here)
            ),
        )
    })?;
    let held: Vec<(String, String)> = (facts.roles.iter())
        .filter(|(h, r)| h == holder && r != "orchestrator" && role.is_none_or(|role| r == role))
        .cloned()
        .collect();
    if held.is_empty() {
        return Err(Refusal::new(
            RefusalCode::UnknownId,
            format!("{holder} holds no {} on {slug}", role.unwrap_or("role")),
        ));
    }
    let staffing = declared.staffing.map(|r| word(&r));
    let (staffed, mut drop): (Vec<_>, Vec<_>) =
        (held.into_iter()).partition(|(_, r)| staffing.as_deref() == Some(r.as_str()));
    let Some((_, staffed_role)) = staffed.first().cloned() else {
        return Ok(Recovery {
            transition: None,
            drop,
            acquires: false,
        });
    };
    let recovery = declared.recovery.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::UnsupportedActivation,
            format!(
                "{} is staffed by its {staffed_role} and declares no recovery",
                named(&here)
            ),
        )
    })?;
    let action = match trigger {
        Trigger::Release => &recovery.release,
        Trigger::LostHolder => &recovery.lost_holder,
        Trigger::AttemptFailure | Trigger::CancellationAcknowledged => {
            return Err(Refusal::new(
                RefusalCode::UnknownId,
                format!(
                    "{} is an attempt's ending, not a holder's exit",
                    trigger.as_str()
                ),
            ));
        }
    };
    if action.release_role {
        drop.extend(staffed);
    } else if trigger == Trigger::Release {
        return Err(Refusal::new(
            RefusalCode::ExplicitActionRequired,
            format!(
                "{} keeps its {staffed_role} when released (its recovery's release keeps the \
                 role): take an edge out of it (`just tasks workflow actions {slug}`)",
                named(&here)
            ),
        ));
    }
    if action.release_reservation {
        drop.extend(
            (facts.roles.iter())
                .filter(|(_, r)| r == "reserved")
                .cloned(),
        );
    }
    drop.sort();
    drop.dedup();
    let recovered = Recovered {
        trigger,
        holder: holder.to_string(),
        role: staffed_role,
        manual: action.manual,
    };
    Ok(Recovery {
        transition: Some(acted(
            context,
            facts,
            position,
            declared,
            action,
            Some(Operation::Release),
            recovered,
            principal,
            at,
        )),
        drop,
        acquires: action.slot == Slot::Acquire,
    })
}

/// The transition `action`, the recovery of `declared`, the position the task `facts` read stands
/// at, records: a move to its target or a stay, the role it lets go and its slot effect, what it
/// `recovered`, and the accepted output a stay at a satisfying position keeps.
#[expect(
    clippy::too_many_arguments,
    reason = "one transition's parts, each read once"
)]
fn acted(
    context: Context,
    facts: &Facts,
    position: &TaskPosition,
    declared: &Position,
    action: &RecoveryAction,
    operation: Option<Operation>,
    recovered: Recovered,
    principal: &Principal,
    at: f64,
) -> Transition {
    let here = position.at();
    let to = action.target.clone().unwrap_or_else(|| here.clone());
    let destination = position_of(context.definition, &to)
        .expect("a validated definition declares every recovery target");
    let mut effects = Vec::new();
    if action.release_role
        && let Some(role) = declared.staffing
    {
        effects.push(Effect::ReleaseRole { role });
    }
    effects.push(Effect::Slot {
        policy: action.slot,
    });
    let revision = position.revision + 1;
    let slug = &facts.slug;
    Transition {
        id: Transition::id_of(slug, revision),
        slug: slug.clone(),
        revision,
        generation: context.generation,
        digest: context.revision.digest.clone(),
        edge: None,
        operation,
        activation: None,
        from: Some(here),
        // A stay keeps the pair a blocked task resumes to; a move goes to ordinary work.
        resume: action
            .target
            .is_none()
            .then(|| position.resume.clone())
            .flatten(),
        to,
        status: super::activate::projection(destination.legacy),
        principal: principal.clone(),
        effects,
        key: None,
        recovery: Some(recovered),
        // A stay keeps the accepted output standing; a recovery never accepts one.
        accepted: super::admit::accepted_after(
            Some(declared),
            destination,
            facts.accepted.as_deref(),
            None,
        ),
        at,
    }
}

/// An execution attempt's ending as the board observed it (`crate::Store::observe_attempt`): the
/// attempt, what its ending answers, the role and handle it ran as, and the workflow revision and
/// pair its task stood at when it was admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub attempt: String,
    pub trigger: Trigger,
    pub role: String,
    pub handle: Option<String>,
    pub revision: u64,
    pub at: Pair,
}

impl Observation {
    /// What a transition answering this ending records it recovered: its trigger, and the handle
    /// the attempt ran as, or the attempt itself where it ran as none, in its role; no person's
    /// wait yet, which the action answering it decides.
    pub fn recovered(&self) -> Recovered {
        Recovered {
            trigger: self.trigger,
            holder: self.handle.clone().unwrap_or_else(|| self.attempt.clone()),
            role: self.role.clone(),
            manual: false,
        }
    }
}

/// What an attempt's ending proposes: the automatic edge the board takes through admission,
/// recording what it answered, or a recovery written as a holder's is, keyed `attempt/ID`, with
/// the automatic edge whose retry bound it exhausted, if it did.
#[derive(Clone, Debug, PartialEq)]
pub enum Proposal {
    Edge {
        edge: String,
        answered: Recovered,
    },
    Recovery {
        recovery: Box<Recovery>,
        exhausted: Option<String>,
    },
}

/// What `observation` proposes for the task `facts` read, whose recorded `transitions` these are,
/// as `principal` at `at`; or refused, nothing written: a task no workflow holds; one that moved
/// since the attempt was admitted (`stale_task`), whose ending stays on the attempt; a pair the
/// workflow no longer declares; and a position that declares no action for it, where the task
/// stays as it was.
///
/// A failure takes the automatic edge leaving the pair while the board has taken it fewer times
/// than its retry bound since the task last stood at the bound's `exhausted` pair; past it, the
/// position's `attempt_failure` action, sent to that pair to wait on a person. With no automatic
/// edge, or one admission `refused` the board's take of (a dependency not yet met, a slot full),
/// a failure is the position's `attempt_failure` action, and an acknowledged cancel its
/// `cancellation_acknowledged` one. An action releases the position's staffed role, whoever
/// holds it, and every reservation, as it says.
pub fn observed(
    context: Context,
    facts: &Facts,
    observation: &Observation,
    transitions: &[Transition],
    refused: bool,
    principal: &Principal,
    at: f64,
) -> Result<Proposal, Refusal> {
    let slug = &facts.slug;
    let position = facts.position.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::UnknownId,
            format!("no custom workflow holds {slug}"),
        )
    })?;
    let here = position.at();
    let id = &observation.attempt;
    if position.revision != observation.revision || here != observation.at {
        return Err(Refusal::new(
            RefusalCode::StaleTask,
            format!(
                "{slug} is at {} (workflow revision {}), not at {} (revision {}) where attempt \
                 {id} was admitted: its ending stays on the attempt and moves nothing",
                named(&here),
                position.revision,
                named(&observation.at),
                observation.revision
            ),
        ));
    }
    let declared = position_of(context.definition, &here).ok_or_else(|| {
        Refusal::new(
            RefusalCode::StaleWorkflow,
            format!(
                "{slug} is at {}, which the active workflow does not declare",
                named(&here)
            ),
        )
    })?;
    let mut answered = observation.recovered();
    let mut exhausted = None;
    let actions = declared.recovery.as_ref();
    let action = match observation.trigger {
        Trigger::AttemptFailure => {
            let automatic: Vec<&Edge> = (context.definition.edges.iter())
                .filter(|e| e.automatic && e.from == here)
                .collect();
            if let [_, second, ..] = automatic[..] {
                return Err(Refusal::new(
                    RefusalCode::UnsupportedActivation,
                    format!(
                        "more than one automatic edge leaves {}, {second} among them: the board \
                         cannot choose",
                        named(&here),
                        second = second.id
                    ),
                ));
            }
            let failure = actions.map(|a| a.attempt_failure.clone());
            let bound = automatic.first().and_then(|edge| {
                let retry = edge.retry.as_ref()?;
                (taken(transitions, edge, &retry.exhausted) >= retry.maximum)
                    .then_some((edge, retry))
            });
            match (automatic.first(), bound) {
                (_, Some((edge, retry))) => {
                    // Exhausted: the failure's own action, sent where the bound says.
                    exhausted = Some(edge.id.clone());
                    RecoveryAction {
                        target: Some(retry.exhausted.clone()),
                        manual: true,
                        ..failure.unwrap_or(RecoveryAction {
                            target: None,
                            manual: true,
                            slot: Slot::Retain,
                            release_role: false,
                            release_reservation: false,
                        })
                    }
                }
                (Some(edge), None) if !refused => {
                    return Ok(Proposal::Edge {
                        edge: edge.id.clone(),
                        answered,
                    });
                }
                // No automatic edge, or admission refused the board's take of it.
                _ => failure.ok_or_else(|| undeclared(slug, &here, observation.trigger))?,
            }
        }
        Trigger::CancellationAcknowledged => {
            actions
                .map(|a| a.cancellation_acknowledged.clone())
                .ok_or_else(|| undeclared(slug, &here, observation.trigger))?
        }
        trigger @ (Trigger::Release | Trigger::LostHolder) => {
            return Err(Refusal::new(
                RefusalCode::UnknownId,
                format!(
                    "{} is a holder's exit, not an attempt's ending",
                    trigger.as_str()
                ),
            ));
        }
    };
    answered.manual = action.manual;
    let staffing = declared.staffing.map(|r| word(&r));
    let mut drop: Vec<(String, String)> = (facts.roles.iter())
        .filter(|(_, r)| {
            (action.release_role && staffing.as_deref() == Some(r.as_str()))
                || (action.release_reservation && r == "reserved")
        })
        .cloned()
        .collect();
    drop.sort();
    drop.dedup();
    let transition = Transition {
        key: Some(format!("attempt/{id}")),
        ..acted(
            context, facts, position, declared, &action, None, answered, principal, at,
        )
    };
    Ok(Proposal::Recovery {
        recovery: Box::new(Recovery {
            transition: Some(transition),
            drop,
            acquires: action.slot == Slot::Acquire,
        }),
        exhausted,
    })
}

/// How many times the board took the automatic edge `edge` on an attempt's ending, of the task
/// whose `transitions` these are, since the task last stood at `exhausted`, where its retry bound
/// sends it: whoever brought it there (the bound itself, a person or an activation), its next
/// failures start a fresh count. A caller's take of an edge of that id, under a definition where
/// it was not automatic, is no take of the board's. Activation keeps `exhausted` off every cycle
/// through the edge ([`super::activate::supported`]), so no lap of its retries restarts the count.
fn taken(transitions: &[Transition], edge: &Edge, exhausted: &Pair) -> u32 {
    // An edge's transition records what it recovered only where the board took it.
    let board =
        |t: &&Transition| t.edge.as_deref() == Some(edge.id.as_str()) && t.recovery.is_some();
    let since = (transitions.iter().rev()).take_while(|t| t.to != *exhausted);
    since.filter(board).count() as u32
}

fn undeclared(slug: &str, here: &Pair, trigger: Trigger) -> Refusal {
    Refusal::new(
        RefusalCode::ExplicitActionRequired,
        format!(
            "{} declares no {} recovery: {slug} stays as it was",
            named(here),
            trigger.as_str()
        ),
    )
}

fn position_of<'a>(definition: &'a Definition, pair: &Pair) -> Option<&'a Position> {
    definition.positions.iter().find(|p| p.at == *pair)
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

#[cfg(test)]
#[path = "recover_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "recover_attempt_tests.rs"]
pub(crate) mod attempt_tests;
