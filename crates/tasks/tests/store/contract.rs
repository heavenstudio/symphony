//! The board as a contract between builds (`schema::VERSION`, `schema::ADDED`): an additive step
//! leaves the contract as it was, so builds on either side of one read and write the board; only a
//! breaking step moves the board under its writers, and only it is held by the writer rule.
//!
//! The steps here are fixtures, a build's that this one lacks: the first defines a table of its
//! own and an optional field on every open table (`schema::OPEN`), the second another table. A
//! build one or two steps behind keeps the field on every row it writes; a build ahead that may
//! not move the board uses it as it is, refused only what needs its steps; one that may adds them.

use crate::common::{HOST, NOW, database, refused};
use crate::copy::everything;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use symphony_board::live::Live;
use symphony_board::schema::{self, COMPATIBLE, OPEN, VERSION, WRITABLE_FROM_SINCE, keeps_writes};
use symphony_board::{Database, Error, NewTask, Opening, Program, Raw, Schema, Standing, Store};

/// The additive steps this build has of its own, which every fixture step comes after.
const OWN: i64 = schema::ADDITIONS;

/// The first fixture step: the table `fixture_tag`, and the optional field `fixture_note` on every
/// open table.
fn one_on() -> Schema {
    let (mut surreal, mut postgres) = (
        "DEFINE TABLE IF NOT EXISTS fixture_tag SCHEMAFULL;
         DEFINE FIELD IF NOT EXISTS name ON fixture_tag TYPE string;"
            .to_string(),
        "CREATE TABLE IF NOT EXISTS fixture_tag (id jsonb PRIMARY KEY, doc jsonb);".to_string(),
    );
    for table in OPEN {
        surreal +=
            &format!("\nDEFINE FIELD IF NOT EXISTS fixture_note ON {table} TYPE option<string>;");
        postgres += &format!("\nALTER TABLE {table} ADD COLUMN IF NOT EXISTS fixture_note text;");
    }
    Schema::this_build().then_additive(&surreal, &postgres, &["fixture_tag"])
}

/// The second fixture step, after the first: the table `fixture_mark`.
fn two_on() -> Schema {
    one_on().then_additive(
        "DEFINE TABLE IF NOT EXISTS fixture_mark SCHEMALESS;",
        "CREATE TABLE IF NOT EXISTS fixture_mark (id jsonb PRIMARY KEY, doc jsonb);",
        &["fixture_mark"],
    )
}

/// A store `how` opens on `db`.
fn opened(db: &Database, how: &Opening) -> Result<Store, Error> {
    Store::opening_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        how,
    )
}

fn open(db: &Database, how: &Opening) -> Store {
    opened(db, how).unwrap()
}

/// A project's live board whose repository commits no schema source, in a folder of its own: a
/// build that opens it moves it only as `just tasks migrate` (`Opening::migrating`).
struct NotMine(PathBuf);

impl NotMine {
    fn new(db: &Database) -> NotMine {
        let dir =
            std::env::temp_dir().join(format!("contract-{}-{}", std::process::id(), db.database()));
        std::fs::create_dir_all(&dir).unwrap();
        NotMine(dir)
    }

    fn live(&self) -> Option<Live> {
        Some(Live {
            project: "fixture".into(),
            common: self.0.clone(),
        })
    }
}

impl Drop for NotMine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The additive steps `meta:additions` records.
fn additions(db: &Database) -> Option<i64> {
    let found = Raw::of(db).row("meta", "additions").field("version");
    found.unwrap().as_i64()
}

/// Every row of each open table, by its id.
fn open_rows(db: &Database) -> BTreeMap<&'static str, BTreeMap<String, Value>> {
    let raw = Raw::of(db);
    (OPEN.iter())
        .map(|table| {
            let rows = raw.table(table).get().unwrap();
            let by_id = (rows.as_array().unwrap().iter())
                .map(|row| (row["id"].to_string(), row.clone()))
                .collect();
            (*table, by_id)
        })
        .collect()
}

