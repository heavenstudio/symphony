//! A board's address ([`Client`]), the words that say a board's server is slow ([`slow`]), and
//! the one HTTP request the board's commands send a server on this machine ([`exchange`]).
//!
//! The address is what a command and a consumer's code hand a store: `postgresql:` and the folder
//! of its server's socket, the database, and how long each answer is waited for; the store opens
//! the board it names ([`crate::Database::named`]) and talks to it through [`crate::pg`].

use crate::db::plain;
use crate::socket::Socket;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// How long a store opened on an address waits for each answer, unless it is told otherwise
/// ([`Client::waiting`]): a whole board's copy, on a loaded machine, is the longest.
const TIMEOUT: Duration = Duration::from_secs(300);

/// A board's address: its server (`postgresql:` and its socket's folder), the namespace and the
/// database it names, and how long each answer is waited for. It sends nothing itself.
#[derive(Debug)]
pub struct Client {
    addr: String,
    ns: String,
    db: String,
    timeout: Duration,
}

impl Client {
    /// `ns` and `db` are names this code chooses (letters, digits, `_`), never a caller's text.
    pub fn new(addr: &str, ns: &str, db: &str) -> Client {
        for name in [ns, db] {
            assert!(
                plain(name),
                "{name:?} is not a plain name for a namespace or database"
            );
        }
        Client {
            addr: addr.into(),
            ns: ns.into(),
            db: db.into(),
            timeout: TIMEOUT,
        }
    }

    /// This address, its board's answers each waited for at most `timeout`.
    pub fn waiting(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// Preserve a caller's request bound when adapting an address to another database engine.
    pub(crate) fn wait_limit(&self) -> Duration {
        self.timeout
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn database(&self) -> &str {
        &self.db
    }

    pub fn namespace(&self) -> &str {
        &self.ns
    }
}

/// What a failure says of a server that took the request on a connection that worked and gave no
/// whole answer within the wait ([`crate::pg`]'s, on a request or a startup).
pub(crate) const SLOW: &str = " is slow: no answer within ";

/// Whether `error` is a request's failure ([`crate::pg::Client`]'s on a request or a startup) on a
/// server that was slow, not unreachable: the connection worked and the answer was late. Whatever
/// wraps it keeps its words.
pub fn slow(error: &str) -> bool {
    error.contains(SLOW)
}

/// Send `request` to `addr` on a connection of its own and read the response to its end. Its socket
/// is the client's kind ([`crate::sys::connect`]), so no spawn copies it during the exchange, and
/// it waits only in `poll` ([`exchanged`]): the one close a drain can still meet, in its connect,
/// leaves it answering as a sound socket does, where a blocking read's EBADF would fail a live
/// server's answer (a field written while the command says the server did not answer, a health
/// probe saying it is not healthy).
pub(crate) fn exchange(
    addr: &str,
    request: &[u8],
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let cannot = |e: std::io::Error| format!("cannot reach the server at {addr}: {e}");
    let to = addr
        .to_socket_addrs()
        .map_err(cannot)?
        .next()
        .ok_or_else(|| format!("{addr} names no address"))?;
    let stream = crate::sys::connect(&to, timeout).map_err(cannot)?;
    let raw = exchanged(stream, request, timeout)
        .map_err(|e| format!("the server at {addr} did not answer: {e}"))?;
    parse_response(&raw)
}

/// `request` written on `stream` and the response read to the end of the connection, each read and
/// each write waiting at most `timeout`, in `poll` ([`crate::socket`]), and a wait that runs out
/// saying `EAGAIN`, as the socket's own timeouts did: a socket macOS drained answers as a sound
/// one does.
fn exchanged(stream: TcpStream, request: &[u8], timeout: Duration) -> std::io::Result<Vec<u8>> {
    stream.set_nodelay(true)?;
    let socket = Socket::waiting(stream, timeout)?;
    (&socket).write_all(request)?;
    let mut raw = Vec::new();
    (&socket).read_to_end(&mut raw)?;
    Ok(raw)
}

/// An HTTP/1.1 response's head: its status, how its body is sent, and whether the server closes
/// the connection after it.
struct Head {
    status: u16,
    length: Option<usize>,
    chunked: bool,
    close: bool,
    /// Where the body starts.
    end: usize,
}

impl Head {
    /// The head `raw` starts with; None while it has not all come.
    fn read(raw: &[u8]) -> Result<Option<Head>, String> {
        let Some(end) = find(raw, b"\r\n\r\n") else {
            return Ok(None);
        };
        let head = String::from_utf8_lossy(&raw[..end]);
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let status = first
            .strip_prefix("HTTP/1.1 ")
            .or_else(|| first.strip_prefix("HTTP/1.0 "))
            .and_then(|rest| rest.get(..3)?.parse::<u16>().ok())
            .ok_or_else(|| format!("not an HTTP response: {head:?}"))?;
        let mut read = Head {
            status,
            length: None,
            chunked: false,
            close: first.starts_with("HTTP/1.0 "),
            end: end + 4,
        };
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            if name == "content-length" {
                read.length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("a Content-Length of {value:?}"))?,
                );
            } else if name == "transfer-encoding" {
                read.chunked = value.eq_ignore_ascii_case("chunked");
            } else if name == "connection" {
                read.close = value.eq_ignore_ascii_case("close");
            }
        }
        Ok(Some(read))
    }
}

