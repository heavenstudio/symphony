//! A consumer builds the execution contract without the board/server, using constructors and
//! exhaustive matches only with a fallback. Existing plugin/Transport constructors remain in
//! their own guards. This is a compile contract, not an execution coordinator implementation.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

use symphony_plugin::execution::{
    Adapter, AttemptId, AttemptRef, Authorization, Call, CallLimit, Capabilities, ContentRef,
    ConversationId, ExternalLink, Input, Observation, Operation, OperationIdentity, Outcome,
    Principal, PrincipalKind, Profile, Receipt, Request, Resource, ResourceId, ResumeFrom,
    ResumeMode, RuntimeState, Scope, StopMode, Unknown, invoke, wire,
};

struct Consumer;

impl Adapter for Consumer {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::ContactLost)
    }
}

#[test]
fn public_constructors_and_the_adapter_keep_their_shapes() {
    let scope = Scope::new("project".into(), "board".into());
    let attempt = AttemptRef::new(scope.clone(), "task".into(), AttemptId::new([1; 16]));
    let principal = Principal::new(PrincipalKind::Service, "issuer".into(), "worker".into());
    let mut capabilities = Capabilities::default();
    capabilities.operations.push(Operation::Start);
    let profile = Profile::new(scope, "configuration".into(), capabilities);
    let input = Input::new(
        "revision".into(),
        "digest".into(),
        "commit".into(),
        ContentRef::new("manifest".into(), "digest".into()),
    );
    let identity = OperationIdentity::new(
        attempt.clone(),
        profile.configuration.clone(),
        input.digest.clone(),
        "key".into(),
        Operation::Start,
        1,
    )
    .with_epochs(1, 2);
    let call = Call::new(
        identity.clone(),
        principal.clone(),
        Request::Start { input },
    );
    let grant = Authorization::from_host(
        principal,
        attempt.clone(),
        Operation::Start,
        "policy".into(),
    );
    let resource = Resource::new(
        attempt,
        profile.configuration.clone(),
        ResourceId::new("opaque".into()),
        "generation".into(),
    );
    let mut from = ResumeFrom::new(resource.clone(), "quiescence-fact".into());
    from.conversation = Some(ConversationId::new("conversation".into()));
    let _ = Request::ReconcileResume {
        resource: None,
        from,
        mode: ResumeMode::ReuseWithNewGeneration,
    };
    let _: fn(&Request) -> u32 = Request::since;
    let _ = Receipt::new(identity, resource.clone(), "evidence".into());
    let _ = Observation::new(resource.clone(), RuntimeState::Running);
    let _ = ExternalLink::new(resource, "view".into(), 1000);
    let _ = (StopMode::HardStop, ResumeMode::ReuseWithNewGeneration);
    let envelope = wire::Envelope::new(grant, call, CallLimit::new(1000).unwrap());
    let _ = |outcome: Outcome| match outcome {
        Outcome::Unknown(Unknown::ContactLost) => "reconcile",
        _ => "inspect",
    };
    let _: fn(&dyn Adapter, &Profile, &Authorization, &Call, Instant) -> Outcome = invoke;
    let _: fn(&wire::Envelope, &dyn Adapter, &Profile) -> Result<wire::Answer, String> =
        wire::Envelope::dispatch;
    let _: &dyn Adapter = &Consumer;
    assert_eq!(wire::negotiate(wire::VERSION).unwrap(), profile.contract);
    assert_eq!(envelope.authorization.principal().subject, "worker");
}

/// Only the parent test's private pipe is trusted. No shipped process runner or UI plugin seam
/// is implied by this serialization fixture. Linked dispatch refusals are covered by unit tests.
#[test]
fn wire_child() {
    if std::env::var_os("SYMPHONY_EXECUTION_CONTRACT_FIXTURE").is_none() {
        return;
    }
    let mut input = String::new();
    std::io::stdin()
        .take(65_536)
        .read_to_string(&mut input)
        .unwrap();
    let envelope: wire::Envelope = serde_json::from_str(&input).unwrap();
    assert_eq!(wire::negotiate(envelope.contract).unwrap(), wire::VERSION);
    assert_eq!(
        envelope.authorization.principal().kind,
        PrincipalKind::Agent
    );
    let resource = Resource::new(
        envelope.call.identity.attempt.clone(),
        envelope.call.identity.configuration.clone(),
        ResourceId::new("provider-resource".into()),
        "generation".into(),
    );
    let answer = Outcome::Accepted(Receipt::new(
        envelope.call.identity.clone(),
        resource,
        "accepted-not-completed".into(),
    ));
    let answer = wire::Answer::new(envelope.call.identity.clone(), answer);
    println!(
        "EXECUTION_WIRE={}",
        serde_json::to_string(&(envelope, answer)).unwrap()
    );
}

#[test]
fn owned_contract_and_actor_context_cross_a_real_process_without_plugin_negotiation() {
    let scope = Scope::new("project".into(), "board".into());
    let target = AttemptRef::new(scope, "task".into(), AttemptId::new([3; 16]));
    let principal = Principal::new(
        PrincipalKind::Agent,
        "trusted-host".into(),
        "executor".into(),
    );
    let identity = OperationIdentity::new(
        target.clone(),
        "configuration".into(),
        "input-digest".into(),
        "persisted-key".into(),
        Operation::Start,
        1,
    )
    .with_epochs(2, 4);
    let input = Input::new(
        "revision".into(),
        "input-digest".into(),
        "commit".into(),
        ContentRef::new("manifest".into(), "digest".into()),
    );
    let envelope = wire::Envelope::new(
        Authorization::from_host(principal.clone(), target, Operation::Start, "policy".into()),
        Call::new(identity, principal, Request::Start { input }),
        CallLimit::new(1000).unwrap(),
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "wire_child", "--nocapture"])
        .env("SYMPHONY_EXECUTION_CONTRACT_FIXTURE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let bytes = serde_json::to_vec(&envelope).unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(&bytes) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("fixture input: {error}");
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let response = stdout
        .lines()
        .find_map(|line| line.split_once("EXECUTION_WIRE=").map(|(_, json)| json))
        .expect("child response");
    let (read, answer): (wire::Envelope, wire::Answer) = serde_json::from_str(response).unwrap();
    assert_eq!(read, envelope);
    assert_eq!(answer.identity, envelope.call.identity);
    let Outcome::Accepted(receipt) = answer.outcome else {
        panic!("receipt only, no job lifetime in the pipe");
    };
    assert_eq!(receipt.identity, envelope.call.identity);
    assert_eq!(receipt.resource.attempt, envelope.call.identity.attempt);
    assert_eq!(receipt.evidence, "accepted-not-completed");
}
