//! When a listed process started, as the kernel stamps it ([`process_starts`]): libproc on macOS,
//! `/proc` on Linux, and whether a process it cannot date has ended or only cannot be read.

#[cfg(target_os = "macos")]
use std::ffi::c_int;
#[cfg(target_os = "linux")]
use std::ffi::{c_int, c_long};

/// `ESRCH`, no such process, as both macOS and Linux number it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
const ESRCH: i32 = 3;

/// What the kernel says of when a listed process started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Start {
    /// In seconds since the epoch, as the board's clock counts them, by the kernel's own stamp.
    At(f64),
    /// It has ended since it was listed (or is a zombie, or its pid names another process now): it
    /// works nowhere.
    Ended,
    /// It may still run, but its start cannot be read (no permission, an unexpected record, a
    /// system with no reader): when it began is unknown, not known to be before or after anything.
    Unreadable,
}

/// When each of `pids` started ([`Start`]), in order.
pub fn process_starts(pids: &[i64]) -> Vec<Start> {
    #[cfg(target_os = "macos")]
    let started = |pid: i64| match c_int::try_from(pid).ok().filter(|pid| *pid > 0) {
        Some(pid) => bsd_started(bsd_start(pid)),
        None => Start::Ended,
    };
    #[cfg(target_os = "linux")]
    let started = {
        // A start in /proc/PID/stat is in clock ticks after the boot /proc/stat names, in seconds.
        let boot = std::fs::read_to_string("/proc/stat").ok().and_then(|stat| {
            let line = stat.lines().find_map(|line| line.strip_prefix("btime "))?;
            line.trim().parse::<f64>().ok()
        });
        // SAFETY: sysconf reads a configuration value and touches no memory of ours.
        let ticks = u32::try_from(unsafe { sysconf(LINUX_CLOCK_TICKS) })
            .ok()
            .filter(|t| *t > 0);
        move |pid: i64| {
            if pid <= 0 {
                return Start::Ended;
            }
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            proc_started(stat, boot, ticks)
        }
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let started = |_: i64| Start::Unreadable;
    pids.iter().map(|pid| started(*pid)).collect()
}

/// A start as libproc answered for a pid ([`bsd_start`]): no such process (`ESRCH`) or none any
/// more has ended; any other failure cannot be read.
#[cfg(any(target_os = "macos", test))]
fn bsd_started(read: std::io::Result<Option<(u64, u64)>>) -> Start {
    match read {
        Ok(Some((seconds, micros))) => match (u32::try_from(seconds), u32::try_from(micros)) {
            (Ok(seconds), Ok(micros)) => Start::At(f64::from(seconds) + f64::from(micros) / 1e6),
            _ => Start::Unreadable,
        },
        Ok(None) => Start::Ended,
        Err(e) if e.raw_os_error() == Some(ESRCH) => Start::Ended,
        Err(_) => Start::Unreadable,
    }
}

/// A start as a read of `/proc/PID/stat` gave it, with the boot time `/proc/stat` names and the
/// clock ticks a second: a pid with no such file, or a zombie, has ended; a file that cannot be
/// read or parsed, or no boot time or ticks, cannot be read.
#[cfg(any(target_os = "linux", test))]
fn proc_started(read: std::io::Result<String>, boot: Option<f64>, ticks: Option<u32>) -> Start {
    let stat = match read {
        Ok(stat) => stat,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Start::Ended,
        Err(e) if e.raw_os_error() == Some(ESRCH) => return Start::Ended,
        Err(_) => return Start::Unreadable,
    };
    let Some((_, fields)) = stat.rsplit_once(')') else {
        return Start::Unreadable;
    };
    let fields: Vec<&str> = fields.split_whitespace().collect();
    if matches!(fields.first(), Some(&"Z" | &"X" | &"x")) {
        return Start::Ended;
    }
    let after_boot: Option<f64> = fields.get(19).and_then(|ticks| ticks.parse().ok());
    match (boot, after_boot, ticks) {
        (Some(boot), Some(after_boot), Some(ticks)) => {
            Start::At(boot + after_boot / f64::from(ticks))
        }
        _ => Start::Unreadable,
    }
}

/// `_SC_CLK_TCK`, the clock ticks a second, as glibc and musl number it.
#[cfg(target_os = "linux")]
const LINUX_CLOCK_TICKS: c_int = 2;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn sysconf(name: c_int) -> c_long;
}

