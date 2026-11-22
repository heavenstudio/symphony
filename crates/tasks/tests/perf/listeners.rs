//! The recipients' native channels: one Unix socket each, each accepted by a thread of this process
//! as its connections come, which reads each to its end on a thread of its own and keeps what it
//! said.
//! A delivery to any of them is a socket write, as to a live Claude session, so what a write costs
//! is the board's, not a transport's.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use symphony_board::lock::Recovered as _;

pub struct Listeners {
    /// Each recipient's socket, by handle.
    pub paths: BTreeMap<String, PathBuf>,
    said: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
    stop: Arc<AtomicBool>,
}

impl Listeners {
    /// A socket in `dir` for each of `handles`, served from now on.
    pub fn new(dir: &Path, handles: &[String]) -> Listeners {
        let mut paths = BTreeMap::new();
        let said: Arc<Mutex<BTreeMap<String, Vec<String>>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let numbered = handles.iter().enumerate();
        for (n, handle) in numbered {
            let path = dir.join(format!("c{n}.sock"));
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path).expect("a recipient's socket binds");
            paths.insert(handle.clone(), path);
            let (heard, stopped, handle) = (Arc::clone(&said), Arc::clone(&stop), handle.clone());
            // Each connection as it comes; the drop's own connection wakes the accept to its stop.
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    let stream = match stream {
                        Ok(stream) => stream,
                        Err(why) if passing(&why) => continue,
                        Err(why) => panic!("{handle}'s socket's accept failed: {why}"),
                    };
                    let (heard, handle) = (Arc::clone(&heard), handle.clone());
                    std::thread::spawn(move || {
                        let lines = read_frame(stream);
                        (heard_so_far(&heard).entry(handle).or_default()).extend(lines);
                    });
                }
            });
        }
        Listeners { paths, said, stop }
    }

    /// Everything `handle`'s socket has been sent so far, one message's content a line.
    pub fn heard(&self, handle: &str) -> Vec<String> {
        (heard_so_far(&self.said).get(handle).cloned()).unwrap_or_default()
    }

    /// Waits until `handle`'s socket is sent a line holding `text`, which a quiet machine delivers
    /// within `allowance`; a stall past the shared backstop ([`test_time::backstop`]) fails.
    pub fn hears(
        &self,
        handle: &str,
        text: &str,
        allowance: Duration,
    ) -> Result<(), test_time::Stall> {
        test_time::until_true(
            format!("{handle}'s socket sent a line holding {text:?}"),
            test_time::backstop(allowance),
            || self.heard(handle).iter().any(|line| line.contains(text)),
        )
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let sockets = self.paths.values();
        for path in sockets {
            // Wakes its accept, which then sees the stop.
            let _ = UnixStream::connect(path);
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Whether an accept's failure is one that passes, so the next accept is made: a signal, or a
/// connection its client dropped before it was taken.
fn passing(why: &std::io::Error) -> bool {
    matches!(
        why.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionAborted
    )
}

/// What every socket has been sent so far, by handle. A panic while it was held (a test's, reading
/// it, or a connection's thread's, adding its lines, which are read whole before) leaves
/// each handle's lines as they were or with one connection's added: every line kept was sent, so
/// what a later reader finds is still what was heard.
fn heard_so_far(
    said: &Mutex<BTreeMap<String, Vec<String>>>,
) -> MutexGuard<'_, BTreeMap<String, Vec<String>>> {
    said.lock().recover()
}

/// The contents of the SDK envelopes one connection sent, read to its end (the board closes its
/// side after one write), however long a loaded machine takes to send it: a read cut short by a
/// time would lose a delivery the board made.
fn read_frame(mut stream: UnixStream) -> Vec<String> {
    let _ = stream.set_nonblocking(false);
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let envelope: serde_json::Value = serde_json::from_str(line).ok()?;
            envelope["message"]["content"].as_str().map(String::from)
        })
        .collect()
}