/// What a build does on the board of `everything()`, reading it and writing rows of its open
/// tables already there: a task's priority, a launch record reopened and closed, a line said, an
/// owner's line read, an ask held or let go (`holder`); and a task made, `made`.
fn worked(store: &Store, holder: Option<&str>, priority: i64, made: &str) {
    store.set_priority("sim", priority, "owner").unwrap();
    store.set_priority("sim/jump", priority, "owner").unwrap();
    store
        .reopen_task_session("sim/jump", "amber-heron", "p2")
        .unwrap();
    (store.close_task_session("sim/jump", "amber-heron", "done again", "r2")).unwrap();
    store
        .say("sim/jump", "amber-heron", "still kept", None)
        .unwrap();
    store.ack("amber-heron", &[4, 9]).unwrap();
    store.hold_ask(2, holder, "taken up").unwrap();
    store
        .create(&NewTask {
            links: vec!["docs/game/SPEC.md#Jumping".into()],
            depends: vec!["sim".into()],
            ..NewTask::new(made, "Land it", "owner")
        })
        .unwrap();
    // And every read of those rows, each past the field it does not know.
    assert_eq!(store.get("sim").unwrap().priority, priority);
    assert!(store.tasks(None).unwrap().len() >= 3);
    assert!(!store.thread("sim/jump").unwrap().is_empty());
    assert!(!store.sessions().unwrap().is_empty());
    store.session("amber-heron").unwrap();
    assert_eq!(
        store.task_sessions("sim/jump").unwrap()[0].closed_why,
        "done again"
    );
    assert!(store.asks(Some(0.0)).unwrap().len() >= 2);
    store.ask(3).unwrap();
    assert_eq!(store.spent_on(&["sim/jump"]).unwrap().len(), 1);
    store.linked_to("docs/game/SPEC.md#Jumping").unwrap();
    store.inbox("brisk-finch").unwrap();
}

#[test]
fn a_whole_board_read_refuses_a_field_its_rows_do_not_name_which_every_other_read_passes() {
    let db = database();
    // A build whose own step defines a field its row structs do not name.
    let store = open(&db, &Opening::write().at(one_on()));
    store.copy(&everything()).unwrap();
    assert_eq!(additions(&db), Some(OWN + 1));
    let clean = store.dump().unwrap();
    let raw = Raw::of(&db);
    raw.row("task", "sim/jump")
        .set(json!({"fixture_note": "kept"}))
        .unwrap();
    let rows = open_rows(&db);
    // Its steps are the board's, so only the exact read stands between the field and a copy,
    // an export or a move to another engine that would drop it.
    let why = refused(store.dump());
    assert!(
        why.starts_with("the board's rows are not the dump's: unknown field `fixture_note`"),
        "{why}"
    );
    assert_eq!(open_rows(&db), rows, "a refused dump writes nothing");
    // Every other read and write passes it, and keeps it.
    let jump = clean
        .tasks
        .iter()
        .find(|task| task.slug == "sim/jump")
        .unwrap();
    assert_eq!(store.get("sim/jump").unwrap().title, jump.title);
    store.set_priority("sim/jump", 4, "owner").unwrap();
    assert_eq!(
        raw.row("task", "sim/jump").field("fixture_note").unwrap(),
        "kept"
    );
    assert_eq!(store.get("sim/jump").unwrap().priority, 4);
}

