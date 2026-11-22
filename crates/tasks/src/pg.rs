//! The board's PostgreSQL client: the server's frontend/backend protocol 3.0 over its Unix socket
//! alone, where the server trusts the user that made it ([`crate::server::postgres`]), so a client
//! has no TCP, no TLS and no password, and speaks only the messages the store's requests need
//! ([`wire`]). No async runtime, no dependency: one connection, and a request the socket cannot
//! take at once written on a thread of its own while its answers are read
//! ([`Connection::exchange`]).
//!
//! A client makes its connection at its first request and keeps it for its life, so a command,
//! one process, is one connection. Its startup names the user [`USER`], its database, UTF-8, and
//! REPEATABLE READ for every transaction: a snapshot from a transaction's first statement, and
//! a write that meets a row another committed since then fails at once, which is the board's rule
//! of a write that met another (`lib.rs`), read and decided again.
//!
//! A request ([`Client::request`]) is its statements, each parsed, bound to its one parameter when
//! it takes one (a `jsonb`, the statement's variables) and run, then one Sync: they run in one
//! implicit transaction, committed at the Sync, or rolled back at the first error, after which the
//! server runs nothing until the Sync. Every value comes back as text. A simple query
//! ([`Client::simple`]) is what takes no parameter: the schema's definitions, a database made.
//!
//! An error is read by its SQLSTATE ([`error_of`]): a serialization failure or a deadlock is
//! [`DbError::Conflict`], to be tried again; a raised exception is [`DbError::Thrown`], its message
//! the words the statement raised; anything else is [`DbError::Failed`], in the server's words and
//! with its code. A connection that fails mid-request is dropped and never used again; a request
//! is never sent twice, a read no more than a write, so a client never repeats work the server may
//! have done.

use crate::db::DbError;
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub mod external;
#[cfg(feature = "hosted-postgresql")]
pub mod hosted;
mod wire;

#[cfg(test)]
mod tests;

/// The superuser a connection names: the one `initdb` made, trusted on the socket.
pub const USER: &str = crate::server::postgres::USER;
/// The database a connection that makes another names: PostgreSQL's own.
pub const MAINTENANCE: &str = "postgres";
/// How long a request may wait on the server, as the SurrealDB client's, to take each write of it
/// and to give each read of its answers: a whole board's copy, on a loaded machine, is the longest.
const TIMEOUT: Duration = Duration::from_secs(300);
/// SQLSTATEs a write is tried again on: `serialization_failure` and `deadlock_detected`.
const AGAIN: [&str; 2] = ["40001", "40P01"];
/// SQLSTATE of `raise_exception`, the code of a statement's own `RAISE EXCEPTION`.
pub(crate) const RAISED: &str = "P0001";
/// SQLSTATE of `invalid_catalog_name`: the database a connection names is not there.
const NO_DATABASE: &str = "3D000";
/// SQLSTATE of `duplicate_database`.
const DATABASE_THERE: &str = "42P04";

/// One statement of a request: its text, and its one parameter, when it takes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step<'a> {
    pub sql: &'a str,
    pub param: Option<&'a str>,
}

impl<'a> Step<'a> {
    /// `sql`, taking no parameter.
    pub fn bare(sql: &'a str) -> Step<'a> {
        Step { sql, param: None }
    }

    /// `sql`, its `$1` the `jsonb` `param`.
    pub fn with(sql: &'a str, param: &'a str) -> Step<'a> {
        Step {
            sql,
            param: Some(param),
        }
    }
}

/// What one statement gave: each row's columns, their text or null, and the command's tag.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Answer {
    pub rows: Vec<Vec<Option<String>>>,
    pub tag: String,
}

impl Answer {
    /// The first column of the first row: a statement that answers one value.
    pub fn value(&self) -> Option<&str> {
        self.rows.first()?.first()?.as_deref()
    }

