//! The case's rendezvous with its build, startup and output stand-ins: a Unix socket at a pathname
//! only this case's generations are given. A loopback TCP port was a number any process on the
//! machine could reach once the system handed it out again: a client of an ended server sent its
//! HTTP request there, and another run's stand-in could have sent its handshake.

use super::LIMIT;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(super) struct Gate {
    listener: UnixListener,
    path: PathBuf,
}

impl Gate {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // macOS holds 104 bytes of a socket's path, fewer than a case's scratch folder takes, so
        // the socket is in /tmp under a name no other gate, run or process is given.
        let path = PathBuf::from("/tmp").join(format!(
            "symphony-after-reload-{}-{}-{unique}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self { listener, path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn entered(&self, kind: &str) -> Entry {
        self.entered_within(kind, LIMIT)
    }

    /// The next handshake, which must be a `kind` one. A connection whose first line is no
    /// handshake (`KIND PID`) is set aside and named if the handshake never comes; a handshake of
    /// another kind fails, since only this case's generations hold the path.
    pub(super) fn entered_within(&self, kind: &str, limit: Duration) -> Entry {
        let began = Instant::now();
        let mut strays = Vec::new();
        loop {
            let mut stream = self.accepted(kind, limit, began, &strays);
            // macOS inherits O_NONBLOCK from the accepting listener.
            stream.set_nonblocking(false).unwrap();
            // A connection that never writes holds the wait only until the deadline.
            let remaining = limit.saturating_sub(began.elapsed());
            match stream.set_read_timeout(Some(remaining.max(Duration::from_millis(1)))) {
                // macOS refuses the option once the peer has closed; a read then cannot block.
                Err(e) if e.kind() != std::io::ErrorKind::InvalidInput => panic!("{kind}: {e}"),
                _ => {}
            }
            let mut line = String::new();
            if let Err(e) = BufReader::new(&mut stream).read_line(&mut line) {
                panic!(
                    "no {kind} handshake within {limit:?}: a connection sent no line: {e}; set aside: {strays:?}; {}",
                    test_time::Load::now()
                );
            }
            match handshake(&line) {
                Some((found, pid)) => {
                    assert_eq!(found, kind, "{line}");
                    return Entry { stream, pid };
                }
                None => strays.push(line),
            }
        }
    }

    fn accepted(
        &self,
        kind: &str,
        limit: Duration,
        began: Instant,
        strays: &[String],
    ) -> UnixStream {
        loop {
            let mut fd = libc::pollfd {
                fd: self.listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let remaining = limit.saturating_sub(began.elapsed());
            let timeout = i32::try_from(remaining.as_millis()).unwrap();
            // SAFETY: poll receives one live descriptor and a writable pollfd for this call.
            let ready = unsafe { libc::poll(&mut fd, 1, timeout) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                panic!("waiting for a {kind} handshake: {error}");
            }
            // poll sets no error when it times out, so the last one would name an unrelated call.
            assert!(
                ready > 0,
                "no {kind} handshake within {limit:?}: the deadline passed; set aside: {strays:?}; {}",
                test_time::Load::now()
            );
            match self.listener.accept() {
                Ok((stream, _)) => return stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        began.elapsed() < limit,
                        "no {kind} handshake within {limit:?}; set aside: {strays:?}; {}",
                        test_time::Load::now()
                    );
                }
                Err(e) => panic!("{kind}: {e}"),
            }
        }
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `KIND PID`, as the stand-ins write it; anything else is no handshake.
fn handshake(line: &str) -> Option<(&str, i64)> {
    let fields: Vec<_> = line.split_whitespace().collect();
    let [kind, pid] = fields[..] else {
        return None;
    };
    let pid: i64 = pid.parse().ok().filter(|pid| *pid > 0)?;
    kind.bytes()
        .all(|b| b.is_ascii_lowercase())
        .then_some((kind, pid))
}

pub(super) struct Entry {
    stream: UnixStream,
    pub(super) pid: i64,
}

impl Entry {
    pub(super) fn release(mut self, action: &str) {
        writeln!(self.stream, "{action}").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::{Gate, handshake};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;

    const SIGHTED: &str = "GET /api/document?path=docs/notes/PAGE.md HTTP/1.1\r\n";

    fn stand_in(gate: &Gate, line: &str) -> UnixStream {
        let mut stream = UnixStream::connect(gate.path()).unwrap();
        stream.write_all(line.as_bytes()).unwrap();
        stream
    }

    fn failure(gate: &Gate, kind: &str, limit: Duration) -> String {
        let payload = catch_unwind(AssertUnwindSafe(|| gate.entered_within(kind, limit)))
            .err()
            .unwrap_or_else(|| panic!("a {kind} wait must fail"));
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn stray_connections_are_set_aside_and_the_handshake_still_arrives() {
        let gate = Gate::new();
        let path = gate.path().to_path_buf();
        assert!(path.as_os_str().len() < 104, "{}", path.display());
        let _http = stand_in(&gate, SIGHTED);
        // A connection that ends before writing, as a stand-in killed after its connect.
        drop(UnixStream::connect(gate.path()).unwrap());
        let build = stand_in(&gate, "build 4242\n");
        let entry = gate.entered("build");
        assert_eq!(entry.pid, 4242);
        entry.release("install");
        let mut answer = String::new();
        BufReader::new(build).read_line(&mut answer).unwrap();
        assert_eq!(answer, "install\n");
        drop(gate);
        assert!(!path.exists(), "the gate removes its socket");
    }

    #[test]
    fn a_handshake_of_another_kind_still_fails() {
        let gate = Gate::new();
        let client = stand_in(&gate, "client 33165\n");
        let why = failure(&gate, "startup", Duration::from_secs(5));
        assert!(why.contains("client 33165"), "{why}");
        let mut answer = String::new();
        assert_eq!(BufReader::new(client).read_line(&mut answer).unwrap(), 0);
    }

    #[test]
    fn a_missing_handshake_still_fails_naming_what_was_set_aside() {
        let gate = Gate::new();
        let _http = stand_in(&gate, SIGHTED);
        let why = failure(&gate, "build", Duration::from_millis(300));
        assert!(
            why.starts_with("no build handshake within 300ms: the deadline passed; set aside: ")
                && why.contains("PAGE.md"),
            "{why}"
        );
        // A connection holding its line back holds the wait only until the deadline.
        let _silent = UnixStream::connect(gate.path()).unwrap();
        let why = failure(&gate, "build", Duration::from_millis(300));
        assert!(why.contains("a connection sent no line"), "{why}");
    }

    #[test]
    fn a_handshake_is_a_lowercase_kind_and_a_positive_pid() {
        assert_eq!(handshake("build 7\n"), Some(("build", 7)));
        assert_eq!(handshake("startup 33165"), Some(("startup", 33165)));
        for stray in [
            SIGHTED,
            "",
            "\n",
            "build",
            "build 0",
            "build -3",
            "build x",
            "Build 7",
            "build 7 8",
        ] {
            assert_eq!(handshake(stray), None, "{stray:?}");
        }
    }
}
