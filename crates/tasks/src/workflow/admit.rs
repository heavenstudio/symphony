//! Admission of one typed operation on a task an active custom workflow holds (docs/workflow/WORKFLOW.md,
//! "Admission"): what the store read decides whether the principal the host bound may take the
//! edge from the pair the caller saw, and what the transition writes. Nothing here writes; the
//! store fences the write on everything read here (`crate::store::workflow`), so a decision made
//! on a snapshot that moved is never written.
//!
//! The principal is the host's, never the request's: the request fences intention (the snapshot
//! the caller saw, its key, its inputs), and a principal holds a capability only through the
//! host's grants ([`super::Grants`]). A task role is held on this task alone, a reviewer is fresh
//! (`Held::unfresh`), a guard is met only by an attestation the board verifies now
//! ([`super::guard`]), and every check this release cannot make refuses: an adapter, an input
//! revision that names no accepted output and that no guard binds, a dependency on a landing.
//!
//! An accepted output (docs/workflow/WORKFLOW.md, "Accepted outputs") is the id of the transition whose
//! edge moved a task to a position that satisfies its dependants from one that does not; it stands
//! while the task stays at such positions ([`accepted_after`]), and an edge depending on
//! `accepted_output` is taken only on the output standing for each task it depends on, which its
//! request names ([`standing`]).

use super::guard::{self, Binding, Checked, Guarding};
use super::{Host, Recovered, TaskPosition, Transition};
use crate::Held;
use std::fmt;
use symphony_api::workflow::{
    Authority, Definition, Dependency, Edge, Effect, NextAction, NextActions, OperationRequest,
    Pair, Position, PositionSnapshot, RefusalCode, Revision, Role, Slot,
};
use symphony_plugin::execution::{Principal, PrincipalKind};

/// An operation refused: its stable code and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: RefusalCode,
    pub message: String,
}

impl Refusal {
    pub fn new(code: RefusalCode, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
        }
    }

    /// Its code as the wire says it.
    pub fn word(&self) -> String {
        word(&self.code)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.word(), self.message)
    }
}

/// The active workflow admission decides under: its definition, revision and generation.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    pub definition: &'a Definition,
    pub revision: &'a Revision,
    pub generation: u64,
}

/// A task admission decides on, read in one snapshot: who filed it, where it stands, the role rows
/// on it, each task it depends on, who ever executed or orchestrated it, and whether the principal,
/// when an agent, is a session the board knows and that has not left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub slug: String,
    pub created_by: String,
    pub position: Option<TaskPosition>,
    /// Each role row on the task: its holder's handle and the role.
    pub roles: Vec<(String, String)>,
    pub dependencies: Vec<Prerequisite>,
    pub held: Held,
    pub present: bool,
    /// The task's own accepted output, as its last transition left it: what a move between two
    /// positions that satisfy dependants carries.
    pub accepted: Option<String>,
}

/// A task another depends on: its status and position, none when it is not on the board, and the
/// accepted output its last transition left, none for one no workflow holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Prerequisite {
    pub slug: String,
    pub found: Option<(String, Option<TaskPosition>)>,
    pub accepted: Option<String>,
}

/// What an admitted operation writes: its transition, the role rows it adds and drops, whether
/// it takes a task slot under the cap, and each guard it met, by the attestation and the
/// authority's accepted state the write fences on.
#[derive(Clone, Debug, PartialEq)]
pub struct Admitted {
    pub transition: Transition,
    pub add: Vec<(String, String)>,
    pub drop: Vec<(String, String)>,
    pub acquires: bool,
    pub guards: Vec<Checked>,
}

/// The snapshot a caller fences its next request on.
pub fn snapshot(context: Context, position: &TaskPosition) -> PositionSnapshot {
    PositionSnapshot {
        workflow: context.revision.clone(),
        activation_generation: context.generation,
        task_revision: position.revision,
        at: position.at(),
    }
}

