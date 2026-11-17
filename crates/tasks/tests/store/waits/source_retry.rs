//! A modeled transaction conflict proves the guarded callback runs again with the original
//! expected state. Only that one RPC reply is modeled; the retry reaches the actual scratch board.

use super::*;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use symphony_board::{Store, cbor, rpc::Client};

struct Conflict {
    addr: String,
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<Value>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Conflict {
    fn before_retry(server: &str, current: Arc<AtomicBool>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (ended, sent, server) = (stop.clone(), seen.clone(), server.to_string());
        let thread = std::thread::spawn(move || {
            let mut conflict = true;
            for caller in listener.incoming() {
                let caller = caller.unwrap();
                if ended.load(Ordering::SeqCst) {
                    break;
                }
                let mut caller = BufReader::new(caller);
                let mut upstream = BufReader::new(TcpStream::connect(&server).unwrap());
                while let Some((raw, body)) = message(&mut caller) {
                    let request = cbor::decode(&body).unwrap();
                    let guarded = request["params"][0]
                        .as_str()
                        .is_some_and(|sql| sql.contains("$source_current"));
                    if guarded {
                        sent.lock().unwrap().push(request["params"][1].clone());
                    }
                    if guarded && conflict {
                        conflict = false;
                        current.store(false, Ordering::SeqCst);
                        let body = cbor::encode(&json!({"id": request["id"], "result": [{
                            "status": "ERR", "kind": "Query", "details": {"kind": "TransactionConflict"},
                            "result": "Cannot COMMIT: Transaction conflict: Write conflict, retry the transaction.",
                        }]}));
                        write!(caller.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/cbor\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
                        caller.get_mut().write_all(&body).unwrap();
                    } else {
                        upstream.get_mut().write_all(&raw).unwrap();
                        let (answer, _) = message(&mut upstream).expect("native board response");
                        caller.get_mut().write_all(&answer).unwrap();
                    }
                }
            }
        });
        Self {
            addr,
            stop,
            seen,
            thread: Some(thread),
        }
    }
}

impl Drop for Conflict {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        drop(TcpStream::connect(&self.addr));
        self.thread.take().unwrap().join().unwrap();
    }
}

fn message(stream: &mut impl BufRead) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut raw = Vec::new();
    loop {
        let start = raw.len();
        if stream.read_until(b'\n', &mut raw).unwrap() == 0 {
            return None;
        }
        if &raw[start..] == b"\r\n" {
            break;
        }
    }
    let head = String::from_utf8_lossy(&raw).to_ascii_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .expect("RPC fixture uses content length")
        .trim()
        .parse()
        .unwrap();
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    raw.extend(&body);
    Some((raw, body))
}

// On SurrealDB alone: a proxy stands before SurrealDB's server.
// SurrealDB alone: the HTTP/CBOR relay injects one modeled conflict response.
#[test]
fn a_real_retry_reobserves_source_without_refreshing_the_frozen_task_fence() {
    for known in [false, true] {
        let f = Fixture::new(Seed::default().session(ACTOR));
        f.task(SLUG);
        let expected = f.store.assessment_state(SLUG).unwrap();
        let original = known.then(|| {
            f.store
                .record_waits_checked(SLUG, RECORD, "original", ACTOR, &expected)
                .unwrap()
        });
        if known {
            let newer = f.store.assessment_state(SLUG).unwrap();
            f.store
                .record_waits_checked(SLUG, RECORD, "newer", ACTOR, &newer)
                .unwrap();
            f.store.leave(ACTOR).unwrap();
        }
        let before = f.store.get(SLUG).unwrap();
        let lines = f.said(SLUG);
        let current = Arc::new(AtomicBool::new(true));
        let proxy = Conflict::before_retry(f.store.client().addr(), current.clone());
        let store = Store::with(
            Client::new(&proxy.addr, "sf", f.store.client().database()),
            Box::new(|| crate::common::NOW),
            Box::new(|_| false),
            crate::common::HOST,
        )
        .unwrap();
        let mut observed = Vec::new();
        let result =
            store.record_waits_guarded(SLUG, RECORD, "original", ACTOR, &expected, &mut || {
                let value = current.load(Ordering::SeqCst);
                observed.push(value);
                if !value && known {
                    Err(symphony_board::Error::Db(
                        "source became unreadable before retry".into(),
                    ))
                } else {
                    Ok(value)
                }
            });
        assert_eq!(observed, [true, false], "known={known}: {result:?}");
        let sent = proxy.seen.lock().unwrap().clone();
        assert_eq!(sent.len(), 2, "known={known}: {sent:?}");
        assert_eq!(
            sent.iter()
                .map(|vars| vars["source_current"].clone())
                .collect::<Vec<_>>(),
            [json!(true), json!(false)]
        );
        for vars in &sent {
            assert_eq!(
                vars["expected_status"], expected.status,
                "known={known}: {vars}"
            );
            assert_eq!(
                vars["expected_version"], expected.version,
                "known={known}: {vars}"
            );
        }
        if let Some(mut original) = original {
            original.current = false;
            assert_eq!(result, Ok(original));
        } else {
            assert!(
                matches!(result, Err(symphony_board::Error::Refused(_))),
                "{result:?}"
            );
        }
        let after = f.store.get(SLUG).unwrap();
        assert_eq!(after.fields, before.fields, "known={known}");
        assert_eq!(after.updated, before.updated, "known={known}");
        assert_eq!(f.said(SLUG), lines, "known={known}");
        drop(store);
        drop(proxy);
    }
}
