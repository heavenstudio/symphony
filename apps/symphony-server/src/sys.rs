//! What the server asks of this machine's processes.

use std::fs::{self, File};
use std::io::{Read, Seek};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};
use symphony_board::sys::{Child, Command, Start, Stdio, process_starts};

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

/// A process signalled by pid that is not this process's child to wait for (the server `--stop`
/// found, the one a hand-off asks): its pid and the start the kernel stamped on it when it was taken,
/// so a process the kernel gives that pid later is never signalled for it. The start is read again
/// just before `kill(2)`, not with it: between the two the process may end, be reaped and its pid be
/// given again, a window of microseconds that macOS, with no pidfd to signal through, cannot close
/// (Linux's `pidfd_send_signal` would). Linux dates a start in clock ticks (10 ms), so there a
/// process given the pid within a tick of the one taken reads as it; macOS in microseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    pub pid: i64,
    start: Start,
}

impl Taken {
    /// Process `pid` as it is now.
    pub fn of(pid: i64) -> Taken {
        Taken::started(pid, start_of(pid))
    }

    /// Process `pid` as it was taken earlier, when its start read `start`.
    pub fn started(pid: i64, start: Start) -> Taken {
        Taken { pid, start }
    }

    /// Whether its pid still names the process taken ([`same`]).
    pub fn alive(&self) -> bool {
        same(&self.start, &start_of(self.pid), || pid_alive(self.pid))
    }

    /// `signal` sent while its pid still names the process taken, `say` called just before:
    /// whether it was sent.
    pub fn signal(&self, how: libc::c_int, say: impl FnOnce()) -> bool {
        let alive = self.alive();
        if alive {
            say();
            signal(self.pid, how);
        }
        alive
    }
}

fn start_of(pid: i64) -> Start {
    process_starts(&[pid]).pop().unwrap_or(Start::Unreadable)
}

/// Whether a process whose start reads `now` is the one taken when it read `then`: the same start,
/// never one that has ended (a zombie among them) or had when taken, and, where either could not
/// be read, whether its pid is `alive`.
fn same(then: &Start, now: &Start, alive: impl FnOnce() -> bool) -> bool {
    match (then, now) {
        (Start::Ended, _) | (_, Start::Ended) => false,
        (Start::Unreadable, _) | (_, Start::Unreadable) => alive(),
        (then, now) => then == now,
    }
}

/// Send `signal` to a group this server started with its child as the group leader, unreaped so
/// the group's id is still its own ([`exited`]).
pub fn signal_group(leader: i64, signal: libc::c_int) {
    if let Ok(leader) = libc::pid_t::try_from(leader)
        && leader > 1
    {
        // SAFETY: the caller supplies only a group it created for a child it has not reaped.
        unsafe { libc::killpg(leader, signal) };
    }
}

/// Whether this process's `child` has ended, left unreaped (`waitid` with `WNOWAIT`): until it is
/// waited for, its pid, and the id of the group it leads, name it and no later process. One that
/// can no longer be waited for has ended; one stopped has not, which macOS answers as well though
/// only `WEXITED` is asked.
pub(crate) fn exited(child: &Child) -> bool {
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: asks whether this process's child ended, writing `info`, reaping none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::from(child.id()),
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    waited != 0 || ended(&info)
}

/// Whether `info`, as `waitid` wrote it, says its child exited or was killed.
fn ended(info: &libc::siginfo_t) -> bool {
    info.si_signo == libc::SIGCHLD
        && matches!(
            info.si_code,
            libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
        )
}

/// Whether a process of the group `leader` leads still runs: a zombie, its leader left unreaped
/// among them, runs nothing. macOS answers a signal 0 to a group of zombies alone EPERM; Linux
/// counts a zombie, so its `/proc` is read.
pub(crate) fn group_runs(leader: i64) -> bool {
    let Ok(leader) = libc::pid_t::try_from(leader) else {
        return false;
    };
    if leader <= 1 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = fs::read_dir("/proc") else {
            return true;
        };
        let group = leader.to_string();
        entries.flatten().any(|entry| {
            let stat = fs::read_to_string(entry.path().join("stat")).unwrap_or_default();
            let fields: Vec<&str> = stat
                .rsplit_once(')')
                .map_or_else(Vec::new, |(_, rest)| rest.split_whitespace().collect());
            fields.get(2) == Some(&group.as_str()) && !matches!(fields.first(), Some(&"Z" | &"X"))
        })
    }
    // SAFETY: signal 0 sends nothing; the caller supplies only a group it created for its child.
    #[cfg(not(target_os = "linux"))]
    unsafe {
        libc::killpg(leader, 0) == 0
    }
}

