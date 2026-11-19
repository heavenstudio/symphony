//! What activating a committed workflow does to a board, decided before any write: whether this
//! host supports the definition at all, and where each task it holds goes (docs/WORKFLOW.md,
//! docs/DECISIONS.md#configurable-workflow-activation B). The store writes the plan in one
//! transaction fenced on what it read (`crate::store::workflow`); nothing here writes.
//!
//! A pair the new definition still declares keeps its task; any other needs a committed mapping,
//! a blocked task's resume pair too. A task the board's existing writers kept starts from
//! built-in v1's pair of its status (`held` while a role holds a concept, design or review task)
//! and its blocked source. The whole activation is refused when one task cannot be placed, and
//! the previous workflow stays active.
//!
//! A return to the board's own writers is an activation too ([`returning`]): every task a
//! workflow holds goes to a pair of built-in v1, its own where v1 declares it, else its mapped
//! one, and takes that pair's status as its position is retired.

use super::{Custom, Grants, Mapping, TaskPosition, Transition, Validated, builtin};
use std::collections::BTreeMap;
use symphony_api::workflow::{
    Completion, Definition, Dependency, Effect, LegacyStatus, Pair, Position, RecoveryAction,
};
use symphony_plugin::execution::Principal;

/// What the host supports: the capabilities it grants, to whom, and the adapters it registered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Host {
    pub grants: Grants,
    pub adapters: Vec<String>,
}

impl Host {
    /// The local host's own policy, fixed here and never read from a definition: the local
    /// owner holds `owner`, the board's recovery service holds `recovery`, and no adapter is
    /// registered. A definition declaring any other capability is unsupported on this host.
    pub fn local() -> Host {
        Host {
            grants: Grants::none()
                .grant("owner", super::local_owner())
                .grant("recovery", super::board_service("recovery")),
            adapters: Vec::new(),
        }
    }
}

/// The definition refused unless this host supports everything it needs: each capability it
/// declares granted to someone, each adapter registered, and nothing this release cannot run yet.
/// Guards wait on revision-bound guards; attempt-driven recovery, automatic edges and retry bounds
/// wait on durable attempts; `accepted_output` dependencies and parent integration on their own
/// follow-ups. Each refusal says which, so the project knows what it asked for.
pub fn supported(definition: &Validated, host: &Host) -> Result<(), String> {
    let d = definition.definition();
    let granted = host.grants.supported();
    let adapters: Vec<&str> = host.adapters.iter().map(String::as_str).collect();
    super::prerequisites(
        definition,
        &granted.into_iter().collect::<Vec<_>>(),
        &adapters,
    )?;
    if let Some(guard) = d.guards.first() {
        return Err(format!(
            "unsupported guard: {} (guards answer once revision-bound guards land, \
             tools/workflow-revision-guards)",
            guard.id
        ));
    }
    let attempts = "needs durable attempts (tools/workflow-attempt-integration)";
    for position in &d.positions {
        let at = named(&position.at);
        if let Some(why) = unsupported_dependency(position.dependencies) {
            return Err(format!("unsupported dependency at {at}: {why}"));
        }
        if let Some(recovery) = &position.recovery {
            for (name, action) in [
                ("attempt_failure", &recovery.attempt_failure),
                (
                    "cancellation_acknowledged",
                    &recovery.cancellation_acknowledged,
                ),
            ] {
                if moves(action) {
                    return Err(format!("unsupported recovery at {at}: {name} {attempts}"));
                }
            }
        }
    }
    for edge in &d.edges {
        let id = &edge.id;
        if edge.automatic {
            return Err(format!(
                "unsupported edge {id}: an automatic edge {attempts}"
            ));
        }
        if edge.retry.is_some() {
            return Err(format!("unsupported edge {id}: a retry bound {attempts}"));
        }
        if let Some(why) = unsupported_dependency(edge.dependencies) {
            return Err(format!("unsupported edge {id}: a dependency on {why}"));
        }
        for effect in &edge.effects {
            let unsupported = match effect {
                Effect::ParentIntegration => "parent integration",
                // No adapter runs here yet: a prepared intent never completes on a caller's word.
                Effect::Prepare { .. } | Effect::Finalize { .. } => {
                    "an adapter's prepare or finalize"
                }
                _ => continue,
            };
            return Err(format!(
                "unsupported edge {id}: {unsupported} (tools/workflow-landing-adapters)"
            ));
        }
    }
    Ok(())
}

/// Why this release cannot check `dependency`, none for one it checks (`none`, `done`): an
/// accepted output, and a landing, which the board knows only from its thread and its git.
fn unsupported_dependency(dependency: Dependency) -> Option<&'static str> {
    match dependency {
        Dependency::None | Dependency::Done => None,
        Dependency::Landed => Some("landed (tools/workflow-landing-adapters)"),
        Dependency::AcceptedOutput => Some("accepted_output (tools/workflow-accepted-output)"),
    }
}

