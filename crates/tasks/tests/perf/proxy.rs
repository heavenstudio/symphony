//! A recording relay between the board's clients and its server: each RPC request passes through
//! unchanged and its query and variables are kept, so the plan check reads what the commands sent,
//! whatever code path sent it, rather than a list of queries written down beside the code.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// One request a client sent: its query text and its variables.
#[derive(Clone, Debug)]
pub struct Request {
    pub sql: String,
    pub vars: Value,
}

pub struct Proxy {
    pub addr: String,
    sent: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    /// A relay on a free local port to the server at `upstream`.
    pub fn to(upstream: &str) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the relay binds");
        listener.set_nonblocking(true).expect("a nonblocking relay");
        let addr = listener.local_addr().unwrap().to_string();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let kept = Arc::clone(&sent);
        let stopped = Arc::clone(&stop);
        let upstream = upstream.to_string();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let _ = client.set_nonblocking(false);
                        let server = TcpStream::connect(&upstream).expect("the server answers");
                        relay(client, server, Arc::clone(&kept));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Proxy { addr, sent, stop }
    }

    /// The requests sent so far, which are then forgotten.
    pub fn take(&self) -> Vec<Request> {
        std::mem::take(&mut *sent_so_far(&self.sent))
    }
}

/// The requests kept so far. A panic while they were held (a relay thread's push of one request,
/// decoded whole before, or a test's take of them all) leaves every request kept whole: what a later
/// reader finds was each sent.
fn sent_so_far(sent: &Mutex<Vec<Request>>) -> MutexGuard<'_, Vec<Request>> {
    sent.lock().unwrap_or_else(|failed| failed.into_inner())
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The server's answers go back as they come; the client's requests are read one at a time (an HTTP
/// head, then the body its length names), kept, and passed on.
fn relay(client: TcpStream, server: TcpStream, kept: Arc<Mutex<Vec<Request>>>) {
    let (mut answers, mut to_client) = (server.try_clone().unwrap(), client.try_clone().unwrap());
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut answers, &mut to_client);
        let _ = to_client.shutdown(Shutdown::Both);
    });
    std::thread::spawn(move || {
        let mut requests = BufReader::new(client);
        let mut to_server = server;
        loop {
            let mut head = Vec::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                match requests.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = to_server.shutdown(Shutdown::Both);
                        return;
                    }
                    Ok(_) => {}
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap_or(0);
                }
                head.extend_from_slice(line.as_bytes());
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            if requests.read_exact(&mut body).is_err() {
                let _ = to_server.shutdown(Shutdown::Both);
                return;
            }
            if let Ok(call) = symphony_board::cbor::decode(&body)
                && let Some(sql) = call["params"][0].as_str()
            {
                let request = Request {
                    sql: sql.to_string(),
                    vars: call["params"][1].clone(),
                };
                sent_so_far(&kept).push(request);
            }
            head.extend_from_slice(&body);
            if to_server.write_all(&head).is_err() {
                return;
            }
        }
    });
}
