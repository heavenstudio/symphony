//! An owned, live process on the test board's data that never answers a health request. Readiness
//! and probes use datagrams; stdout is never read, and neither startup nor cleanup waits on its EOF.
//!
//! And a board's server that is up at once ([`healthy_server`]): a case that is about what `Home`
//! takes for its server, rather than about SurrealDB, starts this instead, so the case waits on
//! that server's own listener, not on a SurrealDB start or shutdown that load stretches.

use crate::common::{answer_healthy, script};
use crate::helper::{answer, event_of};
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use symphony_board::sys::{Child, Command, Stdio};

/// A `surreal` for `Home` whose server is up as soon as it listens: given a server's arguments
/// (`start … --bind ADDR DATA`), it runs this test binary's `helper_healthy` on ADDR, with the data
/// on its command line as a server's is, until this test process ends. Written into `dir`, and run
/// once already ([`script`]).
pub fn healthy_server(dir: &Path) -> PathBuf {
    script(
        &dir.join("healthy-server"),
        &format!(
            concat!(
                "for word do\n",
                "    [ \"$last\" = --bind ] && bind=$word\n",
                "    last=$word\n",
                "done\n",
                "export SYMPHONY_HOME_FIXTURE_OWNER={owner}\n",
                "exec '{exe}' --ignored --exact home_fixture::helper_healthy --test-threads=1 \\\n",
                "    start \"$bind\" \"$last\"",
            ),
            owner = std::process::id(),
            exe = std::env::current_exe().unwrap().display()
        ),
    )
}

const BOUND: Duration = Duration::from_secs(5);

pub struct Unresponsive {
    child: Child,
    events: UdpSocket,
    pub addr: String,
}

impl Unresponsive {
    pub fn start(data: &str) -> Self {
        let events = UdpSocket::bind("127.0.0.1:0").unwrap();
        let child = crate::store::spawn_with_input(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "home_fixture::helper_unresponsive",
                    "--test-threads=1",
                    data,
                ])
                .env(
                    "SYMPHONY_HOME_FIXTURE_EVENTS",
                    events.local_addr().unwrap().to_string(),
                )
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .unwrap();
        // Own the child before waiting: a missing ready event must still stop and reap it.
        let mut server = Self {
            child,
            events,
            addr: String::new(),
        };
        let mut ready = [0; 128];
        let size = event_of(&server.events, &mut server.child, &mut ready)
            .unwrap_or_else(|why| panic!("helper ready event: {why}"));
        server.addr = std::str::from_utf8(&ready[..size])
            .unwrap()
            .strip_prefix("READY ")
            .expect("ready event contains the bound address")
            .to_string();
        assert!(server.is_running(), "the ready helper must be alive");
        server
    }

    pub fn pid(&self) -> i64 {
        i64::from(self.child.id())
    }

    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    pub fn probes(&self) -> usize {
        self.events.set_nonblocking(true).unwrap();
        let mut count = 0;
        let mut event = [0; 128];
        loop {
            match self.events.recv(&mut event) {
                Ok(size) => {
                    assert_eq!(&event[..size], b"PROBE");
                    count += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return count,
                Err(error) => panic!("receiving health probe event: {error}"),
            }
        }
    }

    /// Close the control pipe, wait by child status with a deadline, and force/reap only this child
    /// if it misses that deadline. A killed helper's status fails the caller's normal-exit assertion.
    pub fn shutdown(&mut self) -> ExitStatus {
        drop(self.child.stdin.take());
        let began = Instant::now();
        while began.elapsed() < BOUND {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.child.kill().unwrap();
        self.child.wait().unwrap()
    }
}

impl Drop for Unresponsive {
    fn drop(&mut self) {
        // Also runs during an assertion/startup panic. No shell or grandchild holds a suite pipe.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn dropping_the_fixture_reaps_its_live_child() {
    let mut server = Unresponsive::start("surrealkv://fixture-drop");
    assert!(server.is_running());
    let pid = server.pid();
    drop(server);
    assert!(
        !symphony_board::sys::pid_alive(pid),
        "the owned child must be reaped"
    );
}

#[test]
#[ignore = "owned helper process for the unresponsive Home test"]
fn helper_unresponsive() {
    let socket = std::env::var("SYMPHONY_HOME_FIXTURE_EVENTS").expect("fixture event socket");
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    events.connect(socket).unwrap();
    events.set_write_timeout(Some(BOUND)).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stopping = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let stopping = &stopping;
        let events = &events;
        let worker = scope.spawn(move || {
            for connection in listener.incoming() {
                let connection = connection.unwrap();
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                // Close immediately: the patience assertion must measure Home's retries rather
                // than one slow health request consuming its socket timeout.
                // Report before closing so every completed health check has its event queued.
                events.send(b"PROBE").unwrap();
                drop(connection);
            }
        });
        answer(events, format!("READY {addr}").as_bytes());
        let mut byte = [0];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
        stopping.store(true, Ordering::SeqCst);
        let _wake = TcpStream::connect(addr).unwrap();
        worker.join().unwrap();
    });
}

#[test]
#[ignore = "owned helper process: the board's server that healthy_server starts"]
fn helper_healthy() {
    let owner: i64 = std::env::var("SYMPHONY_HOME_FIXTURE_OWNER")
        .expect("the test process that started it")
        .parse()
        .unwrap();
    let addr: SocketAddr = std::env::args()
        .find_map(|arg| arg.parse().ok())
        .expect("an address to listen on");
    let listener = TcpListener::bind(addr).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            answer_healthy(stream);
        }
    });
    while symphony_board::sys::pid_alive(owner) {
        std::thread::sleep(Duration::from_millis(50));
    }
}
