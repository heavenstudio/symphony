//! Query timeout diagnostics and transport failures that must not resend a request.

use super::{PATIENT, SEVEN, none_waiting, one_reply, read_request, sevens};
use crate::cbor;
use crate::rpc::{Client, DbError, failed_query, find, talk};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// A write: a request that got no answer is never sent again ([`crate::rpc::Kind`]).
const WRITE: &str = "CREATE ONLY message:once SET text = 'said once';";

#[test]
fn a_query_failure_names_its_source_elapsed_time_wait_and_original_cause() {
    assert_eq!(
        failed_query(
            "store/callbacks.rs:734",
            Duration::from_millis(3125),
            Duration::from_secs(3),
            "read failed; the request may have run there, so it was not sent again",
        ),
        "RPC query at store/callbacks.rs:734, elapsed 3.125s, configured wait 3.000s: read failed; the request may have run there, so it was not sent again"
    );
}

/// The server says nothing until the client lets go of it, or until [`PATIENT`] has passed, when it
/// closes the connection: the client's failure says which, and only the wait it was told ends it so.
#[test]
fn a_client_waits_on_a_silent_server_no_longer_than_it_was_told() {
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let held = std::thread::spawn(move || {
        let (mut taken, _) = silent.accept().unwrap();
        taken.set_read_timeout(Some(PATIENT)).unwrap();
        while matches!(taken.read(&mut [0; 4096]), Ok(read) if read > 0) {}
    });
    let client = Client::new(&addr, "sf", "t").waiting(Duration::from_millis(300));
    let failed = client.query(WRITE, json!({})).unwrap_err();
    let said = failed.to_string();
    assert!(
        said.contains(&format!(
            "configured wait 300.000ms: the server at {addr} is slow: no answer within 300ms: "
        )),
        "{said}"
    );
    held.join().unwrap();
}

/// A kept connection that took a request and closed without answering may have run it (a write
/// committed, then the server went): the request fails, and is never sent again, which would run a
/// write twice.
#[test]
fn a_request_a_kept_connection_took_and_never_answered_is_not_sent_again() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (queried, query_ended) = crate::test_channel::channel::<()>();
    let served = std::thread::spawn(move || {
        let (mut kept, _) = listener.accept().unwrap();
        for _ in 0..3 {
            read_request(&mut kept);
            kept.write_all(SEVEN).unwrap();
        }
        read_request(&mut kept);
        drop(kept); // taken, perhaps run, never answered
        // Once the client's query has failed, a request sent again has connected already: the
        // kernel queues a connection before it is accepted.
        let _ = query_ended.recv();
        listener.set_nonblocking(true).unwrap();
        listener.accept().is_ok()
    });
    let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
    sevens(&client, 3);
    let failed = client.query(WRITE, json!({}));
    drop(queried);
    let Err(DbError::Failed(said)) = failed else {
        panic!("{failed:?}")
    };
    assert_eq!(
        said,
        format!(
            "the server at {addr} did not answer: read failed on a pooled socket after 3 completed requests: it closed the connection; the request may have run there, so it was not sent again"
        )
    );
    assert!(!served.join().unwrap(), "the request was sent again");
}

#[test]
fn a_kept_requests_read_timeout_never_resends_a_write_or_keeps_its_socket() {
    for partial in [
        &b""[..],
        &b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\n\r\n\xa2"[..],
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let client = Client::new(&addr, "sf", "t").waiting(Duration::from_millis(300));
        let mut connection = client.connect().unwrap();
        let (mut kept, _) = listener.accept().unwrap();
        // Pool history is fixture state; this case sends only the uncertain RPC.
        connection.served = 15;
        client.requests.store(15, Ordering::SeqCst);
        client.kept.lock().unwrap().push(connection);
        let served = std::thread::spawn(move || {
            let request = read_request(&mut kept);
            kept.write_all(partial).unwrap();
            assert_eq!(kept.read(&mut [0]).unwrap(), 0, "no repeated request");
            assert!(none_waiting(&listener), "no replacement connection");
            request
        });
        let sql = "CREATE ONLY task:once SET title = $title;";
        let vars = json!({"title": "one owner write"});
        let called_at = format!("{}:{}", file!(), line!() + 1);
        let failed = client.query(sql, vars.clone()).unwrap_err();
        let DbError::Failed(said) = failed else {
            panic!("{failed:?}")
        };
        let timing = said
            .strip_prefix(&format!("RPC query at {called_at}, elapsed "))
            .expect("the diagnostic identifies the query call site");
        let (elapsed, cause) = timing
            .split_once(", configured wait 300.000ms: ")
            .expect("the diagnostic retains the configured wait");
        assert!(!elapsed.is_empty(), "elapsed time is present");
        assert_eq!(
            cause,
            format!(
                "the server at {addr} is slow: no answer within 300ms: read failed on a pooled socket after 15 completed requests: {}; the request may have run there, so it was not sent again",
                std::io::Error::from_raw_os_error(libc::EAGAIN)
            )
        );
        assert!(client.kept.lock().unwrap().is_empty());
        assert_eq!(client.requests(), 16);
        let request = served.join().unwrap();
        let body = &request[find(&request, b"\r\n\r\n").unwrap() + 4..];
        assert_eq!(
            cbor::decode(body),
            Ok(json!({"id": 1, "method": "query", "params": [sql, vars]}))
        );
    }
}

#[test]
fn a_fresh_sockets_eof_names_read_and_zero_completed_requests() {
    let (addr, served) = one_reply(b"");
    let client = Client::new(&addr, "sf", "t").waiting(PATIENT);
    assert_eq!(
        client.query(WRITE, json!({})),
        Err(DbError::Failed(format!(
            "the server at {addr} did not answer: read failed on a freshly opened socket after 0 completed requests: it closed the connection; the request may have run there, so it was not sent again"
        )))
    );
    served.join().unwrap();
}

#[test]
fn socket_option_and_write_failures_name_the_actual_operation() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let mut stream = TcpStream::connect(&addr).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    let client = Client::new(&addr, "sf", "t");
    let failure = talk(&mut stream, b"request", Duration::ZERO).err().unwrap();
    assert_eq!(
        client.unanswered(false, 0, failure),
        format!(
            "the server at {addr} did not answer: socket option SO_RCVTIMEO failed on a freshly opened socket after 0 completed requests: cannot set a 0 duration timeout; the request was not sent"
        )
    );
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let failure = talk(&mut stream, b"request", Duration::from_secs(5))
        .err()
        .unwrap();
    assert_eq!(
        client.unanswered(false, 0, failure),
        format!(
            "the server at {addr} did not answer: write failed on a freshly opened socket after 0 completed requests: {}; the request may have run there, so it was not sent again",
            std::io::Error::from_raw_os_error(libc::EPIPE)
        )
    );
    assert_eq!(
        peer.read(&mut [0]).unwrap(),
        0,
        "neither failure sent a request"
    );
}
