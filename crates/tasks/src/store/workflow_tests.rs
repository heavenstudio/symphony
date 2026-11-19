//! The workflow's records and the fences on them, on both databases: what the store writes, and
//! what every other writer, the database's own statements included, is refused.

use super::*;
use crate::rows::Dump;
use crate::store::postgres::Pg;
use crate::store_fixture::{Backend, Fixture, Seed};
use symphony_plugin::execution::PrincipalKind;

const BACKENDS: [Backend; 2] = [Backend::SurrealDb, Backend::PostgreSql];

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn owner() -> Principal {
    Principal::new(PrincipalKind::Human, "local".into(), "owner".into())
}

/// The transition that makes `revision` of `slug`, to `to`, projecting to `status`.
fn moved(slug: &str, revision: u64, to: Pair, status: &str) -> Transition {
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
        to,
        resume: None,
        status: status.into(),
        principal: owner(),
        effects: vec![],
        key: Some(format!("key{revision}")),
        recovery: None,
        at: 1000.0 + revision as f64,
    }
}

/// `slug` made and moved under a workflow to build/working, as `in_progress`.
fn managed(f: &Fixture, slug: &str) -> Transition {
    f.task(slug);
    let first = moved(slug, 1, pair("build", "working"), "in_progress");
    f.store.record_transition(&first, 0).unwrap();
    first
}

fn refused(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Refused(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_board_without_a_workflow_holds_none_of_its_records() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        f.task("sim/jump");
        assert_eq!(f.store.workflow_active().unwrap(), None);
        assert_eq!(f.store.workflow_definition("d1").unwrap(), None);
        assert_eq!(f.store.workflow_position("sim/jump").unwrap(), None);
        assert_eq!(
            refused(f.store.workflow_position("sim/none")),
            "no task sim/none"
        );
        assert_eq!(f.store.raw().table("workflow").count().unwrap(), 0);
        assert!(f.store.dump().unwrap().workflow.is_empty());
    }
}

#[test]
fn a_transition_is_recorded_beside_its_position_once() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        let first = managed(&f, "sim/jump");
        assert_eq!(
            f.store.workflow_position("sim/jump").unwrap(),
            Some(first.position())
        );
        let task = f.store.get("sim/jump").unwrap();
        assert_eq!(task.status, "in_progress");
        let recorded = f
            .store
            .raw()
            .row("workflow", json!(["event", "sim/jump", 1]))
            .get()
            .unwrap();
        assert_eq!(recorded["kind"], "event");
        let body: Transition = serde_json::from_value(recorded["body"].clone()).unwrap();
        assert_eq!(body, first);

        // A second transition from the same revision is stale, and changes nothing.
        let again = moved("sim/jump", 1, pair("build", "blocked"), "blocked");
        let stale = refused(f.store.record_transition(&again, 0));
        assert_eq!(
            stale,
            "stale_task: sim/jump is at workflow revision 1, not 0"
        );
        assert_eq!(
            f.store.workflow_position("sim/jump").unwrap(),
            Some(first.position())
        );

        let mut second = moved("sim/jump", 2, pair("verify", "awaiting_review"), "review");
        second.resume = Some(pair("build", "working"));
        second.from = Some(first.to.clone());
        f.store.record_transition(&second, 1).unwrap();
        assert_eq!(
            f.store.workflow_position("sim/jump").unwrap(),
            Some(second.position())
        );
        assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
        assert_eq!(f.store.raw().table("workflow").count().unwrap(), 2);
    }
}