/// `request` admitted for `principal` on what `facts` read, at `at`, or refused with the first
/// thing that stops it: a task no workflow holds, a stale workflow, generation or task, an edge
/// unknown or not leaving the task's pair, then [`blockers`], its guards decided on `guarding`.
/// Capacity is the store's to decide, in the write ([`Admitted::acquires`]).
pub fn admit(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    request: &OperationRequest,
    guarding: &Guarding<'_>,
    at: f64,
) -> Result<Admitted, Refusal> {
    let taker = Taker::Caller(Some(guarding));
    admitting(context, facts, principal, host, request, taker, at)
}

/// `request`, an automatic edge the board takes on the attempt ending `answered` records,
/// admitted for `principal`, the board's service, as [`admit`] admits a caller's: by the same
/// checks, but that the edge must be automatic, which no caller takes, and it brings no
/// attestation, so a guard refuses it. Its transition records what it answered.
pub fn admit_observed(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    request: &OperationRequest,
    answered: &Recovered,
    at: f64,
) -> Result<Admitted, Refusal> {
    let taker = Taker::Board(answered);
    admitting(context, facts, principal, host, request, taker, at)
}

fn admitting(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    request: &OperationRequest,
    taker: Taker<'_>,
    at: f64,
) -> Result<Admitted, Refusal> {
    let position = held(facts)?;
    let expected = &request.expected;
    if expected.workflow.digest != context.revision.digest {
        return Err(Refusal::new(
            RefusalCode::StaleWorkflow,
            format!(
                "the active workflow is {}, not {}",
                context.revision.digest, expected.workflow.digest
            ),
        ));
    }
    if expected.activation_generation != context.generation {
        return Err(Refusal::new(
            RefusalCode::StaleActivation,
            format!(
                "the active generation is {}, not {}",
                context.generation, expected.activation_generation
            ),
        ));
    }
    if expected.task_revision != position.revision || expected.at != position.at() {
        return Err(Refusal::new(
            RefusalCode::StaleTask,
            format!(
                "{} is at {} (workflow revision {}), not {} (revision {})",
                facts.slug,
                named(&position.at()),
                position.revision,
                named(&expected.at),
                expected.task_revision
            ),
        ));
    }
    let edge = (context.definition.edges.iter())
        .find(|e| e.id == request.edge)
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::UnknownId,
                format!(
                    "workflow {} declares no edge {}",
                    context.revision.digest, request.edge
                ),
            )
        })?;
    if edge.from != position.at() {
        return Err(Refusal::new(
            RefusalCode::UnknownId,
            format!(
                "edge {} leaves {}, and {} is at {}",
                edge.id,
                named(&edge.from),
                facts.slug,
                named(&position.at())
            ),
        ));
    }
    if let Some(refusal) = blocking(context, facts, principal, host, edge, Some(request), taker)
        .into_iter()
        .next()
    {
        return Err(refusal);
    }
    // `blocking` refused a guarded edge no attestations came with.
    let guards = match taker {
        Taker::Caller(Some(guarding)) => met(context, facts, principal, edge, request, guarding)?,
        _ => Vec::new(),
    };
    let handle = agent(principal);
    let mut add = Vec::new();
    let mut drop = Vec::new();
    for effect in &edge.effects {
        match effect {
            Effect::AcquireRole { role } => {
                let handle = handle.expect("blockers() refuses an acquire_role with no agent");
                let role = word(role);
                if !holds(facts, handle, &role) {
                    add.push((handle.to_string(), role));
                }
                if holds(facts, handle, "reserved") {
                    drop.push((handle.to_string(), "reserved".into()));
                }
            }
            Effect::ReleaseRole { role } => {
                let role = word(role);
                drop.extend(
                    (facts.roles.iter())
                        .filter(|(_, held)| *held == role)
                        .cloned(),
                );
            }
            _ => {}
        }
    }
    let resume = edge
        .effects
        .iter()
        .any(|e| matches!(e, Effect::Block { save_resume: true }))
        .then(|| position.at());
    let destination = position_of(context.definition, &edge.to)
        .expect("a validated definition declares every edge's destination");
    let revision = position.revision + 1;
    let id = Transition::id_of(&facts.slug, revision);
    let accepted = accepted_after(
        position_of(context.definition, &position.at()),
        destination,
        facts.accepted.as_deref(),
        Some(&id),
    );
    Ok(Admitted {
        transition: Transition {
            id,
            slug: facts.slug.clone(),
            revision,
            generation: context.generation,
            digest: context.revision.digest.clone(),
            edge: Some(edge.id.clone()),
            operation: Some(edge.operation),
            activation: None,
            from: Some(position.at()),
            to: edge.to.clone(),
            resume,
            status: super::activate::projection(destination.legacy),
            principal: principal.clone(),
            effects: edge.effects.clone(),
            key: Some(request.idempotency_key.clone()),
            recovery: match taker {
                Taker::Board(answered) => Some(answered.clone()),
                Taker::Caller(_) => None,
            },
            accepted,
            at,
        },
        add,
        drop,
        acquires: edge.effects.iter().any(|e| {
            *e == Effect::Slot {
                policy: Slot::Acquire,
            }
        }),
        guards,
    })
}

