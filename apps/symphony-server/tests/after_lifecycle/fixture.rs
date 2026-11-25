// Owned processes, scratch repositories and build/opener fixtures for the lifecycle boundary.
// Every child starts through the board's forkless `symphony_board::sys::Command`: a child std forks
// holds every descriptor this process has until its exec, a sibling case's pipes among them.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::symlink;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::{Child, Command, Stdio};

#[expect(
    clippy::disallowed_methods,
    reason = "symphony-chrome's harness file, read by path: a published package the shared sorter's rule leaves out"
)]
#[path = "../../../../crates/chrome-devtools/src/scratch.rs"]
mod scratch;

#[path = "../../../../tools/test-support/machine_copy.rs"]
pub mod machine_copy;

/// A machine copy's first run starts under the spawn lock the board's `sys` takes for every spawn.
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<ExitStatus> {
    symphony_board::sys::Spawning::status_locked(command)
}

#[path = "stand_ins.rs"]
mod stand_ins;
use stand_ins::StandIns;

#[path = "owner.rs"]
mod owner;
use owner::stage as owner_stage;

#[cfg(target_os = "macos")]
#[path = "forkless.rs"]
mod forkless;

#[path = "killed.rs"]
pub mod killed;

pub const APP: &str = env!("CARGO_BIN_EXE_symphony-server");
/// What a wait of this target takes on a quiet machine with room to spare: its allowance.
pub const PATIENCE: Duration = Duration::from_secs(30);
/// The wall-clock backstop of every wait of this target, which only a stall reaches
/// (`test_time::backstop`, RULES.md `tests/no-test-fails-on-load`).
pub const BACKSTOP: Duration = test_time::backstop(PATIENCE);

const SCRATCH_PREFIX: &str = "after-lifecycle-";

/// The folder every scratch folder of this target is made in, as the fixture names it.
fn scratch_parent() -> PathBuf {
    Path::new(FIXTURE_ROOT).join("../../artifacts/after_lifecycle")
}

/// Validate the freshly linked executable once before measuring server starts. The OS's first
/// image launch is setup, waited for to its end under no case deadline (`setup_success`); every
/// server still has the unchanged patience for its actual startup.
/// Keep the completed child's identity so reuse is observable without a timing threshold.
static PREPARED_APP: OnceLock<u32> = OnceLock::new();
static PREPARED_COMMANDS: OnceLock<[u32; 4]> = OnceLock::new();

fn prepare_app(log: &Path) -> u32 {
    *PREPARED_APP.get_or_init(|| {
        if let Some(pid) = owner::parents_app() {
            return pid;
        }
        let mut command = Command::new(APP);
        command.arg("--help").env_clear().stdin(Stdio::null());
        let mut preparation = Process::spawn(command, log);
        preparation.setup_success();
        assert!(
            preparation.output().starts_with("usage: symphony-server "),
            "the built image must answer help before fixture starts: {}",
            preparation.output()
        );
        preparation.child.id()
    })
}

/// First execution of each immutable script is setup, before any measured recipe start. Scratch
/// links keep opener/process-table observations away from the case's own observable paths.
fn prepare_commands(repo: &Repo, folder: &Path) -> [u32; 4] {
    *PREPARED_COMMANDS.get_or_init(|| {
        let bin = folder.join("commands");
        fs::create_dir_all(&bin).unwrap();
        let images: [(&str, &str, &[&str]); 4] = [
            ("bash", "bash.sh", &["-c", "true"]),
            ("build", "build.sh", &["tasks", "reap"]),
            ("ps", "ps.sh", &["-o", "pid="]),
            ("open", "open.sh", &[]),
        ];
        images.map(|(name, source, args)| {
            let executable = bin.join(name);
            command_fixture(&executable, source);
            let mut command = Command::new(executable);
            command
                .current_dir(&repo.main)
                .env_clear()
                .envs(repo.environment())
                .stdin(Stdio::null())
                .args(args);
            let mut preparation = Process::spawn(command, &folder.join(name));
            preparation.success();
            let pid = preparation.child.id();
            drop(preparation);
            owner_stage(&format!("command preparation {name}"));
            pid
        })
    })
}

/// Own the folder before repository setup, so even setup failures retain their diagnostic files.
struct Scratch {
    path: PathBuf,
    name: String,
}

impl Scratch {
    fn new(name: &str) -> Self {
        static REAP: Once = Once::new();
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = scratch_parent();
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        REAP.call_once(|| scratch::reap_dead_scratch(&root, SCRATCH_PREFIX));
        owner_stage("scratch sweep");
        loop {
            let path = root.join(format!(
                "{SCRATCH_PREFIX}{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Self {
                        path,
                        name: name.into(),
                    };
                }
                // A previous incarnation of this pid may have left this name. Never reuse it.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create scratch {}: {error}", path.display()),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "after_lifecycle case {} failed; scratch kept at {}",
                self.name,
                self.path.display()
            );
        } else if let Err(error) = fs::remove_dir_all(&self.path) {
            panic!("cannot remove scratch {}: {error}", self.path.display());
        }
        owner_stage("scratch removal");
    }
}

pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn command_fixture(path: &Path, name: &str) {
    // Fresh executable scripts incur first-exec validation too, and a checkout's own scripts are
    // fresh in every verify run. Link the machine's one copy of these bytes for every scratch
    // checkout; $0 still names the invoking fixture path (ps/open use it).
    symlink(machine_copy::machine_copy(&script_source(name)), path).unwrap();
    owner_stage(&format!("link {}", path.display()));
}

/// The tracked script a fixture command's machine-wide copy is made from.
pub fn script_source(name: &str) -> PathBuf {
    Path::new(FIXTURE_ROOT)
        .join("tests/after_lifecycle")
        .join(name)
}

pub fn repository(root: &Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        Command::new("git")
            .args([
                "-c",
                "user.name=Lifecycle fixture",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE"),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {}", said(&done));
    owner_stage(&format!("git {}", args[0]));
}

pub fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn output(stdout: &Path, stderr: &Path) -> String {
    format!(
        "{}{}",
        fs::read_to_string(stdout).unwrap_or_default(),
        fs::read_to_string(stderr).unwrap_or_default()
    )
}

const SERVER_LOG_TAIL_BYTES: u64 = 16 * 1024;

/// A bounded byte tail of the opened regular file, preserving malformed or split UTF-8 lossily.
fn server_log_tail(path: &Path) -> std::io::Result<String> {
    // Nonblocking open prevents a substituted FIFO from holding the failure report. Check the
    // opened descriptor's type, and refuse a symlink at the log entry itself.
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let start = metadata.len().saturating_sub(SERVER_LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    // This snapshot's range stays bounded even when the server appends while it is read.
    file.take(metadata.len() - start).read_to_end(&mut bytes)?;
    let mut text = if start == 0 {
        String::new()
    } else {
        format!("[showing last {} bytes]\n", bytes.len())
    };
    text.push_str(&String::from_utf8_lossy(&bytes));
    Ok(text)
}

/// Read the child streams and the detached server's bounded log tail at failure time.
fn diagnostic_output(stdout: &Path, stderr: &Path, server_log: Option<&Path>) -> String {
    let mut text = output(stdout, stderr);
    if let Some(path) = server_log {
        let said = server_log_tail(path).unwrap_or_else(|error| format!("unavailable: {error}"));
        text.push_str(&format!(
            "\ndetached server log {}:\n{said}",
            path.display()
        ));
    }
    text
}

/// This polls an observable event, never a presumed startup duration: only a stall reaches its
/// [`BACKSTOP`].
pub fn until<T>(why: &str, event: impl FnMut() -> Option<T>) -> T {
    poll(BACKSTOP, || why.into(), event, Instant::elapsed)
}

fn poll<T>(
    backstop: Duration,
    why: impl FnOnce() -> String,
    event: impl FnMut() -> Option<T>,
    elapsed: impl FnMut(&Instant) -> Duration,
) -> T {
    poll_for(backstop, event, elapsed).unwrap_or_else(|| panic!("{}", stalled(&why(), backstop)))
}

/// A wait's failure: `why` was still not so when the wait's wall-clock `backstop` ran out, which
/// only a stall reaches, and the machine's load then, on a line of its own after it.
fn stalled(why: &str, backstop: Duration) -> String {
    format!(
        "stalled: {why}\n(the wait's wall-clock backstop of {backstop:?} ran out; {})",
        test_time::Load::now()
    )
}

/// `message` with the line [`stalled`] ends it with taken off, after checking that line names
/// [`BACKSTOP`] and the load: what a stalled wait said of itself.
#[cfg(test)]
fn stalled_why(message: &str) -> &str {
    let (why, load) = message.rsplit_once('\n').unwrap_or_else(|| panic!("{message}"));
    let backstop = format!("(the wait's wall-clock backstop of {BACKSTOP:?} ran out; ");
    assert!(
        load.starts_with(&backstop) && load.contains("load average"),
        "{message}"
    );
    why
}

/// `event`'s value once it gives one, or none once `backstop` has passed on `elapsed`, the wait's
/// clock: the wall's (`Instant::elapsed`), or a test's own, which alone ends the wait, so
/// test_time's wall backstop is never reached first.
fn poll_for<T>(
    backstop: Duration,
    mut event: impl FnMut() -> Option<T>,
    mut elapsed: impl FnMut(&Instant) -> Duration,
) -> Option<T> {
    let began = Instant::now();
    test_time::until("an event of the lifecycle fixture", Duration::MAX, || {
        if let Some(value) = event() {
            return Some(Some(value));
        }
        (elapsed(&began) >= backstop).then_some(None)
    })
    .unwrap_or_else(|stall| panic!("{stall}"))
}

/// Cleanup still fails a passing case, but preserves an original failure and its other reports.
fn wait_cleanup(
    why: impl FnOnce() -> String,
    mut finished: impl FnMut() -> Result<bool, String>,
    elapsed: impl FnMut(&Instant) -> Duration,
) {
    let outcome = poll_for(
        BACKSTOP,
        || match finished() {
            Ok(true) => Some(Ok(())),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        },
        elapsed,
    );
    let message = match outcome {
        Some(Ok(())) => return,
        Some(Err(error)) => format!("{}: {error}", why()),
        None => stalled(&why(), BACKSTOP),
    };
    cleanup_failed(&message);
}

/// Fail a passing case with `message`; beside an original failure, report it and keep that one.
fn cleanup_failed(message: &str) {
    if std::thread::panicking() {
        eprintln!("{message}");
    } else {
        panic!("{message}");
    }
}

/// One request for `path` on `port`, the status and body it was answered with, or none where
/// nothing listens there or the answer is no HTTP. A connection that is taken is answered: only a
/// stall reaches [`BACKSTOP`] while it waits. A wait for an answer is [`answer`]'s.
pub fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), BACKSTOP).ok()?;
    stream.set_read_timeout(Some(BACKSTOP)).ok()?;
    stream.set_write_timeout(Some(BACKSTOP)).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    let (head, body) = reply.split_once("\r\n\r\n")?;
    Some((head.split_whitespace().nth(1)?.parse().ok()?, body.into()))
}

