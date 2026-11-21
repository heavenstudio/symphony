//! One connection to a hosted board: made over verified TLS with SCRAM and channel binding,
//! checked for the server, database and login it reached, given one of its class's slots, and
//! then lent to one request at a time ([`super::pool`]).
//!
//! A request of several statements is one explicit REPEATABLE READ transaction: BEGIN, each
//! statement in turn with its `jsonb` parameter, COMMIT, and its answers given only once the
//! COMMIT is answered. A statement the server refuses rolls the transaction back and leaves the
//! connection clean. A connection lost, or a deadline met, before COMMIT was sent committed
//! nothing; after it was sent, what happened is not known, and the error says so. Either way the
//! connection is not used again and nothing is sent a second time. A request of one statement,
//! which the store sends for a read alone, is that statement and its own transaction.

use super::{Ended, Refusal};
use crate::pg::external::Share;
use crate::pg::{Answer, RAISED};
use crate::rpc::DbError;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_postgres::config::Host;
use tokio_postgres::types::private::BytesMut;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type, to_sql_checked};
use tokio_postgres::{SimpleQueryMessage, error::SqlState};
use tokio_postgres_rustls::MakeRustlsConnect;

/// The server majors this build is proved on (docs/operations/analysis/hosted-postgresql.md).
pub const SUPPORTED: [u32; 2] = [16, 18];
/// How long a cancel request or a closing connection is waited for.
const ENDING: Duration = Duration::from_secs(2);

/// What is said of a connection nothing answered: no server there, or none in time.
const UNREACHED: &str = "the server cannot be reached";
const UNANSWERED: &str = "the server did not answer in time";

pub(super) struct Session {
    client: tokio_postgres::Client,
    driving: tokio::task::JoinHandle<()>,
    tls: MakeRustlsConnect,
    /// The server's process for this connection, as its log names it.
    pub(super) backend: i32,
    pub(super) major: u32,
    pub(super) generation: [u8; 32],
}

/// The variables as the store binds them: `jsonb`, in its text.
#[derive(Debug)]
struct Json<'a>(&'a str);

impl ToSql for Json<'_> {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        if *ty == Type::JSONB {
            out.extend_from_slice(&[1]);
        }
        out.extend_from_slice(self.0.as_bytes());
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::JSONB | Type::JSON | Type::TEXT)
    }

    to_sql_checked!();
}

/// A column as the socket client gives it: its text, or null.
struct Cell(Option<String>);

impl<'a> FromSql<'a> for Cell {
    fn from_sql(
        ty: &Type,
        raw: &'a [u8],
    ) -> Result<Cell, Box<dyn std::error::Error + Sync + Send>> {
        let text = |raw: &[u8]| Ok(std::str::from_utf8(raw)?.to_string());
        let fixed = |size: usize| {
            <[u8; 8]>::try_from([&[0; 8][size..], raw].concat())
                .map_err(|_| "a number of another size than its type's")
        };
        let said: Result<String, Box<dyn std::error::Error + Sync + Send>> = match *ty {
            Type::TEXT | Type::VARCHAR | Type::NAME | Type::BPCHAR | Type::UNKNOWN | Type::JSON => {
                text(raw)
            }
            Type::JSONB => text(raw.get(1..).ok_or("an empty jsonb")?),
            // What a function that returns nothing answers: no text.
            Type::VOID => Ok(String::new()),
            Type::BOOL => Ok(if raw == [1] { "t" } else { "f" }.into()),
            Type::INT8 => Ok(i64::from_be_bytes(fixed(8)?).to_string()),
            Type::INT4 | Type::OID => {
                let wide = fixed(4)?;
                let narrow = <[u8; 4]>::try_from(&wide[4..]).expect("four of eight");
                Ok(if *ty == Type::OID {
                    u32::from_be_bytes(narrow).to_string()
                } else {
                    i32::from_be_bytes(narrow).to_string()
                })
            }
            _ => {
                Err(format!("a column of type {ty}, which the hosted client does not read").into())
            }
        };
        said.map(|text| Cell(Some(text)))
    }

    fn from_sql_null(_: &Type) -> Result<Cell, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Cell(None))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

/// The key of `class`'s slots: the first of an advisory lock's two numbers.
pub(super) fn slot_key(class: &str) -> i32 {
    let digest = Sha256::digest(format!("symphony-board slot {class}"));
    i32::from_be_bytes(digest[..4].try_into().expect("four bytes"))
}

