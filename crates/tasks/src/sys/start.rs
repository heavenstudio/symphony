//! When a listed process started, as the kernel stamps it ([`process_starts`]): libproc on macOS,
//! `/proc` on Linux, and whether a process it cannot date has ended or only cannot be read. Linux
//! dates a start on the clock since boot, which no setting of the wall clock moves, and
//! [`boot_clock`] reads that clock now, so a start there is compared with times read on it alone.

#[cfg(target_os = "macos")]
use std::ffi::c_int;
#[cfg(target_os = "linux")]
use std::ffi::{c_int, c_long};

/// `ESRCH`, no such process, as both macOS and Linux number it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
const ESRCH: i32 = 3;
/// Where Linux names this boot, a new id at each.
#[cfg(target_os = "linux")]
const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

/// A reading of the clock since boot (Linux's `CLOCK_BOOTTIME`, sleep included): the boot it counts
/// from, by that boot's own id, and the seconds since it. Setting the wall clock moves neither; two
/// readings compare only when they name the same boot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Boottime {
    pub boot: String,
    pub at: f64,
}

/// What the kernel says of when a listed process started.
#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    /// In seconds since the epoch, as the board's clock counts them, by the kernel's own stamp, fixed
    /// when it started (macOS): a later setting of the wall clock does not move it.
    At(f64),
    /// On the clock since boot (Linux, whose `/proc/PID/stat` counts a start in ticks after boot): by
    /// the wall clock it would add today's boot time, which moves whenever the wall clock is set.
    AfterBoot(Boottime),
    /// It has ended since it was listed (or is a zombie, or its pid names another process now): it
    /// works nowhere.
    Ended,
    /// It may still run, but its start cannot be read (no permission, an unexpected record, a
    /// system with no reader): when it began is unknown, not known to be before or after anything.
    Unreadable,
}

/// The clock since boot, read now ([`Boottime`]): `/proc/uptime` and the boot's id on Linux; none
/// when it cannot be read.
#[cfg(target_os = "linux")]
pub fn boot_clock() -> Option<Boottime> {
    boot_read(
        std::fs::read_to_string(BOOT_ID),
        std::fs::read_to_string("/proc/uptime"),
    )
}

/// The clock since boot, read now: none where no start is dated on it (macOS stamps a start by the
/// wall clock, fixed when it began).
#[cfg(not(target_os = "linux"))]
pub fn boot_clock() -> Option<Boottime> {
    None
}

/// A reading of the clock since boot from the boot's id and `/proc/uptime` (whose first field is the
/// seconds since boot): none when either cannot be read or makes no sense.
#[cfg(any(target_os = "linux", test))]
fn boot_read(id: std::io::Result<String>, uptime: std::io::Result<String>) -> Option<Boottime> {
    let boot = id.ok()?.trim().to_owned();
    let at: f64 = uptime.ok()?.split_whitespace().next()?.parse().ok()?;
    (!boot.is_empty() && at.is_finite() && at >= 0.0).then_some(Boottime { boot, at })
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
        // A start in /proc/PID/stat is in clock ticks after this boot.
        let boot = std::fs::read_to_string(BOOT_ID)
            .ok()
            .map(|id| id.trim().to_owned());
        // SAFETY: sysconf accepts this platform's constant and touches no memory of ours; a
        // failure returns -1, rejected by the conversion below, leaving start evidence unreadable.
        let ticks = u32::try_from(unsafe { sysconf(LINUX_CLOCK_TICKS) })
            .ok()
            .filter(|t| *t > 0);
        move |pid: i64| {
            if pid <= 0 {
                return Start::Ended;
            }
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            proc_started(stat, boot.as_deref(), ticks)
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

/// A start as a read of `/proc/PID/stat` gave it, in the boot whose id is `boot`, with the clock
/// ticks a second: a pid with no such file, or a zombie, has ended; a file that cannot be read or
/// parsed, or no boot id or ticks, cannot be read.
#[cfg(any(target_os = "linux", test))]
fn proc_started(read: std::io::Result<String>, boot: Option<&str>, ticks: Option<u32>) -> Start {
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
    match (boot.filter(|id| !id.is_empty()), after_boot, ticks) {
        (Some(boot), Some(after_boot), Some(ticks)) => Start::AfterBoot(Boottime {
            boot: boot.to_owned(),
            at: after_boot / f64::from(ticks),
        }),
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
    // SAFETY: libproc receives a writable proc_bsdinfo buffer of exactly the requested size and
    // owns no pointer past the call. Invalid or exited pids return an error, checked before any
    // buffer fields are read.
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
        let (boot, ticks) = (Some("boot-1"), Some(100));
        // Seconds after boot, in that boot, never a wall date by today's boot time.
        let since_boot = Start::AfterBoot(Boottime {
            boot: "boot-1".into(),
            at: 2.5,
        });
        assert_eq!(proc_started(read(stat("S")), boot, ticks), since_boot);
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
        for boot in [None, Some("")] {
            assert_eq!(
                proc_started(read(stat("S")), boot, ticks),
                Start::Unreadable
            );
        }
        assert_eq!(proc_started(read(stat("S")), boot, None), Start::Unreadable);
    }

    #[test]
    fn the_clock_since_boot_reads_the_boot_s_id_and_the_first_field_of_uptime() {
        let read = |text: &str| -> std::io::Result<String> { Ok(text.into()) };
        assert_eq!(
            boot_read(read("boot-1\n"), read("12.50 40.00\n")),
            Some(Boottime {
                boot: "boot-1".into(),
                at: 12.5
            })
        );
        let unread = || Err(Error::from(ErrorKind::NotFound));
        assert_eq!(boot_read(unread(), read("12.50 40.00\n")), None);
        assert_eq!(boot_read(read("boot-1\n"), unread()), None);
        for (id, uptime) in [
            ("\n", "12.50 40.00\n"),
            ("boot-1", ""),
            ("boot-1", "soon 40.00"),
            ("boot-1", "-1.0 0.0"),
            ("boot-1", "inf 0.0"),
            ("boot-1", "NaN 0.0"),
        ] {
            assert_eq!(boot_read(read(id), read(uptime)), None, "{id:?} {uptime:?}");
        }
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
        let since_boot = boot_clock();
        // A wall date where the kernel stamps one, else a time since this boot, before now on it.
        let dated = match (&starts[0], &since_boot) {
            (Start::At(at), None) => *at <= now,
            (Start::AfterBoot(began), Some(clock)) => {
                began.boot == clock.boot && began.at <= clock.at
            }
            _ => false,
        };
        assert!(dated, "{starts:?} {since_boot:?}");
        assert_eq!(starts[1..], [Start::Ended, Start::Ended, Start::Ended]);
    }
}
