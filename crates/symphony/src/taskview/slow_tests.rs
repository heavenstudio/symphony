//! A task page over a slow board: a read whose answer comes later than the wait goes once more and
//! the page gets its answer; a board late twice is said to be slow, never unreachable; a write that
//! got no answer runs once, a defined function's call however it is spelled.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use symphony_board::rpc::{Client, slow};

use super::store_fixture::{Fixture, NOW, RawWrite, Seed};
use super::{brief_row, page};
use crate::taskview::read_failure;
use crate::test_tree::Tree;

/// The store's wait in these tests, and how much later than it a held answer comes.
const WAIT: Duration = Duration::from_millis(300);
const LATE: Duration = Duration::from_millis(1500);
const UNREACHABLE: &str = "the task board cannot be reached: ";

/// A loopback relay in front of the board's server that holds back what the server sends on each of
/// its first `held` connections by [`LATE`]; the request itself reaches the server at once.
struct Stall {
    addr: String,
    connections: Arc<AtomicUsize>,
}

impl Stall {
    fn to(server: &str, held: usize) -> Stall {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let connections = Arc::new(AtomicUsize::new(0));
        let (server, counted) = (server.to_owned(), Arc::clone(&connections));
        std::thread::spawn(move || {
            let callers = listener.incoming().map_while(Result::ok);
            for caller in callers {
                let late = counted.fetch_add(1, Ordering::SeqCst) < held;
                let upstream = TcpStream::connect(&server).unwrap();
                let (mut asked, mut forwarded) =
                    (caller.try_clone().unwrap(), upstream.try_clone().unwrap());
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut asked, &mut forwarded);
                    let _ = forwarded.shutdown(Shutdown::Write);
                });
                std::thread::spawn(move || {
                    let (mut answer, mut caller) = (upstream, caller);
                    let mut buffer = vec![0; 1 << 16];
                    while let Ok(got @ 1..) = answer.read(&mut buffer) {
                        if late {
                            std::thread::sleep(LATE);
                        }
                        if caller.write_all(&buffer[..got]).is_err() {
                            break;
                        }
                    }
                    let _ = caller.shutdown(Shutdown::Write);
                });
            }
        });
        Stall { addr, connections }
    }

    /// A client of `fixture`'s board over this relay, waiting [`WAIT`].
    fn client(&self, fixture: &Fixture) -> Client {
        client_at(&self.addr, fixture)
    }

    /// A store over this relay waiting [`WAIT`], sending nothing until asked.
    fn store(&self, fixture: &Fixture) -> symphony_board::Store {
        store_over(self.client(fixture))
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

fn client_at(addr: &str, fixture: &Fixture) -> Client {
    Client::new(addr, "sf", fixture.store.client().database()).waiting(WAIT)
}

fn store_over(client: Client) -> symphony_board::Store {
    symphony_board::Store::unchecked(client, Box::new(|| NOW), Box::new(|_| true), "this-host")
}

fn family() -> Fixture {
    Fixture::new(
        Seed::default()
            .task_row(brief_row(1, "fixture/parent", "A parent.", None))
            .task_row(brief_row(
                2,
                "fixture/child",
                "A step.",
                Some("fixture/parent"),
            )),
    )
}

/// `count()` once it is at least `want` or 10 s have gone: the server commits a request it was
/// sent in its own time, and only its answer was held.
fn settled(want: usize, count: impl Fn() -> usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let counted = count();
        if counted >= want || Instant::now() > deadline {
            return counted;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_page_whose_read_is_answered_late_once_gets_the_answer_on_the_one_resend() {
    let fixture = family();
    let record = fixture.store.get("fixture/parent").unwrap();
    let stall = Stall::to(fixture.store.client().addr(), 1);
    let page = page(&stall.store(&fixture), &record, &Tree::new().root).unwrap();
    assert_eq!(page.children_count, 1);
    assert_eq!(stall.connections(), 2, "the late read went once more");
}

/// Late twice is slow, 504; a board that refuses the connection, beside it, cannot be reached, 503.
#[test]
fn a_page_whose_read_is_answered_late_twice_says_the_board_is_slow_not_unreachable() {
    let fixture = family();
    let record = fixture.store.get("fixture/parent").unwrap();
    let stall = Stall::to(fixture.store.client().addr(), usize::MAX);
    let failed = page(&stall.store(&fixture), &record, &Tree::new().root).unwrap_err();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(
        said.starts_with("the task board is slow: SurrealDB: RPC query at "),
        "{said}"
    );
    assert!(
        said.contains(", configured wait 300.000ms: the server at "),
        "{said}"
    );
    assert!(said.contains("it was not sent a third time"), "{said}");
    assert!(!said.contains("cannot be reached"), "{said}");
    assert_eq!(stall.connections(), 2, "one sending and one more, no third");

    let gone = {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        closed.local_addr().unwrap().to_string()
    };
    let refused = store_over(client_at(&gone, &fixture));
    let failed = refused.get("fixture/parent").unwrap_err();
    let (status, said) = read_failure(&failed.to_string(), UNREACHABLE);
    assert_eq!(status, 503, "{said}");
    assert!(
        said.starts_with("the task board cannot be reached: SurrealDB: the server at "),
        "{said}"
    );
    assert!(said.ends_with("the request was not sent"), "{said}");
}

#[test]
fn a_write_whose_answer_is_late_runs_once_and_says_the_board_is_slow() {
    let fixture = family();
    let stall = Stall::to(fixture.store.client().addr(), usize::MAX);
    let failed = stall
        .store(&fixture)
        .say("fixture/parent", "owner", "said once", None)
        .unwrap_err()
        .to_string();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(said.contains("so it was not sent again"), "{said}");
    assert_eq!(stall.connections(), 1, "the write was sent again");
    let said = settled(1, || {
        let thread = fixture.store.thread("fixture/parent").unwrap();
        thread
            .iter()
            .filter(|message| message.text == "said once")
            .count()
    });
    assert_eq!(said, 1, "the write ran once");
}

/// A `RETURN` of a defined function's call, spelled with a space, a line or a comment against its
/// `::`, runs the function on the server just as `fn::bump()` does: each, answered late, is sent
/// once, and the record it counts on goes up by one each time.
#[test]
fn a_late_call_of_a_defined_function_runs_once_however_it_is_spelled() {
    let fixture = family();
    let board = fixture.store.client();
    board
        .write(
            "DEFINE TABLE bump SCHEMALESS; \
             DEFINE FUNCTION fn::bump() { UPSERT bump:count SET n += 1; RETURN 1; };",
            json!({}),
        )
        .unwrap();
    let bumps = || {
        let counted = board.query(
            "RETURN (SELECT VALUE n FROM ONLY bump:count) ?? 0;",
            json!({}),
        );
        usize::try_from(counted.unwrap()[0].as_u64().unwrap()).unwrap()
    };
    let stall = Stall::to(board.addr(), usize::MAX);
    let spellings = [
        "RETURN fn::bump();",
        "RETURN fn ::bump();",
        "RETURN fn\n::bump();",
        "RETURN fn/**/::bump();",
        "RETURN fn -- a comment\n::bump();",
    ];
    let mut sent = 0;
    for sql in spellings {
        sent += 1;
        let failed = stall
            .client(&fixture)
            .query(sql, json!({}))
            .unwrap_err()
            .to_string();
        assert!(
            slow(&failed) && failed.ends_with("so it was not sent again"),
            "{sql:?}: {failed}"
        );
        assert_eq!(stall.connections(), sent, "{sql:?} was sent again");
        assert_eq!(settled(sent, bumps), sent, "{sql:?} ran other than once");
    }
}
