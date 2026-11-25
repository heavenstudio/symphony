//! The client without a server: the messages it makes, and what it reads of a server's bytes.
//! Each server answer here was recorded from PostgreSQL 18.7 as this client met it (a proxy on
//! the socket of a scratch server, each answer to its ReadyForQuery), the startup's parameters cut
//! to two; a fake server on a socket of its own plays them back.

use super::*;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

/// A folder of one test's own under the temporary folder, removed when it goes.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pg-fake-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// AuthenticationOk, two of the parameters, the backend's key and ReadyForQuery, idle.
const STARTED: &str = "5200000008000000005300000019636c69656e745f656e636f64696e670055544638\
    0053000000237365727665725f76657273696f6e0031382e372028486f6d656272657729004b0000000c00014d\
    4c8220b27d5a0000000549";
/// `SELECT $1::jsonb, NULL::text` of `{"a": 1}`: the jsonb's text and a null.
const JSONB_AND_NULL: &str = "3100000004320000000444000000160002000000087b2261223a20317dffffffff\
    430000000d53454c4543542031005a0000000549";
/// `SELECT 1`, then a `DO` block's `RAISE EXCEPTION 'refused: no such task'`, then `SELECT 2`,
/// which the server never runs.
const RAISED_AFTER_ONE: &str = "31000000043200000004440000000b00010000000131430000000d53454c4543\
    54203100310000000432000000044500000088534552524f5200564552524f5200435030303031004d726566757365\
    643a206e6f2073756368207461736b0057504c2f706753514c2066756e6374696f6e20696e6c696e655f636f6465\
    5f626c6f636b206c696e6520312061742052414953450046706c5f657865632e63004c33393339005265786563\
    5f73746d745f726169736500005a0000000549";
/// `SELECT 'x' FROM`: a syntax error, SQLSTATE 42601.
const SYNTAX: &str = "450000005b534552524f5200564552524f5200433432363031004d73796e746178206572726f\
    7220617420656e64206f6620696e7075740050313600467363616e2e6c004c3132333200527363616e6e65725f79\
    796572726f7200005a0000000549";
/// A `DO` block's `RAISE NOTICE 'heard'`.
const NOTICE: &str = "310000000432000000044e0000007a534e4f5449434500564e4f5449434500433030303030\
    004d68656172640057504c2f706753514c2066756e6374696f6e20696e6c696e655f636f64655f626c6f636b20\
    6c696e6520312061742052414953450046706c5f657865632e63004c333933390052657865635f73746d745f7261\
    6973650000\
    4300000007444f005a0000000549";
/// An `UPDATE` in an open REPEATABLE READ transaction of a row another committed since its
/// snapshot: SQLSTATE 40001, and ReadyForQuery in a failed transaction.
const SERIALIZATION: &str = "4500000074534552524f5200564552524f5200433430303031004d636f756c64206e\
    6f742073657269616c697a65206163636573732064756520746f20636f6e63757272656e742075706461746500466e\
    6f64654d6f646966795461626c652e63004c3236313200524578656355706461746500005a0000000545";
/// `ROLLBACK`, and ReadyForQuery, idle.
const ROLLED_BACK: &str = "430000000d524f4c4c4241434b005a0000000549";
/// A startup to a database the server does not hold: AuthenticationOk, then FATAL 3D000.
const NO_SUCH_DATABASE: &str = "520000000800000000450000005e53464154414c0056464154414c00433344\
    303030004d646174616261736520226e6f6e65737563682220646f6573206e6f74206578697374004670\
    6f7374696e69742e63004c313031360052496e6974506f7374677265730000";
/// A server that asks for SCRAM-SHA-256 where it should trust.
const PASSWORD: &str = "52000000170000000a534352414d2d5348412d3235360000";

fn bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "{hex}");
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

/// What a fake server answers a connection: its startup's answer, then each request's.
struct Script {
    started: &'static str,
    answers: Vec<&'static str>,
}

/// A fake server on a socket of its own: each connection it takes plays its script, one script a
/// connection, and every byte each connection sent after its startup is handed back.
struct Fake {
    dir: Dir,
    sent: mpsc::Receiver<(Vec<u8>, Vec<u8>)>,
}

impl Fake {
    fn new(scripts: Vec<Script>) -> Fake {
        let dir = Dir::new();
        let listener =
            UnixListener::bind(dir.path().join(crate::server::postgres::SOCKET_FILE)).unwrap();
        let (tell, sent) = crate::test_channel::channel();
        thread::spawn(move || {
            for script in scripts {
                let (mut stream, _) = listener.accept().unwrap();
                let startup = startup(&mut stream);
                stream.write_all(&bytes(script.started)).unwrap();
                let mut after = Vec::new();
                for answer in script.answers {
                    loop {
                        let message = wire::read(&mut stream).unwrap();
                        after.push(message.kind);
                        after.extend(len(message.body.len() + 4));
                        after.extend(&message.body);
                        if message.kind == b'S' || message.kind == b'Q' {
                            break;
                        }
                    }
                    stream.write_all(&bytes(answer)).unwrap();
                }
                finish(&mut stream, &mut after);
                tell.send((startup, after)).unwrap();
            }
        });
        Fake { dir, sent }
    }

