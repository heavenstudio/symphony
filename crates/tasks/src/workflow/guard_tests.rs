//! A guard's attestation and an authority's observation without a board: what a signature, a
//! binding, a time and an accepted state each refuse, how an authority's state moves, and which
//! graphs leave a route around a guard.

use super::*;
use crate::store_fixture::pair;
use crate::workflow::activate::tests::{TWO, custom};
use crate::workflow::{Custom, agent_session, local_owner};
use ed25519_dalek::{Signer, SigningKey};
use symphony_api::workflow::{Attested, AuthorityState};

/// TWO with its acceptance guarded by `approved`, which the `approvals` authority attests, in the
/// project `sim`, and the key of `seed` committed for it.
pub(crate) fn guarded_text(seed: u8) -> String {
    let text = TWO
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        )
        .replacen(
            "operation = \"close_covered\", authority = [{ kind = \"project_capability\", capability = \"owner\" }], inputs = [], dependencies = \"none\", guards = []",
            "operation = \"close_covered\", authority = [{ kind = \"project_capability\", capability = \"owner\" }], inputs = [], dependencies = \"none\", guards = [\"approved\"]",
            1,
        );
    assert!(
        text.contains("guards = [\"approved\"]"),
        "the fixture guards accept"
    );
    format!(
        "{text}\n[project]\nid = \"sim\"\n\n[[workflow_authorities]]\nadapter = \"approvals\"\n\
         issuer = \"ledger\"\ncontract = 1\nkey = \"{}\"\n",
        attester(seed).key
    )
}

pub(crate) fn guarded() -> Custom {
    custom(&guarded_text(7))
}

/// `text`, a workflow with no guard, with its edge `edge` guarded by `approved`, which the
/// `approvals` authority attests by the key of seed 7, in the project `sim`.
pub(crate) fn guarding_edge(text: &str, edge: &str) -> String {
    let start = text
        .find(&format!("{{ id = \"{edge}\","))
        .expect("the edge");
    let end = start + text[start..].find('\n').expect("its line ends");
    let line = text[start..end].replacen("guards = []", "guards = [\"approved\"]", 1);
    assert!(
        line.contains("guards = [\"approved\"]"),
        "{edge} is guarded"
    );
    let text = format!("{}{line}{}", &text[..start], &text[end..])
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        );
    format!(
        "{text}\n[project]\nid = \"sim\"\n\n[[workflow_authorities]]\nadapter = \"approvals\"\n\
         issuer = \"ledger\"\ncontract = 1\nkey = \"{}\"\n",
        attester(7).key
    )
}

pub(crate) fn signing(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn attester(seed: u8) -> Attester {
    Attester {
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        contract: ATTESTATION_CONTRACT,
        key: format!("ed25519:{}", hex(signing(seed).verifying_key().as_bytes())),
    }
}

/// [`guarded_text`] of seed 7 with its acceptance guarded by `reviewed` as well, which a second
/// authority, `reviews` of the issuer `panel`, attests by the key of seed 8.
pub(crate) fn two_authorities_text() -> String {
    let reviews = Attester {
        adapter: "reviews".into(),
        issuer: "panel".into(),
        ..attester(8)
    };
    let text = guarded_text(7)
        .replacen(
            "adapters = [\"approvals\"]",
            "adapters = [\"approvals\", \"reviews\"]",
            1,
        )
        .replacen(
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }, \
             { id = \"reviewed\", adapter = \"reviews\" }]",
            1,
        )
        .replacen(
            "guards = [\"approved\"]",
            "guards = [\"approved\", \"reviewed\"]",
            1,
        );
    assert!(text.contains("guards = [\"approved\", \"reviewed\"]"));
    format!(
        "{text}\n[[workflow_authorities]]\nadapter = \"reviews\"\nissuer = \"panel\"\n\
         contract = 1\nkey = \"{}\"\n",
        reviews.key
    )
}

/// `payload` signed by `seed`'s key as an observation or an attestation.
pub(crate) fn signed(seed: u8, observation: bool, payload: &str) -> Signed {
    let signature = signing(seed).sign(&signed_message(observation, payload));
    Signed {
        payload: payload.into(),
        signature: hex(&signature.to_bytes()),
    }
}

pub(crate) fn sign_attestation(seed: u8, attestation: &Attestation) -> Signed {
    signed(seed, false, &serde_json::to_string(attestation).unwrap())
}

pub(crate) fn sign_observation(seed: u8, observation: &Observation) -> Signed {
    signed(seed, true, &serde_json::to_string(observation).unwrap())
}

/// The accepted state the fixtures attest at.
pub(crate) fn accepted() -> Accepted {
    Accepted {
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation: 3,
        state: "s3".into(),
        conflicted: false,
        admitted: 0,
    }
}

/// A pass of `approved` for the owner accepting `sim/a` from verify/awaiting at revision 4 of
/// generation 2 of `custom`, at the accepted state, issued at 100 for 300 s.
pub(crate) fn passing(custom: &Custom) -> Attestation {
    Attestation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation: 3,
        project: "sim".into(),
        task: "sim/a".into(),
        guard: "approved".into(),
        edge: "accept".into(),
        workflow: custom.revision.digest.clone(),
        activation_generation: 2,
        task_revision: 4,
        at: pair("verify", "awaiting"),
        input_revisions: BTreeMap::new(),
        principal: attestee(&local_owner()),
        result: Attested::Pass,
        facts: vec!["ledger:decision/41".into()],
        reasons: Vec::new(),
        issued_at: 100.0,
        expires_at: 400.0,
    }
}

