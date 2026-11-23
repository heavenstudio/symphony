//! A request on a hosted board: one transaction with its own variables, its errors read as the
//! store reads them, and a connection lost before or after its commit told apart and never
//! sent again.

use crate::cluster::{self, MAJORS, Project, environment, refused, settings};
use std::time::Duration;
use symphony_board::db::DbError;
use symphony_board::pg::Step;
use symphony_board::pg::hosted::{Client, Counts};

const READY: Duration = test_time::backstop(Duration::from_secs(10));

fn table(project: &Project) {
    let client = project.client("web");
    client
        .simple("CREATE TABLE t (id int PRIMARY KEY, n int NOT NULL); INSERT INTO t VALUES (1, 0);")
        .unwrap();
}

fn n(project: &Project) -> String {
    project
        .cluster
        .value(&project.name, "SELECT coalesce(sum(n), -1) FROM t")
}

#[test]
fn a_request_is_one_repeatable_read_transaction_with_variables_of_its_own() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        let answers = client
            .request(&[
                Step::with("SELECT FROM set_config('board.vars', $1::text, true)", r#"{"by": 5, "who": null}"#),
                Step::bare("UPDATE t SET n = n + (current_setting('board.vars')::jsonb ->> 'by')::int"),
                Step::bare("SELECT current_setting('transaction_isolation'), current_setting('board.vars')::jsonb ->> 'who', n FROM t"),
            ])
            .unwrap();
        assert_eq!(answers.len(), 3);
        assert!(answers[0].rows[0].is_empty(), "a row of no columns");
        assert_eq!(
            answers[2].rows,
            [[
                Some("repeatable read".to_string()),
                None,
                Some("5".to_string())
            ]],
            "PostgreSQL {major}"
        );
        // The variables were the transaction's: the next request, on the same connection, has none.
        assert_eq!(
            Project::value(
                &client,
                "SELECT coalesce(nullif(current_setting('board.vars', true), ''), 'unset')"
            ),
            "unset"
        );
        assert_eq!(
            client.counts(),
            Counts {
                idle: 1,
                ..Counts::default()
            }
        );
        // A read sent alone binds its variables as jsonb, and reads at the same isolation.
        let read = client
            .request(&[Step::with(
                "SELECT ($1::jsonb -> 'deep' -> 'null')::text, jsonb_typeof($1), current_setting('transaction_isolation'), $1::jsonb ->> 'é'",
                r#"{"deep": {"null": null}, "é": "ü☃"}"#,
            )])
            .unwrap();
        assert_eq!(
            read[0].rows,
            [[
                Some("null".to_string()),
                Some("object".to_string()),
                Some("repeatable read".to_string()),
                Some("ü☃".to_string())
            ]]
        );
    }
}

#[test]
fn a_refused_statement_rolls_the_whole_request_back_and_leaves_the_connection_clean() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        let thrown = client.request(&[
            Step::bare("UPDATE t SET n = 7"),
            Step::bare("DO $$ BEGIN RAISE EXCEPTION 'the board says %', 'no'; END $$"),
            Step::bare("UPDATE t SET n = 8"),
        ]);
        assert_eq!(thrown, Err(DbError::Thrown("the board says no".into())));
        assert_eq!(
            n(&project),
            "0",
            "PostgreSQL {major}: nothing of it was committed"
        );
        let failed = refused(client.request(&[
            Step::bare("UPDATE t SET n = 7"),
            Step::bare("INSERT INTO t VALUES (1, 41414141)"),
        ]));
        assert!(
            failed.starts_with("the hosted board's PostgreSQL refused a statement: a constraint a row would break [backend ")
                && failed.ends_with("(SQLSTATE 23505)"),
            "{failed}"
        );
        assert!(
            !failed.contains("41414141") && !failed.contains("INSERT"),
            "{failed}"
        );
        assert_eq!(n(&project), "0");
        assert_eq!(
            client.counts(),
            Counts {
                idle: 1,
                ..Counts::default()
            },
            "kept: it is clean"
        );
        assert_eq!(project.cluster.sessions(&project.name), 1);
        client
            .request(&[Step::bare("UPDATE t SET n = 1"), Step::bare("SELECT 1")])
            .unwrap();
        assert_eq!(n(&project), "1");
    }
}

