//! The [`Coordinator`]: admission, a caller's controls and the worker's step, each a write the
//! store fences on the row it read. The rules it applies are `rules.rs`'s; collection, cleanup and
//! eligibility are `evidence.rs`'s.

use super::keys;
use super::reader::Reader;
use super::record::{Fact, Identity, Lease, Pending, Relation, Row, State};
use super::rules::{self, Applied, Bounds, Next};
use super::{
    Act, Admission, Attempt, Continuation, Grant, Pacing, Recorded, Refusal, Stepped, Worked,
};
use crate::store::Store;
use crate::store::execution::{Admission as Admitted, Claim, Commit, Decision, Write};
use crate::{Error, stages};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use symphony_plugin::execution::{
    Adapter, AttemptRef, Authorization, Call, ConversationId, ExternalLink, MAX_CALL_MS, Operation,
    OperationIdentity, Outcome, PrincipalKind, Profile, Request, ResumeFrom, ResumeMode, Scope,
    StopMode, invoke,
};

/// The board's execution attempts under one provider configuration: what a host admits, controls
/// and reads through, and the worker that sends each attempt's operations.
///
/// Every call but the worker's takes the caller's [`Grant`]; the worker acts under the grant the
/// host gave it at [`Coordinator::new`]. Nothing a caller sends, a payload's owner or a claimed
/// handle, stands in for a grant.
pub struct Coordinator<'a> {
    store: &'a Store,
    profile: Profile,
    adapter: &'a dyn Adapter,
    worker: Grant,
    /// The name its leases are taken under: one per coordinator, so two never share a lease.
    owner: String,
    pacing: Pacing,
    /// The capacity scope each attempt it admits reserves a place in, and its size.
    capacity: Option<(String, u32)>,
    /// The workflow host whose policy an attempt's ending is proposed under.
    host: crate::workflow::Host,
}

/// An attempt about to be admitted.
struct Entry {
    task: String,
    lane: String,
    role: String,
    handle: Option<String>,
    input: symphony_plugin::execution::Input,
    request_key: Option<String>,
    predecessor: Option<(String, Relation)>,
    decision: Option<String>,
    runtime_epoch: u64,
    /// Its first operation, a start or a resume.
    first: Request,
    /// What was asked, beyond the entry's own fields, that a repeat of its request key must ask
    /// again: a resume's continuation.
    asked: Value,
}

/// The operation a worker's step makes.
enum Step {
    /// The owed operation, sent (or reconciled, when `reconcile`).
    Owed {
        pending: Box<Pending>,
        reconcile: bool,
    },
    Observe,
}

fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl<'a> Coordinator<'a> {
    /// A coordinator for `profile`'s board and configuration on `store`, sending through
    /// `adapter` as `worker`: the grant the host built for its own worker, which must let it
    /// [`Act::Work`] on the whole board.
    pub fn new(
        store: &'a Store,
        profile: Profile,
        adapter: &'a dyn Adapter,
        worker: Grant,
    ) -> Result<Coordinator<'a>, Refusal> {
        if profile.scope.project.is_empty()
            || profile.scope.board.is_empty()
            || profile.configuration.is_empty()
        {
            return Err(Refusal::Invalid(
                "a provider profile names its project, board and configuration".into(),
            ));
        }
        worker.allows(&profile.scope, None, Act::Work)?;
        let owner = format!(
            "{}:{}",
            worker.principal().subject,
            crate::launch::new_uuid()
        );
        Ok(Coordinator {
            store,
            profile,
            adapter,
            worker,
            owner,
            pacing: Pacing::default(),
            capacity: None,
            host: crate::workflow::Host::local(),
        })
    }

    /// The same coordinator, paced by `pacing`: its lease must outlast each call, which must keep
    /// within the kit's bound.
    pub fn with_pacing(mut self, pacing: Pacing) -> Result<Coordinator<'a>, Refusal> {
        let call = ms(pacing.call);
        if call == 0 || call > u64::from(MAX_CALL_MS) || pacing.lease <= pacing.call {
            return Err(Refusal::Invalid(format!(
                "a call takes 1 to {MAX_CALL_MS} ms, within its lease"
            )));
        }
        if pacing.observe.is_zero()
            || pacing.backoff.is_zero()
            || pacing.backoff_cap < pacing.backoff
        {
            return Err(Refusal::Invalid(
                "observation and backoff take time, the cap no less than the first wait".into(),
            ));
        }
        self.pacing = pacing;
        Ok(self)
    }

    /// The same coordinator, each attempt it admits reserving one of `limit` places in capacity
    /// scope `scope` until it is known that nothing of it runs.
    #[must_use]
    pub fn with_capacity(mut self, scope: &str, limit: u32) -> Coordinator<'a> {
        self.capacity = Some((scope.to_string(), limit));
        self
    }

    /// The same coordinator, proposing each attempt's ending to the workflow under `host`'s
    /// policy ([`Store::observe_attempt`]) rather than the local host's
    /// ([`crate::workflow::Host::local`]).
    #[must_use]
    pub fn with_workflow(mut self, host: crate::workflow::Host) -> Coordinator<'a> {
        self.host = host;
        self
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Attempt `id`'s ending proposed to the workflow that holds its task, once one of this
    /// coordinator's own writes ended it: what the board refuses (its task moved since, no action
    /// answers a cancel) stays on the attempt and moves nothing, and the board says a failure
    /// nothing can answer once on its task's thread.
    fn observe(&self, id: &str) -> Result<(), Refusal> {
        match self.store.observe_attempt(id, &self.host) {
            Ok(_) | Err(Error::Refused(_)) => Ok(()),
            Err(error) => Err(Refusal::Board(error)),
        }
    }

    fn bounds(&self) -> Bounds {
        Bounds {
            unknowns: self.pacing.unknowns,
            backoff_ms: ms(self.pacing.backoff),
            backoff_cap_ms: ms(self.pacing.backoff_cap),
            observe_ms: ms(self.pacing.observe),
        }
    }

    /// Admit an attempt of `admission`'s task in its lane, its start owed: once per request key,
    /// one live attempt per lane, and a place in the capacity scope when there is one.
    pub fn admit(&self, grant: &Grant, admission: Admission) -> Result<Attempt, Refusal> {
        grant.allows(&self.profile.scope, Some(&admission.task), Act::Start)?;
        let first = Request::Start {
            input: admission.input.clone(),
        };
        self.enter(
            grant,
            Entry {
                task: admission.task,
                lane: admission.lane,
                role: admission.role,
                handle: admission.handle,
                input: admission.input,
                request_key: admission.request_key,
                predecessor: None,
                decision: None,
                runtime_epoch: 0,
                first,
                asked: Value::Null,
            },
        )
    }

    /// Admit an attempt that resumes `from`, known paused or ended with its resource bound, from
    /// `continuation`: the same task, lane and input at the next runtime epoch, so nothing of the
    /// predecessor's runtime can write for it. The predecessor, if paused, ends superseded.
    pub fn resume(
        &self,
        grant: &Grant,
        from: &str,
        continuation: Continuation,
        request_key: Option<&str>,
    ) -> Result<Attempt, Refusal> {
        let before = self.row(from)?;
        grant.allows(&self.profile.scope, Some(&before.task), Act::Resume)?;
        self.same_configuration(&before)?;
        if continuation.checkpoint.is_none() && continuation.conversation.is_none() {
            return Err(Refusal::Invalid(
                "a resume continues from a checkpoint or a conversation".into(),
            ));
        }
        // A repeat of the resume that followed it is answered by its request key, as admission
        // answers any repeat: its predecessor is no longer resumable, being followed.
        let repeat = match &before.state.successor {
            Some(successor) => self.followed_by(request_key, successor)?,
            None => false,
        };
        let reuses = continuation.mode == ResumeMode::ReuseWithNewGeneration;
        let (binding, quiescent) = match (repeat, &before.state.resource, before.state.quiescent) {
            (true, Some(binding), Some(quiescent)) => (binding.clone(), quiescent),
            _ => rules::resumable(&before.state, reuses).map_err(Refusal::Invalid)?,
        };
        let previous = self.reference(&before)?;
        let mut resume = ResumeFrom::new(
            rules::resource(&before.identity, &previous, &binding),
            format!("{from}#{quiescent}"),
        );
        resume.checkpoint.clone_from(&continuation.checkpoint);
        resume.conversation = continuation.conversation.clone().map(ConversationId::new);
        let first = Request::Resume {
            input: before.identity.input.clone(),
            from: resume,
            mode: continuation.mode,
        };
        if !self.profile.capabilities.supports(&first) {
            return Err(Refusal::Invalid(format!(
                "its provider cannot resume as {:?}",
                continuation.mode
            )));
        }
        let identity = &before.identity;
        let asked = json!({
            "checkpoint": continuation.checkpoint,
            "conversation": continuation.conversation,
            "mode": continuation.mode,
        });
        self.enter(
            grant,
            Entry {
                task: before.task.clone(),
                lane: identity.lane.clone(),
                role: identity.role.clone(),
                handle: identity.handle.clone(),
                input: identity.input.clone(),
                request_key: request_key.map(str::to_string),
                predecessor: Some((from.to_string(), Relation::Resume)),
                decision: None,
                runtime_epoch: identity.runtime_epoch + 1,
                first,
                asked,
            },
        )
    }

    /// Admit a new execution of `from`'s input after it was held or ended: a person's decision
    /// alone, whose `decision` is the evidence that `from`'s runtime was found ended or isolated.
    /// A held predecessor ends superseded in the same transaction, its place handed to the retry.
    pub fn retry(
        &self,
        grant: &Grant,
        from: &str,
        decision: &str,
        request_key: Option<&str>,
    ) -> Result<Attempt, Refusal> {
        let before = self.row(from)?;
        grant.allows(&self.profile.scope, Some(&before.task), Act::Retry)?;
        if grant.principal().kind != PrincipalKind::Human {
            return Err(Refusal::Unauthorized(
                "a retry is a person's decision".into(),
            ));
        }
        if decision.trim().is_empty() {
            return Err(Refusal::Invalid(
                "a retry names the evidence its predecessor was found or isolated".into(),
            ));
        }
        use super::record::Dispatch;
        if !matches!(before.state.dispatch, Dispatch::Held | Dispatch::Terminal) {
            return Err(Refusal::Invalid(
                "it is neither held nor ended: cancel it, or let recovery settle it".into(),
            ));
        }
        // A repeat of the retry that followed it is answered by its request key at admission.
        if let Some(successor) = &before.state.successor
            && !self.followed_by(request_key, successor)?
        {
            return Err(Refusal::Invalid(format!(
                "attempt {successor} followed it already"
            )));
        }
        let identity = &before.identity;
        self.enter(
            grant,
            Entry {
                task: before.task.clone(),
                lane: identity.lane.clone(),
                role: identity.role.clone(),
                handle: identity.handle.clone(),
                input: identity.input.clone(),
                request_key: request_key.map(str::to_string),
                predecessor: Some((from.to_string(), Relation::Retry)),
                decision: Some(decision.to_string()),
                runtime_epoch: identity.runtime_epoch + 1,
                first: Request::Start {
                    input: identity.input.clone(),
                },
                asked: Value::Null,
            },
        )
    }

    /// The attempt `entry` describes, written as one transaction with its first facts, or the
    /// attempt its request key admitted before.
    fn enter(&self, grant: &Grant, entry: Entry) -> Result<Attempt, Refusal> {
        let input = &entry.input;
        let blank = [
            &entry.task,
            &entry.lane,
            &entry.role,
            &input.task_revision,
            &input.digest,
            &input.repository_revision,
            &input.manifest.reference,
            &input.manifest.digest,
        ];
        if blank.iter().any(|field| field.trim().is_empty())
            || entry.request_key.as_ref().is_some_and(|key| key.is_empty())
        {
            return Err(Refusal::Invalid(
                "an attempt names its task, lane, role and pinned input".into(),
            ));
        }
        let task = self.store.get(&entry.task).map_err(|error| match error {
            Error::Refused(_) => Refusal::Invalid(format!("there is no task {}", entry.task)),
            error => Refusal::Board(error),
        })?;
        if stages::CLOSED.contains(&task.status.as_str()) {
            return Err(Refusal::Invalid(format!(
                "{} is {}: it takes no attempt",
                entry.task, task.status
            )));
        }
        // Where the workflow holds the task now, its pair and revision alone: the position the
        // attempt's ending is proposed from, and only while the task stands there.
        let workflow = (task.fields.get("_board"))
            .and_then(|board| board.get("workflow"))
            .filter(|position| !position.is_null())
            .map(|p| json!({"phase": p["phase"], "state": p["state"], "revision": p["revision"]}));
        let scope = &self.profile.scope;
        let principal = grant.principal().clone();
        let digest = keys::digest(&json!({
            "task": entry.task, "lane": entry.lane, "role": entry.role, "handle": entry.handle,
            "input": entry.input, "predecessor": entry.predecessor, "decision": entry.decision,
            "configuration": self.profile.configuration, "initiator": principal,
            "operation": entry.first.operation(), "asked": entry.asked,
        }));
        let (_, hex) = keys::new_attempt();
        let kind = entry.first.operation();
        let reuses = matches!(
            entry.first,
            Request::Resume {
                mode: ResumeMode::ReuseWithNewGeneration,
                ..
            }
        );
        let key = keys::operation(
            &scope.project,
            &scope.board,
            &self.profile.configuration,
            &hex,
            kind,
            1,
        );
        let pending = Pending {
            sequence: 1,
            kind,
            key: key.clone(),
            digest: keys::digest(&entry.first),
            request: entry.first,
            initiator: principal.clone(),
            sent: false,
            first_sent: None,
            cancels: false,
        };
        let capacity = self.capacity.as_ref().map(|(scope, _)| scope.clone());
        let (predecessor, relation) = entry.predecessor.clone().unzip();
        let identity = Identity {
            project: scope.project.clone(),
            board: scope.board.clone(),
            task: entry.task.clone(),
            lane: entry.lane.clone(),
            role: entry.role,
            handle: entry.handle,
            predecessor: predecessor.clone(),
            relation,
            decision: entry.decision,
            input: entry.input,
            configuration: self.profile.configuration.clone(),
            contract: self.profile.contract,
            capabilities: self.profile.capabilities.clone(),
            initiator: principal.clone(),
            policy_revision: grant.policy_revision().to_string(),
            request_key: entry.request_key.clone(),
            runtime_epoch: entry.runtime_epoch,
            capacity: capacity.clone(),
        };
        let facts = [
            Fact::Admitted {
                initiator: principal.clone(),
                request_key: entry.request_key.clone(),
            },
            Fact::Intent {
                sequence: 1,
                operation: kind,
                key,
                digest: pending.digest.clone(),
                initiator: principal,
            },
        ];
        let state = State {
            sequence: 1,
            pending: Some(pending),
            reserved: capacity,
            ..State::default()
        };
        let vars = json!({
            "attempt": hex,
            "task": entry.task,
            "lane": entry.lane,
            "configuration": self.profile.configuration,
            "row": {"identity": identity, "state": state, "workflow": workflow},
            "facts": facts.iter().zip(1..).map(|(fact, seq)| json!({"seq": seq, "fact": fact})).collect::<Vec<_>>(),
            "request": entry.request_key,
            "digest": digest,
            "predecessor": predecessor,
            "capacity": self.capacity.as_ref().map(|(scope, limit)| json!({"scope": scope, "limit": limit})),
        });
        let follow = |row: &Value| followed(row, &hex, relation.unwrap_or(Relation::Retry), reuses);
        match self
            .store
            .execution_admit(vars, predecessor.as_deref(), follow)?
        {
            Admitted::Admitted => self.view(&hex),
            Admitted::Existing {
                attempt,
                digest: known,
            } => {
                if known.as_deref() == Some(digest.as_str()) {
                    self.view(&attempt)
                } else {
                    Err(Refusal::Conflict(format!(
                        "its request key admitted another attempt, {attempt}"
                    )))
                }
            }
            Admitted::Busy { attempt } => Err(Refusal::Busy(attempt)),
            Admitted::Full { reserved } => Err(Refusal::Full(reserved)),
        }
    }

    /// Whether `successor` is the attempt `request_key` admitted: a repeat of the retry or the
    /// resume that followed its predecessor.
    fn followed_by(&self, request_key: Option<&str>, successor: &str) -> Result<bool, Refusal> {
        let Some(key) = request_key else {
            return Ok(false);
        };
        let fence = self.store.execution_fence(json!(["request", key]))?;
        Ok(fence.is_some_and(|fence| fence["attempt"] == successor))
    }

    /// Ask attempt `id` to cease. From the moment it is written no result of it is eligible; an
    /// attempt whose start never reached the provider ends cancelled at once, with nothing
    /// provisioned. Asking again changes nothing.
    pub fn cancel(&self, grant: &Grant, id: &str) -> Result<Attempt, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile.scope, Some(&row.task), Act::Cancel)?;
        let (committed, ended) = self.store.execution_update(id, |value| {
            let row = Row::of(value)?;
            let first = u64::try_from(row.facts + 1).unwrap_or(0);
            Ok(match rules::cancel(&row.state, grant.principal(), first) {
                None => Decision::Nothing(false),
                Some(applied) => {
                    let ended = !applied.state.live();
                    Decision::write(written(&applied, None, false), ended)
                }
            })
        })?;
        // A cancel of an attempt that had ended, or that nothing was sent for, is known to have
        // taken at once: acknowledged where the workflow holds its task.
        if ended && matches!(committed, Some(Commit::Committed { .. })) {
            self.observe(id)?;
        }
        self.view(id)
    }

    /// Ask attempt `id`'s bound resource to stop in `mode`, which its provider must offer: never a
    /// held attempt's, which waits for a person.
    pub fn stop(&self, grant: &Grant, id: &str, mode: StopMode) -> Result<Attempt, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile.scope, Some(&row.task), Act::Stop)?;
        let (_, refused) = self.store.execution_update(id, |value| {
            let row = Row::of(value)?;
            Ok(
                match rules::stop(&row.identity, &row.state, grant.principal(), mode) {
                    Err(why) => Decision::Nothing(Some(why)),
                    Ok(applied) => Decision::write(written(&applied, None, false), None),
                },
            )
        })?;
        match refused {
            Some(why) => Err(Refusal::Invalid(why)),
            None => self.view(id),
        }
    }

    /// A view of attempt `id`'s resource, resolved now for `grant`'s principal and never kept.
    pub fn external_link(&self, grant: &Grant, id: &str) -> Result<ExternalLink, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile.scope, Some(&row.task), Act::Link)?;
        self.same_configuration(&row)?;
        let Some(binding) = &row.state.resource else {
            return Err(Refusal::Invalid("no resource is bound to it".into()));
        };
        let attempt = self.reference(&row)?;
        let request = Request::ExternalLink {
            resource: rules::resource(&row.identity, &attempt, binding),
            conversation: row.state.conversation.clone().map(ConversationId::new),
        };
        let sequence = row.state.sequence;
        let call = Call::new(
            self.operation(&row, &attempt, Operation::ExternalLink, sequence, None)
                .with_epochs(row.epoch, row.identity.runtime_epoch),
            grant.principal().clone(),
            request,
        );
        let authorization = Authorization::from_host(
            grant.principal().clone(),
            attempt,
            Operation::ExternalLink,
            grant.policy_revision().to_string(),
        );
        let deadline = Instant::now() + self.pacing.call;
        match invoke(
            self.adapter,
            &pinned(&row.identity),
            &authorization,
            &call,
            deadline,
        ) {
            Outcome::Link(link) => Ok(link),
            Outcome::Unsupported(_) => Err(Refusal::Invalid("its provider gives no view".into())),
            other => Err(Refusal::Unavailable(format!("{other:?}"))),
        }
    }

    /// Attempt `id`.
    pub fn attempt(&self, grant: &Grant, id: &str) -> Result<Attempt, Refusal> {
        self.reader().attempt(grant, id)
    }

    /// Every attempt of `task`, oldest first: its history, each retry and resume linked to the
    /// attempt before it.
    pub fn history(&self, grant: &Grant, task: &str) -> Result<Vec<Attempt>, Refusal> {
        self.reader().history(grant, task)
    }

    /// Attempt `id`'s facts, in the order they were appended.
    pub fn facts(&self, grant: &Grant, id: &str) -> Result<Vec<Recorded>, Refusal> {
        self.reader().facts(grant, id)
    }

    /// The attempts whose work is due, up to `limit`, each stepped: what the host's worker loop
    /// runs, and where an attempt whose worker died is taken up once its lease runs out.
    pub fn run_due(&self, limit: usize) -> Result<Vec<Stepped>, Refusal> {
        let due = self
            .store
            .execution_due(&self.profile.configuration, limit)?;
        Ok(due
            .into_iter()
            .map(|id| {
                let worked = self.step(&id);
                (id, worked)
            })
            .collect())
    }

    /// One step of the worker for attempt `id`, under a lease the database's clock times: what it
    /// owes recorded as sent before it is sent, the answer written only while the lease holds.
    pub fn step(&self, id: &str) -> Result<Worked, Refusal> {
        // Another board's or configuration's attempt is refused before its lease is touched.
        self.same_configuration(&self.row(id)?)?;
        let lease_ms = ms(self.pacing.lease);
        let (value, now) = match self.store.execution_claim(id, &self.owner, lease_ms)? {
            Claim::Missing => return Err(Refusal::Unknown(id.to_string())),
            Claim::Held { .. } => return Ok(Worked::Leased),
            Claim::Claimed { row, now } => (row, now),
        };
        let claimed = Row::of(&value)?;
        self.same_configuration(&claimed)?;
        let Some(lease) = claimed.lease.clone() else {
            return Err(Refusal::Board(Error::Db(
                "a claimed execution attempt holds no lease".into(),
            )));
        };
        let fence = json!({"owner": lease.owner, "epoch": lease.epoch});
        let worker = self.worker.principal().clone();
        let (committed, plan) = self.store.execution_update(id, |value| {
            let row = Row::of(value)?;
            let attempt = self.reference(&row).map_err(refused)?;
            let plan = rules::next(&row.identity, &attempt, &row.attempt, &row.state, now);
            let decision = match &plan {
                Next::Wait => {
                    let applied = Applied::from_state(row.state.clone());
                    Decision::write(written(&applied, Some(&fence), true), plan)
                }
                Next::Hold(why) => {
                    let applied = rules::hold(&row.state, why.clone());
                    Decision::write(written(&applied, Some(&fence), true), plan)
                }
                Next::Send { pending, new } => {
                    let mut applied = Applied::from_state(row.state.clone());
                    let mut sent = pending.clone();
                    sent.sent = true;
                    sent.first_sent = sent.first_sent.or(Some(now));
                    if *new {
                        applied.state.sequence = pending.sequence;
                        applied.facts.push(Fact::Intent {
                            sequence: pending.sequence,
                            operation: pending.kind,
                            key: pending.key.clone(),
                            digest: pending.digest.clone(),
                            initiator: pending.initiator.clone(),
                        });
                    }
                    applied.state.pending = Some(sent);
                    applied.facts.push(Fact::Sent {
                        sequence: pending.sequence,
                        worker: worker.clone(),
                        epoch: lease.epoch,
                        reconcile: false,
                    });
                    // Due again when the lease runs out, should this worker die before it answers.
                    applied.due_in = Some(lease_ms);
                    Decision::write(written(&applied, Some(&fence), false), plan)
                }
                Next::Reconcile(pending) => {
                    let mut applied = Applied::from_state(row.state.clone());
                    applied.facts.push(Fact::Sent {
                        sequence: pending.sequence,
                        worker: worker.clone(),
                        epoch: lease.epoch,
                        reconcile: true,
                    });
                    applied.due_in = Some(lease_ms);
                    Decision::write(written(&applied, Some(&fence), false), plan)
                }
                Next::Observe(_) => Decision::Nothing(plan),
            };
            Ok(decision)
        })?;
        match committed {
            Some(Commit::Fenced { .. }) => return Ok(Worked::Leased),
            Some(Commit::Missing) => return Err(Refusal::Unknown(id.to_string())),
            Some(Commit::Committed { .. }) | None => {}
        }
        let step = match plan {
            Next::Wait => return Ok(Worked::Waited),
            Next::Hold(why) => return Ok(Worked::Held(why)),
            Next::Send { pending, .. } => Step::Owed {
                pending: Box::new(pending),
                reconcile: false,
            },
            Next::Reconcile(pending) => Step::Owed {
                pending: Box::new(pending),
                reconcile: true,
            },
            Next::Observe(_) => Step::Observe,
        };
        self.call(id, &claimed, &lease, &fence, step)
    }

    /// Make `step`'s call for attempt `id` (as `claimed` read it, under `lease`) and record its
    /// answer.
    fn call(
        &self,
        id: &str,
        claimed: &Row,
        lease: &Lease,
        fence: &Value,
        step: Step,
    ) -> Result<Worked, Refusal> {
        let identity = &claimed.identity;
        let attempt = self.reference(claimed)?;
        let bound = |state: &State| {
            state
                .resource
                .as_ref()
                .map(|binding| rules::resource(identity, &attempt, binding))
        };
        let worker = self.worker.principal().clone();
        let (operation, request, initiator, sent, reconcile) = match step {
            Step::Owed { pending, reconcile } => {
                let operation = self.operation(
                    claimed,
                    &attempt,
                    pending.kind,
                    pending.sequence,
                    Some(&pending.key),
                );
                let request = match (&pending.request, reconcile) {
                    (request, false) => request.clone(),
                    (Request::Resume { from, mode, .. }, true) => Request::ReconcileResume {
                        resource: bound(&claimed.state),
                        from: from.clone(),
                        mode: *mode,
                    },
                    (_, true) => Request::Reconcile {
                        resource: bound(&claimed.state),
                    },
                };
                (
                    operation,
                    request,
                    pending.initiator.clone(),
                    Some(*pending),
                    reconcile,
                )
            }
            Step::Observe => {
                let Some(resource) = bound(&claimed.state) else {
                    return Ok(Worked::Waited);
                };
                let sequence = claimed.state.sequence;
                let operation =
                    self.operation(claimed, &attempt, Operation::Observe, sequence, None);
                let request = Request::Observe {
                    resource,
                    cursor: claimed.state.cursor.clone(),
                };
                (operation, request, worker.clone(), None, false)
            }
        };
        let kind = request.operation();
        let call = Call::new(
            operation.with_epochs(lease.epoch, identity.runtime_epoch),
            initiator,
            request,
        );
        let authorization = Authorization::from_host(
            worker.clone(),
            attempt,
            kind,
            self.worker.policy_revision().to_string(),
        );
        let deadline = Instant::now() + self.pacing.call;
        let outcome = invoke(
            self.adapter,
            &pinned(identity),
            &authorization,
            &call,
            deadline,
        );
        self.answer(id, lease, fence, sent.as_ref(), reconcile, kind, outcome)
    }

    /// The answer `outcome` to `sent` (or to an observation, with none) recorded for attempt `id`
    /// while `lease` holds; once it does not, kept as a stale fact that changes nothing.
    #[allow(clippy::too_many_arguments)]
    fn answer(
        &self,
        id: &str,
        lease: &Lease,
        fence: &Value,
        sent: Option<&Pending>,
        reconcile: bool,
        operation: Operation,
        outcome: Outcome,
    ) -> Result<Worked, Refusal> {
        let bounds = self.bounds();
        let worker = self.worker.principal().clone();
        let fact = |stale: bool| Fact::Answer {
            sequence: sent.map(|pending| pending.sequence),
            operation,
            outcome: Box::new(outcome.clone()),
            worker: worker.clone(),
            epoch: lease.epoch,
            stale,
        };
        let (committed, ended) = self.store.execution_update(id, |value| {
            let row = Row::of(value)?;
            let answer = u64::try_from(row.facts + 1).unwrap_or(0);
            // Still the operation it owes: a reply to any other is no longer its to act on.
            let owed = match sent {
                None => Some(None),
                Some(sent) => row
                    .state
                    .pending
                    .as_ref()
                    .filter(|owed| owed.sequence == sent.sequence && owed.kind == sent.kind)
                    .map(Some),
            };
            let Some(owed) = owed else {
                let mut applied = Applied::from_state(row.state.clone());
                applied.facts.push(fact(true));
                applied.due_in = Some(0);
                return Ok(Decision::write(written(&applied, Some(fence), true), false));
            };
            let applied = rules::apply(
                &row.identity,
                &row.state,
                owed,
                reconcile,
                &outcome,
                answer,
                &bounds,
            );
            let ended = row.state.live() && !applied.state.live();
            let mut recorded = Applied::from_state(applied.state.clone());
            recorded.facts = [vec![fact(false)], applied.facts].concat();
            recorded.release = applied.release;
            recorded.due_in = applied.due_in;
            Ok(Decision::write(
                written(&recorded, Some(fence), true),
                ended,
            ))
        })?;
        match committed {
            Some(Commit::Committed { .. }) | None => {
                // The answer that ended it, written under the worker's lease: proposed to the
                // workflow, which moves its task only while the task stands where it was.
                if ended {
                    self.observe(id)?;
                }
                Ok(Worked::Answered { operation, outcome })
            }
            Some(Commit::Fenced { .. }) => {
                self.store.execution_update(id, |_| {
                    let write = Write {
                        facts: vec![to_value(&fact(true))],
                        ..Write::default()
                    };
                    Ok(Decision::write(write, ()))
                })?;
                Ok(Worked::Fenced { operation, outcome })
            }
            Some(Commit::Missing) => Err(Refusal::Unknown(id.to_string())),
        }
    }

    /// The identity of operation `sequence` (of kind `kind`) of `row`, its key the one owed or,
    /// for a call never owed (an observation, a link), the one its kind and sequence give.
    fn operation(
        &self,
        row: &Row,
        attempt: &AttemptRef,
        kind: Operation,
        sequence: u64,
        key: Option<&str>,
    ) -> OperationIdentity {
        let identity = &row.identity;
        let key = key.map_or_else(
            || {
                keys::operation(
                    &identity.project,
                    &identity.board,
                    &identity.configuration,
                    &row.attempt,
                    kind,
                    sequence,
                )
            },
            str::to_string,
        );
        OperationIdentity::new(
            attempt.clone(),
            identity.configuration.clone(),
            identity.input.digest.clone(),
            key,
            kind,
            sequence,
        )
    }

    /// Its board's attempts, read with no provider.
    fn reader(&self) -> Reader<'a> {
        Reader::new(self.store, self.profile.scope.clone())
    }

    /// Attempt `id`'s row, if it is this board's.
    pub(super) fn row(&self, id: &str) -> Result<Row, Refusal> {
        self.reader().row(id)
    }

    fn same_board(&self, row: &Row) -> Result<(), Refusal> {
        self.reader().same_board(row)
    }

    /// Refused unless `row` runs under this coordinator's configuration: a resource means
    /// something only within its own.
    fn same_configuration(&self, row: &Row) -> Result<(), Refusal> {
        self.same_board(row)?;
        if row.identity.configuration != self.profile.configuration {
            return Err(Refusal::Scope);
        }
        Ok(())
    }

    /// `row`'s attempt as the kit names it.
    pub(super) fn reference(&self, row: &Row) -> Result<AttemptRef, Refusal> {
        let id = keys::parse(&row.attempt).ok_or_else(|| Refusal::Unknown(row.attempt.clone()))?;
        Ok(AttemptRef::new(
            Scope::new(row.identity.project.clone(), row.identity.board.clone()),
            row.task.clone(),
            id,
        ))
    }

    /// Attempt `id` as a reader sees it.
    pub(super) fn view(&self, id: &str) -> Result<Attempt, Refusal> {
        let reader = self.reader();
        let row = reader.row(id)?;
        reader.view_of(row)
    }

    pub(super) fn store(&self) -> &Store {
        self.store
    }
}