/// Whether a recovery action moves the task: anything but a manual one with nowhere to go.
fn moves(action: &RecoveryAction) -> bool {
    !action.manual || action.target.is_some()
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

/// A task as activation reads it: its row's version, its status, where a workflow holds it, the
/// status a blocked task came from, and the roles held on it, each with whether its holder is
/// alive enough to hold it ([`crate::Health::holds`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub slug: String,
    pub version: i64,
    pub status: String,
    pub position: Option<TaskPosition>,
    pub blocked_from: Option<String>,
    pub roles: Vec<(String, String, bool)>,
    /// The last workflow revision of a task whose position a return retired, which its next
    /// transition continues from; 0 for a task no workflow ever held.
    pub retired: u64,
}

/// What an activation writes: each task moved, by its transition, from the version and workflow
/// revision it was read at, and each other task it read, fenced at its version so no write to it
/// lands between the read and the activation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub moved: Vec<Moved>,
    pub kept: Vec<(String, i64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub version: i64,
    pub expected: u64,
    pub transition: Transition,
}

/// Where activating `custom` as `generation` puts each of `tasks`, from `old`, the definition
/// active now (none: the board's existing writers, read as built-in v1). Refused whole, naming
/// the first task that cannot be placed.
pub fn plan(
    custom: &Custom,
    old: Option<&Definition>,
    tasks: &[Standing],
    generation: u64,
    by: &Principal,
    at: f64,
) -> Result<Plan, String> {
    let legacy = builtin();
    let new = custom.definition.definition();
    let mappings = super::mapped(&custom.mappings);
    let mut plan = Plan::default();
    for task in tasks {
        let slug = &task.slug;
        let (from_definition, from, resume_from, expected, revision) = match &task.position {
            Some(position) => {
                let old = old.ok_or_else(|| {
                    format!("{slug} has a workflow position, and no workflow is active")
                })?;
                (
                    old,
                    position.at(),
                    position.resume.clone(),
                    position.revision,
                    position.revision,
                )
            }
            None => {
                let (from, resume) = legacy_standing(task)?;
                // No position to fence on, and its revisions go on from the one a return retired.
                (&legacy, from, resume, 0, task.retired)
            }
        };
        let source = position_of(from_definition, &from).ok_or_else(|| {
            format!(
                "{slug} is at {}, which its workflow does not declare",
                named(&from)
            )
        })?;
        let to = placed(new, &mappings, slug, &from, "is at")?;
        let resume = resume_from
            .as_ref()
            .map(|resume| placed(new, &mappings, slug, resume, "resumes to"))
            .transpose()?;
        let destination = position_of(new, &to).expect("placed() names a declared pair");
        fabricates_nothing(slug, source, destination)?;
        let status = projection(destination.legacy);
        let changes =
            task.position.is_none() || from != to || resume != resume_from || status != task.status;
        if (changes || source.staffing != destination.staffing)
            && let Some((handle, role, _)) = task
                .roles
                .iter()
                .find(|(_, role, holds)| *holds && (role == "executor" || role == "reviewer"))
        {
            return Err(format!(
                "{slug} is held by {handle} as its {role}, and activation would move it or \
                 change its staffing; a live holder stands in for a live attempt, which drains \
                 first (release it, or let its work finish)"
            ));
        }
        if !changes {
            plan.kept.push((slug.clone(), task.version));
            continue;
        }
        let next = revision + 1;
        plan.moved.push(Moved {
            version: task.version,
            expected,
            transition: Transition {
                id: Transition::id_of(slug, next),
                slug: slug.clone(),
                revision: next,
                generation,
                digest: custom.revision.digest.clone(),
                edge: None,
                operation: None,
                activation: Some(generation),
                from: Some(from),
                to,
                resume,
                status,
                principal: by.clone(),
                effects: Vec::new(),
                key: None,
                recovery: None,
                at,
            },
        });
    }
    Ok(plan)
}