#[test]
fn a_build_steps_behind_reads_and_writes_a_board_ahead_and_keeps_what_it_does_not_know() {
    let db = database();
    let ahead = open(&db, &Opening::write().at(two_on()));
    ahead.copy(&everything()).unwrap();
    assert_eq!(
        (ahead.board_version().unwrap(), additions(&db)),
        (Some(VERSION), Some(OWN + 2))
    );
    assert_eq!(ahead.additions().unwrap(), OWN + 2);
    // Every row of every open table carries the field the steps added.
    let raw = Raw::of(&db);
    for table in OPEN {
        raw.table(table)
            .set(json!({"fixture_note": "kept"}))
            .unwrap();
    }
    let before = open_rows(&db);
    for table in OPEN {
        assert!(!before[table].is_empty(), "{table} has rows");
    }
    // Two steps behind, then one: each opens, reads and writes, and moves nothing back.
    for (behind, holder, priority, made) in [
        (Schema::this_build(), Some("amber-heron"), 2, "sim/land"),
        (one_on(), None, 3, "sim/land/again"),
    ] {
        let steps = behind.additions();
        let store = open(&db, &Opening::write().at(behind));
        assert_eq!(
            store.additions().unwrap(),
            steps,
            "what it may use of the board"
        );
        if steps == OWN + 1 {
            // Its own step's table is there to write.
            raw.row("fixture_tag", "one")
                .create(json!({"name": "one"}))
                .unwrap();
        }
        worked(&store, holder, priority, made);
        assert_eq!(
            additions(&db),
            Some(OWN + 2),
            "a build behind moves no step back"
        );
        // A whole-board read would leave out what its steps lack: refused, naming them.
        let why = refused(store.dump());
        assert!(
            why.contains(&format!(
                "has {} of schema {VERSION}'s additive steps and this build has {steps}",
                OWN + 2
            )),
            "{why}"
        );
    }
    // No row there before lost the field, whatever was written to it.
    let after = open_rows(&db);
    for table in OPEN {
        for id in before[table].keys() {
            if let Some(now) = after[table].get(id) {
                assert_eq!(now["fixture_note"], "kept", "{table} {id}: {now}");
            }
        }
    }
    // And the writes did write them: the tasks, the launch record, the ask.
    for table in ["task", "task_session", "ask"] {
        let changed = (before[table].iter())
            .filter(|(id, row)| after[table].get(*id) != Some(row))
            .count();
        assert!(changed > 0, "{table} rows were written");
    }
    // A row made by a build behind has no field it does not know; the build ahead reads all.
    let made = Raw::of(&db).row("task", "sim/land").get().unwrap();
    assert!(made["fixture_note"].is_null(), "{made}");
    assert!(ahead.tasks(None).unwrap().len() >= 3);
}

#[test]
fn a_build_ahead_that_may_not_move_a_board_uses_it_as_it_is_and_one_that_may_adds_its_steps() {
    let db = database();
    let gate = Program::named("gate-review", "/work/alpha/.target/debug/gate-review");
    let behind = open(&db, &Opening::write().by(gate));
    behind.copy(&everything()).unwrap();
    assert_eq!(additions(&db), Some(OWN));
    let not_mine = NotMine::new(&db);
    // Opened as it is: no step added, and nothing written of what its steps define.
    let ahead = open(&db, &Opening::write().at(two_on()).live(not_mine.live()));
    assert_eq!(
        (additions(&db), ahead.additions().unwrap()),
        (Some(OWN), OWN)
    );
    assert!(
        Raw::of(&db).table("fixture_tag").get().is_err(),
        "no table of its steps"
    );
    worked(&ahead, Some("amber-heron"), 2, "sim/land");
    // A reader ahead reads it too.
    let reader = open(&db, &Opening::read().at(two_on()));
    assert!(reader.tasks(None).unwrap().len() >= 3);
    // What needs its steps is refused, and writes nothing: a whole-board read, and a copy.
    let why = refused(ahead.dump());
    assert!(
        why.contains(&format!(
            "has {OWN} of schema {VERSION}'s additive steps and this build has {}",
            OWN + 2
        )),
        "{why}"
    );
    let rows = open_rows(&db);
    let why = refused(ahead.copy(&everything()));
    assert!(why.contains("additive steps"), "{why}");
    assert_eq!(open_rows(&db), rows, "a refused copy writes nothing");
    assert!(Raw::of(&db).table("fixture_tag").get().is_err());
    // One that may move it adds the steps, and holds no writer: gate-review, seen a moment ago
    // at the board's contract without them, keeps writing it.
    let mover = open(&db, &Opening::write().at(two_on()).migrating());
    assert_eq!(
        (
            additions(&db),
            mover.additions().unwrap(),
            ahead.additions().unwrap()
        ),
        (Some(OWN + 2), OWN + 2, OWN + 2)
    );
    assert_eq!(
        behind.additions().unwrap(),
        OWN,
        "what it may use of the board"
    );
    Raw::of(&db)
        .row("fixture_tag", "one")
        .create(json!({"name": "one"}))
        .unwrap();
    worked(&behind, None, 3, "sim/land/again");
    assert_eq!(
        open(&db, &Opening::write()).board_version().unwrap(),
        Some(VERSION)
    );
}

