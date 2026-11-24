//! A server's signals are watched from the moment their watch is made, before anything outside can
//! see the server: one sent before the watch is first polled is kept and acted on, never met by
//! its default action, which ends the process. Each case raises its signal in a child process of
//! its own, so the rest of this test run never receives it, and a child its signal ended fails the
//! case by that end.
//!
//! The held cases guard `run`'s order itself: a server run in a child, its stdout a pipe already
//! full ([`held`]), cannot write its URL line until the test reads that pipe, so a signal sent
//! before then reaches it while its start has not returned. A watch built once start returns,
//! past that line, would not exist yet.

use super::*;
use crate::testing::Tree;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use symphony_board::sys::{self, Stdio};

/// Set in the child each case runs itself in.
const CHILD: &str = "SYMPHONY_SIGNALS_CHILD";
/// What a child says once its case has passed.
const DONE: &str = "signals case: done";

/// The wall-clock backstop of every wait here: each waits on its event, and only a stall reaches
/// this (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// Run the case `test` (its name under `front::signals_tests`) again in a child of its own, which
/// takes the child's side, and require that child to end well, saying [`DONE`].
fn in_a_child(test: &str) {
    let child = sys::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("front::signals_tests::{test}"),
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env_remove("TASKS_SESSION")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let done = sys::wait_output(child).unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        done.status.signal(),
        None,
        "the child ended by a signal it raised itself: its default action, unwatched: {said}"
    );
    assert!(done.status.success(), "{said}");
    assert!(said.contains(DONE), "{said}");
}

/// Send `signal` to this process, the child a case runs in.
fn raise(signal: libc::c_int) {
    // SAFETY: raise(3) takes any signal number; it returns non-zero only for an invalid one.
    assert_eq!(unsafe { libc::raise(signal) }, 0, "raise({signal})");
}

/// A stop sent after its watch is made and before the watch is polled resolves it at its first
/// poll: SIGTERM, SIGINT and SIGHUP each, a watch of its own made for each.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_sent_before_its_watch_is_polled_is_kept_and_resolves_it() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child("a_stop_sent_before_its_watch_is_polled_is_kept_and_resolves_it");
    }
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let stop = signalled();
        raise(signal);
        tokio::time::timeout(BACKSTOP, stop)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "the watch to see signal {signal}, sent before its first poll: stalled: \
                     the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
                    test_time::Load::now()
                )
            });
    }
    println!("{DONE}");
}

/// A stop sent as a server starts, before it listens or records its port, is kept through the
/// start and acted on once it is done: the stop path runs, the port record is cleared and nothing
/// listens on its port.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_sent_before_the_server_listens_stops_it_once_started_leaving_nothing() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child(
            "a_stop_sent_before_the_server_listens_stops_it_once_started_leaving_nothing",
        );
    }
    let tree = Tree::new();
    tree.write("root/docs/README.md", "# Docs\n");
    let root = tree.path("root");
    let stop = signalled();
    raise(libc::SIGTERM);
    assert!(
        !port_file(&root).exists(),
        "a port recorded before the start"
    );
    let started = start(
        Serve {
            home: None,
            root: root.clone(),
            port: 0,
            hosts: vec!["127.0.0.1".into()],
            install: None,
        },
        false,
        None,
    )
    .await
    .unwrap();
    let port = started.serve.port;
    assert_eq!(
        std::fs::read_to_string(port_file(&root)).unwrap(),
        format!("{port}\n"),
        "the start records its port"
    );
    let status = tokio::time::timeout(BACKSTOP, started.serve_until_stopped(stop))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the stop kept through the start to end the server: stalled: the wait's \
                 wall-clock backstop of {BACKSTOP:?} ran out; {}",
                test_time::Load::now()
            )
        });
    assert_eq!(status, 0);
    assert!(
        !port_file(&root).exists(),
        "the server stopped without clearing its port record"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "something still listens on the stopped server's port {port}"
    );
    println!("{DONE}");
}

