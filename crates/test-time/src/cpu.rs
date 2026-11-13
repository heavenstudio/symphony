//! The CPU a piece of work took: its user and system time, which a loaded machine does not stretch
//! as it stretches a wall. [`thread`] and [`process`] read this process's own clocks, [`spent`] what
//! a closure took on the calling thread, [`of`] a live process by its id, [`reap`] a child at its
//! end with every child it reaped, as `wait4` reports it, and [`children`] what the children this
//! process reaped took in all.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

#[cfg(test)]
mod tests;

/// The CPU the calling thread has taken so far (`CLOCK_THREAD_CPUTIME_ID`).
pub fn thread() -> Duration {
    clock(libc::CLOCK_THREAD_CPUTIME_ID)
}

/// The CPU every thread of this process has taken so far (`CLOCK_PROCESS_CPUTIME_ID`), not its
/// children's.
pub fn process() -> Duration {
    clock(libc::CLOCK_PROCESS_CPUTIME_ID)
}

/// `work` done, and the CPU the calling thread took for it: what another thread or a child does
/// meanwhile is not in it, and neither is a wait.
pub fn spent<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let before = thread();
    let done = work();
    (done, thread().saturating_sub(before))
}

/// The CPU the children this process has reaped took, with every child they reaped
/// (`RUSAGE_CHILDREN`): a child's share is in it once it has been waited for. It is the whole
/// process's: in a test binary, every other test's reaped children are in it too, so a test bills
/// its own child by [`reap`] (or a live one by [`of`]), never by a difference of this.
pub fn children() -> Duration {
    // SAFETY: plain data, for which all zeroes is a value.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: writes one rusage into `usage`.
    let read = unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &raw mut usage) };
    assert_eq!(
        read,
        0,
        "the reaped children's CPU: {}",
        io::Error::last_os_error()
    );
    timeval(usage.ru_utime) + timeval(usage.ru_stime)
}

/// The CPU `pid`, a live process, has taken so far: its own threads', not its children's. A
/// process that is gone, or one this user may not read, is an error.
#[cfg(target_os = "macos")]
pub fn of(pid: u32) -> io::Result<Duration> {
    let raw = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    // SAFETY: plain data, for which all zeroes is a value.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: writes one rusage_info_v2, the size of the flavor asked for, into `info`.
    let read = unsafe { libc::proc_pid_rusage(raw, libc::RUSAGE_INFO_V2, (&raw mut info).cast()) };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    // The kernel counts it in the machine's ticks (`task_power_info`), not nanoseconds.
    let ticks = u128::from(info.ri_user_time) + u128::from(info.ri_system_time);
    let (numer, denom) = timebase();
    let nanos = u64::try_from(ticks * numer / denom).map_err(io::Error::other)?;
    Ok(Duration::from_nanos(nanos))
}

/// How long one of the ticks [`of`] counts in lasts: the machine's, a few tens of nanoseconds.
#[cfg(target_os = "macos")]
pub fn tick() -> Duration {
    let (numer, denom) = timebase();
    Duration::from_nanos(u64::try_from(numer.div_ceil(denom)).expect("a tick in range"))
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

/// The CPU `pid`, a live process, has taken so far: its own threads', not its children's, from
/// `/proc/PID/stat` in the clock's ticks. A process that is gone is an error.
#[cfg(target_os = "linux")]
pub fn of(pid: u32) -> io::Result<Duration> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let ticks =
        stat_ticks(&stat).ok_or_else(|| io::Error::other(format!("a stat line: {stat}")))?;
    Ok(Duration::from_nanos(
        ticks * 1_000_000_000 / ticks_a_second(),
    ))
}

/// How long one of the ticks [`of`] counts in lasts: the clock's, 10 ms at 100 Hz.
#[cfg(target_os = "linux")]
pub fn tick() -> Duration {
    Duration::from_nanos(1_000_000_000 / ticks_a_second())
}

/// The clock's ticks a second, which `/proc/PID/stat` counts in.
#[cfg(target_os = "linux")]
fn ticks_a_second() -> u64 {
    // SAFETY: sysconf reads a configuration value.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(per_second).expect("clock ticks a second")
}

/// A stat line's user and system ticks: after the command's last closing parenthesis, the state is
/// the first field and utime and stime the twelfth and thirteenth (`proc(5)`'s 14 and 15).
#[cfg(any(target_os = "linux", test))]
fn stat_ticks(stat: &str) -> Option<u64> {
    let after = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after.split_whitespace().collect();
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

/// Waits for `pid`, a child of this process, to end and reaps it: how it ended and the CPU it took
/// with every child it reaped, as its reap reports it. Its outputs are read already, or go where
/// they cannot fill; nothing else may reap it (a `std::process::Child` is never waited on after).
/// One that is not this process's child, or was reaped already, is an error.
pub fn reap(pid: u32) -> io::Result<(ExitStatus, Duration)> {
    use std::os::unix::process::ExitStatusExt as _;
    let raw = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    loop {
        let mut status = 0;
        // SAFETY: plain data, for which all zeroes is a value.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this process's child `raw`, writing its status and its usage.
        let waited = unsafe { libc::wait4(raw, &raw mut status, 0, &raw mut usage) };
        if waited == raw {
            let cpu = timeval(usage.ru_utime) + timeval(usage.ru_stime);
            return Ok((ExitStatus::from_raw(status), cpu));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn clock(id: libc::clockid_t) -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: writes one timespec into `time`.
    let read = unsafe { libc::clock_gettime(id, &raw mut time) };
    assert_eq!(read, 0, "clock {id}: {}", io::Error::last_os_error());
    let seconds = u64::try_from(time.tv_sec).expect("a clock since none");
    let nanos = u32::try_from(time.tv_nsec).expect("nanoseconds under a second");
    Duration::new(seconds, nanos)
}

/// A `timeval` as a duration.
fn timeval(time: libc::timeval) -> Duration {
    let seconds = u64::try_from(time.tv_sec).expect("a time since none");
    let micros = u64::try_from(time.tv_usec).expect("microseconds under a second");
    Duration::from_secs(seconds) + Duration::from_micros(micros)
}
