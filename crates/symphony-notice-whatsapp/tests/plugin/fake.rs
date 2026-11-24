//! The Graph API stood in for: a server on a private port of the loopback that answers every
//! request with one status and body and hands the test each request it read. No test reaches Meta.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

/// A request as the stand-in read it.
pub(crate) struct Request {
    /// Its request line, `POST /v27.0/… HTTP/1.1`.
    pub(crate) line: String,
    /// Its headers by lowercase name.
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: String,
}

pub(crate) struct Fake {
    /// The API's URL to name in the settings, its version in it.
    pub(crate) api: String,
    requests: Receiver<Request>,
}

impl Fake {
    /// A stand-in that answers `status` and `body` to every request.
    pub(crate) fn answering(status: u16, body: &str) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a private port");
        let port = listener.local_addr().expect("its address").port();
        let (sent, requests) = mpsc::channel();
        let answer = format!(
            "HTTP/1.1 {status} Answered\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(request) = read(&mut stream) else {
                    continue;
                };
                if sent.send(request).is_err() {
                    return;
                }
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        Fake {
            api: format!("http://127.0.0.1:{port}/v27.0"),
            requests,
        }
    }

    /// The next request it read, waited for: only a stall reaches the backstop.
    pub(crate) fn request(&self) -> Request {
        self.requests
            .recv_timeout(Duration::from_secs(60))
            .expect("a request reached the stand-in")
    }

    /// Whether it read no request, asked once the program has ended: a request is read before it is
    /// answered, so every request the program made is here by then.
    pub(crate) fn read_none(&self) -> bool {
        self.requests.try_recv().is_err()
    }
}

/// The request on `stream`: its line, its headers and the body its `Content-Length` says.
fn read(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let length = (headers.get("content-length"))
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        line: line.trim_end().to_string(),
        headers,
        body: String::from_utf8(body).ok()?,
    })
}
