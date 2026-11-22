//! The owner's guarded operation taken with what the project's approval authority answers, on a
//! scratch board: the authority is asked for the operation's own snapshot and the owner, and the
//! board verifies its answer as it verifies a pushed one, so a pass is taken and an answer signed by
//! another key, for another task or with no signature is refused as `guard_unavailable`. An
//! authority that errs, panics or does not answer in time refuses the operation and moves nothing;
//! a guard no plugin answers is the board's to refuse as before, and attestations given are
//! pushed as before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use symphony_api::workflow::{
    ATTESTATION_CONTRACT, Attestation, Attested, Observation, OperationRequest, Pair, Source,
};
use symphony_board::workflow::guard::{attestee, signed_message};
use symphony_board::workflow::{Attester, Custom, Host, local_owner, written};
use symphony_plugin::Plugin;
use symphony_plugin::authority::{Operation, Signed};

use super::*;
use crate::taskview::page_tests::store_fixture::{Fixture, HELD_FLOW, Seed, take_edge};

/// The seed of the key the project commits for its authority.
pub(super) const KEY: u8 = 7;

/// How a stand-in authority answers.
#[derive(Clone, Copy, PartialEq)]
enum Answering {
    /// A pass of the operation it was asked, signed by `seed`'s key.
    Pass(u8),
    /// A pass for another task than the one asked.
    OtherTask,
    /// A pass with no signature.
    Unsigned,
    /// Why it cannot say.
    Closed,
    /// A panic.
    Broken,
    /// Nothing, until the test lets it.
    Stuck,
}

/// The project's approval authority for `approvals`, as its plugin answers for it.
struct Ledger(Answering);

/// The operations each authority was asked, in order, with how it answers: the tests share it.
static ASKED: Mutex<Vec<(Answering, Operation)>> = Mutex::new(Vec::new());

/// Whether a stuck authority may answer, set once the test is done with it.
static LET_GO: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn signing(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// `payload` signed by `seed`'s key as an observation or an attestation.
pub(super) fn signed(seed: u8, observation: bool, payload: &str) -> symphony_api::workflow::Signed {
    let signature = signing(seed).sign(&signed_message(observation, payload));
    symphony_api::workflow::Signed {
        payload: payload.into(),
        signature: hex(&signature.to_bytes()),
    }
}

/// The pass the ledger gives `operation` at its generation 1 of e1, issued now for ten minutes.
pub(super) fn pass(operation: &Operation) -> Attestation {
    let issued = crate::save::epoch_now() - 1.0;
    Attestation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation: 1,
        project: operation.project.clone(),
        task: operation.task.clone(),
        guard: operation.guard.clone(),
        edge: operation.edge.clone(),
        workflow: operation.snapshot.workflow.clone(),
        activation_generation: operation.snapshot.activation_generation,
        task_revision: operation.snapshot.task_revision,
        at: Pair {
            phase: operation.snapshot.phase.clone(),
            state: operation.snapshot.state.clone(),
        },
        input_revisions: operation.input_revisions.clone(),
        principal: attestee(&operation.principal),
        result: Attested::Pass,
        facts: vec!["decision-1".into()],
        reasons: Vec::new(),
        issued_at: issued,
        expires_at: issued + 600.0,
    }
}

impl Authority for Ledger {
    fn adapter(&self) -> &str {
        "approvals"
    }
    fn attest(&self, _: &Path, operation: &Operation, _: Duration) -> Result<Signed, String> {
        ASKED.lock().unwrap().push((self.0, operation.clone()));
        let mut attestation = pass(operation);
        let seed = match self.0 {
            Answering::Pass(seed) => seed,
            Answering::OtherTask => {
                attestation.task = "sim/other".into();
                KEY
            }
            Answering::Unsigned => {
                let payload = serde_json::to_string(&attestation).unwrap();
                return Ok(Signed::new(payload, String::new()));
            }
            Answering::Closed => return Err("the ledger is closed for the night".into()),
            Answering::Broken => panic!("the ledger's file is gone"),
            Answering::Stuck => {
                let let_go = LET_GO.0.lock().unwrap();
                drop(LET_GO.1.wait_while(let_go, |go| !*go).unwrap());
                return Err("let go".into());
            }
        };
        let signed = signed(seed, false, &serde_json::to_string(&attestation).unwrap());
        Ok(Signed::new(signed.payload, signed.signature))
    }
}

/// A plugin of the project answering for its approval authority.
pub(super) struct Approvals(Ledger);

impl Plugin for Approvals {
    fn name(&self) -> &str {
        "approvals"
    }
    fn authorities(&self) -> Vec<&dyn Authority> {
        vec![&self.0]
    }
}