struct Case {
    custom: Custom,
    attesters: Vec<Attester>,
    accepted: Vec<Accepted>,
    principal: Principal,
    inputs: BTreeMap<String, String>,
    at: Pair,
    revision: u64,
    project: String,
    now: f64,
}

impl Case {
    fn new() -> Case {
        let custom = guarded();
        Case {
            attesters: custom.authorities.clone(),
            custom,
            accepted: vec![accepted()],
            principal: local_owner(),
            inputs: BTreeMap::new(),
            at: pair("verify", "awaiting"),
            revision: 4,
            project: "sim".into(),
            now: 200.0,
        }
    }

    /// `given` checked for `edge` as this case's operation.
    fn check_edge(&self, edge: &str, given: &[Signed]) -> Result<Vec<Checked>, Refusal> {
        let definition = self.custom.definition.definition();
        let edge = definition.edges.iter().find(|e| e.id == edge).unwrap();
        let binding = Binding {
            project: &self.project,
            task: "sim/a",
            edge: &edge.id,
            workflow: &self.custom.revision.digest,
            activation_generation: 2,
            task_revision: self.revision,
            at: &self.at,
            input_revisions: &self.inputs,
            principal: &self.principal,
        };
        let guarding = Guarding {
            project: &self.project,
            attesters: &self.attesters,
            accepted: &self.accepted,
            given,
            now: self.now,
        };
        check(definition, edge, &binding, &guarding)
    }

    fn check(&self, given: &[Signed]) -> Result<Vec<Checked>, Refusal> {
        self.check_edge("accept", given)
    }

    /// The refusal of `attestation`, changed by `change` and signed by the committed key.
    fn refusal(&self, change: impl FnOnce(&mut Attestation)) -> Refusal {
        let mut attestation = passing(&self.custom);
        change(&mut attestation);
        self.check(&[sign_attestation(7, &attestation)])
            .expect_err("refused")
    }
}

#[test]
fn a_pass_covering_exactly_the_operation_meets_its_guard_at_the_accepted_state() {
    let case = Case::new();
    let attestation = passing(&case.custom);
    let signed = sign_attestation(7, &attestation);
    let checked = case.check(std::slice::from_ref(&signed)).unwrap();
    assert_eq!(
        checked,
        vec![Checked {
            guard: "approved".into(),
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            epoch: "e1".into(),
            generation: 3,
            state: "s3".into(),
            digest: digest(&signed.payload),
            facts: vec!["ledger:decision/41".into()],
            input_revisions: BTreeMap::new(),
            expires_at: 400.0,
            signed,
        }]
    );
    assert_eq!(digest("abc").len(), 64);
}

#[test]
fn an_attestation_for_any_other_operation_is_refused_by_what_it_covers() {
    let case = Case::new();
    type Change = fn(&mut Attestation);
    let cases: [(&str, Change); 8] = [
        ("project", |a| a.project = "other".into()),
        ("task", |a| a.task = "sim/b".into()),
        ("edge", |a| a.edge = "submit".into()),
        ("workflow", |a| a.workflow = "0".repeat(64)),
        ("activation generation", |a| a.activation_generation = 1),
        ("task revision", |a| a.task_revision = 3),
        ("pair", |a| a.at = pair("build", "working")),
        ("project", |a| a.project = String::new()),
    ];
    for (what, change) in cases {
        let refused = case.refusal(change);
        assert_eq!(refused.code, RefusalCode::GuardUnavailable, "{what}");
        assert!(
            refused.message.contains(&format!("is for {what} ")),
            "{what}: {}",
            refused.message
        );
    }
}

#[test]
fn another_projects_attestation_is_refused_whatever_project_the_request_names() {
    // The board binds to the project its activation recorded: an attestation the same authority
    // signed for another project never passes here, even when the authority is shared.
    let mut case = Case::new();
    let mut theirs = passing(&case.custom);
    theirs.project = "theirs".into();
    let signed = sign_attestation(7, &theirs);
    let refused = case.check(std::slice::from_ref(&signed)).unwrap_err();
    assert_eq!(refused.code, RefusalCode::GuardUnavailable);
    assert!(
        refused.message.contains("is for project theirs"),
        "{refused}"
    );
    // The same attestation on the board whose project it names passes.
    case.project = "theirs".into();
    assert!(case.check(&[signed]).is_ok());
}

#[test]
fn an_attestation_for_another_principal_is_unauthorized() {
    let case = Case::new();
    let refused = case.refusal(|a| a.principal = attestee(&agent_session("amber")));
    assert_eq!(refused.code, RefusalCode::Unauthorized);
    let refused = case.refusal(|a| a.principal.kind = "agent".into());
    assert_eq!(refused.code, RefusalCode::Unauthorized);
}

