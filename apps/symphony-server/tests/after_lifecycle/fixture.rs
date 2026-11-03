//! Owned processes, scratch repositories and build/opener fixtures for the lifecycle boundary.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::symlink;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

#[path = "../../../../../tools/gate/tests/gate/scratch.rs"]
mod scratch;

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod scratch_tests;

pub const APP: &str = env!("CARGO_BIN_EXE_symphony-server");
pub const PATIENCE: Duration = Duration::from_secs(30);
pub const AGENTS: [&str; 3] = ["TASKS_SESSION", "CODEX_THREAD_ID", "CLAUDECODE"];

const SCRATCH_PREFIX: &str = "after-lifecycle-";

/// Validate the freshly linked executable once before measuring server starts. The OS's first
/// image launch is setup; every server still has the unchanged patience for its actual startup.
/// Keep the completed child's identity so reuse is observable without a timing threshold.
static PREPARED_APP: OnceLock<u32> = OnceLock::new();
static PREPARED_COMMANDS: OnceLock<[u32; 4]> = OnceLock::new();

fn prepare_app(log: &Path) -> u32 {
    *PREPARED_APP.get_or_init(|| {
        let mut command = Command::new(APP);
        command.arg("--help").env_clear().stdin(Stdio::null());
        let mut preparation = Process::spawn(command, log);
        preparation.success();
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
        let environment = repo.command(&repo.main);
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
                .stdin(Stdio::null())
                .args(args);
            for (key, value) in environment.get_envs() {
                if let Some(value) = value {
                    command.env(key, value);
                }
            }
            let mut preparation = Process::spawn(command, &folder.join(name));
            preparation.success();
            preparation.child.id()
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
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../artifacts/after_lifecycle");
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        REAP.call_once(|| scratch::reap_dead_scratch(&root, SCRATCH_PREFIX));
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
    }
}

pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn command_fixture(path: &Path, name: &str) {
    // Fresh executable scripts incur first-exec validation too. Keep one immutable image for
    // every scratch checkout; $0 still names the invoking fixture path (ps/open use it).
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/after_lifecycle")
        .join(name);
    symlink(source, path).unwrap();
}

pub fn repository(root: &Path, args: &[&str]) {
    let done = tasks::sys::output(
        tasks::sys::Command::new("git")
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

/// This polls an observable event, never a presumed startup duration.
pub fn until<T>(why: &str, event: impl FnMut() -> Option<T>) -> T {
    poll(PATIENCE, || why.into(), event, Instant::elapsed)
}

fn poll<T>(
    patience: Duration,
    why: impl FnOnce() -> String,
    event: impl FnMut() -> Option<T>,
    elapsed: impl FnMut(&Instant) -> Duration,
) -> T {
    poll_for(patience, event, elapsed).unwrap_or_else(|| panic!("timed out: {}", why()))
}

fn poll_for<T>(
    patience: Duration,
    mut event: impl FnMut() -> Option<T>,
    mut elapsed: impl FnMut(&Instant) -> Duration,
) -> Option<T> {
    let began = Instant::now();
    loop {
        if let Some(value) = event() {
            return Some(value);
        }
        if elapsed(&began) >= patience {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Cleanup still fails a passing case, but preserves an original failure and its other reports.
fn wait_cleanup(
    why: impl FnOnce() -> String,
    mut finished: impl FnMut() -> Result<bool, String>,
    elapsed: impl FnMut(&Instant) -> Duration,
) {
    let outcome = poll_for(
        PATIENCE,
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
        None => format!("timed out: {}", why()),
    };
    if std::thread::panicking() {
        eprintln!("{message}");
    } else {
        panic!("{message}");
    }
}

pub fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200))
            .ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
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

pub fn front(port: u16) -> Option<symphony_api::Front> {
    let (status, body) = get(port, "/api/front")?;
    (status == 200)
        .then(|| serde_json::from_str(&body).ok())
        .flatten()
}

/// Retains and reaps its exact child even if a test assertion unwinds.
pub struct Process {
    pub child: Child,
    command: String,
    executable: Executable,
    launch: String,
    stdout: PathBuf,
    stderr: PathBuf,
    server_pid: Option<i64>,
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
/// stopped using scratch, including subprocesses of a census shell cancelled by the server.
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
        let fd = writer.as_raw_fd();
        // SAFETY: writer lives through spawn. Only the child's descriptor flag changes, so
        // concurrent tests cannot inherit this endpoint through their unrelated commands.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
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
            stdout,
            stderr,
            server_pid: None,
            descendants: Some(descendants),
            wait_elapsed: Box::new(Instant::elapsed),
        }
    }

    pub fn output(&self) -> String {
        output(&self.stdout, &self.stderr)
    }

    pub fn wait(&mut self) -> ExitStatus {
        let child = &mut self.child;
        let id = child.id();
        let stdout = &self.stdout;
        let stderr = &self.stderr;
        let launch = &self.launch;
        let status = poll(
            PATIENCE,
            || format!("child {id} to exit; {launch}: {}", output(stdout, stderr)),
            || child.try_wait().unwrap(),
            &mut self.wait_elapsed,
        );
        if let Some(pid) = self.server_pid {
            until("private server to exit after its launcher", || {
                (!symphony_server::sys::pid_alive(pid)).then_some(())
            });
        }
        self.server_pid = None;
        status
    }

    pub fn success(&mut self) {
        let status = self.wait();
        let output = self.output();
        let identity = if status.signal().is_some() && output.is_empty() {
            self.executable.failure()
        } else {
            String::new()
        };
        assert!(
            status.success(),
            "child {} ({}) failed with {status}{identity}: {output}",
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
            PATIENCE,
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
        self.server_pid = Some(actual.pid);
        actual
    }

    pub fn signal(&self, signal: i32) {
        symphony_server::sys::signal(
            self.server_pid.unwrap_or(i64::from(self.child.id())),
            signal,
        );
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
    if std::env::var_os("SF_SYMPHONY_PROFILE").is_some() {
        eprintln!(
            "{}: {report}",
            std::thread::current().name().unwrap_or("lifecycle")
        );
    }
    report
}

impl Drop for Process {
    fn drop(&mut self) {
        let server = self.server_pid.take();
        if let Some(pid) = server {
            symphony_server::sys::signal(pid, libc::SIGKILL);
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(pid) = server {
            wait_cleanup(
                || format!("owned private server {pid} to exit"),
                || Ok(!symphony_server::sys::pid_alive(pid)),
                Instant::elapsed,
            );
        }
        if let Some(descendants) = &mut self.descendants {
            descendants.wait();
        }
        profile_warm(&self.output());
        if self.command.starts_with("\"just\"") && std::env::var_os("SF_SYMPHONY_PROFILE").is_some()
        {
            eprintln!("{}\n{}", self.launch, self.output());
        }
    }
}

fn profile_warm(output: &str) {
    if std::env::var_os("SF_SYMPHONY_PROFILE").is_some() {
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
    pub home: PathBuf,
    /// A port the test fixed itself (it holds a listener there); otherwise the server picks one.
    pub port: Option<u16>,
    pub bin: PathBuf,
    pub builds: PathBuf,
    detached: Mutex<Vec<i64>>,
    descendants: Mutex<Vec<Descendants>>,
    next: AtomicUsize,
    _scratch: Scratch,
}

impl Repo {
    pub fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        prepare_app(&scratch.path.join("prepare-app"));
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
        let recipes = include_str!("../../../../../justfile");
        let declaration = recipes.find("\nsymphony *ARGS:").unwrap();
        let start = recipes[..declaration].rfind("\n\n").unwrap();
        // Stop before the next production recipe's commentary; _symphony-build stays verbatim too.
        let docs = recipes[start..]
            .split("# The review board:")
            .next()
            .unwrap();
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
        let home = main.join(".git/sf-symphony");
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
            base,
            main,
            tree,
            home,
            port: None,
            bin,
            detached: Mutex::new(Vec::new()),
            descendants: Mutex::new(Vec::new()),
            next: AtomicUsize::new(0),
            _scratch: scratch,
        };
        prepare_commands(&repo, &repo.base.join("prepare-commands"));
        repo
    }

    pub fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new(APP);
        command
            .current_dir(dir)
            .env_clear()
            .env("HOME", &self.base)
            .env("USER", "lifecycle-fixture")
            .env("LANG", "C")
            .env(
                "PATH",
                format!(
                    "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
                    self.bin.display()
                ),
            )
            .env("SF_SYMPHONY_PORT", self.port.unwrap_or(0).to_string())
            .env("SF_SYMPHONY_TEST_PID", std::process::id().to_string())
            // SF_SYMPHONY_STARTUP_TRACE=1 keeps starter stages in timeout reports. Stop and
            // serving commands stay quiet even when the same fixture environment asks for it.
            .env("SF_SYMPHONY_STARTUP_TRACE", "1")
            // Foreground previews must not discover any surrounding repository's live data.
            // An invalid database name is refused before connecting or starting a board engine.
            .env("TASKS_DATABASE", "lifecycle fixture has no board")
            .env("ASKS_DB", self.base.join("private-asks.db"))
            .env("SF_LIFECYCLE_BINARY", APP)
            .env("SF_LIFECYCLE_LOG", &self.builds)
            .stdin(Stdio::null());
        // Keep the coverage collector on foreground children. Home must still exercise its real
        // environment filter; its detached child writes LLVM's default profile in the scratch cwd.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
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

    pub fn recipe(&self, dir: &Path) -> Command {
        let command = self.command(dir);
        // Command cannot replace its program: apply the same isolated environment explicitly.
        let mut recipe = Command::new("just");
        recipe
            .current_dir(dir)
            .args(["symphony"])
            .stdin(Stdio::null());
        recipe.env_clear();
        recipe
            .env("JUST_VERBOSE", "1")
            .env("JUST_TIMESTAMP", "true")
            .env("JUST_TIMESTAMP_FORMAT", "%s%.3f")
            .env("JUST_TIME", "true");
        for (key, value) in command.get_envs() {
            if let Some(value) = value {
                recipe.env(key, value);
            }
        }
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
                stdout,
                stderr,
                server_pid: None,
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
            write(&self.base.join(format!("owned.{}", record.pid)), "");
            self.detached.lock().unwrap().push(record.pid);
        }
    }

    pub fn record(&self) -> Option<symphony_server::home::Running> {
        serde_json::from_str(&fs::read_to_string(self.home.join("server.json")).ok()?).ok()
    }

    pub fn check_server(&self) -> symphony_api::Front {
        let record = self.record().expect("shared address record");
        let actual = front(record.port).expect("native status");
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
        for &pid in self.detached.lock().unwrap().iter() {
            if symphony_server::sys::pid_alive(pid) {
                symphony_server::sys::signal(pid, libc::SIGKILL);
            }
            wait_cleanup(
                || format!("owned shared server {pid} to exit"),
                || Ok(!symphony_server::sys::pid_alive(pid)),
                Instant::elapsed,
            );
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        self.cleanup();
        for descendants in self.descendants.lock().unwrap().iter_mut() {
            descendants.wait();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recipe_observer_uses_the_interpreter_from_the_original_path() {
        let repo = Repo::new("recipe-interpreter");
        let original = repo.base.join("original-path");
        fs::create_dir(&original).unwrap();
        // An immutable executable distinguishes PATH selection from a hard-coded /bin/bash:
        // false exits 1, whereas bash -c true exits 0.
        symlink("/usr/bin/false", original.join("bash")).unwrap();
        let mut command = Command::new(repo.bin.join("bash"));
        command
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    repo.bin.display(),
                    original.display()
                ),
            )
            .args(["-c", "true"]);
        let mut observer = repo.run(command);
        assert_eq!(observer.wait().code(), Some(1), "{}", observer.output());
    }

    #[test]
    fn a_recipe_timeout_reports_runner_bash_and_the_pending_build_command() {
        use std::os::unix::ffi::OsStrExt;

        struct ReleaseLog(PathBuf);
        impl Drop for ReleaseLog {
            fn drop(&mut self) {
                // An earlier assertion may unwind before the normal release. Unblock any
                // opening writer before Repo waits on descendants, including on that path.
                let _ = fs::OpenOptions::new().read(true).write(true).open(&self.0);
            }
        }

        let repo = Repo::new("recipe-stage-timeout");
        // The build's real log is a FIFO with no reader yet. Its opening writer waits here,
        // giving the report a stable pending recipe stage without spending the wall deadline.
        let name = std::ffi::CString::new(repo.builds.as_os_str().as_bytes()).unwrap();
        // SAFETY: a terminated path in this test's owned scratch folder.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let _release_on_failure = ReleaseLog(repo.builds.clone());
        let mut starter = repo.run(repo.recipe(&repo.tree));
        until("recipe's pending UI build", || {
            let said = starter.output();
            (said.contains("tools/fixture-build ui ")
                && !said.contains("---> symphony-ui-dist completed"))
            .then_some(())
        });
        starter.wait_elapsed = Box::new(|_| PATIENCE);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| starter.wait()));
        // Release the real writer, then let the same start finish and own its server normally.
        let _release = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&repo.builds)
            .unwrap();
        starter.wait_elapsed = Box::new(Instant::elapsed);
        starter.success();
        repo.remember();
        repo.check_server();

        let failure = failure.expect_err("the pending build must fail the exit wait");
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.contains(&starter.launch), "{message}");
        assert!(
            message.contains("script executable=no"),
            "bash must read the recipe without executing a fresh temporary image: {message}"
        );
        for stage in [
            "===> running recipe `symphony`...",
            "· symphony recipe bash entered at ",
            "+ symphony recipe ",
            "_symphony-build",
            "===> running recipe `symphony-ui-dist`...",
            "tools/fixture-build ui ",
        ] {
            assert!(
                message.contains(stage),
                "missing recipe stage {stage}: {message}"
            );
        }
        assert!(
            !message.contains("---> symphony-ui-dist completed"),
            "{message}"
        );
        assert!(!message.contains("start command entered at"), "{message}");
    }

    #[test]
    fn ordinary_starts_and_traced_stop_and_serve_commands_have_no_startup_trace() {
        let repo = Repo::new("quiet-startup");
        let mut command = repo.command(&repo.main);
        command.env_remove("SF_SYMPHONY_STARTUP_TRACE");
        let mut starter = repo.run(command);
        starter.success();
        repo.remember();
        repo.check_server();
        assert_eq!(fs::read_to_string(&starter.stderr).unwrap(), "");
        let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
        assert!(!log.contains("· Symphony startup "), "{log}");

        let mut command = repo.command(&repo.main);
        command.arg("--stop");
        let mut stop = repo.run(command);
        stop.success();
        assert_eq!(fs::read_to_string(&stop.stderr).unwrap(), "");

        // Pass the trace variable to serve directly, rather than relying on Home's filter.
        let mut command = repo.command(&repo.main);
        command.args([
            "serve",
            "--root",
            repo.main.to_str().unwrap(),
            "--port",
            "0",
            "--host",
            "127.0.0.1",
        ]);
        let mut server = repo.run(command);
        server.ready();
        let said = server.output();
        assert!(!said.contains("· Symphony startup "), "{said}");
    }

    #[test]
    fn a_starter_timeout_keeps_the_requested_pending_stage() {
        let repo = Repo::new("traced-startup-timeout");
        fs::create_dir_all(&repo.home).unwrap();
        let lock = File::create(repo.home.join("lock")).unwrap();
        lock.lock().unwrap();
        let mut starter = repo.run(repo.command(&repo.main));
        until("starter waiting on its startup lock", || {
            starter
                .output()
                .contains("begin startup lock")
                .then_some(())
        });
        starter.wait_elapsed = Box::new(|_| PATIENCE);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| starter.wait()));
        let launch = starter.launch.clone();
        drop(starter);
        let failure = failure.expect_err("the held startup lock keeps the starter pending");
        let said = failure.downcast_ref::<String>().unwrap();
        assert!(said.contains(&launch), "{said}");
        assert!(said.contains("end checkout discovery:"), "{said}");
        assert!(said.contains("begin startup lock"), "{said}");
        assert!(!said.contains("end startup lock:"), "{said}");
        assert!(!said.contains("begin spawn detached server"), "{said}");
    }

    #[test]
    fn fixture_repositories_share_one_completed_app_and_command_preparation() {
        let first = Repo::new("prepared-first");
        let pid = *PREPARED_APP
            .get()
            .expect("Repo::new prepares the real image");
        assert!(pid > 0);
        let second = Repo::new("prepared-second");
        assert_eq!(prepare_app(&first.log("unused-preparation")), pid);
        assert_eq!(prepare_app(&second.log("unused-preparation")), pid);
        let commands = *PREPARED_COMMANDS
            .get()
            .expect("Repo::new prepares every fixture command image");
        assert!(commands.iter().all(|pid| *pid > 0));
        // A second preparation would create its owned process logs at one of these paths.
        for repo in [&first, &second] {
            let unused = repo.base.join("unused-command-preparation");
            assert_eq!(prepare_commands(repo, &unused), commands);
            assert!(
                !unused.exists(),
                "prepared command images must not launch again"
            );
            assert!(
                !repo.base.join("unused-preparation-0.out").exists(),
                "a prepared image must not be launched again by later fixtures"
            );
        }
    }

    #[test]
    fn a_readiness_timeout_names_its_child_and_reports_late_open_stage() {
        let scratch = Scratch::new("readiness-timeout");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "read -r line; printf 'startup stdout'; printf '\\n· review timing start: hub branch trees\\n' >&2; read -r line",
            ])
            .stdin(Stdio::piped());
        let mut process = Process::spawn(command, &scratch.path.join("startup"));
        let pid = process.child.id();
        let stdout = process.stdout.clone();
        let stderr = process.stderr.clone();
        let mut input = process.child.stdin.take().unwrap();
        process.wait_elapsed = Box::new(move |_| {
            writeln!(input, "report progress").unwrap();
            until("startup output", || {
                (output(&stdout, &stderr)
                    == "startup stdout\n· review timing start: hub branch trees\n")
                    .then_some(())
            });
            PATIENCE
        });
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.ready()));
        drop(process);
        let failure = failure.expect_err("a child without a readiness event must time out");
        assert_eq!(
            failure.downcast_ref::<String>().unwrap(),
            &format!(
                "timed out: native server readiness for child {pid}: startup stdout\n· review timing start: hub branch trees\n"
            )
        );
    }

    #[test]
    fn a_descendant_that_keeps_its_endpoint_fails_with_its_launch_identity() {
        let scratch = Scratch::new("descendant-timeout");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf ready; read -r release"])
            .stdin(Stdio::piped());
        let mut process = Process::spawn(command, &scratch.path.join("held"));
        until("descriptor holder readiness", || {
            (process.output() == "ready").then_some(())
        });
        let pid = process.child.id();
        process.descendants.as_mut().unwrap().wait_elapsed = Box::new(|_| PATIENCE);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process.descendants.as_mut().unwrap().wait();
        }));
        // The clock reaches the unchanged deadline while this exact child still holds its end.
        // Drop kills the child and waits on these same Descendants: give them the real clock back,
        // so an EOF trailing the reap (a concurrent fork's copy of the endpoint) is waited for.
        process.descendants.as_mut().unwrap().wait_elapsed = Box::new(Instant::elapsed);
        drop(process);
        let failure = failure.expect_err("a live holder must fail the bounded EOF wait");
        assert_eq!(
            failure.downcast_ref::<String>().unwrap(),
            &format!(
                "timed out: descendants of child {pid} (\"/bin/sh\" [\"-c\", \"printf ready; read -r release\"]) to exit before scratch removal"
            )
        );
    }

    #[test]
    fn cleanup_waits_for_orphaned_fixture_descendants_before_removing_scratch() {
        for repository in [false, true] {
            let repo = Repo::new("descendant-cleanup");
            let base = repo.base.clone();
            let mut command = Command::new("/bin/sh");
            // The launcher exits while its child holds the lifetime endpoint and waits for the
            // test's input. The child's final write must finish before scratch can disappear.
            command
                .args([
                    "-c",
                    "exec 3<&0; sh -c 'printf ready; read -r line; printf finished > \"$1\"' fixture \"$1\" <&3 & exit",
                    "fixture",
                ])
                .arg(base.join("late-write"))
                .stdin(Stdio::piped());
            let mut process = if repository {
                repo.run(command)
            } else {
                Process::spawn(command, &repo.log("orphan"))
            };
            let mut input = process.child.stdin.take().unwrap();
            process.success();
            until("orphaned fixture readiness", || {
                (process.output() == "ready").then_some(())
            });
            let (finished, completion) = std::sync::mpsc::channel();
            let cleanup = std::thread::spawn(move || {
                drop(process);
                drop(repo);
                finished.send(()).unwrap();
            });
            // A bounded negative observation, not cleanup's synchronization. Input releases the
            // descendant; EOF on its inherited endpoint is what releases cleanup.
            let early = completion.recv_timeout(Duration::from_millis(50));
            let scratch_present = base.is_dir();
            writeln!(input, "finish").unwrap();
            completion.recv_timeout(PATIENCE).unwrap();
            cleanup.join().unwrap();
            assert_eq!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
            assert!(
                scratch_present,
                "scratch removed while a descendant could write"
            );
            assert!(!base.exists());
        }
    }

    /// Another case's fixture command can be spawned while this one makes its endpoint. Racing
    /// children live on, so a writer one took would hold back that endpoint's EOF until cleanup.
    #[test]
    fn a_spawn_racing_endpoint_creation_does_not_inherit_the_endpoint() {
        const RACERS: usize = 16;
        let scratch = Scratch::new("racing-spawn");
        let start = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let spawner = scope.spawn(|| {
                start.wait();
                (0..RACERS)
                    .map(|index| {
                        // cat waits on its input until its Process is dropped.
                        let mut command = Command::new("/bin/cat");
                        command.stdin(Stdio::piped());
                        Process::spawn(command, &scratch.path.join(format!("racer-{index}")))
                    })
                    .collect::<Vec<_>>()
            });
            start.wait();
            for made in 0.. {
                let (mut endpoint, writer) = Descendants::endpoint(
                    &scratch.path.join(format!("endpoint-{made}")),
                    format!("racing endpoint {made}"),
                );
                drop(writer);
                // A fork between its own fork and exec holds the writer only until that exec.
                endpoint.wait();
                if spawner.is_finished() {
                    break;
                }
            }
            for racer in &mut spawner.join().unwrap() {
                assert!(
                    racer.child.try_wait().unwrap().is_none(),
                    "racer {} ended before every endpoint was observed",
                    racer.child.id()
                );
            }
        });
    }

    #[test]
    fn unsuccessful_children_report_their_pid_command_status_and_output() {
        let scratch = Scratch::new("unsuccessful-child");
        for (index, (script, status, output)) in [
            ("exit 7", "exit status: 7", ""),
            (
                "printf 'child stdout'; printf 'child stderr' >&2; exit 9",
                "exit status: 9",
                "child stdoutchild stderr",
            ),
            ("kill -TERM $$", "signal: 15", ""),
            (
                "printf 'signal stdout'; kill -TERM $$",
                "signal: 15",
                "signal stdout",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            let mut process = Process::spawn(command, &scratch.path.join(index.to_string()));
            let pid = process.child.id();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process.success();
            }));
            let failure = result.expect_err("unsuccessful child must fail success()");
            let message = failure.downcast_ref::<String>().unwrap();
            assert!(message.starts_with(&format!("child {pid} (\"/bin/sh\" [\"-c\", ")));
            assert!(message.contains(script), "{message}");
            assert!(message.contains(status), "{message}");
            assert!(message.ends_with(&format!(": {output}")), "{message}");
            if status == "signal: 15" && output.is_empty() {
                let meta = fs::metadata("/bin/sh").unwrap();
                let identity = format!(
                    "inode={} mtime={}.{:09}",
                    meta.ino(),
                    meta.mtime(),
                    meta.mtime_nsec()
                );
                assert!(
                    message.contains(&format!("at spawn: {identity}; at failure: {identity}")),
                    "{message}"
                );
            } else {
                assert!(!message.contains("at spawn:"), "{message}");
            }
        }
    }

    #[test]
    fn a_silent_signal_reports_the_original_and_current_executable_identity() {
        use std::os::unix::fs::PermissionsExt;
        let script = "#!/bin/sh\nprintf ready > \"$1\"\nread -r release\nkill -TERM $$\n";
        for removed in [false, true] {
            let scratch = Scratch::new("signal-image");
            let executable = scratch.path.join("child");
            let entered = scratch.path.join("entered");
            write(&executable, script);
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
            File::open(&executable)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1000))
                .unwrap();
            let before = fs::metadata(&executable).unwrap();
            let mut command = Command::new(&executable);
            command.arg(&entered).stdin(Stdio::piped());
            let mut child = Process::spawn(command, &scratch.path.join("process"));
            until("silent child entered its input wait", || {
                (fs::read_to_string(&entered).ok().as_deref() == Some("ready")).then_some(())
            });
            // Replace/unlink the path, never the running image's bytes. Input releases the owned
            // child only after the new path state is established; no startup timing is assumed.
            let after = if removed {
                fs::remove_file(&executable).unwrap();
                None
            } else {
                let replacement = scratch.path.join("replacement");
                write(&replacement, script);
                fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
                File::open(&replacement)
                    .unwrap()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(2000))
                    .unwrap();
                fs::rename(&replacement, &executable).unwrap();
                Some(fs::metadata(&executable).unwrap())
            };
            writeln!(child.child.stdin.as_mut().unwrap(), "release").unwrap();
            let failure =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child.success()))
                    .expect_err("a signalled child must still fail success()");
            let message = failure.downcast_ref::<String>().unwrap();
            assert!(message.contains("signal: 15"), "{message}");
            assert!(
                message.contains(&format!("executable {executable:?}")),
                "{message}"
            );
            assert!(
                message.contains(&format!(
                    "at spawn: inode={} mtime=1000.000000000",
                    before.ino()
                )),
                "{message}"
            );
            if let Some(after) = after {
                assert_ne!(before.ino(), after.ino());
                assert!(
                    message.contains(&format!(
                        "at failure: inode={} mtime=2000.000000000",
                        after.ino()
                    )),
                    "{message}"
                );
            } else {
                assert!(message.contains("at failure: unavailable:"), "{message}");
            }
            assert!(child.output().is_empty());
        }
    }

    #[test]
    fn timed_out_wait_reports_output_written_after_polling_begins() {
        assert_eq!(
            poll(
                Duration::ZERO,
                || panic!("successful waits need no diagnostic"),
                || Some(7),
                Instant::elapsed,
            ),
            7
        );
        let scratch = Scratch::new("wait-timeout");
        let logs = scratch.path.join("timeout");
        let mut command = Command::new("/bin/sh");
        // The first input releases both writes; the second read keeps this exact child pending.
        command
            .args([
                "-c",
                r"read -r line; printf 'late stdout\n'; printf 'late stderr\n' >&2; read -r line",
            ])
            .stdin(Stdio::piped());
        let mut process = Process::spawn(command, &logs);
        let id = process.child.id();
        assert!(process.child.try_wait().unwrap().is_none());
        assert!(process.output().is_empty());
        let stdout = process.stdout.clone();
        let stderr = process.stderr.clone();
        let mut input = process.child.stdin.take().unwrap();
        process.wait_elapsed = Box::new(move |_: &Instant| {
            // The clock is read after Process::wait has begun and observed its pending child.
            // Release and observe real child output, then reach the unchanged wait deadline.
            writeln!(input, "emit").unwrap();
            until("late child stdout and stderr", || {
                (output(&stdout, &stderr) == "late stdout\nlate stderr\n").then_some(())
            });
            PATIENCE
        });
        // The expected timeout leaves Process owning its child, which is cleaned up below.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.wait()));
        let launch = process.launch.clone();
        drop(process);
        let timeout = result.unwrap_err();
        let message = timeout.downcast_ref::<String>().unwrap();
        assert_eq!(
            message,
            &format!("timed out: child {id} to exit; {launch}: late stdout\nlate stderr\n")
        );
    }
}