/// The answer to `path` on `port`, once the server there gives one ([`get`]): a wait on it.
pub fn answer(port: u16, path: &str) -> (u16, String) {
    until(&format!("an answer to GET {path} on port {port}"), || {
        get(port, path)
    })
}

/// One read of the native status on `port` ([`get`]), or none.
pub fn front(port: u16) -> Option<symphony_api::Front> {
    let (status, body) = get(port, "/api/front")?;
    (status == 200)
        .then(|| serde_json::from_str(&body).ok())
        .flatten()
}

/// The native status on `port`, once the server there gives it ([`front`]): a wait on it.
pub fn status(port: u16) -> symphony_api::Front {
    until(&format!("the native status on port {port}"), || front(port))
}

/// Retains and reaps its exact child even if a test assertion unwinds.
pub struct Process {
    pub child: Child,
    command: String,
    executable: Executable,
    launch: String,
    /// When its spawn began, by the wall clock: where the system's log for its pid is read from.
    spawned: std::time::SystemTime,
    stdout: PathBuf,
    stderr: PathBuf,
    server_log: Option<PathBuf>,
    /// The server [`Process::ready`] found answering, the child itself or a process under it.
    server: Option<killed::Owned>,
    /// The signals this Process sent its child itself.
    sent: Vec<i32>,
    /// What was known of the child's pid when it ended by a signal this Process did not send.
    killed: Option<String>,
    descendants: Option<Descendants>,
    // Normal commands use Instant::elapsed; the private clock makes wait-boundary tests deterministic.
    wait_elapsed: Box<dyn FnMut(&Instant) -> Duration + Send>,
}

/// Keep the pre-spawn identity even if another command replaces the executable's path later.
struct Executable {
    path: PathBuf,
    at_spawn: String,
}

impl Executable {
    fn capture(command: &Command) -> Self {
        let path = PathBuf::from(command.get_program());
        let at_spawn = Self::identity(&path);
        Self { path, at_spawn }
    }

    fn identity(path: &Path) -> String {
        match fs::metadata(path) {
            Ok(meta) => format!(
                "inode={} mtime={}.{:09}",
                meta.ino(),
                meta.mtime(),
                meta.mtime_nsec()
            ),
            Err(error) => format!("unavailable: {error}"),
        }
    }

    fn failure(&self) -> String {
        format!(
            "; executable {:?}; at spawn: {}; at failure: {}",
            self.path,
            self.at_spawn,
            Self::identity(&self.path)
        )
    }
}

/// Every descendant inherits the writer. EOF proves even an orphaned fixture command has
/// stopped using scratch. A server starts its own children without it: those stand-ins hold
/// [`StandIns`] instead.
struct Descendants {
    reader: File,
    origin: String,
    wait_elapsed: Box<dyn FnMut(&Instant) -> Duration + Send>,
}

