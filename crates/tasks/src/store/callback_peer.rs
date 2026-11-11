//! A UDS address is not an identity. Read the listener's kernel credentials on the connection
//! used for probing or sending, and pair its pid with its process start time to survive pid reuse.
//! macOS: sys/un.h LOCAL_PEERPID, libproc.h PROC_PIDTBSDINFO; Linux: unix(7), proc_pid_stat(5).

use serde::{Deserialize, Serialize};
use std::ffi::{c_int, c_void};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Owner {
    pid: i32,
    started: String,
}

impl Owner {
    /// A connect failure says nothing about a paused or busy listener. Only a missing process or
    /// a different start time proves that this registration's owner has gone. Unknown stays retryable.
    pub(super) fn alive(&self) -> Option<bool> {
        if !crate::sys::pid_alive(i64::from(self.pid)) {
            return Some(false);
        }
        match started(self.pid) {
            Ok(start) => Some(start == self.started),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    }
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

#[cfg(target_os = "macos")]
type Nfds = u32;
#[cfg(not(target_os = "macos"))]
type Nfds = usize;

unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    #[link_name = "connect"]
    fn socket_connect(fd: c_int, address: *const c_void, length: u32) -> c_int;
    fn getsockopt(
        fd: c_int,
        level: c_int,
        name: c_int,
        value: *mut c_void,
        length: *mut u32,
    ) -> c_int;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, length: u32)
    -> c_int;
    fn poll(fds: *mut PollFd, count: Nfds, timeout: c_int) -> c_int;
}

#[repr(C)]
struct Address {
    #[cfg(target_os = "macos")]
    length: u8,
    #[cfg(target_os = "macos")]
    family: u8,
    #[cfg(not(target_os = "macos"))]
    family: u16,
    #[cfg(target_os = "macos")]
    path: [u8; 104],
    #[cfg(not(target_os = "macos"))]
    path: [u8; 108],
}

/// Nonblocking connect with one deadline: a stopped listener with a full accept queue must not
/// hold a writer or the sweep forever. No bytes are sent by this function.
pub(super) fn connect(path: &str) -> io::Result<(UnixStream, Owner)> {
    let mut address = Address {
        #[cfg(target_os = "macos")]
        length: 0,
        family: 1, // AF_UNIX on macOS and Linux.
        #[cfg(target_os = "macos")]
        path: [0; 104],
        #[cfg(not(target_os = "macos"))]
        path: [0; 108],
    };
    if path.len() >= address.path.len() || path.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Unix socket path",
        ));
    }
    address.path[..path.len()].copy_from_slice(path.as_bytes());
    let length = std::mem::offset_of!(Address, path) + path.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.length = u8::try_from(length).expect("Unix address fits sun_len");
    }
    // SAFETY: socket has no pointer arguments; a successful descriptor is owned exactly once below.
    let fd = unsafe { socket(1, 1, 0) }; // SOCK_STREAM.
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is a new, valid AF_UNIX stream descriptor and ownership is transferred here.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    // SAFETY: address is a valid sockaddr_un with its initialized, NUL-terminated path in bounds.
    let connected = unsafe {
        socket_connect(
            fd,
            (&raw const address).cast(),
            u32::try_from(length).expect("Unix address fits socklen_t"),
        )
    };
    if connected != 0 {
        let error = io::Error::last_os_error();
        #[cfg(target_os = "macos")]
        let in_progress = 36;
        #[cfg(not(target_os = "macos"))]
        let in_progress = 115;
        if error.raw_os_error() != Some(in_progress) {
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = c_int::try_from(remaining.as_millis()).unwrap_or(250);
            if timeout == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "socket connect timed out",
                ));
            }
            let mut waiting = PollFd {
                fd,
                events: 4,
                revents: 0,
            }; // POLLOUT.
            // SAFETY: waiting is one initialized pollfd, valid for the duration of this call.
            let ready = unsafe { poll(&raw mut waiting, 1, timeout) };
            if ready > 0 {
                if let Some(error) = stream.take_error()? {
                    return Err(error);
                }
                stream.peer_addr()?;
                break;
            }
            if ready == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "socket connect timed out",
                ));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    stream.set_nonblocking(false)?;
    let owner = owner(&stream)?;
    Ok((stream, owner))
}

