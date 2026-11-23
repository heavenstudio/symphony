//! Admission on a board: an operation admitted and written with its history,
//! its key, and every fence that refuses or decides it again.

use super::*;
use crate::store_fixture::{Fixture, Seed};
pub(super) use crate::store_fixture::{pair, refused};
use crate::workflow::activate::tests::{MAPPED, TWO, custom};
use crate::workflow::{Custom, agent_session, local_owner};
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use symphony_api::workflow::Pair;
/// TWO, its claim depending on done work when `depends`.
fn two(depends: bool) -> Custom {
    let text = format!("{TWO}{MAPPED}");
    custom(&if depends {
        text.replacen(
            "operation = \"claim\", authority = [{ kind = \"task_role\", role = \"executor\" }], \
             inputs = [], dependencies = \"none\"",
            "operation = \"claim\", authority = [{ kind = \"task_role\", role = \"executor\" }], \
             inputs = [], dependencies = \"done\"",
            1,
        )
    } else {
        text
    })
}

/// A board with sessions amber and birch, sim/a and sim/b ready and sim/busy in progress, the
/// three orchestrated by orch, sim/a depending on sim/dep (done) and sim/b on sim/busy, sim/gone
/// cancelled, and `custom` activated.
pub(super) fn board(custom: &Custom) -> Fixture {
    let seed = Seed::default()
        .session("amber")
        .session("birch")
        .session("orch")
        .task("sim/a", "ready")
        .task("sim/b", "ready")
        .task("sim/busy", "in_progress")
        .task("sim/dep", "done")
        .task("sim/gone", "cancelled")
        .role("sim/a", "orch", "orchestrator")
        .role("sim/b", "orch", "orchestrator")
        .role("sim/busy", "orch", "orchestrator")
        .link("sim/a", "dep", "sim/dep")
        .link("sim/b", "dep", "sim/busy");
    let f = Fixture::new(seed);
    f.store.activate_workflow(custom, &Host::local()).unwrap();
    f
}

/// The request of `edge` on `slug` from where `principal` reads it now, under `key`.
pub(super) fn request(
    store: &Store,
    slug: &str,
    principal: &Principal,
    edge: &str,
    key: &str,
) -> OperationRequest {
    let next = store
        .workflow_actions(slug, principal, &Host::local())
        .unwrap();
    OperationRequest {
        project: store.database().database().into(),
        task: slug.into(),
        edge: edge.into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: key.into(),
        inputs: BTreeMap::new(),
    }
}

pub(super) fn apply(
    store: &Store,
    principal: &Principal,
    request: &OperationRequest,
) -> Result<OperationResult, Error> {
    store.apply_operation(principal, &Host::local(), request)
}

pub(super) fn take(
    store: &Store,
    slug: &str,
    principal: &Principal,
    edge: &str,
) -> Result<OperationResult, Error> {
    let request = request(store, slug, principal, edge, &format!("{slug}:{edge}"));
    apply(store, principal, &request)
}

pub(super) fn roles(f: &Fixture, slug: &str) -> Vec<(String, String)> {
    let task = f.store.get(slug).unwrap();
    let mut roles: Vec<_> = (task.roles.iter())
        .map(|r| (r.handle.clone(), r.role.clone()))
        .collect();
    roles.sort();
    roles
}

pub(super) fn events(f: &Fixture, slug: &str) -> Vec<(u64, Option<String>, Pair)> {
    let dump = f.store.dump().unwrap();
    (dump.workflow.iter())
        .filter(|row| row.kind == "event")
        .map(|row| serde_json::from_value::<Transition>(Value::Object(row.body.clone())).unwrap())
        .filter(|t| t.slug == slug)
        .map(|t| (t.revision, t.edge, t.to))
        .collect()
}

use crate::workflow::Transition;

#[test]
fn an_agent_claims_and_submits_and_the_owner_accepts_each_move_on_the_record() {
    let f = board(&two(false));
    let amber = agent_session("amber");
    let claimed = take(&f.store, "sim/a", &amber, "claim").unwrap();
    assert_eq!(claimed.transition, "sim/a@2");
    assert_eq!(claimed.next.snapshot.at, pair("build", "working"));
    assert_eq!(claimed.next.snapshot.task_revision, 2);
    let submit = (claimed.next.actions.iter())
        .find(|a| a.edge == "submit")
        .unwrap();
    assert!(submit.unavailable.is_empty(), "{submit:?}");
    let task = f.store.get("sim/a").unwrap();
    assert_eq!(task.status, "in_progress");
    assert_eq!(
        roles(&f, "sim/a"),
        vec![
            ("amber".into(), "executor".into()),
            ("orch".into(), "orchestrator".into())
        ]
    );
    take(&f.store, "sim/a", &amber, "submit").unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "review");
    let owner = local_owner();
    let accepted = take(&f.store, "sim/a", &owner, "accept").unwrap();
    assert!(accepted.next.actions.is_empty());
    let position = f.store.workflow_position("sim/a").unwrap().unwrap();
    assert_eq!(
        (position.at(), position.revision),
        (pair("verify", "accepted"), 4)
    );
    assert_eq!(f.store.get("sim/a").unwrap().status, "done");
    assert_eq!(
        events(&f, "sim/a"),
        vec![
            (1, None, pair("build", "available")),
            (2, Some("claim".into()), pair("build", "working")),
            (3, Some("submit".into()), pair("verify", "awaiting")),
            (4, Some("accept".into()), pair("verify", "accepted")),
        ]
    );
    let said = f.said("sim/a");
    assert!(
        said.contains(
            &"took claim (sim/a@2): build/available → build/working, workflow revision 2"
                .to_string()
        ),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|line| line.starts_with("took accept (sim/a@4)"))
    );
}

