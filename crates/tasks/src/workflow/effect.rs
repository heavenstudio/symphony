//! The boundary of an external effect on a task an active custom workflow holds (docs/workflow/WORKFLOW.md,
//! "External effects"): a landing, or any other effect a host-verified adapter makes outside the
//! board. An effect is two edges, a prepare and a finalize, with the adapter's work between them:
//!
//! 1. **Prepared.** The prepare edge is admitted as any operation is, and its write records the
//!    [`Intent`] beside its transition: what the effect is bound to ([`Bindings`]), under the
//!    workflow generation and digest that admitted it. The intent is durable before any I/O, and
//!    its identity is its prepare's transition (`SLUG@REVISION`): the task's workflow revision the
//!    prepare made is the reservation.
//! 2. **Dispatched.** Before it sends anything, the adapter reads its bindings again from the
//!    system they describe ([`Adapter::current`]), and the board moves the intent on only while
//!    they are the ones bound, the workflow is the one that admitted it and the task has not moved
//!    since ([`stale`]). A reservation stale or cancelled by then is withdrawn, and nothing is
//!    sent.
//! 3. **Finalized.** Only on what the adapter read back of the effect itself
//!    ([`Adapter::outcome`]), never on a caller's word: the finalize edge is taken, and the
//!    receipt read back recorded. A request in flight cannot be recalled, so an effect that
//!    happened is always recorded: where the policy or the evidence it was bound to changed in
//!    the gap, or the task or its workflow moved, the outcome is recorded with a recovery
//!    requirement ([`requirement`]), never as fresh policy satisfied.
//!
//! Nothing here writes: `crate::store`'s `admission/effects.rs` does, fenced on what it read.

use super::Active;
use super::TaskPosition;
use serde::{Deserialize, Serialize};
use symphony_api::workflow::{Definition, Edge, Effect, Pair};
use symphony_plugin::execution::Principal;

/// The adapter of the Symphony's default landing (`just land`, `crate::land`).
pub const DEFAULT_LAND: &str = "default_land";
/// The adapter of a project's own landing: the seam a `land` recipe of the project's links.
pub const PROJECT_LAND: &str = "project_land";

/// What an effect is bound to when it is prepared, each read by its adapter from the system it
/// describes, never from a request: the project and task, the repository and, where the effect is
/// a pull request's, its identity, the exact head that was reviewed, the revision of the landing
/// policy in force, the revision of the evidence snapshot it was judged on, and where it goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub project: String,
    pub task: String,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<String>,
    pub head: String,
    pub policy: String,
    pub evidence: String,
    pub destination: String,
}

impl Bindings {
    /// The first binding `now` holds another value of, with both: none when they are the same.
    pub fn moved(&self, now: &Bindings) -> Option<String> {
        let pull = |b: &Bindings| b.pull.clone().unwrap_or_default();
        [
            ("project", &self.project, &now.project),
            ("task", &self.task, &now.task),
            ("repository", &self.repository, &now.repository),
            ("pull request", &pull(self), &pull(now)),
            ("head", &self.head, &now.head),
            ("landing policy", &self.policy, &now.policy),
            ("evidence", &self.evidence, &now.evidence),
            ("destination", &self.destination, &now.destination),
        ]
        .into_iter()
        .find(|(_, bound, now)| bound != now)
        .map(|(name, bound, now)| format!("its {name} was {bound} when prepared and is {now}"))
    }
}

/// Where an intent stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Recorded with its prepare's transition; nothing sent.
    Prepared,
    /// Its bindings read again and found standing: the adapter may send its request.
    Dispatched,
    /// The effect happened, and what the adapter read back of it is recorded.
    Finalized,
    /// Stale or cancelled before dispatch: nothing was sent, and nothing will be under it.
    Withdrawn,
}

impl Stage {
    /// Its word on the record.
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Prepared => "prepared",
            Stage::Dispatched => "dispatched",
            Stage::Finalized => "finalized",
            Stage::Withdrawn => "withdrawn",
        }
    }
}

/// One external effect's durable intent (`workflow:['intent', SLUG, REVISION]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    /// Its key: the transition of its prepare, `SLUG@REVISION`.
    pub id: String,
    pub slug: String,
    /// The task's workflow revision its prepare made: the reservation.
    pub revision: u64,
    pub adapter: String,
    /// The idempotency key of the prepare's request.
    pub key: String,
    pub bindings: Bindings,
    /// The activation generation and the digest of the workflow that admitted the prepare.
    pub generation: u64,
    pub digest: String,
    pub principal: Principal,
    pub stage: Stage,
    /// What the adapter read back of the effect: a merge commit, a deployment's receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
    /// The transition that took the finalize edge; none while the outcome is recorded and the
    /// edge is not taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalized: Option<String>,
    /// What a person has to recover: the effect happened under a policy, evidence, workflow or
    /// position that no longer stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    /// Why it was withdrawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<String>,
    pub at: f64,
}