/// The status and body of an HTTP/1.1 response read to its end: a `Content-Length` body or a chunked one.
fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let head = Head::read(raw)?.ok_or("the response ended inside its head")?;
    let rest = &raw[head.end..];
    let body = if head.chunked {
        unchunked(rest)?.ok_or("the body ended inside a chunk")?
    } else if let Some(length) = head.length {
        rest.get(..length)
            .ok_or_else(|| format!("the body ended at {} of its {length} bytes", rest.len()))?
            .to_vec()
    } else {
        rest.to_vec()
    };
    Ok((head.status, body))
}

/// A chunked body put back together; None while its last chunk has not come.
fn unchunked(mut rest: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut body = Vec::new();
    loop {
        let Some(line) = find(rest, b"\r\n") else {
            return Ok(None);
        };
        let size = String::from_utf8_lossy(&rest[..line]);
        let size = size.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size, 16).map_err(|_| format!("a chunk size of {size:?}"))?;
        rest = &rest[line + 2..];
        if size == 0 {
            return Ok(rest.starts_with(b"\r\n").then_some(body));
        }
        let Some(chunk) = rest.get(..size) else {
            return Ok(None);
        };
        body.extend_from_slice(chunk);
        let Some(after) = rest.get(size..size + 2) else {
            return Ok(None);
        };
        if after != b"\r\n" {
            return Err("a chunk did not end where its size said".into());
        }
        rest = &rest[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    mod exchange;

    use super::*;

    /// The wait of a sending whose answer is to come in time, where the wait is no part of what
    /// a test is of: the wall-clock backstop of the 5 s those tests once gave, which no loaded
    /// machine runs out before the test's server answers.
    const PATIENT: Duration = test_time::backstop(Duration::from_secs(5));

    /// How long a test's socket read waits for what its peer writes or for the peer to close: the
    /// backstop of the 2 s these tests once gave it.
    const READ_PATIENCE: Duration = test_time::backstop(Duration::from_secs(2));

    #[test]
    fn a_slow_failure_is_known_by_its_words_wherever_it_is_wrapped() {
        let late = "the board's PostgreSQL at /tmp/symphony-pg-1 is slow: no answer within 3s: \
                    Resource temporarily unavailable (os error 35); the request may have run there, \
                    so it was not sent again";
        assert!(slow(late), "{late}");
        let wrapped = format!("the board's database: {late}");
        assert!(slow(&wrapped), "{wrapped}");
        for other in [
            "the board's PostgreSQL at /tmp/symphony-pg-1: Connection refused (os error 61)",
            "the task board is starting",
            "a slow board",
        ] {
            assert!(!slow(other), "{other}");
        }
    }

    #[test]
    fn an_address_keeps_its_server_names_and_wait() {
        let client = Client::new("postgresql:/tmp/symphony-pg-1", "sf", "board_1");
        assert_eq!(
            (client.addr(), client.namespace(), client.database()),
            ("postgresql:/tmp/symphony-pg-1", "sf", "board_1")
        );
        assert_eq!(client.wait_limit(), TIMEOUT);
        let patient = client.waiting(Duration::from_secs(7));
        assert_eq!(patient.wait_limit(), Duration::from_secs(7));
    }

    #[test]
    fn a_response_with_a_length_gives_its_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 4\r\n\r\n[1,2]extra";
        assert_eq!(parse_response(raw), Ok((200, b"[1,2".to_vec())));
    }

    #[test]
    fn a_chunked_response_is_put_back_together() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[1,\r\na;ext=1\r\n2]        \r\n0\r\n\r\n";
        assert_eq!(parse_response(raw), Ok((200, b"[1,2]        ".to_vec())));
    }

    #[test]
    fn a_response_read_to_its_close_has_the_rest_as_its_body() {
        assert_eq!(
            parse_response(b"HTTP/1.1 503 Service Unavailable\r\n\r\nbusy"),
            Ok((503, b"busy".to_vec()))
        );
    }

    #[test]
    fn a_header_line_without_a_colon_is_passed_over() {
        let raw = b"HTTP/1.1 200 OK\r\nnot a header\r\ncontent-length: 2\r\n\r\nok";
        assert_eq!(parse_response(raw), Ok((200, b"ok".to_vec())));
    }

    #[test]
    fn a_cut_off_response_is_refused() {
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\nshort").is_err());
        assert!(
            parse_response(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nab")
                .is_err()
        );
        assert!(parse_response(b"garbage\r\n\r\n").is_err());
    }

    /// A chunked body cut anywhere ends inside a chunk, and one whose chunk is not where its size
    /// says, or whose size is no hexadecimal number, says so.
    #[test]
    fn a_chunked_body_cut_or_malformed_anywhere_is_refused_with_its_reason() {
        let head = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n";
        let with = |body: &[u8]| parse_response(&[head.as_slice(), body].concat());
        for cut in [
            b"3\r\n[1,\r\n2".as_slice(),
            b"3\r\n[1,",
            b"3\r\n[1,\r",
            b"3\r\n[1,\r\n0\r\n",
        ] {
            assert_eq!(
                with(cut),
                Err("the body ended inside a chunk".into()),
                "{:?}",
                String::from_utf8_lossy(cut)
            );
        }
        assert_eq!(
            with(b"3\r\n[1,X\r\n0\r\n\r\n"),
            Err("a chunk did not end where its size said".into())
        );
        assert_eq!(
            with(b"zz\r\n[1,]\r\n0\r\n\r\n"),
            Err("a chunk size of \"zz\"".into())
        );
    }

    #[test]
    fn a_name_that_is_not_a_plain_identifier_is_refused() {
        for bad in ["sf; REMOVE", "", "a-b", "t`x"] {
            assert!(
                std::panic::catch_unwind(|| Client::new("127.0.0.1:1", bad, "tasks")).is_err(),
                "{bad}"
            );
            assert!(
                std::panic::catch_unwind(|| Client::new("127.0.0.1:1", "sf", bad)).is_err(),
                "{bad}"
            );
        }
    }

    /// A server that takes one request, keeps it, and gives `reply`.
    fn one_reply(reply: &'static [u8]) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let Some(end) = find(&request, b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length: usize = head
                    .split("content-length: ")
                    .nth(1)
                    .unwrap()
                    .split("\r\n")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            stream.write_all(reply).unwrap();
            request
        });
        (addr, served)
    }

    /// One request read off `stream` to the end of its body.
    fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0, "the client closed before its request ended");
            request.extend_from_slice(&chunk[..n]);
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
                return request;
            }
        }
    }
}