/// What the server said, with nothing of its own words: its SQLSTATE and where its log has it.
fn refused(error: &tokio_postgres::error::DbError, backend: i32) -> Refusal {
    Refusal::Server {
        state: error.code().code().to_string(),
        at: super::correlation(backend),
    }
}

/// A connecting error by its kind alone: the driver's and the server's own words can hold an
/// address, a name or a part of the secret.
fn unconnected(error: &tokio_postgres::Error) -> Refusal {
    if let Some(server) = error.as_db_error() {
        return match server.code().code() {
            "28P01" | "28000" => Refusal::Unavailable("the server refused the login"),
            "3D000" => Refusal::Unavailable(
                "the database is not there: the platform makes it, and nothing here does",
            ),
            "53300" => Refusal::Capacity("the server has no connection left for the login"),
            "42501" => Refusal::Unavailable("the login may not connect to the database"),
            _ => refused(server, 0),
        };
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        // The handshake's own error is what an I/O error carries, not its source.
        let carried = cause
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .and_then(|inner| inner.downcast_ref::<rustls::Error>());
        if let Some(tls) = cause.downcast_ref::<rustls::Error>().or(carried) {
            return Refusal::Unavailable(match tls {
                rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName)
                | rustls::Error::InvalidCertificate(
                    rustls::CertificateError::NotValidForNameContext { .. },
                ) => "the server's certificate is not for the name the URL gives",
                rustls::Error::InvalidCertificate(rustls::CertificateError::Expired)
                | rustls::Error::InvalidCertificate(rustls::CertificateError::ExpiredContext {
                    ..
                }) => "the server's certificate has expired",
                rustls::Error::InvalidCertificate(_) => {
                    "the server's certificate is not one the deployment's authority signed"
                }
                _ => "the TLS handshake failed",
            });
        }
        source = cause.source();
    }
    // The driver's kinds, by the fixed words each starts with.
    let said = error.to_string();
    Refusal::Unavailable(if said.starts_with("authentication error") {
        "the server did not offer SCRAM with channel binding, and nothing weaker is spoken"
    } else if said.starts_with("error performing TLS handshake") {
        "the server did not speak TLS, and nothing else is spoken"
    } else if said.starts_with("timeout") || said.starts_with("connection timed out") {
        UNANSWERED
    } else {
        UNREACHED
    })
}

/// A connection to the first of the host's addresses that takes one, and the task that drives
/// it. Each address is asked in turn, as the driver asks them, and when none takes one the
/// refusal is the one that tells most ([`telling`]): the driver's own walk says the last
/// address's alone, so a name with an address nothing listens on after the server's (`localhost`,
/// where the server holds one of its two) would say only that the server cannot be reached,
/// whatever the server answered.
async fn reached(
    config: &tokio_postgres::Config,
    tls: &MakeRustlsConnect,
) -> Result<(tokio_postgres::Client, tokio::task::JoinHandle<()>), Refusal> {
    let unreached = Refusal::Unavailable(UNREACHED);
    let (Some(Host::Tcp(host)), port) = (
        config.get_hosts().first(),
        config.get_ports().first().copied().unwrap_or(5432),
    ) else {
        return Err(unreached);
    };
    let Ok(addresses) = tokio::net::lookup_host((host.as_str(), port)).await else {
        return Err(unreached);
    };
    let mut refusals = Vec::new();
    for address in addresses {
        let mut at = config.clone();
        // The name stays the one the certificate is checked against.
        at.hostaddr(address.ip());
        match at.connect(tls.clone()).await {
            Ok((client, connection)) => {
                let driving = tokio::spawn(async move {
                    // Its end, well or not, is read from the client: closed.
                    let _ = connection.await;
                });
                return Ok((client, driving));
            }
            Err(error) => refusals.push(unconnected(&error)),
        }
    }
    Err(telling(refusals).unwrap_or(unreached))
}

