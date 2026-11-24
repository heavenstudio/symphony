//! A recording relay between the board's clients and its PostgreSQL. Its socket's folder is its
//! own, short, under `/tmp` and named for this process, and holds the server's socket file
//! (`.s.PGSQL.5432`, [`SOCKET_FILE`]), so a command reaches the board through it as
//! `TASKS_SERVER=postgresql:FOLDER` ([`Proxy::addr`]), and a store as a client of that folder.
//! Each connection is passed to the server's own socket byte for byte, both ways. The client's
//! messages are read as they pass (protocol 3.0: an untyped startup, then each a type and a
//! length), and each request is kept up to its Sync, one round trip ([`Request`]): every statement
//! it parsed or queried, with the parameter it bound, the variables the store sends as its one
//! `jsonb`. The plan check and the counts read what the commands sent, whatever code path sent it,
//! rather than a list of statements written down beside the code.
//!
//! The server's answers are read as they pass too, message by message, and what each notice says
//! is kept ([`Proxy::notices`]): the plan check has the server send it the plan of every statement
//! a session runs as a notice of the session's own (`crate::board`), which reaches the relay whole
//! and before the answer it belongs to.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use symphony_board::db::DbError;
use symphony_board::lock::Recovered as _;
use symphony_board::pg::{self, Answer, Step};
use symphony_board::server::postgres::{SOCKET_FILE, SOCKET_PATH_MAX};

/// The startup's codes that ask for an encryption first, after which the client sends its startup
/// again: SSLRequest and GSSENCRequest.
const ENCRYPTION_ASKED: [i32; 2] = [80_877_103, 80_877_104];

/// The longest message the relay reads: a board's whole dump goes in one request, under this.
const LONGEST: usize = 1 << 30;

/// One statement a client sent: its text, and the parameter it bound, as text, when it bound one.
#[derive(Clone, Debug, PartialEq)]
pub struct Sent {
    pub sql: String,
    pub param: Option<String>,
}

/// One request a client sent, one round trip: its statements up to its Sync (each parsed, bound
/// and run), or one simple query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// Its statements' text, one after another, `;` and a line between them.
    pub sql: String,
    /// The variables it carried: its first parameter, the one `jsonb` the store binds once in a
    /// request, or null when it bound none.
    pub vars: Value,
    /// Each statement, with the parameter it bound.
    pub steps: Vec<Sent>,
}

impl Request {
    /// The request of `steps`, in order.
    pub fn of(steps: Vec<Sent>) -> Request {
        let sql = (steps.iter())
            .map(|step| step.sql.as_str())
            .collect::<Vec<_>>()
            .join(";\n");
        let vars = (steps.iter())
            .find_map(|step| step.param.as_deref())
            .and_then(|param| serde_json::from_str(param).ok())
            .unwrap_or(Value::Null);
        Request { sql, vars, steps }
    }

    /// Whether one of its statements holds `text`.
    pub fn holds(&self, text: &str) -> bool {
        self.steps.iter().any(|step| step.sql.contains(text))
    }

    /// Whether every statement of it only reads: each a `SELECT` that locks no row. A write's
    /// block (`DO`), a row written, a lock taken or a simple query of the schema's is not one, and
    /// is never sent again.
    pub fn reads(&self) -> bool {
        (self.steps.iter()).all(|step| {
            let sql = step.sql.trim_start();
            sql.get(..6)
                .is_some_and(|head| head.eq_ignore_ascii_case("SELECT"))
                && !sql.contains("FOR UPDATE")
                && !sql.contains("FOR SHARE")
        })
    }

    /// The request sent again on `client`, as it was sent: each statement, the library's own text,
    /// with the parameter it bound, in one transaction. Each statement's answer, in order.
    pub fn again(&self, client: &pg::Client) -> Result<Vec<Answer>, DbError> {
        let steps: Vec<Step> = (self.steps.iter())
            .map(|step| match step.param.as_deref() {
                Some(param) => Step::with(&step.sql, param),
                None => Step::bare(&step.sql),
            })
            .collect();
        client.request(&steps)
    }
}