#[test]
fn input_revisions_drifting_from_the_attested_ones_are_stale() {
    let mut case = Case::new();
    case.inputs.insert("design".into(), "r2".into());
    let refused = case.refusal(|a| {
        a.input_revisions.insert("design".into(), "r1".into());
    });
    assert_eq!(refused.code, RefusalCode::StaleInput);
    let none = case.refusal(|_| {});
    assert_eq!(none.code, RefusalCode::StaleInput);
    let mut same = passing(&case.custom);
    same.input_revisions = case.inputs.clone();
    assert!(case.check(&[sign_attestation(7, &same)]).is_ok());
}

#[test]
fn a_refusal_an_unknown_or_a_pass_resting_on_nothing_never_passes() {
    let case = Case::new();
    let refused = case.refusal(|a| {
        a.result = Attested::Refuse;
        a.facts.clear();
        a.reasons = vec!["decision 41 revoked".into()];
    });
    assert_eq!(refused.code, RefusalCode::GuardUnavailable);
    assert!(
        refused
            .message
            .contains("refuses approved for sim/a: decision 41 revoked")
    );
    let unknown = case.refusal(|a| {
        a.result = Attested::Unknown;
        a.reasons = vec!["ledger unreachable".into()];
    });
    assert_eq!(unknown.code, RefusalCode::GuardUnavailable);
    assert!(unknown.message.contains("cannot say"));
    let empty = case.refusal(|a| a.facts.clear());
    assert_eq!(empty.code, RefusalCode::GuardUnavailable);
    assert!(empty.message.contains("passes on no decision"));
}

#[test]
fn a_signature_by_any_other_key_or_over_any_other_bytes_does_not_verify() {
    let case = Case::new();
    let attestation = passing(&case.custom);
    let unverified = |given: Signed| {
        let refused = case.check(&[given]).unwrap_err();
        assert_eq!(refused.code, RefusalCode::GuardUnavailable);
        refused.message
    };
    assert!(unverified(sign_attestation(8, &attestation)).contains("does not verify"));
    // An observation's signature is no attestation's: the contexts differ.
    let payload = serde_json::to_string(&attestation).unwrap();
    assert!(unverified(signed(7, true, &payload)).contains("does not verify"));
    let mut tampered = sign_attestation(7, &attestation);
    tampered.payload = tampered.payload.replace("sim/a", "sim/b");
    assert!(unverified(tampered).contains("does not verify"));
    let mut short = sign_attestation(7, &attestation);
    short.signature.truncate(64);
    assert!(unverified(short).contains("64 bytes"));
    let mut upper = sign_attestation(7, &attestation);
    upper.signature = upper.signature.to_uppercase();
    assert!(unverified(upper).contains("64 bytes"));
}

#[test]
fn a_guard_is_met_only_by_its_own_authority_never_by_another_one_the_workflow_commits() {
    let custom = custom(&two_authorities_text());
    let mut case = Case::new();
    case.attesters = custom.authorities.clone();
    case.accepted.push(Accepted {
        adapter: "reviews".into(),
        issuer: "panel".into(),
        epoch: "f1".into(),
        generation: 5,
        state: "t5".into(),
        ..accepted()
    });
    case.custom = custom;
    let approved = Attestation {
        workflow: case.custom.revision.digest.clone(),
        ..passing(&case.custom)
    };
    // `reviews`'s word on `reviewed`, at the state the board accepted from it.
    let reviewed = Attestation {
        adapter: "reviews".into(),
        issuer: "panel".into(),
        epoch: "f1".into(),
        generation: 5,
        guard: "reviewed".into(),
        ..approved.clone()
    };
    let met = case
        .check(&[
            sign_attestation(7, &approved),
            sign_attestation(8, &reviewed),
        ])
        .unwrap();
    let by: Vec<(&str, &str, &str)> = (met.iter())
        .map(|c| (c.guard.as_str(), c.adapter.as_str(), c.issuer.as_str()))
        .collect();
    assert_eq!(
        by,
        [
            ("approved", "approvals", "ledger"),
            ("reviewed", "reviews", "panel")
        ]
    );
    // Each authority attesting the other's guard, its own key and issuer, the other's state.
    let by_approvals = Attestation {
        guard: "reviewed".into(),
        epoch: "f1".into(),
        generation: 5,
        ..approved.clone()
    };
    let by_reviews = Attestation {
        guard: "approved".into(),
        epoch: "e1".into(),
        generation: 3,
        ..reviewed.clone()
    };
    for given in [
        [
            sign_attestation(7, &approved),
            sign_attestation(7, &by_approvals),
        ],
        [
            sign_attestation(8, &by_reviews),
            sign_attestation(8, &reviewed),
        ],
    ] {
        let refused = case.check(&given).unwrap_err();
        assert_eq!(refused.code, RefusalCode::GuardUnavailable);
        assert!(refused.message.contains("only"), "{refused}");
    }
    // Nor does either alone meet the edge's other guard.
    let alone = case.check(&[sign_attestation(7, &approved)]).unwrap_err();
    assert!(
        alone
            .message
            .contains("taken with an attestation from reviews"),
        "{alone}"
    );
}

