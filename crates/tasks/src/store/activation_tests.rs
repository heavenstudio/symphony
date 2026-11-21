//! Automatic activation on a board, on both databases: what it writes, what it leaves, and the
//! fences that refuse it whole.

use super::*;
use crate::health::{LIVE_SECONDS, STALE_SECONDS};
use crate::store_fixture::{Backend, Fixture, Seed, task};
use crate::workflow::activate::tests::{BACK, MAPPED, TWO, custom};
use crate::workflow::tests::Scratch;
use crate::workflow::{Committed, Custom, Host, follow};
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

const BACKENDS: [Backend; 2] = [Backend::SurrealDb, Backend::PostgreSql];

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn refused(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Refused(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A board of one task in each open status, a blocked one that came from in_progress, and a done
/// and a cancelled one the activation leaves to the board's writers.
fn board(backend: Backend) -> Fixture {
    let mut blocked = task("sim/blocked", "blocked");
    blocked.id = 9;
    blocked.fields = BTreeMap::from([(
        "_board".to_string(),
        BTreeMap::from([(
            "blocked".to_string(),
            json!({"on": "task:sim/ready", "by": "owner", "at": 1.0, "from": "in_progress"}),
        )]),
    )]);
    let seed = [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "cancelled",
    ]
    .iter()
    .fold(Seed::default(), |seed, status| {
        seed.task(&format!("sim/{status}"), status)
    })
    .task_row(blocked);
    Fixture::on(backend, seed)
}

fn mapped() -> Custom {
    custom(&format!("{TWO}{MAPPED}"))
}

/// `custom` as if committed at `commit`, its revision recomputed from that commit.
fn at_commit(custom: &Custom, commit: char) -> Custom {
    let mut source = custom.revision.source.clone();
    source.commit = commit.to_string().repeat(40);
    Custom {
        revision: custom.definition.revision(source).unwrap(),
        ..custom.clone()
    }
}

fn committed(custom: Option<Custom>) -> Committed {
    Committed {
        commit: "3".repeat(40),
        custom,
        returning: Vec::new(),
    }
}

fn status(f: &Fixture, slug: &str) -> String {
    f.store.get(slug).unwrap().status
}

#[test]
fn a_project_that_commits_no_workflow_writes_nothing() {
    for backend in BACKENDS {
        let f = board(backend);
        let before = f.store.dump().unwrap();
        assert_eq!(
            f.store.follow_committed(&committed(None), &Host::local()),
            Ok(Followed::Legacy)
        );
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");
    }
}

#[test]
fn a_first_activation_places_every_open_task_and_records_why() {
    for backend in BACKENDS {
        let f = board(backend);
        let two = mapped();
        let followed = f
            .store
            .follow_committed(&committed(Some(two.clone())), &Host::local());
        let active = Active {
            generation: 1,
            digest: two.revision.digest.clone(),
        };
        assert_eq!(
            followed,
            Ok(Followed::Activated(active.clone())),
            "{backend:?}"
        );
        assert_eq!(f.store.workflow_active().unwrap(), Some(active.clone()));
        let recorded = f
            .store
            .workflow_definition(&active.digest)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.definition, *two.definition.definition());
        assert_eq!(recorded.revision, two.revision);

        for (slug, at, resume, projected) in [
            ("sim/backlog", pair("build", "available"), None, "ready"),
            ("sim/concept", pair("build", "available"), None, "ready"),
            ("sim/design", pair("build", "available"), None, "ready"),
            ("sim/ready", pair("build", "available"), None, "ready"),
            (
                "sim/in_progress",
                pair("build", "working"),
                None,
                "in_progress",
            ),
            ("sim/review", pair("verify", "awaiting"), None, "review"),
            (
                "sim/blocked",
                pair("build", "blocked"),
                Some(pair("build", "working")),
                "blocked",
            ),
        ] {
            let position = f.store.workflow_position(slug).unwrap().expect(slug);
            assert_eq!(
                (position.at(), position.resume.clone()),
                (at, resume),
                "{slug}"
            );
            assert_eq!(
                (
                    position.generation,
                    position.revision,
                    position.digest.as_str()
                ),
                (1, 1, active.digest.as_str())
            );
            assert_eq!(status(&f, slug), projected, "{backend:?} {slug}");
        }
        // Closed tasks the board's writers kept stay theirs.
        for slug in ["sim/done", "sim/cancelled"] {
            assert_eq!(f.store.workflow_position(slug).unwrap(), None);
        }
        let raw = f.store.raw();
        let event = raw
            .row("workflow", json!(["event", "sim/backlog", 1]))
            .get()
            .unwrap();
        let transition: Transition = serde_json::from_value(event["body"].clone()).unwrap();
        assert_eq!(transition.from, Some(pair("backlog", "available")));
        assert_eq!(transition.activation, Some(1));
        assert_eq!(
            transition.principal,
            crate::workflow::board_service("activation")
        );
        let activation = raw.row("workflow", json!(["activation", 1])).get().unwrap();
        assert_eq!(activation["body"]["moved"], json!(7));
        assert_eq!(activation["body"]["previous"], Value::Null);
        assert_eq!(
            activation["body"]["mappings"],
            serde_json::to_value(&two.mappings).unwrap()
        );
        // The same commit again is the active one: nothing more is written.
        let after = f.store.dump().unwrap();
        assert_eq!(
            f.store
                .follow_committed(&committed(Some(two)), &Host::local()),
            Ok(Followed::Unchanged(active))
        );
        assert_eq!(f.store.dump().unwrap(), after);
    }
}

/// A mapping of built-in v1's held review, which [`MAPPED`] leaves out.
const HELD: &str = r#"
[[workflow_mappings]]
from = { phase = "review", state = "held" }
to = { phase = "verify", state = "awaiting" }
"#;

#[test]
fn only_a_commit_that_changes_the_workflow_activates_it() {
    for backend in BACKENDS {
        let f = board(backend);
        let first = at_commit(&mapped(), 'a');
        f.store.activate_workflow(&first, &Host::local()).unwrap();
        let active = Active {
            generation: 1,
            digest: first.revision.digest.clone(),
        };
        // Another commit of the same workflow (any landing): the generation, every task's version
        // and every record stay as they were, and so does what a request was read from.
        let before = f.store.dump().unwrap();
        let read = f.store.workflow_read().unwrap();
        let position = f.store.workflow_position("sim/ready").unwrap();
        assert_eq!(
            f.store
                .activate_workflow(&at_commit(&mapped(), 'b'), &Host::local()),
            Ok(Followed::Unchanged(active.clone())),
            "{backend:?}"
        );
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");
        assert_eq!(f.store.workflow_read().unwrap(), read);
        // A commit of other mappings, or of another definition (a label), changes the workflow:
        // each is a generation, which moves no task its definition still declares.
        let events = || {
            (f.store.raw().table("workflow"))
                .matching(json!({"kind": "event"}))
                .count()
                .unwrap()
        };
        let recorded = events();
        let remapped = at_commit(&custom(&format!("{TWO}{MAPPED}{HELD}")), 'c');
        assert_eq!(
            f.store.activate_workflow(&remapped, &Host::local()),
            Ok(Followed::Activated(Active {
                generation: 2,
                digest: remapped.revision.digest.clone()
            }))
        );
        // The server's read, kept until the board's marker moves, carries the generation.
        assert_eq!(f.store.clock_board().unwrap().workflow_generation, Some(2));
        let relabelled = TWO.replace(r#"label = "Build""#, r#"label = "Make""#);
        assert_ne!(relabelled, TWO);
        let relabelled = at_commit(&custom(&format!("{relabelled}{MAPPED}{HELD}")), 'd');
        assert_eq!(
            f.store.activate_workflow(&relabelled, &Host::local()),
            Ok(Followed::Activated(Active {
                generation: 3,
                digest: relabelled.revision.digest.clone()
            }))
        );
        assert_eq!(f.store.workflow_position("sim/ready").unwrap(), position);
        assert_eq!(events(), recorded);
        assert_eq!(
            f.store.clock_board().unwrap().workflow_generation,
            Some(3),
            "a relabel moves no task, and its read is read again: {backend:?}"
        );
        let activation = f
            .store
            .raw()
            .row("workflow", json!(["activation", 3]))
            .get()
            .unwrap();
        assert_eq!(activation["body"]["moved"], json!(0), "{backend:?}");
        assert_eq!(activation["body"]["previous"]["generation"], json!(2));
        // The same definition and mappings again, from yet another commit: still generation 3.
        assert_eq!(
            f.store
                .activate_workflow(&at_commit(&relabelled, 'e'), &Host::local()),
            Ok(Followed::Unchanged(Active {
                generation: 3,
                digest: relabelled.revision.digest.clone()
            }))
        );
    }
}

#[test]
fn a_reactivation_maps_a_removed_pair_and_its_resume_pairs() {
    for backend in BACKENDS {
        let f = board(backend);
        let first = mapped();
        f.store.activate_workflow(&first, &Host::local()).unwrap();
        // The next commit folds build/blocked's resume target, build/working, into a new state.
        let text = TWO
            .replace(
                "{ id = \"working\", label = \"Working\" }",
                "{ id = \"doing\", label = \"Doing\" }",
            )
            .replace("state = \"working\"", "state = \"doing\"");
        let next = at_commit(
            &custom(&format!(
                "{text}[[workflow_mappings]]\nfrom = {{ phase = \"build\", state = \"working\" }}\nto = {{ phase = \"build\", state = \"doing\" }}\n"
            )),
            'c',
        );
        f.store.activate_workflow(&next, &Host::local()).unwrap();
        let working = f
            .store
            .workflow_position("sim/in_progress")
            .unwrap()
            .unwrap();
        assert_eq!(
            (working.at(), working.revision, working.generation),
            (pair("build", "doing"), 2, 2)
        );
        let blocked = f.store.workflow_position("sim/blocked").unwrap().unwrap();
        assert_eq!(blocked.at(), pair("build", "blocked"));
        assert_eq!(blocked.resume, Some(pair("build", "doing")), "{backend:?}");
        assert_eq!(blocked.revision, 2);
        // A task whose pair and resume stay keeps its revision and the generation that moved it.
        let ready = f.store.workflow_position("sim/ready").unwrap().unwrap();
        assert_eq!((ready.revision, ready.generation), (1, 1));
    }
}

/// A board read carries the active generation in its own round trip, and an activation moves the
/// board's marker however few tasks it writes, so the server's kept read is read again.
#[test]
fn an_activation_moves_the_boards_marker_and_a_board_read_carries_its_generation() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        assert_eq!(f.store.board().unwrap().workflow_generation, None);
        assert_eq!(f.store.clock_board().unwrap().workflow_generation, None);
        let marker = f.store.marker().unwrap();
        let two = mapped();
        f.store.activate_workflow(&two, &Host::local()).unwrap();
        assert_ne!(
            f.store.marker().unwrap(),
            marker,
            "an activation that keeps and moves no task: {backend:?}"
        );
        assert_eq!(
            f.store.clock_board().unwrap().workflow_generation,
            Some(1),
            "{backend:?}"
        );
        assert_eq!(f.store.board().unwrap().workflow_generation, Some(1));
    }
}