/// The relay: its socket's folder and what passed through it.
pub struct Proxy {
    /// The folder of its socket, which a client names as the server's.
    pub folder: PathBuf,
    /// Where a command reaches the board through it: `postgresql:` and its folder.
    pub addr: String,
    sent: Arc<Mutex<Vec<Request>>>,
    notices: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

/// How many relays this process has made, which names the next one's folder.
static RELAYS: AtomicUsize = AtomicUsize::new(0);

impl Proxy {
    /// A relay to the PostgreSQL `upstream` names as a command's `TASKS_SERVER` does,
    /// `postgresql:` and its socket's folder.
    pub fn to(upstream: &str) -> Proxy {
        let upstream = Path::new(upstream.strip_prefix("postgresql:").unwrap_or(upstream));
        let folder = PathBuf::from(format!(
            "/tmp/symphony-perf-relay-{}-{}",
            std::process::id(),
            RELAYS.fetch_add(1, Ordering::SeqCst)
        ));
        // An ended run's, whose process had this one's id.
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).expect("the relay's folder");
        let socket = folder.join(SOCKET_FILE);
        assert!(
            socket.as_os_str().len() <= SOCKET_PATH_MAX,
            "the relay's socket {} is longer than a socket path may be",
            socket.display()
        );
        let listener = UnixListener::bind(&socket).expect("the relay binds");
        let addr = format!("postgresql:{}", folder.display());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let notices = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let kept = Arc::clone(&sent);
        let noticed = Arc::clone(&notices);
        let stopped = Arc::clone(&stop);
        let upstream = upstream.join(SOCKET_FILE);
        // Each connection as it comes; the drop's own connection wakes the accept to its stop.
        std::thread::spawn(move || {
            for client in listener.incoming() {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                let client = match client {
                    Ok(client) => client,
                    Err(why) if passing(&why) => continue,
                    Err(why) => panic!("the relay's accept failed: {why}"),
                };
                let server = UnixStream::connect(&upstream).expect("the server answers");
                relay(client, server, Arc::clone(&kept), Arc::clone(&noticed));
            }
        });
        Proxy {
            folder,
            addr,
            sent,
            notices,
            stop,
        }
    }

    /// A client of the board's database through the relay.
    pub fn client(&self, database: &str) -> pg::Client {
        pg::Client::new(&self.folder, database).waiting(std::time::Duration::from_secs(600))
    }

    /// The requests sent so far, which are then forgotten.
    pub fn take(&self) -> Vec<Request> {
        std::mem::take(&mut *sent_so_far(&self.sent))
    }

    /// What each notice the server sent so far said, in the order they came, which are then
    /// forgotten.
    pub fn notices(&self) -> Vec<String> {
        std::mem::take(&mut *self.notices.lock().recover())
    }
}

/// The requests kept so far. A panic while they were held (a relay thread's push of one request,
/// read whole before, or a test's take of them all) leaves every request kept whole: what a later
/// reader finds was each sent.
fn sent_so_far(sent: &Mutex<Vec<Request>>) -> MutexGuard<'_, Vec<Request>> {
    sent.lock().recover()
}