    /// The first column of every row that gives one, in order: a statement that answers a list,
    /// an item a row.
    pub fn column(&self) -> Vec<&str> {
        let firsts = self.rows.iter().filter_map(|row| row.first()?.as_deref());
        firsts.collect()
    }
}

/// One database of the PostgreSQL whose socket is in the folder `socket`.
#[derive(Debug)]
pub struct Client {
    socket: PathBuf,
    database: String,
    timeout: Duration,
    requests: AtomicUsize,
    log: Arc<crate::board_health::QueryLog>,
    connection: Mutex<Option<Connection>>,
}

/// One connection: the socket its answers are read from, and the same socket a request is written
/// to ([`Connection::exchange`]).
#[derive(Debug)]
struct Connection {
    stream: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// `database` is a name this code chooses (letters, digits, `_`), never a caller's text.
    pub fn new(socket: &Path, database: &str) -> Client {
        assert!(
            crate::db::plain(database),
            "{database:?} is not a plain name for a database"
        );
        Client {
            socket: socket.into(),
            database: database.into(),
            timeout: TIMEOUT,
            requests: AtomicUsize::new(0),
            log: crate::board_health::queries_of(
                &format!("postgresql:{}", socket.display()),
                "",
                database,
            ),
            connection: Mutex::new(None),
        }
    }

    /// This client, waiting at most `timeout` for the server to take each write of a request and
    /// to give each read of its answers: a server that does neither in time ends the request in
    /// an error, and its connection.
    pub fn waiting(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// How long this client waits on the server ([`Client::waiting`]).
    pub fn wait(&self) -> Duration {
        self.timeout
    }

    /// A client of its own, with no connection yet, of the same database.
    pub fn another(&self) -> Client {
        Client::new(&self.socket, &self.database).waiting(self.timeout)
    }

    /// A client of its own of the same database that waits at most `most` for each answer, or
    /// this one's wait when that is shorter.
    pub fn bounded(&self, most: Duration) -> Client {
        self.another().waiting(self.timeout.min(most))
    }

    /// The board's request and write telemetry, shared by all its clients.
    pub fn queries(&self) -> Arc<crate::board_health::QueryLog> {
        Arc::clone(&self.log)
    }

    /// How many requests this client has sent: its round trips, for the tests that count them.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// A client of `database` on the same server.
    pub fn of(&self, database: &str) -> Client {
        Client::new(&self.socket, database).waiting(self.timeout)
    }

    /// The folder the server's socket is in.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    /// Where it reaches its board, as a command names it: `postgresql:` and the socket's folder.
    pub fn addr(&self) -> String {
        format!("postgresql:{}", self.socket.display())
    }

    /// Each of `steps`' answers, in order, all of them in one implicit transaction ([`Client`]).
    #[track_caller]
    pub fn request(&self, steps: &[Step]) -> Result<Vec<Answer>, DbError> {
        let mut out = Vec::new();
        for step in steps {
            wire::parse(&mut out, step.sql, step.param.is_some());
            wire::bind(&mut out, step.param);
            wire::execute(&mut out);
        }
        wire::sync(&mut out);
        self.measured(
            &out,
            steps.iter().any(|step| crate::schema::mutates(step.sql)),
        )
    }

    /// `sql`'s statements, which take no parameter, as one simple query: one implicit
    /// transaction unless they say otherwise; the answer of each that gave one.
    #[track_caller]
    pub fn simple(&self, sql: &str) -> Result<Vec<Answer>, DbError> {
        let mut out = Vec::new();
        wire::query(&mut out, sql);
        self.measured(&out, crate::schema::mutates(sql))
    }

    /// The database this client names, made on its server if it is not there: through a
    /// connection to [`MAINTENANCE`]. Another's making it first is its being there.
    pub fn make_database(&self) -> Result<(), DbError> {
        let maker = self.of(MAINTENANCE);
        match maker.simple(&format!("CREATE DATABASE {}", self.database)) {
            Ok(_) => Ok(()),
            Err(DbError::Failed(said)) if database_there(&said) => Ok(()),
            Err(other) => Err(other),
        }
    }

    /// Count each request once, including failures, without recording SQL or bound values.
    #[track_caller]
    fn measured(&self, out: &[u8], write: bool) -> Result<Vec<Answer>, DbError> {
        let began = Instant::now();
        let sent = self.exchange(out);
        self.log.record(crate::board_health::Query {
            at: crate::sweep::trigger::now(),
            ms: began.elapsed().as_secs_f64() * 1000.0,
            source: std::panic::Location::caller(),
            write,
            failed: sent.is_err(),
        });
        sent
    }

    /// `out` sent on this client's connection, made first when it has none, and every message the
    /// server answered with up to its next ReadyForQuery: what each statement gave, or the first
    /// error. A connection that failed is dropped.
    fn exchange(&self, out: &[u8]) -> Result<Vec<Answer>, DbError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let mut held = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            *held = Some(self.connect()?);
        }
        let connection = held.as_mut().expect("made above");
        match connection.exchange(out) {
            Ok(answered) => answered,
            Err(broken) => {
                *held = None;
                Err(DbError::Failed(format!(
                    "the board's PostgreSQL at {}: {broken}",
                    self.socket.display()
                )))
            }
        }
    }

    fn connect(&self) -> Result<Connection, DbError> {
        let path = self.socket.join(crate::server::postgres::SOCKET_FILE);
        let unreachable = |why: std::io::Error| {
            DbError::Failed(format!(
                "the board's PostgreSQL is not answering on {}: {why}",
                path.display()
            ))
        };
        let stream = UnixStream::connect(&path).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(unreachable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(unreachable)?;
        let writer = stream.try_clone().map_err(unreachable)?;
        let mut connection = Connection {
            stream: BufReader::new(stream),
            writer,
        };
        let startup = wire::startup(&[
            ("user", USER),
            ("database", &self.database),
            ("application_name", "symphony-board"),
            ("client_encoding", "UTF8"),
            ("default_transaction_isolation", "repeatable read"),
        ]);
        connection
            .started(&startup)
            .map_err(|failed| match failed {
                Started::Refused(error) => error,
                Started::Broken(why) => unreachable(why),
            })?;
        Ok(connection)
    }
}

