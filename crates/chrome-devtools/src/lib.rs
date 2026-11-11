//! Headless Chrome, driven over the DevTools protocol: one browser per launch, on a profile of
//! its own, leashed so it dies with the process that launched it however that ends. The client's
//! browser tests (apps/symphony-ui/tests/browser) and `symphony shot` both drive it.
//!
//! A native leash owns and waits for Chrome: the launching program run again ([`Leash`]), which
//! hands itself to [`leash_main`] first. EOF on its launcher's pipe terminates the browser even
//! after SIGKILL, then removes its private profile. Startup failures use the same cleanup.
//!
//! Every wait is bounded by the browser's own work, not the wall: the CPU its recorded process
//! group spends, and the observation turns it completes (a scheduler delay spends nothing).

use std::collections::HashMap;
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

mod process;
mod profiles;
#[cfg(test)]
mod tests;

const POLL: Duration = Duration::from_millis(25);
const PROTOCOL_CPU: Duration = Duration::from_secs(60);
const CLEANUP_CPU: Duration = Duration::from_secs(1);

/// What went wrong driving Chrome, in one message that names what was waited for.
#[derive(Debug)]
pub struct Error {
    message: String,
    past_deadline: bool,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            past_deadline: false,
        }
    }

    fn deadline(what: &str) -> Self {
        Self {
            message: format!("{what}: no reply before the deadline"),
            past_deadline: true,
        }
    }

    /// Whether the wait ended because the deadline [`Chrome::set_deadline`] set had passed, rather
    /// than because Chrome failed.
    pub fn past_deadline(&self) -> bool {
        self.past_deadline
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Browser work and completed observations share one allowance. Socket timeouts and
/// scheduler delays spend nothing; an observer reply proves the browser had a turn. A caller's
/// deadline, when there is one, ends the wait too, however little the browser has spent.
struct WaitBudget {
    cpu: process::Budget,
    polls: u128,
    max_polls: u128,
    deadline: Option<Instant>,
}

impl WaitBudget {
    fn new(cpu: Duration, members: &[process::Member]) -> Self {
        Self {
            // Zero credit still accepts the initial result. That evaluation keeps
            // its protocol CPU ceiling; one observer turn can end an idle reply.
            cpu: process::Budget::new(if cpu.is_zero() { PROTOCOL_CPU } else { cpu }, members),
            polls: 0,
            max_polls: cpu.as_nanos().div_ceil(POLL.as_nanos()).max(1),
            deadline: None,
        }
    }

    fn until(self, deadline: Option<Instant>) -> Self {
        Self { deadline, ..self }
    }

    fn past_deadline(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn exhausted(&mut self, members: &[process::Member]) -> bool {
        self.past_deadline() || self.cpu.exhausted(members) || self.polls >= self.max_polls
    }

    fn poll(&mut self, members: &[process::Member]) -> bool {
        self.polls += 1;
        self.exhausted(members)
    }
}

const CHROMES: [&str; 4] = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "google-chrome",
    "chromium",
    "chrome",
];

/// The Chrome binary on this machine: `SF_CHROME`, else the first of `CHROMES` found.
pub fn find() -> Option<String> {
    if let Some(named) = std::env::var_os("SF_CHROME") {
        return Some(named.to_string_lossy().into_owned());
    }
    CHROMES.iter().find_map(|name| {
        if name.starts_with('/') {
            Path::new(name).exists().then(|| name.to_string())
        } else {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|full| full.exists())
                .map(|full| full.to_string_lossy().into_owned())
        }
    })
}

/// What one request cost, as Chrome saw it.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub url: String,
    pub kind: String,
    pub from_cache: bool,
    pub status: i64,
    /// Bytes on the wire, headers included: the finished total, or what arrived so far.
    pub bytes: f64,
    pub finished: bool,
    /// Why it failed (`net::ERR_…`), when it did; such a request never finishes.
    pub failed: Option<String>,
    /// The page itself gave it up (an abort, or the page going away), rather than the network.
    pub canceled: bool,
}

/// The variable a leash reads its browser and profile from, as JSON `[binary, profile]`.
const LEASH: &str = "SF_SYMPHONY_CHROME_LEASH";

/// How a launch starts the leash that owns its browser: the launching program run again with
/// `args`, a run that hands itself to [`leash_main`] before anything else. A test binary names
/// the test that does; a program's `main` asks first thing.
pub struct Leash {
    program: PathBuf,
    args: Vec<String>,
    reports: bool,
}

