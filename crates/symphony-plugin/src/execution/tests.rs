use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

struct Fixture {
    profile: Profile,
    call: Call,
}

impl Fixture {
    fn new() -> Self {
        let scope = Scope::new("project-a".into(), "board-a".into());
        let attempt = AttemptRef::new(scope.clone(), "tools/task".into(), AttemptId::new([1; 16]));
        let identity = OperationIdentity::new(
            attempt,
            "account/environment".into(),
            "sha256:input".into(),
            "start-1".into(),
            Operation::Start,
            1,
        )
        .with_epochs(7, 3);
        let capabilities = Capabilities {
            operations: vec![
                Operation::Start,
                Operation::Reconcile,
                Operation::Observe,
                Operation::Cancel,
                Operation::Stop,
                Operation::Resume,
                Operation::ExternalLink,
            ],
            stop_modes: vec![StopMode::Pause, StopMode::Terminate, StopMode::HardStop],
            resume_modes: vec![ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration],
            idempotency_retention_ms: Some(60_000),
            external_view: true,
            ..Capabilities::default()
        };
        let input = Input::new(
            "revision-1".into(),
            identity.input_digest.clone(),
            "git:commit".into(),
            ContentRef::new("manifest:1".into(), "sha256:manifest".into()),
        );
        Self {
            profile: Profile::new(scope, identity.configuration.clone(), capabilities),
            call: Call::new(
                identity,
                Principal::new(PrincipalKind::Agent, "host".into(), "executor".into()),
                Request::Start { input },
            ),
        }
    }

    fn authorization(&self) -> Authorization {
        Authorization::from_host(
            self.call.initiator.clone(),
            self.call.identity.attempt.clone(),
            self.call.request.operation(),
            "policy-1".into(),
        )
    }

    fn resource(&self) -> Resource {
        Resource::new(
            self.call.identity.attempt.clone(),
            self.call.identity.configuration.clone(),
            ResourceId::new("resource-a".into()),
            "generation-1".into(),
        )
    }

    fn receipt(&self) -> Receipt {
        Receipt::new(
            self.call.identity.clone(),
            self.resource(),
            "provider-receipt".into(),
        )
    }

    fn invoke(&self, adapter: &dyn Adapter) -> Outcome {
        // Injected clock avoids a correctness assertion on a loaded machine's elapsed time.
        let now = Instant::now();
        invoke_with_clock(
            adapter,
            &self.profile,
            &self.authorization(),
            &self.call,
            now + Duration::from_secs(1),
            || now,
        )
    }

    fn set_request(&mut self, request: Request) {
        self.call.identity.kind = request.operation();
        self.call.request = request;
    }
}

#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    jobs: AtomicUsize,
    starts: Mutex<HashMap<ProviderKey, (Input, Receipt)>>,
    lose_response: AtomicBool,
    reply: Mutex<Option<Outcome>>,
    deadlines: Mutex<Vec<Instant>>,
}

/// Retry identity excludes worker/runtime epochs, which can change during recovery, and the
/// input digest so that reusing the same operation key with different input is a conflict.
#[derive(PartialEq, Eq, Hash)]
struct ProviderKey {
    attempt: AttemptRef,
    configuration: String,
    kind: Operation,
    sequence: u64,
    key: String,
}

impl ProviderKey {
    fn of(identity: &OperationIdentity) -> Self {
        Self {
            attempt: identity.attempt.clone(),
            configuration: identity.configuration.clone(),
            kind: identity.kind,
            sequence: identity.sequence,
            key: identity.key.clone(),
        }
    }
}

