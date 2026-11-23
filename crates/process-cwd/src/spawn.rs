//! Children started without a fork, and the pipes their input and output go through, which no child
//! another thread spawns can hold open: the board's helpers (`symphony_board::sys` re-exports them) and the
//! scans' `lsof`.
//!
//! std takes its fork path for a `pre_exec`, a session, or a changed environment with a bare program
//! name. On macOS that path makes its exec-status pipe with `pipe()` and marks it close-on-exec in a
//! second call, and `spawn()` reads it to its end: a child another thread spawns in that gap keeps the
//! writer, and `spawn()` waits for that child's whole life. Here macOS spawns with `posix_spawn` and
//! `POSIX_SPAWN_CLOEXEC_DEFAULT`, the way `gate::process::spawn::Launch` does: no status pipe exists,
//! and the child inherits only its standard streams and the descriptors it is handed, never another
//! thread's not-yet-close-on-exec ends. Elsewhere std's spawn is used: `pipe2` and its close-on-exec
//! socket pair are atomic there.
//!
//! Close-on-exec is not enough on macOS: every spawn and fork copies each descriptor not
//! close-on-fork into the child it makes until that child's exec, and a spawn that fails leaves its
//! child holding them after it returns, until the kernel tears it down (milliseconds, measured).
//! So every [`pipe`]'s ends are close-on-fork, which no spawn copies; the copies a child is handed
//! exist only under [`spawning`]; and a bare name is looked up, and the file found read for what
//! exec answers (a file it may not run, a `#!` line's interpreter missing or refused, no `#!` line
//! at all, which `sh` runs), before anything is spawned. A start then leaves no failed child behind
//! but where only an exec can answer (a file it cannot read, a `#!` line it cannot settle, a header
//! the kernel refuses), whose child holds the copies it was handed for those milliseconds. An end
//! reaches a child only through [`Command`], never through a std spawn, which cannot be handed one.
//! A TCP socket kept across spawns is close-on-fork once [`connect`] has made it, and close-on-exec
//! from just after it exists.
//!
//! A start that fails answers the error std's answers, with one difference, on macOS before 26:
//! a name with a slash the kernel refuses for its path fails here with the kernel's error, as std
//! fails it on macOS 26 and on Linux, where std's is ENOENT. That is a path not found (ENOTDIR,
//! ELOOP, ENAMETOOLONG: a directory to start from that is a file, a path through one) and any
//! other refusal of a name that cannot be read where it is, EACCES for one under a folder it may
//! not search. std starts such a name with `posix_spawnp`, which up to Libc-1698 (macOS 15) ran it
//! through its search's reading of a refusal, ENOENT for each of those, and since Libc-1725
//! (macOS 26) hands on the refusal of a name it did not search for.
//!
//! A pipe's first ends are not close-on-exec until a second call, and a std child keeps each
//! descriptor not close-on-exec at its spawn for its life: a std spawn in a binary that uses the
//! helpers takes [`spawning`] for its spawn alone ([`Spawning`]), so it never starts in that gap.
//!
//! [`pipe`] is a real pipe, not a FIFO, though a FIFO opens close-on-exec in one call: macOS can
//! lose a FIFO's end, and a read already blocked on one when its last writer goes may wait for
//! good, while a nonblocking read on it answers EOF (measured, about once in a few hundred
//! thousand reads under load).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, PipeReader, PipeWriter, Read};
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};

mod control;
pub use control::{Interrupts, set_nonblocking};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdin, ChildStdout, ExitStatus, Output};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;
#[cfg(target_vendor = "apple")]
use std::time::Instant;

static SPAWNING: Mutex<()> = Mutex::new(());

/// Held while this process holds a descriptor for a child that any spawn would copy: the copies
/// [`Command::spawn`] hands its child, and a [`pipe`] or [`connect`]'s socket until its descriptors
/// are close-on-fork. Every helper spawn holds it, and a std spawn in a binary that uses the helpers
/// takes it around its `spawn()` ([`Spawning`]), so that it never copies one (a std child keeps each
/// descriptor not close-on-exec for its life).
/// Never held across a helper's spawn, pipe or [`connect`]: they take it themselves.
pub fn spawning() -> MutexGuard<'static, ()> {
    SPAWNING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// std's spawn, status and output, each with [`spawning`] held for the spawn alone and released
/// before the wait: what a std spawn in a binary that uses the helpers starts its child with, so
/// that the child never keeps a [`pipe`]'s first ends, and the lock never waits on a child. What each
/// answers is std's own.
pub trait Spawning {
    /// std's `spawn`, under [`spawning`].
    fn spawn_locked(&mut self) -> io::Result<std::process::Child>;

    /// std's `status`: its streams inherited unless set, spawned under [`spawning`], waited after.
    fn status_locked(&mut self) -> io::Result<ExitStatus>;

    /// std's `output` for a command whose streams are not set: stdin null and both outputs piped
    /// (set here, as [`output`] sets them), spawned under [`spawning`], read to their end after.
    fn output_locked(&mut self) -> io::Result<Output>;
}

impl Spawning for std::process::Command {
    fn spawn_locked(&mut self) -> io::Result<std::process::Child> {
        let _spawning = spawning();
        self.spawn()
    }

    fn status_locked(&mut self) -> io::Result<ExitStatus> {
        self.spawn_locked()?.wait()
    }

    fn output_locked(&mut self) -> io::Result<Output> {
        self.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn_locked()?
            .wait_with_output()
    }
}

/// Where a child's standard stream goes: this process's own, `/dev/null`, a descriptor handed
/// to the child alone, or a pipe whose other end [`Child`] keeps.
#[derive(Debug)]
pub enum Stdio {
    Inherit,
    Null,
    Fd(OwnedFd),
    Piped,
}

impl Stdio {
    pub fn null() -> Self {
        Self::Null
    }

    pub fn inherit() -> Self {
        Self::Inherit
    }

    /// The stream through a real pipe of its own, as std's `Stdio::piped` makes it, which no other
    /// spawn copies: the child gets one end, and the other is the matching `stdin`, `stdout` or
    /// `stderr` of its [`Child`].
    pub fn piped() -> Self {
        Self::Piped
    }
}

impl From<OwnedFd> for Stdio {
    fn from(fd: OwnedFd) -> Self {
        Self::Fd(fd)
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Self {
        Self::Fd(file.into())
    }
}

impl From<PipeReader> for Stdio {
    fn from(pipe: PipeReader) -> Self {
        Self::Fd(pipe.into())
    }
}

impl From<PipeWriter> for Stdio {
    fn from(pipe: PipeWriter) -> Self {
        Self::Fd(pipe.into())
    }
}

/// A child to start, built as std's `Command` is (the same method names), which can also say what a
/// std `Command` cannot after the fact: that its environment was cleared, its group and its session.
/// Its standard streams are handed to the child at [`Command::spawn`]: the command keeps none after,
/// so it never holds a writer whose end its reader waits for.
#[derive(Debug)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    cleared: bool,
    vars: BTreeMap<OsString, Option<OsString>>,
    group: Option<i32>,
    session: bool,
    kept: Vec<RawFd>,
    /// The signals the child starts with at their default action.
    defaults: Vec<libc::c_int>,
    /// Whether the child starts with no signal blocked.
    unblocked: bool,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl Command {
    // Each generic method hands its text to one that is not, so the code that builds or changes the
    // command is compiled once, here, rather than again in every crate and for every type it is
    // called with.

    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self::named(program.as_ref())
    }

    fn named(program: &OsStr) -> Self {
        Self {
            program: program.to_owned(),
            args: Vec::new(),
            cwd: None,
            cleared: false,
            vars: BTreeMap::new(),
            group: None,
            session: false,
            kept: Vec::new(),
            defaults: Vec::new(),
            unblocked: false,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.push_arg(arg.as_ref())
    }

    fn push_arg(&mut self, arg: &OsStr) -> &mut Self {
        self.args.push(arg.to_owned());
        self
    }

    pub fn args(&mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> &mut Self {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.set_var(key.as_ref(), value.as_ref())
    }

    fn set_var(&mut self, key: &OsStr, value: &OsStr) -> &mut Self {
        self.vars.insert(key.to_owned(), Some(value.to_owned()));
        self
    }

    pub fn envs(
        &mut self,
        vars: impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>,
    ) -> &mut Self {
        for (key, value) in vars {
            self.env(key, value);
        }
        self
    }

    /// `key` left out of the child's environment, as std's `env_remove` leaves it out.
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.remove_var(key.as_ref())
    }

    fn remove_var(&mut self, key: &OsStr) -> &mut Self {
        let key = key.to_owned();
        if self.cleared {
            self.vars.remove(&key);
        } else {
            self.vars.insert(key, None);
        }
        self
    }

    /// The child's environment is only what is set after this, as std's `env_clear` makes it.
    pub fn env_clear(&mut self) -> &mut Self {
        self.cleared = true;
        self.vars.clear();
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.set_dir(dir.as_ref())
    }

    fn set_dir(&mut self, dir: &Path) -> &mut Self {
        self.cwd = Some(dir.to_owned());
        self
    }

    /// The child in process group `group`, or leading a group of its own for 0, as std's
    /// `process_group` puts it.
    pub fn process_group(&mut self, group: i32) -> &mut Self {
        self.group = Some(group);
        self
    }

    /// The child in a session of its own (`setsid`), away from its starter's terminal.
    pub fn new_session(&mut self) -> &mut Self {
        self.session = true;
        self
    }

    /// Descriptor `fd` handed to the child under the same number; it must stay open until
    /// [`Command::spawn`]. This process's copy keeps its close-on-exec flag.
    pub fn pass_fd(&mut self, fd: RawFd) -> &mut Self {
        self.kept.push(fd);
        self
    }

    /// The child starts with `signal`, one a process may catch or ignore, at its default action
    /// whatever this process's disposition, as `trap - SIGNAL` cannot make it in a shell that
    /// found it ignored (a background job of a non-interactive shell inherits SIGINT and SIGQUIT
    /// so). Every other disposition is inherited as std's spawn inherits it; a number that names
    /// no such signal fails the spawn.
    pub fn default_signal(&mut self, signal: libc::c_int) -> &mut Self {
        self.defaults.push(signal);
        self
    }

    /// The child starts with every signal a process may catch or ignore at its default action
    /// ([`Command::default_signal`] for each) and none blocked, as launchd or cron starts a job,
    /// whatever this process was started ignoring or blocking: a background job of a
    /// non-interactive shell ignores INT and QUIT, and one under `nohup` HUP, which a child
    /// otherwise keeps across its exec.
    pub fn default_signals(&mut self) -> &mut Self {
        self.defaults
            .extend((1..=MAX_SIGNAL).filter(|&signal| catchable(signal)));
        self.unblocked = true;
        self
    }

    pub fn stdin(&mut self, io: impl Into<Stdio>) -> &mut Self {
        self.stdin = Some(io.into());
        self
    }

    pub fn stdout(&mut self, io: impl Into<Stdio>) -> &mut Self {
        self.stdout = Some(io.into());
        self
    }

    pub fn stderr(&mut self, io: impl Into<Stdio>) -> &mut Self {
        self.stderr = Some(io.into());
        self
    }

    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    pub fn get_args(&self) -> impl ExactSizeIterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    pub fn get_current_dir(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Start the child, its standard streams this process's own unless set. One that cannot start
    /// fails with the error std's spawn gives it.
    pub fn spawn(&mut self) -> io::Result<Child> {
        self.catchable()?;
        let [stdin, stdout, stderr] = [self.stdin.take(), self.stdout.take(), self.stderr.take()]
            .map(|io| io.unwrap_or(Stdio::Inherit));
        // Before the lock: a pipe's making takes it itself.
        let (stdin, input) = handed(stdin, true)?;
        let (stdout, out) = handed(stdout, false)?;
        let (stderr, err) = handed(stderr, false)?;
        // The copies handed to the child live inside native::spawn alone.
        let process = {
            let _spawning = spawning();
            native::spawn(self, [stdin, stdout, stderr])?
        };
        Ok(Child {
            process,
            stdin: input.map(ChildStdin::from),
            stdout: out.map(ChildStdout::from),
            stderr: err.map(ChildStderr::from),
        })
    }

    /// EINVAL, before anything is made, for a [`Command::default_signal`] that names no signal a
    /// process may catch or ignore, as `sigaction` refuses one to set: macOS's `sigaddset` takes
    /// any number, so both platforms answer the same.
    fn catchable(&self) -> io::Result<()> {
        if self.defaults.iter().all(|&signal| catchable(signal)) {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(libc::EINVAL))
        }
    }

    /// The child's environment as std makes it: this process's as it stands when nothing changed it,
    /// else sorted by name, from this process's unless cleared, with the changes.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(dead_code, reason = "std builds it there")
    )]
    fn environment(&self) -> Vec<(OsString, OsString)> {
        if !self.cleared && self.vars.is_empty() {
            return std::env::vars_os().collect();
        }
        let mut env: BTreeMap<OsString, OsString> = if self.cleared {
            BTreeMap::new()
        } else {
            std::env::vars_os().collect()
        };
        for (key, value) in &self.vars {
            match value {
                Some(value) => env.insert(key.clone(), value.clone()),
                None => env.remove(key),
            };
        }
        env.into_iter().collect()
    }

    /// Whether std starts it through its fork path, which changes directory before anything else:
    /// for a `pre_exec` (what a session, a passed descriptor, a signal's default or the emptied
    /// mask takes there), a `PATH` changed or cleared under a bare name, or a relative name with a
    /// directory to start from.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(dead_code, reason = "std starts it there")
    )]
    fn forks(&self) -> bool {
        let program = self.program.as_bytes();
        let bare = !program.contains(&b'/');
        self.session
            || !self.kept.is_empty()
            || !self.defaults.is_empty()
            || self.unblocked
            || (bare && (self.cleared || self.vars.contains_key(OsStr::new("PATH"))))
            || (!bare && !program.starts_with(b"/") && self.cwd.is_some())
    }
}