impl Descendants {
    /// Both ends of a FIFO at `at`, close-on-exec from the moment they exist: open(2) takes
    /// O_CLOEXEC atomically and std always passes it. A socket pair cannot be on macOS, which has
    /// no SOCK_CLOEXEC: std flags each end after socketpair, and a spawn on another test thread in
    /// between keeps the writer for its whole life.
    fn endpoint(at: &Path, origin: String) -> (Self, File) {
        mkfifo(at);
        let reader = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(at)
            .unwrap();
        // The reader is open, so this neither blocks nor fails.
        let writer = fs::OpenOptions::new().write(true).open(at).unwrap();
        // Only these two descriptors ever reach the FIFO.
        fs::remove_file(at).unwrap();
        (
            Self {
                reader,
                origin,
                wait_elapsed: Box::new(Instant::elapsed),
            },
            writer,
        )
    }

    fn spawn(command: &mut Command, at: &Path) -> (Self, File) {
        let (descendants, writer) = Self::endpoint(at, invocation(command));
        let writer = above_shell_descriptors(writer);
        // The writer lives through spawn, which hands it to the child alone under its own number.
        // This process's copy stays close-on-exec, so concurrent tests' unrelated commands
        // cannot inherit this endpoint.
        command.pass_fd(writer.as_raw_fd());
        (descendants, writer)
    }

    fn started(mut self, pid: u32) -> Self {
        self.origin = format!("descendants of child {pid} ({})", self.origin);
        self
    }

    fn exited(mut reader: &File) -> Result<bool, String> {
        match reader.read(&mut [0]) {
            Ok(0) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            other => Err(format!("fixture descendant lifetime: {other:?}")),
        }
    }

    fn wait(&mut self) {
        let reader = &self.reader;
        let origin = &self.origin;
        wait_cleanup(
            || format!("{origin} to exit before scratch removal"),
            || Self::exited(reader),
            &mut self.wait_elapsed,
        );
    }
}

/// The lowest descriptor a shell script does not name: POSIX redirections take one digit
/// (`exec 3<&0`), and the child receives the endpoint under the number it has here.
const ABOVE_SHELL_DESCRIPTORS: libc::c_int = 10;