impl Adapter for Fake {
    fn execute(&self, call: &Call, auth: &Authorization, deadline: Instant) -> Outcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.deadlines.lock().unwrap().push(deadline);
        assert!(auth.permits(call));
        if let Some(reply) = self.reply.lock().unwrap().clone() {
            return reply;
        }
        let resource = || {
            Resource::new(
                call.identity.attempt.clone(),
                call.identity.configuration.clone(),
                ResourceId::new("resource-a".into()),
                "generation-1".into(),
            )
        };
        let receipt =
            |resource| Receipt::new(call.identity.clone(), resource, "provider-receipt".into());
        match &call.request {
            Request::Start { input } => {
                let mut starts = self.starts.lock().unwrap();
                let entry = starts
                    .entry(ProviderKey::of(&call.identity))
                    .or_insert_with(|| {
                        let number = self.jobs.fetch_add(1, Ordering::SeqCst) + 1;
                        let mut bound = resource();
                        if number > 1 {
                            bound.id = ResourceId::new(format!("resource-{number}"));
                        }
                        (input.clone(), receipt(bound))
                    });
                if entry.0 != *input {
                    return Outcome::Rejected(Rejection::Provider("key/input conflict".into()));
                }
                if self.lose_response.swap(false, Ordering::SeqCst) {
                    return Outcome::Unknown(Unknown::ContactLost);
                }
                Outcome::Accepted(receipt(entry.1.resource.clone()))
            }
            Request::Reconcile { .. } => self
                .starts
                .lock()
                .unwrap()
                .get(&ProviderKey::of(&call.identity))
                .map(|(_, found)| Outcome::Found(receipt(found.resource.clone())))
                .unwrap_or(Outcome::Unknown(Unknown::Visibility)),
            Request::Observe { resource, .. } => {
                Outcome::Observed(Observation::new(resource.clone(), RuntimeState::Running))
            }
            Request::Cancel { resource } | Request::Stop { resource, .. } => {
                Outcome::Accepted(receipt(resource.clone()))
            }
            Request::Resume { from, mode, .. } => {
                let mut next = resource();
                match mode {
                    ResumeMode::NewResource => next.id = ResourceId::new("resource-b".into()),
                    ResumeMode::ReuseWithNewGeneration => {
                        next.id = from.resource.id.clone();
                        next.generation = "generation-2".into();
                    }
                }
                Outcome::Accepted(receipt(next))
            }
            Request::ExternalLink { resource, .. } => Outcome::Link(ExternalLink::new(
                resource.clone(),
                "https://provider.invalid/view".into(),
                1000,
            )),
        }
    }
}

#[test]
fn lost_start_response_reconciles_the_same_key_without_creating_another_job() {
    let mut fixture = Fixture::new();
    let fake = Fake::default();
    fake.lose_response.store(true, Ordering::SeqCst);
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unknown(Unknown::ContactLost)
    );
    let start = fixture.call.clone();
    fixture.call.request = Request::Reconcile { resource: None };
    fixture.call.identity.fencing_epoch += 1;
    let Outcome::Found(found) = fixture.invoke(&fake) else {
        panic!("must find the accepted resource");
    };
    assert_eq!(found.resource, fixture.resource());
    assert_eq!(
        found.identity.fencing_epoch, 8,
        "takeover observes the same runtime with a new worker fence"
    );
    fixture.call = start;
    assert!(matches!(fixture.invoke(&fake), Outcome::Accepted(_)));
    assert!(matches!(fixture.invoke(&fake), Outcome::Accepted(_)));
    assert_eq!(fake.jobs.load(Ordering::SeqCst), 1);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 4);
    // Resource acceptance is independent of the job's Running/terminal state.
    fixture.set_request(Request::Observe {
        resource: found.resource,
        cursor: None,
    });
    assert!(matches!(
        fixture.invoke(&fake),
        Outcome::Observed(Observation {
            state: RuntimeState::Running,
            ..
        })
    ));
}

#[test]
fn duplicate_key_with_changed_input_is_rejected_by_the_provider() {
    let mut fixture = Fixture::new();
    let fake = Fake::default();
    assert!(matches!(fixture.invoke(&fake), Outcome::Accepted(_)));
    let Request::Start { input } = &mut fixture.call.request else {
        unreachable!()
    };
    input.repository_revision = "git:different".into();
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Rejected(Rejection::Provider("key/input conflict".into()))
    );
    assert_eq!(fake.jobs.load(Ordering::SeqCst), 1);
}

