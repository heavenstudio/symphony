//! A whole-board copy through a reader's store is held to the store's own schema at its swap as
//! at its staging: staged at the reader's schema, its swap is refused on a board a migration moved
//! one schema on meanwhile, though that step keeps the reader writing, and the board, its rows and
//! its recovery generation are as they were. On SurrealDB, whose copy is staged across requests;
//! PostgreSQL's is one request, held as staging is. On PostgreSQL a copy held past its limit is
//! ended by the server and leaves the board as it was.

use super::*;
use crate::store_fixture::{Backend, Fixture, HOST, NOW, Seed};
use crate::{Database, NewTask, Opening, Schema, Standing};

/// A copy still running at its limit is ended by the server, in the server's own words, and does
/// not replace the board once nothing holds it: it is held at its first insert by a lock the test
/// holds until the copy has answered. The limit is the test's (2 s), and the store's client waits
/// on the server as long as any board's does, so a loaded machine's slow start of a backend, the
/// store's opening and the server's late word all wait rather than fail.
#[test]
fn a_postgresql_copy_held_past_its_limit_is_ended_by_the_server_and_leaves_the_board() {
    let seed = Seed::default()
        .session("amber-heron")
        .task("sim/jump", "ready")
        .task("sim/land", "review");
    let f = Fixture::new(Backend::PostgreSql, seed.clone());
    let Database::PostgreSql(holder) = f.other_database() else {
        panic!("this board is on PostgreSQL");
    };
    holder
        .simple(
            "CREATE FUNCTION fixture_held() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN PERFORM pg_advisory_xact_lock(7); RETURN NULL; END $$; \
             CREATE TRIGGER fixture_held BEFORE INSERT ON task \
             FOR EACH STATEMENT EXECUTE FUNCTION fixture_held(); \
             SELECT pg_advisory_lock(7);",
        )
        .unwrap();
    let generation = f.store.recovery_generation().unwrap();
    let other = seed
        .clone()
        .task("sim/orbit", "ready")
        .session("brisk-finch");
    let ended = f
        .store
        .copy_limited(&other.dump, |_| Duration::from_secs(2));
    holder.simple("SELECT pg_advisory_unlock(7)").unwrap();
    match ended {
        Err(Error::Db(why)) => assert_eq!(
            why,
            "the copy did not finish in 2 s and was ended, the board left as it was: it had \
             copied no table and was at the 3 of task"
        ),
        other => panic!("expected the server to end the held copy, got {other:?}"),
    }
    assert_eq!(f.store.dump().unwrap(), seed.dump);
    assert_eq!(f.store.recovery_generation().unwrap(), generation);
}

#[test]
fn a_readers_copy_staged_at_its_schema_is_refused_its_swap_on_a_board_a_declared_step_moved_on() {
    let db = Backend::SurrealDb.database();
    let opened = |how: &Opening| {
        Store::opening_on(
            db.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            how,
        )
        .unwrap()
    };
    let writer = opened(&Opening::write());
    writer
        .create(&NewTask::new("old/one", "Made at this version", "owner"))
        .unwrap();
    let reader = Store::reading_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::read(),
    )
    .unwrap();
    let generation = || (writer.raw().row("recovery", "board").field("generation")).unwrap();
    let before = generation();
    // The copy, staged through the reader at its own schema: the board's one task, retitled, keyed
    // as the copy keys a task.
    let mut task = values(&writer.dump().unwrap().tasks).remove(0);
    task["sid"] = task["id"].clone();
    task["id"] = task["slug"].clone();
    task["title"] = json!("From the copy");
    let tables = [("task", vec![task])];
    reader.stage("copy_held", &tables).unwrap();
    // A newer build moves the board one schema on, its step declared to keep the reader writing.
    let next = Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;");
    let newer = opened(&Opening::write().at(next.keeping_writes()));
    let ahead = crate::schema::VERSION + 1;
    assert_eq!(
        reader.board_standing().unwrap(),
        Standing {
            version: Some(ahead),
            writes: true
        }
    );

    let counters = json!({"last": 0, "last_task": 1, "last_ask": 0});
    let why = match reader.swap_staged("copy_held", &tables, counters) {
        Err(Error::Refused(why)) => why,
        other => panic!("the swap ran on the board one schema ahead: {other:?}"),
    };
    assert!(why.contains(&format!("at schema {ahead}")), "{why}");
    assert_eq!(
        newer.get("old/one").unwrap().title,
        "Made at this version",
        "the board's rows are its own"
    );
    assert_eq!(
        generation(),
        before,
        "the recovery generation is the board's own"
    );
    // The hold is the swap's: the reader's ordinary write takes the board.
    reader
        .say("old/one", "owner", "one schema ahead", None)
        .unwrap();
    let thread = newer.thread("old/one").unwrap();
    assert_eq!(thread.last().unwrap().text, "one schema ahead");
}
