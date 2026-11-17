//! The messages of PostgreSQL's frontend/backend protocol 3.0 that the board's client sends and
//! reads, and nothing else: each one a frontend sends made whole in a buffer, and each one the
//! server sends read as its type and its body.

use std::io::{self, Read};

/// The protocol's version in the startup message: 3.0.
const PROTOCOL: i32 = 196_608;
/// The type of the one parameter a statement takes: `jsonb`.
pub(super) const JSONB: i32 = 3802;
/// The longest message the client reads: a board's whole dump comes back in one, under this.
pub(super) const LONGEST: usize = 1 << 30;

/// The startup message: the protocol's version, then each of `params` as a name and its value.
pub(super) fn startup(params: &[(&str, &str)]) -> Vec<u8> {
    let mut body = PROTOCOL.to_be_bytes().to_vec();
    for (name, value) in params {
        cstring(&mut body, name);
        cstring(&mut body, value);
    }
    body.push(0);
    let mut message = len(body.len() + 4).to_vec();
    message.extend(body);
    message
}

/// A simple query: `sql`, which may hold many statements and takes no parameter.
pub(super) fn query(out: &mut Vec<u8>, sql: &str) {
    let mut body = Vec::new();
    cstring(&mut body, sql);
    framed(out, b'Q', &body);
}

/// The unnamed statement parsed from `sql`, its one parameter typed `jsonb` when it `takes` one.
pub(super) fn parse(out: &mut Vec<u8>, sql: &str, takes: bool) {
    let mut body = Vec::new();
    cstring(&mut body, "");
    cstring(&mut body, sql);
    if takes {
        body.extend(1i16.to_be_bytes());
        body.extend(JSONB.to_be_bytes());
    } else {
        body.extend(0i16.to_be_bytes());
    }
    framed(out, b'P', &body);
}

/// The unnamed portal over the unnamed statement, `param` its one parameter as text when it has
/// one, every result column asked for as text.
pub(super) fn bind(out: &mut Vec<u8>, param: Option<&str>) {
    let mut body = Vec::new();
    cstring(&mut body, "");
    cstring(&mut body, "");
    body.extend(0i16.to_be_bytes());
    match param {
        Some(param) => {
            body.extend(1i16.to_be_bytes());
            body.extend(len(param.len()));
            body.extend(param.as_bytes());
        }
        None => body.extend(0i16.to_be_bytes()),
    }
    body.extend(0i16.to_be_bytes());
    framed(out, b'B', &body);
}

/// The unnamed portal run to its end.
pub(super) fn execute(out: &mut Vec<u8>) {
    let mut body = Vec::new();
    cstring(&mut body, "");
    body.extend(0i32.to_be_bytes());
    framed(out, b'E', &body);
}

/// The end of a request: the server answers what came before, ends its implicit transaction and
/// says it is ready.
pub(super) fn sync(out: &mut Vec<u8>) {
    framed(out, b'S', &[]);
}

/// The end of the connection.
pub(super) fn terminate(out: &mut Vec<u8>) {
    framed(out, b'X', &[]);
}

fn framed(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    out.push(kind);
    out.extend(len(body.len() + 4));
    out.extend(body);
}

fn cstring(out: &mut Vec<u8>, text: &str) {
    assert!(
        !text.as_bytes().contains(&0),
        "a protocol string holds no zero byte"
    );
    out.extend(text.as_bytes());
    out.push(0);
}

fn len(n: usize) -> [u8; 4] {
    i32::try_from(n)
        .expect("a message the board sends is under 2 GiB")
        .to_be_bytes()
}

/// A message the server sent: its type, and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Message {
    pub kind: u8,
    pub body: Vec<u8>,
}

/// The next message on `from`: an error when the stream ends inside one, or names a length no
/// message has, or one over [`LONGEST`].
pub(super) fn read(from: &mut impl Read) -> io::Result<Message> {
    let mut head = [0u8; 5];
    from.read_exact(&mut head)?;
    let kind = head[0];
    let length = i32::from_be_bytes([head[1], head[2], head[3], head[4]]);
    let length = usize::try_from(length)
        .ok()
        .filter(|n| (4..=LONGEST).contains(n))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "a message of type {:?} says it is {length} bytes long",
                    char::from(kind)
                ),
            )
        })?;
    let mut body = vec![0u8; length - 4];
    from.read_exact(&mut body)?;
    Ok(Message { kind, body })
}

/// What the server said of an error or a notice: each field by its code (`C` the SQLSTATE, `M`
/// the message, `D` its detail).
pub(super) fn fields(body: &[u8]) -> Vec<(u8, String)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < body.len() && body[at] != 0 {
        let code = body[at];
        let rest = &body[at + 1..];
        let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        found.push((code, String::from_utf8_lossy(&rest[..end]).into_owned()));
        at += 1 + end + 1;
    }
    found
}

/// A data row's columns, each its text or null: none when the body is not a data row's shape.
pub(super) fn row(body: &[u8]) -> Option<Vec<Option<String>>> {
    let count = u16::from_be_bytes([*body.first()?, *body.get(1)?]);
    let mut at = 2;
    let mut columns = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let bytes: [u8; 4] = body.get(at..at + 4)?.try_into().ok()?;
        let length = i32::from_be_bytes(bytes);
        at += 4;
        if length < 0 {
            columns.push(None);
            continue;
        }
        let length = usize::try_from(length).ok()?;
        let value = body.get(at..at + length)?;
        columns.push(Some(String::from_utf8(value.to_vec()).ok()?));
        at += length;
    }
    (at == body.len()).then_some(columns)
}

/// A command's tag (`INSERT 0 1`), or a parameter's name: the string a body starts with.
pub(super) fn text(body: &[u8]) -> String {
    let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).into_owned()
}
