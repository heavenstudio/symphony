//! What the tests stand in for: Slack's Web API, answering one call on a private loopback port
//! and keeping what it heard, so no test reaches Slack or sends anyone a message; and a machine
//! folder of a test's own.

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// A token no Slack issues.
pub(crate) const TOKEN: &str = "xoxb-stand-in-0000-TOKEN";

/// The longest a test waits for what the stand-in heard of a call that has already ended: only a
/// stall reaches it.
const BACKSTOP: Duration = Duration::from_secs(60);

/// One call the stand-in heard: its request line and headers, then its body.
pub(crate) struct Heard {
    pub(crate) head: Vec<String>,
    pub(crate) body: String,
}

impl Heard {
    /// The header `name`'s value, its name read in any case.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.head.iter().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then_some(value.trim())
        })
    }
}

/// The Web API's stand-in, answering its one call.
pub(crate) struct Slack {
    /// The API's base, as [`crate::API`] is Slack's.
    pub(crate) api: String,
    heard: Receiver<Heard>,
}

impl Slack {
    /// A stand-in that answers its one call with `status` and `body`.
    pub(crate) fn answering(status: u16, body: &str) -> Slack {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!(
            "http://127.0.0.1:{}/api",
            listener.local_addr().unwrap().port()
        );
        let answer = format!(
            "HTTP/1.1 {status} Stand-in\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (tell, heard) = channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    break;
                }
                head.push(line);
            }
            let mut call = Heard {
                head,
                body: String::new(),
            };
            let length = call
                .header("content-length")
                .map_or(0, |n| n.parse().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            call.body = String::from_utf8(body).unwrap();
            // Told before it answers, so a call that has its answer was heard.
            tell.send(call).unwrap();
            reader.into_inner().write_all(answer.as_bytes()).unwrap();
        });
        Slack { api, heard }
    }

    /// The call it heard, once the caller has its answer.
    pub(crate) fn heard(&self) -> Heard {
        self.heard
            .recv_timeout(BACKSTOP)
            .expect("the stand-in heard a call")
    }
}

/// An API base on a loopback port nothing listens on: a call to it reaches no host.
pub(crate) fn unanswered() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}/api")
}

/// An empty folder of the test's own, named for `name`.
pub(crate) fn folder(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "symphony-notice-slack-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `token` kept in the machine folder `machine` as the program keeps it: `notices/slack.token`,
/// the folder mode 700 and the file 600.
pub(crate) fn keep(machine: &Path, token: &str) {
    let notices = machine.join(crate::NOTICES);
    fs::create_dir_all(&notices).unwrap();
    fs::set_permissions(&notices, fs::Permissions::from_mode(0o700)).unwrap();
    let file = notices.join(crate::TOKEN_FILE);
    fs::write(&file, token).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
}