    fn client(&self) -> Client {
        Client::new(self.dir.path(), "board")
    }

    /// The next connection's startup and what it sent after, once it has ended.
    fn sent(&self) -> (Vec<u8>, Vec<u8>) {
        self.sent.recv().unwrap()
    }
}

/// End the fake's answers and collect the client's last bytes. A client still reading meets the
/// end; one done may already have disconnected.
fn finish(stream: &mut UnixStream, after: &mut Vec<u8>) {
    stream
        .shutdown(std::net::Shutdown::Write)
        .unwrap_or_else(|why| {
            assert_eq!(why.kind(), std::io::ErrorKind::NotConnected, "{why}");
        });
    let _ = stream.read_to_end(after);
}

/// A connection's startup message, read whole: its body.
fn startup(stream: &mut UnixStream) -> Vec<u8> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).unwrap();
    let mut startup = vec![0u8; u32::from_be_bytes(length) as usize - 4];
    stream.read_exact(&mut startup).unwrap();
    startup
}

/// A server of one connection on a socket of its own: the startup read and answered as
/// [`STARTED`] says, then the connection handed to `serve`.
fn serving(serve: impl FnOnce(UnixStream) + Send + 'static) -> Dir {
    let dir = Dir::new();
    let listener =
        UnixListener::bind(dir.path().join(crate::server::postgres::SOCKET_FILE)).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        startup(&mut stream);
        stream.write_all(&bytes(STARTED)).unwrap();
        serve(stream);
    });
    dir
}

/// What `work` gives, on a thread of its own and waited on as a wait on its condition
/// (`test_time::until_true`): work that never ends fails the test as a stall naming `what`, and
/// never hangs it.
fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let running = thread::spawn(work);
    let backstop = test_time::backstop(Duration::from_secs(10));
    test_time::until_true(what, backstop, || running.is_finished())
        .unwrap_or_else(|stall| panic!("{stall}"));
    running
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// A megabyte of `jsonb`: a string, its text more than the sockets on both sides hold.
fn megabyte() -> String {
    format!("\"{}\"", "y".repeat(1 << 20))
}

/// The type of each message in `sent`, in order.
fn kinds(sent: &[u8]) -> Vec<u8> {
    let mut from = sent;
    let mut found = Vec::new();
    while !from.is_empty() {
        found.push(wire::read(&mut from).unwrap().kind);
    }
    found
}

fn len(n: usize) -> [u8; 4] {
    u32::try_from(n).unwrap().to_be_bytes()
}

/// One bare request on a new client of `fake`.
fn one(fake: &Fake) -> Result<Vec<Answer>, DbError> {
    fake.client().request(&[Step::bare("SELECT 1")])
}

/// What the board's health `log` holds of the requests of the last hour: how many were sent, how
/// many got no answer, and whether the slowest got none.
fn recorded(log: &crate::board_health::QueryLog) -> (u64, u64, Option<bool>) {
    let (queries, _) = log.report(crate::sweep::trigger::now());
    let slowest = queries.slowest.map(|slowest| slowest.failed);
    (queries.count, queries.failed, slowest)
}

#[test]
fn a_fake_collects_the_clients_last_bytes_whichever_side_ends_first() {
    bounded("a fake and its client ending in either order", || {
        for client_first in [true, false] {
            let (mut stream, mut client) = UnixStream::pair().unwrap();
            let ended = if client_first {
                client.write_all(b"X\0\0\0\x04").unwrap();
                drop(client);
                None
            } else {
                Some(thread::spawn(move || {
                    // The client stays connected until the fake has ended its answers.
                    assert_eq!(client.read(&mut [0]).unwrap(), 0);
                    client.write_all(b"X\0\0\0\x04").unwrap();
                }))
            };
            let mut after = b"S\0\0\0\x04".to_vec();
            finish(&mut stream, &mut after);
            assert_eq!(after, b"S\0\0\0\x04X\0\0\0\x04");
            if let Some(ended) = ended {
                ended.join().unwrap();
            }
        }
    });
}

#[test]
fn a_request_is_each_statement_parsed_bound_and_run_then_one_sync() {
    let mut out = Vec::new();
    wire::parse(&mut out, "SELECT $1", true);
    wire::bind(&mut out, Some("{}"));
    wire::execute(&mut out);
    wire::sync(&mut out);
    assert_eq!(
        out,
        [
            b"P\0\0\0\x15\0SELECT $1\0\0\x01\0\0\x0e\xda".as_slice(),
            b"B\0\0\0\x12\0\0\0\0\0\x01\0\0\0\x02{}\0\0",
            b"E\0\0\0\x09\0\0\0\0\0",
            b"S\0\0\0\x04",
        ]
        .concat()
    );
    let mut bare = Vec::new();
    wire::parse(&mut bare, "SELECT 1", false);
    wire::bind(&mut bare, None);
    assert_eq!(
        bare,
        [
            b"P\0\0\0\x10\0SELECT 1\0\0\0".as_slice(),
            b"B\0\0\0\x0c\0\0\0\0\0\0\0\0",
        ]
        .concat()
    );
}

