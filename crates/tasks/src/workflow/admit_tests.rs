//! Admission without a board: what each refusal code stands for, who may take which edge, and
//! what an admitted operation writes.

use super::*;
use crate::store_fixture::pair;
use crate::workflow::activate::tests::{TWO, context, custom};
use crate::workflow::{Custom, agent_session, board_service, local_owner};
use std::collections::BTreeMap;
use symphony_api::workflow::{InputValue, Operation};

fn two() -> Custom {
    custom(TWO)
}

/// TWO with work approved short of acceptance: `verify/approved`, which satisfies dependants and
/// is no terminal, entered from `verify/awaiting` by the owner's `approve` and left by `reopen`
/// (back to `build/available`) or `close` (on to `verify/accepted`), and its claim depending on
/// the accepted outputs of the tasks a task depends on.
pub(crate) fn outputs() -> String {
    const APPROVED: &str = r#"{ at = { phase = "verify", state = "approved" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "done", satisfies_dependencies = true, completion = "none" },
  "#;
    const EDGES: &str = r#"{ id = "approve", label = "Approve", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "approved" }, operation = "advance", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "reopen", label = "Reopen", from = { phase = "verify", state = "approved" }, to = { phase = "build", state = "available" }, operation = "advance", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "close", label = "Close", from = { phase = "verify", state = "approved" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  "#;
    let claim = "operation = \"claim\", authority = [{ kind = \"task_role\", role = \"executor\" }], \
                 inputs = [], dependencies = \"none\"";
    assert!(TWO.contains(claim));
    TWO.replace(
        r#"{ id = "accepted", label = "Accepted" },"#,
        r#"{ id = "accepted", label = "Accepted" }, { id = "approved", label = "Approved" },"#,
    )
    .replace(
        r#"  { at = { phase = "verify", state = "accepted" }"#,
        &format!(r#"  {APPROVED}{{ at = {{ phase = "verify", state = "accepted" }}"#),
    )
    .replace(
        r#"  { id = "accept","#,
        &format!(r#"  {EDGES}{{ id = "accept","#),
    )
    .replace(claim, &claim.replace("\"none\"", "\"accepted_output\""))
}

/// A task at `phase/state`, workflow revision 4, filed by the owner, with no roles.
fn at(custom: &Custom, phase: &str, state: &str) -> Facts {
    Facts {
        slug: "sim/a".into(),
        created_by: "owner".into(),
        position: Some(TaskPosition {
            phase: phase.into(),
            state: state.into(),
            resume: None,
            generation: 2,
            revision: 4,
            digest: custom.revision.digest.clone(),
            manual: None,
        }),
        roles: Vec::new(),
        dependencies: Vec::new(),
        held: Held::default(),
        present: true,
        accepted: None,
    }
}

fn with_role(mut facts: Facts, handle: &str, role: &str) -> Facts {
    facts.roles.push((handle.into(), role.into()));
    facts
}

/// The request of `edge` from where `facts` stands, as a caller that read it sends it.
fn request(custom: &Custom, facts: &Facts, edge: &str) -> OperationRequest {
    let position = facts.position.as_ref().unwrap();
    OperationRequest {
        project: "sim".into(),
        task: facts.slug.clone(),
        edge: edge.into(),
        expected: snapshot(context(custom), position),
        input_revisions: BTreeMap::new(),
        idempotency_key: "k1".into(),
        inputs: BTreeMap::new(),
    }
}

/// What a board with no approval authority gives admission: no attestation.
fn unguarded() -> Guarding<'static> {
    Guarding {
        project: "sim",
        attesters: &[],
        accepted: &[],
        given: &[],
        now: 0.0,
    }
}

fn amber() -> Principal {
    agent_session("amber")
}

fn taking(
    custom: &Custom,
    facts: &Facts,
    principal: &Principal,
    edge: &str,
) -> Result<Admitted, Refusal> {
    let request = request(custom, facts, edge);
    admit(
        context(custom),
        facts,
        principal,
        &Host::local(),
        &request,
        &unguarded(),
        70.0,
    )
}

fn code(result: Result<Admitted, Refusal>) -> RefusalCode {
    result.expect_err("refused").code
}

fn edge_mut<'a>(definition: &'a mut Definition, id: &str) -> &'a mut Edge {
    (definition.edges.iter_mut())
        .find(|e| e.id == id)
        .expect("the fixture declares it")
}

/// TWO with `change` made to its definition, unvalidated: admission reads what activation
/// recorded, and these shapes test what admission does with each.
fn changed(change: impl FnOnce(&mut Definition)) -> (Definition, Custom) {
    let custom = two();
    let mut definition = custom.definition.definition().clone();
    change(&mut definition);
    (definition, custom)
}