/// `file` moved above every single-digit descriptor. Wherever it was opened, a fixture script's
/// own redirection of that number would close it in the shell, and no descendant would hold it.
fn above_shell_descriptors(file: File) -> File {
    // SAFETY: duplicates a live descriptor this function owns; the copy is close-on-exec from
    // the moment it exists, as the original was.
    let raised = unsafe {
        libc::fcntl(
            file.as_raw_fd(),
            libc::F_DUPFD_CLOEXEC,
            ABOVE_SHELL_DESCRIPTORS,
        )
    };
    assert_ne!(
        raised,
        -1,
        "cannot raise the lifetime endpoint: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: fcntl just returned this new descriptor; ownership is transferred once.
    unsafe { File::from_raw_fd(raised) }
}

impl Process {
    pub fn spawn(mut command: Command, log: &Path) -> Self {
        let invocation = invocation(&command);
        let stdout = log.with_extension("out");
        let stderr = log.with_extension("err");
        let (descendants, writer) =
            Descendants::spawn(&mut command, &log.with_extension("lifetime"));
        let executable = Executable::capture(&command);
        let launching = std::time::SystemTime::now();
        let spawning = Instant::now();
        let child = command
            .env_remove("BASH_ENV")
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let launch = launch_report(launching, spawning, child.id(), &invocation);
        drop(writer);
        let descendants = descendants.started(child.id());
        Self {
            child,
            command: invocation,
            executable,
            launch,
            spawned: launching,
            stdout,
            stderr,
            server_log: None,
            server: None,
            sent: Vec::new(),
            killed: None,
            descendants: Some(descendants),
            wait_elapsed: Box::new(Instant::elapsed),
        }
    }

    pub fn output(&self) -> String {
        output(&self.stdout, &self.stderr)
    }

    /// The child's end. One by a signal this Process did not send also says on stderr what was
    /// known of its pid then ([`killed::capture`]), kept beside its logs.
    pub fn wait(&mut self) -> ExitStatus {
        let known = self.killed.is_some();
        let status = self.ended();
        if !known && let Some(killed) = &self.killed {
            eprintln!("{killed}");
        }
        status
    }

    /// [`Process::wait`], what was known of a signal's pid kept for the caller to say.
    fn ended(&mut self) -> ExitStatus {
        let child = &mut self.child;
        let id = child.id();
        let stdout = &self.stdout;
        let stderr = &self.stderr;
        let server_log = self.server_log.as_deref();
        let launch = &self.launch;
        let status = poll(
            BACKSTOP,
            || {
                format!(
                    "child {id} to exit; {launch}: {}",
                    diagnostic_output(stdout, stderr, server_log)
                )
            },
            || child.try_wait().unwrap(),
            &mut self.wait_elapsed,
        );
        self.capture_if_killed(status);
        if let Some(server) = self.server.take() {
            until("private server to exit after its launcher", || {
                (!server.alive()).then_some(())
            });
        }
        status
    }

    /// Once, when `status` is an end by a signal this Process did not send: what was known of the
    /// child's pid at that moment, kept beside its logs, for its failure to say.
    fn capture_if_killed(&mut self, status: ExitStatus) {
        if let Some(signal) = status.signal()
            && !self.sent.contains(&signal)
            && self.killed.is_none()
        {
            self.killed = Some(killed::capture(&killed::Dead {
                pid: self.child.id(),
                command: &self.command,
                signal,
                spawned: self.spawned,
                kept: &self.stdout.with_extension("killed"),
            }));
        }
    }

    pub fn success(&mut self) {
        let status = self.ended();
        self.succeeded(status);
    }

    /// `success` for setup's run, which waits for the child's own end, never a case's deadline:
    /// a new file's first exec waits for macOS to assess it, one file at a time machine-wide, as
    /// machine_copy's prime does. The test runner's load-scaled budget still bounds a
    /// hung one.
    fn setup_success(&mut self) {
        let status = self.child.wait().unwrap();
        self.capture_if_killed(status);
        self.succeeded(status);
    }

    /// Fail with the child's pid, command, status and output, for a silent signal its
    /// executable's identity at spawn and now, and for a signal this Process did not send what was
    /// known of its pid then, unless `status` is a success.
    fn succeeded(&self, status: ExitStatus) {
        let output = self.output();
        let identity = if status.signal().is_some() && output.is_empty() {
            self.executable.failure()
        } else {
            String::new()
        };
        let killed = self
            .killed
            .as_deref()
            .map(|killed| format!("\n{killed}"))
            .unwrap_or_default();
        assert!(
            status.success(),
            "child {} ({}) failed with {status}{identity}: {output}{killed}",
            self.child.id(),
            self.command,
        );
    }

    pub fn ready(&mut self) -> symphony_api::Front {
        let stdout = self.stdout.clone();
        let stderr = self.stderr.clone();
        let id = self.child.id();
        let child = &mut self.child;
        let actual = poll(
            BACKSTOP,
            || {
                format!(
                    "native server readiness for child {id}: {}",
                    output(&stdout, &stderr)
                )
            },
            || {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "{}",
                    output(&stdout, &stderr)
                );
                let text = fs::read_to_string(&stdout).unwrap_or_default();
                let port = text
                    .split("http://127.0.0.1:")
                    .nth(1)?
                    .split('/')
                    .next()?
                    .parse()
                    .ok()?;
                assert_ne!(port, 8765);
                let pid: i64 = text
                    .split("(pid ")
                    .nth(1)?
                    .split(')')
                    .next()?
                    .parse()
                    .ok()?;
                front(port).filter(|f| f.pid == pid && f.ready)
            },
            &mut self.wait_elapsed,
        );
        self.server = Some(killed::Owned::of(actual.pid));
        actual
    }

    /// `signal` to the server [`Process::ready`] found, else to the child, journaled.
    pub fn signal(&mut self, signal: i32) {
        let child = i64::from(self.child.id());
        match &self.server {
            Some(server) if server.pid != child => {
                server.signal(signal, &format!("the case signals the server of {}", self.command));
            }
            _ => {
                self.sent.push(signal);
                killed::send(child, signal, &format!("the case signals {}", self.command));
            }
        }
    }
}

fn invocation(command: &Command) -> String {
    format!(
        "{:?} {:?}",
        command.get_program(),
        command.get_args().collect::<Vec<_>>()
    )
}

/// Keep the parent clock even when the executable has not reached its first Rust stage.
fn launch_report(
    launching: std::time::SystemTime,
    spawning: Instant,
    pid: u32,
    command: &str,
) -> String {
    let report = format!(
        "child {pid} spawn began at {} ms since Unix epoch; spawn returned after {:.3} ms: {command}",
        launching
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        spawning.elapsed().as_secs_f64() * 1000.0
    );
    if symphony_env::PROFILE.os().is_some() {
        eprintln!(
            "{}: {report}",
            std::thread::current().name().unwrap_or("lifecycle")
        );
    }
    report
}