#[test]
fn a_startup_names_the_user_its_database_utf8_and_repeatable_read() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![JSONB_AND_NULL],
    }]);
    let client = fake.client();
    let answers = client
        .request(&[Step::with("SELECT $1::jsonb, NULL::text", "{\"a\": 1}")])
        .unwrap();
    assert_eq!(
        answers,
        [Answer {
            rows: vec![vec![Some("{\"a\": 1}".into()), None]],
            tag: "SELECT 1".into(),
        }]
    );
    assert_eq!(answers[0].value(), Some("{\"a\": 1}"));
    drop(client);
    let (startup, after) = fake.sent();
    let text = String::from_utf8(startup[4..].to_vec()).unwrap();
    assert_eq!(&startup[..4], 196_608i32.to_be_bytes());
    assert_eq!(
        text.split('\0').collect::<Vec<_>>(),
        [
            "user",
            "postgres",
            "database",
            "board",
            "application_name",
            "symphony-board",
            "client_encoding",
            "UTF8",
            "default_transaction_isolation",
            "repeatable read",
            "",
            ""
        ]
    );
    assert!(
        after.ends_with(b"X\0\0\0\x04"),
        "a client that goes ends its connection: {after:?}"
    );
}

#[test]
fn a_raised_exception_is_thrown_in_its_words_and_the_connection_serves_the_next_request() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![RAISED_AFTER_ONE, JSONB_AND_NULL],
    }]);
    let client = fake.client();
    let raised = client.request(&[
        Step::bare("SELECT 1"),
        Step::bare("DO $$BEGIN RAISE EXCEPTION 'refused: no such task'; END$$"),
        Step::bare("SELECT 2"),
    ]);
    assert_eq!(raised, Err(DbError::Thrown("refused: no such task".into())));
    let next = client.request(&[Step::bare("SELECT 1")]).unwrap();
    assert_eq!(next[0].rows, [[Some("{\"a\": 1}".to_string()), None]]);
    drop(client);
    let (_, after) = fake.sent();
    assert_eq!(
        kinds(&after),
        b"PBEPBEPBESPBESX",
        "two requests, each one Sync, and nothing sent between them"
    );
}

#[test]
fn any_other_error_fails_in_the_servers_words_and_its_code() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![SYNTAX],
    }]);
    let failed = fake.client().request(&[Step::bare("SELECT 'x' FROM")]);
    assert_eq!(
        failed,
        Err(DbError::Failed(
            "syntax error at end of input (SQLSTATE 42601)".into()
        ))
    );
}

#[test]
fn a_notice_is_passed_over() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![NOTICE],
    }]);
    let answers = fake
        .client()
        .request(&[Step::bare("DO $$BEGIN RAISE NOTICE 'heard'; END$$")])
        .unwrap();
    assert_eq!(
        answers,
        [Answer {
            rows: vec![],
            tag: "DO".into()
        }]
    );
}

#[test]
fn a_serialization_failure_is_a_conflict_and_the_failed_transaction_is_rolled_back() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![SERIALIZATION, ROLLED_BACK, JSONB_AND_NULL],
    }]);
    let client = fake.client();
    assert_eq!(
        client.simple("UPDATE capture_t SET n = n + 1"),
        Err(DbError::Conflict)
    );
    assert!(client.request(&[Step::bare("SELECT 1")]).is_ok());
    drop(client);
    let (_, after) = fake.sent();
    let rollback = [b"Q\0\0\0\x0dROLLBACK\0".as_slice()].concat();
    assert!(
        after.windows(rollback.len()).any(|at| at == rollback),
        "a transaction left failed is rolled back before the next request: {after:?}"
    );
}

#[test]
fn a_deadlock_is_a_conflict_too() {
    let body = |state: &str| {
        let mut body = Vec::new();
        for (code, text) in [(b'S', "ERROR"), (b'C', state), (b'M', "deadlock detected")] {
            body.push(code);
            body.extend(text.as_bytes());
            body.push(0);
        }
        body.push(0);
        body
    };
    assert_eq!(error_of(&body("40P01")), DbError::Conflict);
    assert_eq!(error_of(&body("40001")), DbError::Conflict);
    assert_eq!(
        error_of(&body("23505")),
        DbError::Failed("deadlock detected (SQLSTATE 23505)".into())
    );
    assert_eq!(
        error_of(&[0]),
        DbError::Failed("the server gave no message (SQLSTATE )".into())
    );
}

#[test]
fn a_database_not_there_is_known_by_its_code() {
    let fake = Fake::new(vec![Script {
        started: NO_SUCH_DATABASE,
        answers: vec![],
    }]);
    let Err(DbError::Failed(said)) = one(&fake) else {
        panic!("a database not there is a failure");
    };
    assert_eq!(
        said,
        "database \"nonesuch\" does not exist (SQLSTATE 3D000)"
    );
    assert!(no_database(&said));
    assert!(!no_database(
        "syntax error at end of input (SQLSTATE 42601)"
    ));
}

