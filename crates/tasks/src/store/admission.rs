//! The one admission of a workflow operation on the board (docs/workflow/WORKFLOW.md, "Admission"): the
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
//!
//! A guarded edge is admitted only with the attestations its caller gives
//! ([`Store::apply_attested`], `crate::workflow::guard`), and its write is fenced on each approval
//! authority's state they were checked at: an observation the board accepts between the read and
//! the write makes the admission decide again, on the authority's new generation.

use super::postgres as pg;
use super::surreal::{self, cap as sql_cap, workflow as sql};
use super::{Error, Failure, Opens, Store, Which, executors_or, statement::both};
use crate::workflow::admit::{self, Context, Facts, Prerequisite, Refusal};
use crate::workflow::guard::{Accepted, Attester, Checked, Guarding};
use crate::workflow::{Active, Host, Recorded, TaskPosition, Transition};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use symphony_api::workflow::{
    Authority, Effect, NextActions, OperationRequest, OperationResult, RefusalCode, Role, Signed,
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

/// What a guarded edge's admission recorded (`workflow:['admission', SLUG, REVISION]`): the
/// transition it admitted, by whom, on which input revisions, and each guard it met, with the
/// attestation and the authority's accepted state. It is the board's audit of what it admitted,
/// never an approval: the decisions stay in the project's ledger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub slug: String,
    pub revision: u64,
    pub transition: String,
    pub edge: String,
    pub principal: Principal,
    pub input_revisions: std::collections::BTreeMap<String, String>,
    pub guards: Vec<Checked>,
    pub at: f64,
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
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    attesters: Vec<Attester>,
    #[serde(default)]
    accepted: Vec<Accepted>,
}

/// The task admission decides on; `accepted` is the accepted output its last transition left
/// (`body.accepted` of the event at its workflow revision), as for each [`Found`].
#[derive(Deserialize)]
struct TaskRead {
    slug: String,
    version: i64,
    status: String,
    created_by: String,
    position: Option<TaskPosition>,
    accepted: Option<String>,
}

#[derive(Deserialize)]
struct RoleRead {
    handle: String,
    role: String,
    #[serde(default)]
    expires: Option<f64>,
}

#[derive(Deserialize)]
struct Found {
    slug: String,
    version: i64,
    status: String,
    position: Option<TaskPosition>,
    accepted: Option<String>,
}

/// The accepted outputs of a task a custom workflow holds (docs/workflow/WORKFLOW.md, "Accepted outputs"):
/// its own standing, and, where an edge leaving its pair depends on `accepted_output`, each task
/// it depends on with the output standing for it, which a request taking that edge names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub own: Option<String>,
    pub takes: Vec<(String, Option<String>)>,
}

/// One read's decision context: the workflow active and its definition, the task's facts, and the
/// approval authorities the activation commits with the state the board accepted from each.
struct Decided {
    active: Active,
    recorded: Recorded,
    facts: Facts,
    task: TaskRead,
    found: Vec<Found>,
    receipt: Option<Receipt>,
    /// The reservations on the task that lapsed (past their expiry, or held by a session lost or
    /// gone), which stand against no claim: the write drops them, fenced on each lost reserver's
    /// session as it was read.
    lapsed: Vec<(String, String)>,
    lapsed_seen: Vec<Value>,
    project: Option<String>,
    attesters: Vec<Attester>,
    accepted: Vec<Accepted>,
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

    /// `slug`'s accepted outputs ([`Outputs`]), read as admission reads them; refused as
    /// [`Store::workflow_actions`] is.
    pub fn accepted_outputs(&self, slug: &str) -> Result<Outputs, Error> {
        self.retrying(|| {
            let decided = self.decided(slug, &crate::workflow::board_service("reader"), None)?;
            Ok(outputs(&decided))
        })
    }

    /// [`Store::workflow_actions`] and [`Store::accepted_outputs`] from one read, so the outputs
    /// a page sends with an edge are those of the snapshot it previews; refused as
    /// [`Store::workflow_actions`] is.
    pub fn workflow_actions_with_outputs(
        &self,
        slug: &str,
        principal: &Principal,
        host: &Host,
    ) -> Result<(NextActions, Outputs), Error> {
        self.retrying(|| {
            let decided = self.decided(slug, principal, None)?;
            let next = admit::next_actions(context(&decided), &decided.facts, principal, host)
                .map_err(refusal)?;
            Ok((next, outputs(&decided)))
        })
    }