fn admitted_on(
    definition: &Definition,
    custom: &Custom,
    facts: &Facts,
    principal: &Principal,
    edge: &str,
) -> Result<Admitted, Refusal> {
    let context = Context {
        definition,
        revision: &custom.revision,
        generation: 2,
    };
    let mut request = request(custom, facts, edge);
    request.expected = snapshot(context, facts.position.as_ref().unwrap());
    admit(
        context,
        facts,
        principal,
        &Host::local(),
        &request,
        &unguarded(),
        70.0,
    )
}

#[test]
fn an_executor_claims_an_available_task_taking_its_role_and_a_slot() {
    let two = two();
    let facts = at(&two, "build", "available");
    let admitted = taking(&two, &facts, &amber(), "claim").unwrap();
    assert_eq!(admitted.add, vec![("amber".into(), "executor".into())]);
    assert!(admitted.drop.is_empty());
    assert!(admitted.acquires);
    let t = &admitted.transition;
    assert_eq!(
        (t.id.as_str(), t.revision, t.generation, t.status.as_str()),
        ("sim/a@5", 5, 2, "in_progress")
    );
    assert_eq!(
        (t.from.clone(), t.to.clone(), t.resume.clone()),
        (
            Some(pair("build", "available")),
            pair("build", "working"),
            None
        )
    );
    assert_eq!(
        (t.edge.as_deref(), t.operation, t.key.as_deref()),
        (Some("claim"), Some(Operation::Claim), Some("k1"))
    );
    assert_eq!(
        (t.principal.clone(), t.activation, t.at),
        (amber(), None, 70.0)
    );
    let claim = (two.definition.definition().edges.iter()).find(|e| e.id == "claim");
    assert_eq!(Some(&t.effects), claim.map(|e| &e.effects));
    assert_eq!(t.digest, two.revision.digest);
}

#[test]
fn a_request_from_a_snapshot_that_moved_is_refused_by_what_moved() {
    let two = two();
    let facts = at(&two, "build", "available");
    let refused = |change: &dyn Fn(&mut OperationRequest)| {
        let mut request = request(&two, &facts, "claim");
        change(&mut request);
        admit(
            context(&two),
            &facts,
            &amber(),
            &Host::local(),
            &request,
            &unguarded(),
            1.0,
        )
        .unwrap_err()
    };
    let stale = refused(&|r| r.expected.workflow.digest = "other".into());
    assert_eq!(stale.code, RefusalCode::StaleWorkflow);
    assert_eq!(
        stale.to_string(),
        format!(
            "stale_workflow: the active workflow is {}, not other",
            two.revision.digest
        )
    );
    let generation = refused(&|r| r.expected.activation_generation = 1);
    assert_eq!(generation.code, RefusalCode::StaleActivation);
    let revision = refused(&|r| r.expected.task_revision = 3);
    assert_eq!(
        revision.to_string(),
        "stale_task: sim/a is at build/available (workflow revision 4), not build/available \
         (revision 3)"
    );
    let moved = refused(&|r| r.expected.at = pair("build", "working"));
    assert_eq!(moved.code, RefusalCode::StaleTask);
}

#[test]
fn an_edge_the_workflow_does_not_declare_or_that_leaves_another_pair_is_unknown() {
    let two = two();
    let facts = at(&two, "build", "available");
    let unknown = taking(&two, &facts, &amber(), "teleport").unwrap_err();
    assert_eq!(unknown.code, RefusalCode::UnknownId);
    assert!(unknown.message.ends_with("declares no edge teleport"));
    assert_eq!(
        taking(&two, &facts, &amber(), "submit")
            .unwrap_err()
            .message,
        "edge submit leaves build/working, and sim/a is at build/available"
    );
    let claim = request(&two, &facts, "claim");
    let mut free = facts;
    free.position = None;
    let refused = admit(
        context(&two),
        &free,
        &amber(),
        &Host::local(),
        &claim,
        &unguarded(),
        1.0,
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "unknown_id: the workflow holds no position of sim/a: the board's own writers keep it"
    );
}

#[test]
fn a_task_role_is_held_on_this_task_and_taken_only_when_no_one_else_holds_or_reserved_it() {
    let two = two();
    let available = at(&two, "build", "available");
    let held = with_role(available.clone(), "birch", "executor");
    assert_eq!(
        taking(&two, &held, &amber(), "claim").unwrap_err().message,
        "birch holds the executor of sim/a; edge claim needs the task's executor"
    );
    let reserved = with_role(available.clone(), "birch", "reserved");
    assert_eq!(
        taking(&two, &reserved, &amber(), "claim")
            .unwrap_err()
            .message,
        "sim/a is reserved for birch; edge claim needs the task's executor"
    );
    let mine = with_role(available.clone(), "amber", "reserved");
    let admitted = taking(&two, &mine, &amber(), "claim").unwrap();
    assert_eq!(admitted.add, vec![("amber".into(), "executor".into())]);
    assert_eq!(admitted.drop, vec![("amber".into(), "reserved".into())]);
    let mut gone = available.clone();
    gone.present = false;
    assert_eq!(
        taking(&two, &gone, &amber(), "claim").unwrap_err().message,
        "amber is no session on the board; edge claim needs the task's executor"
    );
    let working = at(&two, "build", "working");
    assert_eq!(
        taking(&two, &working, &amber(), "submit")
            .unwrap_err()
            .message,
        "amber is not the executor of sim/a; edge submit needs the task's executor"
    );
    let executing = with_role(working, "amber", "executor");
    let submitted = taking(&two, &executing, &amber(), "submit").unwrap();
    assert_eq!(submitted.transition.status, "review");
    assert!(!submitted.acquires);
}

