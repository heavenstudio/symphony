//! What the server asks of this machine's processes.

use std::fs::{self, File};
use std::io::{Read, Seek};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};
use symphony_board::sys::{Child, Command, Stdio};

/// The arguments of our serve process in ps's unquoted command line. The executable's path may
/// contain spaces too, so recognize its basename before reading any argument.
pub(crate) fn server_command(command: &str) -> Option<&str> {
    let (program, args) = command.split_once(" serve ")?;
    let name = std::path::Path::new(program).file_name()?;
    (name == "symphony-server" || name == "review-server").then_some(args)
}

/// Whether process `pid` exists; one this user may not signal still exists.
pub fn pid_alive(pid: i64) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false; // 0 and below name process groups, not a process
    }
    // SAFETY: signal 0 sends nothing; kill only checks the process exists and may be signalled.
    let signalled = unsafe { libc::kill(pid, 0) } == 0;
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Send `signal` to process `pid` (never a group: 0 and below are refused).
pub fn signal(pid: i64, signal: libc::c_int) {
    if let Ok(pid) = libc::pid_t::try_from(pid)
        && pid > 0
    {
        // SAFETY: one process, by the pid this server recorded or started.
        unsafe { libc::kill(pid, signal) };
    }
}

/// Send `signal` to a group this server started with its child as the group leader.
pub fn signal_group(leader: i64, signal: libc::c_int) {
    if let Ok(leader) = libc::pid_t::try_from(leader)
        && leader > 1
    {
        // SAFETY: the caller supplies only a group it created for a recorded child.
        unsafe { libc::killpg(leader, signal) };
    }
}

/// Whether a recorded child's own process group still has members, including its compiler children.
pub(crate) fn group_alive(leader: i64) -> bool {
    let Ok(leader) = libc::pid_t::try_from(leader) else {
        return false;
    };
    if leader <= 1 {
        return false;
    }
    // SAFETY: signal 0 sends nothing; the caller supplies only a group it created and recorded.
    let alive = unsafe { libc::killpg(leader, 0) } == 0;
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// This process's parent now.
pub fn parent() -> i64 {
    i64::from(std::os::unix::process::parent_id())
}

/// `name` on the PATH, as a shell finds it.
pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// What `command` prints, when it ends within `timeout`; one that does not is killed and is an error.
/// Its output is in hand when it ends, whatever its descendants still hold ([`output_while`]).
pub fn output_within(command: Command, timeout: Duration) -> Result<Output, String> {
    output_while(command, timeout, || true, moment)
}

/// The real time, on both clocks.
fn moment() -> (Instant, SystemTime) {
    (Instant::now(), SystemTime::now())
}

struct CommandGroup {
    child: Child,
    /// The command's program, which the line said before a kill names.
    program: String,
    stopped: bool,
}

impl CommandGroup {
    /// SIGKILL to the command's group, said on stderr first with the group and `why` whenever the
    /// group still has a member, so a log kept of a process that ended by it shows this sent it.
    fn stop(&mut self, why: &str) {
        self.stop_saying(why, |line| crate::front::tell(format_args!("{line}")));
    }

    /// [`CommandGroup::stop`], its line handed to `say`.
    fn stop_saying(&mut self, why: &str, say: impl FnOnce(&str)) {
        if !self.stopped {
            let group = i64::from(self.child.id());
            if group_alive(group) {
                say(&group_kill(group, &self.program, why));
            }
            signal_group(group, libc::SIGKILL);
            let _ = self.child.wait();
            self.stopped = true;
        }
    }
}

impl Drop for CommandGroup {
    fn drop(&mut self) {
        self.stop("its owner stopped waiting for it");
    }
}

/// What [`CommandGroup::stop`] says before it kills `group`, the group of a command running
/// `program`, for `why`.
fn group_kill(group: i64, program: &str, why: &str) -> String {
    format!("· Symphony: SIGKILL to process group {group} of `{program}`: {why}")
}

pub(crate) fn capture() -> std::io::Result<File> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "symphony-command-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    fs::remove_file(path)?;
    Ok(file)
}

