//! A gate a statement waits at on the board's server until the test opens it: a listener of the
//! test's own, which the statement's `http::get` asks again and again (the server reaches this
//! machine alone, `common::launch`). The test knows the statement reached it by its first question,
//! so what it does next comes after whatever the statement did before; and the statement goes on
//! only once the test opens it, so what the test did meanwhile has committed first. Neither waits on
//! the wall but for its backstop ([`BACKSTOP`]), however long a loaded machine takes either side.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// How long a statement waits at a gate the test never opens, and the test for one never reached:
/// the wall-clock backstop a test's wait is given (`test_time::FLOOR`).
const BACKSTOP: Duration = Duration::from_secs(120);
/// The pause between two of the statement's questions.
const LOOK_MS: u64 = 5;

pub struct Gate {
    addr: String,
    reached: mpsc::Receiver<()>,
    open: Arc<AtomicBool>,
}

impl Gate {
    pub fn new() -> Gate {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (reach, reached) = mpsc::channel();
        let open = Arc::new(AtomicBool::new(false));
        let opened = Arc::clone(&open);
        std::thread::spawn(move || {
            for asked in listener.incoming().map_while(Result::ok) {
                let _ = reach.send(());
                answer(asked, opened.load(Ordering::SeqCst));
            }
        });
        Gate {
            addr,
            reached,
            open,
        }
    }

    /// The SurrealQL that waits at this gate, a statement of a block: until the gate is open, or the
    /// backstop has passed in its pauses.
    pub fn wait_here(&self) -> String {
        let looks = BACKSTOP.as_millis() / u128::from(LOOK_MS);
        format!(
            "FOR $look IN 0..{looks} {{ IF http::get('http://{}/gate') == 'open' {{ BREAK; }}; sleep({LOOK_MS}ms); }};",
            self.addr
        )
    }

    /// Once a statement has reached this gate.
    pub fn reached(&self) {
        self.reached
            .recv_timeout(BACKSTOP)
            .expect("no statement reached the gate within its backstop");
    }

    /// The gate opened, for the statement waiting at it and for every one after.
    pub fn open(&self) {
        self.open.store(true, Ordering::SeqCst);
    }

    /// What opens the gate once it is dropped, however the test goes on: a failure meanwhile never
    /// leaves the statement waiting out its backstop before the test can end.
    pub fn opening(&self) -> Opening<'_> {
        Opening(self)
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.open();
    }
}

/// [`Gate::opening`].
pub struct Opening<'a>(&'a Gate);

impl Drop for Opening<'_> {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// The question on `asked` answered: `open`, or `wait`.
fn answer(mut asked: TcpStream, open: bool) {
    let mut head = Vec::new();
    let mut chunk = [0; 1024];
    while !head.windows(4).any(|end| end == b"\r\n\r\n") {
        match asked.read(&mut chunk) {
            Ok(read) if read > 0 => head.extend_from_slice(&chunk[..read]),
            _ => return,
        }
    }
    let body = if open { "open" } else { "wait" };
    let _ = write!(
        asked,
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
}
