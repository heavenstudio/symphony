//! The roll-up's read of where a custom workflow put each task of its window
//! ([`Store::report_places`]), on both databases: none, with nothing asked of the board, where the
//! roll-up's one read says it runs no workflow; each task's recorded transitions, named by the definition each was made under, on one
//! that does.

use super::*;
use crate::store_fixture::{Backend, Fixture, Seed};
use crate::sweep::trigger::now;
use crate::workflow::activate::tests::{MAPPED, TWO, custom};
use crate::workflow::{Committed, Host};
use symphony_api::workflow::{Operation, Pair, Phase, State};
use symphony_plugin::execution::{Principal, PrincipalKind};

const BACKENDS: [Backend; 2] = [Backend::SurrealDb, Backend::PostgreSql];

fn board() -> Fixture {
    let seed = Seed::default()
        .task("sim/ready", "ready")
        .task("sim/review", "review")
        .task("sim/done", "done");
    Fixture::new(seed)
}

/// Each place of `slug` in `read` as `(phase, phase label, state, state label, status)`.
fn places<'a>(
    read: &'a ReportRead,
    slug: &str,
) -> Vec<(&'a str, &'a str, &'a str, &'a str, &'a str)> {
    let at = (read.tasks.iter())
        .position(|task| task.slug == slug)
        .expect(slug);
    read.places.as_ref().expect("a board that runs a workflow")[at]
        .iter()
        .map(|p| {
            (
                p.phase.as_str(),
                p.phase_label.as_str(),
                p.state.as_str(),
                p.state_label.as_str(),
                p.status.as_str(),
            )
        })
        .collect()
}

#[test]
fn a_board_that_runs_no_workflow_is_asked_nothing_more() {
    for backend in BACKENDS {
        let f = board();
        let log = f.store.database().queries();
        let (first, _) = log.report(now());
        let read = f.store.report_records(0.0).unwrap();
        let (second, _) = log.report(now());
        assert_eq!(second.count - first.count, 1, "{backend:?}: one read");
        assert_eq!((read.workflow, read.places.as_ref()), (false, None));
        let (before, writes_before) = log.report(now());
        let placed = f.store.report_places(read.clone()).unwrap();
        let (after, writes_after) = log.report(now());
        assert_eq!(placed, read, "{backend:?}");
        assert_eq!(
            (
                after.count - before.count,
                writes_after.count - writes_before.count
            ),
            (0, 0),
            "{backend:?}: no request, no write"
        );
    }
}

#[test]
fn each_task_s_places_are_its_own_transitions_named_by_the_definition_each_was_made_under() {
    for backend in BACKENDS {
        let f = board();
        let two = custom(&format!("{TWO}{MAPPED}"));
        let committed = Committed {
            commit: "3".repeat(40),
            custom: Some(two.clone()),
            returning: Vec::new(),
        };
        f.store
            .follow_committed(&committed, &Host::local())
            .unwrap();
        // A later move under a definition the board never recorded: its IDs name it.
        let mut later = f.store.workflow_transitions("sim/ready").unwrap()[0].clone();
        later.revision = 2;
        later.id = Transition::id_of("sim/ready", 2);
        later.digest = "f".repeat(64);
        later.edge = Some("claim".into());
        later.operation = Some(Operation::Claim);
        later.activation = None;
        later.from = Some(later.to.clone());
        later.to = Pair {
            phase: "build".into(),
            state: "working".into(),
        };
        later.status = "in_progress".into();
        later.principal = Principal::new(PrincipalKind::Agent, "local".into(), "exe".into());
        later.at += 60.0;
        f.store.record_transition(&later, 1).unwrap();

        let read = f.store.report_records(0.0).unwrap();
        assert!(read.workflow, "{backend:?}: the one read says it runs one");
        let read = f.store.report_places(read).unwrap();
        assert_eq!(
            read.places.as_ref().map(Vec::len),
            Some(read.tasks.len()),
            "{backend:?}: a list for each task, in their order"
        );
        assert_eq!(
            places(&read, "sim/ready"),
            [
                ("build", "Build", "available", "Available", "ready"),
                ("build", "build", "working", "working", "in_progress"),
            ],
            "{backend:?}"
        );
        assert_eq!(
            places(&read, "sim/review"),
            [("verify", "Verify", "awaiting", "Awaiting", "review")],
            "{backend:?}"
        );
        if read.tasks.iter().any(|task| task.slug == "sim/done") {
            assert_eq!(places(&read, "sim/done"), [], "{backend:?}: never placed");
        }
        let froms = places_from(&read, "sim/ready");
        assert!(froms[0] < froms[1], "{backend:?}: oldest first");
    }
}

fn places_from(read: &ReportRead, slug: &str) -> Vec<f64> {
    let at = (read.tasks.iter())
        .position(|task| task.slug == slug)
        .unwrap();
    read.places.as_ref().unwrap()[at]
        .iter()
        .map(|p| p.from)
        .collect()
}

/// A definition naming `phases` and `states`, `(id, label)` each.
fn named(phases: &[(&str, &str)], states: &[(&str, &str)]) -> Definition {
    let mut definition = crate::workflow::builtin();
    definition.phases = (phases.iter().zip(0..))
        .map(|((id, label), order)| Phase {
            id: (*id).into(),
            label: (*label).into(),
            order,
            phone_order: order,
            column: true,
        })
        .collect();
    definition.states = (states.iter())
        .map(|(id, label)| State {
            id: (*id).into(),
            label: (*label).into(),
        })
        .collect();
    definition
}

#[test]
fn a_place_is_named_by_its_own_digest_s_definition_and_an_id_none_names_reads_as_itself() {
    let board = || Transition {
        id: Transition::id_of("sim/a", 1),
        slug: "sim/a".into(),
        revision: 1,
        generation: 1,
        digest: "d1".into(),
        edge: Some("claim".into()),
        operation: Some(Operation::Claim),
        activation: None,
        from: None,
        to: Pair {
            phase: "build".into(),
            state: "working".into(),
        },
        resume: None,
        status: "in_progress".into(),
        principal: Principal::new(PrincipalKind::Agent, "local".into(), "exe".into()),
        effects: vec![],
        key: None,
        recovery: None,
        accepted: None,
        at: 0.0,
    };
    let mut first = board();
    first.at = 10.0;
    let mut renamed = board();
    renamed.revision = 2;
    renamed.digest = "d2".into();
    renamed.at = 20.0;
    let mut unknown = board();
    unknown.revision = 3;
    unknown.digest = "d2".into();
    unknown.to = Pair {
        phase: "verify".into(),
        state: "awaiting".into(),
    };
    unknown.at = 30.0;
    let mut gone = board();
    gone.revision = 4;
    gone.digest = "gone".into();
    gone.at = 40.0;
    let definitions = [
        (
            "d1".to_string(),
            Some(named(&[("build", "Build")], &[("working", "Working")])),
        ),
        (
            "d2".to_string(),
            Some(named(&[("build", "Make")], &[("working", "Making")])),
        ),
        ("gone".to_string(), None),
    ];
    let named: Vec<(String, String, f64)> = placed(&[first, renamed, unknown, gone], &definitions)
        .into_iter()
        .map(|p| (p.phase_label, p.state_label, p.from))
        .collect();
    assert_eq!(
        named,
        [
            ("Build".into(), "Working".into(), 10.0),
            ("Make".into(), "Making".into(), 20.0),
            ("verify".into(), "awaiting".into(), 30.0),
            ("build".into(), "working".into(), 40.0),
        ]
    );
    assert_eq!(placed(&[], &definitions), []);
}
