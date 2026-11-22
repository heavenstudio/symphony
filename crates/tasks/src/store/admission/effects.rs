//! The writes of an external effect's boundary (docs/WORKFLOW.md, "External effects"), decided by
//! [`crate::workflow::effect`]: a prepare edge admitted and its intent recorded in one write, the
//! intent moved on to dispatched or withdrawn, and the finalize edge taken with what the adapter
//! read back. Each asks the host-verified adapter, never the request, for what the effect is bound
//! to and for what happened, outside any transaction, and writes fenced on what it read: the
//! intent's stage, the task's version, the active generation and digest.
//!
//! A prepare or finalize edge is taken here alone: [`Store::apply_operation`] refuses one, as a
//! caller's word never prepares or finalizes.

use super::{
    Decided, Error, Failure, Host, OperationRequest, Receipt, Refusal, RefusalCode, Store, admit,
    applied, context, guarding, joins_said, pg, receipt_id, refusal, request_digest, sender, sql,
    sql_cap, surreal,
};
use crate::store::statement::both;
use crate::workflow::effect::{self, Adapter, Bindings, Intent, Stage};
use crate::workflow::{Active, TaskPosition, Transition};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_api::workflow::{InputValue, PositionSnapshot};
use symphony_plugin::execution::Principal;

/// What a finalize did: the intent as it stands, and the transition that took the finalize edge,
/// none where the outcome is recorded and the edge could not be taken
/// ([`Intent::recovery`] says why).
#[derive(Clone, Debug, PartialEq)]
pub struct Finalized {
    pub intent: Intent,
    pub transition: Option<String>,
}

/// The approve standing on a task a custom workflow holds ([`Store::reviewed`]): the transition
/// that recorded it, who took it, when, and the head its request named, none when it named none.
#[derive(Clone, Debug, PartialEq)]
pub struct Reviewed {
    pub transition: String,
    pub by: Principal,
    pub head: Option<String>,
    pub at: f64,
}

/// What a dispatch or a finalize decides on ([`surreal::effects::STANDING`]).
#[derive(Deserialize)]
struct Standing {
    intent: Option<Intent>,
    active: Option<Active>,
    task: Option<Stood>,
}

#[derive(Deserialize)]
struct Stood {
    version: i64,
    position: Option<TaskPosition>,
}

