//! The external-effect boundary's decisions, without a board: what a reservation going stale
//! says, what an effect that happened owes a person, and which edges are an adapter's.

use super::*;
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::{Custom, agent_session};

/// TWO, with a delivery phase after its review: work awaiting review is approved by a fresh
/// reviewer naming the head it read, landed by the default landing's adapter (prepared, then
/// confirmed), and may be sent back from its prepared pair.
pub(crate) const LANDING: &str = include_str!("../../tests/fixtures/landing_workflow.toml");

/// LANDING as committed, for `adapter` in the default landing's place.
pub(crate) fn landing(adapter: &str) -> Custom {
    custom(&format!("{LANDING}{MAPPED}").replace(DEFAULT_LAND, adapter))
}

/// [`landing`] with its approve guarded by `approved`, which the `approvals` authority attests by
/// the key of `seed`, in the project `sim`: the land's prepare leaves the pair that guard brings
/// the task into.
pub(crate) fn guarded_landing(adapter: &str, seed: u8) -> Custom {
    custom(&guarded_landing_text(adapter, seed))
}

/// [`guarded_landing_text`] where a task stays in its approved pair: its block and resume taken
/// from the approved pair and back to it, and a reviewer staffing it, whose release keeps the task
/// there.
pub(crate) fn guarded_landing_with_stays_text() -> String {
    let text = guarded_landing_text(DEFAULT_LAND, 7);
    let at = "  { at = { phase = \"deliver\", state = \"approved\" },";
    let start = text.find(at).expect("the approved position");
    let end = start + text[start..].find('\n').expect("its line ends");
    let stay =
        "{ manual = true, slot = \"retain\", release_role = true, release_reservation = false }";
    let wait =
        "{ manual = true, slot = \"retain\", release_role = false, release_reservation = false }";
    let staffed = format!(
        "{at} staffing = \"reviewer\", slot = \"release\", dependencies = \"none\", \
         unsuccessful_dependency = \"block\", accepts_work = false, terminal = false, legacy = \
         \"done\", satisfies_dependencies = false, completion = \"none\", recovery = {{ release = \
         {stay}, lost_holder = {stay}, attempt_failure = {wait}, cancellation_acknowledged = {wait} }} }},"
    );
    let text = format!("{}{staffed}{}", &text[..start], &text[end..]);
    let (available, approved) = (
        "{ phase = \"build\", state = \"available\" }",
        "{ phase = \"deliver\", state = \"approved\" }",
    );
    let (block, resume) = (
        format!("{{ id = \"block\", label = \"Block\", from = {available},"),
        format!("state = \"blocked\" }}, to = {available}, operation = \"resume\","),
    );
    assert!(
        text.contains(&block) && text.contains(&resume),
        "the block and its resume"
    );
    (text.replacen(&block, &block.replace(available, approved), 1)).replacen(
        &resume,
        &resume.replace(available, approved),
        1,
    )
}

/// [`guarded_landing`]'s text.
pub(crate) fn guarded_landing_text(adapter: &str, seed: u8) -> String {
    let approve = "inputs = [\"head\"], dependencies = \"none\", guards = []";
    let text = format!("{LANDING}{MAPPED}")
        .replace(DEFAULT_LAND, adapter)
        .replacen("adapters = [", "adapters = [\"approvals\", ", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        );
    assert_eq!(text.matches(approve).count(), 2, "approve and prepare_land");
    let guarded = text.replacen(
        &format!("operation = \"approve\", authority = [{{ kind = \"task_role\", role = \"reviewer\" }}], {approve}"),
        "operation = \"approve\", authority = [{ kind = \"task_role\", role = \"reviewer\" }], \
         inputs = [\"head\"], dependencies = \"none\", guards = [\"approved\"]",
        1,
    );
    assert_ne!(guarded, text, "the fixture guards approve");
    format!(
        "{guarded}\n[project]\nid = \"sim\"\n\n[[workflow_authorities]]\nadapter = \"approvals\"\n\
         issuer = \"ledger\"\ncontract = 1\nkey = \"{}\"\n",
        crate::workflow::guard::tests::attester(seed).key
    )
}

