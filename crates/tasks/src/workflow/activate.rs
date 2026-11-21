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

/// Refused where `custom` commits an approval authority and names no project: an attestation
/// binds the project it is for, and the activation records the one its guards are held to. The
/// project file's reader refuses the same ([`super::authorities`]); this holds for a `Custom` a
/// caller builds itself.
pub fn bound(custom: &Custom) -> Result<(), String> {
    let named = custom
        .project
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());
    if custom.authorities.is_empty() || named {
        return Ok(());
    }
    Err(
        "workflow_authorities: an attestation names the project it is for, and the workflow \
         names none"
            .into(),
    )
}

/// The definition refused unless this host supports everything it needs, with no approval
/// authority committed: [`supported_with`] for none, so any guard is refused.
pub fn supported(definition: &Validated, host: &Host) -> Result<(), String> {
    supported_with(definition, host, &[])
}

/// The definition refused unless this host supports everything it needs: each capability it
/// declares granted to someone, each adapter registered or committed as an approval authority
/// (`attesters`), every automatic edge one the board can take on an attempt's failure
/// ([`automatic`]), and nothing this release cannot run yet. A guard needs its adapter's committed
/// authority of a contract this release reads, a graph that reaches a guarded pair by no other
/// route ([`super::guard::coverage`]), and an edge a caller takes with an attestation: never one
/// the board takes itself or an adapter prepares or finalizes. `landed` dependencies and parent
/// integration wait on their own follow-ups. Each refusal says which, so the project knows what
/// it asked for.
pub fn supported_with(
    definition: &Validated,
    host: &Host,
    attesters: &[super::Attester],
) -> Result<(), String> {
    let d = definition.definition();
    let granted = host.grants.supported();
    let adapters: Vec<&str> = (host.adapters.iter().map(String::as_str))
        .chain(attesters.iter().map(|a| a.adapter.as_str()))
        .collect();
    for guard in &d.guards {
        let Some(attester) = attesters.iter().find(|a| a.adapter == guard.adapter) else {
            return Err(format!(
                "unsupported guard: {} (its adapter {} needs an approval authority committed in \
                 [[workflow_authorities]])",
                guard.id, guard.adapter
            ));
        };
        let contract = symphony_api::workflow::ATTESTATION_CONTRACT;
        if attester.contract != contract {
            return Err(format!(
                "unsupported guard: {} (its authority {} speaks contract {}, and this release \
                 reads {contract})",
                guard.id, attester.adapter, attester.contract
            ));
        }
    }
    super::prerequisites(
        definition,
        &granted.into_iter().collect::<Vec<_>>(),
        &adapters,
    )?;
    super::guard::coverage(d)?;
    for position in &d.positions {
        let at = named(&position.at);
        if let Some(why) = unsupported_dependency(position.dependencies) {
            return Err(format!("unsupported dependency at {at}: {why}"));
        }
    }
    for edge in &d.edges {
        let id = &edge.id;
        let why = match edge.retry {
            _ if edge.automatic => automatic(d, edge, host),
            Some(_) => {
                Some("a retry bound on an edge a caller takes: the board counts its own takes")
            }
            None => None,
        };
        if let Some(why) = why {
            return Err(format!("unsupported edge {id}: {why}"));
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
        // Its adapter takes a prepare or finalize edge, and no attestation comes with it
        // (`Store::prepare_effect`): its guard could never be met, a finalize's after the effect
        // already happened.
        let adapted = (edge.effects.iter())
            .any(|e| matches!(e, Effect::Prepare { .. } | Effect::Finalize { .. }));
        if adapted && !edge.guards.is_empty() {
            return Err(format!(
                "unsupported edge {id}: a guard on an edge its adapter prepares or finalizes, \
                 which takes no attestation yet"
            ));
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
/// source pair (docs/WORKFLOW.md, "Attempts"), none where it can: another automatic edge leaves
/// that pair, so the board could not choose; it asks inputs, which the board has none of to give;
/// it gives a role, which only an agent session holds, or prepares or finalizes an effect, which
/// only its adapter takes; it is guarded, and no attestation comes with the board's take; its retry bound is exhausted to a pair on a cycle through it, which the
/// task passes every lap, so the count of its takes, which restarts there, never runs out
/// ([`super::recover::observed`], [`laps`]); or none of its authorities is a capability `host`
/// grants the board's recovery service, which takes it. Each reason is fixed text, the edge named
/// by the refusal that carries it.
fn automatic(d: &Definition, edge: &Edge, host: &Host) -> Option<&'static str> {
    let service = super::board_service("recovery");
    let authorized = edge.authority.iter().any(|authority| match authority {
        Authority::ProjectCapability { capability }
        | Authority::SystemCapability { capability } => host.grants.holds(&service, capability),
        Authority::TaskRole { .. } => false,
    });
    if (d.edges.iter()).any(|e| e.automatic && e.from == edge.from && e.id != edge.id) {
        Some("another automatic edge leaves its pair, and the board could not choose which to take")
    } else if !edge.inputs.is_empty() {
        Some("an automatic edge asks for inputs, which the board has none of to give")
    } else if (edge.effects.iter()).any(|e| matches!(e, Effect::AcquireRole { .. })) {
        Some("an automatic edge gives a role, which only an agent session takes")
    } else if super::effect::external(&edge.effects).is_some() {
        Some("an automatic edge prepares or finalizes an effect, which only its adapter takes")
    } else if !edge.guards.is_empty() {
        Some("an automatic edge is guarded, and the board brings no attestation to meet its guard")
    } else if (edge.retry.as_ref()).is_some_and(|r| laps(d, edge, &r.exhausted)) {
        Some("an automatic edge's retry is exhausted to a pair on a cycle through it")
    } else if !authorized {
        Some(
            "no authority of this automatic edge is a capability the host grants the board's recovery service",
        )
    } else {
        None
    }
}

/// Whether `at` lies on a cycle through `edge`: reached from where it goes, and reaching where it
/// leaves, by the moves `d` declares, each edge whoever takes it and each recovery action's
/// target. The one move left out is the `attempt_failure` action of the edge's own source, which
/// its retry bound replaces, sending the task to its exhausted pair instead.
fn laps(d: &Definition, edge: &Edge, at: &Pair) -> bool {
    let reaches = |start: &Pair, goal: &Pair| {
        let mut seen = vec![start];
        let mut next = 0;
        while let Some(&here) = seen.get(next) {
            if here == goal {
                return true;
            }
            next += 1;
            let actions = (d.positions.iter())
                .filter(|p| p.at == *here)
                .filter_map(|p| p.recovery.as_ref())
                .flat_map(|r| {
                    let failure = (*here != edge.from).then_some(&r.attempt_failure);
                    [&r.release, &r.lost_holder, &r.cancellation_acknowledged]
                        .into_iter()
                        .chain(failure)
                });
            let moves = (d.edges.iter())
                .filter(|e| e.from == *here)
                .map(|e| &e.to)
                .chain(actions.filter_map(|a| a.target.as_ref()));
            for to in moves {
                if !seen.contains(&to) {
                    seen.push(to);
                }
            }
        }
        false
    };
    reaches(&edge.to, at) && reaches(at, &edge.from)
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

/// A task as activation reads it: its row's version, its status, where a workflow holds it, the
/// status a blocked task came from, the roles held on it, each with whether its holder is alive
/// enough to hold it ([`crate::Health::holds`]), the accepted output its last transition left, and
/// its execution attempts whose runtime may still be alive ([`crate::Store::live_attempts`]) and
/// that no cancel was asked of: a cancel invalidates an attempt, after which no result of it
/// counts or moves its task.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub slug: String,
    pub version: i64,
    pub status: String,
    pub position: Option<TaskPosition>,
    pub blocked_from: Option<String>,
    pub roles: Vec<(String, String, bool)>,
    pub accepted: Option<String>,
    pub attempts: Vec<String>,
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
        let held = task.position.is_some();
        past_no_guard(slug, old, new, held, Some(&from), &to, "stand at")?;
        if let Some(resume) = &resume {
            past_no_guard(
                slug,
                old,
                new,
                held,
                resume_from.as_ref(),
                resume,
                "resume to",
            )?;
        }
        let status = projection(destination.legacy);
        let moves =
            task.position.is_none() || from != to || resume != resume_from || status != task.status;
        // A pair that stops satisfying dependants supersedes the output accepted there, recorded
        // as a transition that moves nothing; a mapping never accepts one.
        let accepted =
            super::admit::accepted_after(Some(source), destination, task.accepted.as_deref(), None);
        let changes = moves || accepted != task.accepted;
        drained(
            task,
            changes,
            moves || source.staffing != destination.staffing,
        )?;
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
                accepted,
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
/// an outcome, satisfied dependants or a completion it lacks; an execution attempt no cancel was
/// asked of on a task the workflow holds, whose ending is pinned to a position the return retires
/// (let it end, or cancel it); and a live executor or reviewer on a task returning to a status
/// where the board's writers keep no such holder ([`keeps`]), who stands in for a live attempt and
/// drains first.
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
        if let Some(live) = task.attempts.first() {
            return Err(format!(
                "{slug} runs execution attempt {live}, and the return would retire the position \
                 its ending is pinned to; an attempt drains first (let it end, or cancel it)"
            ));
        }
        let mapped = placed(&legacy, &mappings, slug, &from, "is at")?;
        let destination = position_of(&legacy, &mapped).expect("placed() names a declared pair");
        fabricates_nothing(slug, source, destination)?;
        let status = projection(destination.legacy);
        // As an activation records it: a pair that stops satisfying dependants supersedes it.
        let accepted =
            super::admit::accepted_after(Some(source), destination, task.accepted.as_deref(), None);
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
                accepted,
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

/// Refused while `task` runs work that has not drained: an execution attempt no cancel was asked
/// of (its `attempts`), where activation `changes` its position (records it a transition, which leaves the
/// attempt's ending pinned to a revision the task no longer stands at) or `restaffs` it, as an
/// attempt is never rebound to another position (let it end, or cancel it); or a live executor or
/// reviewer where it moves or restaffs it, whose session may be no attempt: one launched before
/// local terminals were attempts, or not by the board's launcher.
fn drained(task: &Standing, changes: bool, restaffs: bool) -> Result<(), String> {
    let slug = &task.slug;
    if let Some(live) = task.attempts.first().filter(|_| changes || restaffs) {
        return Err(format!(
            "{slug} runs execution attempt {live}, and activation would change its position or \
             its staffing; an attempt drains first (let it end, or cancel it), and is never \
             rebound to another position"
        ));
    }
    if let Some((handle, role, _)) = (task.roles.iter())
        .find(|(_, role, holds)| restaffs && *holds && (role == "executor" || role == "reviewer"))
    {
        return Err(format!(
            "{slug} is held by {handle} as its {role}, and activation would move it or change \
             its staffing; a live holder's session drains first (release it, or let its work \
             finish), as it may be no attempt the board sees"
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
            accepted: None,
            attempts: Vec::new(),
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
        past_no_guard(slug, None, &self.definition, false, None, &to, "reopen at")?;
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

/// Refused where a task would come to stand at `to` (or resume to it), a pair only edges guarded by
/// the same guards enter ([`super::guard::guarded_into`]), without having stood there already,
/// `held` by `old` at `from`, under those guards: a mapping, a placement or a reopening never takes
/// a task past a guard, as no attestation covers it.
fn past_no_guard(
    slug: &str,
    old: Option<&Definition>,
    new: &Definition,
    held: bool,
    from: Option<&Pair>,
    to: &Pair,
    stands: &str,
) -> Result<(), String> {
    let Some(guards) = super::guard::guarded_into(new, to) else {
        return Ok(());
    };
    let stood = held
        && from == Some(to)
        && old.and_then(|old| super::guard::guarded_into(old, to)) == Some(guards);
    if stood {
        return Ok(());
    }
    Err(format!(
        "{slug} would {stands} {}, which only edges guarded by {} enter; activation never takes a \
         task past a guard: map it to a pair before it",
        named(to),
        guards.join(", ")
    ))
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