/// `edge`'s guards met by `guarding`'s attestations, each covering `request` taken by `principal`
/// from where `facts` stands under the active workflow, in the project the activation recorded
/// (`guarding`), never the one the request names ([`guard::check`]).
fn met(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    edge: &Edge,
    request: &OperationRequest,
    guarding: &Guarding<'_>,
) -> Result<Vec<Checked>, Refusal> {
    let position = held(facts)?;
    let from = position.at();
    let binding = Binding {
        project: guarding.project,
        task: &facts.slug,
        edge: &edge.id,
        workflow: &context.revision.digest,
        activation_generation: context.generation,
        task_revision: position.revision,
        at: &from,
        input_revisions: &request.input_revisions,
        principal,
    };
    guard::check(context.definition, edge, &binding, guarding)
}

/// Why a guard of `request`'s edge refuses it for `principal` with `guarding`'s attestations, as a
/// page shows it ([`guard::Explained`]): none where the edge is not one leaving the task's pair,
/// its guards are met, or what refuses names no single guard. It decides nothing: admission's own
/// refusal ([`admit`]) is the answer, and this says which guard it was.
pub fn guard_refusal(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    request: &OperationRequest,
    guarding: &Guarding<'_>,
) -> Option<symphony_api::workflow::GuardRefusal> {
    let position = held(facts).ok()?;
    let from = position.at();
    let edge =
        (context.definition.edges.iter()).find(|e| e.id == request.edge && e.from == from)?;
    let binding = Binding {
        project: guarding.project,
        task: &facts.slug,
        edge: &edge.id,
        workflow: &context.revision.digest,
        activation_generation: context.generation,
        task_revision: position.revision,
        at: &from,
        input_revisions: &request.input_revisions,
        principal,
    };
    guard::explained(context.definition, edge, &binding, guarding)
        .err()?
        .guard
        .map(|guard| *guard)
}

/// The edges leaving the task's pair, each with what stops `principal` taking it now: a preview,
/// never a permit, which shows an action the principal cannot take with the reason, and names the
/// authority it needs without saying who holds it. An edge that prepares or finalizes an external
/// effect is never a caller's to take, whoever asks: it is listed as `explicit_action_required`,
/// its adapter's ([`super::effect::taken_by`]).
pub fn next_actions(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
) -> Result<NextActions, Refusal> {
    let position = held(facts)?;
    let actions = (context.definition.edges.iter())
        .filter(|e| e.from == position.at() && !e.automatic)
        .map(|edge| {
            let mut unavailable: Vec<RefusalCode> =
                blockers(context, facts, principal, host, edge, None, None)
                    .into_iter()
                    .map(|r| r.code)
                    .collect();
            if super::effect::external(&edge.effects).is_some() {
                unavailable.push(RefusalCode::ExplicitActionRequired);
            }
            unavailable.dedup();
            NextAction {
                edge: edge.id.clone(),
                operation: edge.operation,
                destination: edge.to.clone(),
                label: edge.label.clone(),
                responsible: edge.authority.clone(),
                required_inputs: edge.inputs.clone(),
                unavailable,
            }
        })
        .collect();
    Ok(NextActions {
        snapshot: snapshot(context, position),
        actions,
    })
}