#[test]
fn a_commit_the_server_refuses_gives_no_answers_and_commits_nothing() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        client
            .simple("CREATE TABLE child (t int REFERENCES t (id) DEFERRABLE INITIALLY DEFERRED);")
            .unwrap();
        let failed = refused(client.request(&[
            Step::bare("UPDATE t SET n = 9"),
            Step::bare("INSERT INTO child VALUES (404)"),
            Step::bare("SELECT 'answered before the commit'"),
        ]));
        assert!(
            failed.ends_with("(SQLSTATE 23503)"),
            "PostgreSQL {major}: {failed}"
        );
        assert_eq!(n(&project), "0");
        assert_eq!(
            client.counts().idle,
            1,
            "the server rolled it back: the connection is clean"
        );
    }
}

#[test]
fn a_write_that_met_anothers_since_its_snapshot_is_a_conflict_to_decide_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        let operator = project.cluster.operator(&project.name);
        operator.simple("SELECT pg_advisory_lock(4242)").unwrap();
        std::thread::scope(|scope| {
            let late = scope.spawn(|| {
                client.request(&[
                    Step::bare("SELECT n FROM t"),
                    Step::bare("SELECT pg_advisory_xact_lock(4242)"),
                    Step::bare("UPDATE t SET n = n + 100"),
                ])
            });
            test_time::wait::until_true("the request to wait on the lock", READY, || {
                project.cluster.value(
                    "postgres",
                    &format!("SELECT count(*) FROM pg_stat_activity WHERE usename = '{}' AND wait_event_type = 'Lock'", project.name),
                ) == "1"
            })
            .unwrap();
            operator
                .request(&[Step::bare("UPDATE t SET n = n + 1")])
                .unwrap();
            operator.simple("SELECT pg_advisory_unlock(4242)").unwrap();
            assert_eq!(
                late.join().unwrap(),
                Err(DbError::Conflict),
                "PostgreSQL {major}"
            );
        });
        assert_eq!(n(&project), "1");
        assert_eq!(client.counts().idle, 1);
    }
}

#[test]
fn a_connection_lost_before_its_commit_committed_nothing_and_is_not_sent_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        let lost = refused(client.request(&[
            Step::bare("UPDATE t SET n = n + 1"),
            Step::bare("SELECT pg_terminate_backend(pg_backend_pid())"),
            Step::bare("UPDATE t SET n = n + 1"),
        ]));
        assert!(
            lost.contains("the server ended the session before any commit")
                && lost.contains("nothing of the request was committed"),
            "PostgreSQL {major}: {lost}"
        );
        assert_eq!(n(&project), "0");
        assert_eq!(
            client.counts(),
            Counts::default(),
            "the connection is gone, not lent again"
        );
        assert_eq!(client.requests(), 1, "sent once");
        // The next request makes a connection of its own.
        client
            .request(&[Step::bare("UPDATE t SET n = n + 1"), Step::bare("SELECT 1")])
            .unwrap();
        assert_eq!(n(&project), "1");
    }
}

#[test]
fn an_idle_connection_the_server_ended_is_replaced_for_the_next_request() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        assert_eq!(Project::value(&client, "SELECT 1"), "1");
        project.cluster.terminate(&project.name);
        // The pool sees its end when it next looks at the connection: nothing is sent on it.
        test_time::wait::until_true(
            "the request after the server ended the session",
            READY,
            || {
                client
                    .request(&[Step::bare("UPDATE t SET n = n + 1"), Step::bare("SELECT 1")])
                    .is_ok()
            },
        )
        .unwrap();
        assert_eq!(n(&project), "1", "PostgreSQL {major}: written once");
    }
}