#[test]
fn provider_keys_are_scoped_and_worker_takeover_reuses_the_original_job() {
    let fake = Fake::default();
    let mut first = Fixture::new();
    assert!(matches!(first.invoke(&fake), Outcome::Accepted(_)));
    first.call.identity.fencing_epoch += 1;
    let Outcome::Accepted(recovered) = first.invoke(&fake) else {
        panic!("same operation under a new worker fence");
    };
    assert_eq!(
        recovered.identity.fencing_epoch,
        first.call.identity.fencing_epoch
    );
    assert_eq!(fake.jobs.load(Ordering::SeqCst), 1);
    let mut other = Fixture::new();
    other.profile.scope.project = "project-b".into();
    other.call.identity.attempt.scope.project = "project-b".into();
    let Outcome::Accepted(accepted) = other.invoke(&fake) else {
        panic!("a separate project has its own retry scope");
    };
    assert_ne!(accepted.resource.id, recovered.resource.id);
    assert_eq!(fake.jobs.load(Ordering::SeqCst), 2);
    let Request::Start { input } = &mut first.call.request else {
        unreachable!()
    };
    input.digest = "different-input".into();
    first.call.identity.input_digest = input.digest.clone();
    assert_eq!(
        first.invoke(&fake),
        Outcome::Rejected(Rejection::Provider("key/input conflict".into()))
    );
    assert_eq!(fake.jobs.load(Ordering::SeqCst), 2);
}