impl Drop for Client {
    /// A connection held is ended as the protocol ends one, so the server's backend goes at once.
    fn drop(&mut self) {
        let held = self
            .connection
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(connection) = held.as_mut() {
            let mut out = Vec::new();
            wire::terminate(&mut out);
            let _ = connection.writer.write_all(&out);
        }
    }
}

/// Why a startup did not end ready: the server refused it, or the connection broke.
enum Started {
    Refused(DbError),
    Broken(std::io::Error),
}

impl Connection {
    /// The startup sent, and the server's answers read up to its first ReadyForQuery: refused
    /// unless it trusts the connection, since the client has no password to give.
    fn started(&mut self, startup: &[u8]) -> Result<(), Started> {
        self.writer.write_all(startup).map_err(Started::Broken)?;
        loop {
            let message = wire::read(&mut self.stream).map_err(Started::Broken)?;
            match message.kind {
                b'R' => {
                    let code = message
                        .body
                        .get(..4)
                        .map(|b| i32::from_be_bytes(b.try_into().expect("four bytes")));
                    if code != Some(0) {
                        return Err(Started::Refused(DbError::Failed(format!(
                            "the board's PostgreSQL asks for a password (authentication {}), and \
                             it is trusted on its socket alone: its data is not one the board made",
                            code.map_or("of no code".into(), |code| code.to_string())
                        ))));
                    }
                }
                b'E' => return Err(Started::Refused(error_of(&message.body))),
                b'Z' => return Ok(()),
                // ParameterStatus, BackendKeyData, a notice: nothing the board reads.
                b'S' | b'K' | b'N' => {}
                other => return Err(Started::Broken(unexpected(other, "a startup"))),
            }
        }
    }