#[test]
fn a_return_moves_the_boards_marker_and_a_board_read_carries_no_generation() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        assert_eq!(f.store.clock_board().unwrap().workflow_generation, Some(1));
        let marker = f.store.marker().unwrap();
        assert_eq!(
            f.store.follow_committed(&dropped(BACK), &Host::local()),
            Ok(Followed::Returned { generation: 2 }),
            "{backend:?}"
        );
        assert_ne!(
            f.store.marker().unwrap(),
            marker,
            "a return that keeps and moves no task: {backend:?}"
        );
        // The kept read is read again, so the page hears that no workflow is active.
        assert_eq!(
            f.store.clock_board().unwrap().workflow_generation,
            None,
            "{backend:?}"
        );
        assert_eq!(f.store.board().unwrap().workflow_generation, None);
    }
}

#[test]
fn an_activation_refused_leaves_the_board_and_the_previous_workflow_as_they_were() {
    for backend in BACKENDS {
        let f = board(backend);
        let before = f.store.dump().unwrap();
        let unmapped = custom(TWO);
        let why = refused(
            f.store
                .follow_committed(&committed(Some(unmapped)), &Host::local()),
        );
        assert_eq!(
            why,
            "workflow_mappings: sim/backlog is at backlog/available, which the workflow no longer \
             declares; map it to a pair it declares"
        );
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");

        let unsupported = refused(f.store.activate_workflow(&mapped(), &Host::default()));
        assert_eq!(unsupported, "workflow: unsupported capability: owner");
        assert_eq!(f.store.dump().unwrap(), before);
    }
}

/// A commit with no `[workflow]`, its file keeping `mappings` onto built-in v1.
fn dropped(mappings: &str) -> Committed {
    Committed {
        commit: "4".repeat(40),
        custom: None,
        returning: crate::workflow::written(mappings).unwrap().1,
    }
}

