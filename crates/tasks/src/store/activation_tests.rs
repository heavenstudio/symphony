//! Automatic activation on a board, on both databases: what it writes, what it leaves, and the
//! fences that refuse it whole.

use super::*;
use crate::store_fixture::{Backend, Fixture, Seed, task};
use crate::workflow::activate::tests::{MAPPED, TWO, custom};
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
            json!({"on": "sim/ready", "by": "owner", "at": 1.0, "from": "in_progress"}),
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

#[test]
fn a_new_commit_of_the_same_workflow_is_a_generation_that_moves_nothing() {
    for backend in BACKENDS {
        let f = board(backend);
        let first = at_commit(&mapped(), 'a');
        f.store.activate_workflow(&first, &Host::local()).unwrap();
        let before = f.store.workflow_position("sim/ready").unwrap().unwrap();
        let events = f
            .store
            .raw()
            .table("workflow")
            .matching(json!({"kind": "event"}))
            .count();
        let second = at_commit(&mapped(), 'b');
        assert_eq!(
            f.store.activate_workflow(&second, &Host::local()),
            Ok(Followed::Activated(Active {
                generation: 2,
                digest: second.revision.digest.clone()
            }))
        );
        assert_eq!(
            f.store.workflow_position("sim/ready").unwrap(),
            Some(before)
        );
        assert_eq!(
            f.store
                .raw()
                .table("workflow")
                .matching(json!({"kind": "event"}))
                .count(),
            events
        );
        let activation = f
            .store
            .raw()
            .row("workflow", json!(["activation", 2]))
            .get()
            .unwrap();
        assert_eq!(activation["body"]["moved"], json!(0), "{backend:?}");
        assert_eq!(activation["body"]["previous"]["generation"], json!(1));
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

        // Once a workflow is active, dropping it from the file is refused, and it stays.
        f.store
            .activate_workflow(&mapped(), &Host::local())
            .unwrap();
        let active = f.store.workflow_active().unwrap();
        let back = refused(f.store.follow_committed(&committed(None), &Host::local()));
        assert!(
            back.starts_with(&format!(
                "workflow: commit {} sets no [workflow], and generation 1 is active",
                "3".repeat(40)
            )),
            "{back}"
        );
        assert_eq!(f.store.workflow_active().unwrap(), active);
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
            "activation": {}, "moved": [], "kept": [], "at": 1.0,
        });
        assert_eq!(
            refused(write(empty.clone())),
            "stale_workflow: the active generation is 0, not 1",
            "{backend:?}"
        );
        let mut stale_task = empty;
        stale_task["expected"] = json!(0);
        stale_task["kept"] = json!([{"slug": "sim/ready", "version": 99}]);
        assert_eq!(
            refused(write(stale_task)),
            "stale_task: sim/ready changed while its workflow activated"
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
