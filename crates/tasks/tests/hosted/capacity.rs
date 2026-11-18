//! What bounds a hosted board's connections: a process's pool and its line, the deployment's
//! budget counted in the database across processes, and the server's own cap for the login; a
//! request given up on, before its commit and at it; and a rotated secret.

use crate::cluster::{self, MAJORS, Project, environment, refused, settings};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphony_board::pg::Step;
use symphony_board::pg::hosted::{Client, Counts};

const READY: Duration = test_time::backstop(Duration::from_secs(10));
/// The advisory lock the operator holds while a test's requests wait on it.
const HELD: &str = "4243";

fn table(project: &Project) {
    project.cluster.operate(
        &project.name,
        &format!("SET ROLE {}; CREATE TABLE t (id int PRIMARY KEY, n int NOT NULL); INSERT INTO t VALUES (1, 0);", project.name),
    );
}

fn n(project: &Project) -> String {
    project.cluster.value(&project.name, "SELECT n FROM t")
}

/// A request that writes, then waits on the lock the operator holds.
fn held() -> [Step<'static>; 2] {
    [
        Step::bare("UPDATE t SET n = n + 1"),
        Step::bare("SELECT pg_advisory_xact_lock(4243)"),
    ]
}

/// How many of the login's sessions wait on a lock.
fn waiting(project: &Project) -> String {
    project.cluster.value(
        "postgres",
        &format!("SELECT count(*) FROM pg_stat_activity WHERE usename = '{}' AND wait_event_type = 'Lock'", project.name),
    )
}

fn until(what: &str, so: impl FnMut() -> bool) {
    test_time::wait::until_true(what, READY, so).unwrap();
}

#[test]
fn a_pool_lends_its_share_and_refuses_the_line_past_its_length_and_its_wait() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        // Two clients of the one pool: one waits for a connection as long as it takes, the
        // other a second, so which refusal a borrower is given never depends on who was first.
        let client = Client::connect(
            &settings("acquire_timeout_seconds = 3600\nqueue_max = 1"),
            project.environment("solo"),
        )
        .unwrap();
        let hasty = Client::connect(
            &settings("acquire_timeout_seconds = 1\nqueue_max = 1"),
            project.environment("solo"),
        )
        .unwrap();
        let operator = project.cluster.operator(&project.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| client.request(&held()));
            until("the first request to wait on the lock", || {
                waiting(&project) == "1"
            });
            let second = scope.spawn(|| client.request(&[Step::bare("SELECT 1")]));
            until("the second request to wait in line", || {
                client.counts().waiting == 1
            });
            assert_eq!(
                client.counts(),
                Counts {
                    active: 1,
                    waiting: 1,
                    ..Counts::default()
                }
            );
            let third = refused(client.request(&[Step::bare("SELECT 1")]));
            assert!(
                third.contains("as many requests wait as the pool's line holds"),
                "PostgreSQL {major}: {third}"
            );
            assert_eq!(
                project.cluster.sessions(&project.name),
                1,
                "one connection, whatever was asked"
            );
            operator
                .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
                .unwrap();
            first.join().unwrap().unwrap();
            // The second was lent the connection the first returned.
            second.join().unwrap().unwrap();
        });
        assert_eq!(n(&project), "1");
        // With the line empty, a borrower that waits its second out is told so: the deadline
        // is its own, and nothing else is in the line to race it.
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| client.request(&held()));
            until("the holder to wait on the lock", || {
                waiting(&project) == "1"
            });
            let late = refused(hasty.request(&[Step::bare("SELECT 1")]));
            assert!(
                late.contains("none came free within the wait for one"),
                "PostgreSQL {major}: {late}"
            );
            operator
                .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
                .unwrap();
            holder.join().unwrap().unwrap();
        });
        assert_eq!(n(&project), "2");
        assert_eq!(
            client.counts(),
            Counts {
                idle: 1,
                ..Counts::default()
            }
        );
    }
}

#[test]
fn a_request_given_up_on_is_cancelled_uncommitted_and_its_connection_is_not_lent_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        let client = project.client("web").bounded(Duration::from_millis(500));
        let operator = project.cluster.operator(&project.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        let lost = refused(client.request(&held()));
        assert!(
            lost.contains("the server did not answer in time")
                && lost.contains("nothing of the request was committed"),
            "PostgreSQL {major}: {lost}"
        );
        until("the cancelled session to end", || {
            project.cluster.sessions(&project.name) == 0
        });
        operator
            .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
            .unwrap();
        assert_eq!(n(&project), "0");
        assert_eq!(client.counts(), Counts::default());
        assert_eq!(client.requests(), 1, "sent once");
    }
}