#[test]
fn a_capability_is_held_only_by_whom_the_host_grants_it() {
    let two = two();
    let facts = at(&two, "build", "available");
    let refused = taking(&two, &facts, &amber(), "block").unwrap_err();
    assert_eq!(
        refused.to_string(),
        "unauthorized: amber may not take edge block on sim/a: it needs the project capability \
         owner"
    );
    assert_eq!(
        code(taking(&two, &facts, &board_service("recovery"), "block")),
        RefusalCode::Unauthorized
    );
    let blocked = taking(&two, &facts, &local_owner(), "block").unwrap();
    assert_eq!(blocked.transition.resume, Some(pair("build", "available")));
    assert_eq!(blocked.transition.status, "blocked");
    let mut host = Host::local();
    host.grants = host.grants.grant("owner", amber());
    let request = request(&two, &facts, "block");
    assert!(
        admit(
            context(&two),
            &facts,
            &amber(),
            &host,
            &request,
            &unguarded(),
            1.0
        )
        .is_ok()
    );
}

#[test]
fn a_human_wait_takes_a_person_whatever_capability_an_agent_or_service_was_granted() {
    let two = two();
    let awaiting = at(&two, "verify", "awaiting");
    let mut host = Host::local();
    host.grants = (host.grants)
        .grant("owner", amber())
        .grant("owner", board_service("recovery"));
    for principal in [amber(), board_service("recovery")] {
        let request = request(&two, &awaiting, "accept");
        let refused = admit(
            context(&two),
            &awaiting,
            &principal,
            &host,
            &request,
            &unguarded(),
            1.0,
        );
        assert_eq!(code(refused), RefusalCode::HumanWait);
    }
    let accepted = taking(&two, &awaiting, &local_owner(), "accept").unwrap();
    assert_eq!(accepted.transition.status, "done");
}

#[test]
fn only_an_agent_takes_a_role_and_a_capability_alone_never_gives_one() {
    let (definition, two) = changed(|d| {
        edge_mut(d, "claim")
            .authority
            .push(Authority::ProjectCapability {
                capability: "owner".into(),
            });
    });
    let facts = at(&two, "build", "available");
    let refused = admitted_on(&definition, &two, &facts, &local_owner(), "claim").unwrap_err();
    assert_eq!(
        refused.to_string(),
        "explicit_action_required: edge claim gives its taker the executor role, which only an \
         agent session holds"
    );
}

#[test]
fn a_reviewer_is_fresh_whatever_the_graph_says() {
    let (definition, two) = changed(|d| {
        let accept = edge_mut(d, "accept");
        accept.human_wait = false;
        accept.authority = vec![Authority::TaskRole {
            role: Role::Reviewer,
        }];
    });
    let awaiting = with_role(at(&two, "verify", "awaiting"), "amber", "reviewer");
    assert!(admitted_on(&definition, &two, &awaiting, &amber(), "accept").is_ok());
    let mut executed = awaiting.clone();
    executed.held.executed.insert("amber".into());
    assert_eq!(
        admitted_on(&definition, &two, &executed, &amber(), "accept")
            .unwrap_err()
            .message,
        "amber executed sim/a: its reviewer is a fresh session, one that did not write, execute \
         or request it; edge accept needs the task's reviewer"
    );
    let orchestrating = with_role(awaiting.clone(), "amber", "orchestrator");
    assert_eq!(
        code(admitted_on(
            &definition,
            &two,
            &orchestrating,
            &amber(),
            "accept"
        )),
        RefusalCode::Unauthorized
    );
    let mut filed = awaiting.clone();
    filed.created_by = "amber".into();
    assert!(
        admitted_on(&definition, &two, &filed, &amber(), "accept")
            .unwrap_err()
            .message
            .starts_with("amber filed sim/a")
    );
}