#[test]
fn the_workflow_read_names_the_active_definition_and_none_before_one_is_active() {
    let f = Fixture::new(Seed::default().task("sim/a", "ready"));
    assert_eq!(f.store.workflow_read().unwrap(), None);
    assert_eq!(
        refused(
            f.store
                .workflow_actions("sim/a", &local_owner(), &Host::local())
        ),
        "unsupported_activation: no custom workflow is active on this board: the board's own \
         writers keep its tasks"
    );
    let custom = two(false);
    f.store.activate_workflow(&custom, &Host::local()).unwrap();
    let read = f.store.workflow_read().unwrap().unwrap();
    assert_eq!(read.revision, custom.revision);
    assert_eq!(&read.definition, custom.definition.definition());
    assert_eq!(read.activation_generation, 1);
    assert_eq!(
        refused(
            f.store
                .workflow_actions("sim/none", &local_owner(), &Host::local())
        ),
        "unknown_id: no task sim/none"
    );
}

#[test]
fn a_repeated_key_answers_its_first_result_and_another_request_under_it_is_refused() {
    let f = board(&two(false));
    let amber = agent_session("amber");
    let claim = request(&f.store, "sim/a", &amber, "claim", "k");
    assert_eq!(f.store.workflow_receipt(&amber, "k").unwrap(), None);
    let first = apply(&f.store, &amber, &claim).unwrap();
    let again = apply(&f.store, &amber, &claim).unwrap();
    assert_eq!(first, again);
    let receipt = f.store.workflow_receipt(&amber, "k").unwrap().unwrap();
    assert_eq!(
        (receipt.sent, receipt.transition, receipt.principal),
        (claim.clone(), "sim/a@2".to_string(), amber.clone())
    );
    assert_eq!(events(&f, "sim/a").len(), 2);
    let submit = request(&f.store, "sim/a", &amber, "submit", "k");
    assert_eq!(
        refused(apply(&f.store, &amber, &submit)),
        "explicit_action_required: the key k was used for another request (sim/a@2); a new \
         request takes a new key"
    );
    // A key is its principal's: another's same key is its own.
    let birch = agent_session("birch");
    let other = request(&f.store, "sim/b", &birch, "claim", "k");
    assert_eq!(
        apply(&f.store, &birch, &other).unwrap().transition,
        "sim/b@2"
    );
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
}

#[test]
fn a_request_from_a_revision_another_moved_past_is_stale_and_two_from_one_make_one_transition() {
    let f = board(&two(false));
    let (amber, birch) = (agent_session("amber"), agent_session("birch"));
    let late = request(&f.store, "sim/a", &birch, "claim", "b");
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    assert!(refused(apply(&f.store, &birch, &late)).starts_with(
        "stale_task: sim/a is at build/working (workflow revision 2), not build/available"
    ));
    let gate = Arc::new(Barrier::new(2));
    let racers: Vec<_> = [("amber", amber), ("birch", birch)]
        .into_iter()
        .map(|(key, principal)| {
            let store = f.other();
            let request = request(&store, "sim/b", &principal, "claim", key);
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                gate.wait();
                store.apply_operation(&principal, &Host::local(), &request)
            })
        })
        .collect();
    let results: Vec<_> = racers.into_iter().map(|r| r.join().unwrap()).collect();
    let won: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
    assert_eq!(won.len(), 1, "{results:?}");
    let lost = results.into_iter().find(|r| r.is_err()).unwrap();
    assert!(refused(lost).starts_with("stale_task: sim/b is at build/working"));
    assert_eq!(events(&f, "sim/b").len(), 2);
    assert_eq!(
        roles(&f, "sim/b")
            .iter()
            .filter(|(_, r)| r == "executor")
            .count(),
        1
    );
}

