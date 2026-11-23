//! The workflow's records and the fences on them: what the store writes, and what every other
//! writer, the database's own statements included, is refused.

use super::*;
use crate::rows::Dump;
use crate::store::postgres::Pg;
use crate::store_fixture::{Fixture, Seed, pair, refused};
use symphony_plugin::execution::PrincipalKind;
#[test]
fn a_position_and_a_transition_are_read_by_field_as_their_decoders_read_them() {
    let full = json!({
        "phase": "build", "state": "working", "resume": {"phase": "build", "state": "available"},
        "generation": 2, "revision": 7, "digest": "d1",
        "manual": {"trigger": "lost_holder", "holder": "amber", "role": "executor", "manual": true},
    });
    let with = |key: &str, value: Value| {
        let mut changed = full.clone();
        changed[key] = value;
        changed
    };
    let without = |key: &str| {
        let mut changed = full.clone();
        changed.as_object_mut().unwrap().remove(key);
        changed
    };
    let cases = [
        full.clone(),
        without("resume"),
        without("manual"),
        with("resume", Value::Null),
        with("manual", Value::Null),
        with(
            "manual",
            json!({"trigger": "release", "holder": "a", "role": "reviewer", "manual": false}),
        ),
        with(
            "manual",
            json!({"trigger": "attempt_failure", "holder": "ab12", "role": "checker", "manual": true}),
        ),
        // Each refused as its decoder refuses it.
        without("digest"),
        without("generation"),
        with("revision", json!("7")),
        with("revision", json!(-1)),
        with("revision", json!(7.5)),
        with("phase", json!(1)),
        with("extra", json!(1)),
        with("resume", json!({"phase": "build"})),
        with(
            "resume",
            json!({"phase": "build", "state": "a", "extra": 1}),
        ),
        with(
            "manual",
            json!({"trigger": "gone", "holder": "a", "role": "r", "manual": true}),
        ),
        with(
            "manual",
            json!({"trigger": "release", "holder": "a", "role": "r", "manual": "yes"}),
        ),
        with(
            "manual",
            json!({"trigger": "release", "holder": "a", "role": "r", "manual": true, "x": 1}),
        ),
        with("manual", json!("lost_holder")),
        with(
            "manual",
            json!({"trigger": "cancellation_acknowledged", "holder": "a", "role": "r", "manual": true, "attempt": "a"}),
        ),
        json!(["build", "working"]),
        Value::Null,
    ];
    let mut read = 0;
    for case in cases {
        let decoded: Option<TaskPosition> = serde_json::from_value(case.clone()).ok();
        assert_eq!(TaskPosition::of(&case), decoded, "{case}");
        read += usize::from(decoded.is_some());
    }
    assert_eq!(read, 7, "the cases the decoder reads");
    let position = TaskPosition::of(&full).unwrap();
    assert_eq!(
        (position.at(), position.revision),
        (pair("build", "working"), 7)
    );

    // A transition's body is read for what the alarm says of it, whatever else it records.
    let body =
        serde_json::to_value(moved("sim/a", 2, pair("build", "working"), "in_progress")).unwrap();
    let read = Moved::of(&body).unwrap();
    assert_eq!(
        (read.slug.as_str(), read.revision, read.status.as_str()),
        ("sim/a", 2, "in_progress")
    );
    assert_eq!(read.at, body["at"].as_f64().unwrap());
    for missing in ["slug", "revision", "status", "at"] {
        let mut short = body.clone();
        short.as_object_mut().unwrap().remove(missing);
        assert_eq!(Moved::of(&short), None, "{missing}");
    }
}

