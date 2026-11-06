//! A write run as the `tasks` binary (`cli::main_detaching`) answers before its callback dispatch,
//! which the detached `tasks dispatch` it starts makes once the command has ended; one that cannot
//! start leaves the dispatch to the command, as before.

use crate::{Case, Ran};
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::cli::main_detaching;

/// A socket of the test's own that keeps every frame sent to it.
struct Listener {
    path: PathBuf,
    socket: UnixListener,
}

impl Listener {
    fn new(name: &str) -> Listener {
        let path = PathBuf::from(format!(
            "/tmp/sf-detached-{}-{name}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let socket = UnixListener::bind(&path).unwrap();
        socket.set_nonblocking(true).unwrap();
        Listener { path, socket }
    }

    /// The event lines sent until one holds `marker`, waited for up to `within`.
    fn until(&self, marker: &str, within: Duration) -> Vec<String> {
        let deadline = Instant::now() + within;
        let mut lines = Vec::new();
        while Instant::now() < deadline {
            match self.socket.accept() {
                Ok((stream, _)) => {
                    for frame in frames(stream).lines() {
                        let frame: serde_json::Value = serde_json::from_str(frame).unwrap();
                        let content = frame["message"]["content"].as_str().unwrap();
                        lines.extend(content.lines().map(String::from));
                    }
                    if lines.iter().any(|line| line.contains(marker)) {
                        return lines;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("callback listener: {e}"),
            }
        }
        panic!("no line held {marker:?} within {within:?}; lines: {lines:?}")
    }
}

/// What one connection sent, read until the sender closes it (nothing, for a probe).
fn frames(mut stream: UnixStream) -> String {
    stream.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut bytes, mut buffer) = (Vec::new(), [0; 4096]);
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return String::from_utf8(bytes).unwrap(),
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "a callback frame did not close");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => panic!("callback frame: {e}"),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A worker executing `slug`, its callback `listener`, and another session to write to it.
fn board(case: &Case, slug: &str, listener: &Listener) -> (String, String) {
    let recipient = case.register("worker");
    let writer = case.register("worker");
    let store = case.store();
    store
        .create(&symphony_board::NewTask::new(slug, "Detached", &writer))
        .unwrap();
    store.claim(slug, &recipient).unwrap();
    store
        .callback(
            &recipient,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    (recipient, writer)
}

fn say(case: &Case, program: &Path, args: &[&str]) -> Ran {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_detaching(
        &args,
        &case.env,
        &mut "".as_bytes(),
        &mut out,
        &mut err,
        program,
    );
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn a_post_answers_first_and_the_detached_dispatcher_it_starts_delivers_it() {
    let case = Case::new();
    let listener = Listener::new("started");
    let (recipient, writer) = board(&case, "tools/detached", &listener);
    let ran = say(
        &case,
        Path::new(env!("CARGO_BIN_EXE_tasks")),
        &[
            "say",
            "tools/detached",
            "over to you",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.err, "", "the command itself dispatched nothing");
    let id = ran
        .out
        .strip_prefix("sent #")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    let lines = listener.until(&format!("MSG #{id} "), Duration::from_secs(60));
    let delivered = lines.iter().filter(|l| l.contains("over to you")).count();
    assert_eq!(delivered, 1, "{lines:?}");
}

#[test]
fn a_dispatcher_that_cannot_start_leaves_the_dispatch_to_the_command() {
    let case = Case::new();
    let listener = Listener::new("unstarted");
    let (recipient, writer) = board(&case, "tools/undetached", &listener);
    let ran = say(
        &case,
        Path::new("/nonexistent/tasks"),
        &[
            "say",
            "tools/undetached",
            "here and now",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    // Delivered before the command answered: the socket already holds the frame.
    listener.until("here and now", Duration::from_secs(1));
}
