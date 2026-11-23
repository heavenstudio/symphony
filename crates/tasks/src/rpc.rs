//! SurrealDB's RPC over plain HTTP: `POST /rpc` with method `query`, the statements and their
//! variables, answered with one result per statement. The variables travel beside the text, so nothing
//! a task says is ever spliced into a statement, and in CBOR ([`crate::cbor`]), since the RPC's JSON
//! takes a string that starts like a record id, a datetime or a UUID as one. A client keeps the
//! connections the server leaves open and sends its next requests on them, so a call is not also a
//! connect and a close; a transaction (`BEGIN; ...; COMMIT;`) lives inside one request.

use crate::cbor;
use crate::lock::Recovered as _;
use serde_json::{Value, json};
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod descriptor;
mod read;

pub(crate) use read::SLOW;
pub use read::{Kind, slow};

/// How long a request may wait on the server: a whole board's copy, on a loaded machine, is the longest.
/// Every request waits as long, a rule's and a read's too, but the heartbeat, which a hook waits on
/// ([`Client::waiting`]).
const TIMEOUT: Duration = Duration::from_secs(300);

/// How long a connection's check waits for data that should not come ([`check`]): a wait at all,
/// since only a wait meets a drained socket's refusal, and the shortest the socket takes.
const CHECK_WAIT: Duration = Duration::from_micros(1);

pub use crate::db::{DbError, plain};

/// One namespace and database on a SurrealDB server at `addr` (`127.0.0.1:PORT`).
#[derive(Debug)]
pub struct Client {
    addr: String,
    ns: String,
    db: String,
    requests: AtomicUsize,
    timeout: Duration,
    /// Whether a read that met its wait goes once more: not for a bounded client
    /// ([`Client::bounded`]), whose caller waits that long at most and leaves the server one
    /// sending of the work, not two.
    resend_late: bool,
    /// The wait of a read's second sending in a test, in place of `timeout`: its first sending runs
    /// a short wait out, and the answer to its second comes however late a loaded machine lets the
    /// test's server give it.
    #[cfg(test)]
    resend_timeout: Option<Duration>,
    /// Connections the server left open after its reply, for the next requests.
    kept: Mutex<Vec<Connection>>,
    /// Where each request is counted: the board's, shared by every client of it in this process.
    log: Arc<crate::board_health::QueryLog>,
}

#[derive(Debug)]
struct Connection {
    stream: TcpStream,
    descriptor: descriptor::Descriptor,
    /// Requests whose complete HTTP replies have been received on this socket.
    served: usize,
}

impl Connection {
    fn failure(&self, mut failure: Failure) -> Failure {
        if failure.bad_descriptor {
            failure.why.push_str("; ");
            failure.why.push_str(&self.descriptor.failure(&self.stream));
        }
        failure
    }

    /// A check's EBADF on a number that still holds this connection's socket, the same ends both
    /// ways: a drained socket ([`crate::sys::connect`]), its evidence. A number that no
    /// longer holds it was closed by code that does not own it: the failure, with what it holds now.
    fn drained(&self, mut failure: Failure) -> Result<String, Failure> {
        match self.descriptor.recheck(&self.stream) {
            Ok(same) => Ok(format!("{} answered {}; {same}", failure.step, failure.why)),
            Err(seen) => {
                failure.why.push_str("; ");
                failure.why.push_str(&seen);
                Err(failure)
            }
        }
    }
}

impl Client {
    /// `ns` and `db` are names this code chooses (letters, digits, `_`), never a caller's text.
    pub fn new(addr: &str, ns: &str, db: &str) -> Client {
        for name in [ns, db] {
            assert!(
                plain(name),
                "{name:?} is not a plain name for a namespace or database"
            );
        }
        Client {
            addr: addr.into(),
            ns: ns.into(),
            db: db.into(),
            requests: AtomicUsize::new(0),
            timeout: TIMEOUT,
            resend_late: true,
            #[cfg(test)]
            resend_timeout: None,
            kept: Mutex::new(Vec::new()),
            log: crate::board_health::queries_of(addr, ns, db),
        }
    }

    /// A client of the same board that waits at most `most` for each answer, or this one's wait
    /// when that is shorter, and sends a read that met it no second time: so a caller waits no
    /// longer than that on what it can do without, and the server is never handed work it could not
    /// finish in time twice. A read whose connection failed otherwise still goes once more. It keeps
    /// connections of its own, and is counted in the board's log as this one is.
    pub fn bounded(&self, most: Duration) -> Client {
        Client {
            addr: self.addr.clone(),
            ns: self.ns.clone(),
            db: self.db.clone(),
            requests: AtomicUsize::new(0),
            timeout: self.timeout.min(most),
            resend_late: false,
            #[cfg(test)]
            resend_timeout: self.resend_timeout,
            kept: Mutex::new(Vec::new()),
            log: Arc::clone(&self.log),
        }
    }

    /// What every client of this board in this process sent it ([`crate::board_health`]).
    pub fn queries(&self) -> Arc<crate::board_health::QueryLog> {
        Arc::clone(&self.log)
    }