#[test]
fn a_breaking_step_refuses_builds_on_the_other_side_and_alone_holds_the_writers_behind() {
    let db = database();
    let gate = Program::named("gate-review", "/work/alpha/.target/debug/gate-review");
    open(&db, &Opening::write().by(gate))
        .copy(&everything())
        .unwrap();
    // An additive move of a board gate-review wrote a moment ago goes ahead.
    open(&db, &Opening::write().at(two_on()).migrating());
    assert_eq!(additions(&db), Some(OWN + 2));
    // A breaking one is held by it, before any write.
    let breaking = || {
        two_on().then_each(
            "DEFINE FIELD OVERWRITE title ON task TYPE option<string>;",
            "ALTER TABLE task ALTER COLUMN title DROP NOT NULL;",
        )
    };
    let why = refused(opened(&db, &Opening::write().at(breaking()).migrating()).map(drop));
    assert!(
        why.starts_with(&format!(
            "this board has a writer seen in the last week behind schema {}, the one this \
                 migrate moves it to",
            VERSION + 1
        )),
        "{why}"
    );
    assert_eq!(
        (
            open(&db, &Opening::write()).board_version().unwrap(),
            additions(&db)
        ),
        (Some(VERSION), Some(OWN + 2))
    );
    // Told to leave it behind, it moves the board, which counts its own steps on the new
    // contract, none; and every build of the contract before is refused, its steps and all.
    let newer = open(
        &db,
        &Opening::write().at(breaking()).migrating().leave_behind(),
    );
    assert_eq!(
        (newer.board_version().unwrap(), additions(&db)),
        (Some(VERSION + 1), Some(0))
    );
    for older in [Schema::this_build(), two_on()] {
        let why = refused(opened(&db, &Opening::write().at(older)).map(drop));
        assert!(
            why.contains(&format!("is at schema {}", VERSION + 1)),
            "{why}"
        );
    }
    newer
        .create(&NewTask::new("sim/after", "After the move", "owner"))
        .unwrap();
}

#[test]
fn a_board_not_made_yet_has_no_additive_step_and_the_read_of_them_makes_nothing() {
    // `tasks migrate` reads the steps a board has before it opens it to write, on a board that
    // may not be made: SurrealDB answers no record, PostgreSQL no table.
    let db = database();
    let reader = Store::unchecked_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::read(),
    );
    assert_eq!(reader.additions().unwrap(), 0);
    assert_eq!(reader.board_version().unwrap(), None);
    // Its steps read once the board is made by a build that has one: the least of the board's
    // and its own, so a build without the step reads none.
    let made = open(&db, &Opening::write().at(one_on()));
    assert_eq!(
        (made.additions().unwrap(), reader.additions().unwrap()),
        (OWN + 1, OWN)
    );
    assert_eq!(additions(&db), Some(OWN + 1));
}

