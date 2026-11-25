//! One descriptor of a board connection's socket, which never waits in the socket itself: it is
//! nonblocking, and a read or a write that cannot go at once waits for the socket in `poll`, at
//! most the client's wait each time, as a blocking socket's timeout bounds each of its reads and
//! writes. A wait that runs out says so as that timeout does, `EAGAIN`.
//!
//! macOS drains a socket when one of its descriptors is closed while another process holds that
//! descriptor, as every `lsof` does for each descriptor it reads (`proc_pidfdinfo`): the socket is
//! marked draining for good, and every later wait in its own blocking read or write fails with
//! EBADF, on each descriptor of it, while what does not wait goes on as before. The board's
//! connection has such a close in its making ([`super::unshared`]), and the board's own scans
//! (`lsof +D`, `lsof -iTCP`) read every process's descriptors beside it. A drained socket's
//! `poll`, and its reads and writes that do not wait, answer as a sound socket's do (measured on
//! macOS 26), so a connection that waits only in `poll` is one no drain can break.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// One descriptor of a connection's socket and the client's wait ([`super::Client::waiting`]).
#[derive(Debug)]
pub(super) struct Socket {
    pub(super) socket: UnixStream,
    pub(super) wait: Duration,
}

impl Socket {
    /// `socket`, nonblocking from now on, each read and write of it waiting at most `wait`. A
    /// zero wait is refused, as std refuses a socket's zero timeout: it would never wait at all.
    pub(super) fn waiting(socket: UnixStream, wait: Duration) -> io::Result<Socket> {
        if wait.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot set a 0 duration timeout",
            ));
        }
        socket.set_nonblocking(true)?;
        Ok(Socket { socket, wait })
    }

    /// As much of `out` as the socket takes now, written without waiting: how many bytes that
    /// was, none when it takes nothing.
    pub(super) fn write_now(&self, out: &[u8]) -> io::Result<usize> {
        let mut to = &self.socket;
        let mut taken = 0;
        while taken < out.len() {
            match to.write(&out[taken..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(more) => taken += more,
                Err(why) if why.kind() == io::ErrorKind::WouldBlock => break,
                Err(why) if why.kind() == io::ErrorKind::Interrupted => {}
                Err(why) => return Err(why),
            }
        }
        Ok(taken)
    }

    /// The socket's reads, its writes or both ended, for every descriptor of it: a read or a
    /// write waiting on another thread then answers at once.
    pub(super) fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.socket.shutdown(how)
    }

    /// What `once` gives the first time it goes without waiting, waiting between for `events`
    /// in `poll`, at most the client's wait from the first time it would have waited.
    fn waited<T>(
        &self,
        events: libc::c_short,
        mut once: impl FnMut(&UnixStream) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut by = None;
        loop {
            match once(&self.socket) {
                Err(why) if why.kind() == io::ErrorKind::WouldBlock => {
                    let by = *by.get_or_insert_with(|| Instant::now().checked_add(self.wait));
                    self.ready(events, by)?;
                }
                Err(why) if why.kind() == io::ErrorKind::Interrupted => {}
                done => return done,
            }
        }
    }

    /// The socket ready for `events` (an end or an error included) before `by`, or `EAGAIN`, as
    /// a blocking socket's timeout says it. No `by` is a wait longer than the clock counts: none.
    /// A `by` already past is `EAGAIN` before any `poll`, so a socket `poll` calls ready while its
    /// read or write still would wait never keeps the operation past the client's wait.
    fn ready(&self, events: libc::c_short, by: Option<Instant>) -> io::Result<()> {
        loop {
            let within = match by {
                None => -1,
                Some(by) => {
                    let left = by.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(io::Error::from_raw_os_error(libc::EAGAIN));
                    }
                    libc::c_int::try_from(left.as_nanos().div_ceil(1_000_000))
                        .unwrap_or(libc::c_int::MAX)
                }
            };
            let mut asked = libc::pollfd {
                fd: self.socket.as_raw_fd(),
                events,
                revents: 0,
            };
            // SAFETY: asked is one initialized pollfd, and its descriptor is the socket's, open
            // for as long as self is.
            match unsafe { libc::poll(&raw mut asked, 1, within) } {
                0 => return Err(io::Error::from_raw_os_error(libc::EAGAIN)),
                -1 => {
                    let why = io::Error::last_os_error();
                    if why.kind() != io::ErrorKind::Interrupted {
                        return Err(why);
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}

impl Read for &Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.waited(libc::POLLIN, |mut socket| socket.read(buf))
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for &Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.waited(libc::POLLOUT, |mut socket| socket.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Socket;
    use std::io::{self, Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::Duration;

    /// A wait no answer in these tests is late for: what they wait on is there or never comes.
    const LONG: Duration = Duration::from_secs(300);
    /// A wait that runs out: nothing in these tests ever comes within it.
    const SHORT: Duration = Duration::from_millis(20);

    /// `work`'s answer, on a thread of its own, waited on as a condition: work that never ends
    /// fails the test as a stall, never hangs it.
    fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
        let running = thread::spawn(work);
        let backstop = test_time::backstop(Duration::from_secs(10));
        test_time::until_true(what, backstop, || running.is_finished())
            .unwrap_or_else(|stall| panic!("{stall}"));
        running
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    fn eagain(why: &io::Error) -> bool {
        why.kind() == io::ErrorKind::WouldBlock && why.raw_os_error() == Some(libc::EAGAIN)
    }

    #[test]
    fn a_read_with_nothing_to_read_meets_its_wait_as_eagain_then_reads_what_comes() {
        let (ours, mut peer) = UnixStream::pair().unwrap();
        let mut socket = Socket::waiting(ours, SHORT).unwrap();
        let mut got = [0; 5];
        let why = socket.read(&mut got).unwrap_err();
        assert!(eagain(&why), "{why}");
        peer.write_all(b"later").unwrap();
        socket.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"later");
    }

    #[test]
    fn a_read_with_a_long_wait_gets_what_the_peer_sends() {
        let (ours, mut peer) = UnixStream::pair().unwrap();
        let reading = bounded("a read of what the peer sends", move || {
            let socket = Socket::waiting(ours, LONG).unwrap();
            let reader = thread::spawn(move || {
                let mut got = [0; 4];
                (&socket).read_exact(&mut got).map(|()| got)
            });
            peer.write_all(b"sent").unwrap();
            reader.join().unwrap()
        });
        assert_eq!(&reading.unwrap(), b"sent");
    }

    #[test]
    fn a_write_the_socket_cannot_hold_waits_for_the_peer_to_read_and_one_it_never_reads_meets_its_wait()
     {
        let (ours, mut peer) = UnixStream::pair().unwrap();
        let socket = Socket::waiting(ours, LONG).unwrap();
        let out = vec![b'y'; 1 << 20];
        let read = thread::spawn(move || {
            let mut all = Vec::new();
            peer.read_to_end(&mut all).map(|_| all)
        });
        let written = bounded("a megabyte written as the peer reads it", move || {
            (&socket).write_all(&out)?;
            // Nothing is buffered here: what was written is the socket's.
            (&socket).flush()
        });
        written.unwrap();
        assert_eq!(read.join().unwrap().unwrap(), vec![b'y'; 1 << 20]);

        let (ours, _unread) = UnixStream::pair().unwrap();
        let socket = Socket::waiting(ours, SHORT).unwrap();
        let why = (&socket).write_all(&vec![b'y'; 1 << 20]).unwrap_err();
        assert!(eagain(&why), "{why}");
    }

    #[test]
    fn a_write_now_takes_what_the_socket_holds_and_never_waits() {
        let (ours, _unread) = UnixStream::pair().unwrap();
        let socket = Socket::waiting(ours, LONG).unwrap();
        assert_eq!(socket.write_now(b"short").unwrap(), 5);
        let taken = bounded("a write that does not wait", move || {
            let taken = socket.write_now(&vec![b'y'; 1 << 20]).unwrap();
            (taken, socket.write_now(b"more").unwrap())
        });
        assert!((1..(1 << 20)).contains(&taken.0), "{taken:?}");
        assert_eq!(taken.1, 0, "a full socket takes nothing more");

        let (ours, gone) = UnixStream::pair().unwrap();
        drop(gone);
        let socket = Socket::waiting(ours, LONG).unwrap();
        let why = socket.write_now(b"to no one").unwrap_err();
        assert_eq!(why.kind(), io::ErrorKind::BrokenPipe, "{why}");
    }

    #[test]
    fn a_shutdown_ends_a_read_waiting_on_another_thread_however_long_its_wait() {
        let (ours, _peer) = UnixStream::pair().unwrap();
        let socket = std::sync::Arc::new(Socket::waiting(ours, Duration::MAX).unwrap());
        let reader = std::sync::Arc::clone(&socket);
        let read = bounded("a read a shutdown ends", move || {
            let reading = thread::spawn(move || (&*reader).read(&mut [0; 1]));
            socket.shutdown(Shutdown::Both).unwrap();
            reading.join().unwrap()
        });
        assert_eq!(read.unwrap(), 0, "the end of the socket");
    }

    #[test]
    fn a_zero_wait_is_refused_as_std_refuses_a_zero_timeout() {
        let (ours, _peer) = UnixStream::pair().unwrap();
        let std_says = ours.set_read_timeout(Some(Duration::ZERO)).unwrap_err();
        let why = Socket::waiting(ours, Duration::ZERO).unwrap_err();
        assert_eq!(why.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(why.to_string(), std_says.to_string());
    }

    /// A socket macOS has drained (a descriptor of it closed while a read held that descriptor,
    /// as `lsof` holds each it reads): a blocking read of it fails with EBADF, the control, and
    /// the connection's socket still waits, reads and writes as a sound one does.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_socket_macos_drained_still_waits_reads_and_writes_in_poll() {
        let (kept, mut peer) = drained();
        let mut control = kept.try_clone().unwrap();
        control.set_read_timeout(Some(SHORT)).unwrap();
        let blocked = control.read(&mut [0; 1]).unwrap_err();
        assert_eq!(
            blocked.raw_os_error(),
            Some(libc::EBADF),
            "drained: {blocked}"
        );

        let mut socket = Socket::waiting(kept, SHORT).unwrap();
        let why = socket.read(&mut [0; 1]).unwrap_err();
        assert!(
            eagain(&why),
            "a drained socket's wait runs out as any: {why}"
        );
        socket.wait = LONG;
        let echo = thread::spawn(move || {
            let mut got = vec![0; 1 << 20];
            peer.read_exact(&mut got)?;
            peer.write_all(&got[..5])?;
            Ok::<_, io::Error>(got)
        });
        let answered = bounded("a megabyte each way on a drained socket", move || {
            (&socket).write_all(&vec![b'y'; 1 << 20])?;
            let mut back = [0; 5];
            (&socket).read_exact(&mut back).map(|()| back)
        });
        assert_eq!(&answered.unwrap(), b"yyyyy");
        assert_eq!(echo.join().unwrap().unwrap(), vec![b'y'; 1 << 20]);
    }

    #[test]
    fn a_socket_poll_calls_ready_whose_operation_still_would_wait_meets_its_wait() {
        let (ours, _peer) = UnixStream::pair().unwrap();
        // An empty socket is always ready to write: poll answers at once every time.
        let socket = Socket::waiting(ours, SHORT).unwrap();
        let tries = bounded("an operation that always would wait", move || {
            let mut tries = 0u64;
            let said = socket.waited(libc::POLLOUT, |_| {
                tries += 1;
                Err::<(), _>(io::ErrorKind::WouldBlock.into())
            });
            (said, tries)
        });
        let why = tries.0.unwrap_err();
        assert!(eagain(&why), "{why}");
        assert!(tries.1 > 1, "poll answered ready, and it was tried again");
    }

    /// How many pairs [`drained`] makes before it fails: a read the reader thread waits in is
    /// drained by the first, and another try is for a reader waiting somewhere else at that look.
    #[cfg(target_vendor = "apple")]
    const DRAIN_TRIES: u32 = 100;

    /// `PROC_PIDTHREADID64INFO` (`sys/proc_info.h`), which the libc crate does not name: a thread
    /// of a process by its id, as `pthread_threadid_np` gives it.
    #[cfg(target_vendor = "apple")]
    const PROC_PIDTHREADID64INFO: libc::c_int = 15;

    /// Whether the thread of this process whose id is `id` waits in the kernel now.
    #[cfg(target_vendor = "apple")]
    fn waiting_in_the_kernel(id: u64) -> bool {
        // SAFETY: proc_threadinfo is integers and a byte array, for which zero is a value.
        let mut info: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(size_of_val(&info)).expect("its size fits an int");
        // SAFETY: info is writable for size bytes, the size given, and outlives the call.
        let got = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                PROC_PIDTHREADID64INFO,
                id,
                (&raw mut info).cast(),
                size,
            )
        };
        got == size && info.pth_run_state == libc::TH_STATE_WAITING
    }

    /// A socket pair's first end, drained, and its peer. A thread reads one descriptor of it,
    /// which has nothing to read; once that thread waits in the kernel, which is its read, a
    /// descriptor of `/dev/null` is put in that descriptor's place (`dup2`, so the number is never
    /// free for another test's file), and that close, the read holding the descriptor, drains the
    /// socket. A try whose reader waited somewhere else at that look drained nothing, and is made
    /// again on a pair of its own, up to [`DRAIN_TRIES`].
    #[cfg(target_vendor = "apple")]
    fn drained() -> (UnixStream, UnixStream) {
        use std::os::fd::AsRawFd as _;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        let null = std::fs::File::open("/dev/null").unwrap();
        for _ in 0..DRAIN_TRIES {
            let (held, peer) = UnixStream::pair().unwrap();
            let kept = held.try_clone().unwrap();
            let fd = held.as_raw_fd();
            let id = Arc::new(AtomicU64::new(0));
            let said = Arc::clone(&id);
            let reader = thread::spawn(move || {
                let mut mine = 0u64;
                // SAFETY: mine is writable, and pthread_self is this thread.
                assert_eq!(
                    unsafe { libc::pthread_threadid_np(libc::pthread_self(), &raw mut mine) },
                    0
                );
                said.store(mine, Ordering::SeqCst);
                let mut byte = 0u8;
                // SAFETY: fd stays open until this thread is joined: held keeps it, and the dup2
                // below only puts another file in its place.
                unsafe { libc::read(fd, (&raw mut byte).cast(), 1) }
            });
            let backstop = test_time::backstop(Duration::from_secs(10));
            test_time::until_true("the reader waits in its read", backstop, || {
                let id = id.load(Ordering::SeqCst);
                id != 0 && waiting_in_the_kernel(id)
            })
            .unwrap_or_else(|stall| panic!("{stall}"));
            // SAFETY: both descriptors are open; fd is held's, which owns what dup2 puts there.
            assert_ne!(unsafe { libc::dup2(null.as_raw_fd(), fd) }, -1);
            reader.join().unwrap();
            drop(held);
            let probe = kept.try_clone().unwrap();
            probe
                .set_read_timeout(Some(Duration::from_millis(1)))
                .unwrap();
            let read = (&probe).read(&mut [0; 1]);
            if read.is_err_and(|why| why.raw_os_error() == Some(libc::EBADF)) {
                return (kept, peer);
            }
        }
        panic!("no socket drained in {DRAIN_TRIES} pairs");
    }
}