#[test]
fn no_writer_moves_a_managed_task_past_its_record() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        let first = managed(&f, "sim/jump");
        let task = || f.store.raw().row("task", "sim/jump");
        let follows = refused(task().set(json!({"status": "ready"})));
        assert!(
            follows.contains("the status of sim/jump follows its workflow position"),
            "{backend:?}: {follows}"
        );
        let beside = refused(task().set(json!({"fields._board.workflow.phase": "deploy"})));
        assert!(
            beside.contains("sim/jump moves from workflow revision 1 only to the next"),
            "{backend:?}: {beside}"
        );
        let forged = refused(task().set(json!({
            "fields._board.workflow.phase": "deploy",
            "fields._board.workflow.revision": 2,
        })));
        assert!(
            forged.contains("sim/jump is at a workflow position only beside the transition"),
            "{backend:?}: {forged}"
        );
        let removed = refused(task().set(json!({"fields._board.workflow": null})));
        assert!(
            removed.contains("sim/jump keeps its workflow position"),
            "{backend:?}: {removed}"
        );
        let replaced = refused(task().set(json!({"fields": {"plugin": {"x": 1}}})));
        assert!(
            replaced.contains("keeps its workflow position"),
            "{replaced}"
        );
        // The board's own writers meet the same fence.
        let worker = f.session("worker");
        f.task("sim/held");
        f.store.claim("sim/held", &worker).unwrap();
        let held = moved("sim/held", 1, pair("build", "working"), "in_progress");
        f.store.record_transition(&held, 0).unwrap();
        let moved_by_hand = refused(f.store.move_to("sim/held", "ready", &worker));
        assert!(
            moved_by_hand.starts_with("explicit_action_required: a custom workflow holds sim/held"),
            "{backend:?}: {moved_by_hand}"
        );
        // The owner's own move meets the fence itself.
        let owner = refused(f.store.move_to("sim/held", "cancelled", "owner"));
        assert!(
            owner.starts_with("explicit_action_required: the status of sim/held follows its"),
            "{backend:?}: {owner}"
        );
        // A field nothing in the workflow reads is the board's to change, as ever.
        task().set(json!({"title": "Jump, renamed"})).unwrap();
        assert_eq!(
            f.store.workflow_position("sim/jump").unwrap(),
            Some(first.position())
        );
        assert_eq!(f.store.get("sim/jump").unwrap().status, "in_progress");

        // A task no workflow holds is the board's own writers', unfenced.
        f.task("sim/free");
        f.store
            .raw()
            .row("task", "sim/free")
            .set(json!({"status": "review"}))
            .unwrap();
        let planted = refused(f.store.raw().row("task", "sim/free").set(json!({
            "fields._board.workflow": first.position(),
        })));
        assert!(
            planted.contains("sim/free is at a workflow position only beside the transition"),
            "{backend:?}: {planted}"
        );
    }
}

#[test]
fn nothing_writes_a_workflow_record_past_its_rules() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        managed(&f, "sim/jump");
        let raw = f.store.raw();
        let event = || raw.row("workflow", json!(["event", "sim/jump", 1]));
        let refusal = "workflow: a workflow's records are written by its activation and its \
                       operations alone, never past them";
        assert_eq!(refused(event().set(json!({"kind": "intent"}))), refusal);
        assert_eq!(refused(event().add("at", 1)), refusal);
        assert_eq!(refused(event().put(json!({"kind": "event"}))), refusal);
        assert_eq!(refused(event().delete()), refusal);
        let fresh = raw.row("workflow", json!(["event", "sim/jump", 2]));
        assert_eq!(
            refused(fresh.create(json!({"kind": "event", "body": {}, "at": 1.0}))),
            refusal
        );
        assert_eq!(refused(raw.table("workflow").delete()), refusal);
        assert!(event().exists().unwrap());
    }
}

/// The database's own statements, as a hand at its server would send them.
fn by_hand(f: &Fixture, surreal: &'static str, postgres: &'static str) -> Result<Value, Error> {
    f.store.retrying(|| {
        f.store
            .transact(both(surreal, Pg::script(postgres)), json!({}))
    })
}

