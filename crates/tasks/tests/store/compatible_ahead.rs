//! A store opened to read writes a board one schema ahead of it only when that schema's step says
//! it keeps a Symphony one schema behind writing (`schema::COMPATIBLE`), by the record a migration
//! writes beside the version (`meta:schema`'s `writable_from`), which the guard reads in each
//! write's own transaction. On either database:
//!
//! - every migration writes the record with the version, for a step declared and one that is not,
//!   and for a move of two steps at once;
//! - a reader opened before the move and one opened after write a board whose record lets them,
//!   and every other store is refused there as before: a writer opened before the move, a store
//!   taken unchecked (a writer's or a reader's opening), and a whole-board copy or a rebind by the
//!   reader itself;
//! - an undeclared step, a record missing or of another shape, a board two ahead, a board behind,
//!   and a record a later move replaced each keep the reader refused;
//! - a write bound to a recovery generation is fenced on it one ahead as at its own schema.
//!
//! These stores all run this build's code: an older schema is `Schema::before` or this one, and a
//! newer one `Schema::then_each`, so what they prove is the guard and the record, on the database,
//! never the code of a released Symphony.

use crate::common::{HOST, NOW, database, refused};
use crate::recovery::Memory;
use serde_json::{Value, json};
use std::sync::Arc;
use symphony_board::recovery::Authority;
use symphony_board::rows::Dump;
use symphony_board::{Database, Error, NewTask, Opening, Raw, Schema, Standing, Store};

/// The store `how` opens on `db` by the rule, its clock at `NOW` and no process alive.
fn opened(db: Database, how: &Opening) -> Result<Store, Error> {
    Store::opening_on(db, Box::new(|| NOW), Box::new(|_| false), HOST, how)
}

/// The store the Symphony opens on `db`: the reader's own entry ([`Store::reading_on`]).
fn reader(db: &Database, schema: Schema) -> Store {
    let how = Opening::read().at(schema).of(Some("alpha"));
    Store::reading_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &how,
    )
    .unwrap()
}

/// A store taken unchecked on `db` by `how`: no opening read, so no role but the schema.
fn unchecked(db: &Database, how: &Opening) -> Store {
    Store::unchecked_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        how,
    )
}

/// The board on `db` moved to `schema` by a writer of it.
fn moved(db: &Database, schema: Schema) -> Store {
    opened(db.another(), &Opening::write().at(schema)).unwrap()
}

/// The version and the `writable_from` the board's record holds (null: none).
fn record(db: &Database) -> (Option<i64>, Value) {
    let row = Raw::of(db)
        .row("meta", "schema")
        .fields(&["version", "writable_from"])
        .unwrap();
    (row["version"].as_i64(), row["writable_from"].clone())
}

/// A table of the test's own, named `name`, as a schema step in each database's language.
fn table_step(name: &str) -> (String, String) {
    (
        format!("DEFINE TABLE IF NOT EXISTS {name} SCHEMALESS;"),
        format!("CREATE TABLE IF NOT EXISTS {name} (id jsonb PRIMARY KEY, doc jsonb);"),
    )
}

/// `schema` and then a step adding the table `name` alone: a newer build's, declaring nothing.
fn then_table(schema: &Schema, name: &str) -> Schema {
    let (surreal, postgres) = table_step(name);
    schema.then_each(&surreal, &postgres)
}

/// The next build's schema, its step declared to keep the Symphony one behind writing.
fn listed() -> Schema {
    then_table(&Schema::this_build(), "fixture_next").keeping_writes()
}

/// The next build's schema, its step declaring nothing.
fn unlisted() -> Schema {
    then_table(&Schema::this_build(), "fixture_next")
}

/// A board made at this build's version on `backend`, holding the task `old/one`: its database.
fn board() -> Database {
    let db = database();
    moved(&db, Schema::this_build())
        .create(&NewTask::new("old/one", "Made at this version", "owner"))
        .unwrap();
    db
}

/// A write of the Symphony's kind by `store`: a line on `old/one`'s thread saying `text`.
fn said(store: &Store, text: &str) -> Result<(), Error> {
    store.say("old/one", "owner", text, None).map(drop)
}

/// The lines of `old/one`'s thread, as the board's own build reads them.
fn thread(db: &Database) -> Vec<String> {
    let store = unchecked(db, &Opening::write());
    let lines = store.thread("old/one").unwrap();
    lines.into_iter().map(|line| line.text).collect()
}