impl Leash {
    pub fn new(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            reports: true,
        }
    }

    /// Keep the leash's own reports (each shutdown, a slow cleanup) off the launcher's stderr.
    /// A failure still reaches the launcher, in the error its wait returns.
    #[must_use]
    pub fn quiet(mut self) -> Self {
        self.reports = false;
        self
    }
}

/// In a leash's run of its program: start the browser its launcher named, wait for the launcher's
/// pipe to end (or the browser to exit), then kill and reap the browser's whole group and remove
/// its profile. The exit code to end that run with; `None` in every other run.
pub fn leash_main() -> Option<i32> {
    let config = std::env::var(LEASH).ok()?;
    let Ok((binary, profile)) = serde_json::from_str::<(String, PathBuf)>(&config) else {
        leash_report(format_args!("Chrome leash: unreadable {LEASH} {config:?}"));
        return Some(2);
    };
    Some(leash(&binary, &profile))
}

fn leash(binary: &str, profile: &Path) -> i32 {
    let started = std::fs::File::create(profile.with_extension("stderr")).and_then(|stderr| {
        Command::new(binary)
            .args(chrome_args(profile))
            // Its singleton and fetch folders, which a killed Chrome leaves, go with the profile.
            .env("MAC_CHROMIUM_TMPDIR", profile)
            .env("TMPDIR", profile)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .process_group(0)
            .spawn()
    });
    let mut browser = match started {
        Ok(child) => BrowserGroup(child),
        Err(error) => {
            // The launcher still holds the logs open: it reads this reason after the leash ends.
            let reason = format!("browser did not start: {binary}: {error}");
            let _ = std::fs::write(profile.with_extension("exit"), &reason);
            leash_report(format_args!(
                "Chrome shutdown {}: {reason}",
                profile.display()
            ));
            let _ = std::fs::remove_dir_all(profile);
            let _ = std::fs::remove_file(profile.with_extension("exit"));
            let _ = std::fs::remove_file(profile.with_extension("stderr"));
            return 1;
        }
    };
    let child = &mut browser.0;
    if let Err(error) = std::fs::write(profile.join("browser.pid"), format!("{}\n", child.id())) {
        leash_report(format_args!(
            "Chrome browser.pid {}: {error}",
            profile.display()
        ));
    }
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        let _ = send.send(());
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                leash_report(format_args!("Chrome wait {}: {error}", profile.display()));
                break None;
            }
        }
        if !matches!(
            receive.recv_timeout(POLL),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            // EOF is the shutdown event. Kill and reap the owned group immediately;
            // a graceful-shutdown wall allowance says nothing about helper termination.
            break None;
        }
    };
    let reason = status.map_or_else(
        || "suite stdin closed".to_string(),
        |status| format!("browser exited: {status}"),
    );
    if let Err(error) = std::fs::write(profile.with_extension("exit"), &reason) {
        leash_report(format_args!(
            "Chrome shutdown record {}: {error}",
            profile.display()
        ));
    }
    leash_report(format_args!(
        "Chrome shutdown {}: {reason}",
        profile.display()
    ));
    let group = browser.0.id();
    drop(browser);
    let cleanup = cleanup_profile(profile, group);
    let _ = std::fs::remove_file(profile.with_extension("exit"));
    let _ = std::fs::remove_file(profile.with_extension("stderr"));
    if let Err(error) = cleanup {
        leash_report(format_args!(
            "Chrome cleanup {}: {error}",
            profile.display()
        ));
        return 101;
    }
    status.map_or(0, |status| status.code().unwrap_or(1))
}

fn chrome_args(profile: &Path) -> Vec<String> {
    [
        "--headless=new",
        "--mute-audio",
        "--disable-gpu",
        "--no-sandbox",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-background-networking",
        "--disable-background-timer-throttling",
        // Chrome otherwise runs a page it counts as in the background or covered at background
        // priority (macOS darwinbg), where a loaded machine starves its renderer for minutes.
        "--disable-renderer-backgrounding",
        "--disable-backgrounding-occluded-windows",
        "--disable-component-update",
        "--disable-sync",
        "--disable-features=Translate,OptimizationHints,MediaRouter",
        "--remote-debugging-port=0",
    ]
    .into_iter()
    .map(String::from)
    .chain([
        format!("--user-data-dir={}", profile.display()),
        "about:blank".into(),
    ])
    .collect()
}

/// The fixtures' descriptor closing (tools/fixture_process.sh), run before the leash starts: the
/// leash and its browser keep no pipe or socket their launcher's own parent handed it.
const CLOSE_FDS: &str = include_str!("../../../tools/fixture_process.sh");

