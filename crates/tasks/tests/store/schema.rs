//! The schema rule for writers of different versions on one board: a board at a store's own
//! version opens with one read and no write; an older board, or none, is migrated forward once by a
//! writer, its steps and version in one transaction, and refused to a reader after that one read,
//! nothing defined or written; a newer one is refused to a writer before any write and read by a
//! reader one version behind, whose writes are refused; and a store that opened before a newer
//! build migrated is refused at its next write.

use crate::common::{HOST, NOW, client, refused};
use serde_json::{Value, json};
use symphony_board::rows::Dump;
use symphony_board::rpc::Client;
use symphony_board::{Error, NewTask, Opening, Schema, Store};

/// The store `how` opens on `client`, its clock at `NOW` and no process alive.
fn opened(client: Client, how: &Opening) -> Result<Store, Error> {
    Store::opening(client, Box::new(|| NOW), Box::new(|_| false), HOST, how)
}

/// Another client of `client`'s database, which has made no request yet.
fn again(client: &Client) -> Client {
    Client::new(client.addr(), "sf", client.database())
}

/// The version `meta:schema` records.
fn version(client: &Client) -> Option<i64> {
    let found = client
        .query(
            "RETURN (SELECT VALUE version FROM ONLY meta:schema);",
            json!({}),
        )
        .unwrap();
    found.last().and_then(Value::as_i64)
}

/// The tables the database holds.
fn tables(client: &Client) -> Vec<String> {
    let found = client
        .query("RETURN object::keys((INFO FOR DB).tables);", json!({}))
        .unwrap();
    serde_json::from_value(found.last().cloned().unwrap()).unwrap()
}

/// A newer build's schema: this one's, then a table of its own.
fn newer() -> Schema {
    Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;")
}

/// A board made at this build's version, holding the task `old/one`: its client.
fn board() -> Client {
    let client = client();
    let store = opened(again(&client), &Opening::write()).unwrap();
    store
        .create(&NewTask::new("old/one", "Made at this version", "owner"))
        .unwrap();
    client
}

#[test]
fn a_board_at_the_stores_version_opens_with_one_read_and_nothing_defined_or_written() {
    let client = board();
    for how in [Opening::write(), Opening::read()] {
        let fresh = again(&client);
        let store = opened(fresh, &how).unwrap();
        assert_eq!(store.client().requests(), 1, "{how:?}");
        assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
    }
    assert_eq!(version(&client), Some(Schema::this_build().version()));
}

#[test]
fn a_database_not_made_yet_is_defined_and_brought_to_the_version_once() {
    let client = client();
    let store = opened(again(&client), &Opening::write()).unwrap();
    assert_eq!(version(&client), Some(Schema::this_build().version()));
    assert!(tables(&client).contains(&"task".to_string()));
    store
        .create(&NewTask::new("new/one", "On a board made now", "owner"))
        .unwrap();
    // A namespace no one made is defined with its database.
    let fresh = crate::common::client();
    let elsewhere = || Client::new(fresh.addr(), fresh.database(), "tasks");
    opened(elsewhere(), &Opening::write()).unwrap();
    assert_eq!(version(&elsewhere()), Some(Schema::this_build().version()));
}

/// Why a request in `client`'s database fails, which the test expects it to.
fn fails(client: &Client) -> String {
    client
        .query("RETURN (SELECT VALUE version FROM ONLY meta:schema);", json!({}))
        .err()
        .map(|why| format!("{why:?}"))
        .expect("the request fails")
}

#[test]
fn a_reader_of_a_database_not_made_yet_is_refused_and_defines_nothing() {
    let read = client();
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
    read.define_then("RETURN NONE;", json!({})).unwrap();
    let why = refused(opened(again(&read), &Opening::read()).map(drop));
    assert!(why.starts_with("this board is not made yet"), "{why}");
    assert_eq!(tables(&read), Vec::<String>::new());
    // The board's command line, a writer, makes it; then the reader reads it.
    opened(again(&read), &Opening::write()).unwrap();
    let reader = opened(again(&read), &Opening::read()).unwrap();
    assert_eq!(reader.board().unwrap().tasks.len(), 0);
    assert_eq!(version(&read), Some(Schema::this_build().version()));
}

