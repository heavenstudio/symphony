use std::net::TcpListener;
use std::thread;

use super::*;

/// The fixture list the wire's own test pins (`crates/symphony-api/fixtures/projects.json`).
const LISTED: &str = include_str!("../../../crates/symphony-api/fixtures/projects.json");

/// A stand-in on a free loopback port that answers one request with `answer`, and the request it
/// read, once it has been read.
fn answering(answer: impl Into<Vec<u8>>) -> (Server, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let answer = answer.into();
    let served = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream.write_all(&answer).unwrap();
        String::from_utf8(request).unwrap()
    });
    (
        Server::parse(&format!("http://127.0.0.1:{port}")).unwrap(),
        served,
    )
}

const WAIT: Duration = Duration::from_secs(120);

#[test]
fn a_servers_list_is_read_with_one_request_naming_its_host() {
    let answer = format!("HTTP/1.0 200 OK\r\ncontent-type: application/json\r\n\r\n{LISTED}");
    let (server, served) = answering(answer);
    let Answer::Listed(list) = projects(&server, WAIT) else {
        panic!("the fixture list was not read")
    };
    assert_eq!(list, serde_json::from_str::<ProjectList>(LISTED).unwrap());
    assert_eq!(list.projects.len(), 3);
    let request = served.join().unwrap();
    assert_eq!(
        request,
        format!(
            "GET /api/projects HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            server.port()
        )
    );
}

#[test]
fn a_server_without_the_list_is_older_and_any_other_answer_is_lost_saying_why() {
    let (server, _) = answering(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
    assert_eq!(projects(&server, WAIT), Answer::Older);
    for (answer, said) in [
        (
            &b"HTTP/1.1 403 Forbidden\r\n\r\nno"[..],
            "answered 403 for its list",
        ),
        (
            b"HTTP/1.1 200 OK\r\n\r\n{\"projects\": 3}",
            "answers no Symphony's list",
        ),
        (
            b"HTTP/1.1 200 OK\r\n\r\n<html>",
            "answers no Symphony's list",
        ),
        (b"SSH-2.0-OpenSSH_9.9\r\n", "sent no HTTP answer"),
        (b"garbage\r\n\r\n", "sent no HTTP answer"),
        (b"", "sent no HTTP answer"),
    ] {
        let (server, _) = answering(answer);
        match projects(&server, WAIT) {
            Answer::Lost(why) => assert!(
                why.starts_with(&format!("http://127.0.0.1:{}", server.port()))
                    && why.contains(said),
                "{why}"
            ),
            other => panic!("{said}: read as {other:?}"),
        }
    }
}

#[test]
fn a_server_that_is_not_there_is_lost() {
    // A port bound and released at once: nothing listens there now.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let server = Server::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    match projects(&server, WAIT) {
        Answer::Lost(why) => assert!(why.contains("does not answer"), "{why}"),
        other => panic!("read as {other:?}"),
    }
}