impl Drop for Process {
    fn drop(&mut self) {
        let server = self.server.take();
        if let Some(server) = &server {
            server.signal(libc::SIGKILL, "a Process dropped: its server");
        }
        if self.child.try_wait().ok().flatten().is_none() {
            killed::note(
                i64::from(self.child.id()),
                libc::SIGKILL,
                &format!("a Process dropped: its own child, {}", self.command),
            );
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(server) = server {
            wait_cleanup(
                || format!("owned private server {} to exit", server.pid),
                || Ok(!server.alive()),
                Instant::elapsed,
            );
        }
        if let Some(descendants) = &mut self.descendants {
            descendants.wait();
        }
        profile_warm(&self.output());
        if self.command.starts_with("\"just\"") && symphony_env::PROFILE.os().is_some()
        {
            eprintln!("{}\n{}", self.launch, self.output());
        }
    }
}

fn profile_warm(output: &str) {
    if symphony_env::PROFILE.os().is_some() {
        for line in output.lines().filter(|line| {
            line.contains("→ native read data warmed:") || line.contains("· Symphony startup ")
        }) {
            eprintln!(
                "{}: {line}",
                std::thread::current().name().unwrap_or("lifecycle")
            );
        }
    }
}

pub struct Repo {
    pub base: PathBuf,
    pub main: PathBuf,
    pub tree: PathBuf,
    /// The machine folder: the one server's record, log, lock and registry.
    pub home: PathBuf,
    /// The install folder in the git common dir: the one server's build and client.
    pub install: PathBuf,
    /// A port the test fixed itself (it holds a listener there); otherwise the server picks one.
    pub port: Option<u16>,
    pub bin: PathBuf,
    pub builds: PathBuf,
    /// The shared servers its cleanup ends, each taken while it answered as itself.
    detached: Mutex<Vec<killed::Owned>>,
    descendants: Mutex<Vec<Descendants>>,
    stand_ins: Mutex<StandIns>,
    next: AtomicUsize,
    _scratch: Scratch,
}

impl Repo {
    fn with_recipe(name: &str, recipes: Option<&str>) -> Self {
        let scratch = Scratch::new(name);
        // Before the first command: preparation runs the ps stand-in too.
        let stand_ins = StandIns::open(&scratch.path);
        prepare_app(&scratch.path.join("prepare-app"));
        owner_stage("app preparation");
        let base = scratch.path.clone();
        let main = base.join("main with spaces");
        fs::create_dir_all(&main).unwrap();
        repository(&main, &["init", "-q", "-b", "main"]);
        repository(&main, &["config", "core.hooksPath", "/dev/null"]);
        write(&main.join("docs/A.md"), "# Alpha\n\nmain document\n");
        write(
            &main.join(".gitignore"),
            "/artifacts/\n/build/\n/.target/\n",
        );

        // Copy the actual recipe verbatim. Build fixtures stand in only for Cargo and bundling;
        // checkout selection, --if-up, argument filtering and exec still run in production bash.
        let docs = recipes
            .map(|recipes| {
                let declaration = recipes.find("\nsymphony *ARGS:").unwrap();
                let start = recipes[..declaration].rfind("\n\n").unwrap();
                // Preserve the supplied real recipe and its _symphony-build body verbatim.
                recipes[start..]
                    .split("# The review board:")
                    .next()
                    .unwrap()
            })
            .unwrap_or("");
        write(
            &main.join("justfile"),
            &format!(
                "{docs}\nsymphony-ui-dist OUT=\"build/symphony-ui\":\n    @tools/fixture-build ui \"{{{{OUT}}}}\"\n"
            ),
        );
        fs::create_dir_all(main.join("tools")).unwrap();
        command_fixture(&main.join("tools/fixture-build"), "build.sh");
        command_fixture(&main.join("tools/rs-bin.sh"), "build.sh");
        repository(&main, &["add", "."]);
        repository(&main, &["commit", "-qm", "Lifecycle fixture"]);
        let tree = base.join("starter with spaces");
        repository(
            &main,
            &[
                "worktree",
                "add",
                "-qb",
                "feature/lifecycle",
                tree.to_str().unwrap(),
            ],
        );
        write(&tree.join("docs/A.md"), "# Alpha\n\nbranch document\n");
        owner_stage("worktree document");
        // The one server's machine folder is the test's own; its build stays in the install folder.
        let home = base.join("machine");
        let bin = base.join("commands");
        fs::create_dir(&bin).unwrap();
        for name in ["open", "xdg-open"] {
            command_fixture(&bin.join(name), "open.sh");
        }
        // Observe the detached environment and real command lines for fixture-owned processes.
        command_fixture(&bin.join("ps"), "ps.sh");
        command_fixture(&bin.join("bash"), "bash.sh");
        symlink("/usr/bin/false", bin.join("tailscale")).unwrap();
        let repo = Self {
            builds: base.join("builds"),
            install: main.join(".git/symphony/server"),
            base,
            main,
            tree,
            home,
            port: None,
            bin,
            detached: Mutex::new(Vec::new()),
            descendants: Mutex::new(Vec::new()),
            stand_ins: Mutex::new(stand_ins),
            next: AtomicUsize::new(0),
            _scratch: scratch,
        };
        prepare_commands(&repo, &repo.base.join("prepare-commands"));
        repo
    }