/// Where returning to the board's own writers as `generation` puts each of `tasks`, from `old`,
/// the definition active now: a task a workflow holds goes to the pair of built-in v1 it stands
/// at where v1 declares it, else where `mappings` send it, a blocked one's resume pair too, and
/// takes that pair's status; its transition retires its position. A concept, design or review
/// pair is recorded as v1's held one where the task's executor or reviewer holds it, as the board
/// reads it from then on. An open task with no position is the board's writers' already, and is
/// only fenced.
///
/// Refused whole, naming the first task: a pair nothing maps; a mapping that would give a task
/// an outcome, satisfied dependants or a completion it lacks; and a live executor or reviewer on
/// a task returning to a status where the board's writers keep no such holder
/// ([`keeps`]), who stands in for a live attempt and drains first.
pub fn returning(
    old: &Definition,
    mappings: &[Mapping],
    tasks: &[Standing],
    generation: u64,
    by: &Principal,
    at: f64,
) -> Result<Plan, String> {
    let legacy = builtin();
    let mappings = super::mapped(mappings);
    let mut plan = Plan::default();
    for task in tasks {
        let slug = &task.slug;
        let Some(position) = &task.position else {
            plan.kept.push((slug.clone(), task.version));
            continue;
        };
        let from = position.at();
        let source = position_of(old, &from).ok_or_else(|| {
            format!(
                "{slug} is at {}, which its workflow does not declare",
                named(&from)
            )
        })?;
        let mapped = placed(&legacy, &mappings, slug, &from, "is at")?;
        let destination = position_of(&legacy, &mapped).expect("placed() names a declared pair");
        fabricates_nothing(slug, source, destination)?;
        let status = projection(destination.legacy);
        let resume = match (&position.resume, status.as_str()) {
            (Some(resume), "blocked") => {
                Some(placed(&legacy, &mappings, slug, resume, "resumes to")?)
            }
            _ => None,
        };
        let resumes = resume.as_ref().map(|pair| pair.phase.as_str());
        if let Some((handle, role, _)) =
            (task.roles.iter()).find(|(_, role, holds)| *holds && !keeps(&status, resumes, role))
        {
            return Err(format!(
                "{slug} is held by {handle} as its {role}, and it would return to {status}, where \
                 the board's own writers keep no {role}; a live holder stands in for a live \
                 attempt, which drains first (release it, or let its work finish)"
            ));
        }
        // The pair as the board reads the task from now on: held where its holder holds it.
        let returned = Standing {
            status: status.clone(),
            position: None,
            blocked_from: None,
            ..task.clone()
        };
        let plain = super::legacy_pair(&status);
        let to = if plain.as_ref() == Some(&mapped) || mapped == held(&status) {
            legacy_standing(&returned)?.0
        } else {
            mapped
        };
        let next = position.revision + 1;
        plan.moved.push(Moved {
            version: task.version,
            expected: position.revision,
            transition: Transition {
                id: Transition::id_of(slug, next),
                slug: slug.clone(),
                revision: next,
                generation,
                digest: super::DEFAULT.into(),
                edge: None,
                operation: None,
                activation: Some(generation),
                from: Some(from),
                to,
                resume,
                status,
                principal: by.clone(),
                effects: Vec::new(),
                key: None,
                recovery: None,
                at,
            },
        });
    }
    Ok(plan)
}

/// Whether the board's own writers keep a holder of `role` on a task in `status`, a blocked one
/// by the status it resumes to (`resumes`; unknown, it is kept, as a hold keeps its roles): an
/// executor while its work is prepared, built or reviewed, a reviewer while it is reviewed. An
/// orchestrator and a reservation are the board's coordination everywhere.
pub fn keeps(status: &str, resumes: Option<&str>, role: &str) -> bool {
    let status = match status {
        "blocked" => match resumes {
            Some(resumes) => resumes,
            None => return true,
        },
        other => other,
    };
    match role {
        "executor" => matches!(status, "concept" | "design" | "in_progress" | "review"),
        "reviewer" => status == "review",
        _ => true,
    }
}

/// The active workflow as a write that reopens a task with no position reads it: its definition,
/// the mappings its activation recorded, and its generation and digest.
#[derive(Clone, Debug, PartialEq)]
pub struct Placing {
    pub definition: Definition,
    pub mappings: Vec<Mapping>,
    pub generation: u64,
    pub digest: String,
}

/// A task with no position that a board writer's move takes out of `left` (done or cancelled) to
/// `status`, and the roles held on it after the move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reopening<'a> {
    pub slug: &'a str,
    pub left: &'a str,
    pub status: &'a str,
    pub roles: &'a [(String, String, bool)],
    /// The last workflow revision of a position a return retired, 0 for a task never held.
    pub retired: u64,
}

