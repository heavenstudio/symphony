//! Waiting on a helper process this test binary started: its readiness is a datagram it sends
//! ([`answer`]), waited for while the helper lives and given up at once when it exits first
//! ([`event_of`]), never against a wall-clock bound that load can exceed. And a child started with
//! an input of its own ([`spawn_with_input`]).

use crate::children::Child as _;
use crate::common::signal;
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use symphony_board::sys::{Child, Command, Stdio};

/// `command` started with its stdin a [`symphony_board::sys::pipe`], kept as `child.stdin`, and its
/// other streams as `command` sets them: what the child reads ends once the test drops that input,
/// never later because a child another test spawned took a copy of it. The store tests' own
/// (`tests/store/common.rs`), which this target no longer includes.
pub fn spawn_with_input(command: &mut Command) -> std::io::Result<Child> {
    let (input, stdin) = symphony_board::sys::pipe()?;
    let mut child = command.stdin(input).spawn()?;
    child.stdin = Some(std::os::fd::OwnedFd::from(stdin).into());
    Ok(child)
}

/// How long [`event_of`] reads before it looks at its helper again.
const LOOK: Duration = Duration::from_millis(50);

/// The byte [`event_of`] writes on its helper's input for each answer it takes.
const ACK: u8 = b'a';

/// The helper's next answer on `events`, written into `message`: its size; or, once `helper` has
/// exited with none taken, how it ended. A helper process says it is ready with that datagram, and
/// load can stretch its start past any wall-clock bound, so a live helper is waited on for as long
/// as it lives, while one that exits first (a panic, a refused start, a signal) ends the wait at
/// once. Each answer taken is acknowledged on the helper's input, and a helper [`answer`]s by
/// waiting for that byte, so it cannot exit with its answer still in flight (loopback delivers a
/// datagram after its send returns): an exit seen before the acknowledgement is a helper whose
/// answer never came. Leaves `events` blocking, with no read timeout.
pub fn event_of(
    events: &UdpSocket,
    helper: &mut Child,
    message: &mut [u8],
) -> Result<usize, String> {
    use std::io::ErrorKind::{Interrupted, TimedOut, WouldBlock};
    let pid = helper.id();
    events.set_nonblocking(false).unwrap();
    events.set_read_timeout(Some(LOOK)).unwrap();
    let got = loop {
        if let Some(status) = helper.try_wait().unwrap() {
            break Err(format!(
                "helper {pid} exited before its answer was taken: {status}"
            ));
        }
        match events.recv(message) {
            Ok(size) => break acknowledge(helper).map(|()| size),
            Err(error) if matches!(error.kind(), WouldBlock | TimedOut | Interrupted) => {}
            Err(error) => break Err(format!("receiving helper {pid}'s datagram: {error}")),
        }
    };
    events.set_read_timeout(None).unwrap();
    got
}

/// Lets `helper` past its [`answer`]; if it has gone, how it ended.
fn acknowledge(helper: &mut Child) -> Result<(), String> {
    let pid = helper.id();
    let input = helper
        .stdin
        .as_mut()
        .unwrap_or_else(|| panic!("helper {pid} has no input to acknowledge on"));
    input.write_all(&[ACK]).map_err(|error| {
        let status = helper.wait().unwrap();
        format!("helper {pid} ended as it answered ({error}): {status}")
    })
}

/// A helper's side of [`event_of`]: sends `message` on `events` (connected to the waiting test's
/// socket), then waits on stdin for its acknowledgement, which must come before the helper goes on
/// or exits.
pub fn answer(events: &UdpSocket, message: &[u8]) {
    events.send(message).unwrap();
    let mut ack = [0];
    std::io::stdin()
        .read_exact(&mut ack)
        .expect("the test acknowledges every answer it takes");
    assert_eq!(ack, [ACK]);
}

/// A stand-in helper for [`event_of`]: `cat`, alive until its input is dropped, saying nothing.
fn quiet_helper() -> Child {
    spawn_with_input(
        Command::of("cat")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .unwrap()
}

#[test]
fn a_live_helper_that_never_answers_is_waited_on_until_it_ends_and_its_end_is_named() {
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut helper = quiet_helper();
    let pid = i64::from(helper.id());
    let patience = Duration::from_secs(1);
    let began = Instant::now();
    // Twenty looks pass with the helper alive and silent; only its end stops the wait.
    let killer = std::thread::spawn(move || {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a test holds for a length of time to show the wait goes on while the helper lives, where a count of the wait's looks is the event"
        )]
        std::thread::sleep(patience);
        signal(pid, "KILL");
    });
    let why = event_of(&events, &mut helper, &mut [0; 64]).unwrap_err();
    killer.join().unwrap();
    assert!(began.elapsed() >= patience, "{:?}", began.elapsed());
    assert!(why.contains(&format!("helper {pid} exited")), "{why}");
    assert!(why.contains("signal: 9"), "{why}");
}