pub(crate) fn captured(mut file: File) -> std::io::Result<Vec<u8>> {
    let length = file.metadata()?.len();
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Whether `timeout` has passed between two moments on either clock: the monotonic clock stops
/// while the machine sleeps, the wall clock does not, and a command that outlasts a sleep is over
/// its time.
fn outlasted(began: (Instant, SystemTime), now: (Instant, SystemTime), timeout: Duration) -> bool {
    now.0.saturating_duration_since(began.0) >= timeout
        || now
            .1
            .duration_since(began.1)
            .is_ok_and(|wall| wall >= timeout)
}

/// Own the command's group through completion, cancellation or `timeout`, measured on the clocks
/// `now` reads. Regular files keep a descendant's inherited output descriptor from holding a
/// reader or this deadline open forever. It runs without the server's session
/// ([`symphony::children`]), whoever hands it.
pub(crate) fn output_while(
    mut command: Command,
    timeout: Duration,
    mut running: impl FnMut() -> bool,
    mut now: impl FnMut() -> (Instant, SystemTime),
) -> Result<Output, String> {
    let out = capture().map_err(|e| e.to_string())?;
    let err = capture().map_err(|e| e.to_string())?;
    let program = command.get_program().to_string_lossy().into_owned();
    // A session of its own: the resulting group belongs only to this child.
    let child = symphony::children::sessionless(&mut command)
        .new_session()
        .stdin(Stdio::null())
        .stdout(out.try_clone().map_err(|e| e.to_string())?)
        .stderr(err.try_clone().map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut owned = CommandGroup {
        child,
        program,
        stopped: false,
    };
    let began = now();
    let status = loop {
        if let Some(status) = owned.child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if !running() {
            owned.stop("its caller cancelled it");
            return Err("command cancelled".into());
        }
        if outlasted(began, now(), timeout) {
            let why = format!("no answer within {} s", timeout.as_secs());
            owned.stop(&why);
            return Err(why);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    owned.stop("it ended, and this ends what it left running in its group");
    Ok(Output {
        status,
        stdout: captured(out).map_err(|e| e.to_string())?,
        stderr: captured(err).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_a_reaped_child_is_not() {
        assert!(pid_alive(i64::from(std::process::id())));
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        assert!(!pid_alive(pid));
        assert!(!pid_alive(0) && !pid_alive(-1));
    }

    #[test]
    fn a_command_is_over_its_time_when_either_clock_says_so() {
        let timeout = Duration::from_secs(240);
        let (mono, wall) = (
            Instant::now(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
        );
        let at = |awake: u64, elapsed: u64| {
            (
                mono + Duration::from_secs(awake),
                wall + Duration::from_secs(elapsed),
            )
        };
        assert!(!outlasted((mono, wall), at(239, 239), timeout));
        assert!(outlasted((mono, wall), at(240, 240), timeout));
        // A sleep during the run: one awake second, thirty-two minutes of wall time.
        assert!(outlasted((mono, wall), at(1, 1920), timeout));
        // A wall clock set back never ends a run early.
        let back = (
            mono + Duration::from_secs(10),
            wall - Duration::from_secs(60),
        );
        assert!(!outlasted((mono, wall), back, timeout));
    }

    #[test]
    fn a_command_that_outlasts_its_time_is_ended_and_an_error() {
        let mut slow = Command::new("sleep");
        slow.arg("30");
        let began = Instant::now();
        assert!(output_within(slow, Duration::from_millis(200)).is_err());
        assert!(began.elapsed() < Duration::from_secs(10));
        let mut quick = Command::new("echo");
        quick.arg("said");
        let out = output_within(quick, Duration::from_secs(30)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "said\n");
        let mut large = Command::new("sh");
        large.args(["-c", "head -c 131072 /dev/zero; printf warning >&2"]);
        let out = output_while(large, Duration::from_secs(30), || true, moment).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, vec![0; 131072]);
        assert_eq!(out.stderr, b"warning");
    }

    /// `command` started as [`output_while`] starts it, in a session of its own, owned as it owns
    /// it once the command itself has ended and been reaped.
    fn ended_group(command: &mut Command, program: &str) -> CommandGroup {
        let child = symphony::children::sessionless(command)
            .new_session()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut owned = CommandGroup {
            child,
            program: program.into(),
            stopped: false,
        };
        let began = Instant::now();
        while owned.child.try_wait().unwrap().is_none() {
            assert!(
                began.elapsed() < Duration::from_secs(30),
                "{program} ran on"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        owned
    }

    #[test]
    fn a_group_kill_is_said_with_its_group_program_and_why_only_while_it_has_a_member() {
        // The command ends at once and leaves a descendant in its group: the stop names the
        // group, the program and why, then the group ends.
        let mut leaves = Command::new("sh");
        leaves.args(["-c", "sleep 30 & exit 0"]);
        let mut owned = ended_group(&mut leaves, "sh");
        let group = i64::from(owned.child.id());
        assert!(group_alive(group), "the descendant holds the group");
        let mut said = Vec::new();
        owned.stop_saying("the case asks", |line| said.push(line.to_owned()));
        assert_eq!(
            said,
            [format!(
                "· Symphony: SIGKILL to process group {group} of `sh`: the case asks"
            )]
        );
        let stopped = Instant::now();
        while group_alive(group) {
            assert!(
                stopped.elapsed() < Duration::from_secs(10),
                "group {group} lives"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // A second stop sends nothing and says nothing.
        owned.stop_saying("again", |line| said.push(line.to_owned()));
        assert_eq!(said.len(), 1, "{said:?}");
        // A command that leaves nothing: there is nothing to kill, and nothing is said.
        let mut owned = ended_group(&mut Command::new("true"), "true");
        owned.stop_saying("nothing left", |line| said.push(line.to_owned()));
        assert_eq!(said.len(), 1, "{said:?}");
    }

    #[test]
    fn a_command_answers_when_it_ends_though_a_descendant_holds_its_output() {
        use std::io::{ErrorKind, Write};
        use std::os::fd::AsRawFd;

        // The descendant reads this pipe, which only the test's end finishes: it holds the
        // command's stdout and stderr until something ends it. A child another test spawns holds
        // neither end (symphony_board::sys::pipe).
        let (held, mut test_end) = symphony_board::sys::pipe().unwrap();
        let timeout = Duration::from_secs(30);
        let fd = held.as_raw_fd();
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "cat <&\"$1\" & printf said",
            "fixture",
            &fd.to_string(),
        ]);
        // fd belongs to held, kept alive through spawn; the child gets it at that number.
        command.pass_fd(fd);
        let (send, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || send.send(output_within(command, timeout)));
        // The command's own deadline: past it, only the test's end closing would let the call go.
        let out = answer
            .recv_timeout(timeout)
            .expect("no answer within the deadline while a descendant held the output")
            .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"said");
        // The owned group ended the descendant: the last reader closes, and a write here fails.
        drop(held);
        let stopped = Instant::now();
        loop {
            match test_end.write(&[0]) {
                Err(error) if error.kind() == ErrorKind::BrokenPipe => break,
                Ok(_) => {
                    assert!(stopped.elapsed() < timeout, "the descendant still reads");
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => panic!("group shutdown: {other:?}"),
            }
        }
    }

    #[test]
    fn cancellation_closes_the_descriptors_of_the_entire_owned_group() {
        use std::io::ErrorKind;
        use std::os::fd::AsRawFd;

        let (reader, writer) = symphony_board::sys::pipe().unwrap();
        symphony_board::sys::set_nonblocking(&reader, true).unwrap();
        let fd = writer.as_raw_fd();
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "sh -c 'printf r >&\"$1\"; exec sleep 30' fixture \"$1\" & wait",
            "fixture",
            &fd.to_string(),
        ]);
        // fd belongs to writer, kept alive through spawn; the child gets it at that number.
        command.pass_fd(fd);
        let mut ready = false;
        let result = output_while(
            command,
            Duration::from_secs(30),
            || {
                let mut input = &reader;
                match input.read(&mut [0]) {
                    Ok(1) => {
                        ready = true;
                        false
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => true,
                    other => panic!("readiness: {other:?}"),
                }
            },
            moment,
        );
        assert!(
            ready,
            "the child must fork its descriptor-holding descendant first"
        );
        assert_eq!(result.unwrap_err(), "command cancelled");
        drop(writer);
        let stopped = Instant::now();
        loop {
            let mut input = &reader;
            match input.read(&mut [0]) {
                Ok(0) => break,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(
                        stopped.elapsed() < Duration::from_secs(5),
                        "a surviving descendant would hold this pipe open"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => panic!("group shutdown: {other:?}"),
            }
        }
    }
}