#[test]
fn a_done_dependency_is_one_the_board_closed_or_the_workflow_holds_where_dependants_go_on() {
    let (definition, two) = changed(|d| edge_mut(d, "claim").dependencies = Dependency::Done);
    let mut facts = at(&two, "build", "available");
    let claim = |facts: &Facts| admitted_on(&definition, &two, facts, &amber(), "claim");
    let managed = |phase: &str, state: &str| {
        let mut position = at(&two, phase, state).position.unwrap();
        position.revision = 9;
        Some(("review".to_string(), Some(position)))
    };
    for (found, refused) in [
        (
            None,
            Some("sim/a depends on sim/dep, which is not on the board"),
        ),
        (
            Some(("ready".to_string(), None)),
            Some("sim/a depends on sim/dep, which is ready"),
        ),
        (Some(("done".to_string(), None)), None),
        (
            managed("verify", "awaiting"),
            Some("sim/a depends on sim/dep, which is at verify/awaiting"),
        ),
        (managed("verify", "accepted"), None),
    ] {
        facts.dependencies = vec![Prerequisite {
            slug: "sim/dep".into(),
            found,
            accepted: None,
        }];
        match refused {
            Some(message) => {
                let refusal = claim(&facts).unwrap_err();
                assert_eq!(
                    (refusal.code, refusal.message.as_str()),
                    (RefusalCode::Dependency, message)
                );
            }
            None => assert!(claim(&facts).is_ok()),
        }
    }
    let (landed, two) = changed(|d| edge_mut(d, "claim").dependencies = Dependency::Landed);
    assert_eq!(
        code(admitted_on(&landed, &two, &facts, &amber(), "claim")),
        RefusalCode::UnsupportedWriter
    );
}

#[test]
fn inputs_are_the_edges_own_and_no_input_revision_is_taken_as_current() {
    let (definition, two) = changed(|d| edge_mut(d, "claim").inputs = vec!["note".into()]);
    let facts = at(&two, "build", "available");
    let context = Context {
        definition: &definition,
        revision: &two.revision,
        generation: 2,
    };
    let send = |change: &dyn Fn(&mut OperationRequest)| {
        let mut request = request(&two, &facts, "claim");
        change(&mut request);
        admit(
            context,
            &facts,
            &amber(),
            &Host::local(),
            &request,
            &unguarded(),
            1.0,
        )
    };
    let missing = send(&|_| {}).unwrap_err();
    assert_eq!(
        missing.to_string(),
        "explicit_action_required: edge claim needs the input note"
    );
    let given = |r: &mut OperationRequest| {
        r.inputs
            .insert("note".into(), InputValue::Text("why".into()));
    };
    assert!(send(&given).is_ok());
    let extra = send(&|r| {
        given(r);
        r.inputs.insert("other".into(), InputValue::Flag(true));
    });
    assert_eq!(
        extra.unwrap_err().to_string(),
        "unknown_id: edge claim declares no input other"
    );
    let revised = send(&|r| {
        given(r);
        r.input_revisions.insert("note".into(), "r1".into());
    });
    assert_eq!(code(revised), RefusalCode::StaleInput);
}

#[test]
fn a_guard_or_an_adapter_this_host_cannot_answer_refuses_rather_than_passing() {
    let (guarded, two) = changed(|d| edge_mut(d, "claim").guards = vec!["policy".into()]);
    let facts = at(&two, "build", "available");
    let refused = admitted_on(&guarded, &two, &facts, &amber(), "claim").unwrap_err();
    assert_eq!(refused.code, RefusalCode::GuardUnavailable);
    for effect in [
        Effect::Prepare {
            adapter: "lander".into(),
        },
        Effect::Finalize {
            adapter: "lander".into(),
        },
        Effect::ParentIntegration,
    ] {
        let (adapted, two) = changed(|d| edge_mut(d, "claim").effects.push(effect.clone()));
        assert_eq!(
            code(admitted_on(&adapted, &two, &facts, &amber(), "claim")),
            RefusalCode::UnsupportedAdapter
        );
    }
    let (automatic, two) = changed(|d| edge_mut(d, "claim").automatic = true);
    assert_eq!(
        code(admitted_on(&automatic, &two, &facts, &amber(), "claim")),
        RefusalCode::UnsupportedWriter
    );
}

#[test]
fn a_block_saves_where_the_task_resumes_and_a_resume_goes_back_there_alone() {
    let two = two();
    let mut blocked = at(&two, "build", "blocked");
    blocked.position.as_mut().unwrap().resume = Some(pair("build", "available"));
    let resumed = taking(&two, &blocked, &local_owner(), "resume").unwrap();
    assert_eq!(resumed.transition.resume, None);
    assert_eq!(resumed.transition.status, "ready");
    blocked.position.as_mut().unwrap().resume = Some(pair("build", "working"));
    assert_eq!(
        taking(&two, &blocked, &local_owner(), "resume")
            .unwrap_err()
            .to_string(),
        "explicit_action_required: edge resume resumes sim/a at build/available, and its block \
         saved build/working"
    );
}