fn events(f: &Fixture) -> u64 {
    (f.store.raw().table("workflow"))
        .matching(json!({"kind": "event"}))
        .count()
        .unwrap()
}

#[test]
fn a_board_returns_to_its_own_writers_and_back_with_its_history_kept() {
    for backend in BACKENDS {
        let f = board(backend);
        let two = mapped();
        f.store.activate_workflow(&two, &Host::local()).unwrap();
        assert_eq!(events(&f), 7);
        let closed = ["sim/done", "sim/cancelled"].map(|slug| f.store.get(slug).unwrap());

        let back = dropped(BACK);
        assert_eq!(
            f.store.follow_committed(&back, &Host::local()),
            Ok(Followed::Returned { generation: 2 }),
            "{backend:?}"
        );
        // No workflow is active, and every task is the board's writers' again, at the status of
        // its mapped built-in pair, its last workflow revision kept.
        assert_eq!(f.store.workflow_active().unwrap(), None);
        assert_eq!(f.store.workflow_read().unwrap(), None);
        for (slug, status) in [
            ("sim/backlog", "ready"),
            ("sim/concept", "ready"),
            ("sim/design", "ready"),
            ("sim/ready", "ready"),
            ("sim/in_progress", "in_progress"),
            ("sim/review", "review"),
            ("sim/blocked", "blocked"),
        ] {
            let task = f.store.get(slug).unwrap();
            assert_eq!(task.status, status, "{backend:?} {slug}");
            assert_eq!(f.store.workflow_position(slug).unwrap(), None, "{slug}");
            assert_eq!(
                task.fields["_board"]["workflow_retired"],
                json!({"revision": 2, "generation": 2}),
                "{backend:?} {slug}"
            );
            assert_eq!(
                task.fields["_board"].contains_key("blocked"),
                status == "blocked",
                "{slug}"
            );
        }
        // Blocked before the workflow, it kept its record through it: what it waits on and who
        // blocked it are as they were, and its source is the status its resume pair maps to.
        let blocked = f.store.get("sim/blocked").unwrap();
        let record = crate::waits_on::Blocked::of(&blocked).expect("a blocked record");
        assert_eq!(
            (
                record.on.as_str(),
                record.from.as_deref(),
                record.by.as_str(),
                record.at
            ),
            ("task:sim/ready", Some("in_progress"), "owner", 1.0),
            "{backend:?}"
        );
        // Closed tasks no workflow held are as they were.
        assert_eq!(
            ["sim/done", "sim/cancelled"].map(|slug| f.store.get(slug).unwrap()),
            closed
        );
        // The history: every earlier record, each return as a transition, and the activation.
        assert_eq!(events(&f), 14);
        let raw = f.store.raw();
        let event = raw
            .row("workflow", json!(["event", "sim/blocked", 2]))
            .get()
            .unwrap();
        let transition: Transition = serde_json::from_value(event["body"].clone()).unwrap();
        assert_eq!(transition.from, Some(pair("build", "blocked")));
        assert_eq!(transition.to, pair("blocked", "waiting"));
        assert_eq!(transition.resume, Some(pair("in_progress", "working")));
        assert_eq!((transition.generation, transition.activation), (2, Some(2)));
        assert_eq!(transition.digest, crate::workflow::DEFAULT);
        let activation = raw.row("workflow", json!(["activation", 2])).get().unwrap();
        let body = &activation["body"];
        assert_eq!(body["returned"], json!(true));
        assert_eq!(body["moved"], json!(7));
        assert_eq!(body["commit"], json!("4".repeat(40)));
        assert_eq!(body["previous"]["generation"], json!(1));
        assert_eq!(body["previous"]["digest"], json!(two.revision.digest));
        assert_eq!(
            f.store.workflow_mappings(2).unwrap(),
            Some(back.returning.clone())
        );
        assert!(
            raw.row("workflow", json!(["activation", 1]))
                .exists()
                .unwrap()
        );
        assert!(
            f.store
                .workflow_definition(&two.revision.digest)
                .unwrap()
                .is_some()
        );
        assert!(!raw.row("workflow", json!(["active"])).exists().unwrap());

        // The same commit again finds none active, and writes nothing.
        let returned = f.store.dump().unwrap();
        assert_eq!(
            f.store.follow_committed(&back, &Host::local()),
            Ok(Followed::Legacy)
        );
        assert_eq!(f.store.return_to_default(&back), Ok(Followed::Legacy));
        assert_eq!(f.store.dump().unwrap(), returned, "{backend:?}");
        // A copy of the returned board is the board, on either database.
        for other in BACKENDS {
            let copied = Fixture::on(
                other,
                Seed {
                    dump: returned.clone(),
                },
            );
            assert_eq!(
                copied.store.dump().unwrap(),
                returned,
                "{backend:?} -> {other:?}"
            );
        }

        // The board's own commands work again: a claim, a release, a move, a reopening.
        let worker = f.session("worker");
        assert_eq!(
            f.store.claim("sim/ready", &worker).unwrap().status,
            "in_progress"
        );
        f.store.release("sim/ready", &worker, None).unwrap();
        assert_eq!(status(&f, "sim/ready"), "ready");
        f.store.move_to("sim/backlog", "backlog", &worker).unwrap();
        f.store.move_to("sim/design", "cancelled", "owner").unwrap();
        assert_eq!(
            events(&f),
            14,
            "{backend:?}: a board writer's move records no transition"
        );

        // The same workflow committed again is generation 3, and each task's revisions go on.
        assert_eq!(
            f.store
                .activate_workflow(&at_commit(&two, 'c'), &Host::local()),
            Ok(Followed::Activated(Active {
                generation: 3,
                digest: at_commit(&two, 'c').revision.digest
            })),
            "{backend:?}"
        );
        let ready = f.store.workflow_position("sim/ready").unwrap().unwrap();
        assert_eq!(
            (ready.at(), ready.revision, ready.generation),
            (pair("build", "available"), 3, 3)
        );
        assert_eq!(status(&f, "sim/backlog"), "ready");
        let blocked = f.store.workflow_position("sim/blocked").unwrap().unwrap();
        assert_eq!(
            blocked.resume,
            Some(pair("build", "working")),
            "{backend:?}"
        );
        // A task the board's writers closed since stays theirs.
        assert_eq!(f.store.workflow_position("sim/design").unwrap(), None);
        // And the board returns again, as generation 4.
        assert_eq!(
            f.store.follow_committed(&back, &Host::local()),
            Ok(Followed::Returned { generation: 4 })
        );
        assert_eq!(f.store.workflow_active().unwrap(), None);
    }
}