/// The highest number [`Command::default_signals`] asks of `sigaction`: above every signal of the
/// systems this crate builds for (macOS's 31, Linux's 64, FreeBSD's 128).
const MAX_SIGNAL: libc::c_int = 128;

/// Whether `signal` names a signal a process may catch or ignore: one `sigaction` reads (not the C
/// library's own, which it refuses) other than KILL and STOP.
fn catchable(signal: libc::c_int) -> bool {
    // SAFETY: sigaction is plain data, for which all zeroes is a value.
    let mut now: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: reads the disposition into `now`, changing none.
    let named = unsafe { libc::sigaction(signal, std::ptr::null(), &raw mut now) } == 0;
    named && signal != libc::SIGKILL && signal != libc::SIGSTOP
}

/// What the child's stream is handed, and for a piped one the end this process keeps of a [`pipe`]:
/// the writer for the child's `input`, the reader for an output. The child's end goes with the
/// spawn, so only the child ever holds it.
fn handed(io: Stdio, input: bool) -> io::Result<(Stdio, Option<OwnedFd>)> {
    let Stdio::Piped = io else {
        return Ok((io, None));
    };
    let (reader, writer) = pipe()?;
    Ok(if input {
        (reader.into(), Some(writer.into()))
    } else {
        (writer.into(), Some(reader.into()))
    })
}

/// A started child, as std's `Child` is used: its id, an end asked or waited for, and the pipes
/// its [`Stdio::piped`] streams are read and written through. Dropping it neither stops nor reaps
/// the child.
#[derive(Debug)]
pub struct Child {
    process: native::Process,
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
}

impl Child {
    pub fn id(&self) -> u32 {
        self.process.id()
    }

    /// SIGKILL to the child; nothing once it has been reaped.
    pub fn kill(&mut self) -> io::Result<()> {
        self.process.kill()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }

    /// The child's end, its input closed first, as std's `wait` closes it.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        self.process.wait()
    }
}

/// A real pipe whose two ends no spawn or fork copies, so the reader's EOF comes when the writers
/// handed out end, and a read already blocked on it sees that end. macOS has no `pipe2` before
/// macOS 27: `pipe()` makes inheritable ends and only `open` makes one close-on-fork, so under
/// [`spawning`] each end is [`reopened`] close-on-exec and close-on-fork, and the first ends are
/// closed before the lock goes. No spawn under [`spawning`] copies them, and no spawn or fork at
/// all copies the ends returned, even until its exec; a std spawn that skips the lock can keep the
/// first ends for its child's life. An end reaches a child only through [`Command`], which hands it
/// a copy made under [`spawning`]: a std spawn, given one, cannot start.
#[cfg(target_vendor = "apple")]
pub fn pipe() -> io::Result<(PipeReader, PipeWriter)> {
    let _spawning = spawning();
    let made = io::pipe()?;
    let reader = reopened(made.0.as_fd(), true, false)?;
    let writer = reopened(made.1.as_fd(), false, true)?;
    drop(made);
    Ok((OwnedFd::from(reader).into(), OwnedFd::from(writer).into()))
}

/// A new descriptor of `fd`'s open file (a [`pipe`]'s end, [`connect`]'s socket), close-on-exec and
/// close-on-fork, opened to read, write or both as `fd` allows. macOS opens `/dev/fd/N` as `dup`
/// copies N, sharing its open file, and only `open` can set close-on-fork. A pipe stays whole when
/// the first descriptor closes while another process reads it (`lsof`): its drain marks it ended
/// only with its open file's last descriptor. A socket's drain has no such check, so that close is
/// the one moment an `lsof` could still mark it draining. Called under [`spawning`], so no spawn
/// copies `fd` meanwhile.
#[cfg(target_vendor = "apple")]
fn reopened(fd: std::os::fd::BorrowedFd<'_>, read: bool, write: bool) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    File::options()
        .read(read)
        .write(write)
        .custom_flags(crate::close_on_fork::O_CLOFORK)
        .open(format!("/dev/fd/{}", fd.as_raw_fd()))
}

/// A pipe whose two ends are close-on-exec from the moment they exist: std's (`pipe2`), as nothing
/// is close-on-fork here.
#[cfg(not(target_vendor = "apple"))]
pub fn pipe() -> io::Result<(PipeReader, PipeWriter)> {
    io::pipe()
}

/// A TCP connection to `to`, waited for at most `timeout`, as std's `TcpStream::connect_timeout`
/// makes it, but with its socket's one descriptor close-on-exec and close-on-fork from the moment it
/// exists: no spawn or fork ever copies it. std makes a socket that is close-on-exec only, and every
/// spawn copies such a descriptor into its child until that child's exec. macOS marks a socket
/// draining when one of its descriptors is closed while another process reads that descriptor
/// (`lsof`), and a draining socket fails every later wait for data with EBADF: a child's exec
/// closing its copy did that to the board's connections. So the socket is made under [`spawning`]
/// and [`reopened`] close-on-exec and close-on-fork, as [`pipe`]'s ends are, its first descriptor
/// closed before the lock goes; it connects after.
#[cfg(target_vendor = "apple")]
pub fn connect(to: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot set a 0 duration timeout",
        ));
    }
    let stream = TcpStream::from(own_socket(to)?);
    // std's sockets never raise SIGPIPE: a write to a closed connection answers EPIPE.
    let on: libc::c_int = 1;
    // SAFETY: on is an initialized int, readable for its size, and stream owns the descriptor.
    let set = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_NOSIGPIPE,
            (&raw const on).cast(),
            libc::socklen_t::try_from(size_of_val(&on)).expect("an int fits socklen_t"),
        )
    };
    if set != 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    if let Err(error) = start_connect(&stream, to) {
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        connected_within(&stream, timeout)?;
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// std's `TcpStream::connect_timeout`: nothing is close-on-fork here, and closing another process's
/// copy of a socket leaves it as it was.
#[cfg(not(target_vendor = "apple"))]
pub fn connect(to: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(to, timeout)
}

/// A TCP socket of `to`'s family, not yet connected, close-on-exec and close-on-fork: made and
/// [`reopened`] under [`spawning`], so no helper spawn copies even its first descriptor.
#[cfg(target_vendor = "apple")]
fn own_socket(to: &SocketAddr) -> io::Result<OwnedFd> {
    let family = if to.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let _spawning = spawning();
    let made = new_socket(family)?;
    let own = reopened(made.as_fd(), true, true)?;
    drop(made);
    Ok(own.into())
}

/// A new TCP socket of `family`, close-on-exec at once, as std makes one: a std spawn that skips
/// [`spawning`] keeps it past its exec only if it starts between these two calls.
#[cfg(target_vendor = "apple")]
fn new_socket(family: libc::c_int) -> io::Result<OwnedFd> {
    use std::os::fd::FromRawFd;
    // SAFETY: socket has no pointer arguments; a descriptor it returns is owned exactly once below.
    let made = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if made == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: made is the new socket's descriptor, which nothing else owns.
    let made = unsafe { OwnedFd::from_raw_fd(made) };
    // SAFETY: F_SETFD sets the descriptor flags of the one made, from an int.
    if unsafe { libc::fcntl(made.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(made)
}

/// `socket`'s connect to `to`, started: done, in progress (EINPROGRESS), or refused.
#[cfg(target_vendor = "apple")]
fn start_connect(socket: &TcpStream, to: &SocketAddr) -> io::Result<()> {
    let family = |family: libc::c_int| {
        libc::sa_family_t::try_from(family).expect("an address family fits sa_family_t")
    };
    let started = match to {
        SocketAddr::V4(to) => {
            let address = libc::sockaddr_in {
                sin_len: u8::try_from(size_of::<libc::sockaddr_in>()).expect("fits sin_len"),
                sin_family: family(libc::AF_INET),
                sin_port: to.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(to.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            // SAFETY: address is an initialized sockaddr_in, readable for its size, and socket
            // owns the descriptor.
            unsafe {
                libc::connect(
                    socket.as_raw_fd(),
                    (&raw const address).cast(),
                    libc::socklen_t::try_from(size_of_val(&address)).expect("fits socklen_t"),
                )
            }
        }
        SocketAddr::V6(to) => {
            let address = libc::sockaddr_in6 {
                sin6_len: u8::try_from(size_of::<libc::sockaddr_in6>()).expect("fits sin6_len"),
                sin6_family: family(libc::AF_INET6),
                sin6_port: to.port().to_be(),
                sin6_flowinfo: to.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: to.ip().octets(),
                },
                sin6_scope_id: to.scope_id(),
            };
            // SAFETY: address is an initialized sockaddr_in6, readable for its size, and socket
            // owns the descriptor.
            unsafe {
                libc::connect(
                    socket.as_raw_fd(),
                    (&raw const address).cast(),
                    libc::socklen_t::try_from(size_of_val(&address)).expect("fits socklen_t"),
                )
            }
        }
    };
    if started == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The end of a connect `socket` started without blocking, waited for at most `timeout`: read as
/// std's `connect_timeout` reads it, the socket's own error after a hang-up or an error.
#[cfg(target_vendor = "apple")]
fn connected_within(socket: &TcpStream, timeout: Duration) -> io::Result<()> {
    let until = Instant::now() + timeout;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "connection timed out",
            ));
        }
        let mut ready = libc::pollfd {
            fd: socket.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        let wait = libc::c_int::try_from(left.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
        // SAFETY: ready is one initialized pollfd, writable across the call.
        match unsafe { libc::poll(&raw mut ready, 1, wait) } {
            -1 => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
            0 => {}
            _ if ready.revents & (libc::POLLHUP | libc::POLLERR) != 0 => {
                return Err(socket
                    .take_error()?
                    .unwrap_or_else(|| io::Error::other("no error set after POLLHUP")));
            }
            _ => return Ok(()),
        }
    }
}

/// `command` started as std's `Command::output` starts it, stdin null and both outputs read through
/// a real pipe of their own: [`wait_output`] then reads them to their end.
pub fn spawn_output(command: &mut Command) -> io::Result<Child> {
    spawn_reading(command, Stdio::Null)
}

/// [`spawn_output`] with the child's stdin [`Stdio::piped`] too: what is written to `stdin`
/// reaches the child, which sees its end once `stdin` is dropped, whether it reads or polls (node
/// never sees a FIFO's end on macOS): a [`pipe`], a real one.
pub fn spawn_piped(command: &mut Command) -> io::Result<Child> {
    spawn_reading(command, Stdio::Piped)
}

/// `command` started with `stdin` and both outputs [`Stdio::piped`], each read through a [`pipe`]
/// of its own, which no other spawn copies. Only the child holds the writers: [`Command::spawn`]
/// hands its streams over.
fn spawn_reading(command: &mut Command, stdin: Stdio) -> io::Result<Child> {
    command
        .stdin(stdin)
        .stdout(Stdio::Piped)
        .stderr(Stdio::Piped)
        .spawn()
}

/// What std's `Child::wait_with_output` answers for a child [`spawn_output`] or [`spawn_piped`]
/// started: its stdin closed, stderr read to its end on a thread of its own, stdout on this one,
/// then the status.
pub fn wait_output(mut child: Child) -> io::Result<Output> {
    drop(child.stdin.take());
    let stderr = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut said = Vec::new();
            stderr.read_to_end(&mut said).map(|_| said)
        })
    });
    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout)?;
    }
    let stderr = match stderr {
        Some(reader) => reader
            .join()
            .unwrap_or_else(|panicked| std::panic::resume_unwind(panicked))?,
        None => Vec::new(),
    };
    let status = child.wait()?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// What std's `Command::output` answers, read through real pipes of its own: what `command` printed