/// Everything that stops `principal` taking `edge` from where the task stands, in the order a
/// refusal names them: what this release cannot run (an adapter), a guard not met by the
/// attestations `guarding` gives (any guard, for a preview, which gives none), a human wait, the
/// authority, the inputs `request` gives (none for a preview), the dependencies, and a resume
/// to a pair other than the one the block saved.
pub fn blockers(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    edge: &Edge,
    request: Option<&OperationRequest>,
    guarding: Option<&Guarding<'_>>,
) -> Vec<Refusal> {
    let taker = Taker::Caller(guarding);
    blocking(context, facts, principal, host, edge, request, taker)
}

/// Who takes an edge: a caller, by its request, with the attestations its guards are decided on
/// (none for a preview); or the board, on the attempt ending it answers, with none.
#[derive(Clone, Copy)]
enum Taker<'t> {
    Caller(Option<&'t Guarding<'t>>),
    Board(&'t Recovered),
}

/// [`blockers`], for an edge `taker` takes: a caller never takes an automatic edge, and the board
/// takes nothing else.
fn blocking(
    context: Context,
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    edge: &Edge,
    request: Option<&OperationRequest>,
    taker: Taker<'_>,
) -> Vec<Refusal> {
    let mut found = Vec::new();
    let id = &edge.id;
    match taker {
        Taker::Caller(_) if edge.automatic => found.push(Refusal::new(
            RefusalCode::UnsupportedWriter,
            format!("edge {id} is automatic: the board takes it, never a caller"),
        )),
        Taker::Board(_) if !edge.automatic => found.push(Refusal::new(
            RefusalCode::UnsupportedWriter,
            format!("edge {id} is a caller's to take: the board takes only an automatic edge"),
        )),
        _ => {}
    }
    for effect in &edge.effects {
        let needs = match effect {
            // An adapter the host registered takes its own edges (`super::effect`).
            Effect::Prepare { adapter } | Effect::Finalize { adapter }
                if host.adapters.contains(adapter) =>
            {
                continue;
            }
            Effect::Prepare { adapter } | Effect::Finalize { adapter } => {
                format!("the adapter {adapter}, which this host does not register")
            }
            Effect::ParentIntegration => {
                "parent integration, which no host-verified adapter runs here yet".to_string()
            }
            _ => continue,
        };
        found.push(Refusal::new(
            RefusalCode::UnsupportedAdapter,
            format!("edge {id} needs {needs}"),
        ));
    }
    if !edge.guards.is_empty() {
        let checked = match (request, taker) {
            (Some(request), Taker::Caller(Some(guarding))) => {
                met(context, facts, principal, edge, request, guarding).err()
            }
            _ => guard::needs_attestation(edge),
        };
        found.extend(checked);
    }
    if edge.human_wait && principal.kind != PrincipalKind::Human {
        found.push(Refusal::new(
            RefusalCode::HumanWait,
            format!("edge {id} waits on a person's deliberate action"),
        ));
    }
    if let Err(refusal) = authorized(facts, principal, host, edge) {
        found.push(refusal);
    }
    if agent(principal).is_none()
        && let Some(role) = edge.effects.iter().find_map(|e| match e {
            Effect::AcquireRole { role } => Some(role),
            _ => None,
        })
    {
        found.push(Refusal::new(
            RefusalCode::ExplicitActionRequired,
            format!(
                "edge {id} gives its taker the {} role, which only an agent session holds",
                word(role)
            ),
        ));
    }
    if let Some(request) = request {
        found.extend(inputs(edge, facts, request));
    }
    if let Err(refusal) = dependencies(context, facts, edge, request) {
        found.push(refusal);
    }
    let position = facts.position.as_ref();
    let saved = position.and_then(|p| p.resume.as_ref());
    if edge.effects.contains(&Effect::ClearBlock)
        && let Some(resume) = saved
        && *resume != edge.to
    {
        found.push(Refusal::new(
            RefusalCode::ExplicitActionRequired,
            format!(
                "edge {id} resumes {} at {}, and its block saved {}",
                facts.slug,
                named(&edge.to),
                named(resume)
            ),
        ));
    }
    // A resume returns a task to a guarded pair only where its block saved that very pair: one
    // that came to the blocked pair by any other edge, or an activation, saved none, and would
    // stand there past the guard (`guard::coverage` counts no resume as a route in).
    if edge.effects.contains(&Effect::ClearBlock)
        && saved.is_none()
        && let Some(guards) = guard::guarded_into(context.definition, &edge.to)
    {
        found.push(Refusal::new(
            RefusalCode::ExplicitActionRequired,
            format!(
                "edge {id} resumes {} at {}, which only edges guarded by {} enter, and no block \
                 saved it there",
                facts.slug,
                named(&edge.to),
                guards.join(", ")
            ),
        ));
    }
    found
}