/// A hand-off asked (SIGUSR1) after its watch is made and before its loop starts is kept, and
/// asked of the server once the loop starts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hand_off_asked_before_its_loop_starts_is_kept_and_asked_of_the_server() {
    if std::env::var_os(CHILD).is_none() {
        return in_a_child(
            "a_hand_off_asked_before_its_loop_starts_is_kept_and_asked_of_the_server",
        );
    }
    let tree = Tree::new();
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
    let asked = hand_off_asked().expect("SIGUSR1 can be watched");
    raise(libc::SIGUSR1);
    let wakes = Wakes::new();
    shared.wake_at_handover(wakes.waker());
    assert!(
        !shared.handover.load(Ordering::SeqCst),
        "a hand-off asked of the server before its loop started"
    );
    handover_when_asked(&shared, asked);
    let woken = wakes.wait(Some(BACKSTOP));
    assert!(
        woken,
        "the kept request to wake the hand-off's waiters: stalled: the wait's wall-clock \
         backstop of {BACKSTOP:?} ran out; {}",
        test_time::Load::now()
    );
    assert!(shared.handover.load(Ordering::SeqCst));
    shared.stop();
    println!("{DONE}");
}

/// Set in a held case's child to the tree its server runs in: `root`, and `home` for the one
/// server.
const TREE: &str = "SYMPHONY_SIGNALS_TREE";
/// Set in a held case's child to the number of the descriptor its held pipe's writer is handed
/// under.
const HELD: &str = "SYMPHONY_SIGNALS_HELD";
/// How a server's URL line begins ([`start`]).
const URL_LINE: &str = "→ Symphony at ";

/// A stop sent as a server of its own writes its URL line, once it answers on its port (the last
/// thing outside sees before that line), ends it by the stop path: the line held, the server
/// keeps the stop through its start, then prints the line, stops and clears its port record. Were
/// its stop watched only once its start returned, past the held line, SIGTERM's default action
/// would end the child.
#[test]
fn a_stop_sent_as_the_url_line_is_written_ends_it_by_the_stop_path() {
    if std::env::var_os(TREE).is_some() {
        return serve_held(false);
    }
    let tree = Tree::new();
    tree.write("root/docs/README.md", "# Docs\n");
    let mut held = Held::start(
        "a_stop_sent_as_the_url_line_is_written_ends_it_by_the_stop_path",
        &tree,
    );
    let port = held.recorded();
    held.answering(port);
    held.signal(libc::SIGTERM);
    held.release();
    held.ended();
}

/// A hand-off asked of the one server (SIGUSR1) at its port record's appearance, its URL line
/// held, is kept through its start and asked of it once its hand-off loop starts: the reload
/// worker takes the request and refuses it (one this case left malformed), and the server goes on
/// to print its URL line and, at a stop, ends by the stop path. Were the hand-off watch built once
/// start returned, past the held line, SIGUSR1's default action would end the child.
#[test]
fn a_hand_off_asked_as_the_one_server_can_be_seen_is_kept_through_its_start() {
    if std::env::var_os(TREE).is_some() {
        return serve_held(true);
    }
    let tree = Tree::new();
    tree.write("root/docs/README.md", "# Docs\n");
    // The one server's sweep runs this stand-in for `tasks`, which answers at once.
    let launcher = tree.path("root/.target/bin/tasks");
    std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    crate::fixture_script::write(&launcher, "#!/bin/sh\nexit 0\n", false);
    tree.write(
        &format!("home/{}", crate::host::HANDOVER),
        "not a hand-off request",
    );
    let mut held = Held::start(
        "a_hand_off_asked_as_the_one_server_can_be_seen_is_kept_through_its_start",
        &tree,
    );
    held.seen();
    held.signal(libc::SIGUSR1);
    held.release();
    held.tells("· Symphony hand-off refused: ");
    held.signal(libc::SIGTERM);
    held.ended();
}