#[test]
fn a_script_says_its_own_transactions_and_one_that_fails_is_rolled_back() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client(symphony_board::pg::external::MAINTENANCE);
        client
            .simple("BEGIN ISOLATION LEVEL READ COMMITTED; UPDATE t SET n = 3; COMMIT;")
            .unwrap();
        assert_eq!(n(&project), "3");
        let failed = refused(
            client.simple("BEGIN; UPDATE t SET n = 4; SELECT no_such_column FROM t; COMMIT;"),
        );
        assert!(
            failed.ends_with("(SQLSTATE 42703)") && !failed.contains("no_such_column"),
            "{failed}"
        );
        assert_eq!(n(&project), "3", "PostgreSQL {major}");
        assert_eq!(client.counts().idle, 1);
        assert_eq!(
            Project::value(&client, "SELECT current_setting('transaction_isolation')"),
            "repeatable read"
        );
    }
}

#[test]
fn a_session_keeps_nothing_of_a_request_but_its_slot() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        for _ in 0..3 {
            client
                .request(&[
                    Step::with("SELECT FROM set_config('board.vars', $1::text, true)", "{}"),
                    Step::bare("UPDATE t SET n = n + 1"),
                ])
                .unwrap();
        }
        let kept = client
            .request(&[Step::bare(
                "SELECT (SELECT count(*) FROM pg_prepared_statements), \
                 (SELECT count(*) FROM pg_class WHERE relpersistence = 't'), \
                 (SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND pid = pg_backend_pid()), \
                 current_setting('search_path'), current_setting('default_transaction_isolation'), \
                 current_setting('application_name'), current_setting('client_encoding')",
            )])
            .unwrap();
        assert_eq!(
            kept[0].rows[0],
            [
                "0",
                "0",
                "1",
                "public",
                "repeatable read",
                "symphony-board",
                "UTF8"
            ]
            .map(|text| Some(text.to_string())),
            "PostgreSQL {major}"
        );
    }
}

#[test]
fn a_statement_sent_alone_that_fails_or_answers_what_is_not_read_commits_nothing() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        assert_eq!(client.server_major(), Ok(major));
        let failed = refused(client.request(&[Step::bare("UPDATE t SET n = 1 / (n - n)")]));
        assert!(
            failed.ends_with("(SQLSTATE 22012)"),
            "PostgreSQL {major}: {failed}"
        );
        assert_eq!(
            client.counts().idle,
            1,
            "a statement's own error leaves it clean"
        );
        // A column of a type the client does not read as the socket client's text is refused,
        // whatever the statement did before it, and its connection is not lent again.
        let unread = refused(
            client.request(&[Step::bare("UPDATE t SET n = 5"), Step::bare("SELECT now()")]),
        );
        assert!(
            unread.contains("a column of a type the hosted client does not read"),
            "PostgreSQL {major}: {unread}"
        );
        assert_eq!(n(&project), "0");
        assert_eq!(client.counts(), Counts::default());
    }
}

#[test]
fn a_script_the_server_ends_or_does_not_answer_is_unknown_and_is_not_sent_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client(symphony_board::pg::external::MAINTENANCE);
        let ended = refused(client.simple(
            "BEGIN; UPDATE t SET n = 6; SELECT pg_terminate_backend(pg_backend_pid()); COMMIT;",
        ));
        assert!(
            ended.contains("while a script that commits ran")
                && ended.contains("whether the request was committed is not known"),
            "PostgreSQL {major}: {ended}"
        );
        assert_eq!(n(&project), "0");
        let operator = project.cluster.operator(&project.name);
        operator.simple("SELECT pg_advisory_lock(4244)").unwrap();
        let hasty = client.bounded(Duration::from_millis(500));
        let late = refused(
            hasty.simple("BEGIN; UPDATE t SET n = 7; SELECT pg_advisory_xact_lock(4244); COMMIT;"),
        );
        assert!(
            late.contains("the server did not answer a script that commits in time"),
            "PostgreSQL {major}: {late}"
        );
        test_time::wait::until_true("the cancelled session to end", READY, || {
            project.cluster.sessions(&project.name) == 0
        })
        .unwrap();
        operator.simple("SELECT pg_advisory_unlock(4244)").unwrap();
        assert_eq!(n(&project), "0");
        assert_eq!(
            (client.requests(), hasty.requests()),
            (1, 1),
            "each sent once"
        );
    }
}