    /// The isolated environment, and all of it, that every fixture command runs in.
    pub fn environment(&self) -> Vec<(&'static str, OsString)> {
        let mut vars: Vec<(&'static str, OsString)> = vec![
            ("HOME", self.base.clone().into()),
            (symphony::machine::DIR_ENV, self.home.clone().into()),
            ("USER", "lifecycle-fixture".into()),
            ("LANG", "C".into()),
            (
                "PATH",
                format!(
                    "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
                    self.bin.display()
                )
                .into(),
            ),
            (
                "SYMPHONY_PORT",
                self.port.unwrap_or(0).to_string().into(),
            ),
            (
                "SYMPHONY_TEST_PID",
                std::process::id().to_string().into(),
            ),
            // SYMPHONY_STARTUP_TRACE=1 keeps starter stages in timeout reports. Stop and
            // serving commands stay quiet even when the same fixture environment asks for it.
            ("SYMPHONY_STARTUP_TRACE", "1".into()),
            // Foreground previews must not discover any surrounding repository's live data.
            // An invalid database name is refused before connecting or starting a board engine.
            ("TASKS_DATABASE", "lifecycle fixture has no board".into()),
            ("ASKS_DB", self.base.join("private-asks.db").into()),
            ("SYMPHONY_LIFECYCLE_BINARY", APP.into()),
            ("SYMPHONY_LIFECYCLE_LOG", self.builds.clone().into()),
        ];
        // Keep the coverage collector on foreground children. Home must still exercise its real
        // environment filter; its detached child writes LLVM's default profile in the scratch cwd.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            vars.push(("LLVM_PROFILE_FILE", profile));
        }
        vars
    }