/// What refuses a write of `store` on a board at `found`: the store's own words for it.
fn refused_at(store: &Store, found: i64) -> String {
    let why = refused(said(store, "refused"));
    assert!(
        why.starts_with(&format!("the alpha board is at schema {found}, "))
            || why.starts_with(&format!("this board is at schema {found}, ")),
        "{why}"
    );
    why
}

/// `db`'s record of the oldest schema whose Symphony may write it replaced by `from`, a value no
/// build writes: the field, or column, retyped to hold it. The database itself says it holds no
/// integer there, as a read back through the fixture gives a whole float as an integer. SurrealDB
/// takes a whole float for the integer already there and writes nothing, so the field is unset
/// first.
fn malformed(db: &Database, from: &Value) {
    match db {
        Database::SurrealDb(client) => {
            let held = client
                .query(
                    "DEFINE FIELD OVERWRITE writable_from ON meta TYPE any;
                     UPDATE meta:schema SET writable_from = NONE RETURN NONE;
                     UPDATE meta:schema SET writable_from = IF $float { <float> $from } ELSE { $from }
                         RETURN NONE;
                     RETURN type::is_int((SELECT VALUE writable_from FROM ONLY meta:schema));",
                    json!({ "from": from, "float": from.is_f64() }),
                )
                .unwrap();
            assert_eq!(held.last(), Some(&json!(false)), "{from}");
        }
        Database::PostgreSql(client) => drop(
            client
                .simple(&format!(
                    "ALTER TABLE meta ALTER COLUMN writable_from TYPE jsonb \
                     USING to_jsonb(writable_from);
                     UPDATE meta SET writable_from = '{}'::jsonb;",
                    from.to_string().replace('\'', "''")
                ))
                .unwrap(),
        ),
    }
}

#[test]
fn every_migration_writes_the_oldest_symphony_that_may_write_beside_the_version() {
    let this = Schema::this_build();
    let v = this.version();
    // A board made by this build, whose step declares nothing.
    let db = database();
    moved(&db, this.clone());
    assert_eq!(record(&db), (Some(v), json!(this.writable_from())));
    assert_eq!(
        this.writable_from(),
        v,
        "this build's own step is not declared"
    );
    // One step on, declared: a Symphony of this schema may write it.
    let db = board();
    moved(&db, listed());
    assert_eq!(record(&db), (Some(v + 1), json!(v)));
    // One step on, undeclared: none older may.
    let db = board();
    moved(&db, unlisted());
    assert_eq!(record(&db), (Some(v + 1), json!(v + 1)));
    // Two steps at once: the record is the last step's, declared or not.
    for (keeps, from) in [(true, v + 1), (false, v + 2)] {
        let db = board();
        let two_on = then_table(&unlisted(), "fixture_later");
        let two_on = if keeps {
            two_on.keeping_writes()
        } else {
            two_on
        };
        moved(&db, two_on);
        assert_eq!(record(&db), (Some(v + 2), json!(from)), "{keeps}");
    }
    // A board made in one go by a newer build whose step is declared.
    let db = database();
    moved(&db, listed());
    assert_eq!(record(&db), (Some(v + 1), json!(v)));
    // A board a build before the record moved, which has none, gains it with the next move.
    let db = board();
    Raw::of(&db)
        .row("meta", "schema")
        .set(json!({"writable_from": null}))
        .unwrap();
    assert_eq!(record(&db), (Some(v), Value::Null));
    moved(&db, listed());
    assert_eq!(record(&db), (Some(v + 1), json!(v)));
}