pub(super) static PASSING: Approvals = Approvals(Ledger(Answering::Pass(KEY)));
/// Passing too, for the test that shows it is never asked.
static UNASKED: Approvals = Approvals(Ledger(Answering::Pass(8)));
pub(super) static OTHER_KEY: Approvals = Approvals(Ledger(Answering::Pass(9)));
pub(super) static OTHER_TASK: Approvals = Approvals(Ledger(Answering::OtherTask));
static UNSIGNED: Approvals = Approvals(Ledger(Answering::Unsigned));
pub(super) static CLOSED: Approvals = Approvals(Ledger(Answering::Closed));
static BROKEN: Approvals = Approvals(Ledger(Answering::Broken));
static STUCK: Approvals = Approvals(Ledger(Answering::Stuck));

pub(super) fn plugins(plugin: &'static Approvals) -> Plugins {
    Plugins::new(vec![plugin])
}

/// What the authority answering as `answering` was asked.
fn asked_of(answering: Answering) -> Vec<Operation> {
    let asked = ASKED.lock().unwrap();
    (asked.iter())
        .filter(|(how, _)| *how == answering)
        .map(|(_, operation)| operation.clone())
        .collect()
}

/// [`HELD_FLOW`] with its acceptance guarded by `approved`, which the `approvals` authority
/// attests, in the project `sim`, the key of [`KEY`] committed for it.
pub(super) fn guarded() -> Custom {
    let text = HELD_FLOW
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
    assert!(text.contains("guards = [\"approved\"]"), "{text}");
    // A ready task the board's writers kept starts available.
    let text = format!(
        "{text}\n[[workflow_mappings]]\nfrom = {{ phase = \"ready\", state = \"available\" }}\n\
         to = {{ phase = \"build\", state = \"available\" }}\n"
    );
    let (definition, mappings) = written(&text).unwrap();
    let definition = definition.unwrap();
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings,
        authorities: vec![Attester {
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            contract: ATTESTATION_CONTRACT,
            key: format!("ed25519:{}", hex(signing(KEY).verifying_key().as_bytes())),
        }],
        project: Some("sim".into()),
    }
}

/// The guarded board with sim/a claimed and submitted by amber, at verify/awaiting, its authority's
/// generation 1 of e1 accepted.
pub(super) fn awaiting() -> Fixture {
    let f = Fixture::new(Seed::default().session("amber").task("sim/a", "ready"));
    f.store
        .activate_workflow(&guarded(), &Host::local())
        .unwrap();
    take_edge(&f.store, "sim/a", "amber", "claim");
    take_edge(&f.store, "sim/a", "amber", "submit");
    let observation = Observation {
        contract: ATTESTATION_CONTRACT,
        adapter: "approvals".into(),
        issuer: "ledger".into(),
        epoch: "e1".into(),
        generation: 1,
        state: "s1".into(),
        previous: None,
        at: 1.0,
    };
    let observed = signed(KEY, true, &serde_json::to_string(&observation).unwrap());
    f.store.observe_authority(&observed).unwrap();
    f
}

/// The owner's request of `edge` on sim/a from where it stands now.
pub(super) fn request(f: &Fixture, edge: &str) -> OperationRequest {
    let next = (f.store)
        .workflow_actions("sim/a", &local_owner(), &Host::local())
        .unwrap();
    OperationRequest {
        project: f.store.database().database().to_string(),
        task: "sim/a".into(),
        edge: edge.into(),
        input_revisions: BTreeMap::from([("design".to_string(), "r1".to_string())]),
        idempotency_key: format!("sim/a@{}:{edge}", next.snapshot.task_revision),
        inputs: BTreeMap::new(),
        expected: next.snapshot,
    }
}

/// `request` taken by the owner as the save takes it, `plugin` answering for the project, within
/// `within`: the fetch, then the board's admission. A refusal's words.
fn take(
    f: &Fixture,
    plugin: &'static Approvals,
    request: &OperationRequest,
    within: Duration,
) -> Result<(), String> {
    let owner = local_owner();
    let root = PathBuf::from("/project");
    let fetched = fetched_within(&f.store, &plugins(plugin), &root, request, &owner, within)
        .map_err(|refused| format!("{} {}", refused.failure.status, refused.failure.error))?;
    (f.store
        .apply_attested(&owner, &Host::local(), request, &fetched))
    .map(drop)
    .map_err(|e| e.to_string())
}

pub(super) fn at(f: &Fixture) -> Pair {
    f.store.workflow_position("sim/a").unwrap().unwrap().at()
}

#[test]
fn a_guarded_edge_is_taken_with_the_pass_its_authority_answers_for_the_operation_and_owner() {
    let f = awaiting();
    let request = request(&f, "accept");
    take(&f, &PASSING, &request, WITHIN).unwrap();
    assert_eq!(at(&f).state, "accepted");
    // Only this test asks the passing ledger, once.
    let asked = asked_of(Answering::Pass(KEY));
    let [operation] = asked.as_slice() else {
        panic!("the passing ledger was asked {asked:?}");
    };
    assert_eq!(
        (
            operation.project.as_str(),
            operation.task.as_str(),
            operation.edge.as_str(),
            operation.guard.as_str()
        ),
        ("sim", "sim/a", "accept", "approved")
    );
    assert_eq!(
        (
            operation.snapshot.workflow.as_str(),
            operation.snapshot.activation_generation,
            operation.snapshot.task_revision,
            operation.snapshot.phase.as_str(),
            operation.snapshot.state.as_str()
        ),
        (
            request.expected.workflow.digest.as_str(),
            request.expected.activation_generation,
            request.expected.task_revision,
            "verify",
            "awaiting"
        )
    );
    assert_eq!(operation.input_revisions, request.input_revisions);
    assert_eq!(operation.principal, local_owner());
}

