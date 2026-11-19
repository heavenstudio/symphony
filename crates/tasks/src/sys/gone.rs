//! The end of a process this one did not start, waited for as the kernel's own event ([`gone`]):
//! a kqueue's `NOTE_EXIT` on macOS, a pidfd that reads ready on Linux. `waitpid` answers only for
//! a child, and looking again by a number is a sleep; this waits on the end itself, a deadline
//! its only time.

use std::io;
use std::time::{Duration, Instant};

/// Whether process `pid` has ended within `most`: at once when no process has the pid, when it
/// ends otherwise, and no once `most` has passed with it still running. An error when the kernel
/// gives no watch of it (another user's process, a platform with neither event), which is never
/// taken for its end.
pub fn gone(pid: i64, most: Duration) -> io::Result<bool> {
    let Some(pid) = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0) else {
        return Ok(true); // no process has such a pid
    };
    let Some(watch) = watch(pid)? else {
        return Ok(true);
    };
    let until = Instant::now() + most;
    loop {
        match watch.ended(until.saturating_duration_since(Instant::now())) {
            // A signal this process took cut the wait short: what is left of it is waited again.
            Err(cut) if cut.kind() == io::ErrorKind::Interrupted => {}
            answer => return answer,
        }
    }
}

/// The machine's load average over 1, 5 and 15 minutes, as `getloadavg` reads it: what a wait
/// that met its backstop says, so a stall on a loaded machine reads as one.
pub fn load_average() -> Option<[f64; 3]> {
    let mut load = [0.0_f64; 3];
    // SAFETY: getloadavg writes at most the three readings it is given room for.
    let read = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
    (read == 3).then_some(load)
}

/// [`load_average`] as a refusal says it: `load average 1.20 0.90 0.70`, or that it is unread.
pub fn load_said() -> String {
    match load_average() {
        Some([one, five, fifteen]) => format!("load average {one:.2} {five:.2} {fifteen:.2}"),
        None => "load average unread".into(),
    }
}

/// A watch of `pid`'s end; none when no process has the pid.
#[cfg(target_os = "macos")]
fn watch(pid: libc::pid_t) -> io::Result<Option<Watch>> {
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    // SAFETY: kqueue takes no argument.
    let queue = unsafe { libc::kqueue() };
    if queue == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor nothing else owns.
    let queue = unsafe { OwnedFd::from_raw_fd(queue) };
    let change = libc::kevent {
        ident: pid.unsigned_abs() as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one change in, no room for an event out: an error is the call's own.
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
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(error),
        };
    }
    Ok(Some(Watch(queue)))
}

/// A watch of `pid`'s end; none when no process has the pid.
#[cfg(target_os = "linux")]
fn watch(pid: libc::pid_t) -> io::Result<Option<Watch>> {
    use std::os::fd::{FromRawFd as _, OwnedFd};
    // SAFETY: pidfd_open takes a pid and flags, and answers a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd == -1 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(error),
        };
    }
    let fd = libc::c_int::try_from(fd).map_err(io::Error::other)?;
    // SAFETY: a new descriptor nothing else owns.
    Ok(Some(Watch(unsafe { OwnedFd::from_raw_fd(fd) })))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn watch(_: libc::pid_t) -> io::Result<Option<Watch>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform gives no event for the end of a process that is not a child",
    ))
}

/// The kernel's watch of one process: a kqueue holding its `NOTE_EXIT`, or its pidfd.
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Watch(std::os::fd::OwnedFd);

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
enum Watch {}

impl Watch {
    /// Whether the process ended within `wait`.
    #[cfg(target_os = "macos")]
    fn ended(&self, wait: Duration) -> io::Result<bool> {
        use std::os::fd::AsRawFd as _;
        let timeout = libc::timespec {
            tv_sec: libc::time_t::try_from(wait.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: libc::c_long::from(wait.subsec_nanos()),
        };
        // SAFETY: kevent is plain data, for which all zeroes is a value.
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        // SAFETY: no change in, room for one event out, waiting at most `timeout`.
        let seen = unsafe {
            libc::kevent(
                self.0.as_raw_fd(),
                std::ptr::null(),
                0,
                &raw mut event,
                1,
                &raw const timeout,
            )
        };
        match seen {
            -1 => Err(io::Error::last_os_error()),
            0 => Ok(false),
            _ if event.flags & libc::EV_ERROR != 0 => Err(io::Error::from_raw_os_error(
                i32::try_from(event.data).unwrap_or(libc::EINVAL),
            )),
            _ => Ok(event.fflags & libc::NOTE_EXIT != 0),
        }
    }

    /// Whether the process ended within `wait`: its pidfd reads ready once it has.
    #[cfg(target_os = "linux")]
    fn ended(&self, wait: Duration) -> io::Result<bool> {
        use std::os::fd::AsRawFd as _;
        let mut polled = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = libc::c_int::try_from(wait.as_millis()).unwrap_or(libc::c_int::MAX);
        // SAFETY: one pollfd in and out, waiting at most `millis`.
        match unsafe { libc::poll(&raw mut polled, 1, millis) } {
            -1 => Err(io::Error::last_os_error()),
            0 => Ok(false),
            _ => Ok(polled.revents & libc::POLLIN != 0),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn ended(&self, _: Duration) -> io::Result<bool> {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{Stdio, command};

    /// A child that runs until its input ends, and its pid.
    fn held() -> (crate::sys::Child, i64) {
        let child = command("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = i64::from(child.id());
        (child, pid)
    }

    #[test]
    fn a_pid_no_process_has_is_gone_at_once() {
        for pid in [0, -1, i64::MAX] {
            assert!(gone(pid, Duration::ZERO).unwrap(), "{pid}");
        }
        let (mut child, pid) = held();
        drop(child.stdin.take());
        child.wait().unwrap();
        // Reaped, so no process has the pid: no wait is made, whatever the bound.
        assert!(gone(pid, Duration::from_secs(3600)).unwrap());
    }

    #[test]
    fn a_running_process_is_not_gone_when_the_bound_is_met() {
        let (mut child, pid) = held();
        assert!(!gone(pid, Duration::ZERO).unwrap());
        assert!(!gone(pid, Duration::from_millis(20)).unwrap());
        drop(child.stdin.take());
        child.wait().unwrap();
    }

    /// The wait ends on the process's end, not on its bound: an hour's bound answers as soon as
    /// the process has ended, which only the event can do.
    #[test]
    fn the_wait_ends_when_the_process_does() {
        let (mut child, pid) = held();
        let waiting = std::thread::spawn(move || gone(pid, Duration::from_secs(3600)));
        drop(child.stdin.take());
        assert!(waiting.join().unwrap().unwrap());
        child.wait().unwrap();
    }

    #[test]
    fn the_load_is_said_as_three_readings() {
        let said = load_said();
        let readings = said.strip_prefix("load average ").unwrap();
        assert_eq!(readings.split(' ').count(), 3, "{said}");
        assert!(readings.split(' ').all(|one| one.parse::<f64>().is_ok()));
    }
}