    /// This client, waiting at most `timeout` for the server to connect and to answer each request
    /// whole. A read sent once more ([`Kind`]) waits as long again, no longer.
    pub fn waiting(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// Preserve a caller's request bound when adapting an address to another database engine.
    pub(crate) fn wait_limit(&self) -> Duration {
        self.timeout
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn database(&self) -> &str {
        &self.db
    }

    pub fn namespace(&self) -> &str {
        &self.ns
    }

    /// How many requests this client has made: a read's round trips, for the tests that count them.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// Each statement's result, in order. The first statement that failed decides the error; a
    /// conflict anywhere decides it first, since the whole transaction can then be retried.
    #[track_caller]
    pub fn query(&self, sql: &str, vars: Value) -> Result<Vec<Value>, DbError> {
        self.call(sql, vars, true)
    }

    /// The namespace and the database, made if they are not there yet, then `sql` in them, in one
    /// request: each statement's result, the four that make and enter them first.
    #[track_caller]
    pub fn define_then(&self, sql: &str, vars: Value) -> Result<Vec<Value>, DbError> {
        let (ns, db) = (&self.ns, &self.db);
        let sql = format!(
            "DEFINE NAMESPACE IF NOT EXISTS {ns}; USE NS {ns}; DEFINE DATABASE IF NOT EXISTS {db}; USE DB {db};\n{sql}"
        );
        self.call(&sql, vars, false)
    }

    /// `scoped`: in this client's namespace and database, which must exist.
    #[track_caller]
    pub(crate) fn call(&self, sql: &str, vars: Value, scoped: bool) -> Result<Vec<Value>, DbError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let body = cbor::encode(&json!({"id": 1, "method": "query", "params": [sql, vars]}));
        let scope = if scoped {
            format!("surreal-ns: {}\r\nsurreal-db: {}\r\n", self.ns, self.db)
        } else {
            String::new()
        };
        let mut request = format!(
            "POST /rpc HTTP/1.1\r\nHost: {}\r\nAccept: application/cbor\r\nContent-Type: application/cbor\r\n\
             Content-Length: {}\r\n{scope}\r\n",
            self.addr,
            body.len()
        )
        .into_bytes();
        request.extend(body);
        let kind = Kind::of(sql);
        let began = Instant::now();
        let sent = self.send(&request, kind);
        self.log.record(crate::board_health::Query {
            at: crate::sweep::trigger::now(),
            ms: began.elapsed().as_secs_f64() * 1000.0,
            source: std::panic::Location::caller(),
            write: kind == Kind::Write,
            failed: sent.is_err(),
        });
        let (status, reply) = sent.map_err(DbError::Failed)?;
        let reply = cbor::decode(&reply).map_err(|e| {
            DbError::Failed(format!(
                "HTTP {status} with a body that is not CBOR ({e}): {}",
                String::from_utf8_lossy(&reply)
            ))
        })?;
        results(reply)
    }
}

/// Failure diagnostics identify the query's source without logging SQL or its variables.
fn failed_query(source: &str, elapsed: Duration, timeout: Duration, error: &str) -> String {
    format!("RPC query at {source}, elapsed {elapsed:.3?}, configured wait {timeout:.3?}: {error}")
}

impl Client {
    /// `request` on a connection the server kept open, else on a new one: the reply's status and body.
    /// Each connection is checked ([`check`]) before anything is written on it, every kind of request
    /// alike: a kept one the server has closed, or that is drained, is dropped for another, and the
    /// request was not sent on it; a new one that is drained is dropped for one more new one, once.
    /// A request written on a connection that then gave back nothing, part
    /// of a reply, or nothing before the timeout is the request's failure and is never sent again,
    /// whatever it says: the server may have run it, and a write run twice is a message said twice.
    /// Only a request `kind` proves a read goes once more, on a new connection ([`Client::resend`]).
    /// A request that met its timeout on either sending names where it was asked, how long it took
    /// and the wait ([`failed_query`]).
    #[track_caller]
    fn send(&self, request: &[u8], kind: Kind) -> Result<(u16, Vec<u8>), String> {
        self.send_checked(request, kind, check)
    }

    #[track_caller]
    fn send_checked(
        &self,
        request: &[u8],
        kind: Kind,
        mut check: impl FnMut(&TcpStream) -> Result<(), Failure>,
    ) -> Result<(u16, Vec<u8>), String> {
        let began = Instant::now();
        let caller = std::panic::Location::caller();
        let kept = loop {
            let kept = self.kept.lock().recover().pop();
            let Some(connection) = kept else {
                break None;
            };
            match check(&connection.stream) {
                Ok(()) => break Some(connection),
                Err(failure) if !failure.bad_descriptor => {}
                Err(failure) => match connection.drained(failure) {
                    Ok(evidence) => eprintln!(
                        "the server at {}: a pooled socket after {} completed requests was drained, so it was dropped before the request was written: {evidence}",
                        self.addr, connection.served
                    ),
                    Err(failure) => {
                        return Err(self.unanswered(true, connection.served, failure));
                    }
                },
            }
        };
        let pooled = kept.is_some();
        let connection = match kept {
            Some(connection) => connection,
            None => self
                .fresh(&mut check)
                .map_err(|failure| self.unanswered(false, 0, failure))?,
        };
        let (timed_out, error) = match self.over(connection, request) {
            Ok(answer) => return Ok(answer),
            Err((served, failure))
                if kind == Kind::Read
                    && failure.written()
                    && (self.resend_late || !failure.timed_out) =>
            {
                match self.resend(request, &mut check, pooled, served, failure) {
                    Ok(answer) => return Ok(answer),
                    Err(failed) => failed,
                }
            }
            Err((served, failure)) => (failure.timed_out, self.unanswered(pooled, served, failure)),
        };
        Err(if timed_out {
            failed_query(
                &format!("{}:{}", caller.file(), caller.line()),
                began.elapsed(),
                self.timeout,
                &error,
            )
        } else {
            error
        })
    }

    /// `request` on `connection`: the reply, the connection kept when the server leaves it open; or
    /// why there was none, and how many requests the connection had carried.
    fn over(
        &self,
        connection: Connection,
        request: &[u8],
    ) -> Result<(u16, Vec<u8>), (usize, Failure)> {
        self.over_within(connection, request, self.timeout)
    }

    /// [`Client::over`], waiting `timeout`.
    fn over_within(
        &self,
        mut connection: Connection,
        request: &[u8],
        timeout: Duration,
    ) -> Result<(u16, Vec<u8>), (usize, Failure)> {
        match talk(&mut connection.stream, request, timeout) {
            Ok(reply) => {
                connection.served += 1;
                Ok(self.keep(connection, reply))
            }
            Err(failure) => Err((connection.served, connection.failure(failure))),
        }
    }

    /// A new connection that passed its check. One the check found drained is dropped, said on
    /// stderr, and replaced by one more new connection, once: if that one is drained too, the request
    /// fails unsent with both evidence lines. Any other failed check fails it unsent at once.
    fn fresh(
        &self,
        check: &mut impl FnMut(&TcpStream) -> Result<(), Failure>,
    ) -> Result<Connection, Failure> {
        let mut first: Option<String> = None;
        loop {
            let connection = self.connect()?;
            let Err(failure) = check(&connection.stream) else {
                return Ok(connection);
            };
            if !failure.bad_descriptor {
                return Err(failure);
            }
            let step = failure.step;
            match (connection.drained(failure), first) {
                (Ok(evidence), None) => {
                    eprintln!(
                        "the server at {}: a freshly opened socket was drained, so it was dropped before the request was written, which goes on one more new connection: {evidence}",
                        self.addr
                    );
                    first = Some(evidence);
                }
                (Ok(second), Some(first)) => {
                    let why = format!(
                        "a new connection and one more were both drained: [{first}] then [{second}]"
                    );
                    return Err(Failure::new(step, why));
                }
                (Err(failure), _) => return Err(failure),
            }
        }
    }

    /// A new connection to the server, its socket close-on-fork from the moment it exists
    /// ([`crate::sys::connect`]), read for its evidence on its own number.
    fn connect(&self) -> Result<Connection, Failure> {
        let connect_failed = |why| Failure::new(Step::Connect, why);
        let to = self
            .addr
            .to_socket_addrs()
            .map_err(|error| connect_failed(error.to_string()))?
            .next()
            .ok_or_else(|| connect_failed(format!("{} names no address", self.addr)))?;
        let stream = crate::sys::connect(&to, self.timeout)
            .map_err(|error| connect_failed(error.to_string()))?;
        let (descriptor, stream) = descriptor::Descriptor::connected(stream);
        Ok(Connection {
            descriptor,
            stream,
            served: 0,
        })
    }

    fn keep(&self, connection: Connection, reply: Reply) -> (u16, Vec<u8>) {
        if reply.open {
            self.kept.lock().recover().push(connection);
        }
        (reply.status, reply.body)
    }

