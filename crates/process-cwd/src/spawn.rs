//! Children started without a fork, and the pipes their input and output go through, which no child
//! another thread spawns can hold open: the board's helpers (`tasks::sys` re-exports them) and the
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
//! So the pipes the helpers make for their own children are close-on-fork, which no spawn copies;
//! the copies a child is handed exist only under [`spawning`]; and a bare name is looked up before
//! it is spawned, so a start leaves no failed child behind. [`pipe`]'s ends stay copyable, as
//! callers hand them to std's spawns too.
//!
//! [`pipe`] is a FIFO there, and macOS can lose a FIFO's end: a read already blocked on one when
//! its last writer goes may wait for good, while a nonblocking read on it answers EOF (measured,
//! about once in a few hundred thousand reads under load). The helpers' own pipes are real ones.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, PipeReader, PipeWriter, Read};
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdin, ChildStdout, ExitStatus, Output};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// `O_CLOFORK`, which the libc crate does not name for Apple (`sys/fcntl.h`): a descriptor no spawn
/// or fork copies. Only `open` sets it; macOS's `F_SETFD` and `F_DUPFD_CLOFORK` do not.
#[cfg(target_vendor = "apple")]
const O_CLOFORK: libc::c_int = 0x0800_0000;

static SPAWNING: Mutex<()> = Mutex::new(());

/// Held while this process holds a descriptor for a child that any spawn would copy: the copies
/// [`Command::spawn`] hands its child, and a helper's pipe until its ends are close-on-fork. Every
/// helper spawn holds it, and a std spawn in a binary that uses the helpers takes it around its
/// `spawn()`, so that it never copies one (a std child keeps each descriptor not close-on-exec for
/// its life).
/// Never held across a helper's spawn or pipe: they take it themselves.
pub fn spawning() -> MutexGuard<'static, ()> {
    SPAWNING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where a child's standard stream goes: this process's own, `/dev/null`, or a descriptor handed
/// to the child alone.
#[derive(Debug)]
pub enum Stdio {
    Inherit,
    Null,
    Fd(OwnedFd),
}

impl Stdio {
    pub fn null() -> Self {
        Self::Null
    }