impl Placing {
    /// Where `task` stands once its move leaves done or cancelled: built-in v1's pair for the
    /// status it moves to (held where its executor or reviewer holds it, as activation reads a
    /// kept task), itself where the active definition declares it, else where the activation's
    /// mappings send it; recorded as its first transition (the next, for a task a return retired),
    /// from v1's pair for the status it left,
    /// made by `by`. Refused, naming the pair, where nothing maps it, where the mapping would give
    /// it an outcome, a satisfaction or a completion it lacks, or where the pair projects another
    /// status than its move asks for.
    pub fn reopened(
        &self,
        task: &Reopening<'_>,
        by: &Principal,
        at: f64,
    ) -> Result<Transition, String> {
        let slug = task.slug;
        let standing = Standing {
            slug: slug.into(),
            version: 0,
            status: task.status.into(),
            position: None,
            blocked_from: None,
            roles: task.roles.to_vec(),
            retired: task.retired,
        };
        let (from, _) = legacy_standing(&standing)?;
        let source = super::builtin::position_at(&from).ok_or_else(|| {
            format!(
                "{slug} would reopen at {}, which the board's own workflow does not declare",
                named(&from)
            )
        })?;
        let mappings = super::mapped(&self.mappings);
        let to = placed(&self.definition, &mappings, slug, &from, "reopens at")?;
        let destination =
            position_of(&self.definition, &to).expect("placed() names a declared pair");
        fabricates_nothing(slug, &source, destination)?;
        let status = projection(destination.legacy);
        if status != task.status {
            return Err(format!(
                "workflow_mappings: {slug} reopens at {}, which stands for {status}, not {}, the \
                 status its move asks for",
                named(&to),
                task.status
            ));
        }
        let revision = task.retired + 1;
        Ok(Transition {
            id: Transition::id_of(slug, revision),
            slug: slug.into(),
            revision,
            generation: self.generation,
            digest: self.digest.clone(),
            edge: None,
            operation: None,
            activation: None,
            from: super::legacy_pair(task.left),
            to,
            resume: None,
            status,
            principal: by.clone(),
            effects: Vec::new(),
            key: None,
            recovery: None,
            at,
        })
    }
}

/// Built-in v1's pair for a task the board's existing writers kept, and the pair a blocked one
/// resumes to: its blocked source's, none when the board recorded none.
fn legacy_standing(task: &Standing) -> Result<(Pair, Option<Pair>), String> {
    let holds = |role: &str| task.roles.iter().any(|(_, held, _)| held == role);
    let status = task.status.as_str();
    let from = match status {
        "concept" | "design" if holds("executor") => held(status),
        "review" if holds("reviewer") => held(status),
        _ => super::legacy_pair(status).ok_or_else(|| {
            format!(
                "{} has the status {status}, which is no board status",
                task.slug
            )
        })?,
    };
    let resume = match status {
        "blocked" => task.blocked_from.as_deref().and_then(super::legacy_pair),
        _ => None,
    };
    Ok((from, resume))
}

fn held(status: &str) -> Pair {
    Pair {
        phase: status.into(),
        state: "held".into(),
    }
}

/// `pair` itself where `new` declares it, else where a mapping sends it; refused when neither.
fn placed(
    new: &Definition,
    mappings: &BTreeMap<&Pair, &Pair>,
    slug: &str,
    pair: &Pair,
    stands: &str,
) -> Result<Pair, String> {
    if position_of(new, pair).is_some() {
        return Ok(pair.clone());
    }
    mappings.get(pair).map(|to| (*to).clone()).ok_or_else(|| {
        format!(
            "workflow_mappings: {slug} {stands} {}, which the workflow no longer declares; map \
             it to a pair it declares",
            named(pair)
        )
    })
}

fn position_of<'a>(definition: &'a Definition, pair: &Pair) -> Option<&'a Position> {
    definition.positions.iter().find(|p| p.at == *pair)
}

/// Refused where `destination` records what `source` lacks: an outcome, the satisfaction of its
/// dependants or a completion. A mapping moves a task; it never approves, completes or lands it.
fn fabricates_nothing(slug: &str, source: &Position, destination: &Position) -> Result<(), String> {
    let refuse = |what: String| {
        Err(format!(
            "{slug} would stand at {} with {what} that {} does not record; a mapping never \
             fabricates one",
            named(&destination.at),
            named(&source.at)
        ))
    };
    if let Some(outcome) = destination.outcome
        && destination.outcome != source.outcome
    {
        return refuse(format!("the outcome {outcome:?}"));
    }
    if destination.satisfies_dependencies && !source.satisfies_dependencies {
        return refuse("its dependants satisfied".into());
    }
    if destination.completion != Completion::None && destination.completion != source.completion {
        return refuse(format!("the completion {:?}", destination.completion));
    }
    Ok(())
}

/// A position's board status, as the task's row holds it.
pub fn projection(legacy: LegacyStatus) -> String {
    serde_json::to_value(legacy)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("a legacy status serializes to its word")
}

/// The mappings a definition was activated with, as its activation records them.
pub(crate) fn recorded_mappings(mappings: &[Mapping]) -> serde_json::Value {
    serde_json::to_value(mappings).expect("mappings serialize")
}

#[cfg(test)]
#[path = "activate_tests.rs"]
pub(crate) mod tests;