    /// `out` sent, and the answers read up to the ReadyForQuery that ends them: the outer error is
    /// the connection's, the inner the server's answer. A transaction a simple query left failed
    /// is rolled back before the answer is given.
    ///
    /// Whatever the server says before it has read the whole request is read as it comes: an
    /// answer or an error at any length (one with the variables in it, a refusal that shows the
    /// row it refused) never leaves the server waiting on a client that waits on it, each with its
    /// socket full, as a client that writes the whole request before it reads can (libpq's
    /// pipeline mode says the same of its own). The socket takes what it can of the request at
    /// once ([`Connection::taken`]): a request it takes whole is sent before anything is read, and
    /// nothing the server says can stop it; the rest of one it does not is written on a thread of
    /// its own while this one reads. A server that takes nothing for the client's wait, or says
    /// nothing, ends the request in an error ([`Client::waiting`]).
    fn exchange(&mut self, out: &[u8]) -> std::io::Result<Result<Vec<Answer>, DbError>> {
        let (answers, status) = self.sent_and_read(out)?;
        if status == b'E' {
            let mut rollback = Vec::new();
            wire::query(&mut rollback, "ROLLBACK");
            let (_rolled_back, _) = self.sent_and_read(&rollback)?;
        }
        Ok(answers)
    }

    /// `out` sent and the answers read up to the next ReadyForQuery, and the transaction status it
    /// says ([`Connection::exchange`]): what the socket takes at once, then any rest written on a
    /// thread of its own while this one reads. The side that fails first ends the connection for
    /// both, so neither waits on the other, and its error is the one given: the other's is what
    /// that end did to it. A thread the system will not give fails the request.
    fn sent_and_read(&mut self, out: &[u8]) -> std::io::Result<(Result<Vec<Answer>, DbError>, u8)> {
        let taken = self.taken(out).map_err(sending)?;
        let out = &out[taken..];
        if out.is_empty() {
            return Connection::answers(&mut self.stream);
        }
        let ended = AtomicBool::new(false);
        // The connection ended for both sides, and whether this was the first to end it.
        let end = |stream: &UnixStream| {
            let first = !ended.swap(true, Ordering::SeqCst);
            let _ = stream.shutdown(Shutdown::Both);
            first
        };
        let Connection { stream, writer } = self;
        let writer: &UnixStream = writer;
        std::thread::scope(|scope| {
            let sending = std::thread::Builder::new()
                .name("board-pg-send".into())
                .spawn_scoped(scope, || {
                    let mut to = writer;
                    (to.write_all(out)).map_err(|why| (sending(why), end(writer)))
                })
                .map_err(|why| {
                    std::io::Error::new(why.kind(), format!("no thread to send a request: {why}"))
                })?;
            let read = Connection::answers(stream).map_err(|why| (why, end(stream.get_ref())));
            let sent = sending
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            given(read, sent)
        })
    }

    /// As much of `out` as the socket takes now, written without waiting: how many bytes that was.
    /// The socket waits again afterwards, also when the write failed.
    fn taken(&self, out: &[u8]) -> std::io::Result<usize> {
        self.writer.set_nonblocking(true)?;
        let mut to = &self.writer;
        let mut taken = 0;
        let wrote = loop {
            if taken == out.len() {
                break Ok(taken);
            }
            match to.write(&out[taken..]) {
                Ok(0) => break Err(std::io::ErrorKind::WriteZero.into()),
                Ok(more) => taken += more,
                Err(why) if why.kind() == std::io::ErrorKind::WouldBlock => break Ok(taken),
                Err(why) if why.kind() == std::io::ErrorKind::Interrupted => {}
                Err(why) => break Err(why),
            }
        };
        let waiting = self.writer.set_nonblocking(false);
        let taken = wrote?;
        waiting?;
        Ok(taken)
    }