/// When process `pid` started, as libproc's `proc_bsdinfo` stamps it: its seconds and microseconds
/// since the epoch; none for a pid that names no process (or another's, reused) or a zombie, and an
/// error when libproc cannot be read.
#[cfg(target_os = "macos")]
pub(crate) fn bsd_start(pid: c_int) -> std::io::Result<Option<(u64, u64)>> {
    // Layout of proc_bsdinfo from the SDK's sys/proc_info.h (136 bytes).
    #[repr(C)]
    #[derive(Default)]
    struct BsdInfo {
        prefix: [u32; 12],
        comm: [u8; 16],
        name: [u8; 32],
        details: [u32; 6],
        seconds: u64,
        micros: u64,
    }
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            size: c_int,
        ) -> c_int;
    }
    let mut info = BsdInfo::default();
    let size = c_int::try_from(std::mem::size_of::<BsdInfo>()).expect("proc_bsdinfo fits int");
    // SAFETY: libproc receives a writable proc_bsdinfo buffer of exactly the requested size.
    let read = unsafe { proc_pidinfo(pid, 3, 0, (&raw mut info).cast(), size) };
    if read != size {
        return Err(std::io::Error::last_os_error());
    }
    // pbi_pid is the fourth word and pbi_status the second; a status of 5 is a zombie (SZOMB).
    let gone = pid <= 0 || info.prefix[3] != u32::try_from(pid).unwrap_or(0) || info.prefix[1] == 5;
    Ok((!gone).then_some((info.seconds, info.micros)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{Command, EPERM};
    use std::io::{Error, ErrorKind};

    #[test]
    fn a_start_libproc_cannot_give_is_unreadable_and_one_for_no_process_has_ended() {
        assert_eq!(
            bsd_started(Ok(Some((1_800_000_000, 250_000)))),
            Start::At(1_800_000_000.25)
        );
        assert_eq!(bsd_started(Ok(None)), Start::Ended);
        assert_eq!(
            bsd_started(Err(Error::from_raw_os_error(ESRCH))),
            Start::Ended
        );
        assert_eq!(
            bsd_started(Err(Error::from_raw_os_error(EPERM))),
            Start::Unreadable
        );
        assert_eq!(bsd_started(Ok(Some((u64::MAX, 0)))), Start::Unreadable);
    }

    #[test]
    fn a_start_procfs_cannot_give_is_unreadable_and_one_for_no_process_has_ended() {
        // The name may hold `)` and spaces; the start is the 22nd field, the 20th after the name.
        let stat = |state: &str| {
            format!("42 (a) b) {state} 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 250 20\n")
        };
        let read = |text: String| -> std::io::Result<String> { Ok(text) };
        let (boot, ticks) = (Some(1_800_000_000.0), Some(100));
        assert_eq!(
            proc_started(read(stat("S")), boot, ticks),
            Start::At(1_800_000_002.5)
        );
        for zombie in ["Z", "X", "x"] {
            assert_eq!(proc_started(read(stat(zombie)), boot, ticks), Start::Ended);
        }
        let gone = || Err(Error::from(ErrorKind::NotFound));
        assert_eq!(proc_started(gone(), boot, ticks), Start::Ended);
        assert_eq!(
            proc_started(Err(Error::from_raw_os_error(ESRCH)), boot, ticks),
            Start::Ended
        );
        let denied = || Err(Error::from_raw_os_error(EPERM));
        assert_eq!(proc_started(denied(), boot, ticks), Start::Unreadable);
        assert_eq!(
            proc_started(read("42 no name".into()), boot, ticks),
            Start::Unreadable
        );
        assert_eq!(
            proc_started(read("42 (a) S 1".into()), boot, ticks),
            Start::Unreadable
        );
        assert_eq!(
            proc_started(read(stat("S")), None, ticks),
            Start::Unreadable
        );
        assert_eq!(proc_started(read(stat("S")), boot, None), Start::Unreadable);
    }

    #[test]
    fn this_process_has_a_start_and_no_pid_or_a_reaped_one_has_ended() {
        let mut done = Command::new("true").spawn().unwrap();
        done.wait().unwrap();
        let pids = [i64::from(std::process::id()), i64::from(done.id()), 0, -1];
        let starts = process_starts(&pids);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        assert!(
            matches!(starts[0], Start::At(at) if at <= now),
            "{starts:?}"
        );
        assert_eq!(starts[1..], [Start::Ended, Start::Ended, Start::Ended]);
    }
}
