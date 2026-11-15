//! The fixture's HTTP client: one request per connection to the server under test, its response
//! read whole, and the event feed, as Server-Sent Events or on a WebSocket, read until a marker
//! arrives.

use super::{LIMIT, stalled};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Instant;
use tungstenite::client::IntoClientRequest;

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) address: SocketAddr,
}

#[derive(Debug)]
pub(super) struct Response {
    pub(super) status: u16,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}

impl Response {
    #[track_caller]
    pub(super) fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }
    #[track_caller]
    pub(super) fn json(&self) -> Value {
        match serde_json::from_slice(&self.body) {
            Ok(value) => value,
            Err(error) => panic!("{self:?}: {error}"),
        }
    }
    #[track_caller]
    pub(super) fn header(&self, name: &str) -> &str {
        match self.headers.get(name) {
            Some(value) => value,
            None => panic!("missing {name}: {self:?}"),
        }
    }
    #[track_caller]
    pub(super) fn status(self, status: u16) -> Self {
        assert_eq!(self.status, status, "{self:?}");
        self
    }
}

impl Fixture {
    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect_timeout(&self.address, LIMIT).unwrap();
        stream.set_read_timeout(Some(LIMIT)).unwrap();
        stream.set_write_timeout(Some(LIMIT)).unwrap();
        stream
    }
    pub(super) fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Response {
        let mut stream = self.connect();
        write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            self.address,
            body.len()
        )
        .unwrap();
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").unwrap();
        }
        stream.write_all(b"\r\n").unwrap();
        stream.write_all(body).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let at = bytes.windows(4).position(|s| s == b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&bytes[..at]).unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let headers = header
            .lines()
            .skip(1)
            .map(|line| {
                let (key, value) = line.split_once(':').unwrap();
                (key.to_ascii_lowercase(), value.trim().to_string())
            })
            .collect();
        Response {
            status,
            headers,
            body: bytes[at + 4..].to_vec(),
        }
    }
    pub(super) fn get(&self, path: &str) -> Response {
        self.request("GET", path, &[], &[])
    }
    pub(super) fn post(&self, path: &str, body: &Value) -> Response {
        self.request(
            "POST",
            path,
            &[("Content-Type", "application/json")],
            body.to_string().as_bytes(),
        )
    }
    /// The feed's path in `scope` as a page of SHELF.md opens it: from the board's version, with
    /// its document and sections.
    fn feed_path(&self, scope: &str) -> String {
        let document = self
            .get(&format!("{scope}/api/document?path=docs/SHELF.md"))
            .status(200)
            .json();
        let document_path: String = document["path"]
            .as_str()
            .unwrap()
            .bytes()
            .map(|byte| format!("%{byte:02X}"))
            .collect();
        let keys: Vec<_> = document["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["key"].as_str().unwrap())
            .collect();
        assert!(!keys.is_empty());
        let sections: String = serde_json::to_string(&keys)
            .unwrap()
            .bytes()
            .map(|byte| format!("%{byte:02X}"))
            .collect();
        let version = self.get(&format!("{scope}/api/board")).status(200).json()["version"]
            .as_u64()
            .unwrap();
        format!("{scope}/api/feed?since={version}&document={document_path}&sections={sections}")
    }
    /// The feed in `scope` as Server-Sent Events.
    pub(super) fn feed(&self, scope: &str) -> Feed {
        let path = self.feed_path(scope);
        let mut stream = self.connect();
        write!(
            stream,
            "GET {path} HTTP/1.0\r\nHost: {}\r\n\r\n",
            self.address
        )
        .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HTTP/1.0 200 "), "{line}");
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
        }
        Feed::Events(reader).subscribed()
    }
    /// The feed in `scope` on a WebSocket, as the server's own page opens it: its Origin the
    /// server's.
    pub(super) fn socket(&self, scope: &str) -> Feed {
        let mut request = format!("ws://{}{}", self.address, self.feed_path(scope))
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "origin",
            format!("http://{}", self.address).parse().unwrap(),
        );
        let (socket, answered) = tungstenite::client(request, self.connect()).unwrap();
        assert_eq!(answered.status(), 101, "{answered:?}");
        Feed::Socket(Box::new(socket)).subscribed()
    }
}

/// The feed a test follows: Server-Sent Events read a line at a time, or a WebSocket a message at
/// a time.
pub(super) enum Feed {
    Events(BufReader<TcpStream>),
    Socket(Box<tungstenite::WebSocket<TcpStream>>),
}
impl Feed {
    /// The feed once it said the server's notice, which it says only after it subscribed: no edit
    /// made after falls in a gap before it.
    fn subscribed(mut self) -> Self {
        self.until("server");
        self
    }
    pub(super) fn until(&mut self, marker: &str) {
        let began = Instant::now();
        loop {
            // The stream's read timeout is the backstop: only a stall leaves a line unread.
            let said = self
                .next()
                .unwrap_or_else(|e| panic!("{}", stalled(format!("the feed's {marker}: {e}"))));
            let Some(said) = said else {
                panic!("feed closed before {marker}");
            };
            if said.contains(marker) {
                return;
            }
            assert!(
                began.elapsed() < LIMIT,
                "{}",
                stalled(format!("the feed's {marker}; the last it said: {said}"))
            );
        }
    }
    /// What the feed says next: a line of its events, or a message of its socket (nothing for a
    /// ping or a pong of the socket's own); none once it is closed.
    fn next(&mut self) -> Result<Option<String>, String> {
        match self {
            Self::Events(reader) => {
                let mut line = String::new();
                let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
                Ok((read > 0).then_some(line))
            }
            Self::Socket(socket) => match socket.read() {
                Ok(tungstenite::Message::Text(text)) => Ok(Some(text.to_string())),
                Ok(tungstenite::Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => {
                    Ok(None)
                }
                Ok(tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) => {
                    Ok(Some(String::new()))
                }
                Ok(other) => Err(format!("not a feed's message: {other:?}")),
                Err(e) => Err(e.to_string()),
            },
        }
    }
}