/// Of the refusals a host's addresses gave, in turn, the first that says more than that nothing
/// answered there (the server's own refusal, or a connection this side refused); the last when
/// none does, as the driver says it; none of none.
fn telling(refusals: Vec<Refusal>) -> Option<Refusal> {
    let silent = |refusal: &Refusal| matches!(refusal, Refusal::Unavailable(why) if [UNREACHED, UNANSWERED].contains(why));
    let said = refusals.iter().position(|refusal| !silent(refusal));
    let at = said.or(refusals.len().checked_sub(1))?;
    refusals.into_iter().nth(at)
}

/// A statement's error as the store reads one ([`crate::pg::error_of`]): a conflict to decide
/// again, the statement's own raised words, or a refusal by its SQLSTATE.
fn failed(error: &tokio_postgres::error::DbError, backend: i32) -> DbError {
    let state = error.code();
    if *state == SqlState::T_R_SERIALIZATION_FAILURE || *state == SqlState::T_R_DEADLOCK_DETECTED {
        DbError::Conflict
    } else if state.code() == RAISED {
        DbError::Thrown(error.message().into())
    } else {
        DbError::Failed(refused(error, backend).to_string())
    }
}

/// Whether `error` ended its session: the server says so by its severity, and by the class of
/// an operator's intervention that ends one.
fn ending(error: &tokio_postgres::error::DbError) -> bool {
    use tokio_postgres::error::Severity;
    matches!(
        error.parsed_severity(),
        Some(Severity::Fatal | Severity::Panic)
    ) || [
        SqlState::ADMIN_SHUTDOWN,
        SqlState::CRASH_SHUTDOWN,
        SqlState::CANNOT_CONNECT_NOW,
    ]
    .contains(error.code())
}

/// The server's major version, when what the session `said` of itself (its version, encoding,
/// database, login, whether the login holds a right a board's may not, is a member of another
/// role, and owns the database) is what a board is kept in for `database` and `user`.
fn held(said: &[&str], database: &str, user: &str) -> Result<u32, Refusal> {
    let at = |column: usize| said.get(column).copied().unwrap_or("");
    let major = at(0).parse::<u32>().unwrap_or(0) / 10_000;
    if !SUPPORTED.contains(&major) {
        return Err(Refusal::Unsupported(format!(
            "the server is PostgreSQL {major}, and this build is proved on {} alone",
            SUPPORTED.map(|major| major.to_string()).join(" and ")
        )));
    }
    if at(1) != "UTF8" {
        return Err(Refusal::Unsupported("the database is not UTF-8".into()));
    }
    if at(2) != database || at(3) != user {
        return Err(Refusal::Unsupported(
            "the session is not in the database and login the URL names".into(),
        ));
    }
    if at(4) != "f" {
        return Err(Refusal::Unsupported(
            "the login is a superuser or may make databases or roles, replicate or pass \
             row security; a board's login may do none of these"
                .into(),
        ));
    }
    if at(5) != "f" {
        return Err(Refusal::Unsupported(
            "the login is a member of another role; a board's login is its own alone".into(),
        ));
    }
    if at(6) != "t" {
        return Err(Refusal::Unsupported(
            "the database is not owned by the login; a board's database is its login's own".into(),
        ));
    }
    Ok(major)
}

impl Session {
    /// A connection to `target`, checked and given one of `share`'s class's slots.
    pub(super) async fn connect(
        target: super::secret::Target,
        share: Share,
    ) -> Result<Session, Refusal> {
        let tls = MakeRustlsConnect::new(target.tls.clone());
        let (client, driving) = reached(&target.config, &tls).await?;
        let mut session = Session {
            client,
            driving,
            tls,
            backend: 0,
            major: 0,
            generation: target.generation,
        };
        if let Err(refusal) = session.checked(&target.database, &target.user).await {
            session.close().await;
            return Err(refusal);
        }
        match session.slotted(&share).await {
            Ok(()) => Ok(session),
            Err(refusal) => {
                session.close().await;
                Err(refusal)
            }
        }
    }

