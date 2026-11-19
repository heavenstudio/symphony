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

use super::{Custom, Grants, Mapping, TaskPosition, Transition, Validated, builtin};
use std::collections::BTreeMap;
use symphony_api::workflow::{
    Authority, Completion, Definition, Dependency, Edge, Effect, LegacyStatus, Pair, Position,
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
    /// owner holds `owner`, the board's recovery service holds `recovery`, and the two landing
    /// adapters are registered: the default landing's and a project's own ([`super::effect`]). A definition declaring any other capability is unsupported on this host.
    pub fn local() -> Host {
        Host {
            grants: Grants::none()
                .grant("owner", super::local_owner())
                .grant("recovery", super::board_service("recovery")),
            adapters: vec![
                super::effect::DEFAULT_LAND.to_string(),
                super::effect::PROJECT_LAND.to_string(),
            ],
        }
    }
}

/// The definition refused unless this host supports everything it needs: each capability it
/// declares granted to someone, each adapter registered, every automatic edge one the board can
/// take on an attempt's failure ([`automatic`]), and nothing this release cannot run yet. Guards
/// wait on revision-bound guards; `landed` dependencies and parent integration on their own
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
    for position in &d.positions {
        let at = named(&position.at);
        if let Some(why) = unsupported_dependency(position.dependencies) {
            return Err(format!("unsupported dependency at {at}: {why}"));
        }
    }
    for edge in &d.edges {
        let id = &edge.id;
        if edge.automatic {
            automatic(d, edge, host).map_err(|why| format!("unsupported edge {id}: {why}"))?;
        } else if edge.retry.is_some() {
            return Err(format!(
                "unsupported edge {id}: a retry bound on an edge a caller takes; the board counts \
                 the takes of an automatic edge alone"
            ));
        }
        if let Some(why) = unsupported_dependency(edge.dependencies) {
            return Err(format!("unsupported edge {id}: a dependency on {why}"));
        }
        for effect in &edge.effects {
            match effect {
                Effect::ParentIntegration => {
                    return Err(format!("unsupported edge {id}: parent integration"));
                }
                // The host's own adapters alone prepare and finalize: a name another list
                // registers (a guard's authority) makes no effect.
                Effect::Prepare { adapter } | Effect::Finalize { adapter }
                    if !host.adapters.contains(adapter) =>
                {
                    return Err(format!(
                        "unsupported edge {id}: this host registers no adapter {adapter} to \
                         prepare or finalize its effect"
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Why this release cannot check `dependency`, none for one it checks (`none`, `done`,
/// `accepted_output`): a landing, which the board knows only from its thread and its git.
fn unsupported_dependency(dependency: Dependency) -> Option<&'static str> {
    match dependency {
        Dependency::None | Dependency::Done | Dependency::AcceptedOutput => None,
        Dependency::Landed => Some("landed"),
    }
}

/// Why the board could not take `edge`, an automatic edge of `d`, on an attempt's failure at its
/// source pair (docs/WORKFLOW.md, "Attempts"): another automatic edge leaves that pair, so the
/// board could not choose; it asks inputs, which the board has none of to give; it gives a role,
/// which only an agent session holds; or none of its authorities is a capability `host` grants
/// the board's recovery service, which takes it.
fn automatic(d: &Definition, edge: &Edge, host: &Host) -> Result<(), String> {
    if let Some(other) =
        (d.edges.iter()).find(|e| e.automatic && e.from == edge.from && e.id != edge.id)
    {
        return Err(format!(
            "an automatic edge, as is {} from {}: the board takes one on an attempt's failure \
             there, and could not choose",
            other.id,
            named(&edge.from)
        ));
    }
    if !edge.inputs.is_empty() {
        return Err(
            "an automatic edge asks for inputs, which the board has none of to give".into(),
        );
    }
    if (edge.effects.iter()).any(|e| matches!(e, Effect::AcquireRole { .. })) {
        return Err(
            "an automatic edge gives a role, which only an agent session takes; the board \
             takes an automatic edge"
                .into(),
        );
    }
    let service = super::board_service("recovery");
    let authorized = edge.authority.iter().any(|authority| match authority {
        Authority::ProjectCapability { capability }
        | Authority::SystemCapability { capability } => host.grants.holds(&service, capability),
        Authority::TaskRole { .. } => false,
    });
    if !authorized {
        return Err(
            "an automatic edge no authority of which this host grants the board's recovery \
             service, which takes it (grant it `recovery`, and name that capability)"
                .into(),
        );
    }
    Ok(())
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

/// A task as activation reads it: its row's version, its status, where a workflow holds it, the
/// status a blocked task came from, the roles held on it, each with whether its holder is alive
/// enough to hold it ([`crate::Health::holds`]), the accepted output its last transition left, and
/// its execution attempts whose runtime may still be alive ([`crate::Store::live_attempts`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub slug: String,
    pub version: i64,
    pub status: String,
    pub position: Option<TaskPosition>,
    pub blocked_from: Option<String>,
    pub roles: Vec<(String, String, bool)>,
    pub accepted: Option<String>,
    pub attempts: Vec<Live>,
}

/// A live execution attempt of a task, as activation reads it: its id, and whether a cancel was
/// asked of it, which invalidates it: from then on no result of it counts or moves its task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub attempt: String,
    pub cancelled: bool,
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
        let (from_definition, from, resume_from, revision) = match &task.position {
            Some(position) => {
                let old = old.ok_or_else(|| {
                    format!("{slug} has a workflow position, and no workflow is active")
                })?;
                (
                    old,
                    position.at(),
                    position.resume.clone(),
                    position.revision,
                )
            }
            None => {
                let (from, resume) = legacy_standing(task)?;
                (&legacy, from, resume, 0)
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
        let moves =
            task.position.is_none() || from != to || resume != resume_from || status != task.status;
        // A pair that stops satisfying dependants supersedes the output accepted there, recorded
        // as a transition that moves nothing; a mapping never accepts one.
        let accepted =
            super::admit::accepted_after(Some(source), destination, task.accepted.as_deref(), None);
        let changes = moves || accepted != task.accepted;
        if moves || source.staffing != destination.staffing {
            drained(task)?;
        }
        if !changes {
            plan.kept.push((slug.clone(), task.version));
            continue;
        }
        let next = revision + 1;
        plan.moved.push(Moved {
            version: task.version,
            expected: revision,
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
                accepted,
                at,
            },
        });
    }
    Ok(plan)
}

/// Refused while `task`, which activation would move or restaff, runs work that has not drained:
/// an execution attempt no cancel was asked of, which activation never rebinds to another
/// position (let it end, or cancel it), or a live executor or reviewer, whose local session is no
/// attempt the board sees until local launches are recorded as attempts
/// (tools/local-provider-attempts).
fn drained(task: &Standing) -> Result<(), String> {
    let slug = &task.slug;
    if let Some(live) = task.attempts.iter().find(|a| !a.cancelled) {
        return Err(format!(
            "{slug} runs execution attempt {}, and activation would move it or change its \
             staffing; an attempt drains first (let it end, or cancel it), and is never \
             rebound to another position",
            live.attempt
        ));
    }
    if let Some((handle, role, _)) = (task.roles.iter())
        .find(|(_, role, holds)| *holds && (role == "executor" || role == "reviewer"))
    {
        return Err(format!(
            "{slug} is held by {handle} as its {role}, and activation would move it or change \
             its staffing; a live holder's session drains first (release it, or let its work \
             finish), as it is no attempt the board sees yet"
        ));
    }
    Ok(())
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
}

impl Placing {
    /// Where `task` stands once its move leaves done or cancelled: built-in v1's pair for the
    /// status it moves to (held where its executor or reviewer holds it, as activation reads a
    /// kept task), itself where the active definition declares it, else where the activation's
    /// mappings send it; recorded as its first transition, from v1's pair for the status it left,
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
            accepted: None,
            attempts: Vec::new(),
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
        Ok(Transition {
            id: Transition::id_of(slug, 1),
            slug: slug.into(),
            revision: 1,
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
            // No edge accepted the output of a task the board's own writers kept.
            accepted: None,
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