#[test]
fn a_payload_naming_a_binding_or_an_input_twice_is_refused() {
    let mut case = Case::new();
    case.inputs = BTreeMap::from([("design".to_string(), "r7".to_string())]);
    let mut attestation = passing(&case.custom);
    attestation.input_revisions = case.inputs.clone();
    let payload = serde_json::to_string(&attestation).unwrap();
    assert!(case.check(&[signed(7, false, &payload)]).is_ok());
    for field in ["\"task\":\"sim/b\",", "\"project\":\"elsewhere\","] {
        let twice = payload.replacen('{', &format!("{{{field}"), 1);
        let refused = case.check(&[signed(7, false, &twice)]).unwrap_err();
        assert_eq!(refused.code, RefusalCode::GuardUnavailable);
        assert!(refused.message.contains("duplicate field"), "{refused}");
    }
    // Read last-wins, the first revision named would be passed over for the one the board binds.
    let twice = payload.replacen(
        "\"design\":\"r7\"",
        "\"design\":\"r1\",\"design\":\"r7\"",
        1,
    );
    assert_ne!(twice, payload);
    let refused = case.check(&[signed(7, false, &twice)]).unwrap_err();
    assert_eq!(refused.code, RefusalCode::GuardUnavailable);
    assert!(
        refused.message.contains("input design is named twice"),
        "{refused}"
    );
}

#[test]
fn a_payload_from_another_contract_issuer_or_authority_is_refused() {
    let case = Case::new();
    let contract = case.refusal(|a| a.contract = 2);
    assert!(contract.message.contains("contract 2"), "{contract}");
    let issuer = case.refusal(|a| a.issuer = "other".into());
    assert!(issuer.message.contains("as other"), "{issuer}");
    let adapter = case.refusal(|a| a.adapter = "elsewhere".into());
    assert_eq!(adapter.code, RefusalCode::GuardUnavailable);
    assert!(adapter.message.contains("names elsewhere"), "{adapter}");
    let garbage = case.check(&[signed(7, false, "not json")]).unwrap_err();
    assert!(garbage.message.contains("names no adapter"), "{garbage}");
    let mut unknown_field: serde_json::Value = serde_json::to_value(passing(&case.custom)).unwrap();
    unknown_field["extra"] = true.into();
    let refused = case
        .check(&[signed(7, false, &unknown_field.to_string())])
        .unwrap_err();
    assert!(refused.message.contains("no attestation"), "{refused}");
}

#[test]
fn an_attestation_outside_its_time_is_refused() {
    let mut case = Case::new();
    let expired = {
        case.now = 400.0;
        case.refusal(|_| {})
    };
    assert!(expired.message.contains("expired at 400"), "{expired}");
    case.now = 200.0;
    let long = case.refusal(|a| a.expires_at = a.issued_at + MAX_VALIDITY + 1.0);
    assert!(long.message.contains("longer than"), "{long}");
    let backwards = case.refusal(|a| a.expires_at = a.issued_at);
    assert!(backwards.message.contains("no later than"), "{backwards}");
    // JSON carries no infinity: a payload saying one is no attestation.
    let infinite = case.refusal(|a| a.expires_at = f64::INFINITY);
    assert_eq!(infinite.code, RefusalCode::GuardUnavailable, "{infinite}");
    let ahead = case.refusal(|a| {
        a.issued_at = 200.0 + SKEW + 1.0;
        a.expires_at = a.issued_at + 10.0;
    });
    assert!(
        ahead.message.contains("ahead of this machine's clock"),
        "{ahead}"
    );
    // Within the skew, and for exactly the longest validity, passes.
    let mut within = passing(&case.custom);
    within.issued_at = 200.0 + SKEW;
    within.expires_at = within.issued_at + MAX_VALIDITY;
    assert!(case.check(&[sign_attestation(7, &within)]).is_ok());
}

#[test]
fn an_attestation_at_any_other_generation_than_the_accepted_one_is_refused() {
    let case = Case::new();
    let behind = case.refusal(|a| a.generation = 2);
    assert!(
        behind.message.contains("a later decision stands"),
        "{behind}"
    );
    let ahead = case.refusal(|a| a.generation = 4);
    assert!(
        ahead.message.contains("publishes that observation first"),
        "{ahead}"
    );
    let epoch = case.refusal(|a| a.epoch = "e2".into());
    assert!(epoch.message.contains("a later decision stands"), "{epoch}");
}

#[test]
fn an_authority_not_committed_not_observed_or_in_conflict_fails_closed() {
    let mut case = Case::new();
    let given = [sign_attestation(7, &passing(&case.custom))];
    case.accepted[0].conflicted = true;
    let conflicted = case.check(&given).unwrap_err();
    assert_eq!(conflicted.code, RefusalCode::GuardUnavailable);
    assert!(conflicted.message.contains("two different decisions"));
    case.accepted.clear();
    let unobserved = case.check(&given).unwrap_err();
    assert!(unobserved.message.contains("accepted no observation"));
    case.attesters.clear();
    let uncommitted = case.check(&given).unwrap_err();
    assert!(
        uncommitted
            .message
            .contains("no authority the active workflow commits")
    );
    let unattested = case.check(&[]).unwrap_err();
    assert!(
        unattested
            .message
            .contains("commits no authority for approvals"),
        "{unattested}"
    );
    let missing = Case::new().check(&[]).unwrap_err();
    assert_eq!(missing.code, RefusalCode::GuardUnavailable);
    assert!(
        missing
            .message
            .contains("taken with an attestation from approvals")
    );
}