#[test]
fn expired_and_excessive_deadlines_do_no_io_and_late_receipts_are_unknown() {
    let fixture = Fixture::new();
    let fake = Fake::default();
    let now = Instant::now();
    assert_eq!(
        invoke(
            &fake,
            &fixture.profile,
            &fixture.authorization(),
            &fixture.call,
            now
        ),
        Outcome::Unknown(Unknown::Deadline)
    );
    for deadline in [
        now,
        now - Duration::from_secs(1),
        now + Duration::from_millis(u64::from(MAX_CALL_MS) + 1),
    ] {
        assert_eq!(
            invoke_with_clock(
                &fake,
                &fixture.profile,
                &fixture.authorization(),
                &fixture.call,
                deadline,
                || now
            ),
            Outcome::Unknown(Unknown::Deadline)
        );
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let deadline = now + Duration::from_secs(1);
    let mut times = [now, deadline].into_iter();
    assert_eq!(
        invoke_with_clock(
            &fake,
            &fixture.profile,
            &fixture.authorization(),
            &fixture.call,
            deadline,
            || times.next().unwrap()
        ),
        Outcome::Unknown(Unknown::Deadline)
    );
    assert_eq!(*fake.deadlines.lock().unwrap(), [deadline]);
    assert_eq!(
        fake.jobs.load(Ordering::SeqCst),
        1,
        "deadline does not assert provider rejection or stop the job"
    );
}

#[test]
fn unsupported_controls_and_capability_modes_do_no_io() {
    let mut fixture = Fixture::new();
    let fake = Fake::default();
    fixture.profile.capabilities = Capabilities::default();
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unsupported(Operation::Start)
    );
    fixture.profile.capabilities.operations =
        vec![Operation::Stop, Operation::ExternalLink, Operation::Resume];
    fixture.set_request(Request::Stop {
        resource: fixture.resource(),
        mode: StopMode::Pause,
    });
    assert_eq!(fixture.invoke(&fake), Outcome::Unsupported(Operation::Stop));
    fixture.set_request(Request::ExternalLink {
        resource: fixture.resource(),
        conversation: None,
    });
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unsupported(Operation::ExternalLink)
    );
    fixture.call.request = Fixture::new().call.request;
    prepare_resume(&mut fixture, ResumeMode::NewResource);
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unsupported(Operation::Resume)
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn current_host_grant_cannot_be_replaced_by_claimed_initiating_owner() {
    let mut fixture = Fixture::new();
    let fake = Fake::default();
    let agent_grant = fixture.authorization();
    fixture.call.initiator = Principal::new(PrincipalKind::Human, "claimed".into(), "owner".into());
    fixture.set_request(Request::Cancel {
        resource: fixture.resource(),
    });
    let now = Instant::now();
    assert_eq!(
        invoke_with_clock(
            &fake,
            &fixture.profile,
            &agent_grant,
            &fixture.call,
            now + Duration::from_secs(1),
            || now
        ),
        Outcome::Rejected(Rejection::Unauthorized)
    );
    assert_eq!(agent_grant.principal().kind, PrincipalKind::Agent);
    assert_eq!(agent_grant.policy_revision(), "policy-1");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn cross_project_board_task_attempt_configuration_and_resource_are_refused() {
    let fake = Fake::default();
    for variant in 0..5 {
        let fixture = Fixture::new();
        let mut target = fixture.call.identity.attempt.clone();
        let mut operation = fixture.call.request.operation();
        match variant {
            0 => target.scope.project = "other".into(),
            1 => target.scope.board = "other".into(),
            2 => target.task = "other".into(),
            3 => target.id = AttemptId::new([2; 16]),
            _ => operation = Operation::Cancel,
        }
        let auth = Authorization::from_host(
            fixture.call.initiator.clone(),
            target,
            operation,
            "policy-1".into(),
        );
        assert!(!auth.permits(&fixture.call));
    }
    for variant in 0..4 {
        let mut fixture = Fixture::new();
        match variant {
            0 => fixture.profile.scope.project = "other".into(),
            1 => fixture.profile.scope.board = "other".into(),
            2 => fixture.profile.configuration = "other".into(),
            _ => {
                let mut resource = fixture.resource();
                resource.attempt.scope.project = "other".into();
                fixture.set_request(Request::Cancel { resource });
            }
        }
        assert_eq!(fixture.invoke(&fake), Outcome::Rejected(Rejection::Scope));
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_identity_input_and_host_context_are_refused_before_io() {
    let fake = Fake::default();
    for variant in 0..12 {
        let mut fixture = Fixture::new();
        match variant {
            0 => fixture.call.identity.key.clear(),
            1 => fixture.call.identity.input_digest.clear(),
            2 => {
                fixture.call.identity.configuration.clear();
                fixture.profile.configuration.clear();
            }
            3 => fixture.call.identity.attempt.task.clear(),
            4 => {
                fixture.call.identity.attempt.scope.project.clear();
                fixture.profile.scope.project.clear();
            }
            5 => {
                fixture.call.identity.attempt.scope.board.clear();
                fixture.profile.scope.board.clear();
            }
            6 => fixture.profile.contract = Version::new(2, 0),
            7 => fixture.call.identity.kind = Operation::Cancel,
            8..=11 => {
                let Request::Start { input } = &mut fixture.call.request else {
                    unreachable!()
                };
                match variant {
                    8 => input.digest = "different".into(),
                    9 => input.task_revision.clear(),
                    10 => input.repository_revision.clear(),
                    _ => input.manifest.digest.clear(),
                }
            }
            _ => unreachable!(),
        }
        let expected = if variant >= 8 {
            Rejection::Input
        } else {
            Rejection::Identity
        };
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Rejected(expected),
            "case {variant}"
        );
    }
    let fixture = Fixture::new();
    for variant in 0..3 {
        let mut principal = fixture.call.initiator.clone();
        let mut policy = "policy-1".to_string();
        match variant {
            0 => principal.issuer.clear(),
            1 => principal.subject.clear(),
            _ => policy.clear(),
        }
        let auth = Authorization::from_host(
            principal,
            fixture.call.identity.attempt.clone(),
            Operation::Start,
            policy,
        );
        assert_eq!(
            validate(&fixture.profile, &auth, &fixture.call),
            Err(Rejection::Unauthorized)
        );
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

fn prepare_resume(fixture: &mut Fixture, mode: ResumeMode) {
    let mut from = ResumeFrom::new(fixture.resource(), "record:confirmed-quiescent".into());
    from.conversation = Some(ConversationId::new("conversation-old".into()));
    fixture.call.identity.attempt.id = AttemptId::new([2; 16]);
    let Request::Start { input } = fixture.call.request.clone() else {
        unreachable!()
    };
    fixture.set_request(Request::Resume { input, from, mode });
}

#[test]
fn resume_requires_a_new_attempt_same_scope_quiescence_and_an_explicit_reference() {
    let fake = Fake::default();
    for mode in [ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration] {
        let mut fixture = Fixture::new();
        prepare_resume(&mut fixture, mode);
        assert!(matches!(fixture.invoke(&fake), Outcome::Accepted(_)));
    }
    for variant in 0..7 {
        let mut fixture = Fixture::new();
        prepare_resume(&mut fixture, ResumeMode::NewResource);
        let Request::Resume { from, .. } = &mut fixture.call.request else {
            unreachable!()
        };
        match variant {
            0 => from.resource.attempt.id = fixture.call.identity.attempt.id.clone(),
            1 => from.resource.attempt.scope.project = "other".into(),
            2 => from.resource.attempt.task = "other".into(),
            3 => from.resource.configuration = "other".into(),
            4 => from.quiescence.clear(),
            5 => from.conversation = None,
            _ => from.resource.generation.clear(),
        }
        assert_eq!(fixture.invoke(&fake), Outcome::Rejected(Rejection::Resume));
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn absence_visibility_rejection_expired_keys_and_conflicts_stay_distinct() {
    let mut fixture = Fixture::new();
    fixture.call.request = Request::Reconcile { resource: None };
    let fake = Fake::default();
    assert_eq!(fixture.invoke(&fake), Outcome::Unknown(Unknown::Visibility));
    let absent = Outcome::Absent {
        evidence: "authoritative-no-later-acceptance".into(),
    };
    *fake.reply.lock().unwrap() = Some(absent.clone());
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unknown(Unknown::InvalidReply)
    );
    fixture.profile.capabilities.authoritative_absence = true;
    assert_eq!(fixture.invoke(&fake), absent);
    for outcome in [
        Outcome::Unknown(Unknown::KeyExpired),
        Outcome::Rejected(Rejection::Provider("definite".into())),
        Outcome::Conflicting {
            matches: vec![fixture.receipt(), fixture.receipt()],
        },
    ] {
        *fake.reply.lock().unwrap() = Some(outcome.clone());
        assert_eq!(fixture.invoke(&fake), outcome);
    }
    let mut foreign = fixture.receipt();
    foreign.resource.attempt.scope.project = "other-project".into();
    *fake.reply.lock().unwrap() = Some(Outcome::Conflicting {
        matches: vec![fixture.receipt(), foreign],
    });
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unknown(Unknown::InvalidReply)
    );
}

#[test]
fn wrong_outcome_kind_identity_input_fence_epoch_or_generation_is_unknown() {
    let fixture = Fixture::new();
    let fake = Fake::default();
    for variant in 0..9 {
        let mut receipt = fixture.receipt();
        match variant {
            0 => receipt.identity.key = "other".into(),
            1 => receipt.identity.input_digest = "other".into(),
            2 => receipt.identity.fencing_epoch += 1,
            3 => receipt.identity.runtime_epoch += 1,
            4 => receipt.resource.attempt.scope.project = "other".into(),
            5 => receipt.resource.configuration = "other".into(),
            6 => receipt.resource.generation.clear(),
            7 => receipt.resource.id = ResourceId::new(String::new()),
            _ => receipt.evidence.clear(),
        }
        *fake.reply.lock().unwrap() = Some(Outcome::Accepted(receipt));
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Unknown(Unknown::InvalidReply)
        );
    }
    for outcome in [
        Outcome::Found(fixture.receipt()),
        Outcome::Observed(Observation::new(fixture.resource(), RuntimeState::Running)),
        Outcome::Link(ExternalLink::new(fixture.resource(), "view".into(), 1)),
        Outcome::Unsupported(Operation::Stop),
        Outcome::Conflicting { matches: vec![] },
    ] {
        *fake.reply.lock().unwrap() = Some(outcome);
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Unknown(Unknown::InvalidReply)
        );
    }
}

#[test]
fn observations_controls_links_and_resume_answers_are_bound_to_exact_resources() {
    let fake = Fake::default();
    for request in [
        Request::Cancel {
            resource: Fixture::new().resource(),
        },
        Request::Stop {
            resource: Fixture::new().resource(),
            mode: StopMode::HardStop,
        },
        Request::Observe {
            resource: Fixture::new().resource(),
            cursor: Some("cursor-1".into()),
        },
        Request::ExternalLink {
            resource: Fixture::new().resource(),
            conversation: Some(ConversationId::new("conversation".into())),
        },
    ] {
        let mut fixture = Fixture::new();
        fixture.set_request(request);
        assert!(!matches!(fixture.invoke(&fake), Outcome::Unknown(_)));
        let mut wrong = fixture.resource();
        wrong.generation = "stale-generation".into();
        let outcome = match fixture.call.request.operation() {
            Operation::Observe => Outcome::Observed(Observation::new(wrong, RuntimeState::Running)),
            Operation::ExternalLink => Outcome::Link(ExternalLink::new(wrong, "view".into(), 10)),
            _ => Outcome::Accepted(Receipt::new(
                fixture.call.identity.clone(),
                wrong,
                "receipt".into(),
            )),
        };
        *fake.reply.lock().unwrap() = Some(outcome);
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Unknown(Unknown::InvalidReply)
        );
        *fake.reply.lock().unwrap() = None;
    }
    for mode in [ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration] {
        let mut fixture = Fixture::new();
        prepare_resume(&mut fixture, mode);
        *fake.reply.lock().unwrap() = Some(Outcome::Accepted(fixture.receipt()));
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Unknown(Unknown::InvalidReply)
        );
    }
}