/// The profile `identity` pinned when it was admitted: every call of the attempt is judged by the
/// contract and capabilities it was admitted under.
fn pinned(identity: &Identity) -> Profile {
    let mut profile = Profile::new(
        Scope::new(identity.project.clone(), identity.board.clone()),
        identity.configuration.clone(),
        identity.capabilities.clone(),
    );
    profile.contract = identity.contract;
    profile
}

pub(super) fn to_value(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("an execution record serializes")
}

/// The write that has the attempt in `row` followed by `by`, as [`rules::followed`] decides,
/// made in `by`'s admission ([`Store::execution_admit`]); none once it was followed.
fn followed(
    row: &Value,
    by: &str,
    relation: Relation,
    reuses: bool,
) -> Result<Option<Write>, Error> {
    let row = Row::of(row)?;
    let applied = rules::followed(&row.state, by, relation, reuses).map_err(Error::Refused)?;
    Ok(applied.map(|applied| written(&applied, None, false)))
}

/// `applied` as a write, under the lease `fence` when the worker writes it.
pub(super) fn written(applied: &Applied, fence: Option<&Value>, release_lease: bool) -> Write {
    let state = &applied.state;
    Write {
        lease: fence.cloned(),
        state: Some((to_value(state), state.open(), state.live(), applied.due_in)),
        release_lease,
        facts: applied.facts.iter().map(to_value).collect(),
        release: applied.release.clone(),
        artifacts: Vec::new(),
    }
}

/// A refusal carried out of a store write as the board's error.
fn refused(refusal: Refusal) -> Error {
    match refusal {
        Refusal::Board(error) => error,
        other => Error::Refused(other.to_string()),
    }
}