#[test]
fn every_attestation_given_must_be_one_the_edge_needs_and_one_per_guard() {
    let case = Case::new();
    let attestation = passing(&case.custom);
    let twice = case
        .check(&[
            sign_attestation(7, &attestation),
            sign_attestation(7, &attestation),
        ])
        .unwrap_err();
    assert!(twice.message.contains("two attestations"), "{twice}");
    let mut other = attestation.clone();
    other.guard = "elsewhere".into();
    let foreign = case.check(&[sign_attestation(7, &other)]).unwrap_err();
    assert_eq!(foreign.code, RefusalCode::UnknownId);
    // An unguarded edge refuses an attestation given for it as well.
    let unguarded = case
        .check_edge("submit", &[sign_attestation(7, &attestation)])
        .unwrap_err();
    assert_eq!(unguarded.code, RefusalCode::UnknownId);
    assert_eq!(case.check_edge("submit", &[]), Ok(Vec::new()));
}

#[test]
fn a_guard_the_definition_does_not_declare_refuses_rather_than_passing() {
    let case = Case::new();
    let mut definition = case.custom.definition.definition().clone();
    let edge = definition
        .edges
        .iter_mut()
        .find(|e| e.id == "accept")
        .unwrap();
    edge.guards = vec!["undeclared".into()];
    let edge = edge.clone();
    let binding = Binding {
        project: "sim",
        task: "sim/a",
        edge: "accept",
        workflow: &case.custom.revision.digest,
        activation_generation: 2,
        task_revision: 4,
        at: &case.at,
        input_revisions: &case.inputs,
        principal: &case.principal,
    };
    let guarding = Guarding {
        project: "sim",
        attesters: &case.attesters,
        accepted: &case.accepted,
        given: &[],
        now: 200.0,
    };
    let refused = check(&definition, &edge, &binding, &guarding).unwrap_err();
    assert!(refused.message.contains("does not declare"), "{refused}");
}

fn observation(generation: u64, state: &str, previous: Option<(&str, u64, &str)>) -> Observation {
    Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation,
        state: state.into(),
        previous: previous.map(|(epoch, generation, state)| AuthorityState {
            epoch: epoch.into(),
            generation,
            state: state.into(),
        }),
        at: 10.0,
    }
}

#[test]
fn an_authority_is_accepted_first_from_no_previous_state_and_then_only_forward() {
    let first = observation(3, "s3", None);
    let Observed::Accepted(accepted) = observe(None, &first).unwrap() else {
        panic!("the first observation is accepted");
    };
    assert_eq!(
        accepted.state(),
        AuthorityState {
            epoch: "e1".into(),
            generation: 3,
            state: "s3".into()
        }
    );
    assert!(
        observe(None, &observation(3, "s3", Some(("e1", 2, "s2"))))
            .unwrap_err()
            .contains("first observation names none")
    );
    assert_eq!(observe(Some(&accepted), &first), Ok(Observed::Same));
    let next = observation(4, "s4", Some(("e1", 3, "s3")));
    let Observed::Accepted(moved) = observe(Some(&accepted), &next).unwrap() else {
        panic!("a state following the accepted one is accepted");
    };
    assert_eq!(moved.generation, 4);
    assert!(
        observe(Some(&moved), &observation(3, "s3", None))
            .unwrap_err()
            .contains("never lowered")
    );
    // Skipping a generation the board did not accept, or naming another previous state.
    assert!(
        observe(
            Some(&accepted),
            &observation(5, "s5", Some(("e1", 4, "s4")))
        )
        .unwrap_err()
        .contains("report again from the state the board accepted")
    );
    assert!(
        observe(Some(&accepted), &observation(5, "s5", None))
            .unwrap_err()
            .contains("names none")
    );
}

#[test]
fn two_decisions_at_one_generation_conflict_until_a_later_state_follows() {
    let accepted = accepted();
    let Observed::Conflict(conflicted) = observe(
        Some(&accepted),
        &observation(3, "other", Some(("e1", 2, "s2"))),
    )
    .unwrap() else {
        panic!("another state at the accepted generation conflicts");
    };
    assert!(conflicted.conflicted);
    assert_eq!(conflicted.state, "s3", "the accepted state is kept, marked");
    // The accepted state again does not clear the conflict: only a later state does.
    let again = observe(
        Some(&conflicted),
        &observation(3, "s3", Some(("e1", 2, "s2"))),
    );
    assert!(again.unwrap_err().contains("report again from the state"));
    let itself = observe(
        Some(&conflicted),
        &observation(3, "s3", Some(("e1", 3, "s3"))),
    );
    assert!(
        itself
            .unwrap_err()
            .contains("follows only a lower generation")
    );
    let Observed::Accepted(resolved) = observe(
        Some(&conflicted),
        &observation(4, "s4", Some(("e1", 3, "s3"))),
    )
    .unwrap() else {
        panic!("a later state following the conflicted one resolves it");
    };
    assert!(!resolved.conflicted);
}