/// is whole once it and the children it handed its outputs to have ended, never later because of
/// another's that took [`spawning`] for its spawn.
pub fn output(command: &mut Command) -> io::Result<Output> {
    wait_output(spawn_output(command)?)
}

#[cfg(target_os = "macos")]
mod native {
    use super::{Command, Stdio};
    use std::ffi::{CString, OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::ExitStatus;

    /// `POSIX_SPAWN_SETSID`, which the libc crate does not name for Apple (`sys/spawn.h`).
    const SETSID: libc::c_int = 0x0400;

    /// The pid stays ours until waitpid reaps it; a cached end prevents both a second reap and a
    /// signal sent to a reused pid.
    #[derive(Debug)]
    pub(super) struct Process {
        pid: libc::pid_t,
        status: Option<ExitStatus>,
    }

    impl Process {
        pub(super) fn id(&self) -> u32 {
            u32::try_from(self.pid).expect("posix_spawn returned a positive child pid")
        }

        pub(super) fn kill(&self) -> io::Result<()> {
            if self.status.is_some() {
                return Ok(());
            }
            // SAFETY: this is our unreaped child's pid, not a process group or a searched pid.
            if unsafe { libc::kill(self.pid, libc::SIGKILL) } == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.reap(libc::WNOHANG)
        }

        pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
            self.reap(0)?
                .ok_or_else(|| io::Error::other("waitpid returned without an exit"))
        }

        fn reap(&mut self, options: libc::c_int) -> io::Result<Option<ExitStatus>> {
            if self.status.is_some() {
                return Ok(self.status);
            }
            loop {
                let mut raw = 0;
                // SAFETY: waits for this owned child only, writing to the stack status word.
                let pid = unsafe { libc::waitpid(self.pid, &raw mut raw, options) };
                if pid == self.pid {
                    self.status = Some(ExitStatus::from_raw(raw));
                    return Ok(self.status);
                }
                if pid == 0 {
                    return Ok(None);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }

    fn checked(code: libc::c_int) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    /// What std refuses a NUL in a program, argument, variable or directory with.
    fn nul() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "nul byte found in provided data",
        )
    }

    fn cstring(bytes: &[u8]) -> io::Result<CString> {
        CString::new(bytes).map_err(|_| nul())
    }

    /// A close-on-exec copy of `fd` numbered above the standard streams, so ordered dup2 actions
    /// never overwrite one another's sources, even when this process's stdin or stdout is closed.
    fn above_stdio(fd: RawFd) -> io::Result<OwnedFd> {
        // SAFETY: duplicates the descriptor without changing it; the copy is close-on-exec.
        let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if copy == -1 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: this newly allocated descriptor has no other owner.
            Ok(unsafe { OwnedFd::from_raw_fd(copy) })
        }
    }

    /// What the child's stream `target` is copied from: none for this process's own when it is
    /// closed, which the child then has closed too.
    fn source(io: &Stdio, target: RawFd) -> io::Result<Option<OwnedFd>> {
        match io {
            Stdio::Inherit => match above_stdio(target) {
                Ok(fd) => Ok(Some(fd)),
                Err(error) if error.raw_os_error() == Some(libc::EBADF) => Ok(None),
                Err(error) => Err(error),
            },
            Stdio::Null => {
                let null = File::options()
                    .read(target == libc::STDIN_FILENO)
                    .write(target != libc::STDIN_FILENO)
                    .open("/dev/null")?;
                above_stdio(null.as_raw_fd()).map(Some)
            }
            Stdio::Fd(fd) => above_stdio(fd.as_raw_fd()).map(Some),
            Stdio::Piped => unreachable!("Command::spawn hands a piped stream its pipe's end"),
        }
    }

    struct Actions(libc::posix_spawn_file_actions_t);

    impl Actions {
        fn new() -> io::Result<Self> {
            let mut raw = std::mem::MaybeUninit::uninit();
            // SAFETY: init writes the opaque handle, which is read only on success.
            checked(unsafe { libc::posix_spawn_file_actions_init(raw.as_mut_ptr()) })?;
            // SAFETY: successful initialization above wrote the handle.
            Ok(Self(unsafe { raw.assume_init() }))
        }

        fn dup(&mut self, from: RawFd, to: RawFd) -> io::Result<()> {
            // SAFETY: the initialized actions own no descriptors; the caller keeps distinct source
            // and target descriptors open through spawn.
            checked(unsafe { libc::posix_spawn_file_actions_adddup2(&raw mut self.0, from, to) })
        }
    }

    impl Drop for Actions {
        fn drop(&mut self) {
            // SAFETY: this handle was initialized once and is destroyed once.
            unsafe { libc::posix_spawn_file_actions_destroy(&raw mut self.0) };
        }
    }

    struct Attributes(libc::posix_spawnattr_t);

    impl Attributes {
        fn new(command: &Command) -> io::Result<Self> {
            let mut raw = std::mem::MaybeUninit::uninit();
            // SAFETY: init writes the opaque handle, which is read only on success.
            checked(unsafe { libc::posix_spawnattr_init(raw.as_mut_ptr()) })?;
            // SAFETY: successful initialization above wrote the handle.
            let mut attrs = Self(unsafe { raw.assume_init() });
            // SAFETY: sigset_t is an integer bitset on macOS; sigemptyset initializes it below.
            let mut defaults = unsafe { std::mem::zeroed() };
            // SAFETY: writes only to the stack signal set.
            unsafe { libc::sigemptyset(&raw mut defaults) };
            // std's spawn sets SIGPIPE's default too.
            for &signal in std::iter::once(&libc::SIGPIPE).chain(&command.defaults) {
                // SAFETY: adds a valid signal (`Command::catchable` checked each asked for) to an
                // initialized set.
                unsafe { libc::sigaddset(&raw mut defaults, signal) };
            }
            let mut flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF;
            if command.unblocked {
                // SAFETY: sigset_t is an integer bitset on macOS; sigemptyset initializes it.
                let mut unblocked = unsafe { std::mem::zeroed() };
                // SAFETY: writes only to the stack signal set.
                unsafe { libc::sigemptyset(&raw mut unblocked) };
                // SAFETY: initialized handles and signal set.
                checked(unsafe {
                    libc::posix_spawnattr_setsigmask(&raw mut attrs.0, &raw const unblocked)
                })?;
                flags |= libc::POSIX_SPAWN_SETSIGMASK;
            }
            // SAFETY: initialized handles and signal set.
            checked(unsafe {
                libc::posix_spawnattr_setsigdefault(&raw mut attrs.0, &raw const defaults)
            })?;
            if let Some(group) = command.group {
                // SAFETY: sets only the initialized attributes; the kernel validates the group.
                checked(unsafe { libc::posix_spawnattr_setpgroup(&raw mut attrs.0, group) })?;
                flags |= libc::POSIX_SPAWN_SETPGROUP;
            }
            if command.session {
                flags |= SETSID;
            }
            let flags = libc::c_short::try_from(flags).expect("spawn flags fit a c_short");
            // SAFETY: initialized attributes. The mask unless emptied, and every disposition not
            // set to its default, are inherited, as std's.
            checked(unsafe { libc::posix_spawnattr_setflags(&raw mut attrs.0, flags) })?;
            Ok(attrs)
        }
    }

    impl Drop for Attributes {
        fn drop(&mut self) {
            // SAFETY: this handle was initialized once and is destroyed once.
            unsafe { libc::posix_spawnattr_destroy(&raw mut self.0) };
        }
    }

    /// The names a bare program is looked for under, as `execvp` and `posix_spawnp` write them: each
    /// entry of the child's own `PATH` (macOS's `/usr/bin:/bin` without one), an empty one `.`.
    fn candidates(program: &OsStr, path: Option<&OsStr>) -> Vec<PathBuf> {
        let path = path.unwrap_or_else(|| OsStr::new("/usr/bin:/bin"));
        std::env::split_paths(path)
            .map(|dir| {
                if dir.as_os_str().is_empty() {
                    Path::new(".").join(program)
                } else {
                    dir.join(program)
                }
            })
            .collect()
    }

    /// A child's pid, or the error number its spawn answered.
    type Spawned = Result<libc::pid_t, libc::c_int>;

    /// What `chdir(dir)` answers, asked without changing this process's directory: the lookup's own
    /// error, then ENOTDIR for anything but a directory, then a directory it may not search.
    fn enterable(dir: &Path) -> io::Result<()> {
        if !std::fs::metadata(dir)?.is_dir() {
            return Err(io::Error::from_raw_os_error(libc::ENOTDIR));
        }
        let dir = cstring(dir.as_os_str().as_bytes())?;
        // SAFETY: the path is NUL-terminated and outlives the call, which only reads it.
        if unsafe { libc::access(dir.as_ptr(), libc::X_OK) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// What an exec of a file answers, where the file itself says it.
    enum Image {
        /// Only an exec can say: it is spawned, and its refusal is the answer.
        Spawn,
        /// ENOEXEC, which `execvp` and `posix_spawnp` answer by running it with `sh`.
        Shell,
        /// Refused before it runs, with this error number.
        Refused(libc::c_int),
    }

    /// The first bytes of a Mach-O or universal file in either byte order: an image the kernel
    /// loads itself (`mach-o/loader.h`, `mach-o/fat.h`).
    const HEADERS: [[u8; 4]; 8] = [
        [0xce, 0xfa, 0xed, 0xfe],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xfe, 0xed, 0xfa, 0xce],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xca, 0xfe, 0xba, 0xbe],
        [0xca, 0xfe, 0xba, 0xbf],
        [0xbe, 0xba, 0xfe, 0xca],
        [0xbf, 0xba, 0xfe, 0xca],
    ];

    /// How much of a file the kernel reads for its `#!` line (`IMG_SHSIZE`).
    const LINE: u64 = 512;

    /// Whether `path` may be executed, as the kernel checks before it reads a byte: a regular file
    /// it may execute (EACCES for anything else); `None` when the lookup or the check fails
    /// otherwise, which only an exec answers.
    fn executable(path: &Path) -> Option<Result<(), libc::c_int>> {
        let found = std::fs::metadata(path).ok()?;
        if !found.is_file() {
            return Some(Err(libc::EACCES));
        }
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: the path is NUL-terminated and outlives the call, which only reads it.
        if unsafe { libc::access(name.as_ptr(), libc::X_OK) } == 0 {
            return Some(Ok(()));
        }
        (io::Error::last_os_error().raw_os_error() == Some(libc::EACCES))
            .then_some(Err(libc::EACCES))
    }

