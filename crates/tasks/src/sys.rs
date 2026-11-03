//! What health asks of this machine: its host name and whether a process is alive, as
//! `gethostname` and `kill(pid, 0)` answer them; and the end of a
//! process, for the board's server ([`crate::server`]); and the local time a line is stamped with.
//! Also how a child is started and its input and output read ([`Command`], [`output`],
//! [`spawn_output`], [`spawn_piped`], [`pipe`]): without a fork and through pipes no child another
//! thread spawns can hold open, from the leaf the cwd scans share ([`process_cwd::spawn`]).

use std::ffi::{c_char, c_int, c_long};
use std::fs::File;

pub use process_cwd::spawn::{
    Child, Command, Stdio, output, pipe, set_nonblocking, spawn_output, spawn_piped, wait_output,
};

unsafe extern "C" {
    fn gethostname(name: *mut c_char, len: usize) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn tzset();
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
}

impl crate::port::Lsof for Command {
    fn lsof(args: &[&str]) -> Self {
        let mut lsof = Command::new("lsof");
        lsof.args(args);
        lsof
    }
}

/// C's `struct tm`, as macOS and glibc lay it out.
#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

const EPERM: i32 = 1;
const SIGKILL: c_int = 9;
const SIGTERM: c_int = 15;

/// A file lock released explicitly before its descriptor closes. A sibling thread can fork while
/// it is held: closing our copy alone leaves the lock held by that child until its exec.
pub struct FileLock(pub(crate) File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// This machine's host name, or empty if it has none it will say.
pub fn hostname() -> String {
    let mut name = [0 as c_char; 256];
    // SAFETY: the buffer is as long as the length given, and gethostname writes no further.
    if unsafe { gethostname(name.as_mut_ptr(), name.len()) } != 0 {
        return String::new();
    }
    let bytes: Vec<u8> = name
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether process `pid` exists: one this user may not signal still exists.
pub fn pid_alive(pid: i64) -> bool {
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false; // 0 and below name process groups, not a process
    }
    // SAFETY: signal 0 sends nothing; kill only checks the process exists and may be signalled.
    let signalled = unsafe { kill(pid, 0) } == 0;
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

/// `seconds` as `time.strftime("%Y-%m-%d %H:%M", time.localtime(seconds))` writes it: this machine's
/// time zone, the seconds floored.
pub fn local_minute(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // a time in seconds is far inside i64
    let time = seconds.floor() as i64;
    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };
    // SAFETY: tzset reads TZ; localtime_r writes only the struct it is given, which outlives the call.
    let done = unsafe {
        tzset();
        localtime_r(&raw const time, &raw mut tm)
    };
    if done.is_null() {
        return String::new();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// Ask process `pid` to end (SIGTERM), or make it (SIGKILL, `hard`): whether the signal went. Only
/// ever one process: 0 and below, which name process groups, are refused.
pub fn terminate(pid: i64, hard: bool) -> bool {
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    let signal = if hard { SIGKILL } else { SIGTERM };
    // SAFETY: kill reads nothing of ours; a pid above 0 names one process.
    unsafe { kill(pid, signal) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_name_is_the_one_the_system_gives() {
        let said = output(&mut Command::new("hostname")).unwrap();
        assert_eq!(hostname(), String::from_utf8(said.stdout).unwrap().trim());
        assert!(!hostname().is_empty());
    }

    #[test]
    fn this_process_is_alive_and_a_reaped_child_is_not() {
        assert!(pid_alive(i64::from(std::process::id())));
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        assert!(!pid_alive(pid));
    }

    #[test]
    fn a_process_this_user_may_not_signal_is_alive() {
        assert!(pid_alive(1), "launchd or init is pid 1 and belongs to root");
    }

    #[test]
    fn a_local_minute_is_the_one_date_gives() {
        for seconds in [0.0_f64, 1_800_000_000.9, 1_791_362_047.125_689] {
            let whole = format!("{}", seconds.floor());
            let bsd = output(Command::new("date").args(["-r", &whole, "+%Y-%m-%d %H:%M"])).unwrap();
            let date = if bsd.status.success() {
                bsd
            } else {
                output(Command::new("date").args(["-d", &format!("@{whole}"), "+%Y-%m-%d %H:%M"]))
                    .unwrap()
            };
            assert_eq!(
                local_minute(seconds),
                String::from_utf8(date.stdout).unwrap().trim(),
                "{seconds}"
            );
        }
    }

    #[test]
    fn terminate_ends_one_process_softly_or_hard() {
        use std::os::unix::process::ExitStatusExt;
        for (hard, signal) in [(false, 15), (true, 9)] {
            let mut child = Command::new("sleep").arg("30").spawn().unwrap();
            assert!(terminate(i64::from(child.id()), hard));
            assert_eq!(child.wait().unwrap().signal(), Some(signal));
        }
    }

    #[test]
    fn terminate_never_signals_a_group() {
        for pid in [0, -1, i64::from(i32::MAX) + 1] {
            assert!(!terminate(pid, false), "{pid}");
        }
    }
    #[test]
    fn no_pid_outside_the_range_is_alive() {
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i64::from(i32::MAX) + 1));
    }
}