    pub fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new(APP);
        command
            .current_dir(dir)
            .env_clear()
            .envs(self.environment())
            .stdin(Stdio::null());
        command
    }

    pub fn external(&self, name: &str) -> PathBuf {
        let root = self.base.join(name);
        write(&root.join("docs/A.md"), "# External\n");
        // Being outside the scratch main does not imply being outside every repository: these
        // artifacts live below the working checkout. Give the external root its own metadata so
        // even environment-free discovery cannot fall through to the surrounding live board.
        repository(&root, &["init", "-q", "-b", "main"]);
        let checkout = symphony_server::repo::checkout(&root).unwrap();
        assert_eq!(checkout.common, root.join(".git"));
        root
    }

    /// `just symphony` in the same isolated environment as [`Repo::command`], with each stage timed.
    pub fn recipe(&self, dir: &Path) -> Command {
        let mut recipe = Command::new("just");
        recipe
            .current_dir(dir)
            .args(["symphony"])
            .stdin(Stdio::null())
            .env_clear()
            .env("JUST_VERBOSE", "1")
            .env("JUST_TIMESTAMP", "true")
            .env("JUST_TIMESTAMP_FORMAT", "%s%.3f")
            .env("JUST_TIME", "true")
            .envs(self.environment());
        recipe
    }

    pub fn log(&self, label: &str) -> PathBuf {
        self.base.join(format!(
            "{label}-{}",
            self.next.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The port the server holds: the one the test fixed, else the one the server picked and
    /// recorded.
    pub fn port(&self) -> u16 {
        self.port
            .unwrap_or_else(|| self.record().expect("shared address record").port)
    }

    pub fn run(&self, command: Command) -> Process {
        let mut process = Process::spawn(command, &self.log("process"));
        process.server_log = Some(self.home.join("server.log"));
        self.descendants
            .lock()
            .unwrap()
            .push(process.descendants.take().unwrap());
        process
    }

    pub fn run_terminal(&self, mut command: Command) -> (Process, File) {
        let invocation = invocation(&command);
        let (master, slave) = terminal();
        let stdout = self.log("terminal").with_extension("out");
        let stderr = stdout.with_extension("err");
        let (descendants, writer) =
            Descendants::spawn(&mut command, &stdout.with_extension("lifetime"));
        let executable = Executable::capture(&command);
        let launching = std::time::SystemTime::now();
        let spawning = Instant::now();
        let child = command
            .env_remove("BASH_ENV")
            .stdout(Stdio::from(slave))
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let launch = launch_report(launching, spawning, child.id(), &invocation);
        drop(writer);
        self.descendants
            .lock()
            .unwrap()
            .push(descendants.started(child.id()));
        (
            Process {
                child,
                command: invocation,
                executable,
                launch,
                spawned: launching,
                stdout,
                stderr,
                server_log: Some(self.home.join("server.log")),
                server: None,
                sent: Vec::new(),
                killed: None,
                descendants: None,
                wait_elapsed: Box::new(Instant::elapsed),
            },
            master,
        )
    }

    pub fn start(&self, dir: &Path, recipe: bool) -> Process {
        let command = if recipe {
            self.recipe(dir)
        } else {
            self.command(dir)
        };
        let mut process = self.run(command);
        process.success();
        self.remember();
        process
    }

    pub fn remember(&self) {
        if let Some(record) = self.record()
            && front(record.port).is_some_and(|actual| {
                actual.pid == record.pid
                    && actual.port == record.port
                    && actual.root == self.main.to_str().unwrap()
            })
        {
            self.own(record.pid);
        }
    }

    /// Own the server on the record for cleanup, whichever registered project started it.
    pub fn adopt(&self) {
        if let Some(record) = self.record()
            && front(record.port).is_some_and(|actual| actual.pid == record.pid)
        {
            self.own(record.pid);
        }
    }

    /// Own the shared server `pid`, which just answered as itself, for cleanup.
    fn own(&self, pid: i64) {
        write(&self.base.join(format!("owned.{pid}")), "");
        self.detached.lock().unwrap().push(killed::Owned::of(pid));
    }

    pub fn record(&self) -> Option<symphony_server::home::Running> {
        serde_json::from_str(&fs::read_to_string(self.home.join("server.json")).ok()?).ok()
    }

    /// `path` of the project the one server on `port` was started by, under its `/<id>/`.
    pub fn page(&self, port: u16, path: &str) -> String {
        let front = status(port);
        format!(
            "/{}{path}",
            front.projects.first().expect("a project is served")
        )
    }

    pub fn check_server(&self) -> symphony_api::Front {
        let record = self.record().expect("shared address record");
        let actual = status(record.port);
        assert!(actual.ready);
        assert_eq!(actual.pid, record.pid);
        assert_eq!(actual.root, self.main.to_str().unwrap());
        assert_eq!(actual.port, record.port);
        assert_eq!(actual.app, None, "the native server has no interpreter app");
        actual
    }

    pub fn private(&self, root: &Path) -> Process {
        let mut command = self.command(&self.tree);
        command.args([
            "--root",
            root.to_str().unwrap(),
            "--port",
            "0",
            "--host",
            "127.0.0.1",
        ]);
        self.run(command)
    }

    pub fn stop(&self) {
        let mut command = self.command(&self.main);
        command.arg("--stop");
        self.run(command).success();
    }

    pub fn build_events(&self) -> Vec<String> {
        fs::read_to_string(&self.builds)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Exercise process cleanup while keeping scratch diagnostics through the caller's assertions.
    pub fn cleanup(&self) {
        self.remember();
        // Only pids recorded by this fixture's own scratch home are eligible for cleanup.
        if !self.detached.lock().unwrap().is_empty() {
            let mut command = self.command(&self.main);
            command.arg("--stop");
            let _ = self.run(command).wait();
        }
        // A server the stop ended is gone, and its pid may name a process started since: each is
        // signalled only while its pid still names the server taken.
        for server in self.detached.lock().unwrap().iter() {
            if server.alive() {
                server.signal(libc::SIGKILL, "a case's cleanup: a shared server its stop left");
            }
            wait_cleanup(
                || format!("owned shared server {} to exit", server.pid),
                || Ok(!server.alive()),
                Instant::elapsed,
            );
        }
    }

    /// Once no server or command of this case runs, refuse every later stand-in and wait for
    /// those that started to end ([`StandIns`]).
    fn close_stand_ins(&self) {
        // Also run while unwinding, when an earlier panic may have poisoned the lock.
        self.stand_ins
            .lock()
            .recover()
            .close();
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        self.cleanup();
        owner_stage("repository cleanup");
        for descendants in self.descendants.lock().unwrap().iter_mut() {
            descendants.wait();
        }
        owner_stage("descendant waits");
        self.close_stand_ins();
        owner_stage("stand-in close");
        profile_warm(&fs::read_to_string(self.home.join("server.log")).unwrap_or_default());
    }
}

/// A FIFO at `path`, held open for reading and writing: a fixture command's read of it waits for a
/// line this test writes, and ends at EOF once the test drops it, however the test ends.
pub fn fifo(path: &Path) -> File {
    mkfifo(path);
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a live, terminated path; the FIFO belongs to this case's scratch folder.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

/// A terminal only on stdout: no controlling terminal, focus, or real browser is involved.
pub fn terminal() -> (File, OwnedFd) {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: valid output pointers, default settings and no name buffer. Own both returned fds.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // openpty sets no close-on-exec, so every child another case spawned would keep the terminal
    // for its whole life. No case waits for its end: the moment before the flag holds nothing back.
    for fd in [master, slave] {
        // SAFETY: F_SETFD changes only the flags of a descriptor openpty just returned.
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            -1
        );
    }
    // SAFETY: openpty returned this fresh descriptor and ownership is transferred once.
    let master = unsafe { File::from_raw_fd(master) };
    // SAFETY: openpty returned this other fresh descriptor, transferred once.
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    (master, slave)
}
