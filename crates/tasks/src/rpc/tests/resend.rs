//! A request that got no answer: a write is never sent again; a read goes once more, on a new
//! connection, and never a third time; a late answer says the server is slow, a refused connection
//! that it did not answer.

use super::{SEVEN, none_waiting, read_request, sevens};
use crate::rpc::{Client, DbError, find, slow};
use serde_json::json;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The client's wait in these tests; a late answer comes well after it.
const WAIT: Duration = Duration::from_millis(300);
const LATE: Duration = Duration::from_millis(1200);
const READ: &str = "RETURN 7;";
const WRITE: &str = "CREATE ONLY message:once SET text = $text;";

/// How the test server answers a request it read, in the order the requests come.
#[derive(Clone, Copy)]
enum Answer {
    /// Seven, after this long.
    After(Duration),
    /// Seven's head and half its body, then the connection closed.
    Cut,
    /// Seven a byte at a time, this long before each: every byte well within the wait, the whole
    /// far past it.
    Trickle(Duration),
}

type Answers = Arc<Mutex<VecDeque<Answer>>>;

/// A server that takes every connection and answers each request it reads as `answers` say, in
/// order (seven at once, past their end), and records each request and each connection.
struct Board {
    addr: String,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    connections: Arc<Mutex<usize>>,
}

impl Board {
    fn answering(answers: Vec<Answer>) -> Board {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(Mutex::new(0));
        let answers = Arc::new(Mutex::new(VecDeque::from(answers)));
        let (seen, made) = (Arc::clone(&requests), Arc::clone(&connections));
        std::thread::spawn(move || {
            let callers = listener.incoming().map_while(Result::ok);
            for stream in callers {
                *made.lock().unwrap() += 1;
                let (seen, answers) = (Arc::clone(&seen), Arc::clone(&answers));
                std::thread::spawn(move || serve(stream, &seen, &answers));
            }
        });
        Board {
            addr,
            requests,
            connections,
        }
    }

    fn client(&self) -> Client {
        Client::new(&self.addr, "sf", "t").waiting(WAIT)
    }

    /// The bodies of the requests the server read, in order.
    fn bodies(&self) -> Vec<Vec<u8>> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request[find(request, b"\r\n\r\n").unwrap() + 4..].to_vec())
            .collect()
    }

    fn connections(&self) -> usize {
        *self.connections.lock().unwrap()
    }
}

/// One connection's requests, each recorded in `seen` and answered as the next of `answers` says,
/// until the client closes it or an answer ends it.
fn serve(mut stream: TcpStream, seen: &Mutex<Vec<Vec<u8>>>, answers: &Answers) {
    while let Some(request) = request_on(&mut stream) {
        seen.lock().unwrap().push(request);
        let answer = answers.lock().unwrap().pop_front();
        if !answered(&mut stream, answer.unwrap_or(Answer::After(Duration::ZERO))) {
            return;
        }
    }
}

/// `answer` on `stream`: whether the connection is still open for another request.
fn answered(stream: &mut TcpStream, answer: Answer) -> bool {
    match answer {
        Answer::After(delay) => {
            std::thread::sleep(delay);
            stream.write_all(SEVEN).is_ok()
        }
        Answer::Cut => {
            let _ = stream.write_all(&SEVEN[..SEVEN.len() - 16]);
            false
        }
        Answer::Trickle(gap) => SEVEN.iter().all(|byte| {
            std::thread::sleep(gap);
            stream.write_all(&[*byte]).is_ok()
        }),
    }
}

/// One request read off `stream`, or None once the client has closed it.
fn request_on(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let got = stream.read(&mut chunk).ok().filter(|&got| got > 0)?;
        request.extend_from_slice(&chunk[..got]);
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
            return Some(request);
        }
    }
}

/// A connection `listener` is offered within a second, or None.
fn another(listener: &TcpListener) -> Option<TcpStream> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return None,
        }
    }
}

fn failed(client: &Client, sql: &str) -> String {
    match client.query(sql, json!({"text": "said once"})) {
        Err(DbError::Failed(said)) => said,
        other => panic!("{other:?}"),
    }
}

fn os(code: i32) -> std::io::Error {
    std::io::Error::from_raw_os_error(code)
}

/// What a query that met its timeout says after where it was asked, how long it took and its wait
/// (`failed_query`).
fn cause(said: &str) -> &str {
    let Some((asked, cause)) = said.split_once(", configured wait 300.000ms: ") else {
        panic!("no timeout diagnostic: {said}")
    };
    assert!(asked.starts_with("RPC query at "), "{said}");
    cause
}

#[test]
fn a_read_with_no_answer_in_time_goes_once_more_on_a_new_connection_and_gets_it() {
    let board = Board::answering(vec![Answer::After(Duration::ZERO), Answer::After(LATE)]);
    let client = board.client();
    sevens(&client, 1);
    assert_eq!(client.query(READ, json!({})), Ok(vec![json!(7)]));
    let bodies = board.bodies();
    assert_eq!(bodies.len(), 3, "the first read, then the late one twice");
    assert_eq!(bodies[1], bodies[2], "the same read went once more");
    assert_eq!(board.connections(), 2, "the second sending had a new one");
    assert_eq!(client.kept.lock().unwrap().len(), 1, "the new one is kept");
    assert_eq!(client.requests(), 2, "two queries, however many sendings");
}