#[test]
fn a_reader_one_behind_writes_a_board_whose_record_lets_it_and_every_other_store_is_refused() {
    let db = board();
    let v = Schema::this_build().version();
    // Opened before the move: the Symphony's store, a writer, and stores taken unchecked.
    let before = reader(&db, Schema::this_build());
    let writer = opened(db.another(), &Opening::write().of(Some("alpha"))).unwrap();
    let beat = unchecked(&db, &Opening::write().of(Some("alpha")));
    let unchecked_reader = unchecked(&db, &Opening::read().of(Some("alpha")));
    let by_opening = opened(db.another(), &Opening::read().of(Some("alpha"))).unwrap();
    moved(&db, listed());
    let after = reader(&db, Schema::this_build());
    for (store, text) in [
        (&before, "from the reader opened before the move"),
        (&after, "from the reader opened after it"),
        (
            &by_opening,
            "from a reader the writers' entry opened to read",
        ),
    ] {
        assert_eq!(
            store.board_standing().unwrap(),
            Standing {
                version: Some(v + 1),
                writes: true
            }
        );
        said(store, text).unwrap();
    }
    after
        .create(&NewTask::new("new/one", "Filed one schema ahead", "owner"))
        .unwrap();
    assert_eq!(
        after.get("new/one").unwrap().title,
        "Filed one schema ahead"
    );
    // Every other store is refused one ahead whatever the record says, before it writes.
    for store in [&writer, &beat, &unchecked_reader] {
        assert_eq!(
            store.board_standing().unwrap(),
            Standing {
                version: Some(v + 1),
                writes: false
            }
        );
        let why = refused_at(store, v + 1);
        assert!(
            why.contains("writes only a board at its own schema"),
            "{why}"
        );
    }
    let why = refused(opened(db.another(), &Opening::write().of(Some("alpha"))).map(drop));
    assert!(
        why.starts_with(&format!("the alpha board is at schema {}", v + 1)),
        "{why}"
    );
    // A whole-board copy is held to the reader's own schema, and stages nothing.
    let why = refused(after.copy(&Dump::default()));
    assert!(
        why.starts_with(&format!("the alpha board is at schema {}", v + 1)),
        "{why}"
    );
    let tasks: Vec<String> = after
        .board()
        .unwrap()
        .tasks
        .into_iter()
        .map(|task| task.slug)
        .collect();
    assert_eq!(tasks, ["old/one", "new/one"], "the copy replaced nothing");
    assert_eq!(
        thread(&db)[1..],
        [
            "from the reader opened before the move",
            "from the reader opened after it",
            "from a reader the writers' entry opened to read",
        ]
    );
    // The newer build reads and writes what the reader wrote.
    let newer = moved(&db, listed());
    said(&newer, "from the newer build").unwrap();
    assert_eq!(newer.thread("old/one").unwrap().len(), 5);
}

#[test]
fn a_reader_is_refused_an_undeclared_step_and_a_record_missing_or_of_another_shape() {
    let v = Schema::this_build().version();
    // An undeclared step: read, and nothing written.
    let db = board();
    let before = reader(&db, Schema::this_build());
    moved(&db, unlisted());
    let after = reader(&db, Schema::this_build());
    for store in [&before, &after] {
        assert_eq!(
            store.board_standing().unwrap(),
            Standing {
                version: Some(v + 1),
                writes: false
            }
        );
        let why = refused_at(store, v + 1);
        assert!(why.contains("`just symphony` in alpha"), "{why}");
        assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
    }
    assert_eq!(thread(&db).len(), 1, "nothing was written");
    // A declared step whose record is gone, or holds what no build writes.
    for from in [
        Value::Null,
        json!(v.to_string()),
        json!(v as f64),
        json!(v as f64 + 0.5),
        json!(true),
        json!([v]),
        json!({"writable_from": v}),
    ] {
        let db = board();
        let reading = reader(&db, Schema::this_build());
        moved(&db, listed());
        said(&reading, "while the record lets it").unwrap();
        if from.is_null() {
            Raw::of(&db)
                .row("meta", "schema")
                .set(json!({"writable_from": null}))
                .unwrap();
        } else {
            malformed(&db, &from);
        }
        if from.is_null() || matches!(db, Database::PostgreSql(_)) {
            assert_eq!(record(&db).1, from);
        }
        assert!(!reading.board_standing().unwrap().writes, "{from}");
        refused_at(&reading, v + 1);
        assert_eq!(thread(&db).len(), 2, "{from}");
    }
}

