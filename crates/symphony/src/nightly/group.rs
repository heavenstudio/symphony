//! The nightly's children, each in a process group of its own and bounded in time: past its time the
//! whole group is stopped, so no helper it started (a remote's transport, a test's server) runs on.

use std::io;
use std::path::Path;
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use symphony_board::sys::{Command, spawn_output, wait_output};

/// How long a stopped group has to end on SIGTERM before SIGKILL.
pub(crate) const GRACE: Duration = Duration::from_secs(10);
/// How often a running check is looked at.
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// Why a bounded command gave no output.
#[derive(Debug)]
pub(crate) enum Failed {
    /// It could not start, or its output could not be read.
    Io(io::Error),
    /// It ran past its time, so long, and its group was killed.
    Late(Duration),
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

/// `command` run in a process group of its own, stdin null and its output read, until `until`:
/// a command still running then has its whole group killed.
pub(crate) fn output_until(command: &mut Command, until: Instant) -> Result<Output, Failed> {
    let started = Instant::now();
    command.process_group(0);
    let child = spawn_output(command).map_err(Failed::Io)?;
    let pid = child.id();
    let (tell, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tell.send(wait_output(child));
    });
    match heard.recv_timeout(until.saturating_duration_since(Instant::now())) {
        Ok(output) => output.map_err(Failed::Io),
        Err(_) => {
            // Its output is read to its end before it is reaped, so a command still writing is
            // unreaped and its group is its own.
            signal(group_of(pid), libc::SIGKILL);
            let _ = heard.recv();
            Err(Failed::Late(started.elapsed()))
        }
    }
}

/// The processes that hold `file` open, by `lsof`: none when no process does.
pub(crate) fn holders(file: &Path) -> Result<Vec<u32>, String> {
    let done = output_until(
        crate::children::command("lsof")
            .args(["-w", "-t", "--"])
            .arg(file),
        Instant::now() + Duration::from_secs(60),
    )
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run lsof: {e}"),
        Failed::Late(took) => format!("lsof did not end within {} s", took.as_secs()),
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
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(3));
        assert_eq!(done.stdout, b"out\n");
        assert_eq!(done.stderr, b"err\n");
        let missing = output_until(
            &mut command("/no/such/program"),
            Instant::now() + Duration::from_secs(30),
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
            Instant::now() + Duration::from_millis(500),
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