/// A held case's own side, in its child: its stdout moved onto the held pipe it was handed, then
/// the server run (`run`), the one server when `home`, to its end, which must be the stop path's,
/// its port record cleared.
fn serve_held(home: bool) {
    let tree = PathBuf::from(std::env::var_os(TREE).unwrap());
    let writer: libc::c_int = std::env::var(HELD).unwrap().parse().unwrap();
    // The harness has said its own lines on stdout by now: from here, what is said there waits
    // for the held pipe's reader.
    std::io::stdout().flush().unwrap();
    // SAFETY: dup2 copies the descriptor this child was handed onto stdout's number, then the
    // copy at the handed number, which nothing else holds, is closed.
    unsafe {
        assert_eq!(libc::dup2(writer, libc::STDOUT_FILENO), libc::STDOUT_FILENO);
        libc::close(writer);
    }
    let root = tree.join("root");
    let status = run(
        Serve {
            home: home.then(|| tree.join("home")),
            root: root.clone(),
            port: 0,
            hosts: vec!["127.0.0.1".into()],
            install: None,
        },
        false,
    );
    assert_eq!(status, 0, "the server's end");
    assert!(
        !port_file(&root).exists(),
        "the server stopped without clearing its port record"
    );
    eprintln!("{DONE}");
}

/// A pipe that takes no more: its writer filled while a write would not block, then made to block
/// again, so the next write to it, a whole line, waits until its reader is read; and how many bytes
/// fill it.
fn held() -> (std::io::PipeReader, std::io::PipeWriter, usize) {
    let (reader, mut writer) = sys::pipe().unwrap();
    sys::set_nonblocking(&writer, true).unwrap();
    let (filler, mut filled) = ([b'.'; 4096], 0);
    // Whole pages first, then single bytes: a page is one write that must fit whole where
    // PIPE_BUF is a page (Linux), so it can leave less than a page free.
    for chunk in [4096, 1] {
        loop {
            match writer.write(&filler[..chunk]) {
                Ok(wrote) => filled += wrote,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("filling the held pipe: {error}"),
            }
        }
    }
    // macOS grows a pipe a write does not fit, up to its largest: the line must find it full too.
    let line = format!("{URL_LINE}http://127.0.0.1:65535/  (pid 4194304)\n");
    assert_eq!(
        writer.write(line.as_bytes()).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the held pipe took a URL line"
    );
    sys::set_nonblocking(&writer, false).unwrap();
    (reader, writer, filled)
}