#[test]
fn link_lifetime_and_nonempty_absence_evidence_are_enforced() {
    let mut fixture = Fixture::new();
    let fake = Fake::default();
    fixture.set_request(Request::ExternalLink {
        resource: fixture.resource(),
        conversation: None,
    });
    for link in [
        ExternalLink::new(fixture.resource(), "view".into(), 0),
        ExternalLink::new(fixture.resource(), "view".into(), u64::MAX),
        ExternalLink::new(fixture.resource(), String::new(), 1),
    ] {
        *fake.reply.lock().unwrap() = Some(Outcome::Link(link));
        assert_eq!(
            fixture.invoke(&fake),
            Outcome::Unknown(Unknown::InvalidReply)
        );
    }
    fixture.call.request = Request::Reconcile {
        resource: Some(fixture.resource()),
    };
    fixture.profile.capabilities.authoritative_absence = true;
    *fake.reply.lock().unwrap() = Some(Outcome::Absent {
        evidence: String::new(),
    });
    assert_eq!(
        fixture.invoke(&fake),
        Outcome::Unknown(Unknown::InvalidReply)
    );
}

#[test]
fn process_envelopes_negotiate_fail_closed_and_ignore_payload_owner_claims() {
    let fixture = Fixture::new();
    let fake = Fake::default();
    let envelope = wire::Envelope::new(
        fixture.authorization(),
        fixture.call.clone(),
        CallLimit::new(1000).unwrap(),
    );
    let mut json = serde_json::to_value(&envelope).unwrap();
    json["owner"] = serde_json::json!("owner");
    json["call"]["owner"] = serde_json::json!(true);
    let decoded: wire::Envelope = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(decoded, envelope);
    assert_eq!(decoded.authorization.principal().kind, PrincipalKind::Agent);
    assert_eq!(wire::negotiate(Version::new(1, 10)).unwrap(), wire::VERSION);
    assert!(wire::negotiate(Version::new(0, 3)).is_err());
    let mut incompatible = decoded.clone();
    incompatible.contract = Version::new(2, 0);
    assert!(incompatible.dispatch(&fake, &fixture.profile).is_err());
    incompatible.contract = Version::new(1, 1);
    assert!(incompatible.dispatch(&fake, &fixture.profile).is_err());
    let mut wrong_pin = fixture.profile.clone();
    wrong_pin.contract = Version::new(1, 1);
    assert!(decoded.dispatch(&fake, &wrong_pin).is_err());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    for field in ["authorization", "call", "contract", "limit"] {
        let mut missing = json.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<wire::Envelope>(missing).is_err(),
            "{field}"
        );
    }
    json["call"]["request"] = serde_json::json!({"future.operation": {}});
    assert!(serde_json::from_value::<wire::Envelope>(json).is_err());
    for budget in [0, MAX_CALL_MS + 1, u32::MAX] {
        assert!(CallLimit::new(budget).is_err());
        assert!(serde_json::from_str::<CallLimit>(&budget.to_string()).is_err());
    }
    assert_eq!(
        CallLimit::new(MAX_CALL_MS).unwrap().milliseconds(),
        MAX_CALL_MS
    );
    let now = Instant::now();
    assert_eq!(
        decoded.accept(
            &fixture.profile,
            decoded
                .dispatch_with_clock(&fake, &fixture.profile, || now)
                .unwrap()
        ),
        Outcome::Accepted(fixture.receipt())
    );
}

