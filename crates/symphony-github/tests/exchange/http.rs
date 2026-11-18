//! The fake repository served on a loopback port, for a real `curl`: one request a connection,
//! answered only when it carries the fixture's token as a bearer, and a lost answer is the
//! connection closed with nothing written.

use crate::github::Fake;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub(crate) struct Server {
    pub(crate) base: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The head of every request as it arrived: its request line and headers.
    pub(crate) heads: Arc<Mutex<Vec<String>>>,
}

fn serve(stream: TcpStream, fake: &Fake, heads: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (found, value) = line.split_once(':')?;
            found
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let length = header("content-length")
        .and_then(|length| length.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let body = String::from_utf8_lossy(&body).into_owned();
    let mut words = head.lines().next().unwrap_or_default().split(' ');
    let (method, path) = (
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
    );
    heads.lock().unwrap().push(head.clone());
    let mut stream = stream;
    if header("authorization").as_deref() != Some(&format!("Bearer {}", crate::TOKEN)) {
        return stream.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
    }
    let (response, lost) = fake.handle(method, path, (length > 0).then_some(body.as_str()));
    if lost {
        return Ok(());
    }
    let mut text = format!("HTTP/1.1 {} Status\r\n", response.status);
    for (name, value) in &response.headers {
        let _ = write!(text, "{name}: {value}\r\n");
    }
    let _ = write!(
        text,
        "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.body.len(),
        response.body
    );
    stream.write_all(text.as_bytes())
}

impl Server {
    pub(crate) fn start(fake: Arc<Fake>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let stop = Arc::new(AtomicBool::new(false));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let (stopping, kept) = (Arc::clone(&stop), Arc::clone(&heads));
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    let _ = serve(stream, &fake, &kept);
                }
            }
        });
        Server {
            base,
            stop,
            thread: Some(thread),
            heads,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept it waits in ends on this connection.
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
