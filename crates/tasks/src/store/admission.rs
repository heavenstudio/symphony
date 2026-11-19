//! The one admission of a workflow operation on the board (docs/WORKFLOW.md, "Admission"): the
//! active workflow read, a principal's next actions previewed, and an operation applied. Every
//! surface that moves a task a custom workflow holds (the CLI, the api the app writes through, an
//! agent) calls [`Store::apply_operation`], with the principal its host bound, never one a request
//! names.
//!
//! An operation is decided on one read ([`crate::workflow::admit`]) and written in one transaction
//! that checks again everything the decision used (`sql::APPLY`): the active generation, the
//! task's version, each dependency's, each role relied on or taken, its key, its agent, and the
//! slot counters it takes under the cap. Anything moved is `stale`, and the board reads and
//! decides again: a caller's expected snapshot is compared afresh each time, so a request the
//! board moved past is refused as stale, never replayed on where the winner left the task.
//!
//! A request's key is the principal's: the same key with the same request answers the result the
//! first one wrote, and the same key with another request is refused.

use super::postgres as pg;
use super::surreal::{self, cap as sql_cap, workflow as sql};
use super::{Error, Failure, Opens, Store, Which, statement::both};
use crate::workflow::admit::{self, Context, Facts, Prerequisite, Refusal};
use crate::workflow::{Active, Host, Recorded, TaskPosition, Transition};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use symphony_api::workflow::{
    Authority, Effect, NextActions, OperationRequest, OperationResult, RefusalCode, Role,
    WorkflowRead,
};
use symphony_plugin::execution::{Principal, PrincipalKind};

/// What a request's key keeps (`workflow:['receipt', KIND, ISSUER, SUBJECT, KEY]`): whose it is,
/// the task, the digest of the request it was used for, and what that request wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub principal: Principal,
    pub key: String,
    pub slug: String,
    pub request: String,
    /// The request itself, which its principal may send again for its result.
    pub sent: OperationRequest,
    pub transition: String,
    pub effects: Vec<Effect>,
}

#[derive(Deserialize)]
struct Read {
    active: Option<Active>,
    task: Option<TaskRead>,
    roles: Vec<RoleRead>,
    dependencies: Vec<String>,
    found: Vec<Found>,
    receipt: Option<Receipt>,
    present: bool,
}

#[derive(Deserialize)]
struct TaskRead {
    slug: String,
    version: i64,
    status: String,
    created_by: String,
    position: Option<TaskPosition>,
}

#[derive(Deserialize)]
struct RoleRead {
    handle: String,
    role: String,
}

#[derive(Deserialize)]
struct Found {
    slug: String,
    version: i64,
    status: String,
    position: Option<TaskPosition>,
}

/// One read's decision context: the workflow active and its definition, and the task's facts.
struct Decided {
    active: Active,
    recorded: Recorded,
    facts: Facts,
    task: TaskRead,
    found: Vec<Found>,
    receipt: Option<Receipt>,
}

impl Store {
    /// The active custom workflow, whole: its definition, revision and generation; none on a board
    /// that never activated one.
    pub fn workflow_read(&self) -> Result<Option<WorkflowRead>, Error> {
        let Some(active) = self.workflow_active()? else {
            return Ok(None);
        };
        let recorded = self.recorded(&active).map_err(stopped)?;
        Ok(Some(WorkflowRead {
            definition: recorded.definition,
            revision: recorded.revision,
            activation_generation: active.generation,
        }))
    }

    /// The edges `principal` could take from where `slug` stands, each with what stops it now
    /// ([`admit::next_actions`]): a preview, never a permit. Refused, its code first, for a task
    /// no workflow holds or a board with no workflow active.
    pub fn workflow_actions(
        &self,
        slug: &str,
        principal: &Principal,
        host: &Host,
    ) -> Result<NextActions, Error> {
        self.retrying(|| {
            let decided = self.decided(slug, principal, None)?;
            let context = context(&decided);
            admit::next_actions(context, &decided.facts, principal, host).map_err(refusal)
        })
    }

