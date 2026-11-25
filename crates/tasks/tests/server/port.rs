//! `symphony_board::port`: a child started on a reserved loopback port, and again on another port only when
//! it ends saying its port was taken, a bounded number of times within one deadline; and whether a
//! process itself listens on an address. The fake children are shells that say what the case needs
//! and end, or say they listen and run until their input closes.

use crate::children::Child as _;
use std::convert::Infallible;
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};
use symphony_board::port::{
    ATTEMPTS, LSOF_AT, NotStarted, Start, in_use, listens, start_on_free_port,
};
use symphony_board::sys;
use symphony_board::sys::{Child, Command};

/// The deadline of a case whose starts are all quick: one it never comes near, however loaded the
/// machine, the backstop of the 20 s it once had.
fn later() -> Instant {
    Instant::now() + test_time::backstop(Duration::from_secs(20))
}

/// What one fake child does.
#[derive(Clone, Copy)]
enum Fake {
    /// Says it listens on its address, and runs until its input closes.
    Binds,
    /// Says on stderr that its address is taken, and ends.
    Taken,
    /// Says on stderr why it cannot start, and ends.
    Fails,
}

/// Starts fake children, one per start, each doing the next thing `plan` says (the last one again
/// once it runs out), and keeps the address each start was given.
struct Starter {
    plan: Vec<Fake>,
    given: Vec<SocketAddr>,
}

impl Starter {
    fn start(&mut self, addr: SocketAddr) -> Start<Child> {
        let fake = self.plan[self.given.len().min(self.plan.len() - 1)];
        self.given.push(addr);
        let script = match fake {
            Fake::Binds => r#"echo "listening on $1"; read -r line || :"#,
            Fake::Taken => r#"echo "start $1: Address already in use (os error 48)" >&2; exit 1"#,
            Fake::Fails => r#"echo "start $1: the disk is full" >&2; exit 1"#,
        };
        let mut child = sys::spawn_piped(Command::of("/bin/sh").args([
            "-c",
            script,
            "fake",
            &addr.to_string(),
        ]))
        .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        if line == format!("listening on {addr}\n") {
            return Start::Up(child);
        }
        let mut said = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut said)
            .unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(1));
        Start::Ended(said)
    }
}

type Started = Result<(Child, SocketAddr), NotStarted<Infallible>>;

fn run(plan: &[Fake], attempts: usize, until: Instant) -> (Vec<SocketAddr>, Started) {
    let mut starter = Starter {
        plan: plan.to_vec(),
        given: Vec::new(),
    };
    let started = start_on_free_port(attempts, until, |addr| Ok(starter.start(addr)));
    (starter.given, started)
}