/// Every position the types can hold, each optional field set and each trigger, is read by field
/// as it is written: a field the types gain breaks the literals below and a trigger breaks the
/// match, and each fails here until `TaskPosition::of` reads it, so a position a newer writer
/// records is never read as no position.
#[test]
fn every_position_the_types_hold_is_read_by_field_as_it_is_written() {
    let triggers = [
        Trigger::Release,
        Trigger::LostHolder,
        Trigger::AttemptFailure,
        Trigger::CancellationAcknowledged,
    ]
    .map(|trigger| match trigger {
        // A trigger the enum gains stops compiling here: list it above as well.
        Trigger::Release
        | Trigger::LostHolder
        | Trigger::AttemptFailure
        | Trigger::CancellationAcknowledged => trigger,
    });
    for trigger in triggers {
        let position = TaskPosition {
            phase: "build".into(),
            state: "working".into(),
            resume: Some(pair("build", "available")),
            generation: 2,
            revision: 7,
            digest: "d1".into(),
            manual: Some(Recovered {
                trigger,
                holder: "amber".into(),
                role: "executor".into(),
                manual: true,
            }),
        };
        let written = serde_json::to_value(&position).unwrap();
        assert_eq!(TaskPosition::of(&written), Some(position), "{written}");
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
        accepted: None,
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

#[test]
fn a_board_without_a_workflow_holds_none_of_its_records() {
    let f = Fixture::new(Seed::default());
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

#[test]
fn a_transition_is_recorded_beside_its_position_once() {
    let f = Fixture::new(Seed::default());
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

#[test]
fn no_writer_moves_a_managed_task_past_its_record() {
    let f = Fixture::new(Seed::default());
    let first = managed(&f, "sim/jump");
    let task = || f.store.raw().row("task", "sim/jump");
    let follows = refused(task().set(json!({"status": "ready"})));
    assert!(
        follows.contains("the status of sim/jump follows its workflow position"),
        "{follows}"
    );
    let beside = refused(task().set(json!({"fields._board.workflow.phase": "deploy"})));
    assert!(
        beside.contains("sim/jump moves from workflow revision 1 only to the next"),
        "{beside}"
    );
    let forged = refused(task().set(json!({
        "fields._board.workflow.phase": "deploy",
        "fields._board.workflow.revision": 2,
    })));
    assert!(
        forged.contains("sim/jump is at a workflow position only beside the transition"),
        "{forged}"
    );
    let removed = refused(task().set(json!({"fields._board.workflow": null})));
    assert!(
        removed.contains("sim/jump keeps its workflow position"),
        "{removed}"
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
        "{moved_by_hand}"
    );
    // The owner's own move meets the fence itself.
    let owner = refused(f.store.move_to("sim/held", "cancelled", "owner"));
    assert!(
        owner.starts_with("explicit_action_required: the status of sim/held follows its"),
        "{owner}"
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
        "{planted}"
    );
}

#[test]
fn nothing_writes_a_workflow_record_past_its_rules() {
    let f = Fixture::new(Seed::default());
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

/// The database's own statements, as a hand at its server would send them.
fn by_hand(f: &Fixture, postgres: &'static str) -> Result<Value, Error> {
    f.store.retrying(|| {
        f.store
            .transact(Statement::from(Pg::script(postgres)), json!({}))
    })
}

#[test]
fn the_database_never_changes_a_recorded_transition_or_moves_the_active_generation_back() {
    let f = Fixture::new(Seed::default());
    managed(&f, "sim/jump");
    let changed = refused(by_hand(
        &f,
        "UPDATE workflow SET body = body || '{\"status\": \"done\"}' \
         WHERE id = '[\"event\", \"sim/jump\", 1]';",
    ));
    assert_eq!(changed, "workflow: a recorded event is never changed");
    let retyped = refused(by_hand(
        &f,
        "UPDATE workflow SET kind = 'intent' WHERE id = '[\"event\", \"sim/jump\", 1]';",
    ));
    assert_eq!(retyped, "workflow: a recorded event is never changed");

    by_hand(
        &f,
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
        "UPDATE workflow SET body = '{\"generation\": 2, \"digest\": \"d2\"}' WHERE id = '[\"active\"]';",
    ));
    assert_eq!(
        unrecorded,
        "workflow: the active generation moves forward only to an activation recorded for it"
    );
    let activation = refused(by_hand(
        &f,
        "UPDATE workflow SET body = body || '{\"digest\": \"d2\"}' WHERE id = '[\"activation\", 1]';",
    ));
    assert_eq!(
        activation,
        "workflow: a recorded activation is never changed"
    );
    assert_eq!(f.store.workflow_active().unwrap().unwrap().generation, 1);
}

#[test]
fn the_database_retires_a_position_only_beside_a_returns_transition() {
    let f = Fixture::new(Seed::default());
    managed(&f, "sim/jump");
    managed(&f, "sim/two");
    // The next transition of each, as an activation's: 7 an ordinary one, 8 a return.
    by_hand(
        &f,
        "INSERT INTO workflow (id, kind, body, at) VALUES \
         ('[\"activation\", 7]', 'activation', '{\"generation\": 7, \"digest\": \"d7\"}', 1.0), \
         ('[\"activation\", 8]', 'activation', '{\"generation\": 8, \"digest\": \"builtin-v1\", \"returned\": true}', 1.0), \
         ('[\"event\", \"sim/jump\", 2]', 'event', '{\"slug\": \"sim/jump\", \"revision\": 2, \"status\": \"ready\", \"activation\": 7}', 1.0), \
         ('[\"event\", \"sim/two\", 2]', 'event', '{\"slug\": \"sim/two\", \"revision\": 2, \"status\": \"ready\", \"activation\": 8}', 1.0);",
    )
    .unwrap();
    let retire = |slug: &str, status: &str| {
        f.store.raw().row("task", slug).set(json!({
            "status": status,
            "fields._board.workflow": null,
        }))
    };
    let keeps = "keeps its workflow position: only an activation that returns the board to \
                 its own writers retires it";
    // Beside an activation that is no return.
    let ordinary = refused(retire("sim/jump", "ready"));
    assert!(ordinary.contains(keeps), "{ordinary}");
    // Beside a return's transition, at another status than it recorded.
    let elsewhere = refused(retire("sim/two", "done"));
    assert!(elsewhere.contains(keeps), "{elsewhere}");
    assert!(f.store.workflow_position("sim/two").unwrap().is_some());
    // Beside it, at its status: the position goes, and the task is the board's writers'.
    retire("sim/two", "ready").unwrap();
    assert_eq!(f.store.workflow_position("sim/two").unwrap(), None);
    assert_eq!(f.store.get("sim/two").unwrap().status, "ready");
    f.store
        .raw()
        .row("task", "sim/two")
        .set(json!({"status": "review"}))
        .unwrap();
    // It never returns to the position its return left.
    let first = moved("sim/two", 1, pair("build", "working"), "in_progress");
    let back = refused(f.store.raw().row("task", "sim/two").set(json!({
        "status": "in_progress",
        "fields._board.workflow": first.position(),
    })));
    assert!(
        back.contains("sim/two moved on from workflow revision 1"),
        "{back}"
    );
}

#[test]
fn the_database_never_changes_an_admission_or_lowers_an_authority_within_its_epoch() {
    let f = Fixture::new(Seed::default());
    by_hand(
        &f,
        "INSERT INTO workflow (id, kind, body, at) VALUES \
         ('[\"admission\", \"sim/a\", 4]', 'admission', '{\"edge\": \"accept\"}', 1.0), \
         ('[\"authority\", \"approvals\"]', 'authority', \
          '{\"epoch\": \"e1\", \"generation\": 3, \"state\": \"s3\"}', 1.0);",
    )
    .unwrap();
    let admission = refused(by_hand(
        &f,
        "UPDATE workflow SET body = body || '{\"edge\": \"other\"}' \
         WHERE id = '[\"admission\", \"sim/a\", 4]';",
    ));
    assert_eq!(admission, "workflow: a recorded admission is never changed");
    let lowered = refused(by_hand(
        &f,
        "UPDATE workflow SET body = body || '{\"generation\": 2}' \
         WHERE id = '[\"authority\", \"approvals\"]';",
    ));
    assert_eq!(
        lowered,
        "workflow: the generation the board accepted from an approval authority is never \
         lowered within its epoch"
    );
    // Forward within its epoch, a mark of conflict, or a new epoch at any generation is a move.
    by_hand(
        &f,
        "UPDATE workflow SET body = body || '{\"generation\": 4, \"conflicted\": true}' \
         WHERE id = '[\"authority\", \"approvals\"]'; \
         UPDATE workflow SET body = body || '{\"epoch\": \"e2\", \"generation\": 1}' \
         WHERE id = '[\"authority\", \"approvals\"]';",
    )
    .unwrap();
    let retyped = refused(by_hand(
        &f,
        "UPDATE workflow SET kind = 'intent' WHERE id = '[\"authority\", \"approvals\"]';",
    ));
    assert_eq!(retyped, "workflow: a recorded authority is never changed");
}

#[test]
fn a_copy_carries_the_workflow_and_refuses_positions_without_their_records() {
    let f = Fixture::new(Seed::default());
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
    let copied = Fixture::new(Seed { dump: dump.clone() });
    assert_eq!(copied.store.dump().unwrap(), dump);
    assert_eq!(
        copied.store.workflow_position("sim/jump").unwrap(),
        Some(first.position())
    );

    // A dump that lost the records cannot put a managed task back: the copy is refused
    // whole and the board is left as it was.
    let lost = Dump {
        workflow: vec![],
        ..dump.clone()
    };
    let target = Fixture::new(Seed::default());
    target.task("sim/kept");
    let refusal = refused(target.store.copy(&lost));
    assert!(
        refusal.contains("sim/jump is at a workflow position only beside the transition"),
        "{refusal}"
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
        "{refusal}"
    );
    assert!(target.store.get("sim/kept").is_ok());

    // A copy of a board with no workflow leaves none of the records it replaced.
    f.store.copy(&Dump::default()).unwrap();
    assert_eq!(f.store.raw().table("workflow").count().unwrap(), 0);
    assert_eq!(f.store.dump().unwrap(), Dump::default());
}

#[test]
fn record_keys_order_part_by_part_a_shorter_key_first() {
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
fn a_schema_step_keeps_every_row_fences_the_writer_before_it_and_its_readers_read_a_managed_task() {
    use crate::store_fixture::{HOST, NOW, database};
    use crate::{NewTask, Opening, Schema};
    let database = database();
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
    assert_eq!(current.dump().unwrap(), before, "the migration moved a row");
    assert_eq!(current.raw().table("workflow").count().unwrap(), 0);
    let first = moved("sim/jump", 1, pair("build", "working"), "in_progress");
    current.record_transition(&first, 0).unwrap();

    let refused_older = older.create(&NewTask::new("sim/late", "Late", "owner"));
    assert!(
        matches!(refused_older, Err(Error::Refused(_))),
        "{refused_older:?}"
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

#[test]
fn a_board_made_at_an_older_schema_migrates_to_this_one_with_its_rows_and_then_opens_as_it_is() {
    use crate::store_fixture::{HOST, NOW, database};
    use crate::{NewTask, Opening, Schema};
    // What an older build's board lacks of this build's definitions: schema 24's workflow
    // records and their fence on tasks, with its triggers' functions, and below 23 the execution
    // attempts' tables as well.
    const NO_WORKFLOW: &str = "DROP TRIGGER task_workflow ON task; DROP TABLE workflow; \
         DROP FUNCTION board_workflow_position(); DROP FUNCTION board_workflow_history();";
    const NO_EXECUTION: &str = "DROP TABLE execution_attempt; DROP TABLE execution_fact; DROP TABLE execution_fence; \
         DROP TABLE execution_artifact;";
    let version = |store: &Store| {
        (store.raw().row("meta", "schema").field("version"))
            .unwrap()
            .as_i64()
    };
    // Whether the database defines `table`, as its own catalogue says.
    let defines = |store: &Store, table: &str| -> bool {
        let postgres = format!("to_jsonb(to_regclass('{table}') IS NOT NULL)");
        (store.retrying(|| store.read(Statement::from(Pg::read(postgres.as_str())), json!({}))))
            .unwrap()
    };
    for (older, lacks) in [
        (22, &[NO_WORKFLOW, NO_EXECUTION][..]),
        (23, &[NO_WORKFLOW][..]),
        // 24 to 26 have every table: what they lack is the fence that lets a position go.
        (24, &[][..]),
        (25, &[][..]),
        (26, &[][..]),
    ] {
        let database = database();
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
        for postgres in lacks {
            old.retrying(|| {
                old.transact::<Value>(Statement::from(Pg::script(*postgres)), json!({}))
            })
            .unwrap();
        }
        assert_eq!(version(&old), Some(older));
        assert_eq!(defines(&old, "workflow"), older >= 24, "at {older}");
        assert_eq!(defines(&old, "execution_attempt"), older >= 23);

        let current = open(Opening::write());
        assert_eq!(
            version(&current),
            Some(crate::schema::VERSION),
            "from {older}"
        );
        assert_eq!(current.get("sim/jump").unwrap(), task, "from {older}");
        for table in [
            "workflow",
            "execution_attempt",
            "execution_fact",
            "execution_fence",
        ] {
            assert!(defines(&current, table), "from {older}: {table}");
            assert_eq!(current.raw().table(table).count().unwrap(), 0, "{table}");
        }
        // The workflow's fence is on the board again: a held task's status follows its position.
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
            "from {older}: {follows}"
        );
        // The older build stops at its next write.
        let late = old.create(&NewTask::new("sim/late", "Late", "owner"));
        assert!(matches!(late, Err(Error::Refused(_))), "{late:?}");

        // At this build's schema it opens as it is, with the one read.
        let again = open(Opening::write());
        assert_eq!(again.database().requests(), 1, "from {older}");
        assert_eq!(again.get("sim/jump").unwrap().status, "in_progress");
        assert_eq!(version(&again), Some(crate::schema::VERSION));
    }
}