#[test]
fn a_server_that_asks_for_a_password_is_refused() {
    let fake = Fake::new(vec![Script {
        started: PASSWORD,
        answers: vec![],
    }]);
    let Err(DbError::Failed(said)) = one(&fake) else {
        panic!("a password asked for is a failure");
    };
    assert!(
        said.contains("asks for a password (authentication 10)"),
        "{said}"
    );
}

#[test]
fn a_connection_that_breaks_mid_answer_is_dropped_and_the_next_request_makes_another() {
    /// The success's answer cut inside its data row.
    const CUT: &str = "31000000043200000004440000001600";
    let fake = Fake::new(vec![
        Script {
            started: STARTED,
            answers: vec![CUT],
        },
        Script {
            started: STARTED,
            answers: vec![JSONB_AND_NULL],
        },
    ]);
    let client = fake.client();
    let Err(DbError::Failed(said)) = client.request(&[Step::bare("SELECT 1")]) else {
        panic!("a cut answer is a failure");
    };
    assert!(said.starts_with("the board's PostgreSQL at "), "{said}");
    assert!(
        !crate::rpc::slow(&said),
        "a broken connection is not slow: {said}"
    );
    let (_, first) = fake.sent();
    assert!(!first.is_empty());
    assert!(client.request(&[Step::bare("SELECT 1")]).is_ok());
    let (count, failed, _) = recorded(&client.queries());
    assert_eq!((count, failed), (2, 1), "the cut one alone went unanswered");
}

#[test]
fn a_server_that_answers_at_length_before_it_has_read_the_request_is_read_while_it_is_written() {
    // A megabyte each way: a client that wrote the whole request before it read would wait on a
    // server that waits on it, each with its socket full.
    const ROWS: usize = 16;
    let column = "x".repeat(64 << 10);
    let mut answer = Vec::new();
    for _ in 0..ROWS {
        answer.push(b'D');
        answer.extend(len(4 + 2 + 4 + column.len()));
        answer.extend(1u16.to_be_bytes());
        answer.extend(len(column.len()));
        answer.extend(column.as_bytes());
    }
    answer.push(b'C');
    answer.extend(len(4 + b"SELECT 16\0".len()));
    answer.extend(b"SELECT 16\0");
    answer.extend(bytes("5a0000000549"));
    let (tell, heard) = crate::test_channel::channel();
    let dir = serving(move |mut stream| {
        stream.write_all(&answer).unwrap();
        let mut sent = Vec::new();
        loop {
            let message = wire::read(&mut stream).unwrap();
            let kind = message.kind;
            sent.push(message);
            if kind == b'S' {
                break;
            }
        }
        tell.send(sent).unwrap();
    });
    let client = Client::new(dir.path(), "board");
    let answers = bounded(
        "a request answered at length before the server has read it",
        move || client.request(&[Step::with("SELECT $1", &megabyte())]),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].tag, "SELECT 16");
    assert_eq!(answers[0].rows, vec![vec![Some(column)]; ROWS]);
    let sent = heard.recv().unwrap();
    let kinds: Vec<u8> = sent.iter().map(|message| message.kind).collect();
    assert_eq!(kinds, b"PBES");
    let bound = String::from_utf8_lossy(&sent[1].body);
    assert!(
        bound.contains(&megabyte()),
        "the server read the request whole"
    );
}

#[test]
fn a_server_that_takes_none_of_a_request_and_says_nothing_fails_it_at_the_clients_wait() {
    let (done, finished) = crate::test_channel::channel::<()>();
    // It holds the connection and reads nothing, until the client has given up.
    let dir = serving(move |stream| {
        let _ = finished.recv();
        drop(stream);
    });
    let client = started_then_waiting(&dir, Duration::from_millis(50));
    let failed = bounded("a request the server takes none of", move || {
        let failed = client.request(&[Step::with("SELECT $1", &megabyte())]);
        (failed, client.connection.lock().unwrap().is_none())
    });
    done.send(()).unwrap();
    let (Err(DbError::Failed(said)), true) = failed else {
        panic!("a request the server never takes fails, and its connection goes: {failed:?}");
    };
    // Its write or its read meets the wait first, and the side that did says it.
    let slow = format!(
        "the board's PostgreSQL at {} is slow: no answer within 50ms: ",
        dir.path().display()
    );
    let why = said
        .strip_prefix(&slow)
        .and_then(|rest| rest.strip_suffix(MAY_HAVE_RUN))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(
        [eagain(), format!("sending a request: {}", eagain())].contains(&why.to_string()),
        "{said}"
    );
    assert!(crate::rpc::slow(&said), "{said}");
}