/// Whether `principal` meets one of `edge`'s authority alternatives on this task: a task role it
/// holds here (or takes, by the edge's own acquire_role, when no one else holds it and no other
/// session's reservation stands), as a fresh reviewer where the role is the reviewer's, or a
/// capability the host granted it. An agent is a session the board knows and that has not left.
fn authorized(
    facts: &Facts,
    principal: &Principal,
    host: &Host,
    edge: &Edge,
) -> Result<(), Refusal> {
    let handle = agent(principal);
    let acquires = |role: &Role| {
        (edge.effects.iter()).any(|e| matches!(e, Effect::AcquireRole { role: r } if r == role))
    };
    let mut why = None;
    for authority in &edge.authority {
        let met = match authority {
            Authority::ProjectCapability { capability }
            | Authority::SystemCapability { capability } => {
                host.grants.holds(principal, capability)
            }
            Authority::TaskRole { role } => match handle {
                None => false,
                Some(_) if !facts.present => {
                    why = Some(format!("{} is no session on the board", who(principal)));
                    false
                }
                Some(handle) => {
                    let word = word(role);
                    let relation = if acquires(role) {
                        takes(facts, handle, &word)
                    } else if holds(facts, handle, &word) {
                        Ok(())
                    } else {
                        Err(format!("{handle} is not the {word} of {}", facts.slug))
                    };
                    match relation.and_then(|()| fresh(facts, handle, *role)) {
                        Ok(()) => true,
                        Err(reason) => {
                            why = Some(reason);
                            false
                        }
                    }
                }
            },
        };
        if met {
            return Ok(());
        }
    }
    let needs: Vec<String> = edge.authority.iter().map(described).collect();
    Err(Refusal::new(
        RefusalCode::Unauthorized,
        match why {
            Some(why) => format!("{why}; edge {} needs {}", edge.id, needs.join(" or ")),
            None => format!(
                "{} may not take edge {} on {}: it needs {}",
                who(principal),
                edge.id,
                facts.slug,
                needs.join(" or ")
            ),
        },
    ))
}

/// Whether `handle` may take `role` on the task by an edge that acquires it: no other session
/// holds it, and no other session's reservation stands.
fn takes(facts: &Facts, handle: &str, role: &str) -> Result<(), String> {
    if let Some((other, _)) = (facts.roles.iter()).find(|(h, r)| r == role && h != handle) {
        return Err(format!("{other} holds the {role} of {}", facts.slug));
    }
    if let Some((other, _)) = (facts.roles.iter()).find(|(h, r)| r == "reserved" && h != handle) {
        return Err(format!("{} is reserved for {other}", facts.slug));
    }
    Ok(())
}

