//! The schema rule for writers of different versions on one board: a board at a store's own
//! version opens with one read and no write; an older board, or none, is migrated forward once by a
//! writer, its steps and version in one transaction, and refused to a reader after that one read,
//! nothing defined or written; a newer one is refused to a writer before any write and read by a
//! reader one version behind, whose writes are refused; and a store that opened before a newer
//! build migrated is refused at its next write. On either database: a newer build's steps are
//! written in each language (`Schema::then_each`).

use crate::common::{Backend, HOST, NOW, conflicts_deadline, refused, retry_conflicts};
use serde_json::json;
use symphony_board::pg::Step;
use symphony_board::rows::Dump;
use symphony_board::rpc::Client;
use symphony_board::{Database, Error, NewTask, Opening, Program, Raw, Schema, Store};

/// The store `how` opens on `db`, its clock at `NOW` and no process alive.
fn opened(db: Database, how: &Opening) -> Result<Store, Error> {
    Store::opening_on(db, Box::new(|| NOW), Box::new(|_| false), HOST, how)
}

/// The store a program that only reads boards opens on `db` by `how` ([`Store::reading_on`]).
fn read(db: Database, how: &Opening) -> Result<Store, Error> {
    Store::reading_on(db, Box::new(|| NOW), Box::new(|_| false), HOST, how)
}

/// Another client of `db`'s database, which has made no request yet.
fn again(db: &Database) -> Database {
    db.another()
}