#[test]
fn process_responses_bind_even_ambiguous_and_absent_results_to_the_exact_operation() {
    let mut fixture = Fixture::new();
    fixture.call.request = Request::Reconcile { resource: None };
    fixture.profile.capabilities.authoritative_absence = true;
    let envelope = wire::Envelope::new(
        fixture.authorization(),
        fixture.call.clone(),
        CallLimit::new(1000).unwrap(),
    );
    for outcome in [
        Outcome::Unknown(Unknown::ContactLost),
        Outcome::Absent {
            evidence: "no-later-acceptance".into(),
        },
        Outcome::Unsupported(Operation::Reconcile),
    ] {
        let answer = wire::Answer::new(fixture.call.identity.clone(), outcome.clone());
        let answer: wire::Answer =
            serde_json::from_str(&serde_json::to_string(&answer).unwrap()).unwrap();
        assert_eq!(envelope.accept(&fixture.profile, answer.clone()), outcome);
        for variant in 0..5 {
            let mut stale = answer.clone();
            match variant {
                0 => stale.identity.fencing_epoch += 1,
                1 => stale.identity.runtime_epoch += 1,
                2 => stale.identity.input_digest = "other".into(),
                3 => stale.identity.attempt.scope.project = "other".into(),
                _ => stale.contract = Version::new(2, 0),
            }
            assert_eq!(
                envelope.accept(&fixture.profile, stale),
                Outcome::Unknown(Unknown::InvalidReply)
            );
        }
    }
    let wrong_kind = wire::Answer::new(
        fixture.call.identity.clone(),
        Outcome::Accepted(fixture.receipt()),
    );
    assert_eq!(
        envelope.accept(&fixture.profile, wrong_kind),
        Outcome::Unknown(Unknown::InvalidReply)
    );
    let mut wrong_pin = fixture.profile.clone();
    wrong_pin.scope.project = "other-project".into();
    assert_eq!(
        envelope.accept(
            &wrong_pin,
            wire::Answer::new(
                fixture.call.identity.clone(),
                Outcome::Unknown(Unknown::ContactLost)
            )
        ),
        Outcome::Unknown(Unknown::InvalidReply)
    );
}

