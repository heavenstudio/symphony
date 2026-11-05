//! A private RPC proxy delays selected statements without changing the server under test.

use super::{LIMIT, packet};
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};

/// Every connection shares the delay counter and completion channel for one private case.
#[derive(Clone)]
struct Forward {
    upstream: String,
    root: std::path::PathBuf,
    phase: &'static str,
    chosen: Arc<Mutex<u8>>,
    send: mpsc::Sender<()>,
}

impl Forward {
    fn delay(&self, call: &mut Value) -> bool {
        let sql = call["params"][0].as_str().unwrap();
        let create = sql.contains("CREATE type::record('task', $slug)");
        let lookup = sql.contains("record::exists(type::record('task', $slug))") && !create;
        let mut chosen = self.chosen.lock().unwrap();
        let applies = match self.phase {
            "open" => sql.contains("INFO FOR DB"),
            "lookup" => lookup,
            "create" | "message" | "reply" | "edited" | "wipe" => create,
            "check" => create && *chosen == 0 || lookup && matches!(*chosen, 1 | 2),
            "reopen" => create && *chosen == 0 || sql.contains("INFO FOR DB") && *chosen == 1,
            "journal" | "concurrent" | "receipt-write" | "journal-write" | "interrupted" => false,
            _ => unreachable!(),
        };
        let delay = applies && (*chosen == 0 || matches!(self.phase, "check" | "reopen"));
        if delay {
            *chosen += 1;
            call["params"][0] = json!(match self.phase {
                "open" | "lookup" => format!("SLEEP 4s;\n{sql}"),
                "create" | "check" | "reopen" if create => sql.replacen(
                    "CREATE type::record('task', $slug)",
                    "SLEEP 4s;\nCREATE type::record('task', $slug)",
                    1
                ),
                "check" | "reopen" => format!("SLEEP 4s;\n{sql}"),
                "message" => sql.replacen(
                    "CREATE type::record('message',",
                    "SLEEP 4s;\nCREATE type::record('message',",
                    1
                ),
                "reply" | "edited" | "wipe" => sql.to_string(),
                _ => unreachable!(),
            });
        }
        delay
    }

    fn forward(&self, mut incoming: BufReader<TcpStream>, first: (String, Vec<u8>)) {
        let mut next = Some(first);
        while let Some((headers, bytes)) = next.take() {
            let mut call = symphony_board::cbor::decode(&bytes).unwrap();
            let delayed = self.delay(&mut call);
            let (reply, bytes) = exchange(
                &self.upstream,
                &headers,
                &symphony_board::cbor::encode(&call),
            );
            if delayed && self.phase == "edited" {
                let vars = &call["params"][1];
                let db = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("surreal-db: "))
                    .expect("private RPC names its database")
                    .trim();
                let client = symphony_board::rpc::Client::new(&self.upstream, "sf", db);
                client.query("UPDATE type::record('task', $slug) SET priority = 4, title = 'Edited title', body = 'Edited body'; DELETE link WHERE slug = $slug AND kind = 'doc';",
                    json!({"slug":vars["slug"]})).unwrap();
            }
            if delayed && self.phase == "wipe" {
                std::fs::remove_dir_all(self.root.join("artifacts/symphony-quick-tasks")).unwrap();
            }
            if delayed && matches!(self.phase, "reply" | "edited" | "wipe") {
                std::thread::sleep(std::time::Duration::from_secs(4));
            }
            if delayed {
                self.send.send(()).unwrap();
            }
            if incoming
                .get_mut()
                .write_all(reply.as_bytes())
                .and_then(|()| incoming.get_mut().write_all(&bytes))
                .is_err()
            {
                break;
            }
            next = match packet(&mut incoming) {
                Ok(next) => next,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
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

/// Owns the listener and all forwarding connections for this case's database.
pub(super) struct Delayed {
    pub(super) address: std::net::SocketAddr,
    worker: Option<std::thread::JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    pub(super) delayed: mpsc::Receiver<()>,
}

impl Delayed {
    pub(super) fn start(upstream: String, phase: &'static str, root: &std::path::Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
        let workers = Arc::new(Mutex::new(Vec::new()));
        // Listener and owner share the handles so teardown joins every connection.
        let saved = workers.clone();
        let (send, delayed) = mpsc::channel();
        let forward = Forward {
            upstream,
            root: root.to_path_buf(),
            phase,
            chosen: Arc::new(Mutex::new(0)),
            send,
        };
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
            worker: Some(worker),
            workers,
            delayed,
        }
    }
}

impl Drop for Delayed {
    fn drop(&mut self) {
        drop(TcpStream::connect(self.address).unwrap());
        self.worker.take().unwrap().join().unwrap();
        let workers = self.workers.lock().unwrap().drain(..).collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