/// A client of the server in `dir` whose connection is made at the client's default wait, then
/// waits `wait` on each write and read of a request: a request's wait that no slow startup, on a
/// loaded machine, meets first.
fn started_then_waiting(dir: &Dir, wait: Duration) -> Client {
    let mut client = Client::new(dir.path(), "board");
    let connection = client.connect().unwrap().unwrap();
    connection.writer.set_write_timeout(Some(wait)).unwrap();
    connection
        .stream
        .get_ref()
        .set_read_timeout(Some(wait))
        .unwrap();
    *client.connection.get_mut().unwrap() = Some(connection);
    client.waiting(wait)
}

/// What a request that went out on a connection that worked and met the client's wait ends with.
const MAY_HAVE_RUN: &str = "; the request may have run there, so it was not sent again";

/// The system's words for a socket's wait run out: `EAGAIN`'s.
fn eagain() -> String {
    std::io::Error::from_raw_os_error(libc::EAGAIN).to_string()
}

#[test]
fn a_request_the_server_takes_and_never_answers_says_it_is_slow_at_the_clients_wait() {
    let (done, finished) = crate::test_channel::channel::<()>();
    // It reads the whole request and answers nothing, until the client has given up.
    let dir = serving(move |mut stream| {
        while wire::read(&mut stream).unwrap().kind != b'S' {}
        let _ = finished.recv();
        drop(stream);
    });
    let client = started_then_waiting(&dir, Duration::from_millis(50));
    let log = client.queries();
    let failed = bounded("a request the server never answers", move || {
        let failed = client.request(&[Step::bare("SELECT 1")]);
        (failed, client.connection.lock().unwrap().is_none())
    });
    done.send(()).unwrap();
    let (Err(DbError::Failed(said)), true) = failed else {
        panic!("a request the server never answers fails, and its connection goes: {failed:?}");
    };
    assert_eq!(
        said,
        format!(
            "the board's PostgreSQL at {} is slow: no answer within 50ms: {}{MAY_HAVE_RUN}",
            dir.path().display(),
            eagain()
        )
    );
    // As the store says it, and the page's read takes it: slow, not unreachable.
    assert!(
        crate::rpc::slow(&format!("the board's database: {said}")),
        "{said}"
    );
    assert_eq!(
        recorded(&log),
        (1, 1, Some(true)),
        "a request with no answer"
    );
}

#[test]
fn a_startup_the_server_never_answers_says_it_is_slow_and_that_no_request_was_sent() {
    let dir = Dir::new();
    let listener =
        UnixListener::bind(dir.path().join(crate::server::postgres::SOCKET_FILE)).unwrap();
    let (done, finished) = crate::test_channel::channel::<()>();
    // It takes the connection and its startup, and says nothing, until the client has given up.
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        startup(&mut stream);
        let _ = finished.recv();
    });
    let client = Client::new(dir.path(), "board").waiting(Duration::from_millis(50));
    let log = client.queries();
    let failed = bounded("a startup the server never answers", move || {
        let failed = client.request(&[Step::bare("SELECT 1")]);
        (failed, client.connection.lock().unwrap().is_none())
    });
    done.send(()).unwrap();
    let (Err(DbError::Failed(said)), true) = failed else {
        panic!("a startup never answered fails, and leaves no connection: {failed:?}");
    };
    assert_eq!(
        said,
        format!(
            "the board's PostgreSQL at {} is slow: no answer within 50ms to the connection's \
             startup on {}: {}; the request was not sent",
            dir.path().display(),
            dir.path()
                .join(crate::server::postgres::SOCKET_FILE)
                .display(),
            eagain()
        )
    );
    assert!(crate::rpc::slow(&said), "{said}");
    assert_eq!(
        recorded(&log),
        (1, 1, Some(true)),
        "a request with no answer"
    );
}

#[test]
fn a_socket_no_server_listens_on_refuses_the_connection_which_is_not_slow() {
    let dir = Dir::new();
    // The socket's file stays when its listener goes: a connection to it is refused, as one past a
    // full backlog is on macOS (Linux blocks that one instead, or says EAGAIN without blocking).
    drop(UnixListener::bind(dir.path().join(crate::server::postgres::SOCKET_FILE)).unwrap());
    let client = Client::new(dir.path(), "board");
    let Err(DbError::Failed(said)) = client.request(&[Step::bare("SELECT 1")]) else {
        panic!("a refused connection is a failure");
    };
    assert_eq!(
        said,
        format!(
            "the board's PostgreSQL is not answering on {}: {}",
            dir.path()
                .join(crate::server::postgres::SOCKET_FILE)
                .display(),
            std::io::Error::from_raw_os_error(libc::ECONNREFUSED)
        )
    );
    assert!(!crate::rpc::slow(&said), "{said}");
    assert_eq!(
        recorded(&client.queries()),
        (1, 1, Some(true)),
        "a request with no answer"
    );
}

