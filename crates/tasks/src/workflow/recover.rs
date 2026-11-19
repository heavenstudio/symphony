//! Recovery of a holder's exit on a task an active custom workflow holds (docs/WORKFLOW.md,
//! "Recovery"): an agent's own release of its role there, or a holder lost, answered by the
//! position's configured action (`release`, `lost_holder`), never by a target the board invents.
//! What the store read decides which role rows go, whether the task stays, waiting on a person
//! when the action says manual, or moves to the action's target, and its slot. Nothing here
//! writes: the store fences the write on everything read here, as it fences an admitted operation.
//!
//! A role the task's position does not staff (an executor's left after its work went to review, a
//! reservation) is the holder's alone to let go: it goes, and the position stays as it was. The
//! orchestrator's role is the board's coordination, never a workflow's.

use super::admit::{Context, Facts, Refusal, word};
use super::{Recovered, Transition, Trigger};
use symphony_api::workflow::{Definition, Effect, Operation, Pair, Position, RefusalCode, Slot};
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
    Ok(Recovery {
        transition: Some(Transition {
            id: Transition::id_of(slug, revision),
            slug: slug.clone(),
            revision,
            generation: context.generation,
            digest: context.revision.digest.clone(),
            edge: None,
            operation: Some(Operation::Release),
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
            recovery: Some(Recovered {
                trigger,
                holder: holder.to_string(),
                role: staffed_role,
                manual: action.manual,
            }),
            // A stay keeps the accepted output standing; a recovery never accepts one.
            accepted: super::admit::accepted_after(
                Some(declared),
                destination,
                facts.accepted.as_deref(),
                None,
            ),
            at,
        }),
        drop,
        acquires: action.slot == Slot::Acquire,
    })
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