/// This process's parent now.
pub fn parent() -> i64 {
    i64::from(std::os::unix::process::parent_id())
}

/// How a wait for a process's end came out ([`until_ends`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The process ended.
    Ended,
    /// The stop came first.
    Stopped,
}

/// Until the process `pid`, this process's child or not, ends, or `stop` reads its end (its
/// pipe's every writer gone): which came first. The wait is the kernel's for that end (a kqueue's
/// `EVFILT_PROC` on macOS, a pidfd on Linux), with no timer. A process gone before the wait is set
/// has ended, and so has one `ended` says has, read once the wait is set: an end before then, its
/// pid given to another process since, counts. A process the kernel will not let this one follow
/// is an error.
pub(crate) fn until_ends(
    pid: i64,
    stop: &impl std::os::fd::AsFd,
    ended: impl Fn() -> bool,
) -> std::io::Result<End> {
    let pid = libc::pid_t::try_from(pid)
        .ok()
        .filter(|&pid| pid > 0)
        .ok_or_else(|| std::io::Error::other(format!("{pid} names no process")))?;
    ends::until(pid, std::os::fd::AsFd::as_fd(stop), ended)
}

/// [`until_ends`] on each kernel.
mod ends {
    use super::End;
    use std::io;
    use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

    /// Whether `problem` says the process is not there to follow: it ended already.
    fn gone(problem: &io::Error) -> bool {
        problem.raw_os_error() == Some(libc::ESRCH)
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn until(
        pid: libc::pid_t,
        stop: BorrowedFd<'_>,
        ended: impl Fn() -> bool,
    ) -> io::Result<End> {
        // SAFETY: kqueue takes no argument.
        let queue = unsafe { libc::kqueue() };
        if queue == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor nothing else owns; no child inherits a kqueue.
        let queue = unsafe { OwnedFd::from_raw_fd(queue) };
        let add = |ident: usize, filter: i16, fflags: u32| {
            let change = libc::kevent {
                ident,
                filter,
                flags: libc::EV_ADD,
                fflags,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: one change in and no room for an event out, so an error is the call's own.
            let added = unsafe {
                libc::kevent(
                    queue.as_raw_fd(),
                    &raw const change,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if added == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        };
        add(
            stop.as_raw_fd().unsigned_abs() as usize,
            libc::EVFILT_READ,
            0,
        )?;
        match add(
            pid.unsigned_abs() as usize,
            libc::EVFILT_PROC,
            libc::NOTE_EXIT,
        ) {
            Err(problem) if gone(&problem) => return Ok(End::Ended),
            added => added?,
        }
        if ended() {
            return Ok(End::Ended);
        }
        loop {
            // SAFETY: kevent is plain data, for which all zeroes is a value.
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: no change in, room for one event out, and no timeout: the wait is the event's.
            let seen = unsafe {
                libc::kevent(
                    queue.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &raw mut event,
                    1,
                    std::ptr::null(),
                )
            };
            if seen == -1 {
                let problem = io::Error::last_os_error();
                if problem.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(problem);
            }
            if event.flags & libc::EV_ERROR != 0 {
                return Err(io::Error::from_raw_os_error(
                    i32::try_from(event.data).unwrap_or(libc::EINVAL),
                ));
            }
            return Ok(if event.filter == libc::EVFILT_PROC {
                End::Ended
            } else {
                End::Stopped
            });
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn until(
        pid: libc::pid_t,
        stop: BorrowedFd<'_>,
        ended: impl Fn() -> bool,
    ) -> io::Result<End> {
        // SAFETY: pidfd_open takes a pid and no flags and returns a new descriptor, close-on-exec,
        // or -1.
        let opened = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if opened == -1 {
            let problem = io::Error::last_os_error();
            return if gone(&problem) {
                Ok(End::Ended)
            } else {
                Err(problem)
            };
        }
        let opened = std::os::fd::RawFd::try_from(opened).map_err(io::Error::other)?;
        // SAFETY: a new descriptor nothing else owns.
        let process = unsafe { OwnedFd::from_raw_fd(opened) };
        if ended() {
            return Ok(End::Ended);
        }
        let mut fds = [
            libc::pollfd {
                fd: process.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: two descriptors this call holds, and no timeout: the wait is the event's.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if ready == -1 {
                let problem = io::Error::last_os_error();
                if problem.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(problem);
            }
            if fds[0].revents != 0 {
                return Ok(End::Ended);
            }
            if fds[1].revents != 0 {
                return Ok(End::Stopped);
            }
        }
    }
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
pub(crate) fn moment() -> (Instant, SystemTime) {
    (Instant::now(), SystemTime::now())
}

struct CommandGroup {
    child: Child,
    /// The command's program, which the line said before a kill names.
    program: String,
    stopped: bool,
}

impl CommandGroup {
    /// SIGKILL to the command's group, said on stderr first with the group and `why` whenever a
    /// process of it still runs, so a log kept of a process that ended by it shows this sent it;
    /// then the command reaped: how it ended. It is reaped only after that signal, so the group's
    /// id is its own until then and the signal reaches no group numbered after it.
    fn stop(&mut self, why: &str) -> std::io::Result<ExitStatus> {
        self.stop_saying(why, |line| crate::front::tell(format_args!("{line}")))
    }

    /// [`CommandGroup::stop`], its line handed to `say`.
    fn stop_saying(&mut self, why: &str, say: impl FnOnce(&str)) -> std::io::Result<ExitStatus> {
        if !self.stopped {
            let group = i64::from(self.child.id());
            if group_runs(group) {
                say(&group_kill(group, &self.program, why));
            }
            signal_group(group, libc::SIGKILL);
            self.stopped = true;
        }
        self.child.wait()
    }
}

impl Drop for CommandGroup {
    fn drop(&mut self) {
        let _ = self.stop("its owner stopped waiting for it");
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

/// What [`output_while`] answers for a command its caller stopped before it ended, or before it
/// started.
const CANCELLED: &str = "command cancelled";

/// Whether `timeout` has passed between two moments on either clock: the monotonic clock stops
/// while the machine sleeps, the wall clock does not, and a command that outlasts a sleep is over
/// its time.
pub(crate) fn outlasted(
    began: (Instant, SystemTime),
    now: (Instant, SystemTime),
    timeout: Duration,
) -> bool {
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
///
/// `running` is asked before the spawn, so a command handed over once its caller stopped never
/// starts, and at each look after the command's end is read, so a stop seen by then wins over an
/// end it raced: no command's answer comes back once its caller stopped, however late a loaded
/// machine lets this look.
pub(crate) fn output_while(
    mut command: Command,
    timeout: Duration,
    mut running: impl FnMut() -> bool,
    mut now: impl FnMut() -> (Instant, SystemTime),
) -> Result<Output, String> {
    if !running() {
        return Err(CANCELLED.into());
    }
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
    loop {
        let ended = exited(&owned.child);
        if !running() {
            let _ = owned.stop("its caller cancelled it");
            return Err(CANCELLED.into());
        }
        if ended {
            break;
        }
        if outlasted(began, now(), timeout) {
            let why = format!("no answer within {} s", timeout.as_secs());
            let _ = owned.stop(&why);
            return Err(why);
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for a child's answer until its deadline, where its pipe is the event"
        )]
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = owned
        .stop("it ended, and this ends what it left running in its group")
        .map_err(|e| e.to_string())?;
    Ok(Output {
        status,
        stdout: captured(out).map_err(|e| e.to_string())?,
        stderr: captured(err).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The deadline these tests give a command that ends by itself, and the backstop of their waits:
    /// six times the 30 s they had as a bound, which only a stall reaches.
    const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

    /// Waits until `condition` holds, failing as a stall past [`BACKSTOP`].
    #[track_caller]
    fn until(what: &str, condition: impl FnMut() -> bool) {
        test_time::until_true(what, BACKSTOP, condition).unwrap_or_else(|stall| panic!("{stall}"));
    }

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
        // It sleeps a day: only its end at the deadline lets the call answer.
        let mut slow = Command::new("sleep");
        slow.arg("86400");
        let ending = std::thread::spawn(move || output_within(slow, Duration::from_millis(200)));
        until("the outlasting command to be ended", || {
            ending.is_finished()
        });
        assert!(ending.join().unwrap().is_err());
        let mut quick = Command::new("echo");
        quick.arg("said");
        let out = output_within(quick, BACKSTOP).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "said\n");
        let mut large = Command::new("sh");
        large.args(["-c", "head -c 131072 /dev/zero; printf warning >&2"]);
        let out = output_while(large, BACKSTOP, || true, moment).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, vec![0; 131072]);
        assert_eq!(out.stderr, b"warning");
        let mut failing = Command::new("sh");
        failing.args(["-c", "exit 3"]);
        let out = output_within(failing, BACKSTOP).unwrap();
        assert_eq!(out.status.code(), Some(3), "the status of its reap");
    }

    #[test]
    fn a_start_names_the_process_taken_and_an_unreadable_one_falls_back_to_its_pid() {
        let (at, later) = (Start::At(1_800_000_000.25), Start::At(1_800_000_001.0));
        let alive = || true;
        assert!(same(&at, &at, || false), "the same start needs no pid");
        // A start that differs is a process the kernel gave the pid later.
        assert!(!same(&at, &later, alive));
        for (then, now) in [(&at, &Start::Ended), (&Start::Ended, &at)] {
            assert!(!same(then, now, alive), "{then:?} {now:?}");
        }
        assert!(!same(&Start::Unreadable, &Start::Ended, alive));
        for (then, now) in [
            (&at, &Start::Unreadable),
            (&Start::Unreadable, &at),
            (&Start::Unreadable, &Start::Unreadable),
        ] {
            assert!(same(then, now, || true), "{then:?} {now:?}");
            assert!(!same(then, now, || false), "{then:?} {now:?}");
        }
    }

    #[test]
    fn a_taken_process_is_signalled_only_while_its_pid_names_it() {
        // It lives until the case kills it, or until its input closes: a wait on one never killed
        // ends at once, as `wait` closes that input first.
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = i64::from(child.id());
        let taken = Taken::of(pid);
        assert!(taken.alive());
        // The same pid with another start, recorded earlier: a process the kernel gave it later.
        let other = Taken::started(pid, Start::At(1.0));
        assert!(!other.alive());
        let mut said = 0;
        assert!(!other.signal(libc::SIGKILL, || said += 1));
        assert_eq!(said, 0, "nothing sent, nothing said");
        assert!(
            child.try_wait().unwrap().is_none(),
            "the other was signalled"
        );
        assert!(taken.signal(libc::SIGKILL, || said += 1));
        assert_eq!(said, 1);
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&child.wait().unwrap()),
            Some(libc::SIGKILL)
        );
        assert!(!taken.alive(), "a reaped child is gone");
        assert!(!taken.signal(libc::SIGTERM, || said += 1));
        assert!(!Taken::of(0).alive() && !Taken::of(-1).alive());
        let unread = Taken::started(i64::from(std::process::id()), Start::Unreadable);
        assert!(unread.alive(), "of unknown start, known by its pid");
    }

    #[test]
    fn a_child_that_ended_is_left_unreaped_holding_its_pid_until_it_is_waited_for() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        until("true to end", || exited(&child));
        // A zombie: no other process can have its pid or lead a group of its id.
        assert!(pid_alive(pid));
        assert_eq!(process_starts(&[pid]), [Start::Ended]);
        assert!(!group_runs(pid), "its group runs nothing");
        assert!(exited(&child), "asking again reaps nothing");
        assert!(child.wait().unwrap().success());
        assert!(!pid_alive(pid));
        assert!(exited(&child), "a reaped child has ended too");
    }

    #[test]
    fn a_stopped_child_has_not_exited() {
        // It sleeps a day: it lives until the case kills it.
        let mut child = Command::new("/bin/sleep").arg("86400").spawn().unwrap();
        let pid = i64::from(child.id());
        signal(pid, libc::SIGSTOP);
        // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits for this test's own child to stop, reaping none.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(child.id()),
                &raw mut info,
                libc::WSTOPPED | libc::WNOWAIT,
            )
        };
        let stopped = exited(&child);
        signal(pid, libc::SIGCONT);
        let resumed = exited(&child);
        // Killed before any assertion, so a failing one leaves no stopped process behind.
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert_eq!((waited, info.si_code), (0, libc::CLD_STOPPED));
        assert!(!stopped, "a stopped child was taken for one that exited");
        assert!(!resumed, "a continued child was taken for one that exited");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGKILL)
        );
    }

    /// `command` started as [`output_while`] starts it, in a session of its own, owned as it owns
    /// it once the command itself has ended, left unreaped.
    fn ended_group(command: &mut Command, program: &str) -> CommandGroup {
        let child = symphony::children::sessionless(command)
            .new_session()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let owned = CommandGroup {
            child,
            program: program.into(),
            stopped: false,
        };
        until(&format!("{program} to end"), || exited(&owned.child));
        owned
    }

    #[test]
    fn a_group_kill_is_said_with_its_group_program_and_why_only_while_it_has_a_member() {
        // The command ends at once and leaves a descendant in its group, which sleeps a day: the
        // stop names the group, the program and why, then the group ends, its leader reaped only
        // after.
        let mut leaves = Command::new("sh");
        leaves.args(["-c", "sleep 86400 & exit 0"]);
        let mut owned = ended_group(&mut leaves, "sh");
        let group = i64::from(owned.child.id());
        assert!(group_runs(group), "the descendant holds the group");
        assert!(pid_alive(group), "its leader is unreaped, its id its own");
        let mut said = Vec::new();
        let status = owned.stop_saying("the case asks", |line| said.push(line.to_owned()));
        assert!(status.unwrap().success(), "the leader's own end");
        assert_eq!(
            said,
            [format!(
                "· Symphony: SIGKILL to process group {group} of `sh`: the case asks"
            )]
        );
        until(&format!("group {group} to end"), || !group_runs(group));
        // A second stop sends nothing and says nothing, and answers the same end.
        let again = owned.stop_saying("again", |line| said.push(line.to_owned()));
        assert!(again.unwrap().success());
        assert_eq!(said.len(), 1, "{said:?}");
        // A command that leaves nothing: there is nothing to kill, and nothing is said.
        let mut owned = ended_group(&mut Command::new("true"), "true");
        let _ = owned.stop_saying("nothing left", |line| said.push(line.to_owned()));
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
        let timeout = BACKSTOP;
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
            .unwrap_or_else(|_| {
                panic!(
                    "no answer within the deadline while a descendant held the output; {}",
                    test_time::Load::now()
                )
            })
            .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"said");
        // The owned group ended the descendant: the last reader closes, and a write here fails.
        drop(held);
        until("the descendant to stop reading", || {
            match test_end.write(&[0]) {
                Err(error) if error.kind() == ErrorKind::BrokenPipe => true,
                Ok(_) => false,
                other => panic!("group shutdown: {other:?}"),
            }
        });
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
            "sh -c 'printf r >&\"$1\"; exec sleep 86400' fixture \"$1\" & wait",
            "fixture",
            &fd.to_string(),
        ]);
        // fd belongs to writer, kept alive through spawn; the child gets it at that number.
        command.pass_fd(fd);
        let mut ready = false;
        let result = output_while(
            command,
            BACKSTOP,
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
        // A surviving descendant, which sleeps a day, would hold this pipe open.
        until("the group's end to close its pipe", || {
            let mut input = &reader;
            match input.read(&mut [0]) {
                Ok(0) => true,
                Err(error) if error.kind() == ErrorKind::WouldBlock => false,
                other => panic!("group shutdown: {other:?}"),
            }
        });
    }