/// Lets a fake child that is up go: its input closes, so it ends, and well.
fn stop(mut child: Child) {
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn a_child_that_binds_at_once_is_started_once_on_the_loopback_address_it_was_given() {
    let (given, started) = run(&[Fake::Binds], ATTEMPTS, later());
    let (child, addr) = started.unwrap();
    assert_eq!(given, [addr]);
    assert!(addr.ip().is_loopback(), "{addr}");
    assert_ne!(addr.port(), 0);
    stop(child);
}

#[test]
fn a_child_whose_port_was_taken_is_started_again_on_another_port() {
    let (given, started) = run(&[Fake::Taken, Fake::Binds], ATTEMPTS, later());
    let (child, addr) = started.unwrap();
    assert_eq!(given.len(), 2, "{given:?}");
    assert_eq!(given[1], addr);
    assert_ne!(given[0].port(), addr.port());
    stop(child);
}

#[test]
fn a_child_whose_port_is_taken_on_every_start_is_given_up_on_with_what_the_last_said() {
    let (given, started) = run(&[Fake::Taken], ATTEMPTS, later());
    let Err(NotStarted::InUse { attempts, output }) = &started else {
        panic!("a port taken each time was not given up on: {started:?}");
    };
    assert_eq!(*attempts, ATTEMPTS);
    assert_eq!(given.len(), ATTEMPTS);
    let last = given.last().unwrap();
    assert_eq!(
        *output,
        format!("start {last}: Address already in use (os error 48)\n")
    );
    let mut ports: Vec<u16> = given.iter().map(SocketAddr::port).collect();
    symphony_board::order::sort_by(&mut ports, &Ord::cmp);
    ports.dedup();
    assert_eq!(ports.len(), ATTEMPTS, "a port was tried twice: {given:?}");
    assert_eq!(
        started.unwrap_err().to_string(),
        format!(
            "its port was taken on each of {ATTEMPTS} starts; the last one said: \
             start {last}: Address already in use (os error 48)\n"
        )
    );
}

#[test]
fn a_child_that_ends_for_another_reason_is_not_started_again() {
    let (given, started) = run(&[Fake::Fails, Fake::Binds], ATTEMPTS, later());
    let Err(NotStarted::Ended(output)) = &started else {
        panic!("a child that failed was not a failure: {started:?}");
    };
    assert_eq!(given.len(), 1, "{given:?}");
    assert_eq!(*output, format!("start {}: the disk is full\n", given[0]));
    assert_eq!(
        started.unwrap_err().to_string(),
        format!(
            "it ended before it answered: start {}: the disk is full\n",
            given[0]
        )
    );
}

#[test]
fn no_attempts_still_makes_one_start() {
    let (given, started) = run(&[Fake::Taken], 0, later());
    assert!(
        matches!(started, Err(NotStarted::InUse { attempts: 1, .. })),
        "{started:?}"
    );
    assert_eq!(given.len(), 1);
}

#[test]
fn no_start_is_begun_once_the_deadline_has_passed() {
    let (given, started) = run(&[Fake::Taken, Fake::Binds], ATTEMPTS, Instant::now());
    let Err(NotStarted::InUse { attempts, output }) = &started else {
        panic!("a start was begun after the deadline: {started:?}");
    };
    assert_eq!(*attempts, 1);
    assert_eq!(given.len(), 1, "{given:?}");
    assert_eq!(
        *output,
        format!("start {}: Address already in use (os error 48)\n", given[0])
    );
}

#[test]
fn a_starters_own_failure_is_returned_at_once() {
    let mut starts = 0;
    let started = start_on_free_port(ATTEMPTS, later(), |_| {
        starts += 1;
        Err::<Start<()>, _>("the server could not be run")
    });
    assert!(
        matches!(
            started,
            Err(NotStarted::Failed("the server could not be run"))
        ),
        "{started:?}"
    );
    assert_eq!(starts, 1);
    assert_eq!(
        started.unwrap_err().to_string(),
        "the server could not be run"
    );
}

#[test]
fn a_taken_address_is_read_in_the_words_systems_and_servers_print() {
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let refused = TcpListener::bind(held.local_addr().unwrap()).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::AddrInUse);
    for said in [
        refused.to_string(),
        format!("{refused:?}"),
        format!("{:?}", std::io::Error::from(std::io::ErrorKind::AddrInUse)),
        "ERROR surrealdb_server::cli: Address already in use (os error 48)".into(),
        "Error: Address already in use (os error 98)".into(),
        "listen tcp 127.0.0.1:8000: bind: address already in use".into(),
    ] {
        assert!(in_use(&said), "{said}");
    }
    for said in [
        "",
        "There was a problem with the datastore: the disk is full",
        "the data folder is in use by another server",
    ] {
        assert!(!in_use(said), "{said}");
    }
}

#[test]
fn a_process_listens_on_the_address_of_its_own_listener_alone() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(listens(std::process::id(), addr, sys::output).unwrap());
    let connected = TcpStream::connect(addr).unwrap();
    let local = connected.local_addr().unwrap();
    assert!(
        !listens(std::process::id(), local, sys::output).unwrap(),
        "a connected socket on {local} is no listener"
    );
    // Binding port zero assigns a different port; no process can listen on zero itself.
    let free = "127.0.0.1:0".parse().unwrap();
    assert!(
        !listens(std::process::id(), free, sys::output).unwrap(),
        "{free}"
    );
}