#[test]
fn a_wait_run_out_is_a_socket_timeout_and_no_other_failure() {
    use std::io::{Error, ErrorKind};
    for why in [
        Error::from_raw_os_error(libc::EAGAIN),
        Error::from_raw_os_error(libc::EWOULDBLOCK),
        Error::from_raw_os_error(libc::ETIMEDOUT),
        Error::from(ErrorKind::WouldBlock),
        Error::from(ErrorKind::TimedOut),
        sending(Error::from_raw_os_error(libc::EAGAIN)),
    ] {
        assert!(waited_out(&why), "{why}");
    }
    for why in [
        Error::from_raw_os_error(libc::ECONNREFUSED),
        Error::from_raw_os_error(libc::ENOENT),
        Error::from_raw_os_error(libc::EPIPE),
        Error::from_raw_os_error(libc::ECONNRESET),
        Error::from_raw_os_error(libc::EBADF),
        Error::from_raw_os_error(libc::EINTR),
        Error::from(ErrorKind::UnexpectedEof),
        Error::from(ErrorKind::InvalidData),
        sending(Error::from_raw_os_error(libc::EPIPE)),
    ] {
        assert!(!waited_out(&why), "{why}");
    }
}

#[test]
fn a_server_gone_after_an_answer_fails_the_next_request_and_drops_its_connection() {
    let (tell, gone) = crate::test_channel::channel();
    let dir = serving(move |mut stream| {
        while wire::read(&mut stream).unwrap().kind != b'S' {}
        stream.write_all(&bytes(JSONB_AND_NULL)).unwrap();
        drop(stream);
        tell.send(()).unwrap();
    });
    let client = Client::new(dir.path(), "board");
    let failed = bounded("a request to a server that has gone", move || {
        client.request(&[Step::bare("SELECT 1")]).unwrap();
        gone.recv().unwrap();
        let failed = client.request(&[Step::bare("SELECT 1")]);
        (failed, client.connection.lock().unwrap().is_none())
    });
    let (Err(DbError::Failed(said)), true) = failed else {
        panic!("a request to a server that has gone fails, and its connection goes: {failed:?}");
    };
    // Either the write or the first read can meet the close. Reconnecting would instead say
    // "is not answering on": this request must fail on the connection it already held.
    assert!(
        said.starts_with(&format!(
            "the board's PostgreSQL at {}: ",
            dir.path().display()
        )),
        "{said}"
    );
    assert!(
        !crate::rpc::slow(&said),
        "a server gone is not slow: {said}"
    );
}

#[test]
fn a_connection_waits_the_clients_wait_on_each_write_as_on_each_read() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![JSONB_AND_NULL],
    }]);
    let wait = Duration::from_secs(7);
    let client = fake.client().waiting(wait);
    client.request(&[Step::bare("SELECT 1")]).unwrap();
    let held = client.connection.lock().unwrap();
    let connection = held.as_ref().expect("the connection is kept");
    assert_eq!(connection.writer.write_timeout().unwrap(), Some(wait));
    assert_eq!(
        connection.stream.get_ref().read_timeout().unwrap(),
        Some(wait)
    );
}

#[test]
fn a_request_the_socket_takes_whole_is_taken_at_once_and_of_a_longer_one_what_it_can_hold() {
    // A peer that reads nothing: what the socket takes is what its buffers hold.
    let (ours, _peer) = UnixStream::pair().unwrap();
    let connection = Connection {
        stream: BufReader::new(ours.try_clone().unwrap()),
        writer: ours,
    };
    let short = [b'S', 0, 0, 0, 4];
    assert_eq!(connection.taken(&short).unwrap(), short.len());
    let long = megabyte().into_bytes();
    let taken = bounded("a write that does not wait", move || {
        connection.taken(&long).unwrap()
    });
    assert!(
        (1..(1 << 20)).contains(&taken),
        "the socket held some and not a megabyte: {taken}"
    );
}

#[test]
fn the_side_of_a_request_that_failed_first_gives_its_error() {
    let failed = |words: &str, first: bool| Err((std::io::Error::other(words.to_string()), first));
    let said = |given: std::io::Result<()>| given.unwrap_err().to_string();
    assert!(given(Ok(()), Ok(())).is_ok());
    assert_eq!(
        said(given(failed("read", true), failed("sent", false))),
        "read"
    );
    assert_eq!(
        said(given(failed("read", false), failed("sent", true))),
        "sent"
    );
    assert_eq!(said(given(Ok(()), failed("sent", true))), "sent");
    assert_eq!(said(given(failed("read", true), Ok(()))), "read");
    assert_eq!(
        said(given(failed("read", false), Ok(()))),
        "read",
        "an error whose side was not first, and no other, is still an error"
    );
}

#[test]
fn no_server_on_the_socket_is_a_failure_naming_it() {
    let dir = Dir::new();
    let Err(DbError::Failed(said)) =
        Client::new(dir.path(), "board").request(&[Step::bare("SELECT 1")])
    else {
        panic!("no server is a failure");
    };
    assert!(
        said.contains("is not answering on") && said.contains(".s.PGSQL.5432"),
        "{said}"
    );
    assert!(!crate::rpc::slow(&said), "no server is not slow: {said}");
}

#[test]
fn an_unexpected_message_breaks_the_connection() {
    // A CopyInResponse, which the client never asks for.
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec!["470000000700000000"],
    }]);
    let Err(DbError::Failed(said)) = fake.client().request(&[Step::bare("COPY t FROM STDIN")])
    else {
        panic!("an unexpected message is a failure");
    };
    assert!(
        said.contains("a message of type 'G' in an answer"),
        "{said}"
    );
}