#[test]
fn a_task_closed_after_a_return_is_reopened_under_the_next_workflow_past_its_retired_revision() {
    for backend in BACKENDS {
        let f = board(backend);
        let held = custom(&format!("{TWO}{MAPPED}{HELD}"));
        f.store.activate_workflow(&held, &Host::local()).unwrap();
        f.store
            .follow_committed(&dropped(BACK), &Host::local())
            .unwrap();
        // Back with the board's writers, its reviewer approves it: done, with no position.
        let birch = f.session("reviewer");
        f.store.take_role("sim/review", &birch, "reviewer").unwrap();
        f.store
            .verdict("sim/review", &birch, "approve", "good")
            .unwrap();
        let task = f.store.get("sim/review").unwrap();
        assert_eq!(task.status, "done", "{backend:?}");
        assert_eq!(
            task.fields["_board"]["workflow_retired"]["revision"],
            json!(2)
        );
        // The next workflow leaves a closed task alone until a write reopens it, which places it
        // at the revision after the one its return made.
        f.store
            .activate_workflow(&at_commit(&held, 'c'), &Host::local())
            .unwrap();
        assert_eq!(f.store.workflow_position("sim/review").unwrap(), None);
        f.store.move_to("sim/review", "review", &birch).unwrap();
        let position = f.store.workflow_position("sim/review").unwrap().unwrap();
        assert_eq!(
            (position.at(), position.revision, position.generation),
            (pair("verify", "awaiting"), 3, 3),
            "{backend:?}"
        );
        assert!(
            (f.store.raw())
                .row("workflow", json!(["event", "sim/review", 3]))
                .exists()
                .unwrap()
        );
    }
}

/// The local owner takes `edge` on `slug` as the workflow's actions offer it now.
fn owner_takes(f: &Fixture, slug: &str, edge: &str) {
    let owner = crate::workflow::local_owner();
    let next = (f.store)
        .workflow_actions(slug, &owner, &Host::local())
        .unwrap();
    let request = symphony_api::workflow::OperationRequest {
        project: f.store.database().database().into(),
        task: slug.into(),
        edge: edge.into(),
        idempotency_key: format!("{edge}@{}", next.snapshot.task_revision),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        inputs: BTreeMap::new(),
    };
    (f.store)
        .apply_operation(&owner, &Host::local(), &request)
        .unwrap();
}

#[test]
fn a_task_blocked_under_the_workflow_returns_parked_naming_where_it_came_from() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        // The owner blocks sim/ready by the workflow's edge: no block record is written for it.
        owner_takes(&f, "sim/ready", "block");
        assert_eq!(status(&f, "sim/ready"), "blocked", "{backend:?}");
        f.store.return_to_default(&dropped(BACK)).unwrap();
        let task = f.store.get("sim/ready").unwrap();
        let record = crate::waits_on::Blocked::of(&task).expect("a blocked record");
        assert_eq!(
            (
                record.on.as_str(),
                record.why.as_deref(),
                record.by.as_str(),
                record.from.as_deref()
            ),
            (
                "parked",
                Some("returned from a custom workflow"),
                "board",
                Some("ready")
            ),
            "{backend:?}"
        );
    }
}

#[test]
fn a_block_record_a_task_left_under_the_workflow_does_not_come_back_on_the_return() {
    for backend in BACKENDS {
        let f = board(backend);
        // Blocked from ready on sim/review by the board's writers before the workflow.
        (f.store.raw().row("task", "sim/ready"))
            .set(json!({
                "status": "blocked",
                "fields._board.blocked":
                    {"on": "task:sim/review", "by": "owner", "at": 1.0, "from": "ready"},
            }))
            .unwrap();
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        // Resumed by the workflow's edge and blocked again by it: the edges write no block
        // record, so the old one stays on the row.
        owner_takes(&f, "sim/ready", "resume");
        assert_eq!(status(&f, "sim/ready"), "ready", "{backend:?}");
        owner_takes(&f, "sim/ready", "block");
        assert_eq!(status(&f, "sim/ready"), "blocked", "{backend:?}");
        let carried = f.store.get("sim/ready").unwrap();
        assert_eq!(
            crate::waits_on::Blocked::of(&carried).map(|record| record.on),
            Some("task:sim/review".to_string()),
            "{backend:?}"
        );
        // That block ended at the resume: the return writes the one it knows of, parked; and
        // sim/blocked, blocked throughout, keeps its own.
        f.store.return_to_default(&dropped(BACK)).unwrap();
        let kept = f.store.get("sim/blocked").unwrap();
        assert_eq!(
            crate::waits_on::Blocked::of(&kept).map(|record| record.on),
            Some("task:sim/ready".to_string()),
            "{backend:?}"
        );
        let task = f.store.get("sim/ready").unwrap();
        let record = crate::waits_on::Blocked::of(&task).expect("a blocked record");
        assert_eq!(
            (
                record.on.as_str(),
                record.why.as_deref(),
                record.by.as_str(),
                record.from.as_deref()
            ),
            (
                "parked",
                Some("returned from a custom workflow"),
                "board",
                Some("ready")
            ),
            "{backend:?}"
        );
    }
}

#[test]
fn a_return_refused_leaves_the_custom_generation_active_and_the_board_as_it_was() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let active = f.store.workflow_active().unwrap();
        let before = f.store.dump().unwrap();
        // No mapping at all, as a file that only drops its [workflow] has.
        assert_eq!(
            refused(f.store.follow_committed(&committed(None), &Host::local())),
            "workflow_mappings: sim/backlog is at build/available, which the workflow no longer \
             declares; map it to a pair it declares",
            "{backend:?}"
        );
        // One pair left out: the task there, or resuming there, refuses the whole return.
        let partial = BACK.replacen(
            "[[workflow_mappings]]\nfrom = { phase = \"verify\", state = \"awaiting\" }\nto = { phase = \"review\", state = \"awaiting_review\" }\n",
            "",
            1,
        );
        assert_ne!(partial, BACK);
        assert_eq!(
            refused(f.store.follow_committed(&dropped(&partial), &Host::local())),
            "workflow_mappings: sim/review is at verify/awaiting, which the workflow no longer \
             declares; map it to a pair it declares"
        );
        // A mapping that would complete unfinished work.
        let completing = BACK.replacen(
            "to = { phase = \"in_progress\", state = \"working\" }",
            "to = { phase = \"done\", state = \"completed\" }",
            1,
        );
        assert!(
            refused(
                f.store
                    .follow_committed(&dropped(&completing), &Host::local())
            )
            .starts_with(
                "workflow: sim/in_progress would stand at done/completed with the outcome"
            ),
            "{backend:?}"
        );
        assert_eq!(f.store.workflow_active().unwrap(), active);
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");
        // The custom workflow still runs: its edges are there to take.
        let actions = f
            .store
            .workflow_actions("sim/ready", &crate::workflow::local_owner(), &Host::local())
            .unwrap();
        assert!(!actions.actions.is_empty());
    }
}