#[test]
fn a_release_drops_every_holder_of_the_role_it_names() {
    let (definition, two) = changed(|d| {
        let submit = edge_mut(d, "submit");
        submit.effects.insert(
            0,
            Effect::ReleaseRole {
                role: Role::Executor,
            },
        );
    });
    let working = with_role(
        with_role(at(&two, "build", "working"), "amber", "executor"),
        "amber",
        "orchestrator",
    );
    let admitted = admitted_on(&definition, &two, &working, &amber(), "submit").unwrap();
    assert_eq!(admitted.drop, vec![("amber".into(), "executor".into())]);
    assert!(admitted.add.is_empty());
}

#[test]
fn the_next_actions_preview_each_edge_and_why_the_principal_cannot_take_it_now() {
    let two = two();
    let facts = at(&two, "build", "available");
    let preview = next_actions(context(&two), &facts, &amber(), &Host::local()).unwrap();
    assert_eq!(
        preview.snapshot,
        snapshot(context(&two), facts.position.as_ref().unwrap())
    );
    let summary = |preview: &NextActions| -> Vec<(String, Vec<RefusalCode>)> {
        (preview.actions.iter())
            .map(|a| (a.edge.clone(), a.unavailable.clone()))
            .collect()
    };
    assert_eq!(
        summary(&preview),
        vec![
            ("block".into(), vec![RefusalCode::Unauthorized]),
            ("claim".into(), vec![]),
        ]
    );
    let claim = (preview.actions.iter())
        .find(|a| a.edge == "claim")
        .unwrap();
    assert_eq!(
        (claim.destination.clone(), claim.label.as_str()),
        (pair("build", "working"), "Claim")
    );
    assert_eq!(
        claim.responsible,
        vec![Authority::TaskRole {
            role: Role::Executor
        }]
    );
    let owner = next_actions(context(&two), &facts, &local_owner(), &Host::local()).unwrap();
    assert_eq!(
        summary(&owner),
        vec![
            ("block".into(), vec![]),
            (
                "claim".into(),
                vec![
                    RefusalCode::Unauthorized,
                    RefusalCode::ExplicitActionRequired
                ]
            ),
        ]
    );
    let awaiting = at(&two, "verify", "awaiting");
    let waiting = next_actions(context(&two), &awaiting, &amber(), &Host::local()).unwrap();
    assert_eq!(
        summary(&waiting),
        vec![(
            "accept".into(),
            vec![RefusalCode::HumanWait, RefusalCode::Unauthorized]
        )]
    );
    let mut free = facts;
    free.position = None;
    assert_eq!(
        next_actions(context(&two), &free, &amber(), &Host::local())
            .unwrap_err()
            .code,
        RefusalCode::UnknownId
    );
}

#[test]
fn a_guarded_edge_is_admitted_with_its_attestation_and_says_which_guard_it_met() {
    use crate::workflow::guard::tests::{accepted, guarded, passing, sign_attestation};
    let custom = guarded();
    let facts = at(&custom, "verify", "awaiting");
    let accepted = [accepted()];
    let mut attestation = passing(&custom);
    attestation
        .input_revisions
        .insert("design".into(), "r7".into());
    let given = [sign_attestation(7, &attestation)];
    let guarding = Guarding {
        project: "sim",
        attesters: &custom.authorities,
        accepted: &accepted,
        given: &given,
        now: 200.0,
    };
    let mut request = request(&custom, &facts, "accept");
    request.input_revisions.insert("design".into(), "r7".into());
    let owner = local_owner();
    let host = Host::local();
    let admitted = admit(
        context(&custom),
        &facts,
        &owner,
        &host,
        &request,
        &guarding,
        200.0,
    )
    .unwrap();
    assert_eq!(admitted.transition.to, pair("verify", "accepted"));
    assert_eq!(admitted.guards.len(), 1);
    assert_eq!(admitted.guards[0].guard, "approved");
    assert_eq!(admitted.guards[0].input_revisions, request.input_revisions);
    // The request's own project is never the binding: the activation's is.
    let mut elsewhere = request.clone();
    elsewhere.project = "other".into();
    assert!(
        admit(
            context(&custom),
            &facts,
            &owner,
            &host,
            &elsewhere,
            &guarding,
            200.0
        )
        .is_ok()
    );
    // Without an attestation, with another input revision, or for another principal: refused.
    let bare = Guarding {
        given: &[],
        ..guarding
    };
    assert_eq!(
        code(admit(
            context(&custom),
            &facts,
            &owner,
            &host,
            &request,
            &bare,
            200.0
        )),
        RefusalCode::GuardUnavailable
    );
    let mut drifted = request.clone();
    drifted.input_revisions.insert("design".into(), "r8".into());
    assert_eq!(
        code(admit(
            context(&custom),
            &facts,
            &owner,
            &host,
            &drifted,
            &guarding,
            200.0
        )),
        RefusalCode::StaleInput
    );
    // The preview names the guard as what the edge waits on.
    let preview = next_actions(context(&custom), &facts, &owner, &host).unwrap();
    let accept = preview.actions.iter().find(|a| a.edge == "accept").unwrap();
    assert!(accept.unavailable.contains(&RefusalCode::GuardUnavailable));
    // An input revision on an edge no guard binds is still refused as unchecked.
    let working = with_role(at(&custom, "build", "working"), "amber", "executor");
    let mut on_submit = self::request(&custom, &working, "submit");
    on_submit
        .input_revisions
        .insert("design".into(), "r7".into());
    assert_eq!(
        code(admit(
            context(&custom),
            &working,
            &amber(),
            &host,
            &on_submit,
            &unguarded(),
            1.0
        )),
        RefusalCode::StaleInput
    );
}