#[test]
fn a_new_epoch_starts_only_from_the_state_the_board_accepted() {
    let mut accepted = accepted();
    accepted.admitted = 9;
    let mut rotated = observation(1, "t1", Some(("e1", 3, "s3")));
    rotated.epoch = "e2".into();
    let Observed::Accepted(next) = observe(Some(&accepted), &rotated).unwrap() else {
        panic!("a new epoch from the accepted state is accepted");
    };
    assert_eq!((next.epoch.as_str(), next.generation), ("e2", 1));
    assert_eq!(next.admitted, 9, "the admissions counted are kept");
    let mut unanchored = observation(1, "t1", None);
    unanchored.epoch = "e2".into();
    assert!(observe(Some(&accepted), &unanchored).is_err());
}

#[test]
fn an_observation_verifies_only_by_its_own_context_and_key() {
    let committed = attester(7);
    let observed = observation(3, "s3", None);
    let signed = sign_observation(7, &observed);
    assert_eq!(
        verified_observation(&committed, &signed),
        Ok(observed.clone())
    );
    assert!(verified_observation(&attester(8), &signed).is_err());
    let other = Observation {
        contract: 2,
        ..observed.clone()
    };
    let contract = verified_observation(&committed, &sign_observation(7, &other)).unwrap_err();
    assert!(contract.contains("contract 2"), "{contract}");
    let as_attestation = self::signed(7, false, &signed.payload);
    assert!(verified_observation(&committed, &as_attestation).is_err());
    assert_eq!(claimed_adapter(&signed), Ok("approvals".into()));
}

#[test]
fn a_committed_key_reads_only_as_ed25519_and_lowercase_hex_of_a_point() {
    let mut key = attester(7);
    assert!(key.verifying_key().is_ok());
    key.key = key.key.replacen("ed25519:", "rsa:", 1);
    assert!(key.verifying_key().unwrap_err().contains("not ed25519:HEX"));
    let mut upper = attester(7);
    upper.key = upper.key.to_uppercase().replacen("ED25519:", "ed25519:", 1);
    assert!(upper.verifying_key().unwrap_err().contains("32 bytes"));
    let mut short = attester(7);
    short.key.truncate(20);
    assert!(short.verifying_key().unwrap_err().contains("32 bytes"));
    let mut odd = attester(7);
    odd.key.push('0');
    assert!(odd.verifying_key().unwrap_err().contains("32 bytes"));
    // About half of all 32 bytes name no point of the curve: the first such y is refused.
    let off_curve = (2u8..)
        .map(|y| {
            let mut bytes = [0u8; 32];
            bytes[0] = y;
            bytes
        })
        .find(|bytes| VerifyingKey::from_bytes(bytes).is_err())
        .unwrap();
    let mut pointless = attester(7);
    pointless.key = format!("ed25519:{}", hex(&off_curve));
    assert!((pointless.verifying_key().unwrap_err()).contains("is no Ed25519 key"));
}

#[test]
fn the_committed_authorities_name_declared_adapters_once_with_an_issuer() {
    let custom = guarded();
    let definition = &custom.definition;
    assert_eq!(check_attesters(definition, &[attester(7)]), Ok(()));
    let mut undeclared = attester(7);
    undeclared.adapter = "elsewhere".into();
    assert!(
        check_attesters(definition, &[undeclared])
            .unwrap_err()
            .contains("no adapter the workflow declares")
    );
    assert!(
        check_attesters(definition, &[attester(7), attester(8)])
            .unwrap_err()
            .contains("named twice")
    );
    let mut nameless = attester(7);
    nameless.issuer.clear();
    assert!(
        check_attesters(definition, &[nameless])
            .unwrap_err()
            .contains("no issuer")
    );
}

#[test]
fn the_project_file_commits_authorities_only_for_a_workflow_and_a_project_id() {
    let text = guarded_text(7);
    let custom = guarded();
    assert_eq!(custom.authorities, vec![attester(7)]);
    assert_eq!(custom.project.as_deref(), Some("sim"));
    let no_id = text.replacen("[project]\nid = \"sim\"\n", "", 1);
    let (definition, _) = crate::workflow::written(&no_id).unwrap();
    assert!(
        crate::workflow::authorities(&no_id, definition.as_ref())
            .unwrap_err()
            .contains("[project] id names none")
    );
    // An id the project file's rule refuses names no project, as the board's lookup reads it.
    for id in ["", " ", "Sim", "sim lab"] {
        assert_eq!(
            crate::workflow::attested_project(&format!("[project]\nid = {id:?}\n")),
            None,
            "{id:?}"
        );
    }
    assert_eq!(
        crate::workflow::attested_project("[project]\nid = \"sim\"\n").as_deref(),
        Some("sim")
    );
    let only = format!(
        "[[workflow_authorities]]\nadapter = \"approvals\"\nissuer = \"ledger\"\ncontract = 1\n\
         key = \"{}\"\n",
        attester(7).key
    );
    assert_eq!(
        crate::workflow::authorities(&only, None),
        Err("workflow_authorities: no [workflow] to guard".into())
    );
    assert_eq!(crate::workflow::authorities("", None), Ok(Vec::new()));
}