#[test]
fn a_live_holder_returns_with_its_work_or_refuses_the_return() {
    for backend in BACKENDS {
        let f = board(backend);
        let worker = f.session("worker");
        f.store.claim("sim/in_progress", &worker).unwrap();
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap_err();
        f.store.release("sim/in_progress", &worker, None).unwrap();
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let amber = crate::workflow::agent_session(&worker);
        let next = f
            .store
            .workflow_actions("sim/ready", &amber, &Host::local())
            .unwrap();
        let request = symphony_api::workflow::OperationRequest {
            project: f.store.database().database().into(),
            task: "sim/ready".into(),
            edge: "claim".into(),
            expected: next.snapshot,
            input_revisions: BTreeMap::new(),
            idempotency_key: "claim".into(),
            inputs: BTreeMap::new(),
        };
        f.store
            .apply_operation(&amber, &Host::local(), &request)
            .unwrap();
        // Mapped to ready, where the board's writers keep no executor: refused, nothing written.
        let before = f.store.dump().unwrap();
        let idle = BACK.replacen(
            "to = { phase = \"in_progress\", state = \"working\" }",
            "to = { phase = \"ready\", state = \"available\" }",
            1,
        );
        let why = refused(f.store.follow_committed(&dropped(&idle), &Host::local()));
        assert!(
            why.starts_with(&format!(
                "workflow: sim/ready is held by {worker} as its executor, and it would return to ready"
            )),
            "{backend:?}: {why}"
        );
        assert_eq!(f.store.dump().unwrap(), before);
        // Mapped to in_progress it returns with its executor, who works on by the board's commands.
        assert_eq!(
            f.store.follow_committed(&dropped(BACK), &Host::local()),
            Ok(Followed::Returned { generation: 2 }),
            "{backend:?}"
        );
        let task = f.store.get("sim/ready").unwrap();
        assert_eq!(task.status, "in_progress");
        assert!(
            task.roles
                .iter()
                .any(|r| r.handle == worker && r.role == "executor")
        );
        assert_eq!(
            f.store
                .move_to("sim/ready", "review", &worker)
                .unwrap()
                .status,
            "review"
        );
    }
}

#[test]
fn a_plan_refused_on_a_read_the_board_moved_on_from_is_planned_again() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let read = f.store.standings().unwrap().tasks;
        assert!(!f.store.moved_since(&read).unwrap(), "{backend:?}");
        // A move after the read of the tasks, as a claim before the read of their holders.
        owner_takes(&f, "sim/ready", "block");
        assert!(f.store.moved_since(&read).unwrap(), "{backend:?}");
        // A task filed after it.
        let read = f.store.standings().unwrap().tasks;
        f.store
            .create(&crate::NewTask::new("sim/new", "New", "owner"))
            .unwrap();
        assert!(f.store.moved_since(&read).unwrap(), "{backend:?}");
    }
}

#[test]
fn of_an_admission_and_a_return_at_once_one_wins_and_the_board_is_whole() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let worker = f.session("worker");
        let amber = crate::workflow::agent_session(&worker);
        let next = f
            .store
            .workflow_actions("sim/ready", &amber, &Host::local())
            .unwrap();
        let request = symphony_api::workflow::OperationRequest {
            project: f.store.database().database().into(),
            task: "sim/ready".into(),
            edge: "claim".into(),
            expected: next.snapshot,
            input_revisions: BTreeMap::new(),
            idempotency_key: "claim".into(),
            inputs: BTreeMap::new(),
        };
        let other = f.other();
        let barrier = Arc::new(Barrier::new(2));
        let back = dropped(BACK);
        let (claimed, returned) = std::thread::scope(|scope| {
            let claim = scope.spawn(|| {
                barrier.wait();
                f.store.apply_operation(&amber, &Host::local(), &request)
            });
            let back = scope.spawn(|| {
                barrier.wait();
                other.follow_committed(&back, &Host::local())
            });
            (claim.join().unwrap(), back.join().unwrap())
        });
        // The return is written either way: before the claim, which is then refused as the
        // workflow it was read under is gone, or after it, planned again from the claimed task.
        assert_eq!(
            returned,
            Ok(Followed::Returned { generation: 2 }),
            "{backend:?}"
        );
        assert_eq!(f.store.workflow_active().unwrap(), None);
        let task = f.store.get("sim/ready").unwrap();
        assert_eq!(f.store.workflow_position("sim/ready").unwrap(), None);
        let held = task
            .roles
            .iter()
            .any(|r| r.handle == worker && r.role == "executor");
        match claimed {
            Ok(_) => {
                assert_eq!(
                    (task.status.as_str(), held),
                    ("in_progress", true),
                    "{backend:?}"
                );
                assert_eq!(
                    task.fields["_board"]["workflow_retired"]["revision"],
                    json!(3)
                );
                assert_eq!(events(&f), 15);
            }
            Err(Error::Refused(why)) => {
                // Refused by whichever read met the return: the claim's first, which found no
                // workflow active, or a later one, fenced on the workflow it had read.
                assert!(
                    why.starts_with("stale_")
                        || why.starts_with("unknown_id")
                        || why.starts_with("unsupported_activation: no custom workflow is active"),
                    "{backend:?}: {why}"
                );
                assert_eq!(
                    (task.status.as_str(), held),
                    ("ready", false),
                    "{backend:?}"
                );
                assert_eq!(
                    task.fields["_board"]["workflow_retired"]["revision"],
                    json!(2)
                );
                assert_eq!(events(&f), 14);
            }
            Err(other) => panic!("{backend:?}: {other:?}"),
        }
    }
}

#[test]
fn a_live_holder_refuses_the_activation_until_its_work_drains() {
    for backend in BACKENDS {
        let f = board(backend);
        let worker = f.session("worker");
        f.store.claim("sim/ready", &worker).unwrap();
        let before = f.store.dump().unwrap();
        let why = refused(f.store.activate_workflow(&mapped(), &Host::local()));
        assert!(
            why.starts_with(&format!(
                "workflow: sim/ready is held by {worker} as its executor"
            )),
            "{backend:?}: {why}"
        );
        assert_eq!(f.store.dump().unwrap(), before);
        f.store.release("sim/ready", &worker, None).unwrap();
        assert!(matches!(
            f.store.activate_workflow(&mapped(), &Host::local()),
            Ok(Followed::Activated(_))
        ));
    }
}

