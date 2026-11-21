//! What a hosted client refuses before any statement of a board's runs: a certificate it cannot
//! verify, a server that speaks no TLS or offers a weaker login than SCRAM bound to the channel,
//! a wrong password, and a login or database a board is not kept in. No refusal says a part of
//! the secret, the login or the address.

use crate::cluster::{self, Certificate, Loopback, MAJORS, Project, environment, settings};
use symphony_board::pg::Step;
use symphony_board::pg::hosted::Client;

/// What the first request of a client of `url`, trusting `ca`, is refused with.
fn refused_at(project: &Project, url: String, ca: &std::path::Path) -> String {
    let vars = [
        ("URL", url),
        ("CA", ca.display().to_string()),
        ("CLASS", "web".to_string()),
    ];
    let said = match Client::connect(&settings(""), environment(&vars)) {
        Err(refusal) => refusal.to_string(),
        Ok(client) => cluster::refused(client.request(&[Step::bare("SELECT 1")])),
    };
    let escaped = cluster::escaped(&project.password);
    for secret in [
        project.password.as_str(),
        &escaped,
        &project.name,
        "postgresql://",
        "127.0.0.1",
        "localhost",
    ] {
        assert!(!said.contains(secret), "a refusal says {secret}: {said}");
    }
    assert_eq!(
        project.cluster.sessions(&project.name),
        0,
        "no session was left: {said}"
    );
    said
}

fn refused(project: &Project) -> String {
    refused_at(project, project.url(), &project.cluster.ca)
}

#[test]
fn a_certificate_another_authority_signed_is_refused() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let said = refused_at(&project, project.url(), &project.cluster.other_ca);
        assert!(
            said.contains("not one the deployment's authority signed"),
            "PostgreSQL {major}: {said}"
        );
    }
}

#[test]
fn a_certificate_for_another_name_is_refused() {
    for major in MAJORS {
        let project = cluster::cluster_with(major, Certificate::OtherName).project();
        for host in ["localhost", "127.0.0.1"] {
            let said = refused_at(&project, project.url_at(host), &project.cluster.ca);
            assert!(
                said.contains("not for the name the URL gives"),
                "PostgreSQL {major} at {host}: {said}"
            );
        }
    }
}

#[test]
fn an_expired_certificate_is_refused() {
    for major in MAJORS {
        let project = cluster::cluster_with(major, Certificate::Expired).project();
        let said = refused(&project);
        assert!(
            said.contains("certificate has expired"),
            "PostgreSQL {major}: {said}"
        );
    }
}

#[test]
fn a_server_that_speaks_no_tls_is_refused_and_no_url_asks_for_less() {
    for major in MAJORS {
        let project = cluster::cluster_with(major, Certificate::None).project();
        let said = refused(&project);
        assert!(
            said.contains("did not speak TLS"),
            "PostgreSQL {major}: {said}"
        );
        for less in ["disable", "prefer"] {
            let said = refused_at(
                &project,
                format!("{}?sslmode={less}", project.url()),
                &project.cluster.ca,
            );
            assert!(
                said.contains("turns TLS off") || said.contains("did not speak TLS"),
                "PostgreSQL {major}, {less}: {said}"
            );
        }
    }
}

/// `localhost` names two addresses, in an order the machine chooses, and a server holds one: the
/// server's refusal is what is said whether the address nothing holds is asked before it or
/// after, never that the server cannot be reached.
#[test]
fn a_refusal_is_the_servers_own_whichever_of_the_names_addresses_it_holds() {
    for loopback in [Loopback::V4, Loopback::V6] {
        let project = cluster::cluster_on(18, Certificate::Good, loopback).project();
        let wrong = project.url().replace(
            &cluster::escaped(&project.password),
            "not-the-password-41414141",
        );
        let said = refused_at(&project, wrong, &project.cluster.ca);
        assert!(
            said.contains("the server refused the login"),
            "{loopback:?}: {said}"
        );
        let said = refused_at(&project, project.url(), &project.cluster.other_ca);
        assert!(
            said.contains("not one the deployment's authority signed"),
            "{loopback:?}: {said}"
        );
        // And the login is let in at the address the server holds.
        let client = Client::connect(&settings(""), project.environment("web")).unwrap();
        client
            .request(&[Step::bare("SELECT 1")])
            .unwrap_or_else(|error| panic!("{loopback:?}: {error:?}"));
    }
}

#[test]
fn a_wrong_password_is_refused_without_saying_either() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let wrong = project.url().replace(
            &cluster::escaped(&project.password),
            "not-the-password-41414141",
        );
        let said = refused_at(&project, wrong, &project.cluster.ca);
        assert!(
            said.contains("the server refused the login"),
            "PostgreSQL {major}: {said}"
        );
        assert!(!said.contains("41414141"), "{said}");
    }
}