/// What an adapter read back of an effect that happened: its receipt, and the revisions of the
/// policy and the evidence that stood when it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub receipt: String,
    pub policy: String,
    pub evidence: String,
}

/// A host-verified adapter: the one program that makes an external effect, registered by name in
/// the host's own policy ([`super::Host::adapters`]). The board asks it, and never a request, for
/// what the effect is bound to now and for what happened.
pub trait Adapter {
    /// The name a definition's prepare and finalize effects call it by.
    fn name(&self) -> &str;

    /// `intent`'s bindings as they stand now, read from the system they describe.
    ///
    /// # Errors
    ///
    /// Why they cannot be read; the effect is then not dispatched.
    fn current(&self, intent: &Intent) -> Result<Bindings, String>;

    /// What happened of `intent`'s effect, read back from the system it was made in by the
    /// intent's own identity: none when it did not happen.
    ///
    /// # Errors
    ///
    /// Why it cannot be read; nothing is then finalized.
    fn outcome(&self, intent: &Intent) -> Result<Option<Outcome>, String>;
}

/// The edge leaving `at` whose effect is `adapter`'s prepare.
pub fn prepare_edge<'a>(definition: &'a Definition, at: &Pair, adapter: &str) -> Option<&'a Edge> {
    edge(
        definition,
        at,
        |e| matches!(e, Effect::Prepare { adapter: a } if a == adapter),
    )
}

/// The edge leaving `at` whose effect is `adapter`'s finalize.
pub fn finalize_edge<'a>(definition: &'a Definition, at: &Pair, adapter: &str) -> Option<&'a Edge> {
    edge(
        definition,
        at,
        |e| matches!(e, Effect::Finalize { adapter: a } if a == adapter),
    )
}

fn edge<'a>(
    definition: &'a Definition,
    at: &Pair,
    is: impl Fn(&Effect) -> bool,
) -> Option<&'a Edge> {
    (definition.edges.iter()).find(|e| e.from == *at && e.effects.iter().any(&is))
}

/// The adapter an edge's effects prepare or finalize for, if they do.
pub fn external(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|e| match e {
        Effect::Prepare { adapter } | Effect::Finalize { adapter } => Some(adapter.as_str()),
        _ => None,
    })
}

/// What takes the edges of `adapter` on `slug`, as a refusal and a list of actions name it: the
/// landing command for a landing's adapter, the Symphony's default or the project's own, which
/// one recipe runs; none for an adapter with no command of the board's.
pub fn taken_by(adapter: &str, slug: &str) -> Option<String> {
    [DEFAULT_LAND, PROJECT_LAND]
        .contains(&adapter)
        .then(|| format!("`just land {slug}`"))
}

/// Why `intent`'s reservation no longer stands, none while it does: the workflow active is not the
/// one that admitted its prepare, the task has moved since (or no workflow holds it), or a binding
/// reads another value `now`.
pub fn stale(
    intent: &Intent,
    active: Option<&Active>,
    position: Option<&TaskPosition>,
    now: Option<&Bindings>,
) -> Option<String> {
    let slug = &intent.slug;
    match active {
        None => return Some("no custom workflow is active on the board now".to_string()),
        Some(active)
            if active.generation != intent.generation || active.digest != intent.digest =>
        {
            return Some(format!(
                "the workflow activated again since it was prepared (generation {} when prepared, \
                 {} now)",
                intent.generation, active.generation
            ));
        }
        Some(_) => {}
    }
    match position {
        None => return Some(format!("no workflow holds {slug} now")),
        Some(position) if position.revision != intent.revision => {
            return Some(format!(
                "{slug} moved on to {}/{} (workflow revision {}, prepared at {})",
                position.phase, position.state, position.revision, intent.revision
            ));
        }
        Some(_) => {}
    }
    now.and_then(|now| intent.bindings.moved(now))
}

/// What a person has to recover of an effect that happened with `outcome`, none when everything
/// it was bound to stood: the policy or the evidence was another when it happened, or the
/// reservation was stale by then ([`stale`]).
pub fn requirement(
    intent: &Intent,
    outcome: &Outcome,
    active: Option<&Active>,
    position: Option<&TaskPosition>,
) -> Option<String> {
    let bound = &intent.bindings;
    let mut why = Vec::new();
    if outcome.policy != bound.policy {
        why.push(format!(
            "the landing policy was {} when prepared and {} when the effect happened",
            bound.policy, outcome.policy
        ));
    }
    if outcome.evidence != bound.evidence {
        why.push(format!(
            "the evidence was {} when prepared and {} when the effect happened",
            bound.evidence, outcome.evidence
        ));
    }
    why.extend(stale(intent, active, position, None));
    (!why.is_empty()).then(|| why.join("; "))
}

#[cfg(test)]
#[path = "effect_tests.rs"]
pub(crate) mod tests;