    /// `request` admitted for `principal`, whom the host bound, and written in one transaction
    /// fenced on everything its decision read, or refused, its code first
    /// ([`admit::Refusal`]); capacity under the project's cap is decided in the write's own
    /// counters. Answers the transition it wrote, its effects and the principal's next actions
    /// after it. A request repeating a key the principal used answers that request's result when
    /// it is the same request, and is refused when it is another.
    pub fn apply_operation(
        &self,
        principal: &Principal,
        host: &Host,
        request: &OperationRequest,
    ) -> Result<OperationResult, Error> {
        let board = self.database().database();
        if request.project != board {
            return Err(Error::Refused(
                Refusal::new(
                    RefusalCode::UnknownId,
                    format!(
                        "the request is for project {}, and this is the board of {board}",
                        request.project
                    ),
                )
                .to_string(),
            ));
        }
        let receipt_id = receipt_id(principal, &request.idempotency_key);
        let digest = request_digest(request);
        let slug = request.task.as_str();
        let receipt = self.retrying(|| {
            let decided = self.decided(slug, principal, Some(&receipt_id))?;
            if let Some(receipt) = decided.receipt {
                if receipt.request == digest && receipt.slug == slug {
                    return Ok(receipt);
                }
                return Err(refusal(Refusal::new(
                    RefusalCode::ExplicitActionRequired,
                    format!(
                        "the key {} was used for another request ({}); a new request takes a \
                         new key",
                        request.idempotency_key, receipt.transition
                    ),
                )));
            }
            let context = context(&decided);
            let at = self.now();
            let admitted = admit::admit(context, &decided.facts, principal, host, request, at)
                .map_err(refusal)?;
            let transition = &admitted.transition;
            let status = transition.status.clone();
            let taking = Taking {
                status: &status,
                acquires: admitted.acquires,
                add: &admitted.add,
                what: format!("{slug} cannot take {}", request.edge),
            };
            let slots = self.taken(&decided, taking, principal)?;
            let receipt = Receipt {
                principal: principal.clone(),
                key: request.idempotency_key.clone(),
                slug: slug.to_string(),
                request: digest.clone(),
                sent: request.clone(),
                transition: transition.id.clone(),
                effects: transition.effects.clone(),
            };
            let row = |(handle, role): &(String, String)| {
                json!({"slug": slug, "handle": handle, "role": role, "since": at})
            };
            let pair = |(handle, role): &(String, String)| json!({"handle": handle, "role": role});
            let holding: Vec<Value> = relied_on(&decided.facts, principal, request, context)
                .iter()
                .map(pair)
                .collect();
            let guarded = !slots.is_empty();
            let vars = json!({
                "slug": slug,
                "version": decided.task.version,
                "generation": decided.active.generation,
                "digest": decided.active.digest,
                "dependencies": fenced(&decided, request),
                "holding": holding,
                "add": admitted.add.iter().map(row).collect::<Vec<_>>(),
                "drop": admitted.drop.iter().map(pair).collect::<Vec<_>>(),
                "receipt": receipt_id,
                "receipt_body": receipt,
                "actor": agent(principal),
                "transition": transition,
                "position": transition.position(),
                "moved": status != decided.task.status,
                "sender": sender(principal),
                "said": said(transition),
                "now": at,
                "slots": slots,
                "seen": [],
            });
            let logged = surreal::logged("$slug", "$sender", "'system'", "$said", "NONE");
            let guard = if guarded { sql_cap::GUARD } else { "" };
            let body = both(
                format!("{}{logged}{guard}", sql::APPLY),
                pg::workflow::apply(
                    &pg::logged("v ->> 'slug'", "v ->> 'sender'", "'system'", "v ->> 'said'", "NULL"),
                    guarded,
                ),
            );
            self.written(body, vars, slug)?;
            Ok(receipt)
        })?;
        let next = self.workflow_actions(slug, principal, host)?;
        Ok(OperationResult {
            transition: receipt.transition,
            next,
            effects: receipt.effects,
        })
    }

