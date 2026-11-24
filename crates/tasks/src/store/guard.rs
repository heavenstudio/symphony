//! The board's side of a project's approval authorities (docs/workflow/WORKFLOW.md, "Guards"): the state it
//! accepted from each ([`Store::observe_authority`]), what a guarded edge's admission recorded
//! ([`Store::workflow_admission`]) and the check a dispatch makes again right before an effect it
//! cannot take back ([`Store::revalidate_dispatch`]).
//!
//! The decisions stay in the project's ledger. The board keeps, for each authority the active
//! activation commits, the last state it accepted, verified by the key the activation committed:
//! an observation moves it forward only from that state, and a write that admits through it is
//! fenced on it (`store/admission.rs`), so an observation and an admission never pass each other.

use super::postgres::guard as pg;
use super::{Error, Failure, Store, statement::Statement};
use crate::store::admission::Admission;
use crate::workflow::Active;
use crate::workflow::guard::{self, Accepted, Attester, Binding, Checked, Guarding, Observed};
use serde::Deserialize;
use serde_json::{Value, json};
use symphony_api::workflow::{
    Attestation, GuardAuthority, GuardEntry, GuardMet, GuardNeed, GuardStanding, GuardedEdge,
    Signed, TaskGuards,
};

/// What an observation did to the state the board accepted from its authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observing {
    /// It moved the accepted state to this one.
    Accepted(Accepted),
    /// It reported the accepted state again, which changes nothing.
    Unchanged(Accepted),
    /// It reported another decision at the accepted generation: nothing the authority attests is
    /// admitted until it reports a state following this one.
    Conflicted(Accepted),
}

impl Observing {
    /// What the api and `workflow observe --json` answer: the outcome's name and the state
    /// accepted after it.
    pub fn answer(&self) -> serde_json::Value {
        let (observed, authority) = match self {
            Observing::Accepted(a) => ("accepted", a),
            Observing::Unchanged(a) => ("unchanged", a),
            Observing::Conflicted(a) => ("conflicted", a),
        };
        serde_json::json!({"observed": observed, "authority": authority})
    }
}

/// The approval authorities the active activation commits and the state accepted from each.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Authorities {
    #[serde(default)]
    pub attesters: Vec<Attester>,
    #[serde(default)]
    pub accepted: Vec<Accepted>,
}

#[derive(Deserialize)]
struct AuthorityRead {
    generation: Option<u64>,
    #[serde(default)]
    attesters: Vec<Attester>,
    accepted: Option<Accepted>,
}

#[derive(Deserialize)]
struct DispatchRead {
    active: Option<Active>,
    found: bool,
    position: Option<crate::workflow::TaskPosition>,
    transition: Option<crate::workflow::Transition>,
    admission: Option<Admission>,
    project: Option<String>,
    #[serde(default)]
    attesters: Vec<Attester>,
    #[serde(default)]
    accepted: Vec<Accepted>,
}

impl Store {
    /// The approval authorities the active activation commits, with the state the board accepted
    /// from each: none on a board with no workflow active or one that commits no authority.
    pub fn workflow_authorities(&self) -> Result<Authorities, Error> {
        self.retrying(|| self.read(Statement::from(pg::authorities()), json!({})))
    }