#[test]
fn an_automatic_edge_is_the_boards_to_take_on_an_attempts_ending_never_a_callers() {
    let custom = custom(&crate::workflow::recover::attempt_tests::attempts(false));
    let facts = at(&custom, "verify", "checking");
    let service = board_service("recovery");
    let answered = Recovered {
        trigger: crate::workflow::Trigger::AttemptFailure,
        holder: "ab12".into(),
        role: "checker".into(),
        manual: false,
    };
    let observed = |edge: &str| {
        let request = request(&custom, &facts, edge);
        admit_observed(
            context(&custom),
            &facts,
            &service,
            &Host::local(),
            &request,
            &answered,
            70.0,
        )
    };
    let taken = observed("rework").unwrap();
    assert_eq!(
        (
            &taken.transition.edge,
            &taken.transition.to,
            &taken.transition.principal
        ),
        (
            &Some("rework".to_string()),
            &pair("build", "available"),
            &service
        )
    );
    assert_eq!(taken.transition.recovery, Some(answered.clone()));
    // Nor does the board take an edge a caller takes, its authority granted or not.
    let accept = observed("accept").unwrap_err();
    assert_eq!(
        (accept.code, accept.message.as_str()),
        (
            RefusalCode::UnsupportedWriter,
            "edge accept is a caller's to take: the board takes only an automatic edge"
        )
    );
    // The owner, or the service itself, asking as a caller is refused the automatic edge.
    for caller in [local_owner(), service.clone()] {
        let refused = taking(&custom, &facts, &caller, "rework").unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (
                RefusalCode::UnsupportedWriter,
                "edge rework is automatic: the board takes it, never a caller"
            )
        );
    }
    let rework = (custom.definition.definition().edges.iter())
        .find(|e| e.id == "rework")
        .unwrap();
    let previewed = blockers(
        context(&custom),
        &facts,
        &local_owner(),
        &Host::local(),
        rework,
        None,
        None,
    );
    assert!(
        previewed
            .iter()
            .any(|r| r.message == "edge rework is automatic: the board takes it, never a caller"),
        "{previewed:?}"
    );
}

#[test]
fn the_board_takes_no_guarded_edge_on_an_attempts_ending() {
    use crate::workflow::guard::tests::guarding_edge;
    let text = guarding_edge(
        &crate::workflow::recover::attempt_tests::attempts(false),
        "rework",
    );
    let custom = custom(&text);
    let facts = at(&custom, "verify", "checking");
    let answered = Recovered {
        trigger: crate::workflow::Trigger::AttemptFailure,
        holder: "ab12".into(),
        role: "checker".into(),
        manual: false,
    };
    let refused = admit_observed(
        context(&custom),
        &facts,
        &board_service("recovery"),
        &Host::local(),
        &request(&custom, &facts, "rework"),
        &answered,
        70.0,
    )
    .unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            RefusalCode::GuardUnavailable,
            "edge rework is guarded by approved: it is taken with an attestation from the \
             project's authority"
        )
    );
}

#[test]
fn a_resume_returns_a_task_to_a_guarded_pair_only_where_its_block_saved_it() {
    use crate::workflow::effect::tests::{guarded_landing_with_stays_text, landing};
    let custom = custom(&guarded_landing_with_stays_text());
    let owner = local_owner();
    let host = Host::local();
    let resuming = |custom: &Custom, facts: &Facts| {
        let request = request(custom, facts, "resume");
        admit(
            context(custom),
            facts,
            &owner,
            &host,
            &request,
            &unguarded(),
            1.0,
        )
    };
    // Nothing saved (the task came to the blocked pair by another edge, or an activation): the
    // resume would put it at approved past the approve's guard.
    let unsaved = at(&custom, "build", "blocked");
    let refused = resuming(&custom, &unsaved).unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            RefusalCode::ExplicitActionRequired,
            "edge resume resumes sim/a at deliver/approved, which only edges guarded by approved \
             enter, and no block saved it there"
        )
    );
    // Its block saved approved: the resume returns it there.
    let mut saved = unsaved.clone();
    saved.position.as_mut().unwrap().resume = Some(pair("deliver", "approved"));
    let resumed = resuming(&custom, &saved).unwrap();
    assert_eq!(resumed.transition.to, pair("deliver", "approved"));
    // A pair no guarded edge enters takes a resume with nothing saved, as before.
    let plain = landing(crate::workflow::effect::DEFAULT_LAND);
    let unguarded_resume = resuming(&plain, &at(&plain, "build", "blocked")).unwrap();
    assert_eq!(unguarded_resume.transition.to, pair("build", "available"));
}

