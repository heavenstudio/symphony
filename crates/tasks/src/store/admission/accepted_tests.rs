//! Accepted outputs on a board, on both databases: the edge that accepts one names it, a dependant
//! takes the one standing and names it, a supersession refuses it until another is accepted, and
//! a supersession racing an admission lets one of them win.

use super::tests::{BACKENDS, apply, events, refused};
use super::*;
use crate::store_fixture::{Backend, Fixture, Seed};
use crate::workflow::activate::tests::{MAPPED, custom};
use crate::workflow::admit::tests::outputs;
use crate::workflow::{agent_session, local_owner};
use std::collections::BTreeMap;

/// A board with sessions amber, birch and orch, sim/up, sim/down (depending on sim/up) and
/// sim/late (depending on sim/old, closed before the activation), and the workflow whose claim
/// takes the accepted outputs of what a task depends on activated.
fn board(backend: Backend) -> Fixture {
    let seed = Seed::default()
        .session("amber")
        .session("birch")
        .session("orch")
        .task("sim/up", "ready")
        .task("sim/down", "ready")
        .task("sim/late", "ready")
        .task("sim/old", "done")
        .role("sim/up", "orch", "orchestrator")
        .role("sim/down", "orch", "orchestrator")
        .role("sim/late", "orch", "orchestrator")
        .link("sim/down", "dep", "sim/up")
        .link("sim/late", "dep", "sim/old");
    let f = Fixture::on(backend, seed);
    let workflow = custom(&format!("{}{MAPPED}", outputs()));
    f.store
        .activate_workflow(&workflow, &Host::local())
        .unwrap();
    f
}

/// The request of `edge` on `slug` from where `principal` reads it now, taking `named` outputs,
/// under a key of its own.
fn sent(
    store: &Store,
    slug: &str,
    principal: &Principal,
    edge: &str,
    named: &[(&str, &str)],
) -> OperationRequest {
    let mut request = super::tests::request(store, slug, principal, edge, "");
    request.idempotency_key = format!("{slug}@{}:{edge}", request.expected.task_revision);
    request.input_revisions = (named.iter())
        .map(|(slug, output)| (slug.to_string(), output.to_string()))
        .collect();
    request
}

fn taking(
    store: &Store,
    slug: &str,
    principal: &Principal,
    edge: &str,
    named: &[(&str, &str)],
) -> Result<OperationResult, Error> {
    apply(store, principal, &sent(store, slug, principal, edge, named))
}

/// `slug` claimed and submitted by amber and approved by the owner: the output its approval
/// accepted.
fn accepted(store: &Store, slug: &str) -> String {
    let amber = agent_session("amber");
    taking(store, slug, &amber, "claim", &[]).unwrap();
    taking(store, slug, &amber, "submit", &[]).unwrap();
    taking(store, slug, &local_owner(), "approve", &[])
        .unwrap()
        .transition
}

/// What stops birch's claim of `slug` now, as the preview says it.
fn previewed(store: &Store, slug: &str) -> Vec<RefusalCode> {
    let next = (store.workflow_actions(slug, &agent_session("birch"), &Host::local())).unwrap();
    (next.actions.into_iter())
        .find(|a| a.edge == "claim")
        .unwrap()
        .unavailable
}

/// Each transition of `slug`: its revision and the accepted output standing after it.
fn standing(f: &Fixture, slug: &str) -> Vec<(u64, Option<String>)> {
    let dump = f.store.dump().unwrap();
    (dump.workflow.iter())
        .filter(|row| row.kind == "event")
        .map(|row| serde_json::from_value::<Transition>(Value::Object(row.body.clone())).unwrap())
        .filter(|t| t.slug == slug)
        .map(|t| (t.revision, t.accepted))
        .collect()
}

fn takes(output: Option<&str>) -> Outputs {
    Outputs {
        own: None,
        takes: vec![("sim/up".into(), output.map(str::to_string))],
    }
}

