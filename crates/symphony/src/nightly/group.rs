//! The nightly's children, each in a process group of its own and bounded ([`Bound`]): past the
//! run's time, or once the run is told to stop, the whole group is stopped, so no helper it started
//! (a remote's transport, a test's server) runs on. A group is told from a later one given the same
//! id by its leader's start ([`Stamp`], [`lives`]).

use std::io;
use std::path::Path;
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use symphony_board::sys::{Boottime, Command, Start, process_starts, spawn_output, wait_output};

/// How long a stopped group has to end on SIGTERM before SIGKILL.
pub(crate) const GRACE: Duration = Duration::from_secs(10);
/// How often a running child is looked at.
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// What a run's waits end at: its time, or sooner when it is told to stop (`heard`, the run's
/// HUP, INT or TERM).
#[derive(Clone, Copy)]
pub(crate) struct Bound<'a> {
    pub(crate) until: Instant,
    heard: Option<&'a dyn Fn() -> bool>,
}

impl<'a> Bound<'a> {
    /// At `until` alone.
    pub(crate) fn until(until: Instant) -> Bound<'a> {
        Bound { until, heard: None }
    }

    /// At `until`, or once `heard` says the run was told to stop.
    pub(crate) fn hearing(until: Instant, heard: &'a dyn Fn() -> bool) -> Bound<'a> {
        Bound {
            until,
            heard: Some(heard),
        }
    }

    /// Whether the run was told to stop.
    pub(crate) fn interrupted(&self) -> bool {
        self.heard.is_some_and(|heard| heard())
    }

    /// The same, ending `most` from now when that is sooner.
    pub(crate) fn sooner(self, most: Duration) -> Bound<'a> {
        Bound {
            until: self.until.min(Instant::now() + most),
            ..self
        }
    }

    /// The same, ending no sooner than `least` from now.
    pub(crate) fn later(self, least: Duration) -> Bound<'a> {
        Bound {
            until: self.until.max(Instant::now() + least),
            ..self
        }
    }
}

/// Why a bounded command gave no output.
#[derive(Debug)]
pub(crate) enum Failed {
    /// It could not start, or its output could not be read.
    Io(io::Error),
    /// It ran past its time, so long, and its group was killed.
    Late(Duration),
    /// The run was told to stop after it had run so long, and its group was killed.
    Interrupted(Duration),
}

/// When a process started, as the kernel stamps it: what tells a group's leader from a later
/// process given the same id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum Stamp {
    /// In seconds since the epoch (macOS).
    At(f64),
    /// On the clock since boot (Linux).
    AfterBoot(Boottime),
}

/// The start of the process `pid`, when it runs and can be read.
pub(crate) fn stamp(pid: i64) -> Option<Stamp> {
    match process_starts(&[pid]).pop()? {
        Start::At(at) => Some(Stamp::At(at)),
        Start::AfterBoot(boot) => Some(Stamp::AfterBoot(boot)),
        Start::Ended | Start::Unreadable => None,
    }
}

/// Whether the process group `group`, whose leader started at `start`, still has a process: its
/// leader, or what that left running when it ended. A group of that id whose living leader started
/// at another time is another group; one whose start cannot be told counts as that group.
pub(crate) fn lives(group: i64, start: Option<&Stamp>) -> bool {
    let Ok(id) = libc::pid_t::try_from(group) else {
        return false;
    };
    if id <= 1 {
        return false;
    }
    // SAFETY: signal 0 sends nothing: killpg only says whether the group has a process.
    let asked = unsafe { libc::killpg(id, 0) };
    if asked != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        return false;
    }
    match (start, stamp(group)) {
        (Some(start), Some(now)) => *start == now,
        _ => true,
    }
}

/// The process group a child leading its own is.
fn group_of(pid: u32) -> libc::pid_t {
    libc::pid_t::try_from(pid).unwrap_or(libc::pid_t::MAX)
}