/// The board's health counts a request as unanswered only when it got no answer, a session the
/// server ended or a wait run out, never for an error the server answered: a raised exception, a
/// row a constraint refused, a column the client does not read.
#[test]
fn only_a_request_that_got_no_answer_is_unanswered_in_the_boards_health() {
    let unanswered = |client: &symphony_board::pg::hosted::Client| {
        let (queries, _) = client
            .queries()
            .report(symphony_board::sweep::trigger::now());
        (queries.count, queries.failed)
    };
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web");
        let before = unanswered(&client).0;
        let thrown = client.request(&[Step::bare(
            "DO $$ BEGIN RAISE EXCEPTION 'the board says no'; END $$",
        )]);
        assert_eq!(thrown, Err(DbError::Thrown("the board says no".into())));
        refused(client.request(&[Step::bare("INSERT INTO t VALUES (1, 1)")]));
        refused(client.request(&[Step::bare("SELECT now()")]));
        assert_eq!(
            unanswered(&client),
            (before + 3, 0),
            "PostgreSQL {major}: each answered"
        );
        refused(client.request(&[Step::bare("SELECT pg_terminate_backend(pg_backend_pid())")]));
        let operator = project.cluster.operator(&project.name);
        operator.simple("SELECT pg_advisory_lock(4245)").unwrap();
        let hasty = client.bounded(Duration::from_millis(500));
        let late = refused(hasty.request(&[Step::bare("SELECT pg_advisory_xact_lock(4245)")]));
        assert!(
            late.contains("the server did not answer in time"),
            "PostgreSQL {major}: {late}"
        );
        test_time::wait::until_true("the cancelled session to end", READY, || {
            project.cluster.sessions(&project.name) == 0
        })
        .unwrap();
        operator.simple("SELECT pg_advisory_unlock(4245)").unwrap();
        assert_eq!(
            unanswered(&client),
            (before + 5, 2),
            "PostgreSQL {major}: the ended session and the wait run out went unanswered"
        );
    }
}

/// A connection the server refused is answered in the board's health, as the socket client counts
/// a startup the server refused: a login it does not let in, a database it does not hold, and a
/// certificate this side does not take. Each is refused in its own words and leaves no session.
#[test]
fn a_connection_the_server_refused_is_no_unanswered_request() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let wrong = project.url().replace(
            &cluster::escaped(&project.password),
            "not-the-password-41414141",
        );
        let elsewhere = format!(
            "{}never_made",
            project.url().strip_suffix(project.name.as_str()).unwrap()
        );
        let ca = project.cluster.ca.display().to_string();
        let other_ca = project.cluster.other_ca.display().to_string();
        for (case, url, ca, said) in [
            (
                "a wrong password",
                wrong,
                &ca,
                "the server refused the login",
            ),
            (
                "a database not there",
                elsewhere,
                &ca,
                "the database is not there",
            ),
            (
                "a certificate of another authority",
                project.url(),
                &other_ca,
                "not one the deployment's authority signed",
            ),
        ] {
            let vars = [
                ("URL", url),
                ("CA", ca.clone()),
                ("CLASS", "web".to_string()),
            ];
            let client = Client::connect(&settings(""), environment(&vars)).unwrap();
            let (before, _) = client
                .queries()
                .report(symphony_board::sweep::trigger::now());
            let refusal = refused(client.request(&[Step::bare("SELECT 1")]));
            assert!(
                refusal.contains(said),
                "PostgreSQL {major}, {case}: {refusal}"
            );
            let (after, _) = client
                .queries()
                .report(symphony_board::sweep::trigger::now());
            assert_eq!(
                (after.count - before.count, after.failed - before.failed),
                (1, 0),
                "PostgreSQL {major}, {case}: one request, answered"
            );
            assert_eq!(
                client.counts(),
                Counts::default(),
                "PostgreSQL {major}, {case}"
            );
        }
        assert_eq!(
            project.cluster.sessions(&project.name),
            0,
            "PostgreSQL {major}"
        );
    }
}