#[test]
fn the_database_never_changes_a_recorded_transition_or_moves_the_active_generation_back() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        managed(&f, "sim/jump");
        let changed = refused(by_hand(
            &f,
            "UPDATE workflow:['event', 'sim/jump', 1] SET body.status = 'done' RETURN NONE; RETURN NONE;",
            "UPDATE workflow SET body = body || '{\"status\": \"done\"}' \
             WHERE id = '[\"event\", \"sim/jump\", 1]';",
        ));
        assert_eq!(
            changed, "workflow: a recorded event is never changed",
            "{backend:?}"
        );
        let retyped = refused(by_hand(
            &f,
            "UPDATE workflow:['event', 'sim/jump', 1] SET kind = 'intent' RETURN NONE; RETURN NONE;",
            "UPDATE workflow SET kind = 'intent' WHERE id = '[\"event\", \"sim/jump\", 1]';",
        ));
        assert_eq!(retyped, "workflow: a recorded event is never changed");

        by_hand(
            &f,
            "CREATE workflow:['activation', 1] CONTENT { kind: 'activation', body: { generation: 1, digest: 'd1' }, at: 1.0 } RETURN NONE;
             CREATE workflow:['active'] CONTENT { kind: 'active', body: { generation: 1, digest: 'd1' }, at: 1.0 } RETURN NONE;
             RETURN NONE;",
            "INSERT INTO workflow (id, kind, body, at) VALUES \
             ('[\"activation\", 1]', 'activation', '{\"generation\": 1, \"digest\": \"d1\"}', 1.0), \
             ('[\"active\"]', 'active', '{\"generation\": 1, \"digest\": \"d1\"}', 1.0);",
        )
        .unwrap();
        assert_eq!(
            f.store.workflow_active().unwrap(),
            Some(Active {
                generation: 1,
                digest: "d1".into()
            })
        );
        let unrecorded = refused(by_hand(
            &f,
            "UPDATE workflow:['active'] SET body = { generation: 2, digest: 'd2' } RETURN NONE; RETURN NONE;",
            "UPDATE workflow SET body = '{\"generation\": 2, \"digest\": \"d2\"}' WHERE id = '[\"active\"]';",
        ));
        assert_eq!(
            unrecorded,
            "workflow: the active generation moves forward only to an activation recorded for it"
        );
        let activation = refused(by_hand(
            &f,
            "UPDATE workflow:['activation', 1] SET body.digest = 'd2' RETURN NONE; RETURN NONE;",
            "UPDATE workflow SET body = body || '{\"digest\": \"d2\"}' WHERE id = '[\"activation\", 1]';",
        ));
        assert_eq!(
            activation,
            "workflow: a recorded activation is never changed"
        );
        assert_eq!(
            f.store.workflow_active().unwrap().unwrap().generation,
            1,
            "{backend:?}"
        );
    }
}

#[test]
fn a_copy_carries_the_workflow_and_refuses_positions_without_their_records() {
    for backend in BACKENDS {
        let f = Fixture::on(backend, Seed::default());
        let first = managed(&f, "sim/jump");
        f.task("sim/free");
        let dump = f.store.dump().unwrap();
        assert_eq!(dump.workflow.len(), 1);
        assert_eq!(dump.workflow[0].key, json!(["event", "sim/jump", 1]));
        assert_eq!(
            dump.tasks
                .iter()
                .find(|t| t.slug == "sim/jump")
                .unwrap()
                .fields["_board"]["workflow"],
            serde_json::to_value(first.position()).unwrap()
        );
        for other in BACKENDS {
            let copied = Fixture::on(other, Seed { dump: dump.clone() });
            assert_eq!(
                copied.store.dump().unwrap(),
                dump,
                "{backend:?} -> {other:?}"
            );
            assert_eq!(
                copied.store.workflow_position("sim/jump").unwrap(),
                Some(first.position())
            );
        }

        // A dump that lost the records cannot put a managed task back: the copy is refused
        // whole and the board is left as it was.
        let lost = Dump {
            workflow: vec![],
            ..dump.clone()
        };
        let target = Fixture::on(backend, Seed::default());
        target.task("sim/kept");
        let refusal = refused(target.store.copy(&lost));
        assert!(
            refusal.contains("sim/jump is at a workflow position only beside the transition"),
            "{backend:?}: {refusal}"
        );
        assert!(target.store.get("sim/kept").is_ok());
        assert!(target.store.get("sim/jump").is_err());

        // Nor a task put back at a position its later transitions left, with every record kept.
        let mut second = moved("sim/jump", 2, pair("verify", "awaiting_review"), "review");
        second.from = Some(first.to.clone());
        f.store.record_transition(&second, 1).unwrap();
        let mut rolled_back = f.store.dump().unwrap();
        let task = (rolled_back.tasks.iter_mut())
            .find(|t| t.slug == "sim/jump")
            .unwrap();
        task.status = first.status.clone();
        task.fields.get_mut("_board").unwrap().insert(
            "workflow".into(),
            serde_json::to_value(first.position()).unwrap(),
        );
        let refusal = refused(target.store.copy(&rolled_back));
        assert!(
            refusal.contains(
                "sim/jump moved on from workflow revision 1; it never returns to a position it left"
            ),
            "{backend:?}: {refusal}"
        );
        assert!(target.store.get("sim/kept").is_ok());

        // A copy of a board with no workflow leaves none of the records it replaced.
        f.store.copy(&Dump::default()).unwrap();
        assert_eq!(
            f.store.raw().table("workflow").count().unwrap(),
            0,
            "{backend:?}"
        );
        assert_eq!(f.store.dump().unwrap(), Dump::default());
    }
}

