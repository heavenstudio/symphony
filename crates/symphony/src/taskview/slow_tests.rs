//! A task page over a slow board: a read whose answer comes later than the wait goes once more and
//! the page gets its answer; a board late twice is said to be slow, never unreachable; a write that
//! got no answer runs once.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::store_fixture::{Fixture, NOW, Seed};
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
            for caller in listener.incoming().map_while(Result::ok) {
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
                    while let Ok(n @ 1..) = answer.read(&mut buffer) {
                        if late {
                            std::thread::sleep(LATE);
                        }
                        if caller.write_all(&buffer[..n]).is_err() {
                            break;
                        }
                    }
                    let _ = caller.shutdown(Shutdown::Write);
                });
            }
        });
        Stall { addr, connections }
    }

    /// A store over this relay waiting [`WAIT`], sending nothing until asked.
    fn store(&self, f: &Fixture) -> symphony_board::Store {
        let client =
            symphony_board::rpc::Client::new(&self.addr, "sf", f.store.client().database())
                .waiting(WAIT);
        symphony_board::Store::unchecked(client, Box::new(|| NOW), Box::new(|_| true), "this-host")
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
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

#[test]
fn a_page_whose_read_is_answered_late_once_gets_the_answer_on_the_one_resend() {
    let f = family();
    let record = f.store.get("fixture/parent").unwrap();
    let stall = Stall::to(f.store.client().addr(), 1);
    let page = page(&stall.store(&f), &record, &Tree::new().root).unwrap();
    assert_eq!(page.children_count, 1);
    assert_eq!(stall.connections(), 2, "the late read went once more");
}

#[test]
fn a_page_whose_read_is_answered_late_twice_says_the_board_is_slow() {
    let f = family();
    let record = f.store.get("fixture/parent").unwrap();
    let stall = Stall::to(f.store.client().addr(), usize::MAX);
    let failed = page(&stall.store(&f), &record, &Tree::new().root).unwrap_err();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(
        said.starts_with("the task board is slow: SurrealDB: the server at "),
        "{said}"
    );
    assert!(said.contains("it was not sent a third time"), "{said}");
    assert!(!said.contains("cannot be reached"), "{said}");
    assert_eq!(stall.connections(), 2, "one sending and one more, no third");
}

#[test]
fn a_write_whose_answer_is_late_runs_once_and_says_the_board_is_slow() {
    let f = family();
    let stall = Stall::to(f.store.client().addr(), usize::MAX);
    let failed = stall
        .store(&f)
        .say("fixture/parent", "owner", "said once", None)
        .unwrap_err()
        .to_string();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(said.contains("so it was not sent again"), "{said}");
    assert_eq!(stall.connections(), 1, "the write was sent again");
    // The server took the write at once and commits it in its own time; only its answer was held.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let said = loop {
        let said = f
            .store
            .thread("fixture/parent")
            .unwrap()
            .into_iter()
            .filter(|message| message.text == "said once")
            .count();
        if said > 0 || std::time::Instant::now() > deadline {
            break said;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(said, 1, "the write ran once");
}

#[test]
fn a_board_that_refuses_the_connection_cannot_be_reached() {
    let addr = {
        let gone = TcpListener::bind("127.0.0.1:0").unwrap();
        gone.local_addr().unwrap().to_string()
    };
    let store = symphony_board::Store::unchecked(
        symphony_board::rpc::Client::new(&addr, "sf", "t").waiting(WAIT),
        Box::new(|| NOW),
        Box::new(|_| true),
        "this-host",
    );
    let failed = store.get("fixture/parent").unwrap_err().to_string();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 503, "{said}");
    assert!(
        said.starts_with("the task board cannot be reached: SurrealDB: the server at "),
        "{said}"
    );
    assert!(said.ends_with("the request was not sent"), "{said}");
}