struct Owner {
    leash: Child,
    profile: PathBuf,
    stderr: std::fs::File,
    shutdown: std::fs::File,
}

impl Owner {
    /// Read before teardown: the leash removes the profile even when Chrome exits itself.
    fn diagnostics(&mut self) -> String {
        use std::io::{Read, Seek, SeekFrom};

        let leash = self.leash.try_wait();
        let reason = (|| -> std::io::Result<String> {
            let mut file = self.shutdown.try_clone()?;
            file.rewind()?;
            let mut reason = String::new();
            file.read_to_string(&mut reason)?;
            Ok(if reason.is_empty() {
                "not recorded yet".into()
            } else {
                reason
            })
        })()
        .unwrap_or_else(|error| format!("unavailable: {error}"));
        let members = self.snapshot();
        let stderr = (|| -> std::io::Result<String> {
            let mut file = self.stderr.try_clone()?;
            let length = file.metadata()?.len();
            file.seek(SeekFrom::Start(length.saturating_sub(8_192)))?;
            let mut bytes = Vec::new();
            file.take(8_192).read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        })()
        .unwrap_or_else(|error| format!("unavailable: {error}"));
        format!(
            "leash={} status={leash:?}; shutdown={reason}; members={members:?}; Chrome stderr (last 8192 bytes): {stderr}",
            self.leash.id()
        )
    }

    fn members(&self) -> Result<Vec<process::Member>> {
        self.snapshot().map_err(|error| {
            Error::new(format!("read owned Chrome group's CPU and state: {error}"))
        })
    }

    fn snapshot(&self) -> std::io::Result<Vec<process::Member>> {
        let group = std::fs::read_to_string(self.profile.join("browser.pid"))
            .ok()
            .filter(|text| text.ends_with('\n'))
            .and_then(|text| text.trim().parse::<u32>().ok())
            .filter(|pid| *pid > 0)
            .unwrap_or(self.leash.id());
        process::members(group)
    }

    /// Success comes only from the handshake; exit and CPU exhaustion report broken startup.
    fn ready<T>(
        &mut self,
        what: &str,
        cpu: Duration,
        mut handshake: impl FnMut(&Path) -> Option<T>,
    ) -> std::io::Result<T> {
        let mut budget = process::Budget::new(cpu, &self.snapshot()?);
        loop {
            if let Some(value) = handshake(&self.profile) {
                return Ok(value);
            }
            if let Some(status) = self.leash.try_wait()? {
                return Err(std::io::Error::other(format!(
                    "Chrome's leash exited {status} before {what}"
                )));
            }
            if budget.exhausted(&self.snapshot()?) {
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    format!("Chrome spent {:?} CPU without {what}", budget.used()),
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    fn launch(binary: &str, leash: &Leash) -> Result<Self> {
        let profile = profiles::next_profile()
            .map_err(|error| Error::new(format!("Chrome's profile: {error}")))?;
        let made = |error: std::io::Error| {
            Error::new(format!("Chrome's profile {}: {error}", profile.display()))
        };
        std::fs::create_dir(&profile).map_err(made)?;
        // From here every failure leaves nothing: the profile and its logs go with this guard.
        let mut started = Unstarted {
            profile: &profile,
            armed: true,
        };
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700)).map_err(made)?;
        // Keep open readers after the leash unlinks these names. Diagnostics survive its
        // cleanup, while a suite killed before Drop leaves no log files behind.
        let log = |extension| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(profile.with_extension(extension))
                .map_err(made)
        };
        let stderr = log("stderr")?;
        let shutdown = log("exit")?;
        let leash = Command::new("bash")
            .args([
                "-c",
                &format!("{CLOSE_FDS}\nfixture_close_fds; exec \"$@\""),
                "browser-leash",
            ])
            .arg(&leash.program)
            .args(&leash.args)
            .env(LEASH, json!([binary, profile]).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(if leash.reports {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .process_group(0)
            .spawn()
            .map_err(|error| Error::new(format!("start Chrome under its leash: {error}")))?;
        started.armed = false;
        drop(started);
        Ok(Self {
            leash,
            profile,
            stderr,
            shutdown,
        })
    }
}

/// A launch's profile and logs until its leash has started: removed if the launch fails first.
struct Unstarted<'a> {
    profile: &'a Path,
    armed: bool,
}

impl Drop for Unstarted<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(self.profile);
            let _ = std::fs::remove_file(self.profile.with_extension("exit"));
            let _ = std::fs::remove_file(self.profile.with_extension("stderr"));
        }
    }
}