#[test]
fn record_keys_order_as_surrealdb_orders_compound_ids() {
    let mut keys = vec![
        json!(["event", "sim/jump", 10]),
        json!(["active"]),
        json!(["event", "sim/jump", 9]),
        json!(["activation", 2]),
        json!(["event", "sim/a", 1]),
        json!(["definition", "ff"]),
        json!(["activation", 10]),
    ];
    keys.sort_by(key_order);
    assert_eq!(
        keys,
        vec![
            json!(["activation", 2]),
            json!(["activation", 10]),
            json!(["active"]),
            json!(["definition", "ff"]),
            json!(["event", "sim/a", 1]),
            json!(["event", "sim/jump", 9]),
            json!(["event", "sim/jump", 10]),
        ]
    );
    assert_eq!(key_order(&json!(1), &json!("a")), Ordering::Less);
    assert_eq!(key_order(&json!(["a"]), &json!(["a", 1])), Ordering::Less);
}

#[test]
fn schema_24_keeps_every_row_fences_the_writer_before_it_and_its_readers_read_a_managed_task() {
    use crate::store_fixture::{HOST, NOW};
    use crate::{NewTask, Opening, Schema};
    for backend in BACKENDS {
        let database = backend.database();
        let open = |how| {
            Store::opening_on(
                database.another(),
                Box::new(|| NOW),
                Box::new(|_| false),
                HOST,
                &how,
            )
            .unwrap()
        };
        let previous = Schema::this_build().before();
        let older = open(Opening::write().at(previous.clone()));
        older
            .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
            .unwrap();
        older
            .create(&NewTask::new("sim/free", "Free task", "owner"))
            .unwrap();
        let before = older.dump().unwrap();

        let current = open(Opening::write());
        assert_eq!(
            current.dump().unwrap(),
            before,
            "{backend:?}: the migration moved a row"
        );
        assert_eq!(current.raw().table("workflow").count().unwrap(), 0);
        let first = moved("sim/jump", 1, pair("build", "working"), "in_progress");
        current.record_transition(&first, 0).unwrap();

        let refused_older = older.create(&NewTask::new("sim/late", "Late", "owner"));
        assert!(
            matches!(refused_older, Err(Error::Refused(_))),
            "{backend:?}: {refused_older:?}"
        );
        let reader = open(Opening::read().at(previous));
        let task = reader.get("sim/jump").unwrap();
        assert_eq!(task.status, "in_progress");
        assert_eq!(
            task.fields["_board"]["workflow"],
            serde_json::to_value(first.position()).unwrap()
        );
        assert_eq!(
            reader.get("sim/free").unwrap().status,
            before.tasks[1].status
        );
        assert!(
            reader
                .board()
                .unwrap()
                .tasks
                .iter()
                .any(|t| t.slug == "sim/jump")
        );
    }
}