#[test]
fn a_login_the_server_lets_in_by_less_than_bound_scram_is_refused() {
    for major in MAJORS {
        let cluster = cluster::cluster(major);
        // Each is let in by the method its name says (`cluster`'s `pg_hba.conf`).
        let plain = "NOSUPERUSER NOCREATEDB NOCREATEROLE";
        for name in ["weak_trust", "weak_md5", "weak_password"] {
            if cluster.value(
                "postgres",
                &format!("SELECT count(*) FROM pg_roles WHERE rolname = '{name}'"),
            ) == "0"
            {
                let project = cluster.project_of(name, plain, "ENCODING 'UTF8'");
                if name == "weak_md5" {
                    // Its password kept as MD5, so the server asks for MD5 and not SCRAM.
                    cluster.operate(
                        "postgres",
                        &format!(
                            "SET password_encryption = 'md5'; ALTER ROLE weak_md5 PASSWORD '{}'",
                            project.password
                        ),
                    );
                    assert!(
                        cluster
                            .value(
                                "postgres",
                                "SELECT rolpassword FROM pg_authid WHERE rolname = 'weak_md5'"
                            )
                            .starts_with("md5")
                    );
                }
                let said = refused(&project);
                assert!(
                    said.contains("did not offer SCRAM with channel binding"),
                    "PostgreSQL {major}, {name}: {said}"
                );
            }
        }
    }
}

#[test]
fn a_login_or_database_a_board_is_not_kept_in_is_refused() {
    for major in MAJORS {
        let cluster = cluster::cluster(major);
        let tag = format!("u{}_{major}", std::process::id());
        let utf8 = "ENCODING 'UTF8'";
        for (name, role, expected) in [
            (
                "super",
                "SUPERUSER",
                "superuser or may make databases or roles",
            ),
            (
                "createdb",
                "CREATEDB",
                "superuser or may make databases or roles",
            ),
            (
                "createrole",
                "CREATEROLE",
                "superuser or may make databases or roles",
            ),
            (
                "replication",
                "REPLICATION",
                "superuser or may make databases or roles",
            ),
            (
                "bypassrls",
                "BYPASSRLS",
                "superuser or may make databases or roles",
            ),
            (
                "member",
                "IN ROLE pg_read_all_data",
                "member of another role",
            ),
        ] {
            let project = cluster.project_of(&format!("{tag}_{name}"), role, utf8);
            let said = refused(&project);
            assert!(
                said.contains(expected),
                "PostgreSQL {major}, {name}: {said}"
            );
        }
        let latin = cluster.project_of(
            &format!("{tag}_latin"),
            "",
            "ENCODING 'LATIN1' LOCALE_PROVIDER libc LC_COLLATE 'C' LC_CTYPE 'C'",
        );
        let said = refused(&latin);
        assert!(said.contains("not UTF-8"), "PostgreSQL {major}: {said}");
        // A database another role owns, which the login may connect to.
        let guest = cluster.project_of(&format!("{tag}_guest"), "", utf8);
        cluster.operate(
            "postgres",
            &format!(
                "ALTER DATABASE {} OWNER TO {}",
                guest.name,
                symphony_board::pg::USER
            ),
        );
        cluster.operate(
            "postgres",
            &format!("GRANT CONNECT ON DATABASE {0} TO {0}", guest.name),
        );
        let said = refused(&guest);
        assert!(
            said.contains("not owned by the login"),
            "PostgreSQL {major}: {said}"
        );
        // A database that is not there: nothing here makes it.
        let gone = cluster.project();
        let said = refused_at(
            &gone,
            format!(
                "{}never_made",
                gone.url().strip_suffix(gone.name.as_str()).unwrap()
            ),
            &cluster.ca,
        );
        assert!(
            said.contains("the database is not there"),
            "PostgreSQL {major}: {said}"
        );
        assert_eq!(
            cluster.value(
                "postgres",
                "SELECT count(*) FROM pg_database WHERE datname = 'never_made'"
            ),
            "0"
        );
    }
}

#[test]
fn settings_without_a_budget_or_a_known_class_connect_nowhere() {
    let project = cluster::cluster(18).project();
    let unbudgeted: symphony_board::pg::external::Settings =
        toml::from_str("url_env = \"URL\"\nca_file_env = \"CA\"\nclass_env = \"CLASS\"\n").unwrap();
    let said = Client::connect(&unbudgeted, project.environment("web"))
        .err()
        .map(|refusal| refusal.to_string())
        .unwrap();
    assert!(said.contains("budget"), "{said}");
    let said = Client::connect(&settings(""), project.environment("cron"))
        .err()
        .map(|refusal| refusal.to_string())
        .unwrap();
    assert!(said.contains("a class the budget does not have"), "{said}");
    assert_eq!(project.cluster.sessions(&project.name), 0);
}
