//! One descriptor of a socket the board talks on, which never waits in the socket itself: it is
//! nonblocking, and a read or a write that cannot go at once waits for the socket in `poll`, at
//! most the caller's wait each time, as a blocking socket's timeout bounds each of its reads and
//! writes. A wait that runs out says so as that timeout does, `EAGAIN`. Its stream is a Unix
//! socket's, the board's PostgreSQL connection ([`crate::pg`]), or a TCP connection's, a request
//! to a server on this machine ([`crate::rpc::exchange`]).
//!
//! macOS drains a socket when one of its descriptors is closed while another process holds that
//! descriptor, as every `lsof` does for each descriptor it reads (`proc_pidfdinfo`): the socket is
//! marked draining for good, and every later wait in its own blocking read or write fails with
//! EBADF, on each descriptor of it, while what does not wait goes on as before. Each of those
//! sockets has such a close in its making (the board connection's `pg::unshared`, a TCP
//! connection's [`crate::sys::connect`]), and the board's own scans (`lsof +D`, `lsof -iTCP`)
//! read every process's descriptors beside them. A drained socket's `poll`, and its reads and
//! writes that do not wait, answer as a sound socket's do (measured on macOS 26, a Unix pair and a
//! loopback TCP pair alike), so a socket that waits only in `poll` is one no drain can break.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[cfg(all(test, target_vendor = "apple"))]
pub(crate) mod drain;

/// What a [`Socket`] asks of its stream, which std's Unix and TCP streams each answer by their own
/// methods: one descriptor, made nonblocking, read and written once at a time through a shared
/// reference, and shut down.
pub(crate) trait Stream: AsRawFd {
    /// The stream nonblocking from now on.
    fn nonblocking(&self) -> io::Result<()>;
    /// One read: what the socket holds now, or that it would wait.
    fn read_once(&self, buf: &mut [u8]) -> io::Result<usize>;
    /// One write: what the socket takes now, or that it would wait.
    fn write_once(&self, buf: &[u8]) -> io::Result<usize>;
    /// The socket's reads, its writes or both ended, for every descriptor of it.
    fn end(&self, how: Shutdown) -> io::Result<()>;
}

/// [`Stream`] for each of std's streams named, by that stream's own methods.
macro_rules! streams {
    ($($kind:ty),+) => {$(
        impl Stream for $kind {
            fn nonblocking(&self) -> io::Result<()> {
                self.set_nonblocking(true)
            }

            fn read_once(&self, buf: &mut [u8]) -> io::Result<usize> {
                let mut from: &$kind = self;
                from.read(buf)
            }

            fn write_once(&self, buf: &[u8]) -> io::Result<usize> {
                let mut to: &$kind = self;
                to.write(buf)
            }

            fn end(&self, how: Shutdown) -> io::Result<()> {
                self.shutdown(how)
            }
        }
    )+};
}

streams!(UnixStream, TcpStream);

/// One descriptor of a socket, and the most each of its reads and writes waits: the board
/// client's wait ([`crate::pg::Client::waiting`]), or an exchange's own.
#[derive(Debug)]
pub(crate) struct Socket<S> {
    pub(crate) socket: S,
    pub(crate) wait: Duration,
}