/// A trigger that runs `body` when a transaction that updated `t` commits.
fn at_commit(project: &Project, body: &str) {
    project.cluster.operate(
        &project.name,
        &format!(
            "SET ROLE {}; CREATE FUNCTION at_commit() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN {body}; RETURN NULL; END $$; \
             CREATE CONSTRAINT TRIGGER at_commit AFTER UPDATE ON t DEFERRABLE INITIALLY DEFERRED \
             FOR EACH ROW EXECUTE FUNCTION at_commit();",
            project.name
        ),
    );
}

#[test]
fn a_commit_that_is_not_answered_is_unknown_and_is_never_sent_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        at_commit(&project, &format!("PERFORM pg_advisory_xact_lock({HELD})"));
        let operator = project.cluster.operator(&project.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        // The deadline is the whole request's, and only the commit waits on the lock. A
        // machine too loaded to reach the commit within it ends the request before the commit
        // was sent, which commits nothing and is another answer (the test above); the request
        // is then made again with twice the time, until one reaches its commit. What is
        // asserted is of that one alone, so no load decides the result.
        let mut within = Duration::from_millis(500);
        let (client, unknown) = loop {
            let client = project.client("web").bounded(within);
            let said = refused(
                client.request(&[Step::bare("UPDATE t SET n = n + 1"), Step::bare("SELECT 1")]),
            );
            assert_eq!(client.requests(), 1, "sent once, whatever became of it");
            if !said.contains("nothing of the request was committed") {
                break (client, said);
            }
            assert!(within < READY, "PostgreSQL {major}: {said}");
            until("the session given up on to end", || {
                project.cluster.sessions(&project.name) == 0
            });
            within *= 2;
        };
        assert!(
            unknown.contains("the server did not answer the commit in time")
                && unknown.contains("whether the request was committed is not known"),
            "PostgreSQL {major}: {unknown}"
        );
        until("the session to end", || {
            project.cluster.sessions(&project.name) == 0
        });
        operator
            .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
            .unwrap();
        assert_eq!(client.requests(), 1, "sent once, whatever became of it");
        assert_eq!(
            n(&project),
            "0",
            "this one the server cancelled; the client could not know"
        );
        assert_eq!(client.counts(), Counts::default());
    }
}

#[test]
fn a_session_the_server_ends_at_the_commit_is_unknown_and_is_never_sent_again() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        at_commit(&project, "PERFORM pg_terminate_backend(pg_backend_pid())");
        let client = project.client("web");
        let unknown = refused(
            client.request(&[Step::bare("UPDATE t SET n = n + 1"), Step::bare("SELECT 1")]),
        );
        assert!(
            unknown.contains("after the commit was sent")
                && unknown.contains("whether the request was committed is not known"),
            "PostgreSQL {major}: {unknown}"
        );
        assert_eq!(client.requests(), 1);
        assert_eq!(client.counts(), Counts::default());
        until("the session to end", || {
            project.cluster.sessions(&project.name) == 0
        });
    }
}

