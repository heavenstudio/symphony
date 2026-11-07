//! The recipients' native channels: one Unix socket each, all served by one thread of this process,
//! which reads each connection to its end and keeps what it said. A delivery to any of them is a
//! socket write, as to a live Claude session, so what a write costs is the board's, not a
//! transport's.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
        let mut bound = Vec::new();
        for (n, handle) in handles.iter().enumerate() {
            let path = dir.join(format!("c{n}.sock"));
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path).expect("a recipient's socket binds");
            listener
                .set_nonblocking(true)
                .expect("a nonblocking socket");
            paths.insert(handle.clone(), path);
            bound.push((handle.clone(), listener));
        }
        let said: Arc<Mutex<BTreeMap<String, Vec<String>>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (heard, stopped) = (Arc::clone(&said), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let mut idle = true;
                for (handle, listener) in &bound {
                    while let Ok((stream, _)) = listener.accept() {
                        idle = false;
                        let lines = read_frame(stream);
                        let mut heard = heard.lock().unwrap_or_else(|e| e.into_inner());
                        heard.entry(handle.clone()).or_default().extend(lines);
                    }
                }
                if idle {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        Listeners { paths, said, stop }
    }

    /// Everything `handle`'s socket has been sent so far, one message's content a line.
    pub fn heard(&self, handle: &str) -> Vec<String> {
        let said = self.said.lock().unwrap_or_else(|e| e.into_inner());
        said.get(handle).cloned().unwrap_or_default()
    }

    /// Whether `handle`'s socket is sent a line holding `text` within `wait`.
    pub fn hears(&self, handle: &str, text: &str, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            if self.heard(handle).iter().any(|line| line.contains(text)) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for path in self.paths.values() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The contents of the SDK envelopes one connection sent, read to its end (the board closes its
/// side after one write), within a second.
fn read_frame(mut stream: UnixStream) -> Vec<String> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
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