impl Store {
    /// The intent the prepare that made workflow revision `revision` of `slug` recorded, or none.
    pub fn effect_intent(&self, slug: &str, revision: u64) -> Result<Option<Intent>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(surreal::effects::INTENT, pg::effects::intent()),
                json!({"slug": slug, "revision": revision}),
            )
        })?;
        if body.is_null() {
            return Ok(None);
        }
        crate::objects::read(&body)
            .map(Some)
            .map_err(|e| Error::Db(format!("a workflow intent not in the board's shape: {e}")))
    }

    /// The transition that made workflow revision `revision` of `slug`, or none.
    pub fn workflow_transition(
        &self,
        slug: &str,
        revision: u64,
    ) -> Result<Option<Transition>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(surreal::effects::TRANSITION, pg::effects::transition()),
                json!({"slug": slug, "revision": revision}),
            )
        })?;
        if body.is_null() {
            return Ok(None);
        }
        crate::objects::read(&body).map(Some).map_err(|e| {
            Error::Db(format!(
                "a workflow transition not in the board's shape: {e}"
            ))
        })
    }

    /// The review standing on `slug`, a task a custom workflow holds, read from its transitions
    /// and never from a verdict message, which a workflow's approve writes none of: the last
    /// transition whose edge recorded a review, when it approved, with the head its request gave
    /// as its `head` input, read back from that request's receipt. None when its last review asked
    /// for changes, or no edge it took recorded one.
    pub fn reviewed(&self, slug: &str) -> Result<Option<Reviewed>, Error> {
        use symphony_api::workflow::Effect;
        if self.workflow_position(slug)?.is_none() {
            return Ok(None);
        }
        // Every transition of the task in one read, newest first.
        for transition in self.workflow_transitions(slug)?.into_iter().rev() {
            let review = (transition.effects.iter()).find_map(|e| match e {
                Effect::Review { approved } => Some(*approved),
                _ => None,
            });
            match review {
                None => continue,
                Some(false) => return Ok(None),
                Some(true) => {}
            }
            let receipt = match &transition.key {
                Some(key) => self.workflow_receipt(&transition.principal, key)?,
                None => None,
            };
            let head = receipt.and_then(|receipt| match receipt.sent.inputs.get("head") {
                Some(InputValue::Revision(head)) => Some(head.clone()),
                _ => None,
            });
            return Ok(Some(Reviewed {
                transition: transition.id,
                by: transition.principal,
                head,
                at: transition.at,
            }));
        }
        Ok(None)
    }

    /// `request`, a prepare edge of `adapter`'s, admitted for `principal` as any operation is and
    /// written with its intent, bound to `bindings`, in the one write: the intent is durable
    /// before the adapter sends anything. Refused, its code first, for an adapter the host did not
    /// register, bindings of another project or task, an edge that prepares nothing for the
    /// adapter, and everything admission refuses. A request repeating its key answers the intent
    /// it recorded, and is refused when its bindings are others.
    pub fn prepare_effect(
        &self,
        principal: &Principal,
        host: &Host,
        adapter: &dyn Adapter,
        request: &OperationRequest,
        bindings: &Bindings,
    ) -> Result<Intent, Error> {
        let name = registered(host, adapter)?;
        let board = self.database().database();
        let slug = request.task.as_str();
        if request.project != board || bindings.project != board || bindings.task != slug {
            return Err(refused(
                RefusalCode::UnknownId,
                format!(
                    "the effect is bound to {} of project {}, and the request is for {slug} of {}, \
                     on the board of {board}",
                    bindings.task, bindings.project, request.project
                ),
            ));
        }
        let receipt_id = receipt_id(principal, &request.idempotency_key);
        let digest = request_digest(request);
        self.retrying(|| {
            let decided = self.decided(slug, principal, Some(&receipt_id))?;
            if let Some(receipt) = &decided.receipt {
                return self
                    .prepared_before(receipt, &digest, request, bindings)
                    .map_err(Failure::Stop);
            }
            let at = self.now();
            // The adapter takes the edge, and no attestation comes with it: a guarded prepare
            // edge is refused, as activation refuses one (`activate::supported_with`).
            let admitted = admit::admit(
                context(&decided),
                &decided.facts,
                principal,
                host,
                request,
                &guarding(&decided, &[]),
                at,
            )
            .map_err(refusal)?;
            let transition = &admitted.transition;
            let prepares = (transition.effects.iter())
                .any(|e| matches!(e, symphony_api::workflow::Effect::Prepare { adapter } if adapter == name));
            if !prepares {
                return Err(refusal(Refusal::new(
                    RefusalCode::UnsupportedAdapter,
                    format!("edge {} prepares nothing for the adapter {name}", request.edge),
                )));
            }
            let intent = Intent {
                id: transition.id.clone(),
                slug: slug.to_string(),
                revision: transition.revision,
                adapter: name.to_string(),
                key: request.idempotency_key.clone(),
                bindings: bindings.clone(),
                generation: decided.active.generation,
                digest: decided.active.digest.clone(),
                principal: principal.clone(),
                stage: Stage::Prepared,
                receipt: None,
                finalized: None,
                recovery: None,
                withdrawn: None,
                at,
            };
            let said = format!(
                "prepared {} for {name} ({}): {} → {}, bound to head {}, workflow revision {}",
                request.edge,
                intent.id,
                transition.from.as_ref().map_or_else(String::new, named),
                named(&transition.to),
                bindings.head,
                transition.revision
            );
            let (mut vars, _) = applied(self, &decided, &admitted, principal, request, &digest, at)?;
            vars["said"] = json!(said);
            vars["intent"] = json!(intent);
            self.apply_with(
                vars,
                slug,
                surreal::effects::PREPARED,
                pg::effects::PREPARED,
            )?;
            Ok(intent)
        })
    }

    /// The intent a request's key recorded before, when the request and its bindings are the same.
    fn prepared_before(
        &self,
        receipt: &Receipt,
        digest: &str,
        request: &OperationRequest,
        bindings: &Bindings,
    ) -> Result<Intent, Error> {
        let other = || {
            refused(
                RefusalCode::ExplicitActionRequired,
                format!(
                    "the key {} was used for another request ({}); a new request takes a new key",
                    request.idempotency_key, receipt.transition
                ),
            )
        };
        if receipt.request != digest || receipt.slug != request.task {
            return Err(other());
        }
        let revision = (receipt.transition.rsplit_once('@'))
            .and_then(|(_, revision)| revision.parse().ok())
            .ok_or_else(other)?;
        match self.effect_intent(&request.task, revision)? {
            Some(intent) if intent.bindings == *bindings => Ok(intent),
            _ => Err(other()),
        }
    }

    /// The intent of `slug` at `revision` moved on to dispatched, once `adapter` has read its
    /// bindings again and they are the ones bound, the workflow is the one that admitted its
    /// prepare, the task has not moved since, and each guard that brought it into the pair it was
    /// prepared from stands at the authority's state it was met at (`guard::unstanding`): the
    /// adapter may then send its request. A
    /// reservation stale or cancelled by then is withdrawn and refused, and nothing is sent under
    /// it. One dispatched before is read again the same way, so a run again sends nothing for
    /// bindings that moved; refused where its effect happened already, which is finalized instead.
    pub fn dispatch_effect(
        &self,
        adapter: &dyn Adapter,
        slug: &str,
        revision: u64,
    ) -> Result<Intent, Error> {
        let intent = self.open_intent(adapter, slug, revision)?;
        match intent.stage {
            Stage::Prepared => {}
            Stage::Dispatched => {
                if adapter.outcome(&intent).map_err(unread)?.is_some() {
                    return Err(refused(
                        RefusalCode::ExplicitActionRequired,
                        format!(
                            "the effect of {} happened already: it is finalized, not sent again",
                            intent.id
                        ),
                    ));
                }
            }
            Stage::Finalized => {
                return Err(refused(
                    RefusalCode::ExplicitActionRequired,
                    format!("{} is finalized: nothing more is sent under it", intent.id),
                ));
            }
            Stage::Withdrawn => return Err(withdrawn(&intent)),
        }
        let now = adapter.current(&intent).map_err(unread)?;
        // The guards that brought the task into the pair it was prepared from, met again now:
        // a decision revoked since withdraws the intent, and nothing is sent under it.
        let unguarded = self.prepared_from_unguarded(slug, revision)?;
        let moved = self.retrying(|| {
            let standing = self.standing(slug, revision)?;
            let held = standing.intent.ok_or(Failure::Again)?;
            let stage_was = held.stage;
            match stage_was {
                Stage::Prepared | Stage::Dispatched => {}
                // Another dispatch withdrew it first: its answer is this one's.
                Stage::Withdrawn => return Ok(held),
                Stage::Finalized => {
                    return Err(Failure::Stop(refused(
                        RefusalCode::ExplicitActionRequired,
                        format!("{} is finalized: nothing more is sent under it", held.id),
                    )));
                }
            }
            let position = standing.task.as_ref().and_then(|t| t.position.as_ref());
            let stale = effect::stale(&held, standing.active.as_ref(), position, Some(&now))
                .or_else(|| unguarded.clone());
            let next = Intent {
                stage: if stale.is_some() {
                    Stage::Withdrawn
                } else {
                    Stage::Dispatched
                },
                withdrawn: stale,
                ..held
            };
            self.settle(
                &next,
                stage_was,
                &standing.active,
                standing.task.as_ref(),
                None,
            )?;
            Ok(next)
        })?;
        if moved.stage == Stage::Withdrawn {
            return Err(withdrawn(&moved));
        }
        Ok(moved)
    }

    /// The effect of the intent of `slug` at `revision` finalized on what `adapter` read back of
    /// it, never on a caller's word: refused while the adapter reads back nothing, and for an
    /// intent never dispatched. The receipt read back is recorded, and the finalize edge leaving
    /// the task's pair is taken for `principal`, as an admitted operation is. Where the policy or
    /// the evidence it was bound to was another when the effect happened, the outcome is recorded
    /// with the recovery a person owes it, said to the task's orchestrator; where the task or its
    /// workflow moved in the gap, or the edge is refused, the outcome and that requirement are
    /// recorded and no edge is taken, and a call again takes it once it can be. One finalized
    /// before answers what it recorded.
    pub fn finalize_effect(
        &self,
        principal: &Principal,
        host: &Host,
        adapter: &dyn Adapter,
        slug: &str,
        revision: u64,
    ) -> Result<Finalized, Error> {
        let name = registered(host, adapter)?;
        let intent = self.open_intent(adapter, slug, revision)?;
        match intent.stage {
            Stage::Finalized if intent.finalized.is_some() => {
                return Ok(Finalized {
                    transition: intent.finalized.clone(),
                    intent,
                });
            }
            Stage::Dispatched | Stage::Finalized => {}
            Stage::Prepared => return Err(undispatched(&intent)),
            Stage::Withdrawn => return Err(withdrawn(&intent)),
        }
        let Some(outcome) = adapter.outcome(&intent).map_err(unread)? else {
            return Err(refused(
                RefusalCode::ExplicitActionRequired,
                format!(
                    "the adapter {name} read back no result of {}: an effect is finalized on what \
                     its adapter verified, never on a caller's word",
                    intent.id
                ),
            ));
        };
        let key = format!("{}:finalize", intent.key);
        let receipt_id = receipt_id(principal, &key);
        self.retrying(|| {
            let standing = self.standing(slug, revision)?;
            let held = standing.intent.clone().ok_or(Failure::Again)?;
            if held.stage == Stage::Finalized && held.finalized.is_some() {
                return Ok(Finalized {
                    transition: held.finalized.clone(),
                    intent: held,
                });
            }
            let stage_was = held.stage;
            match stage_was {
                Stage::Dispatched | Stage::Finalized => {}
                // A dispatch run again withdrew it since this finalize read it: reading it again
                // finds the same, so it is answered now.
                Stage::Withdrawn => return Err(Failure::Stop(withdrawn(&held))),
                Stage::Prepared => return Err(Failure::Stop(undispatched(&held))),
            }
            let position = standing.task.as_ref().and_then(|t| t.position.as_ref());
            let mut recovery =
                effect::requirement(&held, &outcome, standing.active.as_ref(), position);
            let reserved = effect::stale(&held, standing.active.as_ref(), position, None).is_none();
            // The edge is taken only from the reservation itself, under the workflow that made it.
            let taken = if reserved {
                let decided = self.decided(slug, principal, Some(&receipt_id))?;
                match self.finalizing(&decided, principal, host, &held, &outcome.receipt, &key) {
                    Ok(taken) => Some((decided, taken)),
                    Err(Failure::Stop(Error::Refused(why))) => {
                        let why = format!("its finalize edge is refused ({why})");
                        recovery = Some(match recovery {
                            Some(before) => format!("{before}; {why}"),
                            None => why,
                        });
                        None
                    }
                    Err(other) => return Err(other),
                }
            } else {
                None
            };
            let mut next = Intent {
                stage: Stage::Finalized,
                receipt: Some(outcome.receipt.clone()),
                recovery: recovery.clone(),
                ..held
            };
            let recipient = recovery.as_ref().map(|_| "@orchestrator");
            let Some((decided, (request, admitted))) = taken else {
                let said = format!(
                    "the effect of {} happened ({name}: {}), and its finalize edge is not taken: \
                     {}. A person recovers it: the outcome is recorded, and nothing here says the \
                     policy now in force accepts it",
                    next.id,
                    outcome.receipt,
                    recovery
                        .as_deref()
                        .unwrap_or("its reservation no longer stands")
                );
                self.settle(
                    &next,
                    stage_was,
                    &standing.active,
                    standing.task.as_ref(),
                    Some((&sender(principal), &said, recipient)),
                )?;
                return Ok(Finalized {
                    intent: next,
                    transition: None,
                });
            };
            let transition = &admitted.transition;
            next.finalized = Some(transition.id.clone());
            let mut said = format!(
                "finalized {} by {name} ({}): {}; {} → {}, workflow revision {}",
                next.id,
                transition.id,
                outcome.receipt,
                transition.from.as_ref().map_or_else(String::new, named),
                named(&transition.to),
                transition.revision
            );
            if let Some(recovery) = &recovery {
                said.push_str(&format!(
                    ". RECOVERY REQUIRED: {recovery}. The effect happened and is recorded as it \
                     was authorized; nothing here says the policy now in force accepts it"
                ));
            }
            let digest = request_digest(&request);
            let at = transition.at;
            let (mut vars, _) =
                applied(self, &decided, &admitted, principal, &request, &digest, at)?;
            vars["said"] = json!(said);
            vars["recipient"] = json!(recipient);
            vars["intent"] = json!(next);
            vars["stage_was"] = json!(stage_was.as_str());
            self.apply_with(vars, slug, surreal::effects::SETTLED, pg::effects::SETTLED)?;
            Ok(Finalized {
                transition: next.finalized.clone(),
                intent: next,
            })
        })
    }

    /// The finalize edge of `intent`'s adapter leaving the task's pair, admitted for `principal`
    /// with the receipt read back as its `receipt` input where the edge declares one.
    fn finalizing(
        &self,
        decided: &Decided,
        principal: &Principal,
        host: &Host,
        intent: &Intent,
        receipt: &str,
        key: &str,
    ) -> Result<(OperationRequest, admit::Admitted), Failure> {
        let context = context(decided);
        let position = decided.facts.position.as_ref().ok_or_else(|| {
            refusal(Refusal::new(
                RefusalCode::UnknownId,
                format!("no workflow holds {}", intent.slug),
            ))
        })?;
        let edge = effect::finalize_edge(context.definition, &position.at(), &intent.adapter)
            .ok_or_else(|| {
                refusal(Refusal::new(
                    RefusalCode::UnknownId,
                    format!(
                        "no edge leaving {} finalizes for the adapter {}",
                        named(&position.at()),
                        intent.adapter
                    ),
                ))
            })?;
        let mut inputs = BTreeMap::new();
        if edge.inputs.iter().any(|input| input == "receipt") {
            inputs.insert("receipt".to_string(), InputValue::Revision(receipt.into()));
        }
        let request = OperationRequest {
            project: self.database().database().into(),
            task: intent.slug.clone(),
            edge: edge.id.clone(),
            expected: PositionSnapshot {
                workflow: context.revision.clone(),
                activation_generation: context.generation,
                task_revision: position.revision,
                at: position.at(),
            },
            input_revisions: BTreeMap::new(),
            idempotency_key: key.to_string(),
            inputs,
        };
        let admitted = admit::admit(
            context,
            &decided.facts,
            principal,
            host,
            &request,
            &guarding(decided, &[]),
            self.now(),
        )
        .map_err(refusal)?;
        Ok((request, admitted))
    }

    /// The intent of `slug` at `revision`, which is `adapter`'s; refused where there is none, or it
    /// is another adapter's.
    fn open_intent(
        &self,
        adapter: &dyn Adapter,
        slug: &str,
        revision: u64,
    ) -> Result<Intent, Error> {
        let intent = self.effect_intent(slug, revision)?.ok_or_else(|| {
            refused(
                RefusalCode::UnknownId,
                format!("no effect was prepared at workflow revision {revision} of {slug}"),
            )
        })?;
        if intent.adapter != adapter.name() {
            return Err(refused(
                RefusalCode::UnsupportedAdapter,
                format!(
                    "{} is the adapter {}'s, not {}'s",
                    intent.id,
                    intent.adapter,
                    adapter.name()
                ),
            ));
        }
        Ok(intent)
    }

    fn standing(&self, slug: &str, revision: u64) -> Result<Standing, Failure> {
        self.read_together(
            both(surreal::effects::STANDING, pg::effects::standing()),
            json!({"slug": slug, "revision": revision}),
        )
    }

    /// `next` written over the intent read at `stage_was`, with no transition of its task, fenced
    /// on the task's version and the active workflow as `standing` read them; `line`, its sender,
    /// text and recipient, said on the thread.
    fn settle(
        &self,
        next: &Intent,
        stage_was: Stage,
        active: &Option<Active>,
        task: Option<&Stood>,
        line: Option<(&str, &str, Option<&str>)>,
    ) -> Result<(), Failure> {
        let (surreal_line, pg_line) = match line {
            Some(_) => (
                surreal::logged("$slug", "$sender", "'system'", "$said", "$recipient"),
                pg::logged(
                    "v ->> 'slug'",
                    "v ->> 'sender'",
                    "'system'",
                    "v ->> 'said'",
                    "v ->> 'recipient'",
                ),
            ),
            None => (String::new(), String::new()),
        };
        let (sender, said, recipient) = line.unwrap_or_default();
        let vars = json!({
            "slug": next.slug,
            "version": task.map_or(-1, |t| t.version),
            "generation": active.as_ref().map_or(0, |a| a.generation),
            "digest": active.as_ref().map_or("", |a| a.digest.as_str()),
            "intent": next,
            "stage_was": stage_was.as_str(),
            "sender": sender,
            "said": said,
            "recipient": recipient,
            "now": self.now(),
        });
        self.transact::<Value>(
            both(
                surreal::effects::moved(&surreal_line),
                pg::effects::moved(&pg_line),
            )
            .posting_when(line.is_some()),
            vars,
        )
        .map(drop)
    }

    /// `sql::APPLY` over `vars` with the intent's own statement after it, in one write.
    fn apply_with(
        &self,
        vars: Value,
        slug: &str,
        surreal_intent: &str,
        pg_intent: &str,
    ) -> Result<(), Failure> {
        let guarded = !vars["slots"].as_array().is_none_or(Vec::is_empty);
        let logged = surreal::logged("$slug", "$sender", "'system'", "$said", "$recipient");
        let guard = if guarded { sql_cap::GUARD } else { "" };
        let pg_logged = pg::logged(
            "v ->> 'slug'",
            "v ->> 'sender'",
            "'system'",
            "v ->> 'said'",
            "v ->> 'recipient'",
        );
        let (joins, joins_pg) = joins_said(&vars);
        let body = both(
            format!("{}{surreal_intent}{logged}{joins}{guard}", sql::APPLY),
            pg::workflow::apply(&format!("{pg_intent}\n    {pg_logged}{joins_pg}"), guarded),
        )
        .posting();
        self.written(body, vars, slug).map(drop)
    }
}

