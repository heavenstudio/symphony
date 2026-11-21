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

/// Waits until one connection of `holder`'s database waits on the advisory lock `key`, which
/// `holder` holds: `what` it is that waits.
fn awaited(holder: &pg::Client, key: u32, what: &str) {
    let waiting = || {
        let read = holder.request(&[Step::bare(&format!(
            "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = {key} \
             AND NOT granted AND database = \
             (SELECT oid FROM pg_database WHERE datname = current_database())"
        ))]);
        value(&read.unwrap(), 0) == Some("1")
    };
    let backstop = test_time::backstop(Duration::from_secs(5));
    test_time::until_true(what, backstop, waiting).unwrap();
}

#[test]
fn the_statements_of_one_request_read_one_snapshot_whatever_commits_between_them() {
    let client = with_a_row();
    // Another connection holds the lock the reader waits on between its statements, and a third
    // commits meanwhile: a read sent as a statement a list ([`pg::Answer::column`]) is one board.
    let holder = client.another();
    holder.simple("SELECT pg_advisory_lock(4343)").unwrap();
    let reader = client.another();
    let read = std::thread::scope(|scope| {
        let reading = scope.spawn(|| {
            reader.request(&[
                Step::bare("SELECT n FROM t ORDER BY id"),
                Step::bare("SELECT FROM pg_advisory_xact_lock(4343)"),
                Step::bare("SELECT n FROM t ORDER BY id"),
                Step::bare("SELECT count(*) FROM t"),
            ])
        });
        awaited(&holder, 4343, "the reader waits on the lock");
        client
            .request(&[
                Step::bare("UPDATE t SET n = 10 WHERE id = 1"),
                Step::bare("INSERT INTO t VALUES (2, 20)"),
            ])
            .unwrap();
        holder.simple("SELECT pg_advisory_unlock(4343)").unwrap();
        reading.join().unwrap()
    })
    .unwrap();
    assert_eq!(read[0].column(), ["0"]);
    assert_eq!(
        read[2].column(),
        ["0"],
        "what committed after its first statement"
    );
    assert_eq!(value(&read, 3), Some("1"));
    let after = reader
        .request(&[Step::bare("SELECT n FROM t ORDER BY id")])
        .unwrap();
    assert_eq!(after[0].column(), ["10", "20"], "its next request reads it");
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
        awaited(&holder, 4242, "the first writer waits on the lock");
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

/// A board writer or reader on another connection, with the fixture's clock and host.
fn marker_store(client: &pg::Client, how: symphony_board::Opening) -> symphony_board::Store {
    symphony_board::Store::opening_on(
        client.another().into(),
        Box::new(|| crate::common::NOW),
        Box::new(|_| false),
        crate::common::HOST,
        &how,
    )
    .unwrap()
}

fn marker_client(store: &symphony_board::Store) -> &pg::Client {
    let symphony_board::Database::PostgreSql(client) = store.database() else {
        unreachable!()
    };
    client
}

fn marker_value(client: &pg::Client, expression: &str) -> i64 {
    let answers = client
        .request(&[Step::bare(&format!("SELECT {expression}"))])
        .unwrap();
    value(&answers, 0).unwrap().parse().unwrap()
}

/// Three committed marker rows whose owning connections have ended. They use the already-fresh
/// program line, so making them does not itself perform another writer-line refresh.
fn ended_marker_writers(client: &pg::Client) -> Vec<i64> {
    let mut pids = Vec::new();
    for n in 0..3 {
        let writer = marker_store(client, symphony_board::Opening::write());
        writer
            .create(&symphony_board::NewTask::new(
                &format!("ended/{n}"),
                "From an ended connection",
                "owner",
            ))
            .unwrap();
        pids.push(marker_value(marker_client(&writer), "pg_backend_pid()"));
    }
    await_ended(client, &pids);
    pids
}

/// Wait until the server lists none of `pids`, its processes for connections now closed: a fold
/// takes the rows of ended connections alone, and a server ends one some time after its client
/// closed it, later the busier the machine.
fn await_ended(client: &pg::Client, pids: &[i64]) {
    let encoded = serde_json::to_string(pids).unwrap();
    test_time::until_true(
        "the committed writers' own connections have ended",
        test_time::backstop(Duration::from_secs(5)),
        || {
            let answers = client
                .request(&[Step::with(
                    "SELECT count(*) FROM pg_stat_activity WHERE pid IN \
                     (SELECT value::int FROM jsonb_array_elements_text($1::jsonb))",
                    &encoded,
                )])
                .unwrap();
            value(&answers, 0) == Some("0")
        },
    )
    .unwrap();
}

fn marker_rows(client: &pg::Client) -> i64 {
    marker_value(client, "count(*) FROM counter WHERE id ->> 0 = 'board'")
}

fn refresh_marker(client: &pg::Client, name: &str) -> symphony_board::Store {
    marker_store(
        client,
        symphony_board::Opening::write().by(symphony_board::Program::named(name, name)),
    )
}

#[test]
fn ended_marker_rows_fold_without_moving_the_sum_and_active_writers_keep_advancing() {
    let client = postgresql();
    let active = marker_store(&client, symphony_board::Opening::write());
    active
        .create(&symphony_board::NewTask::new(
            "active/one",
            "Still connected",
            "owner",
        ))
        .unwrap();
    ended_marker_writers(&client);
    let was = active.marker().unwrap();
    let rows = marker_rows(&client);
    assert_eq!(rows, 4, "one active writer and three ended writers");
    let reader = marker_store(&client, symphony_board::Opening::read());
    assert_eq!(reader.marker().unwrap(), was);
    assert_eq!(
        marker_rows(&client),
        rows,
        "a reader performs no maintenance"
    );
    let refreshed = refresh_marker(&client, "fold-markers");
    assert_eq!(active.marker().unwrap(), was, "a fold preserves the total");
    assert_eq!(refreshed.marker().unwrap(), was);
    assert_eq!(
        marker_rows(&client),
        2,
        "the retained base and the active writer"
    );
    assert_eq!(
        marker_value(&client, "last FROM counter WHERE id = '[\"board\"]'::jsonb"),
        3,
        "the base retains exactly the ended writers' commits"
    );
    for priority in [1, 2] {
        let before = active.marker().unwrap();
        active
            .set_priority("active/one", priority, "owner")
            .unwrap();
        assert!(
            active.marker().unwrap() > before,
            "the active backend still advances"
        );
    }
    refreshed
        .create(&symphony_board::NewTask::new(
            "new/one",
            "After the fold",
            "owner",
        ))
        .unwrap();
    let committed = active.marker().unwrap();
    assert!(committed > was);
    let ended = marker_value(marker_client(&refreshed), "pg_backend_pid()");
    drop(refreshed);
    await_ended(&client, &[ended]);
    refresh_marker(&client, "fold-markers-again");
    assert_eq!(
        active.marker().unwrap(),
        committed,
        "folding into an existing base preserves it"
    );
    assert_eq!(marker_rows(&client), 2);
}

#[test]
fn a_refused_writer_refresh_rolls_back_its_marker_deletion_and_base_addition() {
    let client = postgresql();
    let active = marker_store(&client, symphony_board::Opening::write());
    ended_marker_writers(&client);
    let was = active.marker().unwrap();
    let rows = marker_rows(&client);
    client
        .simple(
            "CREATE FUNCTION fixture_refuse_writer() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN RAISE EXCEPTION 'fixture refuses writer refresh'; END $$; \
        CREATE TRIGGER fixture_refuse_writer BEFORE INSERT OR UPDATE ON writer \
        FOR EACH ROW EXECUTE FUNCTION fixture_refuse_writer();",
        )
        .unwrap();
    // Writer-line failure remains best effort, but none of that transaction's fold can survive.
    refresh_marker(&client, "refused-marker-refresh");
    assert_eq!(active.marker().unwrap(), was);
    assert_eq!(marker_rows(&client), rows);
    assert_eq!(
        marker_value(
            &client,
            "count(*) FROM counter WHERE id = '[\"board\"]'::jsonb"
        ),
        0
    );
    client
        .simple(
            "DROP TRIGGER fixture_refuse_writer ON writer; \
        DROP FUNCTION fixture_refuse_writer();",
        )
        .unwrap();
    refresh_marker(&client, "refused-marker-refresh");
    assert_eq!(active.marker().unwrap(), was);
    assert_eq!(
        marker_rows(&client),
        1,
        "a later successful refresh folds them"
    );
}

#[test]
fn concurrent_marker_refreshes_never_count_an_ended_backend_twice() {
    let client = postgresql();
    let active = marker_store(&client, symphony_board::Opening::write());
    ended_marker_writers(&client);
    let was = active.marker().unwrap();
    let start = std::sync::Barrier::new(6);
    std::thread::scope(|scope| {
        let refreshes: Vec<_> = (0..6)
            .map(|n| {
                let (client, start) = (&client, &start);
                scope.spawn(move || {
                    start.wait();
                    refresh_marker(client, &format!("concurrent-marker-{n}"));
                })
            })
            .collect();
        for refresh in refreshes {
            refresh.join().unwrap();
        }
    });
    assert_eq!(active.marker().unwrap(), was);
    assert_eq!(
        marker_rows(&client),
        1,
        "the ended writers survive in one base row"
    );
    active
        .create(&symphony_board::NewTask::new(
            "after/one",
            "After concurrent folds",
            "owner",
        ))
        .unwrap();
    assert!(active.marker().unwrap() > was);
}