fn option<T: Default>(stream: &UnixStream, level: c_int, name: c_int) -> io::Result<T> {
    let mut value = T::default();
    let expected = u32::try_from(std::mem::size_of::<T>()).expect("socket option fits socklen_t");
    let mut length = expected;
    // SAFETY: value is writable for exactly length bytes and the fd remains owned by stream.
    let result = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            level,
            name,
            (&raw mut value).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length != expected {
        return Err(io::Error::other(
            "socket returned an incomplete option value",
        ));
    }
    Ok(value)
}

/// A paused peer must have room for the complete envelope, including its newline, before any
/// bytes are sent. An EOF reader can accept a complete JSON value even without that newline.
pub(super) fn reserve_send_buffer(stream: &UnixStream, bytes: usize) -> io::Result<()> {
    let requested = c_int::try_from(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "envelope exceeds SO_SNDBUF range",
        )
    })?;
    #[cfg(target_os = "macos")]
    let (level, name) = (0xffff, 0x1001); // SOL_SOCKET / SO_SNDBUF.
    #[cfg(not(target_os = "macos"))]
    let (level, name) = (1, 7); // Linux SOL_SOCKET / SO_SNDBUF.
    let length = u32::try_from(std::mem::size_of_val(&requested)).expect("int fits socklen_t");
    // SAFETY: requested is an initialized int, readable for length bytes, and stream owns the fd.
    let result = unsafe {
        setsockopt(
            stream.as_raw_fd(),
            level,
            name,
            (&raw const requested).cast(),
            length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // Linux doubles the reservation for bookkeeping (socket(7), SO_SNDBUF). Normalize the
    // readback before checking whether the requested reservation was clamped.
    let capacity: c_int = option(stream, level, name)?;
    #[cfg(target_os = "linux")]
    let reserved = capacity / 2;
    #[cfg(not(target_os = "linux"))]
    let reserved = capacity;
    if reserved < requested {
        return Err(io::Error::other(format!(
            "SO_SNDBUF reserved {reserved} bytes (reported {capacity}), below the requested {bytes} bytes"
        )));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn owner(stream: &UnixStream) -> io::Result<Owner> {
    let pid: i32 = option(stream, 0, 2)?; // SOL_LOCAL / LOCAL_PEERPID.
    Ok(Owner {
        pid,
        started: started(pid)?,
    })
}

#[cfg(target_os = "macos")]
fn started(pid: i32) -> io::Result<String> {
    let Some((seconds, micros)) = crate::sys::bsd_start(pid)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "socket peer is no longer running",
        ));
    };
    Ok(format!("{seconds}.{micros:06}"))
}

#[cfg(target_os = "linux")]
fn owner(stream: &UnixStream) -> io::Result<Owner> {
    #[repr(C)]
    #[derive(Default)]
    struct Credentials {
        pid: i32,
        uid: u32,
        gid: u32,
    }
    let credentials: Credentials = option(stream, 1, 17)?; // SOL_SOCKET / SO_PEERCRED.
    let pid = credentials.pid;
    Ok(Owner {
        pid,
        started: started(pid)?,
    })
}

#[cfg(target_os = "linux")]
fn started(pid: i32) -> io::Result<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let (_, fields) = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("invalid process stat"))?;
    let fields: Vec<&str> = fields.split_whitespace().collect();
    let ticks = fields
        .get(19)
        .filter(|ticks| !ticks.is_empty() && ticks.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| io::Error::other("process start time is missing"))?;
    if pid <= 0 || matches!(fields.first(), Some(&"Z" | &"X" | &"x")) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "socket peer is no longer running",
        ));
    }
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(format!("{}:{ticks}", boot.trim()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn owner(_stream: &UnixStream) -> io::Result<Owner> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native peer identity needs macOS or Linux",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn started(_pid: i32) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native process identity needs macOS or Linux",
    ))
}
