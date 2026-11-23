//! A task's recorded transitions, read back on both databases.

use super::*;
use crate::store_fixture::{Backend, Fixture, Seed};
use symphony_api::workflow::{Operation, Pair};
use symphony_plugin::execution::{Principal, PrincipalKind};

const BACKENDS: [Backend; 2] = [Backend::SurrealDb, Backend::PostgreSql];

/// The transition that makes `revision` of `slug`: odd revisions to build/working (in progress),
/// even ones to review/awaiting (review).
fn moved(slug: &str, revision: u64) -> Transition {
    let (phase, state, status) = if revision % 2 == 1 {
        ("build", "working", "in_progress")
    } else {
        ("review", "awaiting", "review")
    };
    Transition {
        id: Transition::id_of(slug, revision),
        slug: slug.into(),
        revision,
        generation: 1,
        digest: "d1".into(),
        edge: Some(format!("edge{revision}")),
        operation: Some(Operation::Advance),
        activation: None,
        from: None,
        to: Pair {
            phase: phase.into(),
            state: state.into(),
        },
        resume: None,
        status: status.into(),
        principal: Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
        effects: vec![],
        key: Some(format!("key{revision}")),
        recovery: None,
        accepted: None,
        at: 1000.0 + revision as f64,
    }
}

/// `slug` made and moved `count` times under a workflow; its transitions, oldest first.
fn held(f: &Fixture, slug: &str, count: u64) -> Vec<Transition> {
    f.task(slug);
    (1..=count)
        .map(|revision| {
            let transition = moved(slug, revision);
            f.store
                .record_transition(&transition, revision - 1)
                .unwrap();
            transition
        })
        .collect()
}

#[test]
fn a_task_no_workflow_held_and_a_task_not_there_have_none() {
    for backend in BACKENDS {
        let f = Fixture::new(Seed::default());
        f.task("sim/jump");
        assert_eq!(f.store.workflow_transitions("sim/jump").unwrap(), vec![]);
        assert_eq!(f.store.workflow_transitions("sim/none").unwrap(), vec![]);
    }
}

#[test]
fn a_tasks_transitions_are_its_own_by_revision_never_a_neighbours() {
    for backend in BACKENDS {
        let f = Fixture::new(Seed::default());
        // Revision 10 after 9, never after 1 as text would order it.
        let jump = held(&f, "sim/jump", 11);
        // Slugs either side of it, and one it is the start of.
        held(&f, "sim/jum", 2);
        held(&f, "sim/jump-2", 3);
        held(&f, "sim/jumq", 1);
        assert_eq!(
            f.store.workflow_transitions("sim/jump").unwrap(),
            jump,
            "{backend:?}"
        );
        let next = f.store.workflow_transitions("sim/jump-2").unwrap();
        assert_eq!(
            next.iter().map(|t| t.revision).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(next.iter().all(|t| t.slug == "sim/jump-2"));
    }
}