    /// What exec answers for the file at `path`, read from its mode and first bytes where they say:
    /// a file it may not execute is refused (EACCES); a `#!` line's interpreter is refused as that
    /// file would be, and one that is not there answers ENOENT (an interpreter is looked up from
    /// `dir`, the child's directory); a file with neither a `#!` line nor a Mach-O header is ENOEXEC.
    /// Anything a read cannot settle (a file not found or unreadable, a `#!` line not ended within
    /// what the kernel reads, a `#` in it) is spawned for the kernel's own answer.
    fn image(dir: &Path, path: &Path) -> Image {
        match executable(path) {
            Some(Ok(())) => {}
            Some(Err(code)) => return Image::Refused(code),
            None => return Image::Spawn,
        }
        use std::io::Read as _;
        let mut first = Vec::new();
        let read = File::open(path).and_then(|file| file.take(LINE).read_to_end(&mut first));
        if read.is_err() {
            return Image::Spawn;
        }
        let Some(line) = first.strip_prefix(b"#!") else {
            return if first
                .get(..4)
                .is_some_and(|head| HEADERS.iter().any(|h| h == head))
            {
                Image::Spawn
            } else {
                Image::Shell
            };
        };
        let Some(end) = line.iter().position(|&byte| byte == b'\n') else {
            return Image::Spawn;
        };
        let line = &line[..end];
        let interpreter = line
            .split(|&byte| byte == b' ' || byte == b'\t')
            .find(|word| !word.is_empty());
        let Some(interpreter) = interpreter.filter(|_| !line.contains(&b'#')) else {
            return Image::Spawn;
        };
        if interpreter.contains(&0) {
            return Image::Spawn;
        }
        let interpreter = dir.join(OsStr::from_bytes(interpreter));
        match std::fs::metadata(&interpreter) {
            Err(e) if matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR)) => {
                Image::Refused(e.raw_os_error().unwrap_or(libc::ENOENT))
            }
            Err(_) => Image::Spawn,
            Ok(_) => match executable(&interpreter) {
                Some(Err(code)) => Image::Refused(code),
                _ => Image::Spawn,
            },
        }
    }

    fn pointers(strings: &[CString]) -> Vec<*mut libc::c_char> {
        strings
            .iter()
            .map(|text| text.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect()
    }

    pub(super) fn spawn(command: &Command, streams: [Stdio; 3]) -> io::Result<Process> {
        let argv: Vec<CString> = std::iter::once(&command.program)
            .chain(&command.args)
            .map(|arg| cstring(arg.as_bytes()))
            .collect::<io::Result<_>>()?;
        let env = command.environment();
        let environment: Vec<CString> = env
            .iter()
            .map(|(key, value)| {
                let mut entry = key.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(value.as_bytes());
                cstring(&entry)
            })
            .collect::<io::Result<_>>()?;
        let directory = command
            .cwd
            .as_ref()
            .map(|dir| cstring(dir.as_os_str().as_bytes()))
            .transpose()?;
        let base = match &command.cwd {
            Some(dir) if dir.is_absolute() => dir.clone(),
            Some(dir) => std::env::current_dir()?.join(dir),
            None => PathBuf::new(),
        };
        let argv = pointers(&argv);
        let envp = pointers(&environment);
        for &fd in &command.kept {
            // SAFETY: queries the caller's descriptor before copies are made that could reuse an
            // invalid number. A requested but closed descriptor is an error, never inherited.
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        let kept: Vec<OwnedFd> = command
            .kept
            .iter()
            .map(|&fd| above_stdio(fd))
            .collect::<io::Result<_>>()?;
        let [stdin, stdout, stderr] = streams;
        let sources = [
            source(&stdin, libc::STDIN_FILENO)?,
            source(&stdout, libc::STDOUT_FILENO)?,
            source(&stderr, libc::STDERR_FILENO)?,
        ];
        let mut actions = Actions::new()?;
        if let Some(directory) = &directory {
            // SAFETY: the directory string outlives these file actions and the spawn.
            checked(unsafe {
                libc::posix_spawn_file_actions_addchdir_np(&raw mut actions.0, directory.as_ptr())
            })?;
        }
        for (target, from) in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO]
            .into_iter()
            .zip(&sources)
        {
            if let Some(from) = from {
                actions.dup(from.as_raw_fd(), target)?;
            }
        }
        for (copy, &target) in kept.iter().zip(&command.kept) {
            actions.dup(copy.as_raw_fd(), target)?;
        }
        let attrs = Attributes::new(command)?;
        // One spawn of `path`, made absolute from the child's directory: macOS's spawn answers
        // ENOENT for a relative one it started from there.
        let spawn_at = |path: &Path, argv: &[*mut libc::c_char]| -> io::Result<Spawned> {
            let path = cstring(base.join(path).as_os_str().as_bytes())?;
            #[cfg(test)]
            super::tests::SPAWNS.set(super::tests::SPAWNS.get() + 1);
            let mut pid = 0;
            // SAFETY: the pointer arrays are NUL-terminated, their strings and the opaque handles
            // stay live through the call, and pid is written only on success. No fork hook runs.
            let code = unsafe {
                libc::posix_spawn(
                    &raw mut pid,
                    path.as_ptr(),
                    &raw const actions.0,
                    &raw const attrs.0,
                    argv.as_ptr(),
                    envp.as_ptr(),
                )
            };
            Ok(if code == 0 { Ok(pid) } else { Err(code) })
        };
        // A file with no `#!` line (ENOEXEC) is run as `sh NAME ARGS`, NAME as it was looked up,
        // and that start's answer is the answer, as `execvp` and `posix_spawnp` run it.
        let shell = |name: &Path| -> io::Result<Spawned> {
            let script: Vec<CString> = [OsStr::new("sh"), name.as_os_str()]
                .into_iter()
                .chain(command.args.iter().map(OsString::as_os_str))
                .map(|arg| cstring(arg.as_bytes()))
                .collect::<io::Result<_>>()?;
            spawn_at(Path::new("/bin/sh"), &pointers(&script))
        };
        // What the exec would answer is read first where the file says it, so the one spawn is
        // the start (a refused spawn's child holds the copies it was handed after the refusal);
        // from a directory the child cannot enter, its change of directory answers first.
        let readable = command.cwd.is_none() || enterable(&base).is_ok();
        let start = |name: &Path| -> io::Result<Spawned> {
            let image = if readable {
                image(&base, &base.join(name))
            } else {
                Image::Spawn
            };
            match image {
                Image::Refused(code) => Ok(Err(code)),
                Image::Shell => shell(name),
                Image::Spawn => match spawn_at(name, &argv)? {
                    Err(libc::ENOEXEC) => shell(name),
                    started => Ok(started),
                },
            }
        };
        let program = Path::new(&command.program);
        let started = if command.program.as_bytes().contains(&b'/') {
            // A name with a slash is started as it is, its own error the answer.
            start(program)?
        } else {
            let path = env
                .iter()
                .find(|(key, _)| key == "PATH")
                .map(|(_, value)| value.as_os_str());
            'search: {
                if command.program.is_empty() {
                    break 'search Err(libc::ENOENT);
                }
                // A spawn refused for a name that is not there leaves its child holding the copies
                // it was handed until the kernel tears it down, after the refusal: such names are
                // passed over unspawned while one that is there remains, and spawned for the
                // kernel's own answer only when none is. One that is there and that its file says
                // exec refuses is passed over by `start` unspawned too.
                let (there, missing): (Vec<PathBuf>, Vec<PathBuf>) =
                    candidates(&command.program, path)
                        .into_iter()
                        .partition(|name| std::fs::metadata(base.join(name)).is_ok());
                let mut denied = false;
                for name in if there.is_empty() { missing } else { there } {
                    match start(&name)? {
                        Err(libc::ELOOP | libc::ENAMETOOLONG | libc::ENOENT | libc::ENOTDIR) => {}
                        Err(code @ (libc::E2BIG | libc::ENOMEM | libc::ETXTBSY)) => {
                            break 'search Err(code);
                        }
                        // Any other refusal of a name that is not there passes it over; of one
                        // that is, EACCES (a file it may not run, not a folder it may not search)
                        // is kept for the answer while the search goes on, anything else is it.
                        Err(code) if std::fs::metadata(base.join(&name)).is_ok() => {
                            if code != libc::EACCES {
                                break 'search Err(code);
                            }
                            denied = true;
                        }
                        Err(_) => {}
                        started => break 'search started,
                    }
                }
                Err(if denied { libc::EACCES } else { libc::ENOENT })
            }
        };
        started
            .map(|pid| Process { pid, status: None })
            .map_err(|code| {
                // std's fork path changes directory before it looks: a directory it cannot enter
                // is the answer there. Its other path's search reads that as a name not found.
                let entered = match &command.cwd {
                    Some(dir) if command.forks() => enterable(dir),
                    _ => Ok(()),
                };
                entered
                    .err()
                    .unwrap_or_else(|| io::Error::from_raw_os_error(code))
            })
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{Command, Stdio};
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::ExitStatus;

    /// std's child: its fork path's status pipe and its own pipes are close-on-exec at creation here.
    #[derive(Debug)]
    pub(super) struct Process(std::process::Child);

    impl Process {
        pub(super) fn id(&self) -> u32 {
            self.0.id()
        }

        pub(super) fn kill(&mut self) -> io::Result<()> {
            self.0.kill()
        }

        pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.0.try_wait()
        }

        pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
            self.0.wait()
        }
    }

    fn std_io(io: Stdio) -> std::process::Stdio {
        match io {
            Stdio::Inherit => std::process::Stdio::inherit(),
            Stdio::Null => std::process::Stdio::null(),
            Stdio::Fd(fd) => fd.into(),
            Stdio::Piped => unreachable!("Command::spawn hands a piped stream its pipe's end"),
        }
    }

    pub(super) fn spawn(
        command: &Command,
        [stdin, stdout, stderr]: [Stdio; 3],
    ) -> io::Result<Process> {
        let mut child = std::process::Command::new(&command.program);
        child.args(&command.args);
        if command.cleared {
            child.env_clear();
        }
        for (key, value) in &command.vars {
            match value {
                Some(value) => child.env(key, value),
                None => child.env_remove(key),
            };
        }
        if let Some(dir) = &command.cwd {
            child.current_dir(dir);
        }
        if let Some(group) = command.group {
            child.process_group(group);
        }
        child
            .stdin(std_io(stdin))
            .stdout(std_io(stdout))
            .stderr(std_io(stderr));
        if command.session
            || !command.kept.is_empty()
            || !command.defaults.is_empty()
            || command.unblocked
        {
            let (session, kept) = (command.session, command.kept.clone());
            let (defaults, unblocked) = (command.defaults.clone(), command.unblocked);
            // SAFETY: setsid, fcntl, signal and sigprocmask are async-signal-safe and change only
            // the forked child.
            unsafe {
                child.pre_exec(move || {
                    if session && libc::setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    for &fd in &kept {
                        if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    for &signal in &defaults {
                        if libc::signal(signal, libc::SIG_DFL) == libc::SIG_ERR {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    if unblocked {
                        let mut none: libc::sigset_t = std::mem::zeroed();
                        libc::sigemptyset(&raw mut none);
                        if libc::sigprocmask(
                            libc::SIG_SETMASK,
                            &raw const none,
                            std::ptr::null_mut(),
                        ) == -1
                        {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        child.spawn().map(Process)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[cfg(target_os = "macos")]
    thread_local! {
        /// The spawns this thread's helper starts have made, counted where each is made.
        pub(super) static SPAWNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    /// What `run` answers, and how many spawns this thread's helper made for it.
    #[cfg(target_os = "macos")]
    fn counted<T>(run: impl FnOnce() -> T) -> (T, u32) {
        SPAWNS.set(0);
        let answer = run();
        (answer, SPAWNS.get())
    }

    /// A folder of the test's own, removed however the test ends.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("symphony-spawn-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }

        fn script(&self, name: &str, body: &str, mode: u32) -> PathBuf {
            self.file(name, &format!("#!/bin/sh\n{body}\n"), mode)
        }

        fn file(&self, name: &str, text: &str, mode: u32) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Children killed and reaped however the test ends.
    struct Children(Vec<std::process::Child>);

    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// The same calls on std's `Command` and on this one: what std's `output` answers, then
    /// what [`output`] answers. std's spawns under [`spawning`], as every one beside the helpers.
    macro_rules! both {
        ($program:expr) => {{
            let theirs = {
                let _spawning = spawning();
                std::process::Command::new($program).output()
            };
            (theirs, output(&mut Command::new($program)))
        }};
        ($program:expr, |$command:ident| $build:expr) => {{
            let mut $command = std::process::Command::new($program);
            $build;
            let theirs = {
                let _spawning = spawning();
                $command.output()
            };
            let mut $command = Command::new($program);
            $build;
            (theirs, output(&mut $command))
        }};
    }

    fn same(
        case: &str,
        (theirs, ours): (io::Result<Output>, io::Result<Output>),
    ) -> Option<Output> {
        match (theirs, ours) {
            (Ok(theirs), Ok(ours)) => {
                assert_eq!(
                    (&ours.status, &ours.stdout, &ours.stderr),
                    (&theirs.status, &theirs.stdout, &theirs.stderr),
                    "{case}: {:?}",
                    String::from_utf8_lossy(&ours.stderr)
                );
                Some(ours)
            }
            (Err(theirs), Err(ours)) => {
                assert_eq!(
                    (ours.kind(), ours.raw_os_error(), ours.to_string()),
                    (theirs.kind(), theirs.raw_os_error(), theirs.to_string()),
                    "{case}"
                );
                None
            }
            (theirs, ours) => panic!("{case}: std {theirs:?}, here {ours:?}"),
        }
    }

    /// The first number of a kernel release as `uname` writes it (`24.6.0` is 24).
    fn darwin_major(release: &str) -> Option<u32> {
        release.split('.').next()?.parse().ok()
    }

    /// This system's Darwin major version, `None` where it is not macOS.
    fn darwin() -> Option<u32> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        // SAFETY: utsname is plain data, for which all zeroes is a value.
        let mut name: libc::utsname = unsafe { std::mem::zeroed() };
        // SAFETY: uname writes NUL-terminated text into the struct it is handed.
        assert_eq!(unsafe { libc::uname(&raw mut name) }, 0);
        // SAFETY: uname NUL-terminated the release within its array.
        let release = unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) };
        darwin_major(release.to_str().unwrap())
    }

    /// What std answers for a name with a slash the kernel refused with `code`, started outside
    /// its fork path, on Darwin `darwin` (`None` for another system); `found` is whether a `stat`
    /// of the name answers. Its start is `posix_spawnp` (Libc's `sys/posix_spawn.c`). Since
    /// Libc-1725 (macOS 26, Darwin 25) that hands on the refusal of a name it did not search for.
    /// Up to Libc-1698 (macOS 15, Darwin 24) it runs such a name through its search's reading of
    /// a refusal: too much to start (E2BIG, ENOMEM, ETXTBSY) is the answer; a path not found
    /// (ENOENT, ENOTDIR, ELOOP, ENAMETOOLONG) ends as ENOENT; any other is the answer for a name
    /// `found` and ENOENT for one that is not. ENOEXEC is no answer of either, which run the name
    /// with `sh` and answer for that start: nothing is said of it here.
    fn spawnp_answers(code: libc::c_int, found: bool, darwin: Option<u32>) -> libc::c_int {
        assert_ne!(code, libc::ENOEXEC, "posix_spawnp answers for the shell");
        if darwin.is_none_or(|major| major >= 25) {
            return code;
        }
        match code {
            libc::E2BIG | libc::ENOMEM | libc::ETXTBSY => code,
            libc::ENOENT | libc::ENOTDIR | libc::ELOOP | libc::ENAMETOOLONG => libc::ENOENT,
            _ if found => code,
            _ => libc::ENOENT,
        }
    }

    /// Both starts failed, this one with the kernel's `code` and std's with what its
    /// `posix_spawnp` makes of that on Darwin `darwin` for a name a `stat` finds or not (`found`):
    /// the same error wherever that hands it on.
    fn refused_by_the_kernel(
        case: &str,
        (theirs, ours): (io::Result<Output>, io::Result<Output>),
        (code, found): (libc::c_int, bool),
        darwin: Option<u32>,
    ) {
        let said = |e: io::Error| (e.kind(), e.raw_os_error(), e.to_string());
        let ours = ours.expect_err(case);
        let theirs = theirs.expect_err(case);
        assert_eq!(
            said(ours),
            said(io::Error::from_raw_os_error(code)),
            "{case}"
        );
        assert_eq!(
            said(theirs),
            said(io::Error::from_raw_os_error(spawnp_answers(
                code, found, darwin
            ))),
            "{case}: std"
        );
    }

    #[test]
    fn output_answers_as_std_does_for_arguments_environment_directory_and_exit() {
        let tmp = Tmp::new("contract");
        let bin = tmp.0.join("bin with space");
        std::fs::create_dir_all(&bin).unwrap();
        let script = "printf '%s|' \"$PWD\" \"$KEY\" \"${GONE-unset}\" \"$1\" \"$2\"; \
                      printf 'said\\n' >&2; read -r line && printf read; exit 7";
        let said = same(
            "arguments, variables, directory, null stdin, exit",
            both!("/bin/sh", |c| c
                .args(["-c", script, "sh", "an argument: é\nsecond line", ""])
                .env("KEY", "value with spaces")
                .env_remove("GONE")
                .current_dir(&bin)),
        )
        .unwrap();
        assert_eq!(said.status.code(), Some(7));
        assert_eq!(
            String::from_utf8(said.stdout).unwrap(),
            format!(
                "{}|value with spaces|unset|an argument: é\nsecond line||",
                bin.display()
            )
        );
        assert_eq!(said.stderr, b"said\n");
        // The whole environment, inherited as it stands, changed, or cleared and set.
        same("inherited", both!("/usr/bin/env"));
        same(
            "changed",
            both!("/usr/bin/env", |c| c
                .env("SYMPHONY_SPAWN_SET", "1")
                .env_remove("HOME")),
        );
        same(
            "cleared",
            both!("/usr/bin/env", |c| c
                .env("SYMPHONY_SPAWN_EARLIER", "lost")
                .env_clear()
                .env("SYMPHONY_SPAWN_SET", "1")
                .env_remove("SYMPHONY_SPAWN_NEVER")),
        );
        // A bare name along the child's own PATH, relative entries from its directory, a cleared
        // one along the default; a name with a slash from the child's directory.
        tmp.script("bin with space/found", "printf 'found:%s' \"$1\"", 0o755);
        tmp.script("bin with space/denied", "exit 0", 0o644);
        same(
            "child's PATH",
            both!("found", |c| c
                .arg("x")
                .env("PATH", "/no/such:bin with space")
                .current_dir(&tmp.0)),
        )
        .unwrap();
        same("default PATH", both!("true", |c| c.env_clear()));
        same(
            "relative program",
            both!("./found", |c| c.arg("y").current_dir(&bin)),
        )
        .unwrap();
        same(
            "relative directory",
            both!("/bin/pwd", |c| c.current_dir(".")),
        );
        // A file with no #! line is run by /bin/sh, named as it was looked up.
        let plain = tmp.file(
            "bin with space/plain",
            "printf 'plain:%s|%s' \"$0\" \"$1\"",
            0o755,
        );
        let said = same("no #! line, by path", both!(&plain, |c| c.arg("x"))).unwrap();
        assert_eq!(
            said.stdout,
            format!("plain:{}|x", plain.display()).as_bytes()
        );
        let said = same(
            "no #! line, by name",
            both!("plain", |c| c
                .arg("y")
                .env_clear()
                .env("PATH", "bin with space")
                .current_dir(&tmp.0)),
        )
        .unwrap();
        assert_eq!(said.stdout, b"plain:bin with space/plain|y");
        // A PATH entry whose link loops is passed over, as one where the name is missing is.
        let looped = tmp.0.join("looped");
        std::fs::create_dir(&looped).unwrap();
        std::os::unix::fs::symlink("true", looped.join("true")).unwrap();
        let path = format!("{}:/usr/bin:/bin", looped.display());
        same(
            "looping PATH entry",
            both!("true", |c| c.env("PATH", &path)),
        )
        .unwrap();
    }

    #[test]
    fn a_child_that_cannot_start_fails_as_std_fails_it() {
        let tmp = Tmp::new("refused");
        let denied = tmp.script("denied", "exit 0", 0o644);
        let file = tmp.file("a file", "", 0o644);
        // A folder it may not search: no name is found there, none is refused.
        let shut = tmp.0.join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o600)).unwrap();
        for (case, result) in [
            ("missing name", both!("symphony-no-such-program")),
            (
                "missing name, child's PATH",
                both!("symphony-no-such-program", |c| c.env("PATH", &tmp.0)),
            ),
            ("missing path", both!("/no/such/symphony-program")),
            ("denied path", both!(&denied)),
            ("denied name", both!("denied", |c| c.env("PATH", &tmp.0))),
            (
                "name under a folder it may not search",
                both!("true", |c| c.env("PATH", &shut)),
            ),
            ("a directory", both!(&tmp.0)),
            (
                "missing directory",
                both!("/bin/sh", |c| c.current_dir(tmp.0.join("absent"))),
            ),
            // A directory it cannot enter is the answer on std's fork path; std's other path
            // searches and reads it as every name missing.
            (
                "directory a file, cleared",
                both!("true", |c| c.env_clear().current_dir(&file)),
            ),
            (
                "directory a file, inherited",
                both!("true", |c| c.current_dir(&file)),
            ),
            ("empty program", both!("")),
            ("NUL argument", both!("/bin/sh", |c| c.arg("nul\0argument"))),
            (
                "NUL variable",
                both!("/bin/sh", |c| c.env("KEY", "nul\0value")),
            ),
            (
                "NUL directory",
                both!("/bin/sh", |c| c.current_dir("nul\0dir")),
            ),
        ] {
            assert!(same(case, result).is_none(), "{case} started");
        }
        // A name with a slash the kernel refuses for its path: the kernel's error here, and
        // std's too wherever its `posix_spawnp` hands that on. The first two are not found
        // whatever a `stat` says; the last is refused under a folder where none can read it.
        for (case, result, refusal) in [
            (
                "path through a file",
                both!(file.join("sh")),
                (libc::ENOTDIR, false),
            ),
            (
                "directory a file",
                both!("/bin/sh", |c| c.current_dir(&file)),
                (libc::ENOTDIR, true),
            ),
            (
                "path under a folder it may not search",
                both!(shut.join("sh")),
                (libc::EACCES, false),
            ),
        ] {
            refused_by_the_kernel(case, result, refusal, darwin());
        }
    }

    /// What std's `posix_spawnp` makes of a refused name with a slash, by its Libc. A later one
    /// (macOS 26) or another system's hands every refusal on. One before it keeps a start too big
    /// to make, answers ENOENT for each path not found, and keeps any other refusal only for a
    /// name a `stat` finds. A release is read by its first number, and one that has none is no
    /// Darwin.
    #[test]
    fn std_answers_a_refused_path_by_its_libc() {
        let paths = [libc::ENOENT, libc::ENOTDIR, libc::ELOOP, libc::ENAMETOOLONG];
        let too_much = [libc::E2BIG, libc::ENOMEM, libc::ETXTBSY];
        let others = [libc::EACCES, libc::EPERM, libc::EIO, libc::EFAULT];
        for found in [false, true] {
            for code in paths.into_iter().chain(too_much).chain(others) {
                for since in [None, Some(25), Some(26)] {
                    assert_eq!(spawnp_answers(code, found, since), code);
                }
            }
            for before in [Some(0), Some(23), Some(24)] {
                for path in paths {
                    assert_eq!(spawnp_answers(path, found, before), libc::ENOENT);
                }
                for kept in too_much {
                    assert_eq!(spawnp_answers(kept, found, before), kept);
                }
                for other in others {
                    let answer = if found { other } else { libc::ENOENT };
                    assert_eq!(spawnp_answers(other, found, before), answer);
                }
            }
        }
        assert_eq!(darwin_major("24.6.0"), Some(24));
        assert_eq!(darwin_major("25.6.0"), Some(25));
        assert_eq!(darwin_major("25"), Some(25));
        for none in ["", ".1", "x.1", "-1.0", "24a.0"] {
            assert_eq!(darwin_major(none), None, "{none:?}");
        }
        // This system's: a Darwin's on macOS, where the answer differs, and none elsewhere.
        assert_eq!(darwin().is_some(), cfg!(target_os = "macos"));
    }

    /// `posix_spawnp` never answers ENOEXEC, on any Libc: it runs the name with `sh`, so the
    /// model of its answers refuses to say one.
    #[test]
    #[should_panic(expected = "posix_spawnp answers for the shell")]
    fn std_answers_no_refusal_for_a_file_sh_runs() {
        spawnp_answers(libc::ENOEXEC, true, Some(24));
    }

    /// A name exec refuses earlier along `PATH` (a file it may not run, a folder, a `#!` line whose
    /// interpreter is missing or may not run) is read and passed over, and a file with no `#!` line
    /// goes to `sh` at once: the one spawn is the start, as std's answers. With none to start, the
    /// refusal is std's and nothing is spawned. Once the child ends, nothing holds its input.
    ///
    /// The start runs under `/bin/bash`, one image: macOS's `/bin/sh` execs the shell it selects,
    /// and an exec there makes a new process and task and leaves the old task for the kernel to
    /// tear down. That first image keeps the child's input until then, which can outlast the
    /// shell's whole run and its reap under load.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_start_spawns_only_the_child_it_starts() {
        let tmp = Tmp::new("searched");
        let script = |dir: &str, body: &str, mode: u32| {
            std::fs::create_dir_all(tmp.0.join(dir)).unwrap();
            tmp.file(&format!("{dir}/prog"), body, mode)
        };
        script("found", "#!/bin/bash\nprintf found:%s \"$1\"\n", 0o755);
        script("denied", "#!/bin/sh\nexit 9\n", 0o644);
        std::fs::create_dir_all(tmp.0.join("folder/prog")).unwrap();
        script("lost", "#!/no/such/symphony-interpreter\nexit 9\n", 0o755);
        let barred = tmp.file("barred-sh", "#!/bin/sh\nexit 9\n", 0o644);
        script(
            "barred",
            &format!("#!{} -e\nexit 9\n", barred.display()),
            0o755,
        );
        let plain = script("plain", "printf plain:%s \"$1\"", 0o755);
        let empty = script("empty", "", 0o755);
        // What std answers, under the lock as every std spawn here, then what the helper does and
        // how many spawns it made.
        let compare = |case: &str, program: &Path, path: &str| {
            let theirs = std::process::Command::new(program)
                .arg("x")
                .env("PATH", path)
                .current_dir(&tmp.0)
                .output_locked();
            let mut command = Command::new(program);
            command.arg("x").env("PATH", path).current_dir(&tmp.0);
            let (ours, spawns) = counted(|| output(&mut command));
            (same(case, (theirs, ours)), spawns)
        };
        let prog = Path::new("prog");
        for refused in ["denied", "folder", "lost", "barred"] {
            let path = format!("{refused}:found");
            let (done, spawns) = compare(refused, prog, &path);
            assert_eq!(done.unwrap().stdout, b"found:x", "{refused}");
            assert_eq!(spawns, 1, "{refused} before the start was spawned");
            let (none, spawns) = compare(refused, prog, refused);
            assert!(none.is_none(), "{refused} alone started");
            assert_eq!(spawns, 0, "{refused} alone was spawned");
            // Nothing but the ended child held its input's reader.
            let mut child =
                spawn_piped(Command::new("prog").env("PATH", &path).current_dir(&tmp.0)).unwrap();
            let mut stdin = child.stdin.take().unwrap();
            assert!(child.wait().unwrap().success(), "{refused}");
            let refused_write = stdin.write_all(b"unread").unwrap_err();
            assert_eq!(refused_write.kind(), io::ErrorKind::BrokenPipe, "{refused}");
        }
        for (case, program, said) in [
            ("no #! line, by name", prog, "plain:x"),
            ("no #! line, by path", plain.as_path(), "plain:x"),
            ("empty, by path", empty.as_path(), ""),
        ] {
            let (done, spawns) = compare(case, program, "plain");
            assert_eq!(done.unwrap().stdout, said.as_bytes(), "{case}");
            assert_eq!(spawns, 1, "{case}: the file was spawned before sh");
        }
    }

    /// std's spawn, status and output with the lock held for the spawn answer as std's own do: the
    /// arguments, environment and directory, a null input read to its end, both outputs, the exit,
    /// and the error of a program that is not there.
    #[test]
    fn std_spawns_under_the_lock_answer_as_std_does() {
        let tmp = Tmp::new("locked");
        let build = || {
            let mut command = std::process::Command::new("/bin/sh");
            command
                .args([
                    "-c",
                    "printf '%s|%s|%s' \"$PWD\" \"$KEY\" \"$1\"; cat; printf err >&2; exit 3",
                    "sh",
                    "an argument",
                ])
                .env("KEY", "value")
                .current_dir(&tmp.0);
            command
        };
        let theirs = {
            let _spawning = spawning();
            build().output().unwrap()
        };
        let ours = build().output_locked().unwrap();
        assert_eq!(
            (&ours.status, &ours.stdout, &ours.stderr),
            (&theirs.status, &theirs.stdout, &theirs.stderr)
        );
        assert_eq!(ours.status.code(), Some(3));
        assert_eq!(
            ours.stdout,
            format!("{}|value|an argument", tmp.0.display()).as_bytes()
        );
        let status = |script: &str| {
            std::process::Command::new("/bin/sh")
                .args(["-c", script])
                .stdout(std::process::Stdio::null())
                .status_locked()
                .unwrap()
        };
        assert_eq!(status("exit 4").code(), Some(4));
        assert_eq!(status("kill -TERM $$").signal(), Some(libc::SIGTERM));
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 5"])
            .spawn_locked()
            .unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(5));
        let missing = || std::process::Command::new("/no/such/symphony-locked-program");
        let theirs = {
            let _spawning = spawning();
            missing().output().unwrap_err()
        };
        for ours in [
            missing().output_locked().unwrap_err(),
            missing().status_locked().unwrap_err(),
            missing().spawn_locked().unwrap_err(),
        ] {
            assert_eq!(
                (ours.kind(), ours.raw_os_error()),
                (theirs.kind(), theirs.raw_os_error())
            );
        }
    }

    #[test]
    fn a_child_keeps_only_its_streams_and_the_descriptors_it_is_passed() {
        let tmp = Tmp::new("descriptors");
        let passed = File::create(tmp.0.join("passed")).unwrap();
        let private = File::create(tmp.0.join("private")).unwrap();
        // Another thread's descriptor not yet close-on-exec reaches no child either.
        // SAFETY: this test owns the file; only its descriptor flags change.
        assert_eq!(
            unsafe { libc::fcntl(private.as_raw_fd(), libc::F_SETFD, 0) },
            0
        );
        let script = format!(
            "test -e /dev/fd/0 && test -e /dev/fd/{} && test ! -e /dev/fd/{} && printf kept",
            passed.as_raw_fd(),
            private.as_raw_fd()
        );
        let (mut reader, writer) = pipe().unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", &script])
            .pass_fd(passed.as_raw_fd())
            .stdout(writer)
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        let mut said = String::new();
        reader.read_to_string(&mut said).unwrap();
        assert_eq!(said, "kept", "the command keeps no writer after the spawn");
        // SAFETY: flag queries on two files this test still owns.
        let flags = |fd: &File| unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(flags(&passed) & libc::FD_CLOEXEC, libc::FD_CLOEXEC);
        assert_eq!(flags(&private), 0);
        let closed = Command::new("/bin/sh").pass_fd(-1).spawn().unwrap_err();
        assert_eq!(closed.raw_os_error(), Some(libc::EBADF));
    }

    #[test]
    fn a_child_leads_its_own_group_or_session_when_asked_and_shares_ours_otherwise() {
        // SAFETY: getpgid(0) and getsid(0) ask about this process only.
        let (group, session) = unsafe { (libc::getpgid(0), libc::getsid(0)) };
        for case in ["plain", "group", "session"] {
            let mut command = Command::new("sleep");
            match case {
                "group" => command.process_group(0),
                "session" => command.new_session(),
                _ => &mut command,
            };
            let (own_group, own_session) = (case != "plain", case == "session");
            // It lives until killed: no run of the test outlasts its day.
            let mut child = command.arg("86400").spawn().unwrap();
            let pid = libc::pid_t::try_from(child.id()).unwrap();
            // SAFETY: getpgid and getsid read the group and session of this test's own child.
            let (its_group, its_session) = unsafe { (libc::getpgid(pid), libc::getsid(pid)) };
            child.kill().unwrap();
            assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
            assert_eq!(its_group, if own_group { pid } else { group }, "{case}");
            assert_eq!(
                its_session,
                if own_session { pid } else { session },
                "{case}"
            );
        }
    }

    #[test]
    fn a_child_is_waited_for_once_and_its_sigpipe_is_the_default() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "kill -PIPE $$; exit 9"])
            .spawn()
            .unwrap();
        let status = child.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(libc::SIGPIPE),
            "std's default SIGPIPE"
        );
        assert_eq!(child.try_wait().unwrap(), Some(status));
        assert_eq!(child.wait().unwrap(), status, "the end is kept");
        child.kill().unwrap();
        // wait closes the child's input first, as std's does: cat then reads its end.
        let mut cat = spawn_piped(&mut Command::new("cat")).unwrap();
        assert!(cat.wait().unwrap().success());
    }

    /// A child asked to have a signal at its default starts so though this process ignores it, as
    /// a background job inherits SIGINT and SIGQUIT; one not asked for stays ignored. The test runs
    /// again in a process of its own that ignores both, so the runner's dispositions stay as found.
    #[test]
    fn a_child_starts_with_a_signal_at_its_default_when_asked_and_inherits_every_other() {
        const CASE: &str = "PROCESS_CWD_IGNORING_TEST";
        const TEST: &str = "spawn::tests::a_child_starts_with_a_signal_at_its_default_when_asked_and_inherits_every_other";
        let signals = [(libc::SIGINT, "INT"), (libc::SIGQUIT, "QUIT")];
        if std::env::var_os(CASE).is_none() {
            use std::os::unix::process::CommandExt as _;
            let mut ignoring = std::process::Command::new(std::env::current_exe().unwrap());
            ignoring.args(["--exact", TEST]).env(CASE, "1");
            // SAFETY: signal is async-signal-safe and changes only the forked child's dispositions.
            unsafe {
                ignoring.pre_exec(move || {
                    for (signal, _) in signals {
                        if libc::signal(signal, libc::SIG_IGN) == libc::SIG_ERR {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
            let done = ignoring.output_locked().unwrap();
            assert!(done.status.success(), "{done:?}");
            assert!(
                String::from_utf8_lossy(&done.stdout).contains("1 passed"),
                "{done:?}"
            );
            return;
        }
        let run = |script: &str, defaults: &[libc::c_int]| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            for &signal in defaults {
                command.default_signal(signal);
            }
            output(&mut command).unwrap().status
        };
        for (signal, name) in signals {
            let script = format!("kill -{name} $$; exit 7");
            assert_eq!(run(&script, &[]).code(), Some(7), "{name} stays ignored");
            assert_eq!(run(&script, &[signal]).signal(), Some(signal), "{name}");
        }
        // One asked for leaves the other as this process has it.
        let both = "kill -QUIT $$; kill -INT $$; exit 7";
        assert_eq!(run(both, &[libc::SIGINT]).signal(), Some(libc::SIGINT));
        assert_eq!(run(both, &[libc::SIGQUIT]).signal(), Some(libc::SIGQUIT));
        // A number no signal has, or one no process may catch or ignore, fails the spawn.
        for signal in [0, -1, 1000, libc::SIGKILL, libc::SIGSTOP] {
            let refused = Command::new("/bin/sh")
                .default_signal(signal)
                .spawn()
                .unwrap_err();
            assert_eq!(
                refused.raw_os_error(),
                Some(libc::EINVAL),
                "{signal}: {refused}"
            );
        }
        // As on std's fork path, a directory it cannot enter is the answer.
        let tmp = Tmp::new("defaults");
        let file = tmp.file("a file", "", 0o644);
        let entered = Command::new("true")
            .current_dir(&file)
            .default_signal(libc::SIGINT)
            .spawn()
            .unwrap_err();
        assert_eq!(entered.raw_os_error(), Some(libc::ENOTDIR), "{entered}");
    }

    /// A child asked for every default starts with each signal it may catch at its default and none
    /// blocked, though this process ignores INT, QUIT and HUP (a background job under `nohup`) and
    /// blocks USR2; without it, the child inherits them all. The test runs again in a process of its
    /// own that ignores and blocks them, so the runner's dispositions and mask stay as found.
    #[test]
    fn a_child_asked_for_every_default_starts_with_none_ignored_or_blocked() {
        const CASE: &str = "PROCESS_CWD_IGNORING_EVERY_TEST";
        const TEST: &str =
            "spawn::tests::a_child_asked_for_every_default_starts_with_none_ignored_or_blocked";
        let ignored = [
            (libc::SIGINT, "INT"),
            (libc::SIGQUIT, "QUIT"),
            (libc::SIGHUP, "HUP"),
        ];
        if std::env::var_os(CASE).is_none() {
            use std::os::unix::process::CommandExt as _;
            let mut ignoring = std::process::Command::new(std::env::current_exe().unwrap());
            ignoring.args(["--exact", TEST]).env(CASE, "1");
            // SAFETY: signal, sigemptyset, sigaddset and sigprocmask are async-signal-safe and
            // change only the forked child's dispositions and mask.
            unsafe {
                ignoring.pre_exec(move || {
                    for (signal, _) in ignored {
                        if libc::signal(signal, libc::SIG_IGN) == libc::SIG_ERR {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    let mut usr2: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&raw mut usr2);
                    libc::sigaddset(&raw mut usr2, libc::SIGUSR2);
                    if libc::sigprocmask(libc::SIG_BLOCK, &raw const usr2, std::ptr::null_mut())
                        == -1
                    {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let done = ignoring.output_locked().unwrap();
            assert!(done.status.success(), "{done:?}");
            assert!(
                String::from_utf8_lossy(&done.stdout).contains("1 passed"),
                "{done:?}"
            );
            return;
        }
        // Every signal a process may catch, and neither KILL nor STOP.
        let every = Command::new("/bin/sh").default_signals().defaults.clone();
        for signal in 1..32 {
            let catchable = signal != libc::SIGKILL && signal != libc::SIGSTOP;
            assert_eq!(every.contains(&signal), catchable, "{signal}: {every:?}");
        }
        fn inherits(command: &mut Command) -> &mut Command {
            command
        }
        fn defaults(command: &mut Command) -> &mut Command {
            command.default_signals()
        }
        fn usr2(command: &mut Command) -> &mut Command {
            command.default_signal(libc::SIGUSR2)
        }
        let run = |script: &str, build: fn(&mut Command) -> &mut Command| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            build(&mut command);
            output(&mut command).unwrap().status
        };
        for (signal, name) in ignored.into_iter().chain([(libc::SIGUSR2, "USR2")]) {
            let script = format!("kill -{name} $$; exit 7");
            assert_eq!(
                run(&script, inherits).code(),
                Some(7),
                "{name} stays as found"
            );
            assert_eq!(run(&script, defaults).signal(), Some(signal), "{name}");
        }
        // One signal's default leaves the mask as found: USR2 stays blocked.
        assert_eq!(run("kill -USR2 $$; exit 7", usr2).code(), Some(7));
        // As on std's fork path, a directory it cannot enter is the answer.
        let tmp = Tmp::new("every-default");
        let file = tmp.file("a file", "", 0o644);
        let entered = Command::new("true")
            .current_dir(&file)
            .default_signals()
            .spawn()
            .unwrap_err();
        assert_eq!(entered.raw_os_error(), Some(libc::ENOTDIR), "{entered}");
    }

    #[test]
    fn a_pipe_carries_what_is_written_and_ends_when_its_writer_does() {
        let (mut reader, mut writer) = pipe().unwrap();
        writer.write_all(b"answer").unwrap();
        drop(writer);
        let mut said = String::new();
        reader.read_to_string(&mut said).unwrap();
        assert_eq!(said, "answer");
    }

    /// A pipe's end reaches a child through [`Command`], which hands it a copy; on macOS, where no
    /// spawn copies the end itself, a std spawn handed one cannot start.
    #[test]
    fn a_pipes_end_reaches_a_child_through_the_helpers_spawn_only() {
        let (mut reader, writer) = pipe().unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "printf handed >&2"])
            .stderr(writer)
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        let mut said = String::new();
        reader.read_to_string(&mut said).unwrap();
        assert_eq!(said, "handed");
        #[cfg(target_vendor = "apple")]
        {
            let (_reader, writer) = pipe().unwrap();
            let _spawning = spawning();
            let refused = std::process::Command::new("/bin/sh")
                .args(["-c", "printf handed >&2"])
                .stderr(writer)
                .spawn()
                .unwrap_err();
            assert_eq!(refused.raw_os_error(), Some(libc::EBADF), "{refused}");
        }
    }

    #[test]
    fn a_pipes_ends_are_close_on_exec_and_its_reader_blocks_until_told_otherwise() {
        // SAFETY: F_GETFD and F_GETFL only read the flags of a descriptor the borrow keeps open.
        let flags = |fd: &dyn AsRawFd, get| unsafe { libc::fcntl(fd.as_raw_fd(), get) };
        let (mut reader, mut writer) = pipe().unwrap();
        for end in [&reader as &dyn AsRawFd, &writer] {
            assert_eq!(
                flags(end, libc::F_GETFD) & libc::FD_CLOEXEC,
                libc::FD_CLOEXEC
            );
        }
        assert_eq!(flags(&reader, libc::F_GETFL) & libc::O_NONBLOCK, 0);
        set_nonblocking(&reader, true).unwrap();
        let mut byte = [0];
        let unwritten = reader.read(&mut byte).unwrap_err();
        assert_eq!(unwritten.kind(), io::ErrorKind::WouldBlock);
        writer.write_all(b"a").unwrap();
        assert_eq!(reader.read(&mut byte).unwrap(), 1);
        set_nonblocking(&reader, false).unwrap();
        assert_eq!(flags(&reader, libc::F_GETFL) & libc::O_NONBLOCK, 0);
    }

    /// Whether a child std forks holds `fd` before its exec, as its own descriptor table says.
    #[cfg(target_vendor = "apple")]
    fn found(fd: RawFd) -> bool {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/usr/bin/true");
        // SAFETY: fcntl is async-signal-safe and reads only the forked child's own table.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_GETFD) == -1 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(libc::EEXIST))
                }
            });
        }
        let _spawning = spawning();
        match command.status() {
            Ok(status) => {
                assert!(status.success(), "{status}");
                false
            }
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => true,
            Err(e) => panic!("{e}"),
        }
    }

    /// A child std forks holds each descriptor not close-on-fork until its exec, close-on-exec ones
    /// too, as std's pipe's ends; a [`pipe`]'s ends, the helpers' children's streams among them,
    /// never reach it.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_pipes_ends_never_reach_a_forked_child_even_before_its_exec() {
        let (reader, writer) = pipe().unwrap();
        let child = spawn_piped(&mut Command::new("cat")).unwrap();
        for (end, fd) in [
            ("reader", reader.as_raw_fd()),
            ("writer", writer.as_raw_fd()),
            ("child's stdin", child.stdin.as_ref().unwrap().as_raw_fd()),
            ("child's stdout", child.stdout.as_ref().unwrap().as_raw_fd()),
            ("child's stderr", child.stderr.as_ref().unwrap().as_raw_fd()),
        ] {
            assert!(!found(fd), "the {end} reached a forked child");
        }
        let (reader, writer) = io::pipe().unwrap();
        for end in [reader.as_raw_fd(), writer.as_raw_fd()] {
            assert!(found(end), "std's pipe's end reaches a forked child");
        }
        assert!(wait_output(child).unwrap().status.success());
    }

    /// What libproc says each of process `pid`'s descriptors is (`sys/proc_info.h`): a real pipe
    /// is a pipe, a FIFO a vnode.
    #[cfg(target_vendor = "apple")]
    fn kinds(pid: libc::pid_t) -> BTreeMap<RawFd, libc::c_int> {
        let mut table = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0,
            };
            4096
        ];
        let size = libc::c_int::try_from(std::mem::size_of_val(table.as_slice())).unwrap();
        // SAFETY: libproc writes at most `size` bytes of whole entries into the table.
        let used = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                table.as_mut_ptr().cast(),
                size,
            )
        };
        assert!(used > 0, "{}", io::Error::last_os_error());
        table.truncate(usize::try_from(used).unwrap() / size_of::<libc::proc_fdinfo>());
        table
            .into_iter()
            .map(|entry| (entry.proc_fd, entry.proc_fdtype.try_into().unwrap()))
            .collect()
    }

    /// macOS can lose a FIFO's end for a read already blocked on it, so a [`pipe`], and what the
    /// helpers read their children through and feed them through, is a real pipe, not a vnode.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn pipes_and_the_helpers_streams_are_real_pipes() {
        let child = spawn_piped(&mut Command::new("cat")).unwrap();
        let (reader, writer) = pipe().unwrap();
        let ends = [
            ("stdin", child.stdin.as_ref().unwrap().as_raw_fd()),
            ("stdout", child.stdout.as_ref().unwrap().as_raw_fd()),
            ("stderr", child.stderr.as_ref().unwrap().as_raw_fd()),
            ("reader", reader.as_raw_fd()),
            ("writer", writer.as_raw_fd()),
        ];
        let kinds = kinds(std::process::id().try_into().unwrap());
        let found = ends.map(|(end, fd)| (end, kinds.get(&fd).copied()));
        let pipe = Some(libc::PROX_FDTYPE_PIPE);
        assert_eq!(
            found,
            [
                ("stdin", pipe),
                ("stdout", pipe),
                ("stderr", pipe),
                ("reader", pipe),
                ("writer", pipe),
            ]
        );
        assert!(wait_output(child).unwrap().status.success());
    }

    /// The sequence that hung on a FIFO: the read already blocked when its child answers and ends,
    /// beside other threads doing the same. Each read returns, with the whole answer.
    #[test]
    fn a_read_already_blocked_when_its_child_answers_and_ends_returns_the_whole_answer() {
        std::thread::scope(|scope| {
            for thread in 0..6 {
                scope.spawn(move || {
                    for call in 0..8 {
                        // The child answers after the read has begun.
                        let mut command = Command::new("/bin/sh");
                        command.args(["-c", "sleep 0.05; printf %0333d 0; printf refused >&2"]);
                        let done = output(&mut command).unwrap();
                        assert_eq!(
                            (done.stdout, done.stderr, done.status.code()),
                            (vec![b'0'; 333], b"refused".to_vec(), Some(0)),
                            "thread {thread}, call {call}"
                        );
                    }
                });
            }
        });
    }

    /// A board socket is close-on-fork before its connect, its first descriptor never copied, and
    /// after it: no forked child holds it, while std's socket, as the control, reaches one. Once
    /// connected it is the connection asked for, both ways, close-on-exec and without SIGPIPE.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_connected_socket_never_reaches_a_forked_child_before_or_after_its_connect() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let to = listener.local_addr().unwrap();
        let control = TcpStream::connect(to).unwrap();
        assert!(
            found(control.as_raw_fd()),
            "std's socket reaches a forked child"
        );
        let unconnected = own_socket(&to).unwrap();
        assert!(
            !found(unconnected.as_raw_fd()),
            "the socket reached a child before its connect"
        );
        let mut socket = connect(&to, test_time::backstop(Duration::from_secs(5))).unwrap();
        assert!(
            !found(socket.as_raw_fd()),
            "the connected socket reached a forked child"
        );
        assert_eq!(socket.peer_addr().unwrap(), to);
        // SAFETY: F_GETFD reads the flags of a descriptor the stream keeps open.
        let flags = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, libc::FD_CLOEXEC);
        let mut on: libc::c_int = 0;
        let mut length = libc::socklen_t::try_from(size_of_val(&on)).unwrap();
        // SAFETY: on is writable for length bytes and the stream owns the descriptor.
        let got = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&raw mut on).cast(),
                &raw mut length,
            )
        };
        assert_eq!((got, on), (0, 1), "SO_NOSIGPIPE, as std sets it");
        let (_control_peer, _) = listener.accept().unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        assert_eq!(peer.peer_addr().unwrap(), socket.local_addr().unwrap());
        socket.write_all(b"a").unwrap();
        let mut byte = [0];
        peer.read_exact(&mut byte).unwrap();
        peer.write_all(b"b").unwrap();
        socket.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"b");
        let six = TcpListener::bind("[::1]:0").unwrap();
        let socket = connect(
            &six.local_addr().unwrap(),
            test_time::backstop(Duration::from_secs(5)),
        )
        .unwrap();
        assert!(!found(socket.as_raw_fd()));
        assert_eq!(socket.peer_addr().unwrap(), six.local_addr().unwrap());
    }

    /// A board socket's first descriptor is close-on-exec as soon as it exists, as std's are, so a
    /// std spawn that skips the lock drops its copy at its exec: a stream socket of either family.
    /// A family no socket has is refused.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_new_socket_is_close_on_exec_from_its_first_descriptor() {
        for family in [libc::AF_INET, libc::AF_INET6] {
            let made = new_socket(family).unwrap();
            // SAFETY: F_GETFD reads the flags of a descriptor made keeps open.
            let flags = unsafe { libc::fcntl(made.as_raw_fd(), libc::F_GETFD) };
            assert_eq!(flags & libc::FD_CLOEXEC, libc::FD_CLOEXEC, "{family}");
            let kinds = kinds(std::process::id().try_into().unwrap());
            assert_eq!(
                kinds.get(&made.as_raw_fd()),
                Some(&libc::PROX_FDTYPE_SOCKET),
                "{family}"
            );
            let mut kind: libc::c_int = 0;
            let mut length = libc::socklen_t::try_from(size_of_val(&kind)).unwrap();
            // SAFETY: kind is writable for length bytes and made owns the descriptor.
            let got = unsafe {
                libc::getsockopt(
                    made.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_TYPE,
                    (&raw mut kind).cast(),
                    &raw mut length,
                )
            };
            assert_eq!((got, kind), (0, libc::SOCK_STREAM), "{family}");
        }
        let refused = new_socket(-1).unwrap_err();
        assert_eq!(
            refused.raw_os_error(),
            Some(libc::EAFNOSUPPORT),
            "{refused}"
        );
    }

    /// connect fails as std's does: a port nobody listens on, an address no socket can reach, and
    /// a timeout of nothing.
    #[test]
    fn connect_refuses_as_std_does() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let to = free.local_addr().unwrap();
        drop(free);
        let wait = Duration::from_secs(5);
        assert_eq!(
            connect(&to, wait).unwrap_err().kind(),
            io::ErrorKind::ConnectionRefused
        );
        let nowhere: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(
            connect(&nowhere, wait).unwrap_err().raw_os_error(),
            TcpStream::connect_timeout(&nowhere, wait)
                .unwrap_err()
                .raw_os_error()
        );
        assert_eq!(
            connect(&to, Duration::ZERO).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn output_is_what_the_command_printed_and_how_it_ended_with_nothing_to_read() {
        // A Command kept past the call: had it kept the writers, no EOF would ever come.
        let mut command = Command::new("sh");
        command.args(["-c", "printf out; printf err >&2; exit 3"]);
        let done = output(&mut command).unwrap();
        assert_eq!(
            (done.stdout, done.stderr, done.status.code()),
            (b"out".to_vec(), b"err".to_vec(), Some(3))
        );
        assert_eq!(output(&mut command).unwrap().status.code(), Some(3));
        assert_eq!(output(&mut Command::new("cat")).unwrap().stdout, b"");
        assert!(output(&mut Command::new("/no/such/symphony-output-command")).is_err());
    }

    #[test]
    fn a_piped_stream_alone_or_with_others_is_read_and_written_through_its_childs_pipe() {
        fn read(end: Option<impl Read>) -> Option<String> {
            end.map(|mut end| {
                let mut said = String::new();
                end.read_to_string(&mut said).unwrap();
                said
            })
        }
        let each = [true, false, false];
        for piped in [each, [false, true, false], [false, false, true], [true; 3]] {
            // What reaches stdin is copied to stderr after stdout says its word.
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "printf out; cat >&2"]);
            let [stdin, stdout, stderr] =
                piped.map(|piped| if piped { Stdio::piped() } else { Stdio::null() });
            let mut child = command
                .stdin(stdin)
                .stdout(stdout)
                .stderr(stderr)
                .spawn()
                .unwrap();
            let ends = [
                child.stdin.is_some(),
                child.stdout.is_some(),
                child.stderr.is_some(),
            ];
            assert_eq!(ends, piped, "only a piped stream has an end here");
            if let Some(mut input) = child.stdin.take() {
                input.write_all(b"in").unwrap();
            }
            // The command is still here: had it kept a writer, neither read would end.
            let said = (read(child.stdout.take()), read(child.stderr.take()));
            assert!(child.wait().unwrap().success());
            let input = if piped[0] { "in" } else { "" };
            let expected = (
                piped[1].then(|| "out".into()),
                piped[2].then(|| input.into()),
            );
            assert_eq!(said, expected, "{piped:?}");
        }
    }

    #[test]
    fn piped_input_reaches_the_child_and_ends_when_its_writer_goes() {
        let mut cat = Command::new("cat");
        let mut child = spawn_piped(&mut cat).unwrap();
        child.stdin.take().unwrap().write_all(b"answer").unwrap();
        let done = wait_output(child).unwrap();
        assert_eq!(
            (done.stdout, done.status.code()),
            (b"answer".to_vec(), Some(0))
        );
        // An input never taken is closed by the wait, as std's wait_with_output closes it.
        let done = wait_output(spawn_piped(&mut cat).unwrap()).unwrap();
        assert_eq!((done.stdout, done.status.code()), (Vec::new(), Some(0)));
        assert!(spawn_piped(&mut Command::new("/no/such/symphony-output-command")).is_err());
    }

    #[test]
    fn input_to_a_piped_child_that_has_ended_fails_though_its_command_is_kept() {
        // Had the kept Command held the input's reader, the write would go into its buffer.
        let mut command = Command::new("true");
        let mut child = spawn_piped(&mut command).unwrap();
        let mut stdin = child.stdin.take().unwrap();
        assert!(child.wait().unwrap().success());
        let refused = stdin.write_all(b"unread").unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::BrokenPipe);
    }

    /// node polls its stdin and never sees a FIFO's end on macOS: one reading its input to the end
    /// finishes once the writer closes.
    #[test]
    fn a_node_child_reading_its_input_to_the_end_finishes_when_the_writer_closes() {
        let script = "let n = 0; process.stdin.on('data', c => n += c.length)\
                      .on('end', () => console.log(n))";
        let mut child = spawn_piped(Command::new("node").args(["-e", script])).unwrap();
        child.stdin.take().unwrap().write_all(b"answer").unwrap();
        let done = wait_output(child).unwrap();
        assert_eq!(
            (done.stdout, done.status.code()),
            (b"6\n".to_vec(), Some(0)),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    /// Children outliving every answer, each living until it is killed (a day), spawned through std
    /// beside it as another thread's long child would be, each under [`spawning`] as every std spawn beside the helpers, and keeping
    /// whatever else it could copy as it started.
    fn racing(spawned: &AtomicBool) -> Children {
        let mut children = Children(Vec::new());
        for _ in 0..32 {
            let child = std::process::Command::new("sleep")
                .arg("86400")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn_locked()
                .unwrap();
            children.0.push(child);
        }
        spawned.store(true, Ordering::Relaxed);
        children
    }

    #[test]
    fn a_spawn_racing_a_pipes_making_never_holds_the_answer_open() {
        use std::time::{Duration, Instant};
        let spawned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let other = scope.spawn(|| racing(&spawned));
            let mut answers = 0;
            while !spawned.load(Ordering::Relaxed) {
                let (mut reader, writer) = pipe().unwrap();
                set_nonblocking(&reader, true).unwrap();
                drop(writer);
                // No child holds the writer: no spawn or fork copies a pipe's ends, and a racer
                // spawned under the lock never sees its first ones. Its EOF comes at once; only a
                // holder, which lives a day, keeps it past the backstop.
                let made = Instant::now();
                loop {
                    match reader.read(&mut [0]) {
                        Ok(0) => break,
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                && made.elapsed() < test_time::backstop(Duration::from_secs(5)) =>
                        {
                            #[expect(
                                clippy::disallowed_methods,
                                reason = "no-timed-waits baseline: a test looks again on a nonblocking pipe by hand, where test_time::wait::until is the helper"
                            )]
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        other => panic!(
                            "answer {answers} is held open: {other:?}; {}",
                            test_time::Load::now()
                        ),
                    }
                }
                answers += 1;
            }
            drop(other.join().unwrap());
            assert!(answers > 0, "no answer was made beside the spawns");
        });
    }

    /// A std child keeps each descriptor not close-on-exec at its spawn for its life, and a
    /// [`pipe`] is one until it is reopened: a racer spawned under [`spawning`], as [`racing`]'s, keeps
    /// no pipe of the answers made beside it but what this process hands every child, so each
    /// answer ends with its own child while every racer still lives.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_racer_spawned_under_the_lock_keeps_no_pipe_of_an_answer_made_beside_it() {
        let pid = |id: u32| libc::pid_t::try_from(id).unwrap();
        // SAFETY: F_GETFD only reads a descriptor's flags; a closed one answers -1, close-on-exec.
        let inheritable =
            |fd: RawFd| unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC == 0;
        let handed: Vec<RawFd> = kinds(pid(std::process::id()))
            .into_iter()
            .filter(|&(fd, kind)| kind == libc::PROX_FDTYPE_PIPE && inheritable(fd))
            .map(|(fd, _)| fd)
            .collect();
        let spawned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let readers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let mut answers = 0;
                        while !spawned.load(Ordering::Relaxed) {
                            let done = output(Command::new("/bin/echo").arg("answer")).unwrap();
                            assert_eq!(done.stdout, b"answer\n");
                            answers += 1;
                        }
                        answers
                    })
                })
                .collect();
            let mut racers = racing(&spawned);
            let answers: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
            for racer in &mut racers.0 {
                let kept: Vec<RawFd> = kinds(pid(racer.id()))
                    .into_iter()
                    .filter(|&(fd, kind)| kind == libc::PROX_FDTYPE_PIPE && !handed.contains(&fd))
                    .map(|(fd, _)| fd)
                    .collect();
                assert_eq!(
                    kept,
                    Vec::<RawFd>::new(),
                    "racer {} kept a pipe",
                    racer.id()
                );
                assert!(racer.try_wait().unwrap().is_none(), "a racer ended first");
            }
            assert!(answers > 0, "no answer was made beside the racers");
        });
    }

    #[test]
    fn a_wait_ends_when_its_child_does_while_other_threads_spawn_long_children() {
        let spawned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let other = scope.spawn(|| racing(&spawned));
            let mut answers = 0;
            while !spawned.load(Ordering::Relaxed) {
                // A cleared environment and a bare name: what std starts through its fork path.
                let done = output(Command::new("true").env_clear()).unwrap();
                assert!(done.status.success());
                let mut child = spawn_piped(Command::new("cat").env_clear()).unwrap();
                child.stdin.take().unwrap().write_all(b"echo").unwrap();
                assert_eq!(wait_output(child).unwrap().stdout, b"echo");
                // A bare name along a PATH that misses first: the search's own spawns. Once the
                // child has ended nothing holds its input's reader or its output's writer.
                let mut child = spawn_piped(
                    Command::new("true").env("PATH", "/no/such/symphony-spawn:/usr/bin"),
                )
                .unwrap();
                let (mut stdin, mut stdout) =
                    (child.stdin.take().unwrap(), child.stdout.take().unwrap());
                assert!(child.wait().unwrap().success());
                let refused = stdin.write_all(b"unread").unwrap_err();
                assert_eq!(
                    refused.kind(),
                    io::ErrorKind::BrokenPipe,
                    "answer {answers}"
                );
                set_nonblocking(&stdout, true).unwrap();
                assert_eq!(stdout.read(&mut [0]).unwrap(), 0, "answer {answers}");
                answers += 1;
            }
            let mut children = other.join().unwrap();
            // A wait some racer held would have ended only with that racer, a day on.
            for racer in &mut children.0 {
                assert!(racer.try_wait().unwrap().is_none(), "a racer ended first");
            }
            assert!(answers > 0, "no answer was waited for beside the spawns");
        });
    }

    /// A fork child keeps every descriptor not close-on-fork until its exec, and no wait can
    /// shorten that: what keeps the helpers' waits short is that they never fork.
    #[cfg(target_os = "macos")]
    #[test]
    fn no_helper_forks_where_std_does() {
        use std::cell::Cell;
        thread_local! {
            /// The forks this thread made, counted by the handler `fork` runs before it forks.
            static FORKS: Cell<u32> = const { Cell::new(0) };
        }
        extern "C" fn prepare() {
            FORKS.set(FORKS.get() + 1);
        }
        static HANDLER: std::sync::Once = std::sync::Once::new();
        HANDLER.call_once(|| {
            // SAFETY: the handler only bumps the forking thread's count; it is registered once.
            let registered = unsafe { libc::pthread_atfork(Some(prepare), None, None) };
            assert_eq!(registered, 0);
        });
        let tmp = Tmp::new("forkless");
        tmp.script("found", "exit 0", 0o755);
        let passed = File::open("/dev/null").unwrap();
        // Each shape std starts through its fork path.
        for case in [
            "cleared",
            "PATH",
            "session",
            "group",
            "relative",
            "signal",
            "signals",
            "descriptor",
        ] {
            let mut command = Command::new(match case {
                "PATH" => "found",
                "relative" => "./found",
                _ => "true",
            });
            match case {
                "cleared" => command.env_clear(),
                "PATH" => command.env("PATH", &tmp.0),
                "session" => command.new_session(),
                "group" => command.process_group(0),
                "relative" => command.current_dir(&tmp.0),
                "signal" => command.default_signal(libc::SIGINT),
                "signals" => command.default_signals(),
                _ => command.pass_fd(passed.as_raw_fd()),
            };
            assert!(output(&mut command).unwrap().status.success(), "{case}");
            assert_eq!(FORKS.get(), 0, "{case} forked");
        }
        let mut child = spawn_piped(Command::new("cat").env_clear()).unwrap();
        child.stdin.take().unwrap().write_all(b"echo").unwrap();
        assert_eq!(wait_output(child).unwrap().stdout, b"echo");
        assert_eq!(FORKS.get(), 0, "piped forked");
        let theirs = {
            let _spawning = spawning();
            std::process::Command::new("true").env_clear().output()
        };
        assert!(theirs.unwrap().status.success());
        assert_eq!(
            FORKS.get(),
            1,
            "std forks for a bare name in a cleared environment"
        );
    }
}
