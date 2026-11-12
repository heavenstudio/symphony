//! The CPU a process takes, the meter of the timings: user and system time, the work it did, which
//! the machine's load does not multiply as it multiplies a wall. [`taken`] reads a live process
//! (the board's server) as it runs, [`reaped`] what a child took in all once it ends.

use std::io::Read as _;
use std::process::{ExitStatus, Output};
use std::time::Duration;
use symphony_board::sys::Child;

/// The user and system CPU `pid`, a live process, has taken so far: its own threads', not its
/// children's.
#[cfg(target_os = "macos")]
pub fn taken(pid: u32) -> Duration {
    let raw = libc::pid_t::try_from(pid).expect("a process id");
    // SAFETY: plain data, for which all zeroes is a value.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: writes one rusage_info_v2, the size of the flavor asked for, into `info`.
    let read = unsafe { libc::proc_pid_rusage(raw, libc::RUSAGE_INFO_V2, (&raw mut info).cast()) };
    assert_eq!(
        read,
        0,
        "the CPU of process {pid}: {}",
        std::io::Error::last_os_error()
    );
    // The kernel counts it in the machine's ticks (`task_power_info`), not nanoseconds.
    let ticks = u128::from(info.ri_user_time) + u128::from(info.ri_system_time);
    let (numer, denom) = timebase();
    Duration::from_nanos(u64::try_from(ticks * numer / denom).expect("CPU in range"))
}

/// The nanoseconds of the machine's ticks, as a fraction.
#[cfg(target_os = "macos")]
#[expect(
    deprecated,
    reason = "libc's mach_timebase_info, which points at the mach2 crate this workspace does not take"
)]
fn timebase() -> (u128, u128) {
    let mut base = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: writes the timebase into `base`.
    let read = unsafe { libc::mach_timebase_info(&raw mut base) };
    assert!(
        read == 0 && base.numer != 0 && base.denom != 0,
        "the machine's timebase: {read}"
    );
    (u128::from(base.numer), u128::from(base.denom))
}

/// The user and system CPU `pid`, a live process, has taken so far: its own threads', not its
/// children's, from `/proc/PID/stat` in the clock's ticks.
#[cfg(target_os = "linux")]
pub fn taken(pid: u32) -> Duration {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .unwrap_or_else(|why| panic!("the CPU of process {pid}: {why}"));
    // After the command's closing parenthesis, the state is the first field and utime and stime
    // the twelfth and thirteenth (`proc(5)`'s 14 and 15).
    let after = stat.rsplit_once(')').expect("a stat line").1;
    let fields: Vec<&str> = after.split_whitespace().collect();
    let ticks: u64 = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    // SAFETY: sysconf reads a configuration value.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let per_second = u64::try_from(per_second).expect("clock ticks a second");
    Duration::from_nanos(ticks * 1_000_000_000 / per_second)
}

/// `child`, started with its outputs piped, read to their end and reaped: what it said, how it
/// ended, and the user and system CPU it took with every child it reaped, as its reap reports it.
pub fn reaped(mut child: Child) -> (Output, Duration) {
    drop(child.stdin.take());
    let stderr = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut said = Vec::new();
            stderr.read_to_end(&mut said).map(|_| said)
        })
    });
    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout).expect("the child's stdout");
    }
    let stderr = match stderr {
        Some(reader) => reader.join().unwrap().expect("the child's stderr"),
        None => Vec::new(),
    };
    let pid = libc::pid_t::try_from(child.id()).expect("a process id");
    loop {
        let mut status = 0;
        // SAFETY: plain data, for which all zeroes is a value.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this process's child `pid`, writing its status and its usage. `Child`
        // neither reaps nor signals it once dropped.
        let waited = unsafe { libc::wait4(pid, &raw mut status, 0, &raw mut usage) };
        if waited == pid {
            use std::os::unix::process::ExitStatusExt as _;
            let output = Output {
                status: ExitStatus::from_raw(status),
                stdout,
                stderr,
            };
            return (output, time(usage.ru_utime) + time(usage.ru_stime));
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::Interrupted,
            "the reap of {pid}: {error}"
        );
    }
}

/// A `timeval` as a duration.
fn time(value: libc::timeval) -> Duration {
    let seconds = u64::try_from(value.tv_sec).expect("a time since none");
    let micros = u64::try_from(value.tv_usec).expect("a time since none");
    Duration::from_secs(seconds) + Duration::from_micros(micros)
}

#[cfg(test)]
mod meter {
    use super::*;
    use std::io::{BufRead as _, BufReader, Write as _};
    use symphony_board::sys::{self, Command};

    /// A child's CPU read while it lives, once it has done its work and waits on its input, is what
    /// its reap says it took in all, in the same unit; the wait between counts for nothing; and the
    /// work counts whatever the load, as a count of steps, not a time, fixes it.
    #[test]
    fn a_childs_cpu_read_while_it_waits_is_what_its_reap_says_and_its_wait_is_none() {
        let script = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done; echo worked; read _";
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let mut child = sys::spawn_piped(&mut command).expect("sh starts");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line, "worked\n");
        let worked = taken(child.id());
        std::thread::sleep(Duration::from_millis(300));
        let waited = taken(child.id());
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        child.stdout = Some(stdout.into_inner());
        let (output, all) = reaped(child);
        assert!(output.status.success(), "{output:?}");
        eprintln!("cpu: worked {worked:?}, after its wait {waited:?}, reaped {all:?}");
        assert!(
            worked >= Duration::from_millis(20),
            "a hundred thousand steps of sh took {worked:?}"
        );
        assert!(
            waited - worked < Duration::from_millis(20),
            "its wait took {:?}",
            waited - worked
        );
        assert!(
            waited <= all && all - waited < Duration::from_millis(50),
            "read while it waited {waited:?}, reaped {all:?}"
        );
    }
}