/// Whether an accept's failure is one that passes, so the next accept is made: a signal, or a
/// connection its client dropped before it was taken.
fn passing(why: &std::io::Error) -> bool {
    matches!(
        why.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionAborted
    )
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wakes the accept, which then sees the stop.
        let _ = UnixStream::connect(self.folder.join(SOCKET_FILE));
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// One message a client sent, read whole: its type (none for the startup's, which has none), its
/// body, and its bytes as they came, to pass on unchanged.
struct Message {
    kind: Option<u8>,
    body: Vec<u8>,
    raw: Vec<u8>,
}

/// The next message on `from`, typed unless it is the startup's (`typed` false): none at the
/// stream's end, or where it names a length no message has.
fn message(from: &mut impl Read, typed: bool) -> Option<Message> {
    let mut raw = Vec::new();
    let kind = if typed {
        let mut kind = [0u8; 1];
        from.read_exact(&mut kind).ok()?;
        raw.push(kind[0]);
        Some(kind[0])
    } else {
        None
    };
    let mut length = [0u8; 4];
    from.read_exact(&mut length).ok()?;
    raw.extend(length);
    let length = usize::try_from(i32::from_be_bytes(length)).ok()?;
    if !(4..=LONGEST).contains(&length) {
        return None;
    }
    let mut body = vec![0u8; length - 4];
    from.read_exact(&mut body).ok()?;
    raw.extend(&body);
    Some(Message { kind, body, raw })
}

/// What a client's stream has sent so far, read message by message: whether its startup is done,
/// the request it is sending, and each request it ended.
#[derive(Default)]
struct Reading {
    started: bool,
    steps: Vec<Sent>,
}

/// What one message did to the request being sent.
#[derive(Debug, PartialEq)]
enum Passing {
    /// Nothing a request keeps.
    Passed,
    /// A request ended, whole.
    Ended(Request),
    /// The connection's end.
    Closed,
}

impl Reading {
    /// `message` read into the request being sent: a simple query is a request of its own, a
    /// Parse a statement, a Bind the parameter of the statement parsed before it, a Sync the
    /// request's end, a Terminate the connection's.
    fn read(&mut self, message: &Message) -> Passing {
        let Some(kind) = message.kind else {
            let code = message
                .body
                .get(..4)
                .map(|code| i32::from_be_bytes([code[0], code[1], code[2], code[3]]));
            self.started = !code.is_some_and(|code| ENCRYPTION_ASKED.contains(&code));
            return Passing::Passed;
        };
        let body = &message.body;
        match kind {
            b'Q' => match cstrings(body, 1).first() {
                Some(sql) => Passing::Ended(Request::of(vec![Sent {
                    sql: sql.clone(),
                    param: None,
                }])),
                None => Passing::Passed,
            },
            b'P' => {
                if let Some(sql) = cstrings(body, 2).get(1) {
                    self.steps.push(Sent {
                        sql: sql.clone(),
                        param: None,
                    });
                }
                Passing::Passed
            }
            b'B' => {
                if let (Some(param), Some(last)) = (first_param(body), self.steps.last_mut()) {
                    last.param = Some(param);
                }
                Passing::Passed
            }
            b'S' if !self.steps.is_empty() => {
                Passing::Ended(Request::of(std::mem::take(&mut self.steps)))
            }
            b'X' => Passing::Closed,
            _ => Passing::Passed,
        }
    }
}

/// The first `n` zero-ended strings of `body`, as many of them as it holds whole.
fn cstrings(body: &[u8], n: usize) -> Vec<String> {
    let (mut found, mut rest) = (Vec::new(), body);
    while found.len() < n {
        let Some(end) = rest.iter().position(|byte| *byte == 0) else {
            break;
        };
        found.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = &rest[end + 1..];
    }
    found
}

/// A Bind's first parameter, as text, when it binds one that is not null: after its portal and its
/// statement (zero-ended), its formats (a count, two bytes each), then its parameters (a count, and
/// each its length and its bytes).
fn first_param(body: &[u8]) -> Option<String> {
    let mut at = 0;
    for _ in 0..2 {
        at += body.get(at..)?.iter().position(|byte| *byte == 0)? + 1;
    }
    let short = |at: usize| -> Option<usize> {
        let two = body.get(at..at + 2)?;
        usize::try_from(i16::from_be_bytes([two[0], two[1]])).ok()
    };
    let formats = short(at)?;
    at += 2 + 2 * formats;
    let params = short(at)?;
    at += 2;
    if params == 0 {
        return None;
    }
    let four = body.get(at..at + 4)?;
    let length = usize::try_from(i32::from_be_bytes([four[0], four[1], four[2], four[3]])).ok()?;
    at += 4;
    Some(String::from_utf8_lossy(body.get(at..at + length)?).into_owned())
}

/// The client's messages and the server's answers, each passed on as it came. The client's are read
/// one at a time, each request kept as it ends; the server's are read one at a time too, each
/// notice's word kept ([`answers`]).
fn relay(
    client: UnixStream,
    server: UnixStream,
    kept: Arc<Mutex<Vec<Request>>>,
    notices: Arc<Mutex<Vec<String>>>,
) {
    let (answering, to_client) = (server.try_clone().unwrap(), client.try_clone().unwrap());
    // Whether the client asked for an encryption the server has not answered yet.
    let asked = Arc::new(AtomicBool::new(false));
    let answer_asked = Arc::clone(&asked);
    std::thread::spawn(move || {
        let mut to_client = to_client;
        answers(
            &mut BufReader::new(answering),
            &mut to_client,
            &answer_asked,
            &notices,
        );
        let _ = to_client.shutdown(Shutdown::Both);
    });
    std::thread::spawn(move || {
        let mut from = BufReader::new(client);
        let mut to_server = server;
        let mut reading = Reading::default();
        loop {
            let Some(message) = message(&mut from, reading.started) else {
                let _ = to_server.shutdown(Shutdown::Both);
                return;
            };
            let read = reading.read(&message);
            // Kept before it is passed on, so a client that has its answer finds it kept.
            if let Passing::Ended(request) = &read {
                sent_so_far(&kept).push(request.clone());
            }
            if message.kind.is_none() && !reading.started {
                asked.store(true, Ordering::SeqCst);
            }
            if to_server.write_all(&message.raw).is_err() || read == Passing::Closed {
                let _ = to_server.shutdown(Shutdown::Write);
                return;
            }
        }
    });
}

/// The server's answers on `from`, each passed on to `to` as it came, until the stream ends or
/// stops reading as messages: a notice's word (its `M` field) kept in `notices` before the notice
/// is passed on, so a client that has its answer finds every notice before it kept. The one byte
/// a server answers an encryption's request with, while the client's request is `asked`, passes
/// as it is.
fn answers(
    from: &mut impl BufRead,
    to: &mut impl Write,
    asked: &AtomicBool,
    notices: &Mutex<Vec<String>>,
) {
    loop {
        match from.fill_buf() {
            Ok([]) | Err(_) => return,
            Ok(bytes) if asked.swap(false, Ordering::SeqCst) => {
                let answer = [bytes[0]];
                from.consume(1);
                if to.write_all(&answer).is_err() {
                    return;
                }
                continue;
            }
            Ok(_) => {}
        }
        let Some(answer) = message(from, true) else {
            return;
        };
        if answer.kind == Some(b'N')
            && let Some(said) = word(&answer.body)
        {
            notices.lock().recover().push(said);
        }
        if to.write_all(&answer.raw).is_err() {
            return;
        }
    }
}

/// What a notice's fields say (its `M`): each field a type and a zero-ended text, a zero after the
/// last.
fn word(body: &[u8]) -> Option<String> {
    let mut rest = body;
    while let Some((&field, after)) = rest.split_first() {
        if field == 0 {
            return None;
        }
        let end = after.iter().position(|byte| *byte == 0)?;
        if field == b'M' {
            return Some(String::from_utf8_lossy(&after[..end]).into_owned());
        }
        rest = &after[end + 1..];
    }
    None
}

#[cfg(test)]
mod read {
    use super::*;

    fn framed(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![kind];
        out.extend(i32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend(body);
        out
    }

    fn cstring(out: &mut Vec<u8>, text: &str) {
        out.extend(text.as_bytes());
        out.push(0);
    }

    /// A Parse of the unnamed statement `sql`, typed `jsonb` when it takes a parameter, as the
    /// board's client sends it.
    fn parse(sql: &str, takes: bool) -> Vec<u8> {
        let mut body = Vec::new();
        cstring(&mut body, "");
        cstring(&mut body, sql);
        if takes {
            body.extend(1i16.to_be_bytes());
            body.extend(3802i32.to_be_bytes());
        } else {
            body.extend(0i16.to_be_bytes());
        }
        framed(b'P', &body)
    }

    /// A Bind of the unnamed portal, `param` its one parameter when it has one.
    fn bind(param: Option<&str>) -> Vec<u8> {
        let mut body = Vec::new();
        cstring(&mut body, "");
        cstring(&mut body, "");
        body.extend(0i16.to_be_bytes());
        match param {
            Some(param) => {
                body.extend(1i16.to_be_bytes());
                body.extend(i32::try_from(param.len()).unwrap().to_be_bytes());
                body.extend(param.as_bytes());
            }
            None => body.extend(0i16.to_be_bytes()),
        }
        body.extend(0i16.to_be_bytes());
        framed(b'B', &body)
    }

    fn startup(code: i32) -> Vec<u8> {
        let mut body = code.to_be_bytes().to_vec();
        cstring(&mut body, "user");
        cstring(&mut body, "postgres");
        body.push(0);
        let mut out = i32::try_from(body.len() + 4)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        out.extend(body);
        out
    }

    /// What a client's bytes read as, message by message, as the relay reads them.
    fn read_all(bytes: &[u8]) -> (Vec<Request>, bool, usize) {
        let mut from = bytes;
        let mut reading = Reading::default();
        let (mut ended, mut closed, mut passed_bytes) = (Vec::new(), false, 0);
        while let Some(message) = message(&mut from, reading.started) {
            passed_bytes += message.raw.len();
            match reading.read(&message) {
                Passing::Ended(request) => ended.push(request),
                Passing::Closed => closed = true,
                Passing::Passed => {}
            }
        }
        (ended, closed, passed_bytes)
    }

    /// A request is its statements up to its Sync, each with the parameter it bound; a simple
    /// query is a request of its own; an SSL request leaves the startup still to come; every byte
    /// is passed on, and a Terminate is the connection's end.
    #[test]
    fn a_request_is_its_statements_with_their_parameters_up_to_its_sync() {
        let mut bytes = startup(80_877_103);
        bytes.extend(startup(196_608));
        let vars = r#"{"slug": "area01/task-0001", "handles": ["perf-0001"]}"#;
        bytes.extend(parse(
            "SELECT FROM set_config('board.vars', $1::text, true)",
            true,
        ));
        bytes.extend(bind(Some(vars)));
        bytes.extend(framed(b'E', &[0, 0, 0, 0, 0]));
        bytes.extend(parse("SELECT 1", false));
        bytes.extend(bind(None));
        bytes.extend(framed(b'E', &[0, 0, 0, 0, 0]));
        bytes.extend(framed(b'S', &[]));
        let mut query = Vec::new();
        cstring(&mut query, "VACUUM (ANALYZE)");
        bytes.extend(framed(b'Q', &query));
        bytes.extend(framed(b'S', &[]));
        bytes.extend(framed(b'X', &[]));
        let (requests, closed, passed) = read_all(&bytes);
        assert_eq!(passed, bytes.len(), "every byte passes on");
        assert!(closed);
        assert_eq!(requests.len(), 2, "{requests:#?}");
        assert_eq!(
            requests[0].steps,
            [
                Sent {
                    sql: "SELECT FROM set_config('board.vars', $1::text, true)".into(),
                    param: Some(vars.into()),
                },
                Sent {
                    sql: "SELECT 1".into(),
                    param: None,
                },
            ]
        );
        assert_eq!(
            requests[0].sql, "SELECT FROM set_config('board.vars', $1::text, true);\nSELECT 1",
            "one round trip's statements, one after another"
        );
        assert_eq!(requests[0].vars["handles"][0], "perf-0001");
        assert!(requests[0].reads());
        assert_eq!(requests[1].steps[0].sql, "VACUUM (ANALYZE)");
        assert_eq!(requests[1].vars, Value::Null);
        assert!(
            !requests[1].reads(),
            "a simple query of maintenance is not a read"
        );
    }

    /// A write's block, a row written or locked is no read, whatever statements beside it read.
    #[test]
    fn a_block_a_write_or_a_lock_is_no_read() {
        let request = |sqls: &[&str]| {
            Request::of(
                (sqls.iter())
                    .map(|sql| Sent {
                        sql: sql.to_string(),
                        param: None,
                    })
                    .collect(),
            )
        };
        assert!(request(&["SELECT 1", "  select 2"]).reads());
        assert!(!request(&["SELECT 1", "DO $board$ BEGIN NULL; END $board$"]).reads());
        assert!(!request(&["INSERT INTO said VALUES (1)"]).reads());
        assert!(!request(&["SELECT 1 FROM task FOR UPDATE"]).reads());
        assert!(request(&["SELECT 'DO'"]).reads());
    }

    /// A notice of the given fields, each a type and its text.
    fn notice(fields: &[(u8, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (field, text) in fields {
            body.push(*field);
            cstring(&mut body, text);
        }
        body.push(0);
        framed(b'N', &body)
    }

    /// The server's answers pass on byte for byte, the encryption's one-byte answer first when one
    /// was asked; each notice's word is kept, in order, and a notice with none keeps nothing.
    #[test]
    fn the_answers_pass_whole_and_each_notices_word_is_kept() {
        let plan = "duration: 0.050 ms  plan:\n{\"Query Text\": \"SELECT 1\"}";
        let mut bytes = vec![b'N'];
        bytes.extend(framed(b'R', &0i32.to_be_bytes()));
        bytes.extend(notice(&[(b'S', "NOTICE"), (b'C', "00000"), (b'M', plan)]));
        bytes.extend(notice(&[(b'S', "NOTICE")]));
        bytes.extend(framed(b'C', b"SELECT 1\0"));
        bytes.extend(notice(&[(b'M', "another")]));
        bytes.extend(framed(b'Z', b"I"));
        let (asked, notices, mut passed) =
            (AtomicBool::new(true), Mutex::new(Vec::new()), Vec::new());
        answers(&mut &bytes[..], &mut passed, &asked, &notices);
        assert_eq!(passed, bytes, "every byte passes on, as it came");
        assert!(
            !asked.load(Ordering::SeqCst),
            "the encryption's answer was taken"
        );
        assert_eq!(notices.into_inner().unwrap(), [plan, "another"]);
    }

    /// Without an encryption asked, a stream that stops reading as messages ends the passing.
    #[test]
    fn answers_that_stop_reading_as_messages_end_the_passing() {
        let mut bytes = framed(b'Z', b"I");
        bytes.extend([b'N', 0, 0, 0, 1]);
        let (asked, notices, mut passed) =
            (AtomicBool::new(false), Mutex::new(Vec::new()), Vec::new());
        answers(&mut &bytes[..], &mut passed, &asked, &notices);
        assert_eq!(passed, framed(b'Z', b"I"));
        assert!(notices.into_inner().unwrap().is_empty());
        assert_eq!(word(b"Mx"), None, "a field cut short says nothing");
    }

    /// A message that names a length no message has, or is cut short, ends the reading.
    #[test]
    fn a_message_of_no_length_or_cut_short_ends_the_reading() {
        assert!(message(&mut &[b'Q', 0, 0, 0, 2][..], true).is_none());
        assert!(message(&mut &[b'Q', 0, 0, 0, 9, b'x'][..], true).is_none());
        assert!(first_param(&[0, 0, 0, 0, 0, 1, 0, 0, 0, 9, b'x']).is_none());
    }
}
