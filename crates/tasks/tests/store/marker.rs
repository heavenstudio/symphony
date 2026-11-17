//! The board's marker on either database: it moves with every commit a reader can see, its own
//! writer's or another's, and with nothing else: no read, no refused write, and no writer's line.

use crate::common::{Backend, Fixture, Seed, refused};
use symphony_board::{NewTask, Raw, Store};

fn board(backend: Backend) -> Fixture {
    Fixture::on(backend, Seed::default().task("tools/one", "ready"))
}

/// The marker `store` reads, which must have moved past `was` after `what`.
fn moved(store: &Store, was: &mut u64, what: &str) {
    let now = store.marker().unwrap();
    assert!(now > *was, "{what}: the marker stayed at {was}, read {now}");
    *was = now;
}

on_both! {
    fn every_commit_a_reader_can_see_moves_the_marker_and_nothing_else_does(backend: Backend) {
        let f = board(backend);
        let store = &f.store;
        let mut was = store.marker().unwrap();
        store
            .create(&NewTask::new("tools/two", "Another", "owner"))
            .unwrap();
        moved(store, &mut was, "a task made");
        store.say("tools/one", "owner", "a line", None).unwrap();
        moved(store, &mut was, "a line said");
        store.set_priority("tools/one", 1, "owner").unwrap();
        moved(store, &mut was, "a priority set");
        let handle = f.session("worker");
        moved(store, &mut was, "a session registered");
        store
            .say("tools/one", &handle, "a session's line", None)
            .unwrap();
        moved(store, &mut was, "a session's line");

        store.board().unwrap();
        store.get("tools/one").unwrap();
        store.thread("tools/one").unwrap();
        store.tasks(None).unwrap();
        assert_eq!(store.marker().unwrap(), was, "a read moves no marker");

        refused(store.say("tools/none", "owner", "nowhere", None));
        refused(store.set_priority("tools/none", 2, "owner"));
        assert_eq!(store.marker().unwrap(), was, "a refused write commits nothing");

        let other = f.other();
        other.say("tools/one", "owner", "from another", None).unwrap();
        moved(store, &mut was, "another writer's line, read by this one");
        assert_eq!(other.marker().unwrap(), was, "both read the one marker");
    }

    fn a_writers_line_moves_no_marker(backend: Backend) {
        let f = board(backend);
        let was = f.store.marker().unwrap();
        let lines = || Raw::of(f.store.database()).table("writer").count().unwrap();
        let before = lines();
        // A writer on another host opens the board, and its line is written.
        let other = f.elsewhere();
        assert_eq!(lines(), before + 1, "the other host's line is written");
        assert_eq!(f.store.marker().unwrap(), was, "a writer's line moves no marker");
        assert_eq!(other.marker().unwrap(), was);
    }
}

#[test]
fn a_statement_with_no_postgresql_twin_yet_is_refused_there_naming_its_line_and_nothing_is_sent() {
    let f = board(Backend::PostgreSql);
    let sent = f.store.database().requests();
    let why = f
        .store
        .say_keyed("tools/one", "owner", "once", None, "a-key")
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("has no PostgreSQL twin yet: nothing was sent"),
        "{why}"
    );
    assert!(why.contains("src/store/messages.rs:"), "{why}");
    assert_eq!(f.store.database().requests(), sent, "nothing was sent");
    assert_eq!(f.store.thread("tools/one").unwrap().len(), 0);
}