#[test]
fn a_dependant_takes_the_output_standing_and_none_superseded_until_another_is_accepted() {
    for backend in BACKENDS {
        let f = board(backend);
        let (birch, owner) = (agent_session("birch"), local_owner());
        let claim =
            |named: &[(&str, &str)]| refused(taking(&f.store, "sim/down", &birch, "claim", named));
        let none_standing = "dependency: sim/down depends on the accepted output of sim/up, which \
                             has none standing: it is at build/available";
        assert_eq!(
            previewed(&f.store, "sim/down"),
            vec![RefusalCode::Dependency]
        );
        assert_eq!(claim(&[("sim/up", "sim/up@1")]), none_standing);
        assert_eq!(f.store.accepted_outputs("sim/down").unwrap(), takes(None));

        let first = accepted(&f.store, "sim/up");
        assert_eq!(first, "sim/up@4");
        assert_eq!(
            f.store.accepted_outputs("sim/up").unwrap(),
            Outputs {
                own: Some(first.clone()),
                takes: Vec::new(),
            }
        );
        assert_eq!(
            f.store.accepted_outputs("sim/down").unwrap(),
            takes(Some("sim/up@4"))
        );
        assert_eq!(previewed(&f.store, "sim/down"), vec![]);
        assert_eq!(
            claim(&[]),
            "explicit_action_required: edge claim takes the accepted output of sim/up: name the \
             one it takes, sim/up = sim/up@4"
        );

        // Reopened, its output is superseded, and none stands until another is accepted.
        taking(&f.store, "sim/up", &owner, "reopen", &[]).unwrap();
        assert_eq!(f.store.accepted_outputs("sim/down").unwrap(), takes(None));
        assert_eq!(claim(&[("sim/up", &first)]), none_standing);
        let second = accepted(&f.store, "sim/up");
        assert_eq!(second, "sim/up@8");
        assert_eq!(
            claim(&[("sim/up", &first)]),
            "stale_input: sim/down takes sim/up@4 of sim/up, whose accepted output standing is \
             sim/up@8"
        );

        // Closed, it keeps the output its approval accepted, which the dependant takes.
        taking(&f.store, "sim/up", &owner, "close", &[]).unwrap();
        let named = sent(
            &f.store,
            "sim/down",
            &birch,
            "claim",
            &[("sim/up", &second)],
        );
        assert_eq!(
            apply(&f.store, &birch, &named).unwrap().transition,
            "sim/down@2"
        );
        let receipt = (f.store.workflow_receipt(&birch, &named.idempotency_key)).unwrap();
        assert_eq!(
            receipt.unwrap().sent.input_revisions,
            BTreeMap::from([("sim/up".to_string(), second.clone())])
        );
        assert_eq!(
            standing(&f, "sim/up"),
            vec![
                (1, None),
                (2, None),
                (3, None),
                (4, Some(first.clone())),
                (5, None),
                (6, None),
                (7, None),
                (8, Some(second.clone())),
                (9, Some(second.clone())),
            ]
        );
        assert_eq!(standing(&f, "sim/down"), vec![(1, None), (2, None)]);

        // A task the board's own writers closed has no accepted output to take.
        assert_eq!(
            refused(taking(
                &f.store,
                "sim/late",
                &birch,
                "claim",
                &[("sim/old", "sim/old@1")]
            )),
            "dependency: sim/late depends on the accepted output of sim/old, which has none \
             standing: it is done"
        );
        assert_eq!(events(&f, "sim/late").len(), 1);
    }
}

/// The actions and the outputs from one read: what [`Store::workflow_actions`] and
/// [`Store::accepted_outputs`] answer apart, whoever reads, for the round trips of the actions
/// alone, and refused as the actions are.
#[test]
fn the_actions_and_the_outputs_come_from_one_read_as_each_reads_them_apart() {
    for backend in BACKENDS {
        let f = board(backend);
        let (birch, host) = (agent_session("birch"), Host::local());
        let stops = |next: &NextActions| {
            let claim = next.actions.iter().find(|a| a.edge == "claim").unwrap();
            claim.unavailable.clone()
        };
        let together = |slug: &str| f.store.workflow_actions_with_outputs(slug, &birch, &host);
        let apart = |slug: &str, principal: &Principal| {
            let next = f.store.workflow_actions(slug, principal, &host).unwrap();
            (next, f.store.accepted_outputs(slug).unwrap())
        };
        let (next, outputs) = together("sim/down").unwrap();
        assert_eq!((next.clone(), outputs.clone()), apart("sim/down", &birch));
        assert_eq!(outputs, takes(None));
        assert_eq!(stops(&next), vec![RefusalCode::Dependency]);

        let output = accepted(&f.store, "sim/up");
        let (next, outputs) = together("sim/down").unwrap();
        assert_eq!((next.clone(), outputs.clone()), apart("sim/down", &birch));
        assert_eq!(outputs, takes(Some(&output)));
        assert_eq!(stops(&next), vec![]);
        // Its own output, and no edge leaving its pair takes another's.
        let owner = local_owner();
        let up = f
            .store
            .workflow_actions_with_outputs("sim/up", &owner, &host);
        let (_, outputs) = up.clone().unwrap();
        assert_eq!(up.unwrap(), apart("sim/up", &owner));
        assert_eq!(
            outputs,
            Outputs {
                own: Some(output),
                takes: Vec::new()
            }
        );

        let requests = || f.store.database().requests();
        let before = requests();
        f.store.workflow_actions("sim/down", &birch, &host).unwrap();
        let alone = requests() - before;
        assert!(alone > 0, "the actions read the board");
        let before = requests();
        together("sim/down").unwrap();
        assert_eq!(
            requests() - before,
            alone,
            "the outputs cost no round trip of their own"
        );

        let unknown = together("sim/none");
        assert_eq!(
            unknown,
            Err(f
                .store
                .workflow_actions("sim/none", &birch, &host)
                .unwrap_err())
        );
        assert!(
            matches!(&unknown, Err(Error::Refused(why)) if why.starts_with("unknown_id")),
            "{unknown:?}"
        );
    }
}

