//! A socket macOS has drained, for the tests of what waits on one ([`super::Socket`],
//! [`crate::rpc::exchange`]): a descriptor of it closed while a read held that descriptor, as an
//! `lsof` holds each it reads, after which every blocking wait on the socket fails with EBADF.

use std::io::{self, Read};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

/// How many pairs [`drained`] makes before it fails: a read the reader thread waits in is drained
/// by the first, and another try is for a reader waiting somewhere else at that look.
const DRAIN_TRIES: u32 = 100;

/// `PROC_PIDTHREADID64INFO` (`sys/proc_info.h`), which the libc crate does not name: a thread of a
/// process by its id, as `pthread_threadid_np` gives it.
const PROC_PIDTHREADID64INFO: libc::c_int = 15;

/// A kind of connected socket the drain makes and reads: std's Unix and TCP streams.
pub(crate) trait Pair: AsRawFd + Send + Sized + 'static {
    /// A connected pair: the end to drain, then its peer.
    fn pair() -> (Self, Self);
    /// Another descriptor of the same socket.
    fn copy(&self) -> Self;
    /// A blocking read of a copy of the socket, waiting at most `wait`: what the read says.
    fn blocked_read(&self, wait: Duration) -> io::Result<usize>;
}

impl Pair for UnixStream {
    fn pair() -> (Self, Self) {
        UnixStream::pair().unwrap()
    }

    fn copy(&self) -> Self {
        self.try_clone().unwrap()
    }

    fn blocked_read(&self, wait: Duration) -> io::Result<usize> {
        let mut probe = self.try_clone()?;
        probe.set_read_timeout(Some(wait))?;
        probe.read(&mut [0; 1])
    }
}

impl Pair for TcpStream {
    fn pair() -> (Self, Self) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (ours, listener.accept().unwrap().0)
    }

    fn copy(&self) -> Self {
        self.try_clone().unwrap()
    }

    fn blocked_read(&self, wait: Duration) -> io::Result<usize> {
        let mut probe = self.try_clone()?;
        probe.set_read_timeout(Some(wait))?;
        probe.read(&mut [0; 1])
    }
}

/// The calling thread's id, as [`waiting_in_the_kernel`] takes it.
pub(crate) fn thread_id() -> u64 {
    let mut mine = 0u64;
    // SAFETY: mine is writable, and pthread_self is this thread.
    assert_eq!(
        unsafe { libc::pthread_threadid_np(libc::pthread_self(), &raw mut mine) },
        0
    );
    mine
}

/// Whether the thread of this process whose id is `id` waits in the kernel now.
pub(crate) fn waiting_in_the_kernel(id: u64) -> bool {
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

/// A socket pair's first end, drained, and its peer. A thread reads one descriptor of it, which
/// has nothing to read; once that thread waits in the kernel, which is its read, a descriptor of
/// `/dev/null` is put in that descriptor's place (`dup2`, so the number is never free for another
/// test's file), and that close, the read holding the descriptor, drains the socket. A try whose
/// reader waited somewhere else at that look drained nothing, and is made again on a pair of its
/// own, up to [`DRAIN_TRIES`].
pub(crate) fn drained<S: Pair>() -> (S, S) {
    let null = std::fs::File::open("/dev/null").unwrap();
    for _ in 0..DRAIN_TRIES {
        let (held, peer) = S::pair();
        let kept = held.copy();
        let fd = held.as_raw_fd();
        let id = Arc::new(AtomicU64::new(0));
        let said = Arc::clone(&id);
        let reader = thread::spawn(move || {
            said.store(thread_id(), Ordering::SeqCst);
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
        let read = kept.blocked_read(Duration::from_millis(1));
        if read.is_err_and(|why| why.raw_os_error() == Some(libc::EBADF)) {
            return (kept, peer);
        }
    }
    panic!("no socket drained in {DRAIN_TRIES} pairs");
}
