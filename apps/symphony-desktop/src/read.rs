//! One read of a server's project list, `GET /api/projects`: a plain HTTP/1.0 request over a
//! socket of std's, as `symphony project add` reads `/api/front`, so the app needs no HTTP client.
//! The answer is read from the server's memory, so a read every few seconds costs it nothing.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use symphony_api::{PROJECTS_PATH, ProjectList};

use crate::address::Server;

/// The most of an answer read: a list is a few kilobytes, so more is no Symphony's.
const MOST: u64 = 4 << 20;

/// What a server said when its list was asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Its list.
    Listed(ProjectList),
    /// It answered as a server, but has no `/api/projects`: an older Symphony, whose pages still
    /// open, with no count.
    Older,
    /// No list: it did not answer, or answered something that is no Symphony's list. Why, in words.
    Lost(String),
}

/// `server`'s list, each step (the connection, the request, the answer) given `wait`.
pub fn projects(server: &Server, wait: Duration) -> Answer {
    match ask(server, wait) {
        Ok(answer) => answer,
        Err(why) => Answer::Lost(why),
    }
}

fn ask(server: &Server, wait: Duration) -> Result<Answer, String> {
    let (host, port) = server.connect_to();
    let addresses: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("{host} is not found: {error}"))?
        .collect();
    let mut last = format!("{host} has no address");
    let mut stream = None;
    for address in &addresses {
        match TcpStream::connect_timeout(address, wait) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(error) => last = format!("{server} does not answer: {error}"),
        }
    }
    let mut stream = stream.ok_or(last)?;
    let fail = |error: std::io::Error| format!("{server} stopped answering: {error}");
    stream.set_read_timeout(Some(wait)).map_err(fail)?;
    stream.set_write_timeout(Some(wait)).map_err(fail)?;
    write!(
        stream,
        "GET {PROJECTS_PATH} HTTP/1.0\r\nHost: {}:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        server.host()
    )
    .map_err(fail)?;
    let mut answer = Vec::new();
    stream.take(MOST).read_to_end(&mut answer).map_err(fail)?;
    Ok(heard(server, &answer))
}

/// What the bytes a server sent back say.
fn heard(server: &Server, answer: &[u8]) -> Answer {
    let Some(split) = answer.windows(4).position(|four| four == b"\r\n\r\n") else {
        return Answer::Lost(format!("{server} sent no HTTP answer"));
    };
    let (head, body) = (&answer[..split], &answer[split + 4..]);
    let status = std::str::from_utf8(head)
        .ok()
        .and_then(|head| head.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok());
    match status {
        Some(200) => match serde_json::from_slice::<ProjectList>(body) {
            Ok(list) => Answer::Listed(list),
            Err(error) => Answer::Lost(format!("{server} answers no Symphony's list: {error}")),
        },
        Some(404) => Answer::Older,
        Some(code) => Answer::Lost(format!("{server} answered {code} for its list")),
        None => Answer::Lost(format!("{server} sent no HTTP answer")),
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