/// `signal` to the process group `group`, which a child of this process leads.
pub(crate) fn signal(group: libc::pid_t, signal: libc::c_int) {
    // SAFETY: killpg sends a signal and touches no memory of this process. The group is one this
    // process made for its own child, whose pid (the group's id) stays reserved while it is
    // unreaped or while any process of the group lives.
    unsafe {
        libc::killpg(group, signal);
    }
}

/// Whether this process's child `pid` has ended, left unreaped (`WNOWAIT`), so its pid still names
/// it and its group; one that cannot be waited for any more has ended.
pub(crate) fn ended(pid: u32) -> bool {
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: reads whether one child of this process ended into `info`, waiting for and reaping
    // none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::from(pid),
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    waited != 0 || info.si_signo == libc::SIGCHLD
}

/// Stop the group the child `pid` leads: SIGTERM, up to [`GRACE`] for its leader to end, then
/// SIGKILL to whatever of it is left. The leader is left unreaped, for its owner to wait for.
pub(crate) fn stop(pid: u32, grace: Duration) {
    let group = group_of(pid);
    signal(group, libc::SIGTERM);
    let until = Instant::now() + grace;
    while !ended(pid) && Instant::now() < until {
        std::thread::sleep(POLL);
    }
    signal(group, libc::SIGKILL);
}

/// `command` run in a process group of its own, stdin null and its output read, within `bound`: a
/// command still running when it ends has its whole group killed.
pub(crate) fn output_until(command: &mut Command, bound: Bound<'_>) -> Result<Output, Failed> {
    let started = Instant::now();
    command.process_group(0);
    let child = spawn_output(command).map_err(Failed::Io)?;
    let pid = child.id();
    let (tell, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tell.send(wait_output(child));
    });
    loop {
        let left = bound.until.saturating_duration_since(Instant::now());
        let stopped = match heard.recv_timeout(left.min(POLL)) {
            Ok(output) => return output.map_err(Failed::Io),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Failed::Io(io::Error::other(
                    "its waiter ended without a word",
                )));
            }
            Err(mpsc::RecvTimeoutError::Timeout) if bound.interrupted() => {
                Failed::Interrupted(started.elapsed())
            }
            Err(mpsc::RecvTimeoutError::Timeout) if left.is_zero() => {
                Failed::Late(started.elapsed())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        // Its output is read to its end before it is reaped, so a command still writing is
        // unreaped and its group is its own.
        signal(group_of(pid), libc::SIGKILL);
        let _ = heard.recv();
        return Err(stopped);
    }
}