pub(crate) fn bindings(head: &str) -> Bindings {
    Bindings {
        project: "board".into(),
        task: "sim/a".into(),
        repository: "/repo/.git".into(),
        pull: None,
        head: head.into(),
        policy: "policy-1".into(),
        evidence: "evidence-1".into(),
        destination: "refs/heads/main".into(),
    }
}

fn intent() -> Intent {
    Intent {
        id: "sim/a@5".into(),
        slug: "sim/a".into(),
        revision: 5,
        adapter: DEFAULT_LAND.into(),
        key: "land:abc:4".into(),
        bindings: bindings("abc"),
        generation: 2,
        digest: "d2".into(),
        principal: agent_session("orch"),
        stage: Stage::Prepared,
        receipt: None,
        finalized: None,
        recovery: None,
        withdrawn: None,
        at: 10.0,
    }
}

fn active(generation: u64, digest: &str) -> Active {
    Active {
        generation,
        digest: digest.into(),
    }
}

fn position(revision: u64) -> TaskPosition {
    TaskPosition {
        phase: "deliver".into(),
        state: "landing".into(),
        resume: None,
        generation: 2,
        revision,
        digest: "d2".into(),
        manual: None,
    }
}

fn outcome(policy: &str, evidence: &str) -> Outcome {
    Outcome {
        receipt: "merge1".into(),
        policy: policy.into(),
        evidence: evidence.into(),
    }
}

#[test]
fn the_landing_definition_is_valid_and_the_local_host_supports_it() {
    for adapter in [DEFAULT_LAND, PROJECT_LAND] {
        let custom = landing(adapter);
        crate::workflow::activate::supported(&custom.definition, &crate::workflow::Host::local())
            .unwrap();
    }
}

#[test]
fn each_binding_that_moved_is_named_with_both_values_and_equal_bindings_name_none() {
    let bound = bindings("abc");
    assert_eq!(bound.moved(&bound), None);
    type Change = fn(&mut Bindings);
    let cases: [(&str, Change); 8] = [
        ("project", |b| b.project = "other".into()),
        ("task", |b| b.task = "sim/b".into()),
        ("repository", |b| b.repository = "/else".into()),
        ("pull request", |b| b.pull = Some("#7".into())),
        ("head", |b| b.head = "def".into()),
        ("landing policy", |b| b.policy = "policy-2".into()),
        ("evidence", |b| b.evidence = "evidence-2".into()),
        ("destination", |b| b.destination = "refs/heads/next".into()),
    ];
    for (name, change) in cases {
        let mut now = bound.clone();
        change(&mut now);
        let said = bound.moved(&now).unwrap();
        assert!(said.starts_with(&format!("its {name} was ")), "{said}");
    }
    let mut now = bound.clone();
    now.head = "def".into();
    assert_eq!(
        bound.moved(&now).unwrap(),
        "its head was abc when prepared and is def"
    );
}

#[test]
fn a_reservation_stands_only_under_its_workflow_at_its_revision_with_its_bindings() {
    let intent = intent();
    let (same, here) = (active(2, "d2"), position(5));
    assert_eq!(
        stale(&intent, Some(&same), Some(&here), Some(&bindings("abc"))),
        None
    );
    assert_eq!(stale(&intent, Some(&same), Some(&here), None), None);
    let none = stale(&intent, None, Some(&here), None).unwrap();
    assert!(none.contains("no custom workflow is active"), "{none}");
    // The prepared generation is said first, then the one active now.
    assert_eq!(
        stale(&intent, Some(&active(3, "d2")), Some(&here), None).unwrap(),
        "the workflow activated again since it was prepared (generation 2 when prepared, 3 now)"
    );
    let digest = stale(&intent, Some(&active(2, "d3")), Some(&here), None).unwrap();
    assert!(digest.contains("activated again"), "{digest}");
    let unheld = stale(&intent, Some(&same), None, None).unwrap();
    assert_eq!(unheld, "no workflow holds sim/a now");
    let moved = stale(&intent, Some(&same), Some(&position(6)), None).unwrap();
    assert!(
        moved.contains("moved on to deliver/landing (workflow revision 6, prepared at 5)"),
        "{moved}"
    );
    let head = stale(&intent, Some(&same), Some(&here), Some(&bindings("def"))).unwrap();
    assert_eq!(head, "its head was abc when prepared and is def");
}