fn with_outputs() -> Custom {
    custom(&outputs())
}

/// `sim/dep` at `phase/state`, workflow revision 9, with `accepted` its last transition left.
fn prerequisite(custom: &Custom, phase: &str, state: &str, accepted: Option<&str>) -> Prerequisite {
    let mut position = at(custom, phase, state).position.unwrap();
    position.revision = 9;
    Prerequisite {
        slug: "sim/dep".into(),
        found: Some(("done".into(), Some(position))),
        accepted: accepted.map(str::to_string),
    }
}

/// What stops amber's claim of `sim/a` now, as the preview says it: no request names an output.
fn previewed(custom: &Custom, facts: &Facts) -> Vec<RefusalCode> {
    let preview = next_actions(context(custom), facts, &amber(), &Host::local()).unwrap();
    let claim = (preview.actions.iter())
        .find(|a| a.edge == "claim")
        .unwrap();
    claim.unavailable.clone()
}

/// `sim/a`'s claim sent with `named` as the outputs it takes.
fn claiming(custom: &Custom, facts: &Facts, named: &[(&str, &str)]) -> Result<Admitted, Refusal> {
    let mut request = request(custom, facts, "claim");
    for (slug, output) in named {
        request
            .input_revisions
            .insert(slug.to_string(), output.to_string());
    }
    admit(
        context(custom),
        facts,
        &amber(),
        &Host::local(),
        &request,
        &unguarded(),
        70.0,
    )
}

#[test]
fn an_accepted_output_dependency_is_met_only_by_the_output_standing_and_named() {
    let custom = with_outputs();
    let mut facts = at(&custom, "build", "available");
    let refused = |facts: &Facts, named: &[(&str, &str)]| {
        let refusal = claiming(&custom, facts, named).unwrap_err();
        (refusal.code, refusal.message)
    };
    // None stands: a task not on the board, one the board's own writers keep (done or not), one
    // the workflow holds short of satisfying its dependants, and one at such a position that no
    // edge accepted.
    for (found, accepted, stands) in [
        (None, None, "not on the board"),
        (Some(("done".to_string(), None)), Some("sim/dep@3"), "done"),
        (Some(("ready".to_string(), None)), None, "ready"),
        (
            prerequisite(&custom, "verify", "awaiting", Some("sim/dep@3")).found,
            Some("sim/dep@3"),
            "at verify/awaiting",
        ),
        (
            prerequisite(&custom, "verify", "approved", None).found,
            None,
            "at verify/approved",
        ),
    ] {
        facts.dependencies = vec![Prerequisite {
            slug: "sim/dep".into(),
            found,
            accepted: accepted.map(str::to_string),
        }];
        assert_eq!(
            refused(&facts, &[("sim/dep", "sim/dep@3")]),
            (
                RefusalCode::Dependency,
                format!(
                    "sim/a depends on the accepted output of sim/dep, which has none standing: \
                     it is {stands}"
                )
            )
        );
        assert_eq!(
            previewed(&custom, &facts),
            vec![RefusalCode::Dependency],
            "{stands}"
        );
    }
    // One stands, at a position that satisfies dependants: taken when named, and only then.
    for (phase, state) in [("verify", "approved"), ("verify", "accepted")] {
        facts.dependencies = vec![prerequisite(&custom, phase, state, Some("sim/dep@5"))];
        assert_eq!(previewed(&custom, &facts), vec![]);
        assert!(claiming(&custom, &facts, &[("sim/dep", "sim/dep@5")]).is_ok());
        assert_eq!(
            refused(&facts, &[]),
            (
                RefusalCode::ExplicitActionRequired,
                "edge claim takes the accepted output of sim/dep: name the one it takes, \
                 sim/dep = sim/dep@5"
                    .into()
            )
        );
        for superseded in ["sim/dep@3", "sim/dep@5 ", "sim/other@5"] {
            assert_eq!(
                refused(&facts, &[("sim/dep", superseded)]),
                (
                    RefusalCode::StaleInput,
                    format!(
                        "sim/a takes {superseded} of sim/dep, whose accepted output standing is \
                         sim/dep@5"
                    )
                )
            );
        }
    }
    // Every task it depends on, each by its own output.
    let mut other = prerequisite(&custom, "verify", "approved", Some("sim/two@2"));
    other.slug = "sim/two".into();
    facts.dependencies.push(other);
    assert!(
        claiming(
            &custom,
            &facts,
            &[("sim/dep", "sim/dep@5"), ("sim/two", "sim/two@2")]
        )
        .is_ok()
    );
    assert_eq!(
        refused(&facts, &[("sim/dep", "sim/dep@5")]).0,
        RefusalCode::ExplicitActionRequired
    );
    // With no task to depend on, the predicate holds, as `done`'s does.
    facts.dependencies.clear();
    assert!(claiming(&custom, &facts, &[]).is_ok());
}