unsafe extern "C" {
    fn kill(pid: std::ffi::c_int, signal: std::ffi::c_int) -> std::ffi::c_int;
}

/// Only a positive, recorded child PID may identify the group this driver owns.
fn signal_group(group: u32, signal: std::ffi::c_int) -> std::io::Result<()> {
    let Ok(group) = std::ffi::c_int::try_from(group) else {
        return Err(std::io::Error::other(format!("{group} is no process id")));
    };
    if group <= 0 {
        return Err(std::io::Error::other(format!(
            "{group} is no process group"
        )));
    }
    // SAFETY: POSIX kill takes two integers; the negative recorded child PID selects only
    // the group created by process_group(0), never a name or a discovered process.
    if unsafe { kill(-group, signal) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

struct BrowserGroup(Child);

impl Drop for BrowserGroup {
    fn drop(&mut self) {
        // The main browser may already be reaped. Its helpers still belong to this recorded
        // group and must die before the leash finishes profile cleanup or exits.
        let _ = signal_group(self.0.id(), 9); // SIGKILL, on Darwin and Linux
        let _ = self.0.wait();
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        drop(self.leash.stdin.take());
        let _ = self.leash.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
        let _ = std::fs::remove_file(self.profile.with_extension("exit"));
        let _ = std::fs::remove_file(self.profile.with_extension("stderr"));
    }
}

/// The launcher's output reader may die before its leash. Diagnostics must never interrupt cleanup.
fn leash_report(message: fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

/// A killed helper can still be writing while the main browser has already been reaped.
/// Keep sweeping until the recorded group is gone AND its final profile removal succeeded:
/// even an earlier successful removal can race a helper recreating part of the profile.
fn cleanup_profile(profile: &Path, group: u32) -> std::io::Result<()> {
    let started = Instant::now();
    let mut attempts = 0;
    let mut budget = None;
    let mut last_report: Option<Instant> = None;
    loop {
        let report = last_report.is_none_or(|last| last.elapsed() >= Duration::from_secs(1));
        let gone = match signal_group(group, 0) {
            Err(error) if error.raw_os_error() == Some(3) => true, // ESRCH
            Ok(()) => false,
            // Darwin can report EPERM while the killed helpers await reaping.
            Err(error) if error.kind() == ErrorKind::PermissionDenied => false,
            Err(error) => return Err(error),
        };
        let removal = match std::fs::remove_dir_all(profile) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        };
        attempts += 1;
        if let Err(error) = &removal {
            if report || error.kind() != ErrorKind::DirectoryNotEmpty {
                leash_report(format_args!(
                    "Chrome cleanup {}: group={group}, gone={gone}, attempt={attempts}: {error}",
                    profile.display()
                ));
            }
            if error.kind() != ErrorKind::DirectoryNotEmpty {
                return removal;
            }
        }
        if gone && removal.is_ok() {
            if attempts > 1 {
                leash_report(format_args!(
                    "Chrome cleanup {}: group={group} gone, removed after {attempts} attempts ({:?})",
                    profile.display(),
                    started.elapsed()
                ));
            }
            return Ok(());
        }
        if attempts == 1 && !gone {
            leash_report(format_args!(
                "Chrome cleanup {}: group={group} still present after browser reap",
                profile.display()
            ));
        }
        if !gone {
            // Snapshot failures during reaping must not replace the authoritative ESRCH wait.
            match process::members(group) {
                Ok(members) => {
                    let budget =
                        budget.get_or_insert_with(|| process::Budget::new(CLEANUP_CPU, &members));
                    if budget.exhausted(&members) {
                        return Err(std::io::Error::new(
                            ErrorKind::TimedOut,
                            format!(
                                "Chrome cleanup group={group} spent {:?} CPU after SIGKILL; \
                                 surviving members: {members:?}",
                                budget.used()
                            ),
                        ));
                    }
                    if report {
                        leash_report(format_args!(
                            "Chrome cleanup group={group} awaiting ESRCH: {members:?}"
                        ));
                    }
                }
                Err(error) if report => {
                    leash_report(format_args!(
                        "Chrome cleanup group={group} snapshot: {error}"
                    ));
                }
                Err(_) => {}
            }
        }
        if report {
            last_report = Some(Instant::now());
        }
        // Killed members add no CPU while waiting for reaping. A missed live helper
        // spends the budget instead of keeping the leash alive forever.
        std::thread::sleep(POLL);
    }
}

pub struct Chrome {
    owner: Owner,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    session: Option<String>,
    /// The launcher's blank target survives navigation in the page under test.
    observer: Option<String>,
    /// Every event since the last `open`, in order.
    events: Vec<Value>,
    /// When each of `events` was read from the browser.
    arrivals: Vec<Instant>,
    /// When every call and wait must end by, if the caller set it.
    deadline: Option<Instant>,
}

/// A mid phone's screen, as the browser tests emulate one: 390x844 CSS pixels at 3x, mobile.
pub const PHONE: (u32, u32, u32) = (390, 844, 3);

impl Chrome {
    /// Chrome at `binary`, on a new profile, under a leash `leash` starts, with one blank page
    /// attached and its page and network events on.
    pub fn launch(binary: &str, leash: &Leash) -> Result<Chrome> {
        let mut owner = Owner::launch(binary, leash)?;
        let (port, path) = owner
            .ready("DevTools port readiness", PROTOCOL_CPU, devtools_endpoint)
            .map_err(|error| {
                Error::new(format!(
                    "Chrome DevTools readiness: {error}; {}",
                    owner.diagnostics()
                ))
            })?;
        let (socket, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}{path}"))
            .map_err(|error| Error::new(format!("connect to Chrome's DevTools: {error}")))?;
        let mut chrome = Chrome {
            owner,
            socket,
            next_id: 0,
            session: None,
            observer: None,
            events: Vec::new(),
            arrivals: Vec::new(),
            deadline: None,
        };
        let targets = chrome.browser_call("Target.getTargets", json!({}))?;
        let blank = targets["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|target| target["type"] == "page" && target["url"] == "about:blank")
            .and_then(|target| target["targetId"].as_str())
            .ok_or_else(|| Error::new(format!("Chrome's launch target is not in {targets}")))?
            .to_owned();
        let observer = chrome.browser_call(
            "Target.attachToTarget",
            json!({"targetId": blank, "flatten": true}),
        )?;
        chrome.observer = Some(session_id(&observer)?);
        let target = chrome.browser_call("Target.createTarget", json!({"url": "about:blank"}))?;
        let target_id = target["targetId"]
            .as_str()
            .ok_or_else(|| Error::new(format!("Target.createTarget: no target in {target}")))?
            .to_string();
        let attached = chrome.browser_call(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        )?;
        chrome.session = Some(session_id(&attached)?);
        chrome.call("Page.enable", json!({}))?;
        chrome.call("Network.enable", json!({}))?;
        Ok(chrome)
    }

    /// From now on, every call and wait here ends by `deadline` at the latest, whatever its own
    /// allowance of browser CPU and observation turns, failing with an error that says so
    /// ([`Error::past_deadline`]); `None` lifts it. The waits are otherwise as they were.
    pub fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// A wait's allowance: `cpu` of browser work and as many observation turns, and the deadline.
    fn budget(&self, cpu: Duration) -> Result<WaitBudget> {
        Ok(WaitBudget::new(cpu, &self.owner.members()?).until(self.deadline))
    }

    /// The leash's process id, which owns the browser.
    pub fn leash_id(&self) -> u32 {
        self.owner.leash.id()
    }

    /// The browser's private profile folder.
    pub fn profile(&self) -> &Path {
        &self.owner.profile
    }

    /// Emulate a mid phone on a tailnet link: CPU slowed 4x, 10 Mbit/s each way, 40 ms round trip,
    /// on its screen ([`Chrome::phone_screen`]).
    pub fn mid_phone(&mut self) -> Result<()> {
        self.call("Emulation.setCPUThrottlingRate", json!({"rate": 4}))?;
        let bytes_per_second = 10_000_000 / 8;
        self.call(
            "Network.emulateNetworkConditions",
            json!({"offline": false, "latency": 40, "downloadThroughput": bytes_per_second, "uploadThroughput": bytes_per_second}),
        )?;
        self.phone_screen()
    }

    /// A mid phone's screen ([`PHONE`]), without slowing its CPU or network.
    pub fn phone_screen(&mut self) -> Result<()> {
        let (width, height, scale) = PHONE;
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": scale, "mobile": true}),
        )?;
        Ok(())
    }

    /// Go to `url` and wait for its document to be parsed.
    pub fn open(&mut self, url: &str) -> Result<()> {
        self.forget_events();
        self.call("Page.navigate", json!({"url": url}))?;
        self.wait_event("Page.domContentEventFired", Duration::from_secs(60))
    }

    /// As [`Chrome::open`], within `allowance` (browser CPU and observation turns, as every wait
    /// here), and a page Chrome could not load at all (a refused connection, an unknown host) is
    /// an error naming why, rather than Chrome's error page.
    pub fn navigate(&mut self, url: &str, allowance: Duration) -> Result<()> {
        self.forget_events();
        let session = self.session.clone();
        let navigated =
            self.session_call_within("Page.navigate", json!({"url": url}), session, allowance)?;
        if let Some(error) = navigated["errorText"]
            .as_str()
            .filter(|error| !error.is_empty())
        {
            return Err(Error::new(format!("{url}: {error}")));
        }
        self.wait_event("Page.domContentEventFired", allowance)
    }

    /// The page's pixels as a PNG, `params` as Page.captureScreenshot takes them (a clip, the
    /// content beyond the viewport).
    pub fn screenshot(&mut self, params: Value) -> Result<Vec<u8>> {
        let mut params = params;
        params["format"] = json!("png");
        let shot = self.call("Page.captureScreenshot", params)?;
        let data = shot["data"].as_str().ok_or_else(|| {
            Error::new("Page.captureScreenshot: no picture in its reply".to_string())
        })?;
        data_encoding::BASE64
            .decode(data.as_bytes())
            .map_err(|error| Error::new(format!("Page.captureScreenshot: {error}")))
    }

    /// Evaluate `expression` in the page (awaiting a promise), its value as JSON.
    pub fn eval(&mut self, expression: &str) -> Result<Value> {
        let mut budget = self.budget(PROTOCOL_CPU)?;
        self.eval_bounded(expression, &mut budget)?.ok_or_else(|| {
            if budget.past_deadline() {
                return Error::deadline(expression);
            }
            Error::new(format!(
                "{expression}: no evaluation reply after {} completed observer polls and {:?} CPU",
                budget.polls,
                budget.cpu.used()
            ))
        })
    }

    fn eval_bounded(&mut self, expression: &str, budget: &mut WaitBudget) -> Result<Option<Value>> {
        let id = self.send(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
            self.session.clone(),
        )?;
        let mut marker = self.observer_poll()?;
        loop {
            if let Some(value) = self.read(true)? {
                if value["id"] == json!(id) {
                    if let Some(error) = value.get("error") {
                        return Err(Error::new(format!("{expression}: {error}")));
                    }
                    let result = &value["result"];
                    if let Some(exception) = result.get("exceptionDetails") {
                        return Err(Error::new(format!("{expression} threw: {exception}")));
                    }
                    return Ok(Some(result["result"]["value"].clone()));
                }
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, budget)?;
                    if budget.exhausted(&self.owner.members()?) {
                        return Ok(None);
                    }
                    marker = self.observer_poll()?;
                } else if value.get("id").is_none() {
                    self.keep(value);
                }
            }
            if budget.exhausted(&self.owner.members()?) {
                return Ok(None);
            }
        }
    }

    /// Wait until `expression` is truthy, bounded by browser CPU and completed polls,
    /// including observer turns while an evaluation's promise is still pending.
    pub fn wait_for(&mut self, expression: &str, cpu: Duration) -> Result<bool> {
        let mut budget = self.budget(cpu)?;
        loop {
            let Some(value) = self.eval_bounded(expression, &mut budget)? else {
                eprintln!(
                    "Chrome evaluation still pending after {} polls and {:?} CPU: {expression}",
                    budget.polls,
                    budget.cpu.used()
                );
                return Ok(false);
            };
            if !(value.is_null()
                || value == json!(false)
                || value == json!(0)
                || value == json!(""))
            {
                return Ok(true);
            }
            if budget.poll(&self.owner.members()?) {
                eprintln!(
                    "Chrome condition still false after {} polls and {:?} CPU: {expression}",
                    budget.polls,
                    budget.cpu.used()
                );
                return Ok(false);
            }
            self.pump(POLL)?;
        }
    }

    /// Observe protocol events while the real browser is paused at a fixture boundary.
    pub fn wait_for_protocol_events(
        &mut self,
        method: &str,
        count: usize,
        cpu: Duration,
    ) -> Result<Vec<Value>> {
        let mut budget = self.budget(cpu)?;
        loop {
            self.pump(POLL)?;
            let events: Vec<_> = self
                .events
                .iter()
                .filter(|event| event["method"] == method)
                .map(|event| event["params"].clone())
                .collect();
            if events.len() >= count || budget.poll(&self.owner.members()?) {
                return Ok(events);
            }
        }
    }

    /// Every protocol event since the last `open` or `navigate`, in order.
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    /// [`Chrome::events`], each with when it was read from the browser: no later than that, the
    /// browser sent it.
    pub fn events_read(&self) -> impl Iterator<Item = (Instant, &Value)> {
        self.arrivals.iter().copied().zip(&self.events)
    }

    fn keep(&mut self, event: Value) {
        self.events.push(event);
        self.arrivals.push(Instant::now());
    }

    fn forget_events(&mut self) {
        self.events.clear();
        self.arrivals.clear();
    }

    /// The requests seen since the last `open`, by Chrome's request id.
    pub fn requests(&mut self) -> Result<Vec<Request>> {
        self.pump(Duration::from_millis(50))?;
        Ok(requests(&self.events))
    }

    pub fn browser_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.session_call(method, params, None)
    }

    /// One DevTools command; its result. Events read meanwhile are kept.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.session_call(method, params, self.session.clone())
    }

    fn session_call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value> {
        self.session_call_within(method, params, session, PROTOCOL_CPU)
    }

    fn session_call_within(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
        allowance: Duration,
    ) -> Result<Value> {
        let mut budget = self.budget(allowance)?;
        let id = self.send(method, params, session)?;
        let mut marker = self.observer_poll()?;
        loop {
            if let Some(value) = self.read(true)? {
                if value["id"] == json!(id) {
                    if let Some(error) = value.get("error") {
                        return Err(Error::new(format!("{method}: {error}")));
                    }
                    return Ok(value["result"].clone());
                }
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, &mut budget)?;
                    if !budget.exhausted(&self.owner.members()?) {
                        marker = self.observer_poll()?;
                    }
                }
                if value.get("id").is_none() {
                    self.keep(value);
                }
            }
            if budget.exhausted(&self.owner.members()?) {
                if budget.past_deadline() {
                    return Err(Error::deadline(method));
                }
                return Err(Error::new(format!(
                    "{method}: no reply after {} completed observer polls and {:?} CPU",
                    budget.polls,
                    budget.cpu.used()
                )));
            }
        }
    }
}