#[test]
fn a_write_with_no_answer_in_time_is_never_sent_again_and_says_the_server_is_slow() {
    let board = Board::answering(vec![Answer::After(LATE)]);
    let said = failed(&board.client(), WRITE);
    assert_eq!(
        cause(&said),
        format!(
            "the server at {} is slow: no answer within 300ms: read failed on a freshly opened socket after 0 completed requests: {}; the request may have run there, so it was not sent again",
            board.addr,
            os(libc::EAGAIN)
        )
    );
    assert!(slow(&said), "{said}");
    assert_eq!(board.bodies().len(), 1, "the write was sent again");
    assert_eq!(board.connections(), 1);
}

#[test]
fn a_read_late_again_on_its_resend_fails_without_a_third_and_says_the_server_is_slow() {
    let board = Board::answering(vec![Answer::After(LATE), Answer::After(LATE)]);
    let said = failed(&board.client(), READ);
    let late = format!(
        "read failed on a freshly opened socket after 0 completed requests: {}",
        os(libc::EAGAIN)
    );
    assert_eq!(
        cause(&said),
        format!(
            "the server at {} is slow: no answer within 300ms: {late}; the request only reads, so it went once more, on a new connection: {late}; it was not sent a third time",
            board.addr
        )
    );
    assert!(slow(&said), "{said}");
    assert_eq!(board.bodies().len(), 2, "one sending and one more");
    assert_eq!(board.connections(), 2);
}

/// Each byte of the reply comes well within the wait, which bounds one read; the whole does not,
/// and the wait bounds the whole. A read trickled twice fails after its two waits, long before
/// the trickle ends, so a page asking is answered within its own limit.
#[test]
fn a_reply_trickling_in_past_the_wait_is_late_and_its_read_gives_up_long_before_it_ends() {
    let gap = WAIT / 6;
    let trickle = gap * u32::try_from(SEVEN.len()).unwrap();
    let board = Board::answering(vec![Answer::Trickle(gap), Answer::Trickle(gap)]);
    let began = Instant::now();
    let said = failed(&board.client(), READ);
    let took = began.elapsed();
    assert!(
        took < trickle / 2,
        "{took:?} against a {trickle:?} trickle: {said}"
    );
    assert!(slow(&said), "{said}");
    assert!(said.ends_with("it was not sent a third time"), "{said}");
    assert_eq!(board.bodies().len(), 2, "one sending and one more");
}

/// The first sending is late and the server then goes: the second finds no server, which decides
/// what the failure says.
#[test]
fn a_late_read_whose_resend_is_refused_says_the_server_did_not_answer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let held = std::thread::spawn(move || {
        let (mut taken, _) = listener.accept().unwrap();
        read_request(&mut taken);
        drop(listener);
        taken.set_read_timeout(Some(LATE)).unwrap();
        let _ = taken.read(&mut [0]);
    });
    let said = failed(&Client::new(&addr, "sf", "t").waiting(WAIT), READ);
    assert_eq!(
        cause(&said),
        format!(
            "the server at {addr} did not answer: read failed on a freshly opened socket after 0 completed requests: {}; the request only reads, so it went once more, on a new connection: connect failed on a freshly opened socket after 0 completed requests: {}; it was not sent there",
            os(libc::EAGAIN),
            os(libc::ECONNREFUSED)
        )
    );
    assert!(!slow(&said), "{said}");
    held.join().unwrap();
}

#[test]
fn a_reply_cut_halfway_sends_a_read_once_more_and_a_write_never() {
    let board = Board::answering(vec![Answer::Cut]);
    assert_eq!(board.client().query(READ, json!({})), Ok(vec![json!(7)]));
    assert_eq!(board.bodies().len(), 2);
    assert_eq!(board.connections(), 2);

    let board = Board::answering(vec![Answer::Cut]);
    let said = failed(&board.client(), WRITE);
    assert_eq!(
        said,
        format!(
            "the server at {} did not answer: read failed on a freshly opened socket after 0 completed requests: the body ended at 16 of its 32 bytes; the request may have run there, so it was not sent again",
            board.addr
        )
    );
    assert!(!slow(&said), "{said}");
    assert_eq!(board.bodies().len(), 1);
}

/// A kept connection that took a read and closed without answering: the read goes once more, on a
/// new connection, and gets its answer there (a write it took is never sent again: `failures.rs`).
#[test]
fn a_read_a_kept_connection_took_and_never_answered_goes_once_more_on_a_new_one() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let served = std::thread::spawn(move || {
        let (mut kept, _) = listener.accept().unwrap();
        for _ in 0..3 {
            read_request(&mut kept);
            kept.write_all(SEVEN).unwrap();
        }
        read_request(&mut kept);
        drop(kept); // taken, perhaps run, never answered
        let mut more = another(&listener)?;
        read_request(&mut more);
        more.write_all(SEVEN).unwrap();
        Some(listener)
    });
    let client = Client::new(&addr, "sf", "t").waiting(Duration::from_secs(5));
    sevens(&client, 3);
    assert_eq!(client.query(READ, json!({})), Ok(vec![json!(7)]));
    let listener = served.join().unwrap().expect("a new connection");
    assert!(none_waiting(&listener), "no third connection");
}