#[test]
fn a_helper_that_exits_before_it_answers_fails_the_wait_naming_its_exit() {
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut helper = Command::of("sh")
        .args(["-c", "exit 3"])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let why = event_of(&events, &mut helper, &mut [0; 64]).unwrap_err();
    assert_eq!(
        why,
        format!(
            "helper {} exited before its answer was taken: exit status: 3",
            helper.id()
        )
    );
}

#[test]
fn a_helper_that_answers_late_is_heard_acknowledged_and_left_running() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("event-of-late-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let read = dir.join("read");
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    // Left nonblocking, as `Unresponsive::probes` leaves its socket: the wait blocks it again.
    events.set_nonblocking(true).unwrap();
    let to = events.local_addr().unwrap();
    // `cat` stands in for the helper, writing out the acknowledgements it is given.
    let mut helper = spawn_with_input(
        Command::of("cat")
            .stdout(std::fs::File::create(&read).unwrap())
            .stderr(Stdio::null()),
    )
    .unwrap();
    let answer = std::thread::spawn(move || {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: the stand-in answers ten of the wait's looks late, to show it looks again, where a count of the wait's looks is the event"
        )]
        std::thread::sleep(LOOK * 10);
        let from = UdpSocket::bind("127.0.0.1:0").unwrap();
        from.send_to(b"ready", to).unwrap();
    });
    let mut message = [0; 64];
    let size = event_of(&events, &mut helper, &mut message).unwrap();
    answer.join().unwrap();
    assert_eq!(&message[..size], b"ready");
    assert!(
        helper.try_wait().unwrap().is_none(),
        "the helper still runs"
    );
    assert_eq!(events.read_timeout().unwrap(), None);
    drop(helper.stdin.take());
    assert!(helper.wait().unwrap().success());
    assert_eq!(std::fs::read(&read).unwrap(), [ACK], "one acknowledgement");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A helper that had gone before its answer was taken never got past its [`answer`]: a datagram
/// waiting for it is not taken for its readiness.
#[test]
fn a_helper_gone_before_its_answer_was_taken_fails_the_wait() {
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut helper = quiet_helper();
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"ready", events.local_addr().unwrap())
        .unwrap();
    drop(helper.stdin.take());
    assert!(helper.wait().unwrap().success());
    let why = event_of(&events, &mut helper, &mut [0; 64]).unwrap_err();
    assert_eq!(
        why,
        format!(
            "helper {} exited before its answer was taken: exit status: 0",
            helper.id()
        )
    );
}

/// How many helpers the send-then-exit case starts, one after another.
const ROUNDS: usize = 100;

/// swift-crane-810's race, at 7eacfd63: a helper that sends and exits at once could be reaped
/// before its datagram arrived, and its answer was missed. Each round's helper answers with its
/// round and exits the moment it is let go; each answer is heard, and each helper exits clean.
#[test]
fn every_helper_that_answers_then_exits_at_once_is_heard() {
    for round in 0..ROUNDS {
        let events = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut helper = spawn_with_input(
            Command::of(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "helper::helper_answers_then_exits",
                    "--test-threads=1",
                ])
                .env(
                    "SYMPHONY_EVENT_OF_EVENTS",
                    events.local_addr().unwrap().to_string(),
                )
                .env("SYMPHONY_EVENT_OF_ROUND", round.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit()),
        )
        .unwrap();
        let mut message = [0; 64];
        let size = event_of(&events, &mut helper, &mut message)
            .unwrap_or_else(|why| panic!("round {round}: {why}"));
        assert_eq!(
            std::str::from_utf8(&message[..size]).unwrap(),
            format!("ready {round}")
        );
        let status = helper.wait().unwrap();
        assert!(status.success(), "round {round}: {status}");
    }
}

#[test]
#[ignore = "owned helper process for event_of's send-then-exit rounds"]
fn helper_answers_then_exits() {
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    events
        .connect(std::env::var("SYMPHONY_EVENT_OF_EVENTS").unwrap())
        .unwrap();
    let round = std::env::var("SYMPHONY_EVENT_OF_ROUND").unwrap();
    answer(&events, format!("ready {round}").as_bytes());
}