    fn unanswered(&self, pooled: bool, served: usize, failure: Failure) -> String {
        let outcome = if failure.written() {
            "the request may have run there, so it was not sent again"
        } else {
            "the request was not sent"
        };
        format!(
            "the server at {} {}: {}; {outcome}",
            self.addr,
            self.verdict(&failure),
            failure.on(pooled, served)
        )
    }
}

/// A reply, and whether its connection is left open for another request.
struct Reply {
    status: u16,
    body: Vec<u8>,
    open: bool,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Connect,
    SocketOption(&'static str),
    Check(&'static str),
    Write,
    Read,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect => f.write_str("connect"),
            Self::SocketOption(name) => write!(f, "socket option {name}"),
            Self::Check(name) => write!(f, "connection check {name}"),
            Self::Write => f.write_str("write"),
            Self::Read => f.write_str("read"),
        }
    }
}

#[derive(Debug)]
struct Failure {
    step: Step,
    why: String,
    bad_descriptor: bool,
    timed_out: bool,
}

impl Failure {
    fn new(step: Step, why: impl ToString) -> Self {
        Self {
            step,
            why: why.to_string(),
            bad_descriptor: false,
            timed_out: false,
        }
    }

    fn io(step: Step, error: std::io::Error) -> Self {
        Self {
            step,
            bad_descriptor: descriptor::bad_descriptor(&error),
            timed_out: matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            why: error.to_string(),
        }
    }
}

/// Write `request` on `stream` and read one reply off it, by its length, its chunks, or, with neither, to
/// the end of the connection: the whole reply within `timeout` of the start. Each read waits only
/// what is left of it, so a reply that comes a few bytes at a time, each before a wait would run
/// out, is late all the same.
fn talk(stream: &mut TcpStream, request: &[u8], timeout: Duration) -> Result<Reply, Failure> {
    let deadline = Instant::now() + timeout;
    stream
        .set_nodelay(true)
        .map_err(|error| Failure::io(Step::SocketOption("TCP_NODELAY"), error))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| Failure::io(Step::SocketOption("SO_RCVTIMEO"), error))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| Failure::io(Step::SocketOption("SO_SNDTIMEO"), error))?;
    stream
        .write_all(request)
        .map_err(|error| Failure::io(Step::Write, error))?;
    let read_failed = |why| Failure::new(Step::Read, why);
    let mut raw = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let mut late = read_failed(format!(
                "the wait ran out with {} bytes of the reply in",
                raw.len()
            ));
            late.timed_out = true;
            return Err(late);
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|error| Failure::io(Step::Read, error))?;
        let n = stream
            .read(&mut chunk)
            .map_err(|error| Failure::io(Step::Read, error))?;
        if n == 0 {
            if raw.is_empty() {
                return Err(read_failed("it closed the connection".into()));
            }
            let (status, body) = parse_response(&raw).map_err(read_failed)?;
            return Ok(Reply {
                status,
                body,
                open: false,
            });
        }
        raw.extend_from_slice(&chunk[..n]);
        if let Some(reply) = whole(&raw).map_err(read_failed)? {
            return Ok(reply);
        }
    }
}

/// What [`check`] asks of a socket.
trait Probe {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()>;
    fn peek(&self, byte: &mut [u8]) -> std::io::Result<usize>;
}

impl Probe for TcpStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }

    fn peek(&self, byte: &mut [u8]) -> std::io::Result<usize> {
        TcpStream::peek(self, byte)
    }
}

/// Whether a connection may carry a request: nothing waiting on it, not closed by the server, and a
/// wait for data taken. A drained socket refuses any wait at once with EBADF, and only a wait sees
/// that, so the peek blocks, for [`CHECK_WAIT`]; a live idle socket answers that its time ran out.
/// An end, a reply that came unasked, or any other answer is a connection not to use.
fn check(stream: &impl Probe) -> Result<(), Failure> {
    stream
        .set_read_timeout(Some(CHECK_WAIT))
        .map_err(|error| Failure::io(Step::Check("SO_RCVTIMEO"), error))?;
    let peeked = |why: &str| Err(Failure::new(Step::Check("peek"), why));
    match stream.peek(&mut [0u8; 1]) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(Failure::io(Step::Check("peek"), error)),
        Ok(0) => peeked("it closed the connection"),
        Ok(_) => peeked("it sent a reply nobody asked for"),
    }
}

/// The reply `raw` holds, once it holds all of one that says how long it is; None while it is still coming
/// (or says nothing of its length, so ends with the connection).
fn whole(raw: &[u8]) -> Result<Option<Reply>, String> {
    let Some(head) = Head::read(raw)? else {
        return Ok(None);
    };
    let rest = &raw[head.end..];
    let body = if head.chunked {
        unchunked(rest)?
    } else if let Some(length) = head.length {
        rest.get(..length).map(<[u8]>::to_vec)
    } else {
        None
    };
    Ok(body.map(|body| Reply {
        status: head.status,
        body,
        open: !head.close && (head.chunked || head.length.is_some()),
    }))
}

/// Send `request` to `addr` on a connection of its own and read the response to its end. Its socket
/// is the client's kind ([`crate::sys::connect`]): a copy a spawn took during the exchange would
/// drain it, and its read's EBADF would fail a live server's answer (a health probe would say it
/// is not healthy).
pub(crate) fn exchange(
    addr: &str,
    request: &[u8],
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let cannot = |e: std::io::Error| format!("cannot reach the server at {addr}: {e}");
    let to = addr
        .to_socket_addrs()
        .map_err(cannot)?
        .next()
        .ok_or_else(|| format!("{addr} names no address"))?;
    let mut stream = crate::sys::connect(&to, timeout).map_err(cannot)?;
    let talk = |stream: &mut TcpStream| -> std::io::Result<Vec<u8>> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(request)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        Ok(raw)
    };
    let raw = talk(&mut stream).map_err(|e| format!("the server at {addr} did not answer: {e}"))?;
    parse_response(&raw)
}

/// Whether a SurrealDB server answers its health check at `addr`.
pub fn healthy(addr: &str) -> bool {
    let request = format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    matches!(
        exchange(addr, request.as_bytes(), Duration::from_secs(2)),
        Ok((200, _))
    )
}

/// An HTTP/1.1 response's head: its status, how its body is sent, and whether the server closes
/// the connection after it.
struct Head {
    status: u16,
    length: Option<usize>,
    chunked: bool,
    close: bool,
    /// Where the body starts.
    end: usize,
}