impl<S: Stream> Socket<S> {
    /// `socket`, nonblocking from now on, each read and write of it waiting at most `wait`. A
    /// zero wait is refused, as std refuses a socket's zero timeout: it would never wait at all.
    pub(crate) fn waiting(socket: S, wait: Duration) -> io::Result<Socket<S>> {
        if wait.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot set a 0 duration timeout",
            ));
        }
        socket.nonblocking()?;
        Ok(Socket { socket, wait })
    }

    /// As much of `out` as the socket takes now, written without waiting: how many bytes that
    /// was, none when it takes nothing.
    pub(crate) fn write_now(&self, out: &[u8]) -> io::Result<usize> {
        let mut taken = 0;
        while taken < out.len() {
            match self.socket.write_once(&out[taken..]) {
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
    pub(crate) fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.socket.end(how)
    }

    /// What `once` gives the first time it goes without waiting, waiting between for `events`
    /// in `poll`, at most the socket's wait from the first time it would have waited.
    fn waited<T>(
        &self,
        events: libc::c_short,
        mut once: impl FnMut(&S) -> io::Result<T>,
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
    /// read or write still would wait never keeps the operation past the socket's wait.
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

impl<S: Stream> Read for &Socket<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.waited(libc::POLLIN, |socket| socket.read_once(buf))
    }
}

impl<S: Stream> Read for Socket<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl<S: Stream> Write for &Socket<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.waited(libc::POLLOUT, |socket| socket.write_once(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Socket;
    use std::io::{self, Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
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

    /// A loopback TCP connection's two ends: ours, then its peer's.
    fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (ours, listener.accept().unwrap().0)
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

    /// A TCP connection's socket as a Unix one's: nonblocking, a read that meets its wait as
    /// EAGAIN, writes and reads each way, what the socket takes now, and a shutdown that ends a
    /// read waiting on another thread.
    #[test]
    fn a_tcp_socket_waits_reads_writes_and_shuts_down_as_a_unix_one() {
        let (ours, mut peer) = tcp_pair();
        let mut socket = Socket::waiting(ours, SHORT).unwrap();
        let why = (&socket).read(&mut [0; 1]).unwrap_err();
        assert!(eagain(&why), "{why}");
        socket.wait = LONG;
        let heard = bounded("a TCP socket read and written each way", move || {
            let mut heard = Vec::new();
            let mut got = [0; 4];
            assert_eq!(socket.write_now(b"ping").unwrap(), 4);
            peer.read_exact(&mut got).unwrap();
            heard.push(got);
            peer.write_all(b"pong").unwrap();
            (&socket).read_exact(&mut got).unwrap();
            heard.push(got);
            (&socket).write_all(b"more").unwrap();
            peer.read_exact(&mut got).unwrap();
            heard.push(got);
            heard
        });
        assert_eq!(heard, [*b"ping", *b"pong", *b"more"]);

        let (ours, _peer) = tcp_pair();
        let socket = std::sync::Arc::new(Socket::waiting(ours, Duration::MAX).unwrap());
        let reader = std::sync::Arc::clone(&socket);
        let read = bounded("a TCP read a shutdown ends", move || {
            let reading = thread::spawn(move || (&*reader).read(&mut [0; 1]));
            socket.shutdown(Shutdown::Both).unwrap();
            reading.join().unwrap()
        });
        assert_eq!(read.unwrap(), 0, "the end of the socket");
    }

    /// A socket macOS has drained (a descriptor of it closed while a read held that descriptor,
    /// as `lsof` holds each it reads): a blocking read of it fails with EBADF, the control, and
    /// the connection's socket still waits, reads and writes as a sound one does.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_socket_macos_drained_still_waits_reads_and_writes_in_poll() {
        let (kept, mut peer) = super::drain::drained::<UnixStream>();
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

    /// An operation that always would wait, on a socket `poll` always calls ready, meets its wait:
    /// without the deadline checked before each `poll` it would never end, which the bound fails
    /// as a stall.
    #[test]
    fn a_socket_poll_calls_ready_whose_operation_still_would_wait_meets_its_wait() {
        let (ours, _peer) = UnixStream::pair().unwrap();
        // An empty socket is always ready to write: poll answers at once every time.
        let socket = Socket::waiting(ours, SHORT).unwrap();
        let said = bounded("an operation that always would wait", move || {
            socket.waited(libc::POLLOUT, |_| {
                Err::<(), _>(io::ErrorKind::WouldBlock.into())
            })
        });
        let why = said.unwrap_err();
        assert!(eagain(&why), "{why}");
    }
}