#[test]
fn the_deployments_budget_is_counted_in_the_database_across_processes_and_by_class() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        // Two processes of the deployment, each with a pool of its own: `worker` has one process
        // of two connections, so a second process of the class finds no slot left.
        let one =
            Client::connect(&settings(""), project.environment_at("localhost", "worker")).unwrap();
        let two =
            Client::connect(&settings(""), project.environment_at("127.0.0.1", "worker")).unwrap();
        let operator = project.cluster.operator(&project.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        std::thread::scope(|scope| {
            let busy: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| one.request(&held()[1..])))
                .collect();
            until("both of the first process's requests to wait", || {
                waiting(&project) == "2"
            });
            let over = refused(two.request(&[Step::bare("SELECT 1")]));
            assert!(
                over.contains("every connection the budget gives this process's class is in use across the deployment"),
                "PostgreSQL {major}: {over}"
            );
            until("the refused connection to end", || {
                project.cluster.sessions(&project.name) == 2
            });
            operator
                .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
                .unwrap();
            for one in busy {
                one.join().unwrap().unwrap();
            }
        });
        // Idle connections keep their slots; a pool shut down gives them back.
        refused(two.request(&[Step::bare("SELECT 1")]));
        assert!(one.shut_down(READY), "PostgreSQL {major}: the pool drains");
        until("the first process's sessions to end", || {
            project.cluster.sessions(&project.name) == 0
        });
        assert_eq!(Project::value(&two, "SELECT 1"), "1");
        let closed = refused(one.request(&[Step::bare("SELECT 1")]));
        assert!(closed.contains("the pool is shut down"), "{closed}");

        // Another class has slots of its own: `worker` full does not hold `web` out.
        let other = cluster::cluster(major).project();
        let worker =
            Client::connect(&settings(""), other.environment_at("localhost", "worker")).unwrap();
        let web = Client::connect(&settings(""), other.environment_at("127.0.0.1", "web")).unwrap();
        let operator = other.cluster.operator(&other.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        std::thread::scope(|scope| {
            let busy: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| worker.request(&held()[1..])))
                .collect();
            until("both workers to wait", || waiting(&other) == "2");
            assert_eq!(Project::value(&web, "SELECT 1"), "1", "PostgreSQL {major}");
            operator
                .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
                .unwrap();
            for one in busy {
                one.join().unwrap().unwrap();
            }
        });
    }
}

#[test]
fn the_servers_cap_for_the_login_is_a_refusal_of_capacity() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        table(&project);
        project.cluster.operate(
            "postgres",
            &format!("ALTER ROLE {} CONNECTION LIMIT 1", project.name),
        );
        let client = project.client("web");
        let operator = project.cluster.operator(&project.name);
        operator
            .simple(&format!("SELECT pg_advisory_lock({HELD})"))
            .unwrap();
        std::thread::scope(|scope| {
            let busy = scope.spawn(|| client.request(&held()));
            until("the first request to wait", || waiting(&project) == "1");
            let over = refused(client.request(&[Step::bare("SELECT 1")]));
            assert!(
                over.contains("the server has no connection left for the login"),
                "PostgreSQL {major}: {over}"
            );
            operator
                .simple(&format!("SELECT pg_advisory_unlock({HELD})"))
                .unwrap();
            busy.join().unwrap().unwrap();
        });
        assert_eq!(n(&project), "1");
    }
}

#[test]
fn a_rotated_secret_is_read_again_and_the_connections_made_with_the_old_one_are_retired() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let url = Arc::new(Mutex::new(project.url()));
        let (read, ca) = (Arc::clone(&url), project.cluster.ca.display().to_string());
        let client = Client::connect(
            &settings(""),
            Box::new(move |name| match name {
                "URL" => Some(read.lock().unwrap().clone()),
                "CA" => Some(ca.clone()),
                "CLASS" => Some("web".into()),
                _ => None,
            }),
        )
        .unwrap();
        let before = Project::value(&client, "SELECT pg_backend_pid()");
        assert_eq!(
            Project::value(&client, "SELECT pg_backend_pid()"),
            before,
            "kept while the secret is"
        );
        project.cluster.operate(
            "postgres",
            &format!("ALTER ROLE {} PASSWORD 'rotated-{major}'", project.name),
        );
        *url.lock().unwrap() = project.url().replace(
            &cluster::escaped(&project.password),
            &format!("rotated-{major}"),
        );
        let after = Project::value(&client, "SELECT pg_backend_pid()");
        assert_ne!(
            after, before,
            "PostgreSQL {major}: a connection made with the new secret"
        );
        until("the old connection to end", || {
            project.cluster.sessions(&project.name) == 1
        });
        // A URL that comes to name another board is refused: a process is never moved under.
        let elsewhere = cluster::cluster(major).project();
        *url.lock().unwrap() = elsewhere.url();
        let moved = refused(client.request(&[Step::bare("SELECT 1")]));
        assert!(
            moved.contains("now names another endpoint, database or login"),
            "PostgreSQL {major}: {moved}"
        );
        assert_eq!(elsewhere.cluster.sessions(&elsewhere.name), 0);
        // The old secret, were it still read, is refused by the server.
        let stale = Client::connect(
            &settings(""),
            environment(&[
                ("URL", project.url_at("127.0.0.1")),
                ("CA", project.cluster.ca.display().to_string()),
                ("CLASS", "web".into()),
            ]),
        )
        .unwrap();
        let said = refused(stale.request(&[Step::bare("SELECT 1")]));
        assert!(said.contains("the server refused the login"), "{said}");
    }
}