    pub fn inherit() -> Self {
        Self::Inherit
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
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl Command {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            cwd: None,
            cleared: false,
            vars: BTreeMap::new(),
            group: None,
            session: false,
            kept: Vec::new(),
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args(&mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> &mut Self {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.vars
            .insert(key.as_ref().to_owned(), Some(value.as_ref().to_owned()));
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
        let key = key.as_ref().to_owned();
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
        self.cwd = Some(dir.as_ref().to_owned());
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
        let streams = [self.stdin.take(), self.stdout.take(), self.stderr.take()]
            .map(|io| io.unwrap_or(Stdio::Inherit));
        // The copies handed to the child live inside native::spawn alone.
        let process = {
            let _spawning = spawning();
            native::spawn(self, streams)?
        };
        Ok(Child {
            process,
            stdin: None,
            stdout: None,
            stderr: None,
        })
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
    /// for a `pre_exec` (what a session or a passed descriptor takes there), a `PATH` changed or
    /// cleared under a bare name, or a relative name with a directory to start from.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(dead_code, reason = "std starts it there")
    )]
    fn forks(&self) -> bool {
        let program = self.program.as_bytes();
        let bare = !program.contains(&b'/');
        self.session
            || !self.kept.is_empty()
            || (bare && (self.cleared || self.vars.contains_key(OsStr::new("PATH"))))
            || (!bare && !program.starts_with(b"/") && self.cwd.is_some())
    }
}

/// A started child, as std's `Child` is used: its id, an end asked or waited for, and the pipes
/// [`spawn_output`] and [`spawn_piped`] read and write it through. Dropping it neither stops nor
/// reaps the child.
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

/// A pipe whose two ends are close-on-exec from the moment they exist, so a child another thread
/// spawns meanwhile never keeps one and the reader's EOF comes when the writers handed out end.
/// macOS has no `pipe2` or `SOCK_CLOEXEC`: std's pipe (behind `Stdio::piped`) and
/// `UnixStream::pair` set the flag in a second call, and a spawn between the two inherits both ends
/// for the child's whole life. A FIFO opened with `O_CLOEXEC`, then unlinked, is atomic there;
/// elsewhere std's own pipe is. macOS's `poll` never reports a FIFO's end, and a read already
/// blocked on one can miss it for good while a nonblocking read finds it, so the helpers read and
/// feed their own children through real pipes of their own. Its ends are not close-on-fork, so
/// any spawn, std's too, can hand one to its child.
#[cfg(target_vendor = "apple")]
pub fn pipe() -> io::Result<(PipeReader, PipeWriter)> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static PIPES: AtomicU64 = AtomicU64::new(0);
    loop {
        let path = std::env::temp_dir().join(format!(
            "sf-pipe-{}-{}",
            std::process::id(),
            PIPES.fetch_add(1, Ordering::Relaxed)
        ));
        let named = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: the path is NUL-terminated and outlives the call.
        if unsafe { libc::mkfifo(named.as_ptr(), 0o600) } != 0 {
            let why = io::Error::last_os_error();
            if why.kind() == io::ErrorKind::AlreadyExists {
                continue; // an earlier process with this pid left the name
            }
            return Err(why);
        }
        // std opens every file close-on-exec in the open call itself. The reader opens without
        // waiting for a writer (O_NONBLOCK), so the writer's open then finds it and returns.
        let ends = File::options()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .and_then(|reader| Ok((reader, File::options().write(true).open(&path)?)));
        let unlinked = std::fs::remove_file(&path);
        let (reader, writer) = ends?;
        unlinked?;
        set_nonblocking(&reader, false)?;
        return Ok((OwnedFd::from(reader).into(), OwnedFd::from(writer).into()));
    }
}

/// [`pipe`] for the helpers' own children: a real pipe, whose end a read already blocked on it
/// sees. `pipe()` makes inheritable ends there and only `open` makes one close-on-fork, so under
/// [`spawning`] each end is reopened through `/dev/fd` close-on-exec and close-on-fork, and the
/// first ends are closed before the lock goes. No spawn under [`spawning`] copies them, and no
/// spawn or fork at all copies the ends returned, even until its exec; a std spawn that skips the
/// lock can keep the first ends for its child's life. Only a helper spawn can hand one over,
/// through a copy made under [`spawning`].
#[cfg(target_vendor = "apple")]
fn own_pipe() -> io::Result<(PipeReader, PipeWriter)> {
    use std::os::unix::fs::OpenOptionsExt;
    let _spawning = spawning();
    let made = io::pipe()?;
    let reopen = |end: RawFd, write: bool| {
        File::options()
            .read(!write)
            .write(write)
            .custom_flags(O_CLOFORK)
            .open(format!("/dev/fd/{end}"))
    };
    let reader = reopen(made.0.as_raw_fd(), false)?;
    let writer = reopen(made.1.as_raw_fd(), true)?;
    drop(made);
    Ok((OwnedFd::from(reader).into(), OwnedFd::from(writer).into()))
}

/// A pipe whose two ends are close-on-exec from the moment they exist: std's (`pipe2`).
#[cfg(not(target_vendor = "apple"))]
pub fn pipe() -> io::Result<(PipeReader, PipeWriter)> {
    io::pipe()
}

/// [`pipe`] for the helpers' own children: std's, as nothing is close-on-fork here.
#[cfg(not(target_vendor = "apple"))]
fn own_pipe() -> io::Result<(PipeReader, PipeWriter)> {
    pipe()
}

/// A pipe for a child's stdin, which the child may poll for its end (node never sees a FIFO's
/// end on macOS): [`own_pipe`], a real one.
fn input_pipe() -> io::Result<(PipeReader, PipeWriter)> {
    own_pipe()
}

/// `fd` made to read and write without blocking, or to block again.
pub fn set_nonblocking(fd: &impl AsFd, nonblocking: bool) -> io::Result<()> {
    let fd = fd.as_fd().as_raw_fd();
    // SAFETY: F_GETFL reads the status flags of a descriptor the borrow keeps open.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    let flags = if nonblocking {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    // SAFETY: F_SETFL sets the status flags of that same open descriptor, from an int.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// `command` started as std's `Command::output` starts it, stdin null and both outputs read through
/// a real pipe of their own: [`wait_output`] then reads them to their end.
pub fn spawn_output(command: &mut Command) -> io::Result<Child> {
    spawn_reading(command, Stdio::Null)
}

/// [`spawn_output`] with the child's stdin a pipe too, as `Stdio::piped` makes it: what is
/// written to `stdin` reaches the child, which sees its end once `stdin` is dropped, whether it
/// reads or polls: a real pipe, close-on-fork on macOS.
pub fn spawn_piped(command: &mut Command) -> io::Result<Child> {
    let (input, stdin) = input_pipe()?;
    let mut child = spawn_reading(command, input.into())?;
    child.stdin = Some(ChildStdin::from(OwnedFd::from(stdin)));
    Ok(child)
}

/// `command` started with `stdin` and both outputs read through a close-on-fork [`own_pipe`] of
/// their own. Only the child holds the writers: [`Command::spawn`] hands its streams over.
fn spawn_reading(command: &mut Command, stdin: Stdio) -> io::Result<Child> {
    let (stdout, out_writer) = own_pipe()?;
    let (stderr, err_writer) = own_pipe()?;
    let mut child = command
        .stdin(stdin)
        .stdout(out_writer)
        .stderr(err_writer)
        .spawn()?;
    child.stdout = Some(ChildStdout::from(OwnedFd::from(stdout)));
    child.stderr = Some(ChildStderr::from(OwnedFd::from(stderr)));
    Ok(child)
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
            // SAFETY: adds the valid SIGPIPE signal to an initialized set.
            unsafe { libc::sigaddset(&raw mut defaults, libc::SIGPIPE) };
            // SAFETY: initialized handles and signal set. std's spawn sets SIGPIPE's default too.
            checked(unsafe {
                libc::posix_spawnattr_setsigdefault(&raw mut attrs.0, &raw const defaults)
            })?;
            let mut flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF;
            if let Some(group) = command.group {
                // SAFETY: sets only the initialized attributes; the kernel validates the group.
                checked(unsafe { libc::posix_spawnattr_setpgroup(&raw mut attrs.0, group) })?;
                flags |= libc::POSIX_SPAWN_SETPGROUP;
            }
            if command.session {
                flags |= SETSID;
            }
            let flags = libc::c_short::try_from(flags).expect("spawn flags fit a c_short");
            // SAFETY: initialized attributes; the mask and other dispositions are inherited, as std's.
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
        let start = |name: &Path| -> io::Result<Spawned> {
            match spawn_at(name, &argv)? {
                Err(libc::ENOEXEC) => {
                    let script: Vec<CString> = [OsStr::new("sh"), name.as_os_str()]
                        .into_iter()
                        .chain(command.args.iter().map(OsString::as_os_str))
                        .map(|arg| cstring(arg.as_bytes()))
                        .collect::<io::Result<_>>()?;
                    spawn_at(Path::new("/bin/sh"), &pointers(&script))
                }
                started => Ok(started),
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
                // kernel's own answer only when none is.
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
        if command.session || !command.kept.is_empty() {
            let (session, kept) = (command.session, command.kept.clone());
            // SAFETY: setsid and fcntl are async-signal-safe and change only the forked child.
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

    /// A folder of the test's own, removed however the test ends.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sf-spawn-{name}-{}", std::process::id()));
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
                .env("SF_SPAWN_SET", "1")
                .env_remove("HOME")),
        );
        same(
            "cleared",
            both!("/usr/bin/env", |c| c
                .env("SF_SPAWN_EARLIER", "lost")
                .env_clear()
                .env("SF_SPAWN_SET", "1")
                .env_remove("SF_SPAWN_NEVER")),
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
            ("missing name", both!("sf-no-such-program")),
            (
                "missing name, child's PATH",
                both!("sf-no-such-program", |c| c.env("PATH", &tmp.0)),
            ),
            ("missing path", both!("/no/such/sf-program")),
            ("denied path", both!(&denied)),
            ("denied name", both!("denied", |c| c.env("PATH", &tmp.0))),
            (
                "name under a folder it may not search",
                both!("true", |c| c.env("PATH", &shut)),
            ),
            ("a directory", both!(&tmp.0)),
            ("path through a file", both!(file.join("sh"))),
            (
                "missing directory",
                both!("/bin/sh", |c| c.current_dir(tmp.0.join("absent"))),
            ),
            // A directory it cannot enter is the answer for a name with a slash and on std's fork
            // path; std's other path searches and reads it as every name missing.
            (
                "directory a file",
                both!("/bin/sh", |c| c.current_dir(&file)),
            ),
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
            let mut child = command.arg("60").spawn().unwrap();
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

    #[test]
    fn a_pipe_carries_what_is_written_and_ends_when_its_writer_does() {
        let (mut reader, mut writer) = pipe().unwrap();
        writer.write_all(b"answer").unwrap();
        drop(writer);
        let mut said = String::new();
        reader.read_to_string(&mut said).unwrap();
        assert_eq!(said, "answer");
    }

    #[test]
    fn a_pipes_end_can_be_handed_to_a_child_std_spawns() {
        use std::os::unix::process::CommandExt;
        let (mut reader, writer) = pipe().unwrap();
        let mut child = {
            let _spawning = spawning();
            std::process::Command::new("/bin/sh")
                .args(["-c", "printf handed >&2"])
                .stderr(writer)
                .process_group(0)
                .spawn()
                .unwrap()
        };
        assert!(child.wait().unwrap().success());
        let mut said = String::new();
        reader.read_to_string(&mut said).unwrap();
        assert_eq!(said, "handed");
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

    /// A child std forks holds each descriptor not close-on-fork until its exec, close-on-exec ones
    /// too, as [`pipe`]'s ends; the pipes the helpers make for their own children never reach it.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn the_helpers_own_pipe_ends_never_reach_a_forked_child_even_before_its_exec() {
        use std::os::unix::process::CommandExt;
        let found = |fd: RawFd| {
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
        };
        let (reader, writer) = own_pipe().unwrap();
        let (input, stdin) = input_pipe().unwrap();
        for (end, fd) in [
            ("output reader", reader.as_raw_fd()),
            ("output writer", writer.as_raw_fd()),
            ("stdin reader", input.as_raw_fd()),
            ("stdin writer", stdin.as_raw_fd()),
        ] {
            assert!(!found(fd), "the {end} reached a forked child");
        }
        let (reader, writer) = pipe().unwrap();
        for end in [reader.as_raw_fd(), writer.as_raw_fd()] {
            assert!(found(end), "a std spawn can hand pipe()'s ends over");
        }
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

    /// macOS can lose a FIFO's end for a read already blocked on it, so what the helpers read their
    /// children through, and feed them through, is a real pipe. [`pipe`] stays a FIFO.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn the_helpers_read_and_feed_their_children_through_real_pipes() {
        let child = spawn_piped(&mut Command::new("cat")).unwrap();
        let (fifo, _writer) = pipe().unwrap();
        let ends = [
            ("stdin", child.stdin.as_ref().unwrap().as_raw_fd()),
            ("stdout", child.stdout.as_ref().unwrap().as_raw_fd()),
            ("stderr", child.stderr.as_ref().unwrap().as_raw_fd()),
            ("pipe()", fifo.as_raw_fd()),
        ];
        let kinds = kinds(std::process::id().try_into().unwrap());
        let found = ends.map(|(end, fd)| (end, kinds.get(&fd).copied()));
        let (pipe, vnode) = (libc::PROX_FDTYPE_PIPE, libc::PROX_FDTYPE_VNODE);
        assert_eq!(
            found,
            [
                ("stdin", Some(pipe)),
                ("stdout", Some(pipe)),
                ("stderr", Some(pipe)),
                ("pipe()", Some(vnode)),
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
        assert!(output(&mut Command::new("/no/such/sf-output-command")).is_err());
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
        assert!(spawn_piped(&mut Command::new("/no/such/sf-output-command")).is_err());
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

    /// Children outliving every answer, spawned through std beside it as another thread's long
    /// child would be, each under [`spawning`] as every std spawn beside the helpers, and keeping
    /// whatever else it could copy as it started.
    fn racing(spawned: &AtomicBool) -> Children {
        let mut children = Children(Vec::new());
        for _ in 0..32 {
            let _spawning = spawning();
            let child = std::process::Command::new("sleep")
                .arg("60")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
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
                // A child forked by another test holds the writer until its exec, no longer.
                let made = Instant::now();
                loop {
                    match reader.read(&mut [0]) {
                        Ok(0) => break,
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                && made.elapsed() < Duration::from_secs(5) =>
                        {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        other => panic!("answer {answers} is held open: {other:?}"),
                    }
                }
                answers += 1;
            }
            drop(other.join().unwrap());
            assert!(answers > 0, "no answer was made beside the spawns");
        });
    }

    /// A std child keeps each descriptor not close-on-exec at its spawn for its life, and a helper's
    /// pipe is one until it is reopened: a racer spawned under [`spawning`], as [`racing`]'s, keeps
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
                let mut child =
                    spawn_piped(Command::new("true").env("PATH", "/no/such/sf-spawn:/usr/bin"))
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
            // A wait some racer held would have ended only with that racer, 60 s on.
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