    /// The server's messages on `stream` up to its next ReadyForQuery, and the transaction status
    /// it says.
    fn answers(stream: &mut impl BufRead) -> std::io::Result<(Result<Vec<Answer>, DbError>, u8)> {
        let mut answers = Vec::new();
        let mut current = Answer::default();
        let mut error = None;
        loop {
            let message = wire::read(stream)?;
            match message.kind {
                b'D' => {
                    let row = wire::row(&message.body).ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "a data row the client cannot read",
                        )
                    })?;
                    current.rows.push(row);
                }
                b'C' => {
                    current.tag = wire::text(&message.body);
                    answers.push(std::mem::take(&mut current));
                }
                b'I' => answers.push(std::mem::take(&mut current)),
                b'E' => {
                    error.get_or_insert_with(|| error_of(&message.body));
                }
                b'Z' => {
                    let status = message.body.first().copied().unwrap_or(b'I');
                    return Ok((error.map_or(Ok(answers), Err), status));
                }
                // ParseComplete, BindComplete, NoData, a row's description, a notice, a
                // parameter's new value: nothing the board reads.
                b'1' | b'2' | b'n' | b'T' | b't' | b'N' | b'S' | b'A' => {}
                other => return Err(unexpected(other, "an answer")),
            }
        }
    }
}

/// What a request's two sides came to ([`Connection::sent_and_read`]): the answers when both
/// ended well, else the error of the side that failed first, each error with whether its side was
/// the first to end the connection.
fn given<T>(
    read: Result<T, (std::io::Error, bool)>,
    sent: Result<(), (std::io::Error, bool)>,
) -> std::io::Result<T> {
    match (read, sent) {
        (Ok(read), Ok(())) => Ok(read),
        (Err((why, true)), _) | (_, Err((why, true))) => Err(why),
        (Err((why, false)), _) | (_, Err((why, false))) => Err(why),
    }
}

/// A write's failure, said as the sending of a request.
fn sending(why: std::io::Error) -> std::io::Error {
    std::io::Error::new(why.kind(), format!("sending a request: {why}"))
}

fn unexpected(kind: u8, during: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "a message of type {:?} in {during}, which the board's client never asks for",
            char::from(kind)
        ),
    )
}

/// The server's error, read by its SQLSTATE ([`Client`]).
pub(crate) fn error_of(body: &[u8]) -> DbError {
    let fields = wire::fields(body);
    let field = |code: u8| {
        fields
            .iter()
            .find(|(found, _)| *found == code)
            .map(|(_, text)| text.as_str())
    };
    let state = field(b'C').unwrap_or("");
    let message = field(b'M').unwrap_or("the server gave no message");
    if AGAIN.contains(&state) {
        DbError::Conflict
    } else if state == RAISED {
        DbError::Thrown(message.into())
    } else {
        let detail = field(b'D').map(|d| format!(": {d}")).unwrap_or_default();
        DbError::Failed(format!("{message}{detail} (SQLSTATE {state})"))
    }
}

/// Whether `said`, a [`DbError::Failed`]'s words, is the server's error of SQLSTATE `state`.
fn coded(said: &str, state: &str) -> bool {
    said.ends_with(&format!("(SQLSTATE {state})"))
}

/// Concurrent CREATE DATABASE can meet either on the database's name or on the catalogue's
/// unique index for that name. No other unique constraint means this database was made.
fn database_there(said: &str) -> bool {
    coded(said, DATABASE_THERE)
        || (coded(said, "23505") && said.contains("\"pg_database_datname_index\""))
}

/// Whether `said` is the server's refusal of a connection to a database it does not hold: a board
/// no one has opened.
pub fn no_database(said: &str) -> bool {
    coded(said, NO_DATABASE)
}