    /// `request` admitted for `principal`, whom the host bound, and written in one transaction
    /// fenced on everything its decision read, or refused, its code first
    /// ([`admit::Refusal`]); capacity under the project's cap is decided in the write's own
    /// counters. Answers the transition it wrote, its effects and the principal's next actions
    /// after it. A request repeating a key the principal used answers that request's result when
    /// it is the same request, and is refused when it is another. Given no attestation, a guarded
    /// edge is refused ([`Store::apply_attested`]).
    pub fn apply_operation(
        &self,
        principal: &Principal,
        host: &Host,
        request: &OperationRequest,
    ) -> Result<OperationResult, Error> {
        self.apply(principal, host, request, &[], None)
    }

    /// [`Store::apply_operation`], each guard of the edge met by one of `attestations`: signed by
    /// the key the active activation commits for its authority, covering exactly this operation
    /// and principal, passing, in its time, and at the authority's generation the board accepted
    /// (`crate::workflow::guard::check`). The write is fenced on those accepted states and records
    /// the admission (`workflow:['admission', SLUG, REVISION]`). A repeated key answers its
    /// first result whatever attestations come with it: the operation is not taken twice.
    pub fn apply_attested(
        &self,
        principal: &Principal,
        host: &Host,
        request: &OperationRequest,
        attestations: &[Signed],
    ) -> Result<OperationResult, Error> {
        self.apply(principal, host, request, attestations, None)
    }

    /// Which guard refuses `request` taken by `principal` with `attestations`, as the board reads
    /// it now, for a page to show beside a refusal [`Store::apply_attested`] gave
    /// ([`admit::guard_refusal`]): none where no single guard does. A read of its own, after the
    /// refusal, which decides nothing and writes nothing.
    pub fn guard_refusal(
        &self,
        principal: &Principal,
        request: &OperationRequest,
        attestations: &[Signed],
    ) -> Result<Option<symphony_api::workflow::GuardRefusal>, Error> {
        self.retrying(|| {
            let decided = self.decided(&request.task, principal, None)?;
            let guarding = guarding(&decided, attestations);
            Ok(admit::guard_refusal(
                context(&decided),
                &decided.facts,
                principal,
                request,
                &guarding,
            ))
        })
    }