    /// `signed`, an observation from one of the active activation's authorities, verified by the
    /// key it commits and decided against the state the board accepted from it
    /// ([`guard::observe`]), then written only if that state is still the one it was decided on.
    /// Refused when no workflow is active, no committed authority is the one it names, it does
    /// not verify, or it does not follow the accepted state: the refusal names that state, which
    /// the authority reports again from.
    pub fn observe_authority(&self, signed: &Signed) -> Result<Observing, Error> {
        let adapter = guard::claimed_adapter(signed)
            .map_err(|why| Error::Refused(format!("workflow: an observation: {why}")))?;
        self.retrying(|| {
            let read: AuthorityRead = self.read(
                Statement::from(pg::authority()),
                json!({"adapter": adapter}),
            )?;
            let Some(generation) = read.generation else {
                return Err(stop(
                    "no workflow is active, so the board observes no authority".into(),
                ));
            };
            let attester = (read.attesters.iter())
                .find(|a| a.adapter == adapter)
                .ok_or_else(|| {
                    stop(format!(
                        "{adapter} is no approval authority the active workflow commits"
                    ))
                })?;
            let observation = guard::verified_observation(attester, signed)
                .map_err(|why| stop(format!("an observation from {adapter}: {why}")))?;
            let (observing, next) = match guard::observe(read.accepted.as_ref(), &observation)
                .map_err(|why| stop(format!("an observation from {adapter}: {why}")))?
            {
                Observed::Same => {
                    let same = read
                        .accepted
                        .clone()
                        .expect("the same state is one accepted");
                    return Ok(Observing::Unchanged(same));
                }
                Observed::Accepted(next) => (Observing::Accepted(next.clone()), next),
                Observed::Conflict(next) => (Observing::Conflicted(next.clone()), next),
            };
            self.transact::<Value>(
                Statement::from(pg::observe()),
                json!({
                    "generation": generation,
                    "adapter": adapter,
                    "expected": read.accepted,
                    "body": next,
                    "now": self.now(),
                }),
            )?;
            Ok(observing)
        })
    }