/// Refused where the role is the reviewer's and `handle` executed, orchestrates or orchestrated,
/// or filed the task (RULES.md `workflow/reviewers-not-the-owner`), whatever the graph says.
fn fresh(facts: &Facts, handle: &str, role: Role) -> Result<(), String> {
    if role != Role::Reviewer {
        return Ok(());
    }
    let orchestrates = holds(facts, handle, "orchestrator");
    match facts.held.unfresh(handle, &facts.created_by, orchestrates) {
        None => Ok(()),
        Some(why) => Err(format!(
            "{handle} {} {}: its reviewer is a fresh session, one that did not write, execute or \
             request it",
            match why {
                crate::Unfresh::Executed => "executed",
                crate::Unfresh::Orchestrated => "orchestrated",
                crate::Unfresh::Filed => "filed",
            },
            facts.slug
        )),
    }
}

/// The request's inputs against the edge's: each one it declares given, none it does not, and no
/// input revision but the accepted output of a task this one depends on, named for an edge that
/// depends on accepted outputs ([`dependencies`] checks each is the one standing), or any on a
/// guarded edge, whose attestations bind the input revisions it is taken on ([`guard::check`]):
/// the board records no revision of anything else, so none is current.
fn inputs(edge: &Edge, facts: &Facts, request: &OperationRequest) -> Vec<Refusal> {
    let mut found = Vec::new();
    for given in request.inputs.keys() {
        if !edge.inputs.contains(given) {
            found.push(Refusal::new(
                RefusalCode::UnknownId,
                format!("edge {} declares no input {given}", edge.id),
            ));
        }
    }
    for needed in &edge.inputs {
        if !request.inputs.contains_key(needed) {
            found.push(Refusal::new(
                RefusalCode::ExplicitActionRequired,
                format!("edge {} needs the input {needed}", edge.id),
            ));
        }
    }
    let accepts = |name: &str| {
        edge.dependencies == Dependency::AcceptedOutput
            && (facts.dependencies.iter()).any(|p| p.slug == name)
    };
    if edge.guards.is_empty()
        && let Some(name) = (request.input_revisions.keys()).find(|name| !accepts(name))
    {
        found.push(Refusal::new(
            RefusalCode::StaleInput,
            format!(
                "the board records no revision of {name}: an input revision names the accepted \
                 output of a task {} depends on, for an edge depending on accepted_output",
                facts.slug
            ),
        ));
    }
    found
}