fn roundtrip<T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq>(value: T) {
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value);
}

#[test]
fn all_requests_and_outcomes_roundtrip_with_server_json_features() {
    let mut fixture = Fixture::new();
    let resource = fixture.resource();
    let requests = [
        fixture.call.request.clone(),
        Request::Reconcile {
            resource: Some(resource.clone()),
        },
        Request::Observe {
            resource: resource.clone(),
            cursor: Some("cursor".into()),
        },
        Request::Cancel {
            resource: resource.clone(),
        },
        Request::Stop {
            resource: resource.clone(),
            mode: StopMode::Terminate,
        },
        Request::ExternalLink {
            resource: resource.clone(),
            conversation: None,
        },
    ];
    for request in requests {
        fixture.set_request(request);
        roundtrip(wire::Envelope::new(
            fixture.authorization(),
            fixture.call.clone(),
            CallLimit::new(1000).unwrap(),
        ));
    }
    fixture = Fixture::new();
    prepare_resume(&mut fixture, ResumeMode::NewResource);
    roundtrip(fixture.call.clone());
    let receipt = fixture.receipt();
    let mut observation = Observation::new(resource.clone(), RuntimeState::Succeeded);
    observation.artifacts = Some(ContentRef::new("artifact-manifest".into(), "digest".into()));
    observation.checkpoint = observation.artifacts.clone();
    observation.completion = observation.artifacts.clone();
    observation.conversation = Some(ConversationId::new("conversation".into()));
    observation.cursor = Some("cursor".into());
    for outcome in [
        Outcome::Accepted(receipt.clone()),
        Outcome::Found(receipt.clone()),
        Outcome::Absent {
            evidence: "none".into(),
        },
        Outcome::Conflicting {
            matches: vec![receipt.clone(), receipt],
        },
        Outcome::Observed(observation),
        Outcome::Link(ExternalLink::new(resource, "view".into(), 1000)),
        Outcome::Unsupported(Operation::Resume),
        Outcome::Rejected(Rejection::Unauthorized),
        Outcome::Unknown(Unknown::ContactLost),
    ] {
        roundtrip(outcome);
    }
    roundtrip(fixture.profile.clone());
    for state in [
        RuntimeState::Provisioning,
        RuntimeState::Running,
        RuntimeState::Paused,
        RuntimeState::Succeeded,
        RuntimeState::Failed,
        RuntimeState::Terminated,
        RuntimeState::Destroyed,
        RuntimeState::Unknown,
    ] {
        roundtrip(state);
    }
    for kind in [
        PrincipalKind::Human,
        PrincipalKind::Service,
        PrincipalKind::Agent,
    ] {
        roundtrip(Principal::new(kind, "issuer".into(), "subject".into()));
    }
    assert_eq!(AttemptId::new([1; 16]).value(), &[1; 16]);
    assert_eq!(ConversationId::new("opaque".into()).value(), "opaque");
    assert_eq!(
        serde_json::from_str::<Capabilities>("{}").unwrap(),
        Capabilities::default()
    );
}