    #[test]
    fn a_command_handed_over_once_its_caller_stopped_never_starts() {
        // No such program: a spawn would answer that it is not there, and the stop answers first.
        let tree = crate::testing::Tree::new();
        let mut asked = 0;
        let result = output_while(
            Command::new(tree.path("no-such-program")),
            BACKSTOP,
            || {
                asked += 1;
                false
            },
            moment,
        );
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert_eq!(asked, 1, "its caller is asked once, before any spawn");
    }

    #[test]
    fn a_stop_wins_over_the_end_it_raced() {
        use std::io::ErrorKind;
        use std::os::fd::AsRawFd;

        // The command holds the pipe's write end, and once the case lets its own go after the
        // spawn, the pipe ends with the command: a process's descriptors close before its end can
        // be read, so the caller stops by every look that reads it. The old order, which left the
        // loop on an end read without asking its caller, answered Ok here whenever it read the end.
        let (reader, writer) = symphony_board::sys::pipe().unwrap();
        symphony_board::sys::set_nonblocking(&reader, true).unwrap();
        let tree = crate::testing::Tree::new();
        let ran = tree.path("ran");
        let mut command = Command::new("sh");
        command
            .args(["-c", "echo ran > \"$1\"", "fixture"])
            .arg(&ran);
        command.pass_fd(writer.as_raw_fd());
        let mut writer = Some(writer);
        let mut asked = 0;
        let result = output_while(
            command,
            BACKSTOP,
            || {
                asked += 1;
                if asked > 1 {
                    drop(writer.take()); // spawned: the command's copy is the one left
                }
                let mut input = &reader;
                match input.read(&mut [0]) {
                    Ok(0) => false,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => true,
                    other => panic!("the command's end: {other:?}"),
                }
            },
            moment,
        );
        assert_eq!(
            fs::read_to_string(&ran).unwrap(),
            "ran\n",
            "it ran to its end"
        );
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert!(asked >= 2, "asked before the spawn and after: {asked}");
    }

