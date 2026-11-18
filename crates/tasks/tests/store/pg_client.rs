//! The board's PostgreSQL client against the real server, the test binary's scratch one
//! (`common::postgresql`), each test in a database of its own: what the store relies on of it,
//! which its recorded-bytes tests (`src/pg/tests.rs`) cannot show of a server's own behaviour.

use crate::common::{bounded, postgresql};
use std::time::Duration;
use symphony_board::pg::{self, Answer, Step};
use symphony_board::rpc::DbError;

/// A client of a database of its own, made, holding the table `t` and its one row `(1, 0)`.
fn with_a_row() -> pg::Client {
    let client = postgresql();
    client.make_database().unwrap();
    client
        .simple("CREATE TABLE t (id int PRIMARY KEY, n int); INSERT INTO t VALUES (1, 0);")
        .unwrap();
    client
}

/// The one value of `answers`' answer at `at`.
fn value(answers: &[Answer], at: usize) -> Option<&str> {
    answers[at].value()
}

fn n(client: &pg::Client) -> String {
    let read = client.request(&[Step::bare("SELECT n FROM t WHERE id = 1")]);
    value(&read.unwrap(), 0).unwrap().to_string()
}

#[test]
fn a_database_not_there_is_known_by_its_code_and_making_it_twice_is_making_it_once() {
    let client = postgresql();
    let Err(DbError::Failed(said)) = client.request(&[Step::bare("SELECT 1")]) else {
        panic!("a database not made yet refuses a connection");
    };
    assert!(pg::no_database(&said), "{said}");
    client.make_database().unwrap();
    client.another().make_database().unwrap();
    let answers = client.request(&[Step::bare("SELECT 1")]).unwrap();
    assert_eq!(value(&answers, 0), Some("1"));
}

#[test]
fn concurrent_database_makers_all_reach_the_same_database() {
    let client = postgresql();
    let start = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let makers: Vec<_> = (0..8)
            .map(|_| {
                let client = client.another();
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    client.make_database().unwrap();
                    assert_eq!(
                        value(&client.request(&[Step::bare("SELECT 1")]).unwrap(), 0),
                        Some("1")
                    );
                })
            })
            .collect();
        for maker in makers {
            maker.join().unwrap();
        }
    });
}

#[test]
fn a_parameter_is_one_jsonb_and_a_null_column_is_none() {
    let client = postgresql();
    client.make_database().unwrap();
    let answers = client
        .request(&[Step::with(
            "SELECT $1::jsonb -> 'a', NULL::text, ($1::jsonb ->> 'b')",
            r#"{"a": [1, "x"], "b": "é ✓"}"#,
        )])
        .unwrap();
    assert_eq!(
        answers[0].rows,
        [vec![
            Some(r#"[1, "x"]"#.to_string()),
            None,
            Some("é ✓".to_string())
        ]]
    );
}

#[test]
fn a_setting_made_for_the_transaction_is_read_by_the_later_statements_of_its_request_alone() {
    let client = postgresql();
    client.make_database().unwrap();
    let answers = client
        .request(&[
            Step::with("SELECT set_config('board.vars', $1::text, true)", r#"{"x":1}"#),
            Step::bare("DO $$ BEGIN PERFORM set_config('board.answer', current_setting('board.vars'), true); END $$"),
            Step::bare("SELECT current_setting('board.answer')"),
        ])
        .unwrap();
    assert_eq!(value(&answers, 2), Some(r#"{"x": 1}"#));
    let after = client
        .request(&[Step::bare("SELECT current_setting('board.vars', true)")])
        .unwrap();
    assert_eq!(
        value(&after, 0),
        Some(""),
        "the setting ended with its transaction"
    );
}

#[test]
fn a_raised_exception_is_thrown_rolls_its_request_back_and_the_connection_serves_the_next() {
    let client = with_a_row();
    let thrown = client.request(&[
        Step::bare("UPDATE t SET n = 5 WHERE id = 1"),
        Step::bare("DO $$ BEGIN RAISE EXCEPTION 'cut short'; END $$"),
        Step::bare("UPDATE t SET n = 6 WHERE id = 1"),
    ]);
    assert_eq!(thrown.err(), Some(DbError::Thrown("cut short".into())));
    assert_eq!(n(&client), "0", "nothing of the request was kept");
    let failed = client.simple("BEGIN; UPDATE t SET n = 7; SELECT no_such_column FROM t; COMMIT;");
    let Err(DbError::Failed(said)) = failed else {
        panic!("a failed simple query fails: {failed:?}");
    };
    assert!(said.contains("(SQLSTATE 42703)"), "{said}");
    assert_eq!(n(&client), "0", "a failed transaction is rolled back");
    // The table's, then the four here: the rollback after a failure is no request of its own.
    assert_eq!(client.requests(), 5);
}

#[test]
fn two_clients_writing_one_row_at_once_meet_as_a_conflict_and_the_first_to_commit_stands() {
    let client = with_a_row();
    // A third connection holds the lock the first waits on, after its snapshot and before its
    // write, so the second commits in between.
    let holder = client.another();
    holder.simple("SELECT pg_advisory_lock(4242)").unwrap();
    let first = client.another();
    let met = std::thread::scope(|scope| {
        let writing = scope.spawn(|| {
            first.request(&[
                Step::bare("SELECT n FROM t WHERE id = 1"),
                Step::bare("SELECT pg_advisory_xact_lock(4242)"),
                Step::bare("UPDATE t SET n = n + 1 WHERE id = 1"),
            ])
        });
        let waiting = || {
            let read = holder.request(&[Step::bare(
                "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 4242 \
                 AND NOT granted AND database = \
                 (SELECT oid FROM pg_database WHERE datname = current_database())",
            )]);
            value(&read.unwrap(), 0) == Some("1")
        };
        let backstop = test_time::backstop(Duration::from_secs(5));
        test_time::until_true("the first writer waits on the lock", backstop, waiting).unwrap();
        client
            .request(&[Step::bare("UPDATE t SET n = n + 10 WHERE id = 1")])
            .unwrap();
        holder.simple("SELECT pg_advisory_unlock(4242)").unwrap();
        writing.join().unwrap()
    });
    assert_eq!(met.err(), Some(DbError::Conflict));
    assert_eq!(
        n(&client),
        "10",
        "the second's write stands, the first's is not kept"
    );
    first
        .request(&[Step::bare("UPDATE t SET n = n + 1 WHERE id = 1")])
        .unwrap();
    assert_eq!(n(&client), "11", "the first writes when it tries again");
}

#[test]
fn a_request_whose_answer_comes_while_it_is_still_being_sent_is_read_as_it_comes() {
    let client = postgresql();
    client.make_database().unwrap();
    // A megabyte, sent twice: the first statement answers with all of it before the server has
    // read the second, so a client that reads only once it has written would wait on a server
    // that waits on it, each with its socket full.
    let long = format!("\"{}\"", "x".repeat(1 << 20));
    let asked = client.another();
    let answers = bounded(
        "a request answering a megabyte while a megabyte is still being sent",
        Duration::from_secs(10),
        move || {
            asked.request(&[
                Step::with("SELECT $1::jsonb #>> '{}'", &long),
                Step::with("SELECT length($1::jsonb #>> '{}')", &long),
            ])
        },
    )
    .unwrap();
    assert_eq!(value(&answers, 0).map(str::len), Some(1 << 20));
    assert_eq!(value(&answers, 1), Some("1048576"));
}