#[test]
fn a_reader_of_a_newer_build_never_moves_an_older_board_forward_but_refuses_it() {
    let client = client();
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
             moves a board forward: nothing was read or written; `just tasks list` in alpha \
             moves it once the project's board command is at schema {this}, and a newer schema \
             is tried on a scratch board (`TASKS_DATABASE=NAME`)",
            before.version()
        )
    );
    // The board is the older build's as it left it, to read and to write.
    assert_eq!(version(&client), Some(before.version()));
    assert_eq!(tables(&client), held);
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

#[test]
fn a_newer_build_migrates_an_older_board_once_its_steps_and_version_together() {
    let client = board();
    let runs = Schema::this_build()
        .then("DEFINE TABLE IF NOT EXISTS fixture_run SCHEMALESS; CREATE fixture_run RETURN NONE;");
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
    let ran = client
        .query("RETURN count(SELECT * FROM fixture_run);", json!({}))
        .unwrap();
    assert_eq!(ran.last(), Some(&json!(1)), "the steps ran once");
    // What the board held before is still there, read by the newer build.
    let store = opened_at_once.into_iter().next().unwrap().unwrap();
    assert_eq!(store.get("old/one").unwrap().title, "Made at this version");
}

#[test]
fn a_migration_cut_short_before_its_commit_leaves_the_board_as_it_was_for_the_next_to_migrate() {
    let client = board();
    let cut = Schema::this_build()
        .then("DEFINE TABLE IF NOT EXISTS fixture_half SCHEMALESS; THROW 'cut short';");
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

#[test]
fn an_older_writer_is_refused_a_migrated_board_before_any_write_naming_the_project() {
    let client = board();
    opened(again(&client), &Opening::write().at(newer())).unwrap();
    let older = again(&client);
    let why = refused(opened(older, &Opening::write().of(Some("alpha"))).map(drop));
    let this = Schema::this_build().version();
    assert_eq!(
        why,
        format!(
            "the alpha board is at schema {}, and this build writes only a board at its own \
             schema, {this}: nothing was written; `just symphony` in alpha brings its Symphony \
             to the board's schema",
            this + 1
        )
    );
    // The refusal came from the one read: the board is the newer build's as it left it.
    assert_eq!(version(&client), Some(newer().version()));
    let unnamed = refused(Store::open(again(&client)).map(drop));
    assert!(unnamed.starts_with("this board is at schema"), "{unnamed}");
    assert!(
        unnamed.contains("`just symphony` in its project"),
        "{unnamed}"
    );
}

#[test]
fn a_reader_one_version_behind_reads_a_newer_board_and_is_refused_every_write() {
    let client = board();
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

#[test]
fn a_reader_more_than_one_version_behind_is_refused_before_it_reads() {
    let client = board();
    let two_on = newer().then("DEFINE TABLE IF NOT EXISTS fixture_later SCHEMALESS;");
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

#[test]
fn a_store_opened_before_a_newer_build_migrated_is_refused_at_its_next_write() {
    let client = board();
    let writer = opened(again(&client), &Opening::write().of(Some("alpha"))).unwrap();
    let beat = Store::unchecked_as(
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

#[test]
fn an_older_build_brings_a_board_to_its_own_version_and_this_one_migrates_it_on() {
    let client = client();
    let before = Schema::this_build().before();
    opened(again(&client), &Opening::write().at(before.clone())).unwrap();
    assert_eq!(version(&client), Some(before.version()));
    let store = opened(again(&client), &Opening::write()).unwrap();
    assert_eq!(version(&client), Some(Schema::this_build().version()));
    store
        .create(&NewTask::new("on/now", "Written at this version", "owner"))
        .unwrap();
}