#[test]
fn a_principal_is_named_by_its_kind_issuer_and_subject() {
    let owner = attestee(&local_owner());
    assert_eq!(owner.kind, "human");
    let agent = attestee(&agent_session("amber"));
    assert_eq!(
        (agent.kind.as_str(), agent.subject.as_str()),
        ("agent", "amber")
    );
    let service = attestee(&crate::workflow::board_service("land"));
    assert_eq!(service.kind, "service");
}

fn edges_into(definition: &mut Definition, to: Pair, guards: &[&str]) {
    let mut extra = definition
        .edges
        .iter()
        .find(|e| e.id == "accept")
        .unwrap()
        .clone();
    extra.id = "shortcut".into();
    extra.from = pair("build", "working");
    extra.to = to;
    extra.guards = guards.iter().map(|g| g.to_string()).collect();
    definition.edges.push(extra);
}

#[test]
fn a_guarded_pair_is_reached_only_under_its_guards() {
    let custom = guarded();
    let base = custom.definition.definition().clone();
    assert_eq!(coverage(&base), Ok(()));
    assert_eq!(
        guarded_into(&base, &pair("verify", "accepted")),
        Some(&["approved".to_string()][..])
    );
    assert_eq!(guarded_into(&base, &pair("verify", "awaiting")), None);
    let mut around = base.clone();
    edges_into(&mut around, pair("verify", "accepted"), &[]);
    assert!(
        coverage(&around)
            .unwrap_err()
            .starts_with("unguarded route: edge shortcut enters verify/accepted under no guard")
    );
    let mut same = base.clone();
    edges_into(&mut same, pair("verify", "accepted"), &["approved"]);
    assert_eq!(coverage(&same), Ok(()));
    // A self-loop and a block's resume are no route in.
    let mut looped = base.clone();
    let mut stay = looped
        .edges
        .iter()
        .find(|e| e.id == "accept")
        .unwrap()
        .clone();
    stay.id = "stay".into();
    stay.from = pair("verify", "accepted");
    stay.guards.clear();
    looped.edges.push(stay);
    assert_eq!(coverage(&looped), Ok(()));
}

#[test]
fn the_initial_pair_or_a_recovery_target_is_never_guarded() {
    let custom = guarded();
    let base = custom.definition.definition().clone();
    let mut initial = base.clone();
    let accept = initial.edges.iter_mut().find(|e| e.id == "accept").unwrap();
    accept.to = pair("build", "available");
    accept.from = pair("verify", "awaiting");
    let claim_back = initial.edges.iter_mut().find(|e| e.id == "resume").unwrap();
    claim_back.guards = vec!["approved".into()];
    let refused = coverage(&initial).unwrap_err();
    assert!(refused.contains("is the initial pair"), "{refused}");
    // build/working's recoveries move a task to build/available; guard the edge into it.
    let mut recovered = base.clone();
    let blocked = pair("build", "blocked");
    for position in recovered.positions.iter_mut() {
        if position.at == pair("build", "working")
            && let Some(recovery) = position.recovery.as_mut()
        {
            recovery.release.target = Some(blocked.clone());
        }
    }
    let block = recovered
        .edges
        .iter_mut()
        .find(|e| e.id == "block")
        .unwrap();
    block.guards = vec!["approved".into()];
    let refused = coverage(&recovered).unwrap_err();
    assert!(
        refused.contains("the release recovery of build/working"),
        "{refused}"
    );
}

#[test]
fn a_preview_names_the_guard_an_edge_waits_on() {
    let custom = guarded();
    let definition = custom.definition.definition();
    let accept = definition.edges.iter().find(|e| e.id == "accept").unwrap();
    let refused = needs_attestation(accept).unwrap();
    assert_eq!(refused.code, RefusalCode::GuardUnavailable);
    let submit = definition.edges.iter().find(|e| e.id == "submit").unwrap();
    assert_eq!(needs_attestation(submit), None);
}

#[test]
fn a_guard_met_stands_for_a_dispatch_only_at_the_state_and_key_it_was_met_by() {
    let case = Case::new();
    let met = case
        .check(&[sign_attestation(7, &passing(&case.custom))])
        .unwrap();
    let attesters = case.attesters.clone();
    assert_eq!(unstanding(&met, &attesters, &[accepted()]), None);
    // Nothing the board accepted since the admission counts as a move: its admissions do not.
    let counted = Accepted {
        admitted: 9,
        ..accepted()
    };
    assert_eq!(unstanding(&met, &attesters, &[counted]), None);
    let said = |attesters: &[Attester], accepted: &[Accepted]| {
        unstanding(&met, attesters, accepted).expect("it no longer stands")
    };
    // A later decision: a generation on, a new epoch, or another state at the same generation.
    for later in [
        Accepted {
            generation: 4,
            state: "s4".into(),
            ..accepted()
        },
        Accepted {
            epoch: "e2".into(),
            ..accepted()
        },
        Accepted {
            state: "x3".into(),
            ..accepted()
        },
    ] {
        let why = said(&attesters, std::slice::from_ref(&later));
        assert!(
            why.starts_with("approvals decided again since approved was met at generation 3 of e1: a later decision stands"),
            "{why}"
        );
    }
    let conflicted = Accepted {
        conflicted: true,
        ..accepted()
    };
    assert!(said(&attesters, &[conflicted]).contains("in conflict at generation 3 of e1"));
    assert!(said(&attesters, &[]).contains("accepts no state from it now"));
    assert!(said(&[], &[accepted()]).contains("no longer commits"));
    // The authority's key changed with a later activation: the word it met by is not its own.
    let rekeyed = [attester(8)];
    assert!(said(&rekeyed, &[accepted()]).contains("no longer verifies by the key committed"));
    // No guard met: nothing to stand.
    assert_eq!(unstanding(&[], &[], &[]), None);
}