/// The requests `events` tell of, in the order each was first seen, by Chrome's request id.
fn requests(events: &[Value]) -> Vec<Request> {
    let mut requests: HashMap<String, Request> = HashMap::new();
    let mut order = Vec::new();
    for event in events {
        let params = &event["params"];
        let Some(id) = params["requestId"].as_str() else {
            continue;
        };
        if !requests.contains_key(id) {
            order.push(id.to_string());
        }
        let request = requests.entry(id.to_string()).or_default();
        match event["method"].as_str().unwrap_or("") {
            "Network.requestWillBeSent" => {
                request.url = params["request"]["url"].as_str().unwrap_or("").to_string();
                request.kind = params["type"].as_str().unwrap_or("").to_string();
            }
            "Network.requestServedFromCache" => request.from_cache = true,
            "Network.responseReceived" => {
                request.status = params["response"]["status"].as_i64().unwrap_or(0);
                if params["response"]["fromDiskCache"].as_bool() == Some(true) {
                    request.from_cache = true;
                }
            }
            // CORS hides the response from Runtime/fetch and responseReceived. The wire
            // status still proves an opaque-origin request was refused before any write.
            "Network.responseReceivedExtraInfo" if request.status == 0 => {
                request.status = params["statusCode"].as_i64().unwrap_or(request.status);
            }
            "Network.dataReceived" if !request.finished => {
                request.bytes += params["encodedDataLength"].as_f64().unwrap_or(0.0);
            }
            "Network.loadingFinished" => {
                request.bytes = params["encodedDataLength"]
                    .as_f64()
                    .unwrap_or(request.bytes);
                request.finished = true;
            }
            "Network.loadingFailed" => {
                request.failed = Some(params["errorText"].as_str().unwrap_or("").to_string());
                request.canceled = params["canceled"].as_bool() == Some(true);
            }
            _ => {}
        }
    }
    order
        .into_iter()
        .filter_map(|id| requests.remove(&id))
        .filter(|request| !request.url.is_empty())
        .collect()
}

