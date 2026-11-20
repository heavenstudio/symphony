//! The board's side of a project's approval authorities (docs/WORKFLOW.md, "Guards"): the state it
//! accepted from each ([`Store::observe_authority`]), what a guarded edge's admission recorded
//! ([`Store::workflow_admission`]) and the check a dispatch makes again right before an effect it
//! cannot take back ([`Store::revalidate_dispatch`]).
//!
//! The decisions stay in the project's ledger. The board keeps, for each authority the active
//! activation commits, the last state it accepted, verified by the key the activation committed:
//! an observation moves it forward only from that state, and a write that admits through it is
//! fenced on it (`store/admission.rs`), so an observation and an admission never pass each other.

use super::postgres::guard as pg;
use super::surreal::guard as sql;
use super::{Error, Failure, Store, statement::both};
use crate::store::admission::Admission;
use crate::workflow::Active;
use crate::workflow::guard::{self, Accepted, Attester, Binding, Checked, Guarding, Observed};
use serde::Deserialize;
use serde_json::{Value, json};
use symphony_api::workflow::Signed;

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
        self.retrying(|| self.read(both(sql::AUTHORITIES, pg::authorities()), json!({})))
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
                both(sql::AUTHORITY, pg::authority()),
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
                both(sql::OBSERVE, pg::observe()),
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
                both(sql::ADMISSION_RECORD, pg::admission_record()),
                json!({"slug": slug, "revision": revision}),
            )
        })?;
        if body.is_null() {
            return Ok(None);
        }
        serde_json::from_value(body)
            .map(Some)
            .map_err(|e| Error::Db(format!("an admission not in the board's shape: {e}")))
    }

    /// Why the guards that brought `slug` into the pair its effect was prepared from, by the
    /// prepare at its transition `revision`, no longer stand ([`guard::unstanding`]), none while
    /// they do or where no guard admitted that transition: the admission of the transition before
    /// the prepare, while the prepare is the only transition since, which the dispatch's own
    /// check of the task's revision holds (`Store::dispatch_effect`).
    pub(in crate::store) fn prepared_from_unguarded(
        &self,
        slug: &str,
        revision: u64,
    ) -> Result<Option<String>, Error> {
        let Some(before) = revision.checked_sub(1) else {
            return Ok(None);
        };
        let Some(admission) = self.workflow_admission(slug, before)? else {
            return Ok(None);
        };
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
                both(sql::DISPATCH, pg::dispatch()),
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

fn stop(why: String) -> Failure {
    Failure::Stop(Error::Refused(format!("workflow: {why}")))
}