    /// The session's settings made the board's, and the server, database and login it reached
    /// held to what this build supports.
    async fn checked(&mut self, database: &str, user: &str) -> Result<(), Refusal> {
        const SQL: &str = "SET default_transaction_isolation = 'repeatable read'; \
            SET search_path = public; \
            SELECT current_setting('server_version_num'), current_setting('server_encoding'), \
            current_database(), session_user, \
            r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls, \
            EXISTS (SELECT 1 FROM pg_auth_members m WHERE m.member = r.oid), \
            (SELECT d.datdba = r.oid FROM pg_database d WHERE d.datname = current_database()), \
            pg_backend_pid() FROM pg_roles r WHERE r.rolname = session_user";
        let answered = self.client.simple_query(SQL).await.map_err(|error| {
            error
                .as_db_error()
                .map_or(Refusal::Unavailable(UNREACHED), |e| refused(e, 0))
        })?;
        let row = answered
            .iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            })
            .ok_or(Refusal::Unsupported(
                "the login has no role of its own".into(),
            ))?;
        let said: Vec<&str> = (0..8).map(|column| row.get(column).unwrap_or("")).collect();
        let major = held(&said, database, user)?;
        self.major = major;
        self.backend = said[7].parse().unwrap_or(0);
        Ok(())
    }

    /// One of the class's slots taken for the session's life: an advisory lock no other
    /// connection of the deployment holds, so the class's connections, counted in the database,
    /// never pass its budget whatever each process believes.
    async fn slotted(&self, share: &Share) -> Result<(), Refusal> {
        let key = slot_key(&share.class);
        for slot in 0..share.class_max {
            let sql = format!("SELECT pg_try_advisory_lock({key}, {slot})");
            let answered = self.client.simple_query(&sql).await.map_err(|error| {
                error
                    .as_db_error()
                    .map_or(Refusal::Unavailable(UNREACHED), |e| {
                        refused(e, self.backend)
                    })
            })?;
            let taken = answered.iter().any(|message| {
                matches!(message, SimpleQueryMessage::Row(row) if row.get(0) == Some("t"))
            });
            if taken {
                return Ok(());
            }
        }
        Err(Refusal::Capacity(
            "every connection the budget gives this process's class is in use across the \
             deployment",
        ))
    }

    pub(super) fn open(&self) -> bool {
        !self.client.is_closed()
    }

    /// The connection ended, and its end waited for a moment.
    pub(super) async fn close(self) {
        let Session {
            client,
            mut driving,
            ..
        } = self;
        drop(client);
        if tokio::time::timeout(ENDING, &mut driving).await.is_err() {
            driving.abort();
        }
    }

    /// Asks the server to stop what the session runs: a request given up on.
    async fn cancel(&self) {
        let token = self.client.cancel_token();
        let _ = tokio::time::timeout(ENDING, token.cancel_query(self.tls.clone())).await;
    }

    /// One statement's rows, `param` its `jsonb` when it takes one.
    async fn step(&self, sql: &str, param: Option<&str>) -> Result<Answer, tokio_postgres::Error> {
        let json = param.map(Json);
        let params: Vec<(&(dyn ToSql + Sync), Type)> = json
            .iter()
            .map(|json| (json as &(dyn ToSql + Sync), Type::JSONB))
            .collect();
        let rows = self.client.query_typed(sql, &params).await?;
        let mut answer = Answer::default();
        for row in rows {
            let mut columns = Vec::with_capacity(row.len());
            for column in 0..row.len() {
                columns.push(row.try_get::<_, Cell>(column)?.0);
            }
            answer.rows.push(columns);
        }
        Ok(answer)
    }

    /// `steps`' answers, all of them in one transaction ([`self`]), within `within`.
    pub(super) async fn request(
        &self,
        steps: Vec<(String, Option<String>)>,
        within: Duration,
    ) -> Result<Vec<Answer>, Ended> {
        let committing = Arc::new(AtomicBool::new(false));
        let sent = Arc::clone(&committing);
        let alone = steps.len() == 1;
        let work = async move {
            if alone {
                let (sql, param) = &steps[0];
                sent.store(crate::schema::mutates(sql), Ordering::SeqCst);
                return match self.step(sql, param.as_deref()).await {
                    Ok(answer) => Ok(vec![answer]),
                    Err(error) => Err(self.ended(&error, &sent, false).await),
                };
            }
            if let Err(error) = self
                .client
                .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
                .await
            {
                return Err(self.ended(&error, &sent, false).await);
            }
            let mut answers = Vec::with_capacity(steps.len());
            for (sql, param) in &steps {
                match self.step(sql, param.as_deref()).await {
                    Ok(answer) => answers.push(answer),
                    Err(error) => return Err(self.ended(&error, &sent, true).await),
                }
            }
            sent.store(true, Ordering::SeqCst);
            match self.client.batch_execute("COMMIT").await {
                Ok(()) => Ok(answers),
                // A COMMIT the server refuses rolled back: its error is the request's, and the
                // answers gathered before it are not given.
                Err(error) => Err(self.ended(&error, &sent, false).await),
            }
        };
        match tokio::time::timeout(within, work).await {
            Ok(done) => done,
            Err(_) => {
                self.cancel().await;
                Err(Ended::broken(if committing.load(Ordering::SeqCst) {
                    Refusal::Unknown("the server did not answer the commit in time")
                } else {
                    Refusal::Lost("the server did not answer in time")
                }))
            }
        }
    }

    /// What a failed statement ends its request with: the server's refusal, the transaction
    /// rolled back when one is `open`, the connection clean; or a connection that broke, which
    /// committed nothing unless the commit was `sent`.
    async fn ended(&self, error: &tokio_postgres::Error, sent: &AtomicBool, open: bool) -> Ended {
        let Some(server) = error.as_db_error() else {
            // A row the client could not read is the client's own failure on a connection
            // whose transaction is still open: it ends with the connection, uncommitted.
            if error.to_string().starts_with("error deserializing") {
                return Ended::broken(Refusal::Unsupported(
                    "a statement answered a column of a type the hosted client does not read"
                        .into(),
                ));
            }
            return Ended::broken(if sent.load(Ordering::SeqCst) {
                Refusal::Unknown("the connection was lost after the commit was sent")
            } else {
                Refusal::Lost("the connection was lost before any commit")
            });
        };
        if ending(server) {
            return Ended::broken(self.gone(sent.load(Ordering::SeqCst)));
        }
        let said = failed(server, self.backend);
        let clean = !open || self.client.batch_execute("ROLLBACK").await.is_ok();
        Ended { said, clean }
    }

    /// A session the server ended, an operator's act or a failover: nothing of its request was
    /// committed unless its commit was `sent`, in which case the server may have taken it.
    fn gone(&self, sent: bool) -> Refusal {
        if sent {
            Refusal::Unknown("the server ended the session after the commit was sent")
        } else {
            Refusal::Lost("the server ended the session before any commit")
        }
    }

    /// `sql`'s statements as one simple query, which says its own transactions: a migration.
    pub(super) async fn simple(&self, sql: String, within: Duration) -> Result<(), Ended> {
        let work = async {
            match self.client.batch_execute(&sql).await {
                Ok(()) => Ok(()),
                Err(error) => match error.as_db_error() {
                    Some(server) if ending(server) => Err(Ended::broken(Refusal::Unknown(
                        "the server ended the session while a script that commits ran",
                    ))),
                    Some(server) => {
                        let said = failed(server, self.backend);
                        // The script's own transaction, if it left one failed, is rolled back.
                        let clean = self.client.batch_execute("ROLLBACK").await.is_ok();
                        Err(Ended { said, clean })
                    }
                    None => Err(Ended::broken(Refusal::Unknown(
                        "the connection was lost while a script that commits ran",
                    ))),
                },
            }
        };
        match tokio::time::timeout(within, work).await {
            Ok(done) => done,
            Err(_) => {
                self.cancel().await;
                Err(Ended::broken(Refusal::Unknown(
                    "the server did not answer a script that commits in time",
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_held_to_the_majors_proved_and_to_a_login_and_database_of_its_own() {
        let good = ["180007", "UTF8", "board", "login", "f", "f", "t", "41"];
        assert_eq!(held(&good, "board", "login"), Ok(18));
        let with = |column: usize, value: &'static str| {
            let mut said = good;
            said[column] = value;
            held(&said, "board", "login").map_err(|refusal| refusal.to_string())
        };
        assert_eq!(with(0, "160016"), Ok(16));
        // Every other major, older, between and newer, and a version that is no number.
        for (version, major) in [
            ("150012", 15),
            ("170004", 17),
            ("190000", 19),
            ("90624", 9),
            ("", 0),
            ("eighteen", 0),
        ] {
            let refused = with(0, version).unwrap_err();
            assert!(
                refused.contains(&format!("the server is PostgreSQL {major}, "))
                    && refused.contains("proved on 16 and 18 alone"),
                "{version}: {refused}"
            );
        }
        for (column, value, why) in [
            (1, "LATIN1", "not UTF-8"),
            (1, "SQL_ASCII", "not UTF-8"),
            (2, "postgres", "not in the database and login the URL names"),
            (3, "another", "not in the database and login the URL names"),
            (4, "t", "superuser or may make databases or roles"),
            (4, "", "superuser or may make databases or roles"),
            (5, "t", "member of another role"),
            (6, "f", "not owned by the login"),
            (6, "", "not owned by the login"),
        ] {
            let refused = with(column, value).unwrap_err();
            assert!(refused.contains(why), "{column} {value}: {refused}");
        }
        // A row short of its columns is refused, never read as a login that may do nothing.
        assert!(held(&good[..4], "board", "login").is_err());
    }

    #[test]
    fn the_refusal_of_a_hosts_addresses_is_the_first_that_says_more_than_no_answer() {
        let (unreached, late) = (
            Refusal::Unavailable(UNREACHED),
            Refusal::Unavailable(UNANSWERED),
        );
        let login = Refusal::Unavailable("the server refused the login");
        let full = Refusal::Capacity("the server has no connection left for the login");
        assert_eq!(telling(vec![]), None);
        assert_eq!(telling(vec![unreached.clone()]), Some(unreached.clone()));
        // Whichever address the server holds, its refusal is the one said.
        for order in [
            vec![login.clone(), unreached.clone()],
            vec![unreached.clone(), login.clone()],
            vec![late.clone(), login.clone(), unreached.clone()],
        ] {
            assert_eq!(telling(order.clone()), Some(login.clone()), "{order:?}");
        }
        // The first of two that tell, and the last of those that do not, as the driver's walk.
        assert_eq!(
            telling(vec![full.clone(), login.clone()]),
            Some(full.clone())
        );
        assert_eq!(
            telling(vec![unreached.clone(), late.clone()]),
            Some(late.clone())
        );
        assert_eq!(telling(vec![late, unreached.clone()]), Some(unreached));
    }

    #[test]
    fn a_class_has_one_key_and_another_class_another() {
        assert_eq!(slot_key("web"), slot_key("web"));
        assert_ne!(slot_key("web"), slot_key("worker"));
        assert_ne!(slot_key("web"), slot_key(crate::pg::external::MAINTENANCE));
    }

    #[test]
    fn the_variables_go_as_jsonb_with_its_version_or_as_plain_text() {
        let mut out = BytesMut::new();
        Json("{\"a\":1}").to_sql(&Type::JSONB, &mut out).unwrap();
        assert_eq!(&out[..], b"\x01{\"a\":1}");
        let mut out = BytesMut::new();
        Json("{}").to_sql(&Type::TEXT, &mut out).unwrap();
        assert_eq!(&out[..], b"{}");
        assert!(Json::accepts(&Type::JSON) && !Json::accepts(&Type::INT4));
    }

    #[test]
    fn a_column_reads_as_the_text_the_socket_client_gives() {
        let read = |ty: &Type, raw: &[u8]| Cell::from_sql(ty, raw).map(|cell| cell.0.unwrap());
        assert_eq!(read(&Type::TEXT, "é".as_bytes()).unwrap(), "é");
        assert_eq!(read(&Type::JSONB, b"\x01[1]").unwrap(), "[1]");
        assert_eq!(read(&Type::JSON, b"[1]").unwrap(), "[1]");
        assert_eq!(read(&Type::BOOL, &[1]).unwrap(), "t");
        assert_eq!(read(&Type::BOOL, &[0]).unwrap(), "f");
        assert_eq!(read(&Type::INT8, &(-7i64).to_be_bytes()).unwrap(), "-7");
        assert_eq!(read(&Type::INT4, &(-7i32).to_be_bytes()).unwrap(), "-7");
        assert_eq!(
            read(&Type::OID, &u32::MAX.to_be_bytes()).unwrap(),
            "4294967295"
        );
        assert!(Cell::from_sql_null(&Type::TEXT).unwrap().0.is_none());
        assert!(read(&Type::JSONB, b"").is_err());
        assert!(read(&Type::INT8, &[1, 2]).is_err());
        assert!(read(&Type::TEXT, &[0xff]).is_err());
        let other = read(&Type::FLOAT8, &[0; 8]).unwrap_err().to_string();
        assert!(
            other.contains("float8, which the hosted client does not read"),
            "{other}"
        );
    }
}