    /// [`Store::apply_attested`], or, with what it `answered`, the automatic edge the board
    /// takes on an execution attempt's ending (`Store::observe_attempt`), admitted by the same
    /// checks, with no attestation, and written by the same write.
    fn apply(
        &self,
        principal: &Principal,
        host: &Host,
        request: &OperationRequest,
        attestations: &[Signed],
        answered: Option<&crate::workflow::Recovered>,
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
            let guarding = guarding(&decided, attestations);
            let admitted = match answered {
                None => admit::admit(
                    context,
                    &decided.facts,
                    principal,
                    host,
                    request,
                    &guarding,
                    at,
                ),
                Some(answered) => admit::admit_observed(
                    context,
                    &decided.facts,
                    principal,
                    host,
                    request,
                    answered,
                    at,
                ),
            }
            .map_err(refusal)?;
            // A caller's word never prepares or finalizes: the edge's adapter takes it.
            if let Some(adapter) = crate::workflow::effect::external(&admitted.transition.effects) {
                let by = crate::workflow::effect::taken_by(adapter, slug)
                    .map(|command| format!("; {command} takes it"))
                    .unwrap_or_default();
                return Err(refusal(Refusal::new(
                    RefusalCode::ExplicitActionRequired,
                    format!(
                        "edge {} is the adapter {adapter}'s to take, with what it reads of the \
                         effect itself: no request prepares or finalizes one{by}",
                        request.edge
                    ),
                )));
            }
            let (mut vars, receipt) =
                applied(self, &decided, &admitted, principal, request, &digest, at)?;
            vars["said"] = json!(said(&admitted.transition));
            let guarded = !vars["slots"].as_array().is_none_or(Vec::is_empty);
            let (joins, joins_pg) = joins_said(&vars);
            let logged = surreal::logged("$slug", "$sender", "'system'", "$said", "NONE");
            let logged_pg = pg::logged(
                "v ->> 'slug'",
                "v ->> 'sender'",
                "'system'",
                "v ->> 'said'",
                "NULL",
            );
            let guard = if guarded { sql_cap::GUARD } else { "" };
            let body = both(
                format!("{}{logged}{joins}{guard}", sql::APPLY),
                pg::workflow::apply(&format!("{logged_pg}{joins_pg}"), guarded),
            )
            .posting();
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
        crate::objects::read(&body)
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
            accepted: None,
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
        let read: Read =
            self.read_together(both(sql::ADMISSION, pg::workflow::admission()), vars)?;
        let active = read.active.ok_or_else(|| {
            refusal(Refusal::new(
                RefusalCode::UnsupportedActivation,
                "no custom workflow is active on this board: the board's own writers keep its tasks",
            ))
        })?;
        let task = read.task.ok_or_else(|| {
            refusal(Refusal::new(
                RefusalCode::UnknownId,
                // A workflow's refusal, by its code, of the slug as the request named it: not
                // the board's own refusal of a slug no task has (`work::no_task`).
                format!("no task {slug}"),
            ))
        })?;
        let recorded = self.recorded(&active)?;
        let (roles, lapsed, lapsed_seen) = self.lapsing(read.roles)?;
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
                .map(|d| {
                    let found = (read.found.iter()).find(|f| f.slug == *d);
                    Prerequisite {
                        slug: d.clone(),
                        found: found.map(|f| (f.status.clone(), f.position.clone())),
                        accepted: found.and_then(|f| f.accepted.clone()),
                    }
                })
                .collect(),
            held,
            present: read.present,
            accepted: task.accepted.clone(),
        };
        Ok(Decided {
            active,
            recorded,
            facts,
            task,
            found: read.found,
            receipt: read.receipt,
            lapsed,
            lapsed_seen,
            project: read.project,
            attesters: read.attesters,
            accepted: read.accepted,
        })
    }

    /// `roles` as admission reads them, and the reservations among them that lapsed, as the
    /// board's own claim lapses them: past their expiry, or held by a session lost or gone, each
    /// such session fenced at the beat it was read at. Sessions are read only where a
    /// reservation could have lapsed by its holder.
    #[expect(
        clippy::type_complexity,
        reason = "one read's three parts, each used once"
    )]
    fn lapsing(
        &self,
        roles: Vec<RoleRead>,
    ) -> Result<(Vec<(String, String)>, Vec<(String, String)>, Vec<Value>), Failure> {
        let now = self.now();
        let reserving = roles.iter().any(|r| r.role == "reserved");
        let sessions = if reserving {
            self.sessions().map_err(Failure::Stop)?
        } else {
            Vec::new()
        };
        let (mut kept, mut lapsed, mut seen) = (Vec::new(), Vec::new(), Vec::new());
        for role in roles {
            let pair = (role.handle, role.role);
            if pair.1 != "reserved" {
                kept.push(pair);
                continue;
            }
            let session = sessions.iter().find(|s| s.row.handle == pair.0);
            let (drop, fence) = super::lapsed(role.expires, session.map(|s| s.health), now);
            if drop {
                if fence && let Some(session) = session {
                    seen.push(json!({"handle": pair.0, "last_seen": session.row.last_seen}));
                }
                lapsed.push(pair);
            } else {
                kept.push(pair);
            }
        }
        Ok((kept, lapsed, seen))
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
    /// slot, or moves the task to a position whose slot policy holds one from a position that
    /// holds none, is counted as a start would be, and refused as `capacity` past the cap. The
    /// ledger counts the task at its destination by that position's policy
    /// ([`crate::slots::Entry::slot`]), never by the status it projects to.
    fn taken(
        &self,
        decided: &Decided,
        taking: Taking<'_>,
        principal: &Principal,
    ) -> Result<Vec<Value>, Failure> {
        let status = taking.status;
        let starts = taking.holds && !taking.held;
        let Some(cap) = self.slots() else {
            return Ok(Vec::new());
        };
        if !(taking.acquires || starts) {
            return Ok(Vec::new());
        }
        let slug = &decided.task.slug;
        let view = self.view(&Which::Starting(slug))?;
        let adds = (taking.add.iter())
            .filter(|(_, role)| role == "executor")
            .map(|(handle, _)| handle.clone());
        let by = executors_or(adds.chain(view.executors()), agent(principal));
        let opens = Opens {
            status: Some(status.to_string()),
            by,
            what: taking.what,
            slot: Some(taking.holds),
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
/// whether the task's position holds one before it (`held`) and its destination after it
/// (`holds`), the roles it adds, and what a refusal says cannot happen.
struct Taking<'a> {
    status: &'a str,
    acquires: bool,
    held: bool,
    holds: bool,
    add: &'a [(String, String)],
    what: String,
}

impl Decided {
    /// Whether the position at `at` holds a slot by its slot policy, whatever status it projects
    /// to ([`crate::slots::Entry::slot`]).
    fn holds_at(&self, at: &symphony_api::workflow::Pair) -> bool {
        (self.recorded.definition.positions.iter())
            .find(|p| p.at == *at)
            .is_some_and(|p| p.slot != symphony_api::workflow::Slot::Release)
    }

    /// Whether the task's position as read holds a slot.
    fn holds_now(&self) -> bool {
        (self.task.position.as_ref()).is_some_and(|p| self.holds_at(&p.at()))
    }
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

/// The task's accepted outputs as `decided` reads them ([`Outputs`]): its own standing, and,
/// where an edge leaving its pair depends on `accepted_output`, the one standing for each task it
/// depends on.
fn outputs(decided: &Decided) -> Outputs {
    let context = context(decided);
    let at = decided.facts.position.as_ref().map(TaskPosition::at);
    let own = Prerequisite {
        slug: decided.facts.slug.clone(),
        found: Some((decided.task.status.clone(), decided.facts.position.clone())),
        accepted: decided.facts.accepted.clone(),
    };
    let takes = (context.definition.edges.iter()).any(|e| {
        Some(&e.from) == at.as_ref()
            && e.dependencies == symphony_api::workflow::Dependency::AcceptedOutput
    });
    let standing = |p: &Prerequisite| admit::standing(context, p).map(str::to_string);
    Outputs {
        own: standing(&own),
        takes: if takes {
            (decided.facts.dependencies.iter())
                .map(|p| (p.slug.clone(), standing(p)))
                .collect()
        } else {
            Vec::new()
        },
    }
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

/// The transition's line on its task's thread, naming the attempt whose failure the board took it
/// on, for an automatic edge.
fn said(transition: &Transition) -> String {
    let named = |p: &symphony_api::workflow::Pair| format!("{}/{}", p.phase, p.state);
    let mut line = format!(
        "took {} ({}): {} → {}, workflow revision {}",
        transition.edge.as_deref().unwrap_or("an edge"),
        transition.id,
        transition.from.as_ref().map_or_else(String::new, named),
        named(&transition.to),
        transition.revision
    );
    // The board's take answering an attempt records what it recovered, keyed by the attempt.
    let key = (transition.recovery.as_ref()).and(transition.key.as_deref());
    if let Some(attempt) = key.and_then(|k| k.strip_prefix("attempt/")) {
        line.push_str(&format!(", on the failure of attempt {attempt}"));
    }
    line
}

/// The variables of `sql::APPLY` for `admitted`, and its receipt: one builder for every write
/// that takes an edge ([`Store::apply_operation`], and an external effect's prepare and finalize):
/// the receipt under the request's key, the roles relied on, added and dropped (with the
/// reservations that lapsed, which the decision no longer counts), the dependencies and the slot
/// counters fenced, and each role the edge gives its taker, said as a join ([`joins_said`]).
fn applied(
    store: &Store,
    decided: &Decided,
    admitted: &admit::Admitted,
    principal: &Principal,
    request: &OperationRequest,
    digest: &str,
    at: f64,
) -> Result<(Value, Receipt), Failure> {
    let slug = request.task.as_str();
    let transition: &Transition = &admitted.transition;
    let taking = Taking {
        status: &transition.status,
        acquires: admitted.acquires,
        held: decided.holds_now(),
        holds: decided.holds_at(&transition.to),
        add: &admitted.add,
        what: format!("{slug} cannot take {}", request.edge),
    };
    let slots = store.taken(decided, taking, principal)?;
    let receipt = Receipt {
        principal: principal.clone(),
        key: request.idempotency_key.clone(),
        slug: slug.to_string(),
        request: digest.to_string(),
        sent: request.clone(),
        transition: transition.id.clone(),
        effects: transition.effects.clone(),
    };
    let row = |(handle, role): &(String, String)| json!({"slug": slug, "handle": handle, "role": role, "since": at});
    let pair = |(handle, role): &(String, String)| json!({"handle": handle, "role": role});
    // Each role the edge gives its taker is said as the board's own join says it, so every reader
    // of who held a task's roles (a fresh reviewer's, a post's recipient, where the time went)
    // reads it from the thread as it reads a join (`Store::ever_held`).
    let joined: Vec<String> = (admitted.add.iter())
        .map(|(handle, role)| format!("{handle} joined as {role}"))
        .collect();
    // Each authority once, however many guards it met: the write is fenced on its state and
    // counts the admission, and a guarded edge's admission is recorded.
    let mut authorities: Vec<Value> = Vec::new();
    for g in &admitted.guards {
        if authorities
            .iter()
            .all(|a| a["adapter"] != g.adapter.as_str())
        {
            authorities.push(json!({"adapter": g.adapter, "epoch": g.epoch,
                "generation": g.generation, "state": g.state}));
        }
    }
    let admission = (!admitted.guards.is_empty()).then(|| Admission {
        slug: slug.to_string(),
        revision: transition.revision,
        transition: transition.id.clone(),
        edge: request.edge.clone(),
        principal: principal.clone(),
        input_revisions: request.input_revisions.clone(),
        guards: admitted.guards.clone(),
        at,
    });
    let vars = json!({
        "slug": slug,
        "version": decided.task.version,
        "generation": decided.active.generation,
        "digest": decided.active.digest,
        "dependencies": fenced(decided, request),
        "holding": relied_on(&decided.facts, principal, request, context(decided))
            .iter()
            .map(pair)
            .collect::<Vec<_>>(),
        "add": admitted.add.iter().map(row).collect::<Vec<_>>(),
        "drop": (admitted.drop.iter())
            .chain(decided.lapsed.iter().filter(|l| !admitted.drop.contains(l)))
            .map(pair)
            .collect::<Vec<_>>(),
        "receipt": receipt_id(principal, &request.idempotency_key),
        "receipt_body": receipt,
        "actor": agent(principal),
        "transition": transition,
        "position": transition.position(),
        "moved": transition.status != decided.task.status,
        "sender": sender(principal),
        "now": at,
        "slots": slots,
        "seen": decided.lapsed_seen,
        "joined": joined,
        "authorities": authorities,
        "admission": admission,
    });
    Ok((vars, receipt))
}

/// What admission holds `decided`'s guards to: the project and authorities its activation
/// recorded, the states the board accepted from them, and `given`, the attestations the request
/// carries, on this machine's time, never the store's, which its caller may pin (TASKS_NOW): an
/// attestation's expiry is a bound only on a clock no caller sets.
fn guarding<'a>(decided: &'a Decided, given: &'a [Signed]) -> Guarding<'a> {
    Guarding {
        project: decided.project.as_deref().unwrap_or_default(),
        attesters: &decided.attesters,
        accepted: &decided.accepted,
        given,
        now: super::now(),
    }
}

/// The statements, on each store, that say each role `vars` gives as joined (its `joined`), after
/// the write's own line.
fn joins_said(vars: &Value) -> (String, String) {
    let (mut surreal_lines, mut pg_lines) = (String::new(), String::new());
    for i in 0..vars["joined"].as_array().map_or(0, Vec::len) {
        surreal_lines.push_str(&surreal::logged(
            "$slug",
            "$sender",
            "'system'",
            &format!("$joined[{i}]"),
            "NONE",
        ));
        pg_lines.push('\n');
        pg_lines.push_str(&pg::logged(
            "v ->> 'slug'",
            "v ->> 'sender'",
            "'system'",
            &format!("v -> 'joined' ->> {i}"),
            "NULL",
        ));
    }
    (surreal_lines, pg_lines)
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

mod effects;
pub use effects::{Finalized, Reviewed};
mod observation;
mod operations;
mod recovery;
pub use operations::Settled;
pub(in crate::store) use recovery::Released;

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "admission/accepted_tests.rs"]
mod accepted_tests;

#[cfg(test)]
#[path = "admission/guard_tests.rs"]
mod guard_tests;
