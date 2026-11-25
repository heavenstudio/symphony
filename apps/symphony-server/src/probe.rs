//! A plain local GET, for the starter and the supervisor, which run no async runtime.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// `GET path` on loopback `port`: the status and the body, or `None` when nothing answers within
/// `timeout`, for the connection and for each read.
pub fn get(port: u16, path: &str, timeout: Duration) -> Option<(u16, String)> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).ok()?;
    let reply = String::from_utf8_lossy(&reply);
    let (head, body) = reply.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, body.to_string()))
}

/// The server in front on `port`, as its own route says: `None` when nothing there answers as one.
pub fn front(port: u16, timeout: Duration) -> Option<symphony_api::Front> {
    let (status, body) = get(port, symphony_api::FRONT_PATH, timeout)?;
    (status == 200)
        .then(|| serde_json::from_slice(body.as_bytes()).ok())
        .flatten()
}
