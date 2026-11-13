//! A private RPC proxy between the save and its board that makes the save's task CREATE uncertain
//! without a wait: it holds the CREATE and closes the save's connection, so the save never hears
//! how its write went, and runs the CREATE when its phase says.
//!
//! - `create`: the CREATE runs only once the read that follows it has been answered upstream, so
//!   that read finds no task and the save stays pending; the save's next try finds the task.
//! - `reply`: the CREATE runs before the connection is closed, so the read that follows it finds
//!   the task and the save is confirmed at once.

use super::{LIMIT, packet};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// What every connection of one case shares: the CREATE held, and how many CREATEs were run.
#[derive(Clone)]
struct Forward {
    upstream: String,
    phase: &'static str,
    held: Arc<Mutex<Option<(String, Vec<u8>)>>>,
    ran: Arc<Mutex<usize>>,
}

impl Forward {
    fn creates_the_task(bytes: &[u8]) -> bool {
        let call = symphony_board::cbor::decode(bytes).unwrap();
        call["params"][0]
            .as_str()
            .is_some_and(|sql| sql.contains("CREATE type::record('task', $slug)"))
    }

    fn forward(&self, mut incoming: BufReader<TcpStream>, first: (String, Vec<u8>)) {
        let mut next = Some(first);
        while let Some((headers, bytes)) = next.take() {
            if *self.ran.lock().unwrap() == 0 && Self::creates_the_task(&bytes) {
                if self.phase == "reply" {
                    exchange(&self.upstream, &headers, &bytes);
                    *self.ran.lock().unwrap() += 1;
                } else {
                    *self.held.lock().unwrap() = Some((headers, bytes));
                }
                // Closed with no reply: the save cannot tell whether its write ran.
                return;
            }
            let reply = exchange(&self.upstream, &headers, &bytes);
            if let Some((headers, bytes)) = self.held.lock().unwrap().take() {
                exchange(&self.upstream, &headers, &bytes);
                *self.ran.lock().unwrap() += 1;
            }
            if incoming
                .get_mut()
                .write_all(reply.0.as_bytes())
                .and_then(|()| incoming.get_mut().write_all(&reply.1))
                .is_err()
            {
                return;
            }
            next = match packet(&mut incoming) {
                Ok(next) => next,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => return,
                Err(error) => panic!("read the next private proxy request: {error}"),
            };
        }
    }
}

fn exchange(upstream: &str, headers: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let mut remote = TcpStream::connect(upstream).unwrap();
    remote.set_read_timeout(Some(LIMIT)).unwrap();
    remote.set_write_timeout(Some(LIMIT)).unwrap();
    let headers = headers
        .lines()
        .filter(|line| {
            !line.to_ascii_lowercase().starts_with("content-length:") && !line.is_empty()
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    write!(
        remote,
        "{headers}\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    )
    .unwrap();
    remote.write_all(bytes).unwrap();
    packet(&mut BufReader::new(remote)).unwrap().unwrap()
}

/// Owns the listener and every forwarding connection of one case's database.
pub(super) struct Holding {
    pub(super) address: std::net::SocketAddr,
    /// How many of the save's task CREATEs ran: one, once it is over.
    pub(super) ran: Arc<Mutex<usize>>,
    worker: Option<std::thread::JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}

impl Holding {
    pub(super) fn start(upstream: String, phase: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
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
                let mut incoming = BufReader::new(connection.unwrap());
                let Some(first) = packet(&mut incoming).unwrap() else {
                    break;
                };
                let forward = forward.clone();
                saved.lock().unwrap().push(std::thread::spawn(move || {
                    forward.forward(incoming, first);
                }));
            }
        });
        Self {
            address,
            ran,
            worker: Some(worker),
            workers,
        }
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        drop(TcpStream::connect(self.address).unwrap());
        self.worker.take().unwrap().join().unwrap();
        let workers = self.workers.lock().unwrap().drain(..).collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