#[test]
fn the_activation_write_is_refused_as_stale_after_another_write_and_planned_again() {
    for backend in BACKENDS {
        let f = board(backend);
        let two = mapped();
        let write = |vars: Value| {
            f.store.retrying(|| {
                f.store
                    .transact::<Value>(both(sql::ACTIVATE, pg::activate()), vars.clone())
            })
        };
        let empty = json!({
            "expected": 1, "generation": 2, "digest": "d2", "definition": {},
            "activation": {}, "moved": [], "kept": [], "seen": [], "at": 1.0,
        });
        assert_eq!(
            refused(write(empty.clone())),
            "stale_workflow: the active generation is 0, not 1",
            "{backend:?}"
        );
        let mut stale_task = empty.clone();
        stale_task["expected"] = json!(0);
        stale_task["kept"] = json!([{"slug": "sim/ready", "version": 99}]);
        assert_eq!(
            refused(write(stale_task)),
            "stale_task: sim/ready changed while its workflow activated"
        );
        // A holder it read as lost, seen since; a session gone since is seen as well.
        let worker = f.session("worker");
        let mut seen = empty.clone();
        seen["expected"] = json!(0);
        seen["seen"] = json!([{"handle": worker, "last_seen": 1.0}]);
        assert_eq!(
            refused(write(seen.clone())),
            format!("stale_task: {worker} was seen while its workflow activated")
        );
        seen["seen"] = json!([{"handle": "gone", "last_seen": 1.0}]);
        assert_eq!(
            refused(write(seen)),
            "stale_task: gone was seen while its workflow activated"
        );
        // An open task it neither moved nor kept: one filed or reopened since its read.
        let mut unplaced = empty;
        unplaced["expected"] = json!(0);
        assert_eq!(
            refused(write(unplaced.clone())),
            "stale_task: sim/backlog was filed or reopened while its workflow activated"
        );
        // Every task it read planned but one: that one is named.
        let open = [
            "backlog",
            "blocked",
            "concept",
            "design",
            "in_progress",
            "ready",
        ];
        unplaced["kept"] = (open.iter())
            .map(|s| {
                let slug = format!("sim/{s}");
                json!({"slug": slug, "version": f.store.raw().row("task", slug.as_str()).get().unwrap()["version"]})
            })
            .collect();
        assert_eq!(
            refused(write(unplaced)),
            "stale_task: sim/review was filed or reopened while its workflow activated",
            "{backend:?}"
        );
        assert_eq!(f.store.raw().table("workflow").count().unwrap(), 0);

        // Two activations at once: one takes generation 1, the other finds it active.
        let other = f.other();
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                f.store.activate_workflow(&two, &Host::local())
            });
            let second = scope.spawn(|| {
                barrier.wait();
                other.activate_workflow(&two, &Host::local())
            });
            [first.join().unwrap(), second.join().unwrap()]
        });
        let active = Active {
            generation: 1,
            digest: two.revision.digest.clone(),
        };
        let mut outcomes: Vec<_> = results.into_iter().map(Result::unwrap).collect();
        outcomes.sort_by_key(|o| matches!(o, Followed::Unchanged(_)));
        assert_eq!(
            outcomes,
            vec![
                Followed::Activated(active.clone()),
                Followed::Unchanged(active)
            ],
            "{backend:?}"
        );
        assert_eq!(
            f.store
                .raw()
                .table("workflow")
                .matching(json!({"kind": "activation"}))
                .count()
                .unwrap(),
            1
        );
    }
}

/// A holder lost when the plan read it that beats before the write: the write is stale, and the
/// next plan reads it live and refuses, as for a holder live all along.
#[test]
fn a_lost_holder_seen_between_the_plan_and_its_write_refuses_the_activation() {
    for backend in BACKENDS {
        let f = board(backend);
        let worker = f.session("worker");
        f.store.claim("sim/ready", &worker).unwrap();
        f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
        assert!(!f.health(&worker).holds(), "{backend:?}");
        let (beating, handle) = (f.other(), worker.clone());
        let late = f.late(move || beating.touch(&handle).unwrap());
        let why = refused(late.activate_workflow(&mapped(), &Host::local()));
        assert!(
            why.starts_with(&format!(
                "workflow: sim/ready is held by {worker} as its executor"
            )),
            "{backend:?}: {why}"
        );
        assert_eq!(f.store.workflow_active().unwrap(), None);
        assert_eq!(f.store.workflow_position("sim/ready").unwrap(), None);
        assert_eq!(f.store.raw().table("workflow").count().unwrap(), 0);
        // Lost and unseen, its work is moved.
        f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
        assert!(matches!(
            f.store.activate_workflow(&mapped(), &Host::local()),
            Ok(Followed::Activated(_))
        ));
    }
}

/// A task filed with no position between the plan's read and its write: the write is stale, and
/// the next plan places it with the rest.
#[test]
fn a_task_filed_between_the_plan_and_its_write_is_placed_by_the_next_plan() {
    for backend in BACKENDS {
        let f = board(backend);
        let filer = f.other();
        let late = f.late(move || {
            filer
                .create(&crate::NewTask::new("sim/filed", "Filed late", "owner"))
                .unwrap();
        });
        assert!(matches!(
            late.activate_workflow(&mapped(), &Host::local()),
            Ok(Followed::Activated(_))
        ));
        let filed = f.store.workflow_position("sim/filed").unwrap();
        assert_eq!(
            filed.map(|p| (p.at(), p.revision, p.generation)),
            Some((pair("build", "available"), 1, 1)),
            "{backend:?}"
        );
    }
}