#[test]
fn a_board_made_at_22_or_23_migrates_to_this_builds_schema_with_its_rows_and_opens_there_as_it_is()
{
    use crate::store_fixture::{HOST, NOW};
    use crate::{NewTask, Opening, Schema};
    // What an older build's board lacks of this build's definitions: schema 24's workflow
    // records and their fence on tasks, and below 23 the execution attempts' tables as well.
    const NO_WORKFLOW: (&str, &str) = (
        "REMOVE EVENT workflow_position ON task; REMOVE TABLE workflow;",
        "DROP TRIGGER task_workflow ON task; DROP TABLE workflow;",
    );
    const NO_EXECUTION: (&str, &str) = (
        "REMOVE TABLE execution_attempt; REMOVE TABLE execution_fact; \
         REMOVE TABLE execution_fence; REMOVE TABLE execution_artifact;",
        "DROP TABLE execution_attempt; DROP TABLE execution_fact; DROP TABLE execution_fence; \
         DROP TABLE execution_artifact;",
    );
    let version = |store: &Store| {
        (store.raw().row("meta", "schema").field("version"))
            .unwrap()
            .as_i64()
    };
    // Whether the database defines `table`, as its own catalogue says.
    let defines = |store: &Store, table: &str| -> bool {
        let surreal = format!("RETURN (INFO FOR DB).tables.{table} != NONE;");
        let postgres = format!("to_jsonb(to_regclass('{table}') IS NOT NULL)");
        (store.retrying(|| {
            store.read(
                both(surreal.as_str(), Pg::read(postgres.as_str())),
                json!({}),
            )
        }))
        .unwrap()
    };
    for backend in BACKENDS {
        for (older, lacks) in [
            (22, &[NO_WORKFLOW, NO_EXECUTION][..]),
            (23, &[NO_WORKFLOW][..]),
        ] {
            let database = backend.database();
            let open = |how| {
                Store::opening_on(
                    database.another(),
                    Box::new(|| NOW),
                    Box::new(|_| false),
                    HOST,
                    &how,
                )
                .unwrap()
            };
            let mut schema = Schema::this_build();
            while schema.version() > older {
                schema = schema.before();
            }
            let old = open(Opening::write().at(schema));
            old.create(&NewTask::new("sim/jump", "Jump departure", "owner"))
                .unwrap();
            let task = old.get("sim/jump").unwrap();
            for (surreal, postgres) in lacks {
                old.retrying(|| {
                    old.transact::<Value>(both(*surreal, Pg::script(*postgres)), json!({}))
                })
                .unwrap();
            }
            assert_eq!(version(&old), Some(older));
            assert!(!defines(&old, "workflow"), "{backend:?} at {older}");
            assert_eq!(
                defines(&old, "execution_attempt"),
                older == 23,
                "{backend:?}"
            );

            let current = open(Opening::write());
            assert_eq!(
                version(&current),
                Some(crate::schema::VERSION),
                "{backend:?} from {older}"
            );
            assert_eq!(
                current.get("sim/jump").unwrap(),
                task,
                "{backend:?} from {older}"
            );
            for table in [
                "workflow",
                "execution_attempt",
                "execution_fact",
                "execution_fence",
            ] {
                assert!(
                    defines(&current, table),
                    "{backend:?} from {older}: {table}"
                );
                assert_eq!(current.raw().table(table).count().unwrap(), 0, "{table}");
            }
            // Schema 24's fence is on the board again: a held task's status follows its position.
            let first = moved("sim/jump", 1, pair("build", "working"), "in_progress");
            current.record_transition(&first, 0).unwrap();
            let follows = refused(
                current
                    .raw()
                    .row("task", "sim/jump")
                    .set(json!({"status": "ready"})),
            );
            assert!(
                follows.contains("the status of sim/jump follows its workflow position"),
                "{backend:?} from {older}: {follows}"
            );
            // The older build stops at its next write.
            let late = old.create(&NewTask::new("sim/late", "Late", "owner"));
            assert!(matches!(late, Err(Error::Refused(_))), "{late:?}");

            // At this build's schema it opens as it is, with the one read.
            let again = open(Opening::write());
            assert_eq!(again.database().requests(), 1, "{backend:?} from {older}");
            assert_eq!(again.get("sim/jump").unwrap().status, "in_progress");
            assert_eq!(version(&again), Some(crate::schema::VERSION));
        }
    }
}