#[test]
fn a_principal_without_the_role_or_the_capability_or_on_another_board_moves_nothing() {
    let f = board(&two(false));
    let amber = agent_session("amber");
    let before = f.store.dump().unwrap();
    let mut elsewhere = request(&f.store, "sim/a", &amber, "claim", "k");
    elsewhere.project = "elsewhere".into();
    assert_eq!(
        refused(apply(&f.store, &amber, &elsewhere)),
        format!(
            "unknown_id: the request is for project elsewhere, and this is the board of {}",
            f.store.database().database()
        )
    );
    assert_eq!(
        refused(take(&f.store, "sim/a", &amber, "block")),
        "unauthorized: amber may not take edge block on sim/a: it needs the project \
         capability owner"
    );
    assert!(
        refused(take(&f.store, "sim/a", &agent_session("nobody"), "claim"))
            .starts_with("unauthorized: nobody is no session on the board")
    );
    assert_eq!(f.store.dump().unwrap(), before);
    take(&f.store, "sim/a", &amber, "claim").unwrap();
    assert!(
        refused(take(&f.store, "sim/a", &agent_session("birch"), "submit"))
            .starts_with("unauthorized: birch is not the executor of sim/a")
    );
}

#[test]
fn a_dependency_is_checked_in_the_write_and_one_that_moved_after_the_read_is_decided_again() {
    let f = board(&two(true));
    let amber = agent_session("amber");
    let request = request(&f.store, "sim/a", &amber, "claim", "k");
    let store = f.other();
    let late = f.late(move || {
        store.set_priority("sim/dep", 3, "owner").unwrap();
    });
    let before = Store::tried_again_here();
    apply(&late, &amber, &request).unwrap();
    assert!(Store::tried_again_here() > before);
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
    assert_eq!(
        refused(take(&f.store, "sim/b", &amber, "claim")),
        "dependency: sim/b depends on sim/busy, which is at build/working"
    );
    assert_eq!(events(&f, "sim/b").len(), 1);
}

#[test]
fn a_claim_past_the_cap_is_refused_as_capacity_in_the_write_and_moves_nothing() {
    let f = board(&two(false));
    let amber = agent_session("amber");
    let capped = f.other().capping(Some(1));
    let refusal = refused(take(&capped, "sim/a", &amber, "claim"));
    assert!(refusal.starts_with("capacity: "), "{refusal}");
    assert_eq!(f.store.get("sim/a").unwrap().status, "ready");
    assert_eq!(
        roles(&f, "sim/a"),
        vec![("orch".into(), "orchestrator".into())]
    );
    let roomy = f.other().capping(Some(2));
    take(&roomy, "sim/a", &amber, "claim").unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
}

#[test]
fn the_board_writers_neither_move_nor_staff_a_task_the_workflow_holds() {
    let f = board(&two(false));
    let explicit = |result: Result<crate::Task, Error>| {
        let why = refused(result);
        assert!(why.starts_with("explicit_action_required: "), "{why}");
    };
    explicit(f.store.claim("sim/a", "amber"));
    explicit(f.store.move_to("sim/a", "in_progress", "orch"));
    explicit(f.store.take_role("sim/a", "birch", "reviewer"));
    explicit(f.store.move_to("sim/a", "cancelled", "owner"));
    // A reservation is the board's coordination, as a launch makes it: it stands on held
    // work, admission refuses another's claim while it is live, and the reserver's own claim
    // consumes it.
    f.store.reserve("sim/a", "birch", 60.0).unwrap();
    let reserved = take(&f.store, "sim/a", &agent_session("amber"), "claim");
    assert!(refused(reserved).contains("sim/a is reserved for birch"));
    // Lapsed, it stands against nothing: the claim that takes the edge drops it.
    f.advance(61.0);
    for handle in ["amber", "birch", "orch"] {
        f.store.touch(handle).unwrap();
    }
    take(&f.store, "sim/a", &agent_session("amber"), "claim").unwrap();
    assert!(!(f.store.get("sim/a").unwrap().roles.iter()).any(|r| r.role == "reserved"));
    // A lost executor is no takeover's: its position's recovery answers it.
    f.advance(crate::health::LIVE_SECONDS + crate::health::STALE_SECONDS + 1.0);
    f.store.touch("birch").unwrap();
    f.store.touch("orch").unwrap();
    explicit(f.store.claim("sim/a", "birch"));
    // The orchestrator's role is the board's coordination, which its writers keep.
    f.task("sim/c");
    f.store.take_role("sim/c", "birch", "orchestrator").unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
    assert_eq!(events(&f, "sim/a").len(), 2);
    // A release is the position's recovery (store::admission::recovery), not a refusal.
}

