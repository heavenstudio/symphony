//! A whole-board copy through a reader's store is held to the store's own schema at its swap as
//! at its staging: staged at the reader's schema, its swap is refused on a board a migration moved
//! one schema on meanwhile, though that step keeps the reader writing, and the board, its rows and
//! its recovery generation are as they were. On SurrealDB, whose copy is staged across requests;
//! PostgreSQL's is one request, held as staging is.

use super::*;
use crate::store_fixture::{Backend, HOST, NOW};
use crate::{NewTask, Opening, Schema, Standing};

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