/// The edge's dependency predicate on the tasks this one depends on: `done` is met by a task the
/// workflow holds at a position that satisfies its dependants, or one the board's writers keep in
/// done; `accepted_output` by an accepted output standing for each ([`standing`]), which
/// `request` names under the task's slug (none for a preview, which names nothing): one it does
/// not name is to be named, and one other than the output standing (superseded, or never
/// accepted) is stale. A task not on the board meets nothing.
fn dependencies(
    context: Context,
    facts: &Facts,
    edge: &Edge,
    request: Option<&OperationRequest>,
) -> Result<(), Refusal> {
    match edge.dependencies {
        Dependency::None => Ok(()),
        Dependency::AcceptedOutput => {
            for prerequisite in &facts.dependencies {
                let slug = &prerequisite.slug;
                let Some(output) = standing(context, prerequisite) else {
                    return Err(Refusal::new(
                        RefusalCode::Dependency,
                        format!(
                            "{} depends on the accepted output of {slug}, which has none \
                             standing: it is {}",
                            facts.slug,
                            stands(prerequisite)
                        ),
                    ));
                };
                let Some(request) = request else { continue };
                match request.input_revisions.get(slug) {
                    None => {
                        return Err(Refusal::new(
                            RefusalCode::ExplicitActionRequired,
                            format!(
                                "edge {} takes the accepted output of {slug}: name the one it \
                                 takes, {slug} = {output}",
                                edge.id
                            ),
                        ));
                    }
                    Some(named) if named != output => {
                        return Err(Refusal::new(
                            RefusalCode::StaleInput,
                            format!(
                                "{} takes {named} of {slug}, whose accepted output standing is \
                                 {output}",
                                facts.slug
                            ),
                        ));
                    }
                    Some(_) => {}
                }
            }
            Ok(())
        }
        Dependency::Landed => Err(Refusal::new(
            RefusalCode::UnsupportedWriter,
            format!(
                "edge {} depends on {}, which this release does not check",
                edge.id,
                word(&edge.dependencies)
            ),
        )),
        Dependency::Done => {
            for prerequisite in &facts.dependencies {
                let satisfied = match &prerequisite.found {
                    None => false,
                    Some((_, Some(position))) => position_of(context.definition, &position.at())
                        .is_some_and(|p| p.satisfies_dependencies),
                    Some((status, None)) => status == "done",
                };
                if !satisfied {
                    return Err(Refusal::new(
                        RefusalCode::Dependency,
                        format!(
                            "{} depends on {}, which is {}",
                            facts.slug,
                            prerequisite.slug,
                            stands(prerequisite)
                        ),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Where a prerequisite stands, as a refusal names it.
fn stands(prerequisite: &Prerequisite) -> String {
    match &prerequisite.found {
        None => "not on the board".to_string(),
        Some((_, Some(p))) => format!("at {}", named(&p.at())),
        Some((status, None)) => status.clone(),
    }
}

/// The accepted output standing for `prerequisite`: the one its last transition left, while the
/// workflow holds it at a position that satisfies its dependants. None for a task the board's own
/// writers keep, done or not: no edge accepted its output, and none is made up for it.
pub fn standing<'a>(context: Context, prerequisite: &'a Prerequisite) -> Option<&'a str> {
    match &prerequisite.found {
        Some((_, Some(position)))
            if position_of(context.definition, &position.at())
                .is_some_and(|p| p.satisfies_dependencies) =>
        {
            prerequisite.accepted.as_deref()
        }
        _ => None,
    }
}

/// The accepted output standing once a task moves from `source` (none where the definition no
/// longer declares it) to `destination`: none where `destination` does not satisfy dependants,
/// which supersedes the one that stood; `standing` carried where both do; else `minted`, the id
/// of the transition whose edge accepts the output (none for a move no edge makes: an activation's
/// mapping or a recovery never accepts one).
pub fn accepted_after(
    source: Option<&Position>,
    destination: &Position,
    standing: Option<&str>,
    minted: Option<&str>,
) -> Option<String> {
    if !destination.satisfies_dependencies {
        return None;
    }
    (source.is_some_and(|s| s.satisfies_dependencies))
        .then_some(standing)
        .flatten()
        .or(minted)
        .map(str::to_string)
}

/// The task's position, refused for a task no workflow holds.
fn held(facts: &Facts) -> Result<&TaskPosition, Refusal> {
    facts.position.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::UnknownId,
            format!(
                "the workflow holds no position of {}: the board's own writers keep it",
                facts.slug
            ),
        )
    })
}

/// The handle of an agent session the local host bound.
fn agent(principal: &Principal) -> Option<&str> {
    (principal.kind == PrincipalKind::Agent && principal.issuer == "local")
        .then_some(principal.subject.as_str())
}

fn holds(facts: &Facts, handle: &str, role: &str) -> bool {
    (facts.roles.iter()).any(|(h, r)| h == handle && r == role)
}

/// How a refusal names a principal: an agent by its handle, anyone else by issuer and subject.
fn who(principal: &Principal) -> String {
    match agent(principal) {
        Some(handle) => handle.to_string(),
        None => format!("{}/{}", principal.issuer, principal.subject),
    }
}

fn described(authority: &Authority) -> String {
    match authority {
        Authority::TaskRole { role } => format!("the task's {}", word(role)),
        Authority::ProjectCapability { capability } => {
            format!("the project capability {capability}")
        }
        Authority::SystemCapability { capability } => format!("the system capability {capability}"),
    }
}

fn position_of<'a>(definition: &'a Definition, pair: &Pair) -> Option<&'a Position> {
    definition.positions.iter().find(|p| p.at == *pair)
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

/// A vocabulary value as its wire word.
pub(crate) fn word<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("a vocabulary value serializes to its word")
}

#[cfg(test)]
#[path = "admit_tests.rs"]
pub(crate) mod tests;