/// `sim/a`'s transition `revision` from `from` (none for a recovery that stays) to `to`, by `edge`.
fn moved(
    revision: u64,
    from: Option<Pair>,
    to: Pair,
    effects: Vec<Effect>,
) -> super::super::Transition {
    super::super::Transition {
        id: super::super::Transition::id_of("sim/a", revision),
        slug: "sim/a".into(),
        revision,
        generation: 1,
        digest: "d".into(),
        edge: None,
        operation: None,
        activation: None,
        from,
        to,
        resume: None,
        status: "review".into(),
        principal: local_owner(),
        effects,
        key: None,
        recovery: None,
        accepted: None,
        at: 1.0,
    }
}

#[test]
fn the_entry_into_a_pair_is_found_past_every_transition_that_stayed_in_it() {
    let (awaiting, approved, blocked, landing) = (
        pair("verify", "awaiting"),
        pair("deliver", "approved"),
        pair("build", "blocked"),
        pair("deliver", "landing"),
    );
    let block = vec![Effect::Block { save_resume: true }];
    let resume = vec![Effect::ClearBlock];
    let approve = moved(4, Some(awaiting.clone()), approved.clone(), Vec::new());
    let prepare = |revision| {
        moved(
            revision,
            Some(approved.clone()),
            landing.clone(),
            Vec::new(),
        )
    };
    // Straight from the approve to the prepare.
    let straight = [approve.clone(), prepare(5)];
    assert_eq!(entry(&straight, 5, &approved), Some(4));
    // A self-edge and a recovery that stays, recorded with no `from`.
    let stays = [
        approve.clone(),
        moved(5, Some(approved.clone()), approved.clone(), Vec::new()),
        moved(6, None, approved.clone(), Vec::new()),
        prepare(7),
    ];
    assert_eq!(entry(&stays, 7, &approved), Some(4));
    // A block from the pair, a stay while blocked, and the resume back.
    let held = [
        approve.clone(),
        moved(5, Some(approved.clone()), blocked.clone(), block.clone()),
        moved(6, None, blocked.clone(), Vec::new()),
        moved(7, Some(blocked.clone()), approved.clone(), resume.clone()),
        prepare(8),
    ];
    assert_eq!(entry(&held, 8, &approved), Some(4));
    // The latest entry is the one that counts: approved, sent back, approved again.
    let twice = [
        approve.clone(),
        moved(5, Some(approved.clone()), awaiting.clone(), Vec::new()),
        moved(6, Some(awaiting.clone()), approved.clone(), Vec::new()),
        prepare(7),
    ];
    assert_eq!(entry(&twice, 7, &approved), Some(6));
    // A record that has the task elsewhere just before, or no entry at all, finds none.
    let elsewhere = [
        approve.clone(),
        moved(5, Some(approved.clone()), awaiting.clone(), Vec::new()),
        prepare(6),
    ];
    assert_eq!(entry(&elsewhere, 6, &approved), None);
    assert_eq!(entry(&[prepare(1)], 1, &approved), None);
    let unended = [
        moved(3, Some(blocked.clone()), approved.clone(), resume),
        prepare(4),
    ];
    assert_eq!(
        entry(&unended, 4, &approved),
        None,
        "a resume with no block before it"
    );
    // Transitions at or after `before` are not read.
    assert_eq!(
        entry(&[approve, prepare(5), prepare(6)], 5, &approved),
        Some(4)
    );
}

#[test]
fn a_resume_walked_back_to_anything_but_a_block_from_the_pair_finds_no_entry() {
    let (awaiting, approved, blocked, landing) = (
        pair("verify", "awaiting"),
        pair("deliver", "approved"),
        pair("build", "blocked"),
        pair("deliver", "landing"),
    );
    // Approved, prepared, sent back, moved to the blocked pair by an edge that saves nothing, and
    // resumed at approved: what left approved before the resume is the prepare, not a block.
    let history = [
        moved(4, Some(awaiting.clone()), approved.clone(), Vec::new()),
        moved(5, Some(approved.clone()), landing.clone(), Vec::new()),
        moved(6, Some(landing.clone()), awaiting.clone(), Vec::new()),
        moved(7, Some(awaiting), blocked.clone(), Vec::new()),
        moved(8, Some(blocked), approved.clone(), vec![Effect::ClearBlock]),
        moved(9, Some(approved.clone()), landing, Vec::new()),
    ];
    assert_eq!(entry(&history, 9, &approved), None);
}