#[test]
fn a_reader_is_refused_two_ahead_behind_and_a_record_a_later_move_replaced() {
    let v = Schema::this_build().version();
    // Declared, then undeclared: the first move's record does not outlive the second.
    let db = board();
    let at_v = reader(&db, Schema::this_build());
    moved(&db, listed());
    let at_next = reader(&db, listed());
    said(&at_v, "one ahead, declared").unwrap();
    said(&at_next, "at its own schema").unwrap();
    moved(&db, then_table(&listed(), "fixture_later"));
    assert_eq!(record(&db), (Some(v + 2), json!(v + 2)));
    refused_at(&at_next, v + 2);
    refused_at(&at_v, v + 2);
    // Declared twice: the one behind writes, the one two behind does not.
    let db = board();
    let at_v = reader(&db, Schema::this_build());
    moved(&db, listed());
    let at_next = reader(&db, listed());
    moved(&db, then_table(&listed(), "fixture_later").keeping_writes());
    assert_eq!(record(&db), (Some(v + 2), json!(v + 1)));
    said(&at_next, "one ahead again").unwrap();
    assert_eq!(
        at_v.board_standing().unwrap(),
        Standing {
            version: Some(v + 2),
            writes: false
        }
    );
    refused_at(&at_v, v + 2);
    // Two steps at once, declared: the build two behind is refused, the one behind writes.
    let db = board();
    let at_v = reader(&db, Schema::this_build());
    moved(
        &db,
        then_table(&unlisted(), "fixture_later").keeping_writes(),
    );
    refused_at(&at_v, v + 2);
    said(&reader(&db, listed()), "the one behind").unwrap();
    // A board behind the reader's schema, as a restore would leave it, is refused it.
    let db = board();
    moved(&db, listed());
    let ahead_of_board = reader(&db, listed());
    Raw::of(&db)
        .row("meta", "schema")
        .set(json!({"version": v}))
        .unwrap();
    assert_eq!(
        ahead_of_board.board_standing().unwrap(),
        Standing {
            version: Some(v),
            writes: false
        }
    );
    refused_at(&ahead_of_board, v);
}

#[test]
fn a_bound_write_is_fenced_on_the_generation_one_ahead_and_a_rebind_is_refused_there() {
    let db = board();
    let authority = Arc::new(Memory::default());
    let writer = opened(db.another(), &Opening::write()).unwrap();
    writer.recovering(authority.clone()).rebind().unwrap();
    let reading = reader(&db, Schema::this_build()).recovering(authority.clone());
    let generation = reading.recovery_generation().unwrap();
    moved(&db, listed());
    let at = reading.bound("old/one").unwrap().bound;
    reading
        .amend_bound("old/one", &at, "Retitled one ahead", &[], "owner")
        .unwrap();
    assert_eq!(reading.get("old/one").unwrap().title, "Retitled one ahead");
    // A token of another generation is refused one ahead as at its own schema.
    let at = reading.bound("old/one").unwrap().bound;
    authority.publish("another generation").unwrap();
    let why = refused(reading.amend_bound("old/one", &at, "Past the fence", &[], "owner"));
    assert!(why.contains(crate::recovery::RESTORED), "{why}");
    assert_eq!(reading.get("old/one").unwrap().title, "Retitled one ahead");
    // A rebind, the recovery's own write, is held to the reader's schema, and moves nothing.
    let why = refused(reading.rebind());
    assert!(why.starts_with(&format!(
        "the alpha board is at schema {}",
        listed().version()
    )));
    assert_eq!(reading.recovery_generation().unwrap(), generation);
}

/// A write that began its transaction before a migration, on PostgreSQL, meets the record the
/// migration wrote, never the one its snapshot held: undeclared, it is refused; declared, it
/// writes after the move, under the record that lets it.
#[test]
fn a_postgresql_readers_write_with_a_snapshot_before_a_migration_meets_the_record_it_wrote() {
    for (schema, writes) in [(unlisted(), false), (listed(), true)] {
        let db = board(Backend::PostgreSql);
        let Database::PostgreSql(client) = db.another() else {
            unreachable!()
        };
        client
            .simple("BEGIN ISOLATION LEVEL REPEATABLE READ; SELECT version FROM meta;")
            .unwrap();
        let reading = Store::reading_on(
            client.into(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            &Opening::read(),
        )
        .unwrap();
        moved(&db, schema);
        let wrote = said(&reading, "after the move");
        assert_eq!(wrote.is_ok(), writes, "{wrote:?}");
        if !writes {
            let why = refused(wrote);
            assert!(why.starts_with("this board is at schema"), "{why}");
        }
        assert_eq!(thread(&db).len(), 1 + usize::from(writes));
    }
}