#[test]
fn an_input_revision_names_only_an_output_its_edge_takes() {
    let custom = with_outputs();
    let mut facts = at(&custom, "build", "available");
    facts.dependencies = vec![prerequisite(
        &custom,
        "verify",
        "approved",
        Some("sim/dep@5"),
    )];
    let refusal =
        claiming(&custom, &facts, &[("sim/dep", "sim/dep@5"), ("note", "r1")]).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "stale_input: the board records no revision of note: an input revision names the \
         accepted output of a task sim/a depends on, for an edge depending on accepted_output"
    );
    // An edge depending on `done` takes no output, so none is named for it.
    let mut done = custom.definition.definition().clone();
    edge_mut(&mut done, "claim").dependencies = Dependency::Done;
    let mut request = request(&custom, &facts, "claim");
    let context = Context {
        definition: &done,
        revision: &custom.revision,
        generation: 2,
    };
    assert!(
        admit(
            context,
            &facts,
            &amber(),
            &Host::local(),
            &request,
            &unguarded(),
            1.0
        )
        .is_ok()
    );
    request
        .input_revisions
        .insert("sim/dep".into(), "sim/dep@5".into());
    assert_eq!(
        admit(
            context,
            &facts,
            &amber(),
            &Host::local(),
            &request,
            &unguarded(),
            1.0
        )
        .unwrap_err()
        .code,
        RefusalCode::StaleInput
    );
}

#[test]
fn the_edge_that_accepts_an_output_names_it_by_its_transition_and_a_move_away_supersedes_it() {
    let custom = with_outputs();
    let owner = local_owner();
    let taken =
        |facts: &Facts, edge: &str| taking(&custom, facts, &owner, edge).unwrap().transition;
    // Accepted where its edge reaches a position satisfying dependants from one that is not.
    let approved = taken(&at(&custom, "verify", "awaiting"), "approve");
    assert_eq!(
        (approved.id.as_str(), approved.accepted.as_deref()),
        ("sim/a@5", Some("sim/a@5"))
    );
    // Carried by a move between two such positions, and accepted there when none stood.
    let mut standing = at(&custom, "verify", "approved");
    standing.accepted = Some("sim/a@2".into());
    assert_eq!(
        taken(&standing, "close").accepted.as_deref(),
        Some("sim/a@2")
    );
    standing.accepted = None;
    assert_eq!(
        taken(&standing, "close").accepted.as_deref(),
        Some("sim/a@5")
    );
    // Superseded by a move to a position that does not satisfy dependants.
    standing.accepted = Some("sim/a@2".into());
    assert_eq!(taken(&standing, "reopen").accepted, None);
    // A move between two that do not accepts nothing, whatever the facts say.
    let mut working = with_role(at(&custom, "build", "working"), "amber", "executor");
    working.accepted = Some("sim/a@2".into());
    let submitted = taking(&custom, &working, &amber(), "submit").unwrap();
    assert_eq!(submitted.transition.accepted, None);
}

#[test]
fn an_output_stands_after_a_move_by_where_it_goes_from_where_it_was() {
    let custom = with_outputs();
    let d = custom.definition.definition();
    let position = |phase: &str, state: &str| {
        (d.positions.iter())
            .find(|p| p.at == pair(phase, state))
            .unwrap()
    };
    let (open, approved, accepted) = (
        position("build", "available"),
        position("verify", "approved"),
        position("verify", "accepted"),
    );
    let after = |source: Option<&Position>, destination, standing, minted| {
        accepted_after(source, destination, standing, minted)
    };
    assert_eq!(
        after(Some(open), approved, None, Some("t@2")).as_deref(),
        Some("t@2")
    );
    assert_eq!(
        after(Some(open), approved, Some("t@1"), Some("t@2")).as_deref(),
        Some("t@2")
    );
    assert_eq!(after(Some(open), approved, Some("t@1"), None), None);
    assert_eq!(
        after(None, approved, Some("t@1"), Some("t@2")).as_deref(),
        Some("t@2")
    );
    assert_eq!(
        after(Some(approved), accepted, Some("t@1"), Some("t@2")).as_deref(),
        Some("t@1")
    );
    assert_eq!(after(Some(approved), accepted, None, None), None);
    assert_eq!(after(Some(approved), open, Some("t@1"), Some("t@2")), None);
    assert_eq!(after(Some(open), open, Some("t@1"), Some("t@2")), None);
}