#[test]
fn an_effect_owes_a_recovery_when_its_policy_evidence_workflow_or_position_changed() {
    let intent = intent();
    let (same, here) = (active(2, "d2"), position(5));
    let stood = outcome("policy-1", "evidence-1");
    assert_eq!(requirement(&intent, &stood, Some(&same), Some(&here)), None);
    let policy = requirement(
        &intent,
        &outcome("policy-2", "evidence-1"),
        Some(&same),
        Some(&here),
    );
    assert_eq!(
        policy.unwrap(),
        "the landing policy was policy-1 when prepared and policy-2 when the effect happened"
    );
    let evidence = requirement(
        &intent,
        &outcome("policy-1", "evidence-2"),
        Some(&same),
        Some(&here),
    );
    assert!(evidence.unwrap().starts_with("the evidence was evidence-1"));
    let all = requirement(
        &intent,
        &outcome("p", "e"),
        Some(&active(3, "d3")),
        Some(&position(6)),
    )
    .unwrap();
    assert_eq!(all.matches("; ").count(), 2, "{all}");
    assert!(all.contains("activated again"), "{all}");
    let moved = requirement(&intent, &stood, Some(&same), Some(&position(6))).unwrap();
    assert!(moved.contains("moved on"), "{moved}");
}

#[test]
fn an_adapters_edges_are_found_by_its_effect_at_the_pair_they_leave() {
    let custom = landing(DEFAULT_LAND);
    let d = custom.definition.definition();
    let pair = |phase: &str, state: &str| Pair {
        phase: phase.into(),
        state: state.into(),
    };
    let (approved, landing) = (pair("deliver", "approved"), pair("deliver", "landing"));
    assert_eq!(
        prepare_edge(d, &approved, DEFAULT_LAND).unwrap().id,
        "prepare_land"
    );
    assert_eq!(
        finalize_edge(d, &landing, DEFAULT_LAND).unwrap().id,
        "confirm_land"
    );
    assert!(prepare_edge(d, &landing, DEFAULT_LAND).is_none());
    assert!(finalize_edge(d, &approved, DEFAULT_LAND).is_none());
    assert!(prepare_edge(d, &approved, PROJECT_LAND).is_none());
    assert!(finalize_edge(d, &landing, PROJECT_LAND).is_none());
    let effects = |id: &str| &d.edges.iter().find(|e| e.id == id).unwrap().effects;
    assert_eq!(external(effects("prepare_land")), Some(DEFAULT_LAND));
    assert_eq!(external(effects("confirm_land")), Some(DEFAULT_LAND));
    assert_eq!(external(effects("approve")), None);
}

#[test]
fn a_stage_says_its_word_as_the_record_does() {
    for stage in [
        Stage::Prepared,
        Stage::Dispatched,
        Stage::Finalized,
        Stage::Withdrawn,
    ] {
        assert_eq!(serde_json::to_value(stage).unwrap(), stage.as_str());
    }
}

#[test]
fn a_landings_adapter_is_taken_by_the_land_command_and_another_by_none() {
    for adapter in [DEFAULT_LAND, PROJECT_LAND] {
        assert_eq!(
            taken_by(adapter, "sim/a").as_deref(),
            Some("`just land sim/a`")
        );
    }
    assert_eq!(taken_by("deployer", "sim/a"), None);
}
