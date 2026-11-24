//! A private proxy between the save and its board's PostgreSQL, on a socket of its own, that makes
//! the save's task creation uncertain without a wait: it holds the request that creates the task
//! (the one whose statement inserts the task's row) and closes the save's connection, so the save
//! never hears how its write went, and runs that request when its phase says. A request is what
//! the client sends up to its `Sync` (or a simple query), and its answer what the server sends up
//! to its `ReadyForQuery`.
//!
//! - `create`: the creation runs only once the read that follows it has been answered upstream,
//!   so that read finds no task and the save stays pending; the save's next try finds the task.
//! - `reply`: the creation runs before the connection is closed, so the read that follows it finds
//!   the task and the save is confirmed at once.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use symphony_board::server::postgres::SOCKET_FILE;

/// What the statement that creates a task holds: its row's insert.
const CREATES_THE_TASK: &[u8] = b"INSERT INTO task (id, sid, slug";

/// One message after the startup, whole: its type, its length and its body. None once the stream
/// ended between messages.
fn message(stream: &mut UnixStream) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0; 5];
    match stream.read_exact(&mut head) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    let mut whole = head.to_vec();
    whole.resize(1 + length, 0);
    stream.read_exact(&mut whole[5..])?;
    Ok(Some((head[0], whole)))
}

/// The messages read up to and with the first of `last`: a request, ending at its `Sync`, simple
/// query or `Terminate`, or an answer, ending at its `ReadyForQuery`.
fn until(stream: &mut UnixStream, last: &[u8]) -> std::io::Result<Option<Vec<u8>>> {
    let mut read = Vec::new();
    while let Some((kind, whole)) = message(stream)? {
        read.extend_from_slice(&whole);
        if last.contains(&kind) {
            return Ok(Some(read));
        }
    }
    Ok(None)
}

fn request(stream: &mut UnixStream) -> std::io::Result<Option<Vec<u8>>> {
    until(stream, b"SQX")
}

/// The server's answer to what was just sent it.
fn answer(stream: &mut UnixStream) -> Vec<u8> {
    until(stream, b"Z")
        .unwrap()
        .expect("the board's PostgreSQL answered")
}

/// The startup message, which has no type: its length and its body.
fn startup(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let mut whole = length.to_vec();
    whole.resize(u32::from_be_bytes(length) as usize, 0);
    stream.read_exact(&mut whole[4..])?;
    Ok(whole)
}

/// The creation held, with the server connection it runs on once it is let go.
type Held = Arc<Mutex<Option<(UnixStream, Vec<u8>)>>>;

/// What every connection of one case shares: the creation held, and how many creations were run.
#[derive(Clone)]
struct Forward {
    upstream: PathBuf,
    phase: &'static str,
    held: Held,
    ran: Arc<Mutex<usize>>,
}

impl Forward {
    fn creates_the_task(request: &[u8]) -> bool {
        request
            .windows(CREATES_THE_TASK.len())
            .any(|window| window == CREATES_THE_TASK)
    }

    fn forward(&self, mut incoming: UnixStream, started: &[u8]) {
        let mut server = UnixStream::connect(&self.upstream).unwrap();
        // Only a stall outlasts the backstop: the board's answer is never awaited past it.
        server.set_read_timeout(Some(super::LIMIT)).unwrap();
        server.write_all(started).unwrap();
        if incoming.write_all(&answer(&mut server)).is_err() {
            return;
        }
        loop {
            let asked = match request(&mut incoming) {
                Ok(Some(asked)) => asked,
                Ok(None) => return,
                Err(error) if error.kind() == ErrorKind::ConnectionReset => return,
                Err(error) => panic!("read the next private proxy request: {error}"),
            };
            if asked.first() == Some(&b'X') {
                let _ = server.write_all(&asked);
                return;
            }
            if *self.ran.lock().unwrap() == 0 && Self::creates_the_task(&asked) {
                if self.phase == "reply" {
                    server.write_all(&asked).unwrap();
                    answer(&mut server);
                    *self.ran.lock().unwrap() += 1;
                } else {
                    *self.held.lock().unwrap() = Some((server, asked));
                }
                // Closed with no reply: the save cannot tell whether its write ran.
                return;
            }
            server.write_all(&asked).unwrap();
            let reply = answer(&mut server);
            if let Some((mut other, held)) = self.held.lock().unwrap().take() {
                other.write_all(&held).unwrap();
                answer(&mut other);
                *self.ran.lock().unwrap() += 1;
            }
            if incoming.write_all(&reply).is_err() {
                return;
            }
        }
    }
}

/// Owns the listener and every forwarding connection of one case's database.
pub(super) struct Holding {
    /// Its folder, which holds its socket alone: a short one under `/tmp`, as a socket's path has
    /// a length limit.
    folder: PathBuf,
    /// The proxy's address, as a board's is named: `postgresql:` and its folder.
    pub(super) address: String,
    /// How many of the save's task creations ran: one, once it is over.
    pub(super) ran: Arc<Mutex<usize>>,
    worker: Option<std::thread::JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}

impl Holding {
    /// A proxy to the board's PostgreSQL at `upstream` (`postgresql:` and its socket's folder).
    pub(super) fn start(upstream: &str, phase: &'static str) -> Self {
        static PROXIES: AtomicUsize = AtomicUsize::new(0);
        let upstream =
            PathBuf::from(upstream.strip_prefix("postgresql:").unwrap()).join(SOCKET_FILE);
        let folder = PathBuf::from(format!(
            "/tmp/symphony-hold-{}-{}",
            std::process::id(),
            PROXIES.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        let listener = UnixListener::bind(folder.join(SOCKET_FILE)).unwrap();
        let workers = Arc::new(Mutex::new(Vec::new()));
        let saved = workers.clone();
        let forward = Forward {
            upstream,
            phase,
            held: Arc::default(),
            ran: Arc::default(),
        };
        let ran = forward.ran.clone();
        let worker = std::thread::spawn(move || {
            for connection in listener.incoming() {
                let mut incoming = connection.unwrap();
                // The connection made at the end to end this loop says nothing.
                let Ok(started) = startup(&mut incoming) else {
                    break;
                };
                let forward = forward.clone();
                saved.lock().unwrap().push(std::thread::spawn(move || {
                    forward.forward(incoming, &started);
                }));
            }
        });
        Self {
            address: format!("postgresql:{}", folder.display()),
            folder,
            ran,
            worker: Some(worker),
            workers,
        }
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        drop(UnixStream::connect(self.folder.join(SOCKET_FILE)).unwrap());
        self.worker.take().unwrap().join().unwrap();
        let workers = self.workers.lock().unwrap().drain(..).collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}