/// With a generation active, a task filed and a done one reopened between the next plan's read
/// and its write are placed by the active generation, at pairs the next one maps away: the write
/// is stale, and the next plan moves each by the mappings, so none is left at a pair the active
/// definition does not declare.
#[test]
fn a_task_placed_between_a_later_plan_and_its_write_is_moved_by_the_next_plan() {
    for backend in BACKENDS {
        // sim/late approved before any workflow is active: done, with no position.
        let seed = Seed::default()
            .session("birch")
            .session("orch")
            .task("sim/open", "backlog")
            .task("sim/late", "review")
            .role("sim/late", "birch", "reviewer")
            .role("sim/late", "orch", "orchestrator");
        let f = Fixture::on(backend, seed);
        (f.store.verdict("sim/late", "birch", "approve", "good")).unwrap();
        let first = custom(&format!("{TWO}{MAPPED}{HELD}"));
        f.store.activate_workflow(&first, &Host::local()).unwrap();
        assert_eq!(f.store.workflow_position("sim/late").unwrap(), None);
        // Its reviewer is lost, so a plan may move the task it reviews once it reopens.
        f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
        // The next commit renames the initial pair and the reviewed one, and maps each on.
        let text = (TWO.replace(r#"state = "available""#, r#"state = "open""#))
            .replace(
                r#"{ id = "available", label = "Available" }"#,
                r#"{ id = "open", label = "Open" }"#,
            )
            .replace(r#"state = "awaiting""#, r#"state = "waiting""#)
            .replace(
                r#"{ id = "awaiting", label = "Awaiting" }"#,
                r#"{ id = "waiting", label = "Waiting" }"#,
            );
        let map = |phase: &str, from: &str, to: &str| {
            format!(
                "[[workflow_mappings]]\nfrom = {{ phase = \"{phase}\", state = \"{from}\" }}\nto = {{ phase = \"{phase}\", state = \"{to}\" }}\n"
            )
        };
        let next = custom(&format!(
            "{text}{}{}",
            map("build", "available", "open"),
            map("verify", "awaiting", "waiting")
        ));
        let next = at_commit(&next, 'c');
        let other = f.other();
        let late = f.late(move || {
            (other.create(&crate::NewTask::new("sim/filed", "Filed late", "owner"))).unwrap();
            other.move_to("sim/late", "review", "orch").unwrap();
            // Each placed by the generation active at its write.
            for (slug, at) in [
                ("sim/filed", pair("build", "available")),
                ("sim/late", pair("verify", "awaiting")),
            ] {
                let position = other.workflow_position(slug).unwrap().unwrap();
                assert_eq!((position.at(), position.generation), (at, 1), "{slug}");
            }
        });
        assert!(matches!(
            late.activate_workflow(&next, &Host::local()),
            Ok(Followed::Activated(Active { generation: 2, .. }))
        ));
        for (slug, at) in [
            ("sim/open", pair("build", "open")),
            ("sim/filed", pair("build", "open")),
            ("sim/late", pair("verify", "waiting")),
        ] {
            let position = f.store.workflow_position(slug).unwrap().unwrap();
            assert_eq!(
                (position.at(), position.revision, position.generation),
                (at, 2, 2),
                "{backend:?} {slug}"
            );
        }
    }
}

/// A commit whose project file is the active one's, byte for byte, is the active workflow: the
/// one read of the active generation says so, and no definition or mapping is read. Another file
/// with the same definition and mappings is compared by them.
#[test]
fn a_commit_of_the_active_ones_file_is_unchanged_by_its_blob_alone() {
    for backend in BACKENDS {
        let f = board(backend);
        let first = mapped();
        f.store.activate_workflow(&first, &Host::local()).unwrap();
        let active = f.store.workflow_active().unwrap().unwrap();
        // Another definition under the same blob and format is the active file still: only its
        // bytes are compared, which name the definition and the mappings both.
        let mut relabelled = custom(&format!(
            "{}{MAPPED}",
            TWO.replace(r#"label = "Build""#, r#"label = "Make""#)
        ));
        let mut source = first.revision.source.clone();
        source.commit = "b".repeat(40);
        relabelled.revision = relabelled.definition.revision(source).unwrap();
        assert_ne!(relabelled.revision.digest, active.digest);
        assert_eq!(
            f.store.activate_workflow(&relabelled, &Host::local()),
            Ok(Followed::Unchanged(active.clone())),
            "{backend:?}"
        );
        // The same workflow in a file of other bytes (a limit changed beside it): compared by its
        // definition and mappings, which are the active ones.
        let mut limits = first.clone();
        let mut source = first.revision.source.clone();
        source.project_blob = "c".repeat(40);
        limits.revision = limits.definition.revision(source).unwrap();
        assert_eq!(
            f.store.activate_workflow(&limits, &Host::local()),
            Ok(Followed::Unchanged(active.clone()))
        );
        // Of other bytes and another definition, it activates.
        let mut source = first.revision.source.clone();
        source.project_blob = "d".repeat(40);
        relabelled.revision = relabelled.definition.revision(source).unwrap();
        assert!(matches!(
            f.store.activate_workflow(&relabelled, &Host::local()),
            Ok(Followed::Activated(Active { generation: 2, .. }))
        ));
    }
}

#[test]
fn the_return_write_is_refused_as_stale_after_another_write() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let before = f.store.dump().unwrap();
        let back = |vars: Value| {
            f.store.retrying(|| {
                (f.store).transact::<Value>(
                    both(sql::RETURN_DEFAULT, pg::return_default()),
                    vars.clone(),
                )
            })
        };
        let empty = json!({
            "expected": 2, "generation": 3, "moved": [], "kept": [], "seen": [], "at": 1.0,
            "activation": {"generation": 3, "digest": "builtin-v1", "returned": true},
        });
        // Planned under a generation that is not the active one any more.
        assert_eq!(
            refused(back(empty.clone())),
            "stale_workflow: the active generation is 1, not 2",
            "{backend:?}"
        );
        // A task it only fenced, or one it moves, changed since it was read.
        let mut kept = empty.clone();
        kept["expected"] = json!(1);
        kept["kept"] = json!([{"slug": "sim/ready", "version": 99}]);
        assert_eq!(
            refused(back(kept)),
            "stale_task: sim/ready changed while its workflow returned"
        );
        let version = (f.store.raw().row("task", "sim/ready").field("version"))
            .unwrap()
            .as_i64()
            .unwrap();
        let mut moved = empty.clone();
        moved["expected"] = json!(1);
        moved["moved"] = json!([{
            "slug": "sim/ready", "version": version, "expected": 7,
            "transition": {"revision": 8, "status": "ready"}, "retired": {"revision": 8},
        }]);
        assert_eq!(
            refused(back(moved)),
            "stale_task: sim/ready changed while its workflow returned",
            "{backend:?}"
        );
        // A holder it read as lost beat since, or a task it would select it did not plan.
        let mut seen = empty.clone();
        seen["expected"] = json!(1);
        seen["seen"] = json!([{"handle": "gone", "last_seen": 1.0}]);
        assert_eq!(
            refused(back(seen)),
            "stale_task: gone was seen while its workflow returned",
            "{backend:?}"
        );
        let mut unplanned = empty.clone();
        unplanned["expected"] = json!(1);
        assert_eq!(
            refused(back(unplanned)),
            "stale_task: sim/backlog was filed or reopened while its workflow returned",
            "{backend:?}"
        );
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");

        // A task the workflow closed is selected while it holds a position and leaves the
        // selection as the return moves it to done: beside it, one task the plan left out is
        // still refused, whatever the counts.
        owner_takes(&f, "sim/review", "accept");
        assert_eq!(status(&f, "sim/review"), "done", "{backend:?}");
        let before = f.store.dump().unwrap();
        let version = |slug: &str| {
            (f.store.raw().row("task", slug).field("version"))
                .unwrap()
                .as_i64()
                .unwrap()
        };
        let revision = (f.store.workflow_position("sim/review").unwrap())
            .unwrap()
            .revision;
        let mut closing = empty;
        closing["expected"] = json!(1);
        closing["kept"] = ([
            "sim/blocked",
            "sim/concept",
            "sim/design",
            "sim/in_progress",
            "sim/ready",
        ]
        .iter())
        .map(|slug| json!({"slug": slug, "version": version(slug)}))
        .collect();
        closing["moved"] = json!([{
            "slug": "sim/review", "version": version("sim/review"), "expected": revision,
            "transition": {
                "slug": "sim/review", "revision": revision + 1, "status": "done", "activation": 3,
            },
            "retired": {"revision": revision + 1, "generation": 3},
        }]);
        assert_eq!(
            refused(back(closing)),
            "stale_task: sim/backlog was filed or reopened while its workflow returned",
            "{backend:?}"
        );
        assert_eq!(f.store.dump().unwrap(), before, "{backend:?}");

        // After a return none is active, and an activation planned as a generation another
        // took is stale too.
        f.store.return_to_default(&dropped(BACK)).unwrap();
        let taken = json!({
            "expected": 0, "generation": 2, "digest": "d2", "definition": {},
            "activation": {}, "moved": [], "kept": [], "seen": [], "at": 1.0,
        });
        let activate = f.store.retrying(|| {
            (f.store).transact::<Value>(both(sql::ACTIVATE, pg::activate()), taken.clone())
        });
        assert_eq!(
            refused(activate),
            "stale_workflow: generation 2 is activated already",
            "{backend:?}"
        );
    }
}

#[test]
fn a_copy_of_an_activated_board_keeps_its_workflow() {
    for backend in BACKENDS {
        let f = board(backend);
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let dump = f.store.dump().unwrap();
        assert!(dump.workflow.len() >= 10, "{}", dump.workflow.len());
        let copied = Fixture::on(backend, Seed { dump: dump.clone() });
        assert_eq!(copied.store.dump().unwrap(), dump);
        assert_eq!(
            copied.store.workflow_active().unwrap(),
            f.store.workflow_active().unwrap()
        );
    }
}

#[test]
fn the_board_follows_the_workflow_its_default_branch_commits_and_nothing_else() {
    for backend in BACKENDS {
        let f = board(backend);
        let repo = Scratch::new("follow");
        // A repository with no commit yet commits no workflow, and says nothing.
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        std::fs::write(repo.0.join("symphony.toml"), "[project\n").unwrap();
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        std::fs::remove_file(repo.0.join("symphony.toml")).unwrap();
        repo.commit("empty");
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        // Written, staged or on another branch, a workflow is not committed on the default one.
        std::fs::write(repo.0.join("symphony.toml"), format!("{TWO}{MAPPED}")).unwrap();
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        repo.git(&["checkout", "-q", "-b", "topic"]);
        repo.commit("a workflow on a branch");
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        repo.git(&["checkout", "-q", "main"]);
        repo.git(&["merge", "-q", "--ff-only", "topic"]);
        let Ok(Followed::Activated(active)) = follow(&f.store, &repo.0) else {
            panic!("{backend:?}: the committed workflow activates");
        };
        assert_eq!(active.generation, 1);
        let recorded = f
            .store
            .workflow_definition(&active.digest)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.revision.source.commit, repo.head());
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Unchanged(active)));
        assert_eq!(status(&f, "sim/backlog"), "ready");
        // A commit that drops the workflow and maps nothing is refused, said, and changes
        // nothing; one that keeps mappings onto the board's own pairs returns the board to them.
        std::fs::write(repo.0.join("symphony.toml"), "[project]\nid = \"demo\"\n").unwrap();
        repo.commit("no workflow, no mappings");
        let why = follow(&f.store, &repo.0).unwrap_err();
        assert!(
            why.starts_with("workflow_mappings: sim/backlog is at build/available"),
            "{backend:?}: {why}"
        );
        assert_eq!(f.store.workflow_active().unwrap().unwrap().generation, 1);
        std::fs::write(repo.0.join("symphony.toml"), BACK).unwrap();
        repo.commit("back to the board's writers");
        assert_eq!(
            follow(&f.store, &repo.0),
            Ok(Followed::Returned { generation: 2 }),
            "{backend:?}"
        );
        let activation = (f.store.raw())
            .row("workflow", json!(["activation", 2]))
            .get()
            .unwrap();
        assert_eq!(activation["body"]["commit"], json!(repo.head()));
        assert_eq!(follow(&f.store, &repo.0), Ok(Followed::Legacy));
        assert_eq!(status(&f, "sim/review"), "review");
        assert_eq!(f.store.workflow_position("sim/review").unwrap(), None);
    }
}

#[test]
fn the_default_branch_is_the_one_the_project_file_names() {
    let f = board(Backend::SurrealDb);
    let repo = Scratch::new("trunk");
    repo.git(&["checkout", "-q", "-b", "trunk"]);
    std::fs::write(
        repo.0.join("symphony.toml"),
        format!("[project]\ndefault_branch = \"trunk\"\n{TWO}{MAPPED}"),
    )
    .unwrap();
    repo.commit("trunk's workflow");
    let Ok(Followed::Activated(active)) = follow(&f.store, &repo.0) else {
        panic!("trunk's workflow activates");
    };
    let recorded = f
        .store
        .workflow_definition(&active.digest)
        .unwrap()
        .unwrap();
    assert_eq!(recorded.revision.source.commit, repo.head());
    // A branch the file names and the repository lacks is said, and activates nothing more.
    std::fs::write(
        repo.0.join("symphony.toml"),
        "[project]\ndefault_branch = \"gone\"\n",
    )
    .unwrap();
    assert_eq!(
        follow(&f.store, &repo.0),
        Err("workflow committed on gone: the branch names no commit".into())
    );
    assert_eq!(f.store.workflow_active().unwrap(), Some(active));
}