    /// What the admission of `slug`'s transition `revision` recorded, or none for a transition
    /// no guard admitted.
    pub fn workflow_admission(
        &self,
        slug: &str,
        revision: u64,
    ) -> Result<Option<Admission>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                Statement::from(pg::admission_record()),
                json!({"slug": slug, "revision": revision}),
            )
        })?;
        if body.is_null() {
            return Ok(None);
        }
        crate::objects::read(&body)
            .map(Some)
            .map_err(|e| Error::Db(format!("an admission not in the board's shape: {e}")))
    }

    /// The guards of `slug` as a task's page shows them ([`TaskGuards`]): the authorities the
    /// active activation commits with the state accepted from each, the guards the workflow
    /// declares, the guarded edges leaving the task's pair, and the admission that last brought
    /// it into that pair by a guarded edge ([`guard::entry`]), each guard it met with whether it
    /// still stands ([`guard::standing`]) and what its attestation says while that verifies. All
    /// empty on a board with no workflow active or a workflow that declares no guard. No page
    /// and no host is known here: `page` is none and `asked_here` false, which the Symphony
    /// fills. It reads the board's records and decides nothing.
    pub fn workflow_guards(&self, slug: &str) -> Result<TaskGuards, Error> {
        let Some(active) = self.workflow_active()? else {
            return Ok(TaskGuards::default());
        };
        let recorded = self.workflow_definition(&active.digest)?.ok_or_else(|| {
            Error::Db(format!(
                "the active workflow's definition {} is not recorded",
                active.digest
            ))
        })?;
        let definition = &recorded.definition;
        if definition.guards.is_empty() {
            return Ok(TaskGuards::default());
        }
        let now = self.workflow_authorities()?;
        let authorities = (now.attesters.iter())
            .map(|attester| {
                let accepted = (now.accepted.iter()).find(|a| a.adapter == attester.adapter);
                GuardAuthority {
                    adapter: attester.adapter.clone(),
                    issuer: attester.issuer.clone(),
                    epoch: accepted.map(|a| a.epoch.clone()),
                    generation: accepted.map(|a| a.generation),
                    conflicted: accepted.is_some_and(|a| a.conflicted),
                    admitted: accepted.map_or(0, |a| a.admitted),
                    asked_here: false,
                }
            })
            .collect();
        let guards = (definition.guards.iter())
            .map(|guard| GuardNeed {
                guard: guard.id.clone(),
                adapter: guard.adapter.clone(),
                page: None,
            })
            .collect();
        let transitions = self.workflow_transitions(slug)?;
        let mut read = TaskGuards {
            authorities,
            guards,
            edges: Vec::new(),
            entry: None,
        };
        let Some(last) = transitions.last() else {
            return Ok(read);
        };
        let pair = &last.to;
        read.edges = (definition.edges.iter())
            .filter(|edge| edge.from == *pair && !edge.guards.is_empty())
            .map(|edge| GuardedEdge {
                edge: edge.id.clone(),
                guards: edge.guards.clone(),
            })
            .collect();
        if guard::guarded_into(definition, pair).is_none() {
            return Ok(read);
        }
        let Some(entered) = guard::entry(&transitions, last.revision + 1, pair) else {
            return Ok(read);
        };
        let Some(admission) = self.workflow_admission(slug, entered)? else {
            return Ok(read);
        };
        let label = (definition.edges.iter())
            .find(|edge| edge.id == admission.edge)
            .map_or_else(|| admission.edge.clone(), |edge| edge.label.clone());
        read.entry = Some(GuardEntry {
            label,
            edge: admission.edge.clone(),
            revision: entered,
            principal: guard::attestee(&admission.principal),
            at: admission.at,
            guards: (admission.guards.iter())
                .map(|checked| met(checked, &now))
                .collect(),
        });
        Ok(read)
    }

    /// Why the guards that brought `slug` into the pair its transition `revision` (an external
    /// effect's prepare) left no longer stand for that effect's dispatch, none where they stand or
    /// where the active workflow guards no edge into that pair. The transition that brought it
    /// there is the last that entered the pair, found by going back past those that stayed in it
    /// ([`guard::entry`]): its admission must have met each guard the pair's edges carry, and each
    /// must stand ([`guard::unstanding`]). A transition not found, or found with no admission or
    /// short of a guard, is a reason too, so nothing is dispatched on a guard no record says was
    /// met.
    pub(in crate::store) fn prepared_from_unguarded(
        &self,
        slug: &str,
        revision: u64,
    ) -> Result<Option<String>, Error> {
        // With none active, the dispatch's own check withdraws the intent.
        let Some(active) = self.workflow_active()? else {
            return Ok(None);
        };
        let recorded = self.workflow_definition(&active.digest)?.ok_or_else(|| {
            Error::Db(format!(
                "the active workflow's definition {} is not recorded",
                active.digest
            ))
        })?;
        let transitions = self.workflow_transitions(slug)?;
        let left = (transitions.iter())
            .find(|t| t.revision == revision)
            .and_then(|t| t.from.clone());
        let Some(left) = left else {
            return Ok(Some(format!(
                "{slug} records no move at revision {revision} for its effect to leave from"
            )));
        };
        let Some(guards) = guard::guarded_into(&recorded.definition, &left) else {
            return Ok(None);
        };
        let pair = format!("{}/{}", left.phase, left.state);
        let Some(entered) = guard::entry(&transitions, revision, &left) else {
            return Ok(Some(format!(
                "{slug}'s record holds no transition that brought it into {pair} under {}",
                guards.join(", ")
            )));
        };
        let Some(admission) = self.workflow_admission(slug, entered)? else {
            return Ok(Some(format!(
                "{slug} entered {pair} at revision {entered} with no admission of {}",
                guards.join(", ")
            )));
        };
        let unmet = (guards.iter()).find(|g| !admission.guards.iter().any(|c| c.guard == **g));
        if let Some(unmet) = unmet {
            return Ok(Some(format!(
                "{slug} entered {pair} at revision {entered} without meeting {unmet}, which its \
                 edges carry now"
            )));
        }
        let now = self.workflow_authorities()?;
        Ok(guard::unstanding(
            &admission.guards,
            &now.attesters,
            &now.accepted,
        ))
    }

    /// The check a dispatch makes right before an effect of `slug`'s transition `revision` that it
    /// cannot take back (a merge, a push, a message out), answered with each guard met again:
    /// none for a transition no guard admitted. The task still stands where that transition left
    /// it, under the same activation, and each guard its admission met is met again now, by the
    /// attestations it recorded or by `fresh` ones covering the same operation (the task's
    /// revision before it, its pair, its principal, its input revisions): signed by the
    /// committed key, in its time, at the state the board accepts from its authority now, so a
    /// decision revoked since, an expired attestation or an authority in conflict refuses.
    /// Refused, with the code of what failed first; it reads and writes nothing else, and an
    /// effect that waits after it is the caller's to bound.
    pub fn revalidate_dispatch(
        &self,
        slug: &str,
        revision: u64,
        fresh: &[Signed],
    ) -> Result<Vec<Checked>, Error> {
        let read: DispatchRead = self.retrying(|| {
            self.read(
                Statement::from(pg::dispatch()),
                json!({"slug": slug, "revision": revision}),
            )
        })?;
        use symphony_api::workflow::RefusalCode as Code;
        let refused = |code: Code, why: String| {
            Error::Refused(crate::workflow::admit::Refusal::new(code, why).to_string())
        };
        if !read.found {
            return Err(refused(Code::UnknownId, format!("there is no task {slug}")));
        }
        let Some(transition) = read.transition else {
            return Err(refused(
                Code::UnknownId,
                format!("{slug} has no transition {revision}"),
            ));
        };
        let Some(active) = read.active else {
            return Err(refused(
                Code::StaleWorkflow,
                "no workflow is active, so nothing it admitted is dispatched".into(),
            ));
        };
        if active.generation != transition.generation || active.digest != transition.digest {
            return Err(refused(
                Code::StaleActivation,
                format!(
                    "{slug}'s transition {revision} was admitted under generation {}, and \
                     generation {} is active",
                    transition.generation, active.generation
                ),
            ));
        }
        if read.position.as_ref().map(|p| p.revision) != Some(revision) {
            return Err(refused(
                Code::StaleTask,
                format!("{slug} has moved since its transition {revision}"),
            ));
        }
        let Some(admission) = read.admission else {
            return Ok(Vec::new());
        };
        let recorded = self.workflow_definition(&active.digest)?.ok_or_else(|| {
            Error::Db(format!(
                "the active workflow's definition {} is not recorded",
                active.digest
            ))
        })?;
        let definition = &recorded.definition;
        let edge = (definition.edges.iter())
            .find(|e| e.id == admission.edge)
            .ok_or_else(|| {
                Error::Db(format!(
                    "{slug}'s admission names edge {}, which its definition does not declare",
                    admission.edge
                ))
            })?;
        let from = transition.from.as_ref().ok_or_else(|| {
            Error::Db(format!(
                "{slug}'s admitted transition {revision} has no from"
            ))
        })?;
        let project = read.project.unwrap_or_default();
        let binding = Binding {
            project: &project,
            task: slug,
            edge: &edge.id,
            workflow: &active.digest,
            activation_generation: active.generation,
            task_revision: revision - 1,
            at: from,
            input_revisions: &admission.input_revisions,
            principal: &admission.principal,
        };
        let recorded: Vec<Signed> = admission.guards.iter().map(|g| g.signed.clone()).collect();
        let guarding = Guarding {
            project: &project,
            attesters: &read.attesters,
            accepted: &read.accepted,
            given: if fresh.is_empty() { &recorded } else { fresh },
            // The machine's time, as for an admission ([`Store::apply_attested`]).
            now: super::now(),
        };
        guard::check(definition, edge, &binding, &guarding)
            .map_err(|refusal| Error::Refused(refusal.to_string()))
    }
}

/// One guard an admission met, as a page shows it: its standing against the authorities as they
/// are `now`, and what its attestation says while it still verifies by the committed key.
fn met(checked: &Checked, now: &Authorities) -> GuardMet {
    let (standing, _) = guard::standing(checked, &now.attesters, &now.accepted);
    let verified = !matches!(
        standing,
        GuardStanding::Uncommitted | GuardStanding::Unverified
    );
    let record = (verified.then(|| crate::objects::parse::<Attestation>(&checked.signed.payload)))
        .flatten()
        .map(|attestation| attestation.record());
    GuardMet {
        guard: checked.guard.clone(),
        adapter: checked.adapter.clone(),
        issuer: checked.issuer.clone(),
        epoch: checked.epoch.clone(),
        generation: checked.generation,
        standing,
        record,
    }
}

fn stop(why: String) -> Failure {
    Failure::Stop(Error::Refused(format!("workflow: {why}")))
}