    /// The receipt `principal`'s `key` names, none when no request of its used it: what a caller
    /// that lost the answer sends again for it.
    pub fn workflow_receipt(
        &self,
        principal: &Principal,
        key: &str,
    ) -> Result<Option<Receipt>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::RECEIPT, pg::workflow::receipt()),
                json!({"receipt": receipt_id(principal, key)}),
            )
        })?;
        if body.is_null() {
            return Ok(None);
        }
        serde_json::from_value(body)
            .map(Some)
            .map_err(|e| Error::Db(format!("a workflow receipt not in the board's shape: {e}")))
    }

    /// The first transition of `new`, filed while a custom workflow is active: it starts at the
    /// workflow's initial pair, which its filing records (`sql::PLACED`). None on a board that
    /// never activated one, which files as it always did. Refused when the filing asks for a
    /// status other than the initial pair's: a task starts there, and moves on by an edge.
    pub(super) fn placing(&self, new: &crate::NewTask) -> Result<Option<Transition>, Error> {
        let Some(active) = self.workflow_active()? else {
            return Ok(None);
        };
        let recorded = self.recorded(&active).map_err(stopped)?;
        let initial = recorded.definition.initial.clone();
        let position = (recorded.definition.positions.iter())
            .find(|p| p.at == initial)
            .ok_or_else(|| Error::Db("the active workflow declares no initial position".into()))?;
        let status = crate::workflow::activate::projection(position.legacy);
        if new.status != status {
            return Err(Error::Refused(format!(
                "explicit_action_required: a new task starts at the workflow's initial pair \
                 {}/{} ({status}), not {}: file it there and move it on by an edge",
                initial.phase, initial.state, new.status
            )));
        }
        let principal = if new.by == super::work::OWNER {
            crate::workflow::local_owner()
        } else {
            crate::workflow::agent_session(&new.by)
        };
        Ok(Some(Transition {
            id: Transition::id_of(&new.slug, 1),
            slug: new.slug.clone(),
            revision: 1,
            generation: active.generation,
            digest: active.digest,
            edge: None,
            operation: Some(symphony_api::workflow::Operation::Create),
            activation: None,
            from: None,
            to: initial,
            resume: None,
            status,
            principal,
            effects: Vec::new(),
            key: None,
            recovery: None,
            at: self.now(),
        }))
    }

    /// What admission decides on, read once: refused for a board with no workflow active, a task
    /// not on the board.
    fn decided(
        &self,
        slug: &str,
        principal: &Principal,
        receipt: Option<&Value>,
    ) -> Result<Decided, Failure> {
        let vars = json!({
            "slug": slug,
            "receipt": receipt.cloned().unwrap_or(json!(["receipt"])),
            "actor": agent(principal),
        });
        let read: Read = self.transact(both(sql::ADMISSION, pg::workflow::admission()), vars)?;
        let active = read.active.ok_or_else(|| {
            refusal(Refusal::new(
                RefusalCode::UnsupportedActivation,
                "no custom workflow is active on this board: the board's own writers keep its tasks",
            ))
        })?;
        let task = read.task.ok_or_else(|| {
            refusal(Refusal::new(
                RefusalCode::UnknownId,
                format!("no task {slug}"),
            ))
        })?;
        let recorded = self.recorded(&active)?;
        let roles: Vec<(String, String)> = (read.roles.into_iter())
            .map(|r| (r.handle, r.role))
            .collect();
        let reviews = (recorded.definition.edges.iter())
            .filter(|e| task.position.as_ref().is_some_and(|p| p.at() == e.from))
            .any(|e| {
                e.authority.contains(&Authority::TaskRole {
                    role: Role::Reviewer,
                })
            });
        let held = if reviews {
            self.ever_held(slug).map_err(Failure::Stop)?
        } else {
            crate::Held::default()
        };
        let mut dependencies = read.dependencies;
        dependencies.sort();
        dependencies.dedup();
        let facts = Facts {
            slug: task.slug.clone(),
            created_by: task.created_by.clone(),
            position: task.position.clone(),
            roles,
            dependencies: (dependencies.iter())
                .map(|d| Prerequisite {
                    slug: d.clone(),
                    found: (read.found.iter())
                        .find(|f| f.slug == *d)
                        .map(|f| (f.status.clone(), f.position.clone())),
                })
                .collect(),
            held,
            present: read.present,
        };
        Ok(Decided {
            active,
            recorded,
            facts,
            task,
            found: read.found,
            receipt: read.receipt,
        })
    }

    /// The definition `active` names, as activation recorded it.
    fn recorded(&self, active: &Active) -> Result<Recorded, Failure> {
        self.workflow_definition(&active.digest)
            .map_err(Failure::Stop)?
            .ok_or_else(|| {
                Failure::Stop(Error::Db(format!(
                    "the active workflow's definition {} is not recorded",
                    active.digest
                )))
            })
    }

    /// The slot counters an admitted operation holds under the project's cap: one that takes a
    /// slot, or moves the task into work or review from outside them, is counted as a start
    /// would be, and refused as `capacity` past the cap.
    fn taken(
        &self,
        decided: &Decided,
        taking: Taking<'_>,
        principal: &Principal,
    ) -> Result<Vec<Value>, Failure> {
        let status = taking.status;
        let starts = crate::slots::OPEN.contains(&status)
            && !crate::slots::OPEN.contains(&decided.task.status.as_str());
        let Some(cap) = self.slots() else {
            return Ok(Vec::new());
        };
        if !(taking.acquires || starts) {
            return Ok(Vec::new());
        }
        let slug = &decided.task.slug;
        let view = self.view(&Which::Starting(slug))?;
        let mut by: Vec<String> = (taking.add.iter())
            .filter(|(_, role)| role == "executor")
            .map(|(handle, _)| handle.clone())
            .chain(view.holders("executor").map(|r| r.handle.clone()))
            .collect();
        if by.is_empty()
            && let Some(handle) = agent(principal)
        {
            by.push(handle.to_string());
        }
        let opens = Opens {
            status: Some(status.to_string()),
            by,
            what: taking.what,
            ..Opens::default()
        };
        self.slots_taken(&view, slug, &opens, cap)
            .map_err(|failure| match failure {
                Failure::Stop(Error::Refused(why)) => {
                    refusal(Refusal::new(RefusalCode::Capacity, why))
                }
                other => other,
            })
    }
}