/// A held case's child: its server's stdout a pipe this side filled ([`held`]), so its URL line,
/// its first write there, waits until [`Held::release`] reads it; its stderr a file of its tree.
struct Held {
    child: sys::Child,
    root: PathBuf,
    stderr: PathBuf,
    reader: Option<std::io::PipeReader>,
    filled: usize,
    /// What the child said on its stdout past the filler, once [`Held::release`] read it to its end.
    said: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl Held {
    /// Run the case `test` (its name under `front::signals_tests`) again in a child of its own,
    /// which takes the child's side ([`serve_held`]) on `tree`.
    fn start(test: &str, tree: &Tree) -> Self {
        let (reader, writer, filled) = held();
        let stderr = tree.path("child.err");
        let child = sys::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("front::signals_tests::{test}"),
                "--nocapture",
            ])
            .env(TREE, &tree.root)
            .env(HELD, writer.as_raw_fd().to_string())
            .env_remove("TASKS_SESSION")
            // Unwatched, each signal a case sends ends the child, whatever this process does
            // with it.
            .default_signal(libc::SIGTERM)
            .default_signal(libc::SIGUSR1)
            .pass_fd(writer.as_raw_fd())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        // Only the child holds the writer from here, so the reader's end is the child's.
        drop(writer);
        Self {
            child,
            root: tree.path("root"),
            stderr,
            reader: Some(reader),
            filled,
            said: None,
        }
    }

    /// What the child said on its stderr, and its pid.
    fn output(&self) -> String {
        format!(
            "child {}: {}",
            self.child.id(),
            std::fs::read_to_string(&self.stderr).unwrap_or_default()
        )
    }

    /// Wait for `what` while the child lives: a child that ends first fails the case by its end.
    fn until<T>(&mut self, what: &str, mut event: impl FnMut() -> Option<T>) -> T {
        test_time::until(what, BACKSTOP, || {
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!(
                    "the child ended ({status}) before {what}: {}",
                    self.output()
                );
            }
            event()
        })
        .unwrap_or_else(|stall| panic!("{stall}: {}", self.output()))
    }

    /// Its server's port record's appearance, the first thing outside can see.
    fn seen(&mut self) {
        let record = port_file(&self.root);
        self.until("its port record's appearance", || {
            record.exists().then_some(())
        });
    }

    /// The port its server recorded, once it has.
    fn recorded(&mut self) -> u16 {
        let record = port_file(&self.root);
        self.until("its port recorded", || {
            std::fs::read_to_string(&record).ok()?.trim().parse().ok()
        })
    }

    /// Its server answering its status on `port`.
    fn answering(&mut self, port: u16) {
        self.until("an answer on its port", || answers(port).then_some(()));
    }

    /// Send `signal` to the child.
    fn signal(&self, signal: libc::c_int) {
        let pid = libc::pid_t::try_from(self.child.id()).unwrap();
        // SAFETY: kill(2) sends a signal to this case's own child, which it has not reaped.
        assert_eq!(
            unsafe { libc::kill(pid, signal) },
            0,
            "kill({pid}, {signal})"
        );
    }

    /// Read the held pipe, on a thread of its own to its end, which is the child's.
    fn release(&mut self) {
        let mut reader = self.reader.take().unwrap();
        let filled = self.filled;
        self.said = Some(std::thread::spawn(move || {
            let mut said = Vec::new();
            reader.read_to_end(&mut said).unwrap();
            said.split_off(filled.min(said.len()))
        }));
    }

    /// Its stderr saying `line`.
    fn tells(&mut self, line: &str) {
        let stderr = self.stderr.clone();
        self.until(&format!("its stderr to say {line:?}"), || {
            std::fs::read_to_string(&stderr)
                .is_ok_and(|said| said.contains(line))
                .then_some(())
        });
    }

    /// The child's end, which must be its own: no signal, a success, its case done, and its URL
    /// line said after it was held.
    fn ended(mut self) {
        let status = test_time::until("the child's end", BACKSTOP, || {
            self.child.try_wait().unwrap()
        })
        .unwrap_or_else(|stall| panic!("{stall}: {}", self.output()));
        let said = self.said.take().expect("the held pipe released");
        let said = String::from_utf8_lossy(&crate::testing::joined(
            said,
            "the held pipe's end",
            BACKSTOP,
        ))
        .into_owned();
        let output = format!("{}\nstdout past its filler: {said}", self.output());
        assert_eq!(
            status.signal(),
            None,
            "the child ended by a signal sent as its start held its URL line: its default \
             action, unwatched: {output}"
        );
        assert!(status.success(), "{output}");
        assert!(output.contains(DONE), "{output}");
        assert!(
            said.contains(URL_LINE),
            "no URL line once released: {output}"
        );
    }
}

/// A case's child that has not ended, its server held for good, is ended and reaped with it.
impl Drop for Held {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Whether the server on `port` answers its status (`/api/front`) with a 200.
fn answers(port: u16) -> bool {
    let Ok(mut stream) = sys::connect(&(std::net::Ipv4Addr::LOCALHOST, port).into(), BACKSTOP)
    else {
        return false;
    };
    let request =
        format!("GET /api/front HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let mut reply = String::new();
    stream.set_read_timeout(Some(BACKSTOP)).is_ok()
        && stream.write_all(request.as_bytes()).is_ok()
        && stream.read_to_string(&mut reply).is_ok()
        && reply.starts_with("HTTP/1.1 200 ")
}