#[test]
fn the_rule_before_the_contract_keeps_its_names_and_works_beside_additive_steps() {
    // Its names, as the Symphony and a consumer's code call them: kept beside the contract's
    // until a later task removes them (the two-release rule).
    let _: &[(i64, &str)] = COMPATIBLE;
    let _: fn(i64) -> bool = keeps_writes;
    let _: i64 = WRITABLE_FROM_SINCE;
    let _: fn(Schema) -> Schema = Schema::keeping_writes;
    let _: fn(&Schema) -> i64 = Schema::writable_from;
    let _: fn(&Schema, &str) -> Schema = Schema::then;
    let _: fn(&Schema, &str, &str) -> Schema = Schema::then_each;
    let _: fn(&Schema) -> Schema = Schema::before;
    let _: fn(Option<i64>, &Value, i64, bool) -> bool = schema::writes;
    let _: fn(&Store) -> Result<Standing, Error> = Store::board_standing;
    let _ = |standing: Standing| (standing.version, standing.writes);
    // The contract's, beside them.
    let _: (i64, i64) = (schema::ADDITIONS, VERSION);
    let _: fn(&Schema) -> i64 = Schema::additions;
    let _: fn(&Schema, &str, &str, &[&str]) -> Schema = Schema::then_additive;
    let _: fn(i64) -> Schema = Schema::of_this_contract;
    let _: fn(&Store) -> Result<i64, Error> = Store::additions;
    // What they say of this build: no step of it declares itself, and a schema a step on or
    // back is a breaking one's, with no additive step; one additive step on is the same
    // contract, with the same writers.
    assert!(!keeps_writes(VERSION));
    assert!(COMPATIBLE.iter().all(|(listed, _)| *listed < VERSION));
    let this = Schema::this_build();
    assert_eq!(this.writable_from(), VERSION);
    assert_eq!(this.clone().keeping_writes().writable_from(), VERSION - 1);
    for (moved, version) in [
        (this.before(), VERSION - 1),
        (two_on().then("RETURN NONE;"), VERSION + 1),
    ] {
        assert_eq!((moved.version(), moved.additions()), (version, 0));
    }
    assert_eq!(
        (two_on().version(), two_on().writable_from()),
        (VERSION, VERSION)
    );
    assert!(schema::writes(Some(VERSION), &Value::Null, VERSION, false));
    // On a board with steps this build lacks, it stands at its own schema and writes.
    let db = database();
    open(&db, &Opening::write().at(two_on()));
    for how in [Opening::write(), Opening::read()] {
        let store = open(&db, &how);
        assert_eq!(
            store.board_standing().unwrap(),
            Standing {
                version: Some(VERSION),
                writes: true
            }
        );
    }
    // A step declared to keep a Symphony one behind writing still does, whatever additive
    // steps the newer contract then has.
    let next = two_on()
        .then_each(
            "DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;",
            "CREATE TABLE IF NOT EXISTS fixture_next (id jsonb PRIMARY KEY, doc jsonb);",
        )
        .keeping_writes()
        .then_additive(
            "DEFINE TABLE IF NOT EXISTS fixture_later SCHEMALESS;",
            "CREATE TABLE IF NOT EXISTS fixture_later (id jsonb PRIMARY KEY, doc jsonb);",
            &["fixture_later"],
        );
    assert_eq!(
        (next.version(), next.writable_from(), next.additions()),
        (VERSION + 1, VERSION, 1)
    );
    open(&db, &Opening::write().at(next).migrating().leave_behind());
    assert_eq!(additions(&db), Some(1));
    let reader = open(&db, &Opening::read());
    assert_eq!(
        reader.board_standing().unwrap(),
        Standing {
            version: Some(VERSION + 1),
            writes: true
        }
    );
    reader
        .create(&NewTask::new(
            "sim/next",
            "Filed one schema behind",
            "owner",
        ))
        .unwrap();
    let ahead = open(&db, &Opening::read().at(two_on()));
    assert_eq!(
        ahead.get("sim/next").unwrap().title,
        "Filed one schema behind"
    );
}
