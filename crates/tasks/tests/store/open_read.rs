//! A store opens a board at its build's version on one read of it, which defines and writes
//! nothing: over a slow board that read goes once more, as a page's read does, so a page's open
//! and read get their answers after one resend. A board not made yet is still defined, its
//! namespace once.

use crate::common::{Fixture, HOST, NOW, client, server_addr};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphony_board::rpc::{Client, DbError, Kind};
use symphony_board::{Store, cbor};

/// The store's wait in these tests, and how much later than it a held answer comes.
const WAIT: Duration = Duration::from_millis(300);
const LATE: Duration = Duration::from_millis(1500);

type Sent = Arc<Mutex<Vec<Vec<u8>>>>;

/// A loopback relay in front of the board's server that holds back what the server sends on each
/// of its first `held` connections by [`LATE`] (the request reaches the server at once), and keeps
/// what each connection sent.
struct Relay {
    addr: String,
    sent: Sent,
}

impl Relay {
    fn to(server: &str, held: usize) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let sent: Sent = Arc::default();
        let (server, kept) = (server.to_owned(), Arc::clone(&sent));
        std::thread::spawn(move || {
            for caller in listener.incoming().map_while(Result::ok) {
                let connection = {
                    let mut all = kept.lock().unwrap();
                    all.push(Vec::new());
                    all.len() - 1
                };
                let upstream = TcpStream::connect(&server).unwrap();
                let (asked, forwarded) =
                    (caller.try_clone().unwrap(), upstream.try_clone().unwrap());
                let kept = Arc::clone(&kept);
                std::thread::spawn(move || ask(asked, forwarded, &kept, connection));
                std::thread::spawn(move || answer(upstream, caller, connection < held));
            }
        });
        Relay { addr, sent }
    }

    fn client(&self, ns: &str, db: &str) -> Client {
        Client::new(&self.addr, ns, db).waiting(WAIT)
    }

    /// Each request the relay was sent, in order: the connection it came on and its statements.
    fn requests(&self) -> Vec<(usize, String)> {
        let sent = self.sent.lock().unwrap();
        let on = |(connection, raw): (usize, &Vec<u8>)| {
            statements(raw)
                .into_iter()
                .map(move |sql| (connection, sql))
        };
        sent.iter().enumerate().flat_map(on).collect()
    }
}

/// What the caller sends, kept under `connection` and passed on to the server.
fn ask(mut caller: TcpStream, mut server: TcpStream, kept: &Sent, connection: usize) {
    let mut buffer = vec![0; 1 << 16];
    while let Ok(got @ 1..) = caller.read(&mut buffer) {
        kept.lock().unwrap()[connection].extend_from_slice(&buffer[..got]);
        if server.write_all(&buffer[..got]).is_err() {
            break;
        }
    }
    let _ = server.shutdown(Shutdown::Write);
}

/// What the server sends, passed on to the caller, each part [`LATE`] when the connection is held.
fn answer(mut server: TcpStream, mut caller: TcpStream, late: bool) {
    let mut buffer = vec![0; 1 << 16];
    while let Ok(got @ 1..) = server.read(&mut buffer) {
        if late {
            std::thread::sleep(LATE);
        }
        if caller.write_all(&buffer[..got]).is_err() {
            break;
        }
    }
    let _ = caller.shutdown(Shutdown::Write);
}

/// The statements of each whole request in `raw`, one connection's bytes, in order.
fn statements(mut raw: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    while let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
        let length: usize = head
            .split("content-length: ")
            .nth(1)
            .and_then(|rest| rest.split("\r\n").next())
            .map_or(0, |n| n.trim().parse().unwrap());
        let Some(body) = raw.get(end + 4..end + 4 + length) else {
            break;
        };
        let call = cbor::decode(body).unwrap();
        found.push(call["params"][0].as_str().unwrap().to_string());
        raw = &raw[end + 4 + length..];
    }
    found
}

fn open(client: Client) -> Store {
    Store::with(client, Box::new(|| NOW), Box::new(|_| true), HOST).expect("the store opens")
}

/// A namespace no other test makes.
fn fresh_namespace() -> String {
    static NAMESPACES: AtomicUsize = AtomicUsize::new(0);
    format!(
        "open_read_{}_{}",
        std::process::id(),
        NAMESPACES.fetch_add(1, Ordering::SeqCst)
    )
}

#[test]
fn a_page_read_under_a_slow_board_is_answered_after_one_read_resend_and_its_open_writes_nothing() {
    let f = Fixture::empty();
    let made = f.task("p/a");
    let relay = Relay::to(f.store.client().addr(), 1);
    let store = open(relay.client("sf", f.store.client().database()));
    let opened = relay.requests();
    assert_eq!(opened.len(), 2, "one read and its one resend: {opened:?}");
    assert_eq!(opened[0].1, opened[1].1, "the same read went once more");
    assert_eq!(
        (opened[0].0, opened[1].0),
        (0, 1),
        "the resend had a new connection"
    );
    for (_, sql) in &opened {
        assert_eq!(Kind::of(sql), Kind::Read, "the open sent a write: {sql}");
    }

    assert_eq!(store.get("p/a").unwrap().slug, made.slug);
    let read = relay.requests();
    assert_eq!(read.len(), 3, "the page's read, once: {read:?}");
    assert_eq!(read[2].0, 1, "on the connection the resend opened");
}

#[test]
fn a_board_not_made_yet_is_defined_its_namespace_once_and_then_opens_on_one_read() {
    let ns = fresh_namespace();
    let relay = Relay::to(&server_addr(), 0);
    open(relay.client(&ns, "t"));
    let made = relay.requests();
    assert_eq!(Kind::of(&made[0].1), Kind::Read, "it read first: {made:?}");
    let namespaces = made
        .iter()
        .filter(|(_, sql)| sql.contains("DEFINE NAMESPACE"))
        .count();
    assert_eq!(namespaces, 1, "{made:?}");

    let again = Relay::to(&server_addr(), 0);
    open(again.client(&ns, "t"));
    let reopened = again.requests();
    assert_eq!(reopened.len(), 1, "{reopened:?}");
    assert_eq!(Kind::of(&reopened[0].1), Kind::Read);
}

/// `db`'s namespace and database made, with no board in them; again when that meets another test's
/// definition at once in the shared namespace.
fn bare(db: Client) -> Client {
    let made = (0..100).find_map(|_| match db.define_then("RETURN NONE;", json!({})) {
        Err(DbError::Conflict) => {
            std::thread::sleep(Duration::from_millis(50));
            None
        }
        made => Some(made),
    });
    made.expect("the database kept meeting other writes")
        .unwrap();
    db
}

/// A database the server holds with no tables in it, and one not on the server in a namespace that
/// is: each is made a board, the namespace defined only where the database was not there.
#[test]
fn a_database_without_a_board_is_made_one_its_namespace_defined_only_when_missing() {
    Fixture::empty(); // the shared namespace is on the server
    for (db, defines) in [(bare(client()), false), (client(), true)] {
        let relay = Relay::to(db.addr(), 0);
        let store = open(relay.client("sf", db.database()));
        let sent = relay.requests();
        assert_eq!(Kind::of(&sent[0].1), Kind::Read, "{sent:?}");
        let defined = sent.iter().any(|(_, sql)| sql.contains("DEFINE NAMESPACE"));
        assert_eq!(defined, defines, "{sent:?}");
        assert!(store.tasks(None).unwrap().is_empty());

        let again = Relay::to(db.addr(), 0);
        open(again.client("sf", db.database()));
        assert_eq!(again.requests().len(), 1, "the board was made");
    }
}