    #[test]
    fn a_command_its_caller_keeps_running_answers_its_end() {
        let mut asked = 0;
        let mut command = Command::new("sh");
        command.args(["-c", "printf said; printf told >&2; exit 3"]);
        let out = output_while(
            command,
            BACKSTOP,
            || {
                asked += 1;
                true
            },
            moment,
        )
        .unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(
            (&out.stdout[..], &out.stderr[..]),
            (&b"said"[..], &b"told"[..])
        );
        assert!(
            asked >= 2,
            "asked before the spawn and at the look that read its end: {asked}"
        );
    }

    /// A child that lives until the case kills it: it sleeps a day.
    fn sleeper() -> Child {
        Command::new("/bin/sleep").arg("86400").spawn().unwrap()
    }

    /// [`until_ends`] of `pid` on a thread, beside the stop pipe `stop`, `set` told once its wait
    /// is set (as its `ended` is asked): how it came out, read within [`BACKSTOP`].
    fn followed(
        pid: i64,
        stop: std::io::PipeReader,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Receiver<std::io::Result<End>>,
    ) {
        let (set, is_set) = std::sync::mpsc::channel();
        let (came, came_out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let end = until_ends(pid, &stop, || {
                let _ = set.send(());
                false
            });
            let _ = came.send(end);
        });
        (is_set, came_out)
    }

    #[test]
    fn a_process_followed_wakes_its_follower_as_it_ends() {
        let mut child = sleeper();
        let (stop, _writer) = symphony_board::sys::pipe().unwrap();
        let (is_set, came_out) = followed(i64::from(child.id()), stop);
        is_set.recv_timeout(BACKSTOP).expect("the wait to be set");
        // Killed once the wait is set: the kernel's word of its end is what wakes it.
        child.kill().unwrap();
        let end = came_out.recv_timeout(BACKSTOP).expect("the end to wake it");
        assert_eq!(end.unwrap(), End::Ended);
        child.wait().unwrap();
    }

    #[test]
    fn a_stop_wakes_the_follower_of_a_process_that_goes_on() {
        let mut child = sleeper();
        let (stop, writer) = symphony_board::sys::pipe().unwrap();
        let (is_set, came_out) = followed(i64::from(child.id()), stop);
        is_set.recv_timeout(BACKSTOP).expect("the wait to be set");
        drop(writer);
        let end = came_out
            .recv_timeout(BACKSTOP)
            .expect("the stop to wake it");
        assert_eq!(end.unwrap(), End::Stopped);
        assert!(!exited(&child), "the process goes on");
        // A stop already over when the wait is set ends it as it begins.
        let (stop, writer) = symphony_board::sys::pipe().unwrap();
        drop(writer);
        assert_eq!(
            until_ends(i64::from(child.id()), &stop, || false).unwrap(),
            End::Stopped
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn a_process_ended_before_the_wait_is_set_has_ended() {
        let (stop, _writer) = symphony_board::sys::pipe().unwrap();
        // A zombie, holding its pid unreaped, so no other process can be given it meanwhile.
        let mut child = Command::new("true").spawn().unwrap();
        until("true to end", || exited(&child));
        assert_eq!(
            until_ends(i64::from(child.id()), &stop, || false).unwrap(),
            End::Ended
        );
        child.wait().unwrap();
        // One whose end `ended` says, its pid perhaps another's since: ended, though one runs.
        let mut running = sleeper();
        assert_eq!(
            until_ends(i64::from(running.id()), &stop, || true).unwrap(),
            End::Ended
        );
        running.kill().unwrap();
        running.wait().unwrap();
        for pid in [0, -1, i64::from(i32::MAX) + 1] {
            let refused = until_ends(pid, &stop, || false).unwrap_err();
            assert_eq!(refused.to_string(), format!("{pid} names no process"));
        }
    }
}