impl Chrome {
    fn send(&mut self, method: &str, params: Value, session: Option<String>) -> Result<u64> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(message.to_string()))
            .map_err(|error| Error::new(format!("send {method} to Chrome: {error}")))?;
        Ok(id)
    }

    fn observer_poll(&mut self) -> Result<u64> {
        // Browser commands also run during bootstrap, before a page session exists.
        // A target census proves a browser turn without requiring a renderer session.
        if self.observer.is_none() {
            return self.send("Target.getTargets", json!({}), None);
        }
        self.send(
            "Runtime.evaluate",
            json!({"expression": format!("new Promise(resolve => setTimeout(resolve, {}))", POLL.as_millis()), "awaitPromise": true}),
            self.observer.clone(),
        )
    }

    fn complete_observer_poll(&self, value: &Value, budget: &mut WaitBudget) -> Result<()> {
        if value.get("error").is_some() || value["result"].get("exceptionDetails").is_some() {
            return Err(Error::new(format!("observer poll: {value}")));
        }
        budget.poll(&self.owner.members()?);
        Ok(())
    }

    fn wait_event(&mut self, method: &str, cpu: Duration) -> Result<()> {
        if self
            .events
            .iter()
            .any(|event| event["method"] == json!(method))
        {
            return Ok(());
        }
        let mut budget = self.budget(cpu)?;
        let mut marker = self.observer_poll()?;
        loop {
            if let Some(value) = self.read(true)? {
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, &mut budget)?;
                    if !budget.exhausted(&self.owner.members()?) {
                        marker = self.observer_poll()?;
                    }
                } else if value.get("id").is_none() {
                    let found = value["method"] == json!(method);
                    self.keep(value);
                    if found {
                        return Ok(());
                    }
                }
            }
            if budget.exhausted(&self.owner.members()?) {
                if budget.past_deadline() {
                    return Err(Error::deadline(&format!("waiting for {method}")));
                }
                return Err(Error::new(format!(
                    "no {method} after {} completed observer polls and {:?} browser CPU",
                    budget.polls,
                    budget.cpu.used()
                )));
            }
        }
    }

    /// Drain through a browser-scheduled observation marker. Its protocol reply proves
    /// Chrome ran the observation, even when this driver waited for a core in between.
    pub fn pump(&mut self, window: Duration) -> Result<()> {
        self.session_call("Runtime.evaluate", json!({
            "expression": format!("new Promise(resolve => setTimeout(resolve, {}))", window.as_millis()),
            "awaitPromise": true
        }), self.observer.clone())?;
        Ok(())
    }

    /// One message, checking exit and CPU on each polling wake. A poll may yield to
    /// its caller; a socket wake alone never decides that Chrome failed.
    fn read(&mut self, poll: bool) -> Result<Option<Value>> {
        let mut budget = process::Budget::new(PROTOCOL_CPU, &self.owner.members()?);
        if let MaybeTlsStream::Plain(stream) = self.socket.get_mut() {
            stream
                .set_read_timeout(Some(POLL))
                .map_err(|error| Error::new(format!("Chrome's DevTools socket: {error}")))?;
        }
        loop {
            if budget.exhausted(&self.owner.members()?) {
                return Err(Error::new(format!(
                    "Chrome spent {:?} CPU without a DevTools message",
                    budget.used()
                )));
            }
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(text.as_str())
                        .map(Some)
                        .map_err(|error| Error::new(format!("DevTools sends JSON: {error}")));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(err))
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if !matches!(self.owner.leash.try_wait(), Ok(None)) {
                        return Err(Error::new(format!(
                            "Chrome's leash exited while waiting for DevTools; {}",
                            self.owner.diagnostics()
                        )));
                    }
                    if budget.exhausted(&self.owner.members()?) {
                        return Err(Error::new(format!(
                            "Chrome spent {:?} CPU without a DevTools reply",
                            budget.used()
                        )));
                    }
                    if poll {
                        return Ok(None);
                    }
                }
                Err(err) => {
                    return Err(Error::new(format!(
                        "reading from Chrome: {err}; {}",
                        self.owner.diagnostics()
                    )));
                }
            }
        }
    }
}

fn session_id(attached: &Value) -> Result<String> {
    attached["sessionId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::new(format!("Target.attachToTarget: no session in {attached}")))
}

fn devtools_endpoint(profile: &Path) -> Option<(String, String)> {
    let pid = std::fs::read_to_string(profile.join("browser.pid")).ok()?;
    if !pid.ends_with('\n') || pid.trim().parse::<u32>().ok()? == 0 {
        return None;
    }
    let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    let mut lines = text.lines();
    let port = lines.next()?;
    let path = lines.next()?;
    // Chrome writes port + newline + UUID path, with no final newline. Check the
    // complete UUID instead of treating a partial second line as a usable endpoint.
    let id = path.strip_prefix("/devtools/browser/")?;
    let complete = id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if port.parse::<u16>().ok()? == 0 || !complete {
        return None;
    }
    Some((port.into(), path.into()))
}