/// The adapter's name, refused unless the host registered it.
fn registered<'a>(host: &Host, adapter: &'a dyn Adapter) -> Result<&'a str, Error> {
    let name = adapter.name();
    if host.adapters.iter().any(|registered| registered == name) {
        return Ok(name);
    }
    Err(refused(
        RefusalCode::UnsupportedAdapter,
        format!("this host registers no adapter {name}"),
    ))
}

fn refused(code: RefusalCode, message: String) -> Error {
    Error::Refused(Refusal::new(code, message).to_string())
}

/// An adapter that could not read the system its effect is made in.
fn unread(why: String) -> Error {
    refused(
        RefusalCode::UnsupportedAdapter,
        format!("the adapter could not read what its effect is bound to: {why}"),
    )
}

fn undispatched(intent: &Intent) -> Error {
    refused(
        RefusalCode::ExplicitActionRequired,
        format!(
            "{} was never dispatched: nothing was sent under it, so nothing is finalized",
            intent.id
        ),
    )
}

fn withdrawn(intent: &Intent) -> Error {
    refused(
        RefusalCode::StaleInput,
        format!(
            "{} is withdrawn, and nothing is sent under it: {}",
            intent.id,
            intent
                .withdrawn
                .as_deref()
                .unwrap_or("its reservation no longer stands")
        ),
    )
}

fn named(pair: &symphony_api::workflow::Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