#[test]
fn a_task_filed_under_the_workflow_starts_at_its_initial_pair_on_the_record() {
    let f = board(&two(false));
    let filed = f.task("sim/new");
    assert_eq!(filed.status, "ready");
    let position = f.store.workflow_position("sim/new").unwrap().unwrap();
    assert_eq!(
        (position.at(), position.revision, position.generation),
        (pair("build", "available"), 1, 1)
    );
    assert_eq!(
        events(&f, "sim/new"),
        vec![(1, None, pair("build", "available"))]
    );
    let claimed = take(&f.store, "sim/new", &agent_session("amber"), "claim").unwrap();
    assert_eq!(claimed.transition, "sim/new@2");
    let mut elsewhere = crate::NewTask::new("sim/backlog", "Later", "owner");
    elsewhere.status = "backlog".into();
    assert_eq!(
        refused(f.store.create(&elsewhere)),
        "explicit_action_required: a new task starts at the workflow's initial pair \
         build/available (ready), not backlog: file it there and move it on by an edge"
    );
    assert!(f.store.get("sim/backlog").is_err());
    // A prerequisite cancelled before the filing is its workflow's to answer, by its edges'
    // dependency predicates: the board's own block leaves it at its initial pair.
    let mut after = crate::NewTask::new("sim/after", "After", "owner");
    after.depends = vec!["sim/gone".into()];
    assert_eq!(f.store.create(&after).unwrap().status, "ready");
    assert_eq!(
        events(&f, "sim/after"),
        vec![(1, None, pair("build", "available"))]
    );
}

#[test]
fn a_task_closed_before_the_activation_is_placed_by_the_move_that_reopens_it() {
    // sim/late approved by birch before any workflow is active: done, with no position.
    let seed = Seed::default()
        .session("birch")
        .session("orch")
        .task("sim/late", "review")
        .role("sim/late", "birch", "reviewer")
        .role("sim/late", "orch", "orchestrator")
        .task("sim/gone", "cancelled");
    let f = Fixture::new(seed);
    (f.store)
        .verdict("sim/late", "birch", "approve", "good")
        .unwrap();
    f.store
        .activate_workflow(&custom(&format!("{TWO}{MAPPED}")), &Host::local())
        .unwrap();
    assert_eq!(f.store.workflow_position("sim/late").unwrap(), None);
    // Its reviewer holds it still, so it reopens at built-in v1's held review, which nothing
    // maps: the move is refused, naming the pair, and writes nothing.
    let before = f.store.dump().unwrap();
    assert_eq!(
        refused(f.store.move_to("sim/late", "review", "birch")),
        "workflow_mappings: sim/late reopens at review/held, which the workflow no longer \
         declares; map it to a pair it declares"
    );
    assert_eq!(f.store.dump().unwrap(), before);
    // Mapped by the next workflow commit, the move places it in its own write.
    let held = r#"
[[workflow_mappings]]
from = { phase = "review", state = "held" }
to = { phase = "verify", state = "awaiting" }
"#;
    // The mappings live in the project file, so a commit of them is a file of another blob.
    let mut remapped = custom(&format!("{TWO}{MAPPED}{held}"));
    let mut source = remapped.revision.source.clone();
    source.project_blob = "3".repeat(40);
    remapped.revision = remapped.definition.revision(source).unwrap();
    let generation = f
        .store
        .activate_workflow(&remapped, &Host::local())
        .unwrap();
    assert!(
        matches!(
            generation,
            crate::store::workflow::Followed::Activated(crate::store::workflow::Active {
                generation: 2,
                ..
            })
        ),
        "{generation:?}"
    );
    let task = f.store.move_to("sim/late", "review", "birch").unwrap();
    assert_eq!(task.status, "review");
    let position = f.store.workflow_position("sim/late").unwrap().unwrap();
    assert_eq!(
        (position.at(), position.revision, position.generation),
        (pair("verify", "awaiting"), 1, 2)
    );
    let event = (f.store.raw())
        .row("workflow", json!(["event", "sim/late", 1]))
        .get()
        .unwrap();
    let placed: Transition = serde_json::from_value(event["body"].clone()).unwrap();
    assert_eq!(
        (
            placed.from,
            placed.principal,
            placed.edge,
            placed.activation
        ),
        (
            Some(pair("done", "approved")),
            agent_session("birch"),
            None,
            None
        )
    );
    // Held now: the board's writers refuse it.
    assert_eq!(
        refused(f.store.move_to("sim/late", "in_progress", "birch")),
        crate::store::workflow::explicit_action("sim/late")
    );
    // A write past them meets the database's fence: a cancelled task with no position leaves
    // cancelled only placed.
    assert_eq!(
        refused(
            f.store
                .raw()
                .row("task", "sim/gone")
                .set(json!({"status": "ready"}))
        ),
        "explicit_action_required: sim/gone has no workflow position, and a custom workflow \
         is active: it leaves cancelled only by a write that places it at its mapped pair"
    );
    assert_eq!(f.store.get("sim/gone").unwrap().status, "cancelled");
}