#[test]
fn an_answer_signed_by_another_key_for_another_task_or_unsigned_is_refused_by_the_board() {
    let f = awaiting();
    let request = request(&f, "accept");
    for (plugin, why) in [
        (&OTHER_KEY, "does not verify"),
        (
            &OTHER_TASK,
            "is for task sim/other, and the operation's is sim/a",
        ),
        (&UNSIGNED, "does not verify"),
    ] {
        let refused = take(&f, plugin, &request, WITHIN).unwrap_err();
        assert!(
            refused.starts_with("guard_unavailable: ") && refused.contains(why),
            "{refused}"
        );
        assert_eq!(at(&f).state, "awaiting", "{refused}");
    }
}

#[test]
fn an_authority_that_errs_panics_or_does_not_answer_refuses_the_operation_and_moves_nothing() {
    let f = awaiting();
    let request = request(&f, "accept");
    let said = |plugin, within| take(&f, plugin, &request, within).unwrap_err();
    let guard = "400 guard_unavailable: edge accept's guard approved: its authority approvals gave \
                 no attestation: ";
    assert_eq!(
        said(&CLOSED, WITHIN),
        format!("{guard}the ledger is closed for the night")
    );
    assert_eq!(
        said(&BROKEN, WITHIN),
        format!("{guard}approvals panicked attesting: the ledger's file is gone")
    );
    // The authority answers only once the test lets it: the wait's deadline is what ends it.
    let stuck = said(&STUCK, Duration::from_millis(50));
    *LET_GO.0.lock().unwrap() = true;
    LET_GO.1.notify_all();
    assert_eq!(stuck, format!("{guard}it did not answer within 50ms"));
    assert_eq!(at(&f).state, "awaiting");
}

#[test]
fn with_no_plugin_answering_the_board_refuses_the_guard_and_a_pushed_attestation_still_passes() {
    let f = awaiting();
    let request = request(&f, "accept");
    let owner = local_owner();
    let root = PathBuf::from("/project");
    let none = Plugins::new(Vec::new());
    let fetched = fetched_within(&f.store, &none, &root, &request, &owner, WITHIN)
        .unwrap_or_else(|refused| panic!("{}", refused.failure.error));
    assert!(fetched.is_empty());
    let refused = (f.store)
        .apply_attested(&owner, &Host::local(), &request, &fetched)
        .unwrap_err()
        .to_string();
    assert!(
        refused.starts_with("guard_unavailable: ") && refused.contains("approvals"),
        "{refused}"
    );
    // The authority's word pushed by hand, as before.
    let operation = super::operation("sim", "approved", &request, &owner);
    let pushed = signed(
        KEY,
        false,
        &serde_json::to_string(&pass(&operation)).unwrap(),
    );
    (f.store)
        .apply_attested(&owner, &Host::local(), &request, &[pushed])
        .unwrap();
    assert_eq!(at(&f).state, "accepted");
}

#[test]
fn an_unguarded_edge_or_a_board_with_no_workflow_asks_no_authority() {
    let f = Fixture::new(Seed::default().session("amber").task("sim/b", "ready"));
    let owner = local_owner();
    let root = PathBuf::from("/project");
    let mut request = OperationRequest {
        project: f.store.database().database().to_string(),
        task: "sim/b".into(),
        edge: "accept".into(),
        input_revisions: BTreeMap::new(),
        idempotency_key: "k".into(),
        inputs: BTreeMap::new(),
        expected: symphony_api::workflow::PositionSnapshot {
            workflow: guarded().revision,
            activation_generation: 1,
            task_revision: 1,
            at: Pair {
                phase: "verify".into(),
                state: "awaiting".into(),
            },
        },
    };
    let fetch = |request: &OperationRequest| {
        fetched_within(&f.store, &plugins(&UNASKED), &root, request, &owner, WITHIN)
            .map_err(|refused| refused.failure.error)
    };
    assert_eq!(fetch(&request), Ok(Vec::new()), "no workflow is active");
    f.store
        .activate_workflow(&guarded(), &Host::local())
        .unwrap();
    for edge in ["claim", "nowhere"] {
        request.edge = edge.into();
        assert_eq!(fetch(&request), Ok(Vec::new()), "{edge}");
    }
    assert_eq!(asked_of(Answering::Pass(8)), Vec::new());
}