impl Head {
    /// The head `raw` starts with; None while it has not all come.
    fn read(raw: &[u8]) -> Result<Option<Head>, String> {
        let Some(end) = find(raw, b"\r\n\r\n") else {
            return Ok(None);
        };
        let head = String::from_utf8_lossy(&raw[..end]);
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let status = first
            .strip_prefix("HTTP/1.1 ")
            .or_else(|| first.strip_prefix("HTTP/1.0 "))
            .and_then(|rest| rest.get(..3)?.parse::<u16>().ok())
            .ok_or_else(|| format!("not an HTTP response: {head:?}"))?;
        let mut read = Head {
            status,
            length: None,
            chunked: false,
            close: first.starts_with("HTTP/1.0 "),
            end: end + 4,
        };
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            if name == "content-length" {
                read.length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("a Content-Length of {value:?}"))?,
                );
            } else if name == "transfer-encoding" {
                read.chunked = value.eq_ignore_ascii_case("chunked");
            } else if name == "connection" {
                read.close = value.eq_ignore_ascii_case("close");
            }
        }
        Ok(Some(read))
    }
}

/// The status and body of an HTTP/1.1 response read to its end: a `Content-Length` body or a chunked one.
fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let head = Head::read(raw)?.ok_or("the response ended inside its head")?;
    let rest = &raw[head.end..];
    let body = if head.chunked {
        unchunked(rest)?.ok_or("the body ended inside a chunk")?
    } else if let Some(length) = head.length {
        rest.get(..length)
            .ok_or_else(|| format!("the body ended at {} of its {length} bytes", rest.len()))?
            .to_vec()
    } else {
        rest.to_vec()
    };
    Ok((head.status, body))
}