/// On a board with no workflow active, the one read is refused as the actions are.
#[test]
fn the_one_read_is_refused_as_the_actions_are_on_a_board_no_workflow_holds() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default().task("sim/up", "ready"));
        let (owner, host) = (local_owner(), Host::local());
        let refused = f
            .store
            .workflow_actions_with_outputs("sim/up", &owner, &host);
        assert_eq!(
            refused,
            Err(f
                .store
                .workflow_actions("sim/up", &owner, &host)
                .unwrap_err())
        );
        assert!(
            matches!(&refused, Err(Error::Refused(why)) if why.starts_with("unsupported_activation")),
            "{refused:?}"
        );
    }
}

#[test]
fn a_supersession_between_an_admissions_read_and_its_write_wins_and_one_after_it_loses() {
    for backend in BACKENDS {
        let f = board(backend);
        let birch = agent_session("birch");
        let output = accepted(&f.store, "sim/up");
        let named = sent(
            &f.store,
            "sim/down",
            &birch,
            "claim",
            &[("sim/up", &output)],
        );
        let store = f.other();
        let late = f.late(move || {
            taking(&store, "sim/up", &local_owner(), "reopen", &[]).unwrap();
        });
        let before = Store::tried_again_here();
        assert_eq!(
            refused(apply(&late, &birch, &named)),
            "dependency: sim/down depends on the accepted output of sim/up, which has none \
             standing: it is at build/available"
        );
        assert!(
            Store::tried_again_here() > before,
            "the write's fence decided it again"
        );
        assert_eq!(events(&f, "sim/down").len(), 1);

        // Admitted first, it took the output standing then; the supersession after it moves its
        // own task alone.
        let output = accepted(&f.store, "sim/up");
        let named = sent(
            &f.store,
            "sim/down",
            &birch,
            "claim",
            &[("sim/up", &output)],
        );
        apply(&f.store, &birch, &named).unwrap();
        taking(&f.store, "sim/up", &local_owner(), "reopen", &[]).unwrap();
        assert_eq!(f.store.get("sim/down").unwrap().status, "in_progress");
        assert_eq!(standing(&f, "sim/up").last(), Some(&(9, None)));
    }
}

#[test]
fn an_activation_keeps_an_output_where_it_stands_and_supersedes_it_where_dependants_go_on_no_more()
{
    for backend in BACKENDS {
        let f = board(backend);
        let output = accepted(&f.store, "sim/up");
        let activated = |text: String| {
            let workflow = custom(&format!("{text}{MAPPED}"));
            f.store
                .activate_workflow(&workflow, &Host::local())
                .unwrap();
        };
        activated(outputs().replace("label = \"Approved\"", "label = \"Signed off\""));
        assert_eq!(standing(&f, "sim/up").len(), 4, "kept where it stands");
        assert_eq!(
            f.store.accepted_outputs("sim/down").unwrap(),
            takes(Some(&output))
        );
        let approved = "legacy = \"done\", satisfies_dependencies = true, completion = \"none\"";
        assert!(outputs().contains(approved));
        activated(outputs().replace(
            approved,
            "legacy = \"done\", satisfies_dependencies = false, completion = \"none\"",
        ));
        assert_eq!(standing(&f, "sim/up").last(), Some(&(5, None)));
        assert_eq!(f.store.accepted_outputs("sim/down").unwrap(), takes(None));
        assert_eq!(f.store.get("sim/up").unwrap().status, "done");
    }
}
