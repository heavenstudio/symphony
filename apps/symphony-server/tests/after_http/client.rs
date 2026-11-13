//! The fixture's HTTP client: one request per connection to the server under test, its response
//! read whole, and the event feed read line by line until a marker arrives.

use super::{LIMIT, stalled};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Instant;

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
    pub(super) fn feed(&self, scope: &str) -> Feed {
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
        let mut stream = self.connect();
        write!(
            stream,
            "GET {scope}/api/feed?since={version}&document={document_path}&sections={sections} HTTP/1.0\r\nHost: {}\r\n\r\n",
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
        let mut feed = Feed(reader);
        // Server notice is emitted only after the feed has subscribed; there is no edit/read gap.
        feed.until("server");
        feed
    }
}

pub(super) struct Feed(BufReader<TcpStream>);
impl Feed {
    pub(super) fn until(&mut self, marker: &str) {
        let began = Instant::now();
        loop {
            let mut line = String::new();
            // The stream's read timeout is the backstop: only a stall leaves a line unread.
            let read = self
                .0
                .read_line(&mut line)
                .unwrap_or_else(|e| panic!("{}", stalled(format!("the feed's {marker}: {e}"))));
            assert!(read > 0, "feed closed before {marker}");
            if line.contains(marker) {
                return;
            }
            assert!(
                began.elapsed() < LIMIT,
                "{}",
                stalled(format!("the feed's {marker}; its last line: {line}"))
            );
        }
    }
}