/// What a write takes under the cap: the status it moves its task to, whether it takes a slot,
/// the roles it adds, and what a refusal says cannot happen.
struct Taking<'a> {
    status: &'a str,
    acquires: bool,
    add: &'a [(String, String)],
    what: String,
}

fn context(decided: &Decided) -> Context<'_> {
    Context {
        definition: &decided.recorded.definition,
        revision: &decided.recorded.revision,
        generation: decided.active.generation,
    }
}

/// The dependencies the write fences, at the version read: each one the edge's predicate read,
/// none for an edge that reads none.
fn fenced(decided: &Decided, request: &OperationRequest) -> Vec<Value> {
    let edge = (decided.recorded.definition.edges.iter()).find(|e| e.id == request.edge);
    if edge.is_none_or(|e| e.dependencies == symphony_api::workflow::Dependency::None) {
        return Vec::new();
    }
    (decided.found.iter())
        .map(|f| json!({"slug": f.slug, "version": f.version}))
        .collect()
}

/// The role rows the admitted edge's authority relied on, which the write checks still stand:
/// a task role the principal holds here and does not take by the edge itself.
fn relied_on(
    facts: &Facts,
    principal: &Principal,
    request: &OperationRequest,
    context: Context,
) -> Vec<(String, String)> {
    let Some(handle) = agent(principal) else {
        return Vec::new();
    };
    let Some(edge) = context
        .definition
        .edges
        .iter()
        .find(|e| e.id == request.edge)
    else {
        return Vec::new();
    };
    (edge.authority.iter())
        .filter_map(|a| match a {
            Authority::TaskRole { role } => Some(admit::word(role)),
            _ => None,
        })
        .filter(|role| (facts.roles.iter()).any(|(h, r)| h == handle && r == role))
        .map(|role| (handle.to_string(), role))
        .collect()
}

/// The receipt record's id: the principal's, then its key.
fn receipt_id(principal: &Principal, key: &str) -> Value {
    json!([
        "receipt",
        admit::word(&principal.kind),
        principal.issuer,
        principal.subject,
        key
    ])
}

/// The request's identity for its key: the digest of its JSON, whose maps are ordered.
fn request_digest(request: &OperationRequest) -> String {
    let text = crate::workflow::canonical(request).expect("a request serializes");
    Sha256::digest(text)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The handle of an agent principal, which the write checks is still on the board.
fn agent(principal: &Principal) -> Option<&str> {
    (principal.kind == PrincipalKind::Agent && principal.issuer == "local")
        .then_some(principal.subject.as_str())
}

/// Who the thread names as having taken the edge.
fn sender(principal: &Principal) -> String {
    match (principal.kind, principal.issuer.as_str()) {
        (PrincipalKind::Agent | PrincipalKind::Human, "local") => principal.subject.clone(),
        _ => format!("{}/{}", principal.issuer, principal.subject),
    }
}

/// The transition's line on its task's thread.
fn said(transition: &Transition) -> String {
    let named = |p: &symphony_api::workflow::Pair| format!("{}/{}", p.phase, p.state);
    format!(
        "took {} ({}): {} → {}, workflow revision {}",
        transition.edge.as_deref().unwrap_or("an edge"),
        transition.id,
        transition.from.as_ref().map_or_else(String::new, named),
        named(&transition.to),
        transition.revision
    )
}

fn refusal(refusal: Refusal) -> Failure {
    Failure::Stop(Error::Refused(refusal.to_string()))
}

fn stopped(failure: Failure) -> Error {
    match failure {
        Failure::Stop(error) => error,
        Failure::Again => Error::Db("the board kept moving under a workflow read".into()),
    }
}

mod recovery;
pub(in crate::store) use recovery::Released;

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