#[test]
fn a_message_is_read_whole_and_only_at_a_length_it_can_have() {
    let read = |hex: &str| wire::read(&mut bytes(hex).as_slice());
    assert_eq!(
        read("5a0000000549").unwrap(),
        wire::Message {
            kind: b'Z',
            body: vec![b'I']
        }
    );
    for short in ["5a00000003", "5affffffff", "5a7fffffff"] {
        let said = read(short).unwrap_err().to_string();
        assert!(said.contains("bytes long"), "{short}: {said}");
    }
    assert!(read("5a0000000649").is_err(), "a body cut short");
    assert!(read("5a00").is_err(), "a head cut short");
}

#[test]
fn a_data_rows_columns_are_read_to_its_end_and_no_further() {
    assert_eq!(
        wire::row(&bytes("0002000000017affffffff")),
        Some(vec![Some("z".into()), None])
    );
    assert_eq!(wire::row(&bytes("0000")), Some(vec![]));
    assert_eq!(wire::row(&bytes("000100000002")), None, "a value cut short");
    assert_eq!(
        wire::row(&bytes("0001000000017a00")),
        None,
        "bytes after it"
    );
    assert_eq!(wire::row(&bytes("00")), None);
    assert_eq!(wire::row(&bytes("000100000001ff")), None, "not UTF-8");
}

#[test]
fn an_errors_fields_are_read_by_their_codes() {
    assert_eq!(
        wire::fields(b"SERROR\0C42601\0Mbad\0\0"),
        [
            (b'S', "ERROR".to_string()),
            (b'C', "42601".to_string()),
            (b'M', "bad".to_string())
        ]
    );
    assert_eq!(wire::fields(b"Mcut"), [(b'M', "cut".to_string())]);
    assert_eq!(wire::text(b"INSERT 0 1\0"), "INSERT 0 1");
}

#[test]
#[should_panic(expected = "not a plain name")]
fn a_database_is_a_plain_name() {
    let _ = Client::new(Path::new("/tmp"), "board; DROP");
}

#[test]
fn only_a_duplicate_database_or_its_name_index_means_another_maker_won() {
    assert!(database_there("database already exists (SQLSTATE 42P04)"));
    assert!(database_there(
        "duplicate key value violates unique constraint \"pg_database_datname_index\" (SQLSTATE 23505)"
    ));
    for said in [
        "duplicate key value violates unique constraint \"pg_database_oid_index\" (SQLSTATE 23505)",
        "pg_database_datname_index (SQLSTATE 23505)",
        "\"pg_database_datname_index\" (SQLSTATE 42501)",
        "database already exists (SQLSTATE 42P04) more text",
    ] {
        assert!(!database_there(said), "{said}");
    }
}

#[test]
fn making_a_database_copies_template0_from_the_maintenance_database_and_other_refusals_fail() {
    // The recorded syntax error stands for any refusal but the database being there.
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![SYNTAX],
    }]);
    assert_eq!(
        fake.client().make_database(),
        Err(DbError::Failed(
            "syntax error at end of input (SQLSTATE 42601)".into()
        ))
    );
    let (startup, after) = fake.sent();
    let text = String::from_utf8(startup[4..].to_vec()).unwrap();
    let named: Vec<_> = text.split('\0').collect();
    assert_eq!(named[2..4], ["database", MAINTENANCE]);
    let mut sent = after.as_slice();
    let query = wire::read(&mut sent).unwrap();
    assert_eq!(query.kind, b'Q', "one simple query");
    assert_eq!(query.body, b"CREATE DATABASE board TEMPLATE template0\0");
    assert_eq!(kinds(sent), b"X", "and the maker's connection ended");
}

#[test]
fn a_client_says_where_it_reaches() {
    let client = Client::new(Path::new("/tmp/symphony-pg-1"), "board_1");
    assert_eq!(client.addr(), "postgresql:/tmp/symphony-pg-1");
    assert_eq!(client.database(), "board_1");
    assert_eq!(client.of("postgres").database(), "postgres");
    assert_eq!(client.another().socket(), Path::new("/tmp/symphony-pg-1"));
}

#[test]
fn a_bounded_client_waits_the_shorter_of_its_own_wait_and_the_bound_and_keeps_it() {
    let client = Client::new(Path::new("/tmp/symphony-pg-1"), "board_1");
    assert_eq!(client.timeout, TIMEOUT);
    let short = client.bounded(Duration::from_secs(3));
    assert_eq!(short.timeout, Duration::from_secs(3));
    assert_eq!(
        short.bounded(Duration::from_secs(9)).timeout,
        Duration::from_secs(3)
    );
    assert_eq!(short.another().timeout, Duration::from_secs(3));
    assert_eq!(short.of("postgres").timeout, Duration::from_secs(3));
    assert_eq!((short.database(), short.requests()), ("board_1", 0));
}