#[test]
fn lsof_runs_through_the_runner_it_is_handed_and_what_it_said_is_the_answer() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    /// An lsof that ends with `code`, having printed `stdout` and `stderr`.
    fn said(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
    let asked = |addr: &str, answer: Output| {
        let mut runs = Vec::new();
        let listening = listens(42, addr.parse().unwrap(), |command: &mut Command| {
            runs.push(words(command));
            Ok(answer.clone())
        });
        (listening, runs)
    };

    let (listening, runs) = asked("127.0.0.1:7001", said(0, "p42\nf5\n", ""));
    assert!(listening.unwrap());
    assert_eq!(
        runs,
        ["lsof -nP -w -a -p 42 -iTCP@127.0.0.1:7001 -sTCP:LISTEN -Fp"]
    );
    let (listening, runs) = asked("[::1]:7001", said(0, "p42\n", ""));
    assert!(listening.unwrap());
    assert_eq!(
        runs,
        ["lsof -nP -w -a -p 42 -iTCP@[::1]:7001 -sTCP:LISTEN -Fp"]
    );
    // Nothing listening: lsof exits 1 and says nothing.
    assert!(!asked("127.0.0.1:7001", said(1, "", "")).0.unwrap());
    // Another process's listener there is not this one's.
    assert!(!asked("127.0.0.1:7001", said(0, "p420\n", "")).0.unwrap());
    let refused = asked("127.0.0.1:7001", said(1, "", "lsof: no such option -w\n"))
        .0
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "lsof could not say what pid 42 listens on: lsof: no such option -w"
    );
}

/// A command's program and arguments, as one line.
fn words(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn lsof_found_by_no_name_is_run_at_usr_sbin_and_one_that_cannot_run_is_no_answer() {
    use std::io::{Error, ErrorKind};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    const ARGS: &str = "-nP -w -a -p 42 -iTCP@127.0.0.1:7001 -sTCP:LISTEN -Fp";

    /// What `listens` comes to when each run of its `lsof` answers as `answers` says, in turn, and
    /// the command each run was.
    fn asked(answers: Vec<std::io::Result<Output>>) -> (std::io::Result<bool>, Vec<String>) {
        let mut answers = answers.into_iter();
        let mut runs = Vec::new();
        let listening = listens(
            42,
            "127.0.0.1:7001".parse().unwrap(),
            |command: &mut Command| {
                runs.push(words(command));
                answers.next().expect("no run past the answers given")
            },
        );
        (listening, runs)
    }
    let found = || {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: b"p42\n".to_vec(),
            stderr: Vec::new(),
        })
    };
    let by_name = format!("lsof {ARGS}");
    let at = format!("{LSOF_AT} {ARGS}");
    assert_eq!(LSOF_AT, "/usr/sbin/lsof");

    // One found by name is the one asked: no second run.
    let (listening, runs) = asked(vec![found()]);
    assert!(listening.unwrap());
    assert_eq!(runs, std::slice::from_ref(&by_name));
    // None by name, as a PATH without /usr/sbin finds none on macOS: the one at /usr/sbin answers.
    let (listening, runs) = asked(vec![Err(Error::from(ErrorKind::NotFound)), found()]);
    assert!(listening.unwrap());
    assert_eq!(runs, [by_name.clone(), at.clone()]);
    // None there either: no answer, never a no, its kind kept and both places said.
    let (listening, runs) = asked(vec![
        Err(Error::from_raw_os_error(2)),
        Err(Error::from_raw_os_error(2)),
    ]);
    let not_run = listening.unwrap_err();
    assert_eq!(not_run.kind(), ErrorKind::NotFound);
    assert_eq!(
        not_run.to_string(),
        "lsof could not run: none is found on PATH, and /usr/sbin/lsof could not run either (No \
         such file or directory (os error 2))"
    );
    assert_eq!(runs, [by_name.clone(), at]);
    // One found by name that may not be run is the answer: nothing else is run in its place.
    let (listening, runs) = asked(vec![Err(Error::from_raw_os_error(13))]);
    let denied = listening.unwrap_err();
    assert_eq!(denied.kind(), ErrorKind::PermissionDenied);
    assert_eq!(
        denied.to_string(),
        "lsof could not run: Permission denied (os error 13)"
    );
    assert_eq!(runs, [by_name]);
}

#[test]
fn another_process_does_not_listen_on_this_ones_address() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut other =
        crate::helper::spawn_with_input(Command::of("/bin/sh").args(["-c", "read -r line || :"]))
            .unwrap();
    assert!(!listens(other.id(), addr, sys::output).unwrap());
    drop(other.stdin.take());
    assert!(other.wait().unwrap().success());
    assert!(
        !listens(other.id(), addr, sys::output).unwrap(),
        "an ended process"
    );
}