#[test]
fn a_postgresql_write_with_a_snapshot_before_a_migration_cannot_commit_after_it() {
    let db = board(Backend::PostgreSql);
    let Database::PostgreSql(writer) = again(&db) else {
        unreachable!()
    };
    writer
        .simple("BEGIN ISOLATION LEVEL REPEATABLE READ; SELECT version FROM meta;")
        .unwrap();
    opened(again(&db), &Opening::write().at(newer())).unwrap();
    let writer = Store::unchecked_on(
        writer.into(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write(),
    );
    let why = refused(writer.create(&NewTask::new("late/snapshot", "An old snapshot", "owner")));
    assert!(why.starts_with("this board is at schema"), "{why}");
    assert!(!Raw::of(&db).row("task", "late/snapshot").exists().unwrap());
}

/// The version `meta:schema` records.
fn version(db: &Database) -> Option<i64> {
    let found = Raw::of(db).row("meta", "schema").field("version");
    found.unwrap().as_i64()
}

/// The tables the database holds.
fn tables(db: &Database) -> Vec<String> {
    let found = match db {
        Database::SurrealDb(client) => (client)
            .query("RETURN object::keys((INFO FOR DB).tables);", json!({}))
            .unwrap()
            .last()
            .cloned()
            .unwrap(),
        Database::PostgreSql(client) => {
            let listed = client
                .request(&[Step::bare(
                    "SELECT coalesce(jsonb_agg(table_name::text ORDER BY table_name), '[]')::text \
                     FROM information_schema.tables WHERE table_schema = 'public'",
                )])
                .unwrap();
            serde_json::from_str(listed[0].value().unwrap()).unwrap()
        }
    };
    serde_json::from_value(found).unwrap()
}

/// A table of the test's own, named `name`, as a schema step in each database's language.
fn table_step(name: &str) -> (String, String) {
    (
        format!("DEFINE TABLE IF NOT EXISTS {name} SCHEMALESS;"),
        format!("CREATE TABLE IF NOT EXISTS {name} (id jsonb PRIMARY KEY, doc jsonb);"),
    )
}

/// A newer build's schema: this one's, then a table of its own.
fn newer() -> Schema {
    let (surreal, postgres) = table_step("fixture_next");
    Schema::this_build().then_each(&surreal, &postgres)
}

/// A board made at this build's version on `backend`, holding the task `old/one`: its database.
fn board(backend: Backend) -> Database {
    let db = backend.database();
    let store = opened(again(&db), &Opening::write()).unwrap();
    store
        .create(&NewTask::new("old/one", "Made at this version", "owner"))
        .unwrap();
    db
}

on_both! {
    fn a_board_at_the_stores_version_opens_with_one_read_and_nothing_defined_or_written(
        backend: Backend
    ) {
        let db = board(backend);
        for how in [Opening::write(), Opening::read()] {
            let fresh = again(&db);
            let store = opened(fresh, &how).unwrap();
            assert_eq!(store.database().requests(), 1, "{how:?}");
            assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
        }
        assert_eq!(version(&db), Some(Schema::this_build().version()));
    }

    fn a_database_not_made_yet_is_defined_and_brought_to_the_version_once(backend: Backend) {
        let db = backend.database();
        let store = opened(again(&db), &Opening::write()).unwrap();
        assert_eq!(version(&db), Some(Schema::this_build().version()));
        assert!(tables(&db).contains(&"task".to_string()));
        store
            .create(&NewTask::new("new/one", "On a board made now", "owner"))
            .unwrap();
        // On SurrealDB, a namespace no one made is defined with its database.
        if let Database::SurrealDb(fresh) = Backend::SurrealDb.database() {
            let elsewhere = || Database::from(Client::new(fresh.addr(), fresh.database(), "tasks"));
            opened(elsewhere(), &Opening::write()).unwrap();
            assert_eq!(version(&elsewhere()), Some(Schema::this_build().version()));
        }
    }
}

/// Why a request in `db`'s database fails, which the test expects it to.
fn fails(db: &Database) -> String {
    let failed = match db {
        Database::SurrealDb(client) => (client)
            .query(
                "RETURN (SELECT VALUE version FROM ONLY meta:schema);",
                json!({}),
            )
            .err(),
        Database::PostgreSql(client) => client.request(&[Step::bare("SELECT 1")]).err(),
    };
    failed
        .map(|why| format!("{why:?}"))
        .expect("the request fails")
}

/// `db`'s database made on its server, and nothing in it; on SurrealDB again when its definition
/// meets another test's at once in the shared namespace (`Conflict`), until the fixture's backstop.
fn made_empty(db: &Database) {
    match db {
        Database::SurrealDb(client) => {
            let defined = retry_conflicts(conflicts_deadline(), || {
                client.define_then("RETURN NONE;", json!({}))
            });
            drop(defined.unwrap())
        }
        Database::PostgreSql(client) => client.make_database().unwrap(),
    }
}

on_both! {
    fn a_reader_of_a_database_not_made_yet_is_refused_and_defines_nothing(backend: Backend) {
        let read = backend.database();
        let why = refused(opened(again(&read), &Opening::read().of(Some("alpha"))).map(drop));
        assert_eq!(
            why,
            "the alpha board is not made yet, and a reader never makes a board: nothing was read or \
             written; `just tasks list` in alpha makes it"
        );
        // Its database is not on the server still: the reader defined nothing.
        let unmade = fails(&read);
        assert!(unmade.contains("does not exist"), "{unmade}");
        // A database made, but no board in it, is no board to a reader either.
        made_empty(&read);
        let why = refused(opened(again(&read), &Opening::read()).map(drop));
        assert!(why.starts_with("this board is not made yet"), "{why}");
        assert_eq!(tables(&read), Vec::<String>::new());
        // The board's command line, a writer, makes it; then the reader reads it.
        opened(again(&read), &Opening::write()).unwrap();
        let reader = opened(again(&read), &Opening::read()).unwrap();
        assert_eq!(reader.board().unwrap().tasks.len(), 0);
        assert_eq!(version(&read), Some(Schema::this_build().version()));
    }

    fn the_readers_entry_reads_by_the_rule_and_never_makes_or_moves_a_board_whatever_it_is_handed(
        backend: Backend
    ) {
        // Handed a writer's opening, it reads still: a board not made yet is refused, not made.
        let db = backend.database();
        for how in [Opening::read(), Opening::write(), Opening::write().migrating()] {
            let why = refused(read(again(&db), &how.clone().of(Some("alpha"))).map(drop));
            assert_eq!(
                why,
                "the alpha board is not made yet, and a reader never makes a board: nothing was \
                 read or written; `just tasks list` in alpha makes it",
                "{how:?}"
            );
        }
        let unmade = fails(&db);
        assert!(unmade.contains("does not exist"), "{unmade}");
        // An older board it refuses as a reader does, and leaves as the older build left it.
        let before = Schema::this_build().before();
        let older = opened(again(&db), &Opening::write().at(before.clone())).unwrap();
        older
            .create(&NewTask::new("old/one", "Made by the older build", "owner"))
            .unwrap();
        let held = tables(&db);
        for how in [Opening::read(), Opening::write()] {
            let why = refused(read(again(&db), &how).map(drop));
            assert!(
                why.starts_with(&format!(
                    "this board is at schema {}, behind this build's {}, and a reader never \
                     moves a board forward",
                    before.version(),
                    Schema::this_build().version()
                )),
                "{why}"
            );
        }
        assert_eq!(version(&db), Some(before.version()));
        assert_eq!(tables(&db), held);
        // A board at its version opens with the one read, as by `opening_on`, and one a version
        // ahead is read.
        opened(again(&db), &Opening::write()).unwrap();
        for how in [Opening::read(), Opening::write()] {
            let store = read(again(&db), &how).unwrap();
            assert_eq!(store.database().requests(), 1, "{how:?}");
            assert_eq!(store.get("old/one").unwrap().title, "Made by the older build");
        }
        opened(again(&db), &Opening::write().at(newer())).unwrap();
        let ahead = read(again(&db), &Opening::write()).unwrap();
        assert_eq!(ahead.board().unwrap().tasks.len(), 1);
        assert_eq!(version(&db), Some(newer().version()));
    }

    fn a_reader_of_a_newer_build_never_moves_an_older_board_forward_but_refuses_it(
        backend: Backend
    ) {
        let client = backend.database();
        let before = Schema::this_build().before();
        let older = opened(again(&client), &Opening::write().at(before.clone())).unwrap();
        older
            .create(&NewTask::new("old/one", "Made by the older build", "owner"))
            .unwrap();
        let held = tables(&client);
        let this = Schema::this_build().version();
        let why = refused(opened(again(&client), &Opening::read().of(Some("alpha"))).map(drop));
        assert_eq!(
            why,
            format!(
                "the alpha board is at schema {}, behind this build's {this}, and a reader never \
                 moves a board forward: nothing was read or written; `just tasks migrate` in the \
                 main checkout of alpha moves it once the project's board command is at schema \
                 {this}, and a newer schema is tried on a scratch board (`TASKS_DATABASE=NAME`)",
                before.version()
            )
        );
        // A reader whose schema has a table of its own does not define it either.
        let why = refused(opened(again(&client), &Opening::read().at(newer())).map(drop));
        assert!(
            why.starts_with(&format!(
                "this board is at schema {}, behind this build's {}",
                before.version(),
                newer().version()
            )),
            "{why}"
        );
        // The board is the older build's as it left it, to read and to write.
        assert_eq!(version(&client), Some(before.version()));
        assert_eq!(tables(&client), held);
        assert!(!held.contains(&"fixture_next".to_string()), "{held:?}");
        older
            .say("old/one", "owner", "after the reader", None)
            .unwrap();
        assert_eq!(older.thread("old/one").unwrap().len(), 2);
        // A writer of this build moves it forward, and the reader reads it then.
        opened(again(&client), &Opening::write()).unwrap();
        let reader = opened(again(&client), &Opening::read()).unwrap();
        assert_eq!(
            reader.get("old/one").unwrap().title,
            "Made by the older build"
        );
        assert_eq!(version(&client), Some(this));
    }

    fn a_newer_build_migrates_an_older_board_once_its_steps_and_version_together(backend: Backend) {
        let client = board(backend);
        let runs = Schema::this_build().then_each(
            "DEFINE TABLE IF NOT EXISTS fixture_run SCHEMALESS; CREATE fixture_run RETURN NONE;",
            "CREATE TABLE IF NOT EXISTS fixture_run (id jsonb PRIMARY KEY, doc jsonb); \
             INSERT INTO fixture_run (id, doc) VALUES (to_jsonb(gen_random_uuid()::text), '{}');",
        );
        let opened_at_once: Vec<Result<Store, Error>> = std::thread::scope(|scope| {
            let opens: Vec<_> = (0..6)
                .map(|_| {
                    let (fresh, runs) = (again(&client), &runs);
                    scope.spawn(move || opened(fresh, &Opening::write().at(runs.clone())))
                })
                .collect();
            opens.into_iter().map(|open| open.join().unwrap()).collect()
        });
        for store in &opened_at_once {
            assert!(store.is_ok(), "{:?}", store.as_ref().err());
        }
        assert_eq!(version(&client), Some(runs.version()));
        let ran = Raw::of(&client).table("fixture_run").count().unwrap();
        assert_eq!(ran, 1, "the steps ran once");
        // What the board held before is still there, read by the newer build.
        let store = opened_at_once.into_iter().next().unwrap().unwrap();
        assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
    }

    fn a_migration_cut_short_before_its_commit_leaves_the_board_as_it_was_for_the_next_to_migrate(
        backend: Backend
    ) {
        let client = board(backend);
        let cut = Schema::this_build().then_each(
            "DEFINE TABLE IF NOT EXISTS fixture_half SCHEMALESS; THROW 'cut short';",
            "CREATE TABLE IF NOT EXISTS fixture_half (id jsonb PRIMARY KEY); \
             DO $$ BEGIN RAISE EXCEPTION 'cut short'; END $$;",
        );
        let why = opened(again(&client), &Opening::write().at(cut))
            .err()
            .unwrap();
        assert_eq!(why, Error::Db("cut short".into()));
        assert_eq!(version(&client), Some(Schema::this_build().version()));
        assert!(!tables(&client).contains(&"fixture_half".to_string()));
        opened(again(&client), &Opening::write().at(newer())).unwrap();
        assert_eq!(version(&client), Some(newer().version()));
        assert!(tables(&client).contains(&"fixture_next".to_string()));
    }

    fn an_older_writer_is_refused_a_migrated_board_before_any_write_naming_the_project(
        backend: Backend
    ) {
        let client = board(backend);
        opened(again(&client), &Opening::write().at(newer())).unwrap();
        let older = again(&client);
        let why = refused(opened(older, &Opening::write().of(Some("alpha"))).map(drop));
        let this = Schema::this_build().version();
        let program = Program::this();
        assert_eq!(
            why,
            format!(
                "the alpha board is at schema {}, and this build writes only a board at its own \
                 schema, {this}: nothing was written; rebuild {} ({}) at the board's schema, and \
                 `just symphony` in alpha brings its Symphony to it",
                this + 1,
                program.name,
                program.path
            )
        );
        // The refusal came from the one read: the board is the newer build's as it left it.
        assert_eq!(version(&client), Some(newer().version()));
        let unnamed = match again(&client) {
            Database::SurrealDb(older) => refused(Store::open(older).map(drop)),
            older => refused(opened(older, &Opening::write()).map(drop)),
        };
        assert!(unnamed.starts_with("this board is at schema"), "{unnamed}");
        assert!(
            unnamed.contains("`just symphony` in its project"),
            "{unnamed}"
        );
    }

    fn a_reader_one_version_behind_reads_a_newer_board_and_is_refused_every_write(backend: Backend) {
        let client = board(backend);
        let newer_store = opened(again(&client), &Opening::write().at(newer())).unwrap();
        newer_store
            .create(&NewTask::new("new/two", "Made by the newer build", "owner"))
            .unwrap();
        let reader = opened(again(&client), &Opening::read().of(Some("alpha"))).unwrap();
        assert_eq!(
            reader.get("new/two").unwrap().title,
            "Made by the newer build"
        );
        let slugs: Vec<String> = reader
            .board()
            .unwrap()
            .tasks
            .into_iter()
            .map(|task| task.slug)
            .collect();
        assert_eq!(slugs, ["old/one", "new/two"]);
        assert_eq!(reader.thread("old/one").unwrap().len(), 1);
        let this = Schema::this_build().version();
        let why = refused(reader.create(&NewTask::new("old/three", "From the reader", "owner")));
        assert!(
            why.starts_with(&format!("the alpha board is at schema {}, ", this + 1)),
            "{why}"
        );
        assert!(why.contains("`just symphony` in alpha"), "{why}");
        let said = refused(reader.say("old/one", "owner", "from the reader", None));
        assert_eq!(said, why);
        assert!(newer_store.get("old/three").is_err());
        assert_eq!(newer_store.thread("old/one").unwrap().len(), 1);
    }

    fn a_reader_more_than_one_version_behind_is_refused_before_it_reads(backend: Backend) {
        let client = board(backend);
        let (surreal, postgres) = table_step("fixture_later");
        let two_on = newer().then_each(&surreal, &postgres);
        opened(again(&client), &Opening::write().at(two_on)).unwrap();
        let why = refused(opened(again(&client), &Opening::read().of(Some("alpha"))).map(drop));
        let this = Schema::this_build().version();
        assert_eq!(
            why,
            format!(
                "the alpha board is at schema {}, more than one ahead of this build's {this}: nothing \
                 was read; `just symphony` in alpha brings its Symphony to the board's schema",
                this + 2
            )
        );
    }

    fn a_store_opened_before_a_newer_build_migrated_is_refused_at_its_next_write(backend: Backend) {
        let client = board(backend);
        let writer = opened(again(&client), &Opening::write().of(Some("alpha"))).unwrap();
        let beat = Store::unchecked_on(
            again(&client),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            &Opening::write().of(Some("alpha")),
        );
        opened(again(&client), &Opening::write().at(newer())).unwrap();
        for store in [&writer, &beat] {
            let why = refused(store.create(&NewTask::new("late/one", "After the migration", "owner")));
            assert!(why.starts_with("the alpha board is at schema"), "{why}");
        }
        assert!(writer.get("late/one").is_err());
        // A copy stages nothing on a board it would be refused to swap in.
        refused(writer.copy(&Dump::default()));
        assert!(
            tables(&client)
                .iter()
                .all(|table| !table.starts_with("copy_")),
            "{:?}",
            tables(&client)
        );
        assert_eq!(writer.get("old/one").unwrap().title, "Made at this version");
    }

    fn an_older_build_brings_a_board_to_its_own_version_and_this_one_migrates_it_on(
        backend: Backend
    ) {
        let client = backend.database();
        let before = Schema::this_build().before();
        opened(again(&client), &Opening::write().at(before.clone())).unwrap();
        assert_eq!(version(&client), Some(before.version()));
        let store = opened(again(&client), &Opening::write()).unwrap();
        assert_eq!(version(&client), Some(Schema::this_build().version()));
        store
            .create(&NewTask::new("on/now", "Written at this version", "owner"))
            .unwrap();
    }
}

on_both! {
/// Schema 19 could hold implicit reservations on SurrealDB; its PostgreSQL schema lacked the
/// table. The normal version migration must both add missing storage and preserve earlier rows.
fn schema_19_gains_reservation_storage_and_keeps_existing_reservations(backend: Backend) {
    for existed in [false, true] {
        let db = board(backend);
        let raw = Raw::of(&db);
        let key = json!(["old/one", "worker", 7]);
        if existed {
            raw.row("unanswered_restore", key.clone()).create(json!({
                "slug": "old/one", "handle": "worker", "post": 7
            })).unwrap();
        } else {
            // DDL is engine-specific: emulate the predecessor's catalogue, not a row mutation.
            match &db {
                Database::SurrealDb(client) => { client.query("REMOVE TABLE unanswered_restore;", json!({})).unwrap(); }
                Database::PostgreSql(client) => { client.simple("DROP TABLE unanswered_restore").unwrap(); }
            }
        }
        raw.row("meta", "schema").set(json!({"version": 19})).unwrap();
        let upgraded = opened(again(&db), &Opening::write()).unwrap();
        assert_eq!(version(&db), Some(Schema::this_build().version()));
        assert!(tables(&db).contains(&"unanswered_restore".into()));
        assert_eq!(upgraded.get("old/one").unwrap().title, "Made at this version");
        assert_eq!(upgraded.raw().row("unanswered_restore", key.clone()).exists().unwrap(), existed);
        let reopened = opened(again(&db), &Opening::write()).unwrap();
        assert_eq!(reopened.database().requests(), 1);
        assert_eq!(reopened.raw().row("unanswered_restore", key).exists().unwrap(), existed);
    }
}
}

on_both! {
/// Schema 25's workflow table took six kinds; this build's step adds `authority` and `admission`
/// to the board's check, keeps what the board held, and on PostgreSQL alters the check no more
/// once it holds every kind, as each alter takes the table's exclusive lock.
fn schema_26_gains_the_guards_kinds_and_alters_the_check_once(backend: Backend) {
    let db = board(backend);
    let six = "'active', 'definition', 'activation', 'event', 'receipt', 'intent'";
    let authority = |db: &Database| -> Result<(), symphony_board::db::DbError> {
        match db {
            Database::SurrealDb(client) => client
                .query(
                    "CREATE workflow:['authority', 'approvals'] CONTENT { kind: 'authority', \
                     body: { epoch: 'e1', generation: 1, state: 's1' }, at: 1.0 } RETURN NONE;",
                    json!({}),
                )
                .map(drop),
            Database::PostgreSql(client) => client
                .simple(
                    "INSERT INTO workflow (id, kind, body, at) VALUES \
                     ('[\"authority\", \"approvals\"]', 'authority', \
                     '{\"epoch\": \"e1\", \"generation\": 1, \"state\": \"s1\"}', 1.0)",
                )
                .map(drop),
        }
    };
    // The predecessor's catalogue: its check of six kinds.
    match &db {
        Database::SurrealDb(client) => {
            client
                .query(
                    &format!("DEFINE FIELD OVERWRITE kind ON workflow TYPE string ASSERT $value IN [{six}];"),
                    json!({}),
                )
                .unwrap();
        }
        Database::PostgreSql(client) => {
            client
                .simple(&format!(
                    "ALTER TABLE workflow DROP CONSTRAINT workflow_kind_check, \
                     ADD CONSTRAINT workflow_kind_check CHECK (kind IN ({six}))"
                ))
                .unwrap();
        }
    }
    assert!(authority(&db).is_err(), "the predecessor's check refuses the kind");
    let this = Schema::this_build().version();
    let behind = || {
        Raw::of(&db)
            .row("meta", "schema")
            .set(json!({"version": this - 1}))
            .unwrap();
    };
    behind();
    let upgraded = opened(again(&db), &Opening::write()).unwrap();
    assert_eq!(version(&db), Some(this));
    authority(&db).unwrap();
    assert_eq!(upgraded.get("old/one").unwrap().title, "Made at this version");
    if let Database::PostgreSql(client) = &db {
        let check = || {
            client
                .simple(
                    "SELECT oid::text FROM pg_constraint \
                     WHERE conrelid = 'workflow'::regclass AND conname = 'workflow_kind_check'",
                )
                .unwrap()[0]
                .value()
                .unwrap()
                .to_string()
        };
        let first = check();
        behind();
        opened(again(&db), &Opening::write()).unwrap();
        assert_eq!(version(&db), Some(this));
        assert_eq!(check(), first, "a check holding every kind is not made again");
    }
}
}