#[test]
fn request_telemetry_counts_reads_writes_and_unanswered_requests_once_and_is_shared_by_board() {
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![JSONB_AND_NULL, NOTICE, SYNTAX],
    }]);
    let client = fake.client();
    let log = client.queries();
    assert!(Arc::ptr_eq(&log, &client.another().queries()));
    assert!(!Arc::ptr_eq(&log, &client.of("another").queries()));
    client.request(&[Step::bare("SELECT 1")]).unwrap();
    client
        .request(&[Step::bare(
            "DO $$ BEGIN UPDATE task SET title = 'new'; END $$",
        )])
        .unwrap();
    // The server answered it: an error, and no request that went unanswered.
    assert!(client.simple("SELECT 'x' FROM").is_err());
    let (queries, writes) = log.report(crate::sweep::trigger::now());
    assert_eq!((queries.count, queries.failed, writes.count), (3, 0, 1));
    assert_eq!(client.requests(), 3);
    drop(client);
    fake.sent();
}

/// An error the server answered is an answer, which the board's health never counts as a request
/// that went unanswered: a raised exception, a conflict, any other error, and a startup it
/// refused, for a database it does not hold or with a password asked for.
#[test]
fn an_error_the_server_answered_is_no_unanswered_request() {
    let thrown = |said: &Result<Vec<Answer>, DbError>| matches!(said, Err(DbError::Thrown(_)));
    let conflict = |said: &Result<Vec<Answer>, DbError>| said == &Err(DbError::Conflict);
    let failed = |said: &Result<Vec<Answer>, DbError>| matches!(said, Err(DbError::Failed(_)));
    type Judged = fn(&Result<Vec<Answer>, DbError>) -> bool;
    let cases: [(&str, &'static str, Vec<&'static str>, Judged); 5] = [
        (
            "a raised exception",
            STARTED,
            vec![RAISED_AFTER_ONE],
            thrown,
        ),
        (
            "a serialization failure",
            STARTED,
            vec![SERIALIZATION, ROLLED_BACK],
            conflict,
        ),
        ("a syntax error", STARTED, vec![SYNTAX], failed),
        ("a database not there", NO_SUCH_DATABASE, vec![], failed),
        ("a password asked for", PASSWORD, vec![], failed),
    ];
    for (case, started, answers, judged) in cases {
        let fake = Fake::new(vec![Script { started, answers }]);
        let client = fake.client();
        let said = client.request(&[Step::bare("SELECT 1")]);
        assert!(judged(&said), "{case}: {said:?}");
        assert_eq!(recorded(&client.queries()), (1, 0, Some(false)), "{case}");
        drop(client);
        fake.sent();
    }
}

/// Neither of a connection's two descriptors reaches a child forked beside it, even before its
/// exec, so no child can close a copy of its socket and drain it (macOS fails a draining socket's
/// every wait with EBADF); std's socket, as a control, reaches one. Each is close-on-exec, and the
/// connection serves the next request.
#[cfg(target_vendor = "apple")]
#[test]
fn a_connections_socket_never_reaches_a_forked_child() {
    use std::os::fd::AsRawFd as _;
    let fake = Fake::new(vec![Script {
        started: STARTED,
        answers: vec![JSONB_AND_NULL, JSONB_AND_NULL],
    }]);
    let client = fake.client();
    client.request(&[Step::bare("SELECT 1")]).unwrap();
    {
        let held = client.connection.lock().unwrap();
        let connection = held.as_ref().expect("a connection kept");
        let reader = connection.stream.get_ref().as_raw_fd();
        let writer = connection.writer.as_raw_fd();
        assert_ne!(reader, writer, "two descriptors");
        for (side, fd) in [("reader", reader), ("writer", writer)] {
            assert!(
                !crate::sys::a_forked_child_holds(fd),
                "the {side} reached a forked child"
            );
            // SAFETY: F_GETFD reads the flags of a descriptor the connection keeps open.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_eq!(flags & libc::FD_CLOEXEC, libc::FD_CLOEXEC, "the {side}");
        }
    }
    let (control, _peer) = UnixStream::pair().unwrap();
    assert!(
        crate::sys::a_forked_child_holds(control.as_raw_fd()),
        "std's socket reaches a forked child"
    );
    let next = client.request(&[Step::bare("SELECT 1")]).unwrap();
    assert_eq!(next[0].rows, [[Some("{\"a\": 1}".to_string()), None]]);
    drop(client);
    let (_, after) = fake.sent();
    assert_eq!(
        kinds(&after),
        b"PBESPBESX",
        "both requests on the one connection"
    );
}

#[test]
fn an_answers_column_is_the_first_of_every_row_that_gives_one() {
    let row = |first: Option<&str>| vec![first.map(str::to_string), Some("beside".into())];
    let answer = Answer {
        rows: vec![row(Some("a")), row(None), row(Some("b")), vec![]],
        tag: "SELECT 4".into(),
    };
    assert_eq!(answer.column(), ["a", "b"]);
    assert_eq!(answer.value(), Some("a"));
    assert!(Answer::default().column().is_empty());
}