/// The processes that hold `file` open, by `lsof`: none when no process does.
pub(crate) fn holders(file: &Path) -> Result<Vec<u32>, String> {
    let done = output_until(
        crate::children::command("lsof")
            .args(["-w", "-t", "--"])
            .arg(file),
        Bound::until(Instant::now() + Duration::from_secs(60)),
    )
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run lsof: {e}"),
        Failed::Late(took) | Failed::Interrupted(took) => {
            format!("lsof did not end within {} s", took.as_secs())
        }
    })?;
    let pids: Vec<u32> = String::from_utf8_lossy(&done.stdout)
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    if pids.is_empty() && done.status.code() != Some(1) && !done.status.success() {
        return Err(format!(
            "lsof failed on {}: {}",
            file.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(pids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::children::command;
    use crate::test_tree::Tree;

    fn alive(pid: u32) -> bool {
        symphony_board::sys::pid_alive(i64::from(pid))
    }

    #[test]
    fn a_command_within_its_time_answers_what_it_printed() {
        let done = output_until(
            command("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
            Bound::until(Instant::now() + Duration::from_secs(30)),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(3));
        assert_eq!(done.stdout, b"out\n");
        assert_eq!(done.stderr, b"err\n");
        let missing = output_until(
            &mut command("/no/such/program"),
            Bound::until(Instant::now() + Duration::from_secs(30)),
        );
        assert!(matches!(missing, Err(Failed::Io(_))), "{missing:?}");
    }

    #[test]
    fn a_command_past_its_time_is_stopped_with_what_it_started() {
        let tree = Tree::new();
        let pid = tree.path("helper.pid");
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid.display());
        let started = Instant::now();
        let late = output_until(
            command("sh").args(["-c", &script]),
            Bound::until(Instant::now() + Duration::from_millis(500)),
        );
        assert!(matches!(late, Err(Failed::Late(_))), "{late:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
        let helper: u32 = std::fs::read_to_string(&pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while alive(helper) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        assert!(!alive(helper), "the helper went with the command's group");
    }

    #[test]
    fn a_command_is_stopped_with_its_group_once_the_run_is_told_to_stop() {
        let started = Instant::now();
        let heard = move || started.elapsed() >= Duration::from_millis(300);
        let stopped = output_until(
            command("sh").args(["-c", "sleep 30"]),
            Bound::hearing(Instant::now() + Duration::from_secs(60), &heard),
        );
        assert!(
            matches!(stopped, Err(Failed::Interrupted(_))),
            "{stopped:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(20));
        let quiet = || false;
        let bound = Bound::hearing(Instant::now() + Duration::from_secs(60), &quiet);
        assert!(!bound.interrupted());
        assert!(!Bound::until(Instant::now()).interrupted());
        let soon = Instant::now() + Duration::from_secs(1);
        assert!(Bound::until(soon).sooner(Duration::from_secs(60)).until == soon);
        assert!(Bound::until(soon).later(Duration::from_secs(60)).until > soon);
    }

    #[test]
    fn a_group_lives_while_any_of_it_runs_and_not_as_a_later_group_of_its_id() {
        let tree = Tree::new();
        let left = tree.path("left.pid");
        let script = format!("sleep 30 & echo $! > '{}'; sleep 1", left.display());
        let mut leader = command("sh")
            .args(["-c", &script])
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i64::from(leader.id());
        let start = stamp(group);
        assert!(start.is_some(), "a running process's start is read");
        assert!(lives(group, start.as_ref()));
        assert!(
            lives(group, None),
            "a group whose start is unknown counts as it"
        );
        let other = match start {
            Some(Stamp::At(at)) => Stamp::At(at - 1.0),
            _ => Stamp::At(0.0),
        };
        assert!(
            !lives(group, Some(&other)),
            "a leader that started at another time leads another group"
        );
        // Its leader gone, what it left in the group is the group still.
        leader.wait().unwrap();
        assert!(lives(group, start.as_ref()));
        signal(libc::pid_t::try_from(group).unwrap(), libc::SIGKILL);
        let helper: i64 = std::fs::read_to_string(&left)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while lives(group, start.as_ref()) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        assert!(
            !lives(group, start.as_ref()),
            "helper {helper} went with it"
        );
        for nothing in [0, 1, -5, i64::MAX] {
            assert!(!lives(nothing, None), "{nothing}");
        }
    }

    #[test]
    fn a_stopped_group_ends_on_term_and_its_leader_is_left_to_be_reaped() {
        let mut child = command("sh")
            .args(["-c", "sleep 30"])
            .process_group(0)
            .spawn()
            .unwrap();
        assert!(!ended(child.id()));
        stop(child.id(), GRACE);
        assert!(ended(child.id()));
        let status = child.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGTERM)
        );
        assert!(ended(child.id()), "a reaped child has ended too");
    }

    #[test]
    fn a_group_that_ignores_term_is_killed_after_the_grace() {
        let mut child = command("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        stop(child.id(), Duration::from_millis(300));
        let status = child.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGKILL)
        );
    }

    #[test]
    fn the_holders_of_a_file_are_the_processes_that_hold_it_open() {
        let tree = Tree::new();
        let file = tree.write("index.lock", "");
        assert_eq!(holders(&file), Ok(Vec::new()));
        let mut holder = command("sleep")
            .arg("30")
            .stdin(std::fs::File::open(&file).unwrap())
            .spawn()
            .unwrap();
        let held = holders(&file).unwrap();
        assert_eq!(held, [holder.id()]);
        holder.kill().unwrap();
        holder.wait().unwrap();
        assert_eq!(holders(&tree.path("none")), Ok(Vec::new()));
    }
}