/// A chunked body put back together; None while its last chunk has not come.
fn unchunked(mut rest: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut body = Vec::new();
    loop {
        let Some(line) = find(rest, b"\r\n") else {
            return Ok(None);
        };
        let size = String::from_utf8_lossy(&rest[..line]);
        let size = size.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size, 16).map_err(|_| format!("a chunk size of {size:?}"))?;
        rest = &rest[line + 2..];
        if size == 0 {
            return Ok(rest.starts_with(b"\r\n").then_some(body));
        }
        let Some(chunk) = rest.get(..size) else {
            return Ok(None);
        };
        body.extend_from_slice(chunk);
        let Some(after) = rest.get(size..size + 2) else {
            return Ok(None);
        };
        if after != b"\r\n" {
            return Err("a chunk did not end where its size said".into());
        }
        rest = &rest[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A JSON-RPC reply's statement results, or the error that decides it.
fn results(reply: Value) -> Result<Vec<Value>, DbError> {
    if let Some(error) = reply.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), String::from);
        return Err(DbError::Failed(message));
    }
    let Some(Value::Array(statements)) = reply.get("result") else {
        return Err(DbError::Failed(format!(
            "a reply with neither results nor an error: {reply}"
        )));
    };
    let kind = |s: &Value| {
        let detail = s
            .get("details")
            .and_then(|d| d.get("kind"))
            .and_then(Value::as_str);
        detail
            .or_else(|| s.get("kind").and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    let failed: Vec<&Value> = statements
        .iter()
        .filter(|s| s.get("status").and_then(Value::as_str) != Some("OK"))
        .collect();
    if failed.iter().any(|s| kind(s) == "TransactionConflict") {
        return Err(DbError::Conflict);
    }
    if let Some(first) = failed
        .iter()
        .find(|s| kind(s) != "NotExecuted")
        .or(failed.first())
    {
        let said = first
            .get("result")
            .and_then(Value::as_str)
            .map_or_else(|| first.to_string(), String::from);
        // A THROW in an event comes back as the event's error, the thrown words after its name:
        // thrown all the same, so a refusal an event throws is the write's refusal.
        let in_event = (said.strip_prefix("Error while processing event "))
            .and_then(|rest| rest.split_once(": An error occurred: "))
            .map(|(_, thrown)| thrown.to_string());
        return Err(match (kind(first).as_str(), in_event) {
            ("Thrown", _) => DbError::Thrown(
                said.strip_prefix("An error occurred: ")
                    .unwrap_or(&said)
                    .to_string(),
            ),
            (_, Some(thrown)) => DbError::Thrown(thrown),
            _ => DbError::Failed(said),
        });
    }
    Ok(statements
        .iter()
        .map(|s| s.get("result").cloned().unwrap_or(Value::Null))
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::children::Child as _;
    mod failures;
    mod resend;

    use super::*;
    use serde_json::json;

    /// The wait of a sending whose answer is to come in time, where a short wait is what a test is
    /// of, or where the wait is no part of what it tests: the wall-clock backstop of the 5 s those
    /// tests once gave, which no loaded machine runs out before the test's server answers.
    const PATIENT: Duration = test_time::backstop(Duration::from_secs(5));

    /// How long a test's socket read waits for what its peer writes or for the peer to close: the
    /// backstop of the 2 s these tests once gave it.
    const READ_PATIENCE: Duration = test_time::backstop(Duration::from_secs(2));

    fn ok(result: Value) -> Value {
        json!({"result": result, "status": "OK", "time": "1ms"})
    }

    fn not_executed() -> Value {
        json!({"details": {"kind": "NotExecuted"}, "kind": "Query", "status": "ERR",
               "result": "The query was not executed due to a failed transaction"})
    }

    #[test]
    fn every_statements_result_comes_back_in_order() {
        let reply = json!({"id": 1, "result": [ok(Value::Null), ok(json!([{"slug": "a"}])), ok(json!(42))]});
        assert_eq!(
            results(reply),
            Ok(vec![Value::Null, json!([{"slug": "a"}]), json!(42)])
        );
    }

    #[test]
    fn a_thrown_error_is_its_message() {
        let reply = json!({"id": 1, "result": [ok(Value::Null), not_executed(),
            {"kind": "Thrown", "result": "An error occurred: stale", "status": "ERR"}, not_executed()]});
        assert_eq!(results(reply), Err(DbError::Thrown("stale".into())));
    }

    /// A refusal an event throws is the write's, its words after the event's name.
    #[test]
    fn a_throw_in_an_event_is_thrown() {
        let said = "Error while processing event workflow_position: An error occurred: \
                    refused: workflow: sim/jump keeps its workflow position";
        let reply = json!({"result": [
            {"kind": "Internal", "result": said, "status": "ERR"}, not_executed()]});
        assert_eq!(
            results(reply),
            Err(DbError::Thrown(
                "refused: workflow: sim/jump keeps its workflow position".into()
            ))
        );
        let other = "Error while processing event board_changed: the server is full";
        let reply = json!({"result": [{"kind": "Internal", "result": other, "status": "ERR"}]});
        assert_eq!(results(reply), Err(DbError::Failed(other.into())));
    }

    #[test]
    fn a_conflict_decides_whatever_else_failed() {
        let reply = json!({"id": 1, "result": [ok(Value::Null), not_executed(),
            {"details": {"kind": "TransactionConflict"}, "kind": "Query", "status": "ERR",
             "result": "Cannot COMMIT: Transaction conflict: Write conflict, retry the transaction."}]});
        assert_eq!(results(reply), Err(DbError::Conflict));
    }

    #[test]
    fn any_other_error_is_the_servers_words() {
        let reply = json!({"id": 1, "result": [
            {"kind": "Internal", "status": "ERR", "result": "Found 'bogus' for field `status`"}]});
        assert_eq!(
            results(reply),
            Err(DbError::Failed("Found 'bogus' for field `status`".into()))
        );
    }

    #[test]
    fn a_reply_that_is_an_rpc_error_is_its_message() {
        let reply = json!({"id": 1, "error": {"code": -32000, "kind": "Validation", "message": "Parse error: x"}});
        assert_eq!(
            results(reply),
            Err(DbError::Failed("Parse error: x".into()))
        );
    }

    #[test]
    fn a_reply_with_neither_results_nor_an_error_is_refused() {
        assert!(matches!(results(json!({"id": 1})), Err(DbError::Failed(_))));
    }

    #[test]
    fn a_response_with_a_length_gives_its_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 4\r\n\r\n[1,2]extra";
        assert_eq!(parse_response(raw), Ok((200, b"[1,2".to_vec())));
    }

    #[test]
    fn a_chunked_response_is_put_back_together() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[1,\r\na;ext=1\r\n2]        \r\n0\r\n\r\n";
        assert_eq!(parse_response(raw), Ok((200, b"[1,2]        ".to_vec())));
    }

    #[test]
    fn a_response_read_to_its_close_has_the_rest_as_its_body() {
        assert_eq!(
            parse_response(b"HTTP/1.1 503 Service Unavailable\r\n\r\nbusy"),
            Ok((503, b"busy".to_vec()))
        );
    }

    #[test]
    fn a_header_line_without_a_colon_is_passed_over() {
        let raw = b"HTTP/1.1 200 OK\r\nnot a header\r\ncontent-length: 2\r\n\r\nok";
        assert_eq!(parse_response(raw), Ok((200, b"ok".to_vec())));
    }

    #[test]
    fn a_cut_off_response_is_refused() {
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\nshort").is_err());
        assert!(
            parse_response(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nab")
                .is_err()
        );
        assert!(parse_response(b"garbage\r\n\r\n").is_err());
    }

    #[test]
    fn a_name_that_is_not_a_plain_identifier_is_refused() {
        for bad in ["sf; REMOVE", "", "a-b", "t`x"] {
            assert!(
                std::panic::catch_unwind(|| Client::new("127.0.0.1:1", bad, "tasks")).is_err(),
                "{bad}"
            );
            assert!(
                std::panic::catch_unwind(|| Client::new("127.0.0.1:1", "sf", bad)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_server_that_is_not_there_is_a_failure_not_a_hang() {
        // Binding port zero assigns an ephemeral port; no server can listen on port zero itself.
        let port = 0;
        #[cfg(target_os = "macos")]
        let connect_error = libc::EADDRNOTAVAIL;
        #[cfg(not(target_os = "macos"))]
        let connect_error = libc::ECONNREFUSED;
        let client = Client::new(&format!("127.0.0.1:{port}"), "sf", "tasks");
        assert_eq!(
            client.query("RETURN 1;", json!({})),
            Err(DbError::Failed(format!(
                "the server at 127.0.0.1:{port} did not answer: connect failed on a freshly opened socket after 0 completed requests: {}; the request was not sent",
                std::io::Error::from_raw_os_error(connect_error)
            )))
        );
        assert!(!healthy(&format!("127.0.0.1:{port}")));
    }

    #[test]
    fn failure_messages_keep_the_operation_socket_history_and_original_error() {
        let client = Client::new("127.0.0.1:1234", "sf", "tasks");
        for (step, pooled, served, expected) in [
            (
                Step::Connect,
                false,
                0,
                "connect failed on a freshly opened socket after 0 completed requests: Bad file descriptor (os error 9); the request was not sent",
            ),
            (
                Step::SocketOption("TCP_NODELAY"),
                false,
                0,
                "socket option TCP_NODELAY failed on a freshly opened socket after 0 completed requests: Bad file descriptor (os error 9); the request was not sent",
            ),
            (
                Step::SocketOption("SO_RCVTIMEO"),
                true,
                2,
                "socket option SO_RCVTIMEO failed on a pooled socket after 2 completed requests: Bad file descriptor (os error 9); the request was not sent",
            ),
            (
                Step::SocketOption("SO_SNDTIMEO"),
                true,
                3,
                "socket option SO_SNDTIMEO failed on a pooled socket after 3 completed requests: Bad file descriptor (os error 9); the request was not sent",
            ),
            (
                Step::Write,
                false,
                0,
                "write failed on a freshly opened socket after 0 completed requests: Bad file descriptor (os error 9); the request may have run there, so it was not sent again",
            ),
            (
                Step::Read,
                true,
                4,
                "read failed on a pooled socket after 4 completed requests: Bad file descriptor (os error 9); the request may have run there, so it was not sent again",
            ),
        ] {
            assert_eq!(
                client.unanswered(
                    pooled,
                    served,
                    Failure::new(step, std::io::Error::from_raw_os_error(libc::EBADF))
                ),
                format!("the server at 127.0.0.1:1234 did not answer: {expected}")
            );
        }
    }

    #[test]
    fn only_bad_descriptor_failures_add_the_connected_socket_identity() {
        use std::os::fd::AsRawFd;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (descriptor, stream) = descriptor::Descriptor::connected(
            TcpStream::connect(listener.local_addr().unwrap()).unwrap(),
        );
        let (_peer, _) = listener.accept().unwrap();
        let connection = Connection {
            descriptor,
            stream,
            served: 15,
        };
        let failure = connection.failure(Failure::io(
            Step::Read,
            std::io::Error::from_raw_os_error(libc::EBADF),
        ));
        let text = Client::new("127.0.0.1:1234", "sf", "tasks").unanswered(true, 15, failure);
        assert!(
            text.contains("read failed on a pooled socket after 15 completed requests"),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "descriptor fd={} state=",
                connection.stream.as_raw_fd()
            )),
            "{text}"
        );
        #[cfg(target_os = "macos")]
        assert!(text.contains("state=open-identity-unproven"), "{text}");
        #[cfg(not(target_os = "macos"))]
        assert!(text.contains("state=still-open-same-identity"), "{text}");
        assert!(text.contains("at-connect=Open"), "{text}");
        assert!(text.contains("at-failure=Open"), "{text}");
        for failure in [
            Failure::io(
                Step::Read,
                std::io::Error::from_raw_os_error(libc::ETIMEDOUT),
            ),
            Failure::new(Step::Read, "cut off response"),
        ] {
            let original = failure.why.clone();
            assert_eq!(connection.failure(failure).why, original);
        }
    }

    /// A kept connection to `listener`'s server that has carried `served` requests, and the server's
    /// end of it.
    fn kept_connection(
        listener: &std::net::TcpListener,
        served: usize,
    ) -> (Connection, std::net::TcpStream) {
        let (descriptor, stream) = descriptor::Descriptor::connected(
            TcpStream::connect(listener.local_addr().unwrap()).unwrap(),
        );
        let (peer, _) = listener.accept().unwrap();
        let connection = Connection {
            descriptor,
            stream,
            served,
        };
        (connection, peer)
    }

    fn bad_descriptor_at_peek() -> Failure {
        Failure::io(
            Step::Check("peek"),
            std::io::Error::from_raw_os_error(libc::EBADF),
        )
    }

    /// The split of a check's EBADF, drained side: the number still holds the kept socket, so it
    /// was drained, and the request goes once on a new connection, nothing written on the drained one.
    #[test]
    fn a_drained_kept_connection_is_dropped_and_the_request_goes_once_on_a_new_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        let (drained, mut kept_peer) = kept_connection(&listener, 14);
        client.kept.lock().unwrap().push(drained);
        let served = std::thread::spawn(move || {
            let (mut fresh, _) = listener.accept().unwrap();
            read_request(&mut fresh);
            fresh.write_all(SEVEN).unwrap();
            fresh
        });
        let mut checks = 0;
        let answer = client.send_checked(
            b"POST /rpc HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            Kind::Write,
            |_| {
                checks += 1;
                if checks == 1 {
                    Err(bad_descriptor_at_peek())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(answer.map(|(status, _)| status), Ok(200));
        assert_eq!(checks, 2, "the drained one, then the new one");
        let _fresh = served.join().unwrap();
        assert!(
            closed_unwritten(&mut kept_peer),
            "nothing on the drained socket, which is closed"
        );
        let kept = client.kept.lock().unwrap();
        assert_eq!(kept.len(), 1, "only the new connection is kept");
        assert_eq!(kept[0].served, 1);
    }

    /// The split of a check's EBADF, the other side: the number no longer holds the socket it
    /// connected (here it holds another one), so code that does not own it closed it. The request fails
    /// with that evidence, unsent, and no other connection is made.
    #[test]
    fn a_kept_number_that_no_longer_holds_its_socket_fails_the_request_unsent_with_the_evidence() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client =
            Client::new(&address.to_string(), "sf", "tasks").waiting(Duration::from_millis(20));
        let (mut connection, mut peer) = kept_connection(&listener, 15);
        let (other, _other_peer) = kept_connection(&listener, 0);
        connection.descriptor = other.descriptor;
        client.kept.lock().unwrap().push(connection);
        let mut checks = 0;
        let error = client
            .send_checked(b"must not be sent", Kind::Write, |_| {
                checks += 1;
                Err(bad_descriptor_at_peek())
            })
            .unwrap_err();
        assert_eq!(checks, 1);
        assert!(error.contains("connection check peek failed on a pooled socket after 15 completed requests: Bad file descriptor"), "{error}");
        assert!(error.contains("state=replaced"), "{error}");
        assert!(error.contains("at-connect=Open"), "{error}");
        assert!(error.contains("at-failure=Open"), "{error}");
        assert!(error.ends_with("the request was not sent"), "{error}");
        assert!(closed_unwritten(&mut peer), "no request on the kept socket");
        assert!(none_waiting(&listener), "no replacement connection");
    }

    /// Whether `listener` has no connection waiting: none was made after the ones accepted.
    fn none_waiting(listener: &std::net::TcpListener) -> bool {
        listener.set_nonblocking(true).unwrap();
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    }

    /// Whether the server's end `peer` was closed with nothing written on it: the read waits for
    /// either, up to [`READ_PATIENCE`].
    fn closed_unwritten(peer: &mut std::net::TcpStream) -> bool {
        peer.set_read_timeout(Some(READ_PATIENCE)).unwrap();
        let read = peer.read(&mut [0]);
        let read = read.unwrap_or_else(|error| panic!("neither closed nor written: {error}"));
        read == 0
    }

    /// A fresh connection is checked before its first request too, and one the check finds drained
    /// is dropped like a kept one: the request goes once on one more new connection.
    #[test]
    fn a_drained_fresh_connection_is_dropped_and_the_request_goes_once_on_one_more() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        let served = std::thread::spawn(move || {
            let (drained, _) = listener.accept().unwrap();
            let (mut more, _) = listener.accept().unwrap();
            read_request(&mut more);
            more.write_all(SEVEN).unwrap();
            (drained, more, listener)
        });
        let mut checks = 0;
        let answer = client.send_checked(
            b"POST /rpc HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            Kind::Write,
            |_| {
                checks += 1;
                if checks == 1 {
                    Err(bad_descriptor_at_peek())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(answer.map(|(status, _)| status), Ok(200));
        assert_eq!(checks, 2, "the drained one, then one more");
        let (mut drained, _more, listener) = served.join().unwrap();
        assert!(closed_unwritten(&mut drained));
        assert!(none_waiting(&listener), "no third connection");
        let kept = client.kept.lock().unwrap();
        assert_eq!(kept.len(), 1, "only the one more is kept");
        assert_eq!(kept[0].served, 1);
    }

    /// One more new connection, once: if it is drained too, the request fails unsent with both
    /// evidence lines, each saying its number still held its socket, and no third one is made.
    #[test]
    fn a_second_drained_fresh_connection_fails_the_request_unsent_with_both_evidence_lines() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        let mut checks = 0;
        let error = client
            .send_checked(b"must not be sent", Kind::Write, |_| {
                checks += 1;
                Err(bad_descriptor_at_peek())
            })
            .unwrap_err();
        assert_eq!(checks, 2);
        let ebadf = std::io::Error::from_raw_os_error(libc::EBADF).to_string();
        let lead = format!(
            "the server at {addr} did not answer: connection check peek failed on a freshly opened socket after 0 completed requests: a new connection and one more were both drained: [connection check peek answered {ebadf}; descriptor fd="
        );
        assert!(error.starts_with(&lead), "{error}");
        let held = "still holds its socket, both ends as at connect: local 127.0.0.1:";
        assert_eq!(error.matches(held).count(), 2, "{error}");
        assert!(
            error.contains(&format!(
                "] then [connection check peek answered {ebadf}; descriptor fd="
            )),
            "{error}"
        );
        assert!(error.ends_with("]; the request was not sent"), "{error}");
        for _ in 0..2 {
            let (mut peer, _) = listener.accept().unwrap();
            assert!(closed_unwritten(&mut peer));
        }
        assert!(none_waiting(&listener), "no third connection");
        assert!(client.kept.lock().unwrap().is_empty());
    }

    /// A fresh connection whose check fails without EBADF fails the request unsent at once, with
    /// no other connection made.
    #[test]
    fn a_fresh_connection_whose_check_fails_otherwise_fails_the_request_unsent_at_once() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        let mut checks = 0;
        let error = client
            .send_checked(b"must not be sent", Kind::Write, |_| {
                checks += 1;
                Err(Failure::new(
                    Step::Check("peek"),
                    "it closed the connection",
                ))
            })
            .unwrap_err();
        assert_eq!(checks, 1);
        assert_eq!(
            error,
            format!(
                "the server at {addr} did not answer: connection check peek failed on a freshly opened socket after 0 completed requests: it closed the connection; the request was not sent"
            )
        );
        let (mut peer, _) = listener.accept().unwrap();
        assert!(closed_unwritten(&mut peer));
        assert!(none_waiting(&listener), "no other connection");
    }

    /// A fresh connection whose number, at its check's EBADF, holds another socket was closed by
    /// code that does not own it: the request fails unsent at once with that evidence, no other
    /// connection made. The number is pointed at another socket in one step, never left free.
    #[test]
    fn a_fresh_number_that_no_longer_holds_its_socket_fails_the_request_unsent_at_once() {
        use std::os::fd::AsRawFd;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let other = TcpStream::connect(elsewhere.local_addr().unwrap()).unwrap();
        let mut checks = 0;
        let error = client
            .send_checked(b"must not be sent", Kind::Write, |stream| {
                checks += 1;
                // SAFETY: both numbers belong to streams alive for this whole call; dup2 makes the
                // client's number hold the other socket in one step, so it is never free to reuse.
                let swapped = unsafe { libc::dup2(other.as_raw_fd(), stream.as_raw_fd()) };
                assert_eq!(swapped, stream.as_raw_fd());
                Err(bad_descriptor_at_peek())
            })
            .unwrap_err();
        assert_eq!(checks, 1);
        assert!(error.starts_with(&format!("the server at {addr} did not answer: connection check peek failed on a freshly opened socket after 0 completed requests: Bad file descriptor")), "{error}");
        assert!(error.contains("state=replaced"), "{error}");
        assert!(error.ends_with("the request was not sent"), "{error}");
        let (mut peer, _) = listener.accept().unwrap();
        assert!(
            closed_unwritten(&mut peer),
            "its own socket closed unwritten"
        );
        assert!(none_waiting(&listener), "no other connection");
    }

    /// A child forked while a board connection is kept holds no copy of it, even before its exec, as
    /// its own descriptor table says; std's socket, as a control, reaches one.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_child_forked_beside_a_kept_connection_holds_no_copy_of_it() {
        use std::os::fd::{AsRawFd, RawFd};
        use std::os::unix::process::CommandExt;
        let found = |fd: RawFd| {
            let mut command = std::process::Command::of("/usr/bin/true");
            // SAFETY: fcntl is async-signal-safe and reads only the forked child's own table.
            unsafe {
                command.pre_exec(move || {
                    if libc::fcntl(fd, libc::F_GETFD) == -1 {
                        Ok(())
                    } else {
                        Err(std::io::Error::from_raw_os_error(libc::EEXIST))
                    }
                });
            }
            let _spawning = crate::sys::spawning();
            match command.status() {
                Ok(status) => {
                    assert!(status.success(), "{status}");
                    false
                }
                Err(e) if e.raw_os_error() == Some(libc::EEXIST) => true,
                Err(e) => panic!("{e}"),
            }
        };
        let (addr, served) = serving(vec![(vec![SEVEN], false)]);
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        sevens(&client, 1);
        let kept = client.kept.lock().unwrap()[0].stream.as_raw_fd();
        assert!(!found(kept), "a forked child holds the kept connection");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (plain, _peer) = kept_connection(&listener, 0);
        assert!(found(plain.stream.as_raw_fd()), "std's socket reaches it");
        drop(client);
        assert_eq!(served.join().unwrap(), vec![1]);
    }

    /// Each answer the check can get, in order, through the socket trait: only a wait that ran out
    /// passes; an EBADF anywhere is a bad descriptor for the split; anything else drops the connection.
    #[test]
    fn the_check_passes_only_a_wait_that_ran_out_and_marks_a_bad_descriptor() {
        use std::cell::RefCell;
        use std::io::{Error, ErrorKind};

        struct Fake {
            timeout: Option<i32>,
            peek: fn() -> std::io::Result<usize>,
            seen: RefCell<Vec<String>>,
        }

        impl Probe for Fake {
            fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
                self.seen.borrow_mut().push(format!("timeout {timeout:?}"));
                self.timeout
                    .map_or(Ok(()), |code| Err(Error::from_raw_os_error(code)))
            }

            fn peek(&self, byte: &mut [u8]) -> std::io::Result<usize> {
                self.seen.borrow_mut().push(format!("peek {}", byte.len()));
                (self.peek)()
            }
        }

        let both = vec!["timeout Some(1µs)".to_string(), "peek 1".into()];
        let passed = |peek| {
            let fake = Fake {
                timeout: None,
                peek,
                seen: RefCell::default(),
            };
            let checked = check(&fake);
            assert_eq!(*fake.seen.borrow(), both);
            checked
        };
        assert!(passed(|| Err(ErrorKind::WouldBlock.into())).is_ok());
        assert!(passed(|| Err(ErrorKind::TimedOut.into())).is_ok());
        for (peek, why, bad) in [
            (
                (|| Err(Error::from_raw_os_error(libc::EBADF))) as fn() -> _,
                Error::from_raw_os_error(libc::EBADF).to_string(),
                true,
            ),
            (
                || Err(Error::from_raw_os_error(libc::ENOTCONN)),
                Error::from_raw_os_error(libc::ENOTCONN).to_string(),
                false,
            ),
            (|| Ok(0), "it closed the connection".into(), false),
            (|| Ok(1), "it sent a reply nobody asked for".into(), false),
        ] {
            let failure = passed(peek).unwrap_err();
            assert!(matches!(failure.step, Step::Check("peek")));
            assert_eq!((failure.why, failure.bad_descriptor), (why, bad));
        }
        for (code, bad) in [(libc::EBADF, true), (libc::EINVAL, false)] {
            let fake = Fake {
                timeout: Some(code),
                peek: || Ok(0),
                seen: RefCell::default(),
            };
            let failure = check(&fake).unwrap_err();
            assert_eq!(*fake.seen.borrow(), both[..1]);
            assert!(matches!(failure.step, Step::Check("SO_RCVTIMEO")));
            assert_eq!(failure.bad_descriptor, bad);
        }
    }

    /// The check on real sockets: a live idle one passes after its short wait, one the server
    /// closed or spoke on does not, and none of them loses the byte that came.
    #[test]
    fn the_check_on_real_sockets_passes_only_an_idle_live_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (idle, _idle_peer) = kept_connection(&listener, 0);
        assert!(check(&idle.stream).is_ok());
        assert!(
            check(&idle.stream).is_ok(),
            "a check leaves it fit for the next"
        );
        let (spoken, mut spoken_peer) = kept_connection(&listener, 0);
        spoken_peer.write_all(b"x").unwrap();
        spoken.stream.set_read_timeout(Some(READ_PATIENCE)).unwrap();
        assert_eq!(spoken.stream.peek(&mut [0]).unwrap(), 1);
        let failure = check(&spoken.stream).unwrap_err();
        assert_eq!(failure.why, "it sent a reply nobody asked for");
        let (closed, closed_peer) = kept_connection(&listener, 0);
        drop(closed_peer);
        closed.stream.set_read_timeout(Some(READ_PATIENCE)).unwrap();
        assert_eq!(closed.stream.peek(&mut [0]).unwrap(), 0);
        let failure = check(&closed.stream).unwrap_err();
        assert_eq!(failure.why, "it closed the connection");
        assert!(!failure.bad_descriptor);
    }

    #[test]
    fn each_error_says_what_it_is() {
        assert_eq!(
            DbError::Conflict.to_string(),
            "the transaction met another's write"
        );
        assert_eq!(DbError::Thrown("stale".into()).to_string(), "stale");
        assert_eq!(
            DbError::Failed("Parse error".into()).to_string(),
            "Parse error"
        );
    }

    /// A server that takes one request, keeps it, and gives `reply`.
    fn one_reply(reply: &'static [u8]) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let Some(end) = find(&request, b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length: usize = head
                    .split("content-length: ")
                    .nth(1)
                    .unwrap()
                    .split("\r\n")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            stream.write_all(reply).unwrap();
            request
        });
        (addr, served)
    }

    /// One request read off `stream` to the end of its body.
    fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0, "the client closed before its request ended");
            request.extend_from_slice(&chunk[..n]);
            let Some(end) = find(&request, b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
            let length: usize = head
                .split("content-length: ")
                .nth(1)
                .and_then(|rest| rest.split("\r\n").next())
                .map_or(0, |n| n.parse().unwrap());
            if request.len() >= end + 4 + length {
                return request;
            }
        }
    }

    /// A server that takes `connections` one after another, answering on each its replies, one per
    /// request, then closing it (`true`) or holding it open until every connection was served: how
    /// many requests each connection carried.
    fn serving(
        connections: Vec<(Vec<&'static [u8]>, bool)>,
    ) -> (String, std::thread::JoinHandle<Vec<usize>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let served = std::thread::spawn(move || {
            let mut held = Vec::new();
            let mut carried = Vec::new();
            for (replies, close) in connections {
                let (mut stream, _) = listener.accept().unwrap();
                for reply in &replies {
                    read_request(&mut stream);
                    stream.write_all(reply).unwrap();
                }
                carried.push(replies.len());
                if !close {
                    held.push(stream);
                }
            }
            carried
        });
        (addr, served)
    }

    const SEVEN: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-type: application/cbor\r\ncontent-length: 32\r\n\r\n\xa2bid\x01fresult\x81\xa2fstatusbOKfresult\x07";
    const SEVEN_THEN_CLOSE: &[u8] = b"HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-type: application/cbor\r\ncontent-length: 32\r\n\r\n\xa2bid\x01fresult\x81\xa2fstatusbOKfresult\x07";
    const SEVEN_CHUNKED: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-type: application/cbor\r\ntransfer-encoding: chunked\r\n\r\n10\r\n\xa2bid\x01fresult\x81\xa2fs\r\n10\r\ntatusbOKfresult\x07\r\n0\r\n\r\n";

    fn sevens(client: &Client, times: usize) {
        for _ in 0..times {
            assert_eq!(client.query("RETURN 7;", json!({})), Ok(vec![json!(7)]));
        }
    }

    #[test]
    fn a_clients_requests_go_over_one_connection_the_server_keeps_open() {
        let (addr, served) = serving(vec![(vec![SEVEN, SEVEN, SEVEN], false)]);
        sevens(&Client::new(&addr, "sf", "t").waiting(PATIENT), 3);
        assert_eq!(served.join().unwrap(), vec![3]);
    }

    #[test]
    fn a_kept_connection_the_server_has_since_closed_is_replaced_by_a_new_one() {
        let (addr, served) = serving(vec![(vec![SEVEN], true), (vec![SEVEN], false)]);
        let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
        sevens(&client, 1);
        {
            let kept = client.kept.lock().unwrap();
            assert_eq!(kept.len(), 1, "the first reply left a kept connection");
            // The reply can arrive before the server drops its socket. Wait for the peer's EOF
            // before the second query, using the same read timeout as the requests.
            assert_eq!(kept[0].stream.peek(&mut [0]).unwrap(), 0);
        }
        sevens(&client, 1);
        assert_eq!(served.join().unwrap(), vec![1, 1]);
        assert_eq!(client.requests(), 2);
        assert_eq!(
            client.kept.lock().unwrap()[0].served,
            1,
            "the count belongs to the replacement socket"
        );
    }

    #[test]
    fn a_reply_that_says_it_closes_its_connection_is_the_last_on_it() {
        let (addr, served) = serving(vec![(vec![SEVEN_THEN_CLOSE], false), (vec![SEVEN], false)]);
        sevens(&Client::new(&addr, "sf", "t").waiting(PATIENT), 2);
        assert_eq!(served.join().unwrap(), vec![1, 1]);
    }

    #[test]
    fn a_chunked_reply_on_a_kept_connection_is_read_to_its_last_chunk() {
        let (addr, served) = serving(vec![(vec![SEVEN_CHUNKED, SEVEN], false)]);
        sevens(&Client::new(&addr, "sf", "t").waiting(PATIENT), 2);
        assert_eq!(served.join().unwrap(), vec![2]);
    }

    #[test]
    fn a_query_goes_as_cbor_to_its_namespace_and_database() {
        let reply = b"HTTP/1.1 200 OK\r\ncontent-type: application/cbor\r\ncontent-length: 32\r\n\r\n\xa2bid\x01fresult\x81\xa2fstatusbOKfresult\x07";
        let (addr, served) = one_reply(reply);
        let client = Client::new(&addr, "sf", "tasks");
        assert_eq!(
            client.query("RETURN $n;", json!({"n": 7})),
            Ok(vec![json!(7)])
        );
        assert_eq!(client.requests(), 1);
        let request = served.join().unwrap();
        let end = find(&request, b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&request[..end]).to_string();
        for line in [
            "POST /rpc HTTP/1.1",
            "Content-Type: application/cbor",
            "Accept: application/cbor",
            "surreal-ns: sf",
            "surreal-db: tasks",
        ] {
            assert!(head.lines().any(|l| l == line), "{line:?} not in {head}");
        }
        let body = cbor::decode(&request[end + 4..]).unwrap();
        assert_eq!(
            body,
            json!({"id": 1, "method": "query", "params": ["RETURN $n;", {"n": 7}]})
        );
    }

    #[test]
    fn a_reply_that_is_not_cbor_is_a_failure_naming_the_status() {
        let (addr, served) =
            one_reply(b"HTTP/1.1 415 Unsupported Media Type\r\ncontent-length: 5\r\n\r\nnope!");
        let failed = Client::new(&addr, "sf", "tasks").query("RETURN 1;", json!({}));
        served.join().unwrap();
        match failed {
            Err(DbError::Failed(why)) => assert!(
                why.starts_with("HTTP 415 with a body that is not CBOR")
                    && why.ends_with(": nope!"),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
    }
}
