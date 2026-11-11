//! Headless Chrome, driven over the DevTools protocol: one browser per test, on a profile of
//! its own, leashed so it dies with the test process however that ends.
//!
//! A native leash owns and waits for Chrome. EOF on its suite's pipe terminates the browser
//! even after SIGKILL, then removes its private profile. Startup failures use the same cleanup.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use profiles::next_profile;
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

mod process;
mod profiles;

const POLL: Duration = Duration::from_millis(25);
const PROTOCOL_CPU: Duration = Duration::from_secs(60);
const CLEANUP_CPU: Duration = Duration::from_secs(1);

/// Browser work and completed observations share one allowance. Socket timeouts and
/// scheduler delays spend nothing; an observer reply proves the browser had a turn.
struct WaitBudget {
    cpu: process::Budget,
    polls: u128,
    max_polls: u128,
}

impl WaitBudget {
    fn new(cpu: Duration, members: &[process::Member]) -> Self {
        Self {
            // Zero credit still accepts the initial result. That evaluation keeps
            // its protocol CPU ceiling; one observer turn can end an idle reply.
            cpu: process::Budget::new(if cpu.is_zero() { PROTOCOL_CPU } else { cpu }, members),
            polls: 0,
            max_polls: cpu.as_nanos().div_ceil(POLL.as_nanos()).max(1),
        }
    }

    fn exhausted(&mut self, members: &[process::Member]) -> bool {
        self.cpu.exhausted(members) || self.polls >= self.max_polls
    }

    fn poll(&mut self, members: &[process::Member]) -> bool {
        self.polls += 1;
        self.exhausted(members)
    }
}

#[test]
fn a_zero_allowance_keeps_initial_readiness_and_bounds_both_active_and_idle_replies() {
    let member = |cpu| process::Member {
        pid: 1,
        parent: 0,
        state: 1,
        cpu,
    };
    let mut budget = WaitBudget::new(Duration::ZERO, &[member(Duration::ZERO)]);
    assert!(!budget.exhausted(&[member(Duration::ZERO)]));
    assert!(!budget.exhausted(&[member(Duration::from_millis(1))]));
    assert!(budget.exhausted(&[member(Duration::from_secs(60))]));
    let mut idle = WaitBudget::new(Duration::ZERO, &[]);
    assert!(idle.poll(&[]));
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
            std::path::Path::new(name)
                .exists()
                .then(|| name.to_string())
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
    /// Bytes on the wire, headers included: the finished total, or what arrived so far. An open
    /// event stream counts its response head and the body the page has read: Chrome reports a
    /// stream's transfer size one chunk late, so its last message would wait for the next one.
    pub bytes: f64,
    pub finished: bool,
    /// A speculative `<link rel=prefetch>` (its `Purpose` or `Sec-Purpose` header says so): a
    /// later use's, warmed at idle, not the page's own.
    pub prefetch: bool,
    /// The keep-alive pings an event stream delivered, each named by Chrome's
    /// `Network.eventSourceMessageReceived`; their bytes are in `bytes`.
    pub pings: usize,
}

/// Whether request `headers` (a DevTools header object) name a prefetch.
fn prefetch(headers: &Value) -> bool {
    headers.as_object().is_some_and(|headers| {
        headers.iter().any(|(name, value)| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "purpose" | "sec-purpose"
            ) && value
                .as_str()
                .is_some_and(|value| value.starts_with("prefetch"))
        })
    })
}

/// The requests `events` describe, by Chrome's request id, in the order they were sent.
pub fn requests_from(events: &[Value]) -> Vec<Request> {
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
        let stream = request.kind == "EventSource";
        match event["method"].as_str().unwrap_or("") {
            "Network.requestWillBeSent" => {
                request.url = params["request"]["url"].as_str().unwrap_or("").to_string();
                request.kind = params["type"].as_str().unwrap_or("").to_string();
                request.prefetch |= prefetch(&params["request"]["headers"]);
            }
            "Network.requestWillBeSentExtraInfo" => {
                request.prefetch |= prefetch(&params["headers"]);
            }
            "Network.requestServedFromCache" => request.from_cache = true,
            "Network.responseReceived" => {
                request.status = params["response"]["status"].as_i64().unwrap_or(0);
                if params["response"]["fromDiskCache"].as_bool() == Some(true)
                    || params["response"]["fromPrefetchCache"].as_bool() == Some(true)
                {
                    request.from_cache = true;
                }
                // The head, the only bytes a stream has received when its response arrives.
                if stream && !request.finished {
                    request.bytes = params["response"]["encodedDataLength"]
                        .as_f64()
                        .unwrap_or(0.0);
                }
            }
            // CORS hides the response from Runtime/fetch and responseReceived. The wire
            // status still proves an opaque-origin request was refused before any write.
            "Network.responseReceivedExtraInfo" if request.status == 0 => {
                request.status = params["statusCode"].as_i64().unwrap_or(request.status);
            }
            // A stream's `dataLength` is reported in the task that hands the chunk to the page,
            // just before the messages it completes; its `encodedDataLength` is the previous
            // chunk's. The stand-in and the server send streams uncompressed.
            "Network.dataReceived" if !request.finished => {
                let length = if stream {
                    "dataLength"
                } else {
                    "encodedDataLength"
                };
                request.bytes += params[length].as_f64().unwrap_or(0.0);
            }
            "Network.eventSourceMessageReceived"
                if params["eventName"] == symphony_api::PING_EVENT =>
            {
                request.pings += 1;
            }
            "Network.loadingFinished" => {
                request.bytes = params["encodedDataLength"]
                    .as_f64()
                    .unwrap_or(request.bytes);
                request.finished = true;
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

#[test]
fn an_open_stream_counts_its_head_and_every_chunk_read_and_names_its_pings() {
    // As Chrome reported a stand-in feed: the head, `: open`, two pings and a delta. Each
    // chunk's encodedDataLength is the previous chunk's, so it never names the last one.
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let chunk = |data: u32, encoded: u32| {
        event(
            "Network.dataReceived",
            json!({"requestId": "feed", "dataLength": data, "encodedDataLength": encoded}),
        )
    };
    let message = |name: &str| {
        event(
            "Network.eventSourceMessageReceived",
            json!({"requestId": "feed", "eventName": name, "data": ""}),
        )
    };
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "feed", "type": "EventSource", "request": {"url": "http://h/api/feed"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "glue", "type": "Script", "request": {"url": "http://h/a.js"}}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId": "feed", "response": {"status": 200, "encodedDataLength": 77}}),
        ),
        chunk(8, 0),
        event(
            "Network.dataReceived",
            json!({"requestId": "glue", "dataLength": 900, "encodedDataLength": 300}),
        ),
        event(
            "Network.loadingFinished",
            json!({"requestId": "glue", "encodedDataLength": 512}),
        ),
        chunk(19, 8),
        message(symphony_api::PING_EVENT),
        chunk(19, 19),
        message(symphony_api::PING_EVENT),
        chunk(45, 19),
        message(symphony_api::FEED_EVENT),
    ];
    let requests = requests_from(&events);
    let [feed, glue] = requests.as_slice() else {
        panic!("two requests: {requests:#?}")
    };
    assert_eq!(
        (feed.kind.as_str(), feed.status, feed.bytes, feed.pings),
        ("EventSource", 200, 77.0 + 8.0 + 19.0 + 19.0 + 45.0, 2)
    );
    assert!(!feed.finished);
    assert_eq!((glue.bytes, glue.pings, glue.finished), (512.0, 0, true));
}

#[test]
fn a_prefetch_is_named_by_its_purpose_header_and_served_from_the_prefetch_cache_counts_as_cached()
{
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "warm", "type": "Other", "request": {"url": "http://h/p.wasm", "headers": {}}}),
        ),
        event(
            "Network.requestWillBeSentExtraInfo",
            json!({"requestId": "warm", "headers": {"Sec-Purpose": "prefetch"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "use", "type": "Fetch", "request": {"url": "http://h/p.wasm", "headers": {"Purpose": "other"}}}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId": "use", "response": {"status": 200, "fromPrefetchCache": true}}),
        ),
    ];
    let requests = requests_from(&events);
    let [warm, used] = requests.as_slice() else {
        panic!("two requests: {requests:#?}")
    };
    assert!(warm.prefetch && !warm.from_cache);
    assert!(!used.prefetch && used.from_cache);
}

pub struct Chrome {
    _owner: Owner,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    session: Option<String>,
    /// The launcher's blank target survives navigation in the page under test.
    observer: Option<String>,
    /// Every event since the last `clear`, in order.
    events: Vec<Value>,
}

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

    fn members(&self) -> Vec<process::Member> {
        self.snapshot()
            .expect("read owned Chrome group's CPU and state")
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
        let mut budget = process::Budget::new(cpu, &self.members());
        loop {
            if let Some(value) = handshake(&self.profile) {
                return Ok(value);
            }
            if let Some(status) = self.leash.try_wait()? {
                return Err(std::io::Error::other(format!(
                    "Chrome's leash exited {status} before {what}"
                )));
            }
            if budget.exhausted(&self.members()) {
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    format!("Chrome spent {:?} CPU without {what}", budget.used()),
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    fn launch(binary: &str) -> Self {
        let profile = next_profile();
        std::fs::create_dir(&profile).unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Keep open readers after the leash unlinks these names. Diagnostics survive its
        // cleanup, while a suite killed before Drop leaves no log files behind.
        let log = |extension| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(profile.with_extension(extension))
                .unwrap()
        };
        let stderr = log("stderr");
        let shutdown = log("exit");
        let leash = Command::new("bash")
            .args([
                "-c",
                r#". "$1"; fixture_close_fds; shift; exec "$@""#,
                "browser-leash",
            ])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tools/fixture_process.sh"
            ))
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "chrome::browser_leash", "--nocapture"])
            .env(
                "SF_SYMPHONY_CHROME_LEASH",
                json!([binary, profile]).to_string(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .expect("start Chrome under its leash");
        eprintln!(
            "Chrome launch leash={} profile={}",
            leash.id(),
            profile.display()
        );
        Self {
            leash,
            profile,
            stderr,
            shutdown,
        }
    }
}

unsafe extern "C" {
    fn kill(pid: std::ffi::c_int, signal: std::ffi::c_int) -> std::ffi::c_int;
    fn getpgid(pid: std::ffi::c_int) -> std::ffi::c_int;
}

/// Only a positive, recorded child PID may identify the group this harness owns.
fn signal_group(group: u32, signal: std::ffi::c_int) -> std::io::Result<()> {
    let group = std::ffi::c_int::try_from(group).expect("child PID fits pid_t");
    assert!(group > 0);
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

/// The suite's output reader may die before its leash. Diagnostics must never interrupt cleanup.
fn leash_report(message: std::fmt::Arguments<'_>) {
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

/// Reexecuted in a separate process, with no shared mutable environment in the suite.
#[test]
fn browser_leash() {
    let Ok(config) = std::env::var("SF_SYMPHONY_CHROME_LEASH") else {
        let args = chrome_args(std::path::Path::new("/private/profile"));
        assert!(args.contains(&"--mute-audio".into()));
        assert!(args.contains(&"--user-data-dir=/private/profile".into()));
        return;
    };
    let (binary, profile): (String, PathBuf) = serde_json::from_str(&config).unwrap();
    let mut browser = BrowserGroup(
        Command::new(binary)
            .args(chrome_args(&profile))
            // Its singleton and fetch folders, which a killed Chrome leaves, go with the profile.
            .env("MAC_CHROMIUM_TMPDIR", &profile)
            .env("TMPDIR", &profile)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(profile.with_extension("stderr")).unwrap())
            .process_group(0)
            .spawn()
            .expect("start owned Chrome"),
    );
    let child = &mut browser.0;
    std::fs::write(profile.join("browser.pid"), format!("{}\n", child.id())).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        let _ = send.send(());
    });
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
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
    let cleanup = cleanup_profile(&profile, group);
    let _ = std::fs::remove_file(profile.with_extension("exit"));
    let _ = std::fs::remove_file(profile.with_extension("stderr"));
    if let Err(error) = cleanup {
        panic!("Chrome cleanup {}: {error}", profile.display());
    }
    std::process::exit(status.map_or(0, |status| status.code().unwrap_or(1)));
}

fn chrome_args(profile: &std::path::Path) -> Vec<String> {
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

#[test]
fn profile_cleanup_rechecks_after_helpers_and_reports_other_errors() {
    use std::io::BufRead;

    let profile = next_profile();
    std::fs::create_dir_all(profile.join("nested")).unwrap();
    std::fs::write(profile.join("nested/data"), "profile").unwrap();
    // Announce readiness before cleanup starts. Recreate the profile only after its first
    // removal, so returning on that removal instead of waiting for helpers always leaks it.
    let mut browser = BrowserGroup(
        Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf 'ready\n'
while test -d "$1"; do /bin/sleep 0.01; done
mkdir "$1"
"#,
                "profile-writer",
            ])
            .arg(&profile)
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    std::io::BufReader::new(browser.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let group = browser.0.id();
    let reaper = std::thread::spawn(move || browser.0.wait().unwrap());
    let cleanup = cleanup_profile(&profile, group);
    if cleanup.is_err() {
        // A regression must also stop this owned helper instead of stranding its waiter.
        let _ = signal_group(group, 9);
    }
    let status = reaper.join().unwrap();
    cleanup.unwrap();
    assert!(status.success());
    assert!(!profile.exists(), "cleanup must remove the whole profile");
    cleanup_profile(&profile, group).unwrap();

    // A path that is not a directory is a real fixture error, not a retryable writer race.
    std::fs::write(&profile, "not a directory").unwrap();
    let error = cleanup_profile(&profile, group).unwrap_err();
    std::fs::remove_file(&profile).unwrap();
    assert_eq!(error.kind(), ErrorKind::NotADirectory);
}

/// Keep a removed CPU guard from hanging its regression: end the owned group
/// after more CPU work, so the caller can assert on the wrong completion result.
fn cpu_watchdog(
    group: u32,
    limit: Duration,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<bool>) {
    let (done, receive) = std::sync::mpsc::channel();
    let watcher = std::thread::spawn(move || {
        let mut budget = None;
        while matches!(
            receive.recv_timeout(POLL),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            // A transient snapshot error while a churned child exits is not CPU work.
            let Ok(members) = process::members(group) else {
                continue;
            };
            let budget = budget.get_or_insert_with(|| process::Budget::new(limit, &members));
            if budget.exhausted(&members) {
                let _ = signal_group(group, 9);
                return true;
            }
        }
        false
    });
    (done, watcher)
}

struct StandIn {
    owner: Owner,
    binary: PathBuf,
    group: Option<u32>,
}

impl StandIn {
    fn new(script: &str) -> Self {
        let binary = next_profile();
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let owner = Owner::launch(binary.to_str().unwrap());
        Self {
            owner,
            binary,
            group: None,
        }
    }

    fn group(&mut self) -> u32 {
        let group = self
            .owner
            .ready("recorded stand-in PID", PROTOCOL_CPU, |profile| {
                let pid = std::fs::read_to_string(profile.join("browser.pid")).ok()?;
                if !pid.ends_with('\n') {
                    return None;
                }
                pid.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
            })
            .unwrap();
        self.group = Some(group);
        group
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        // A probe of BrowserGroup's kill must not strand this fixture's spinner.
        // The separate native leash also owns it if this test is killed outright.
        if let Some(group) = self.group {
            let _ = signal_group(group, 9);
        }
        let _ = std::fs::remove_file(&self.binary);
    }
}

#[test]
fn shutdown_diagnostics_keep_browser_exit_and_suite_eof_distinct_after_profile_cleanup() {
    use std::io::{Seek, Write};
    for (script, close_suite, expected) in [
        (
            "#!/bin/sh\nprintf 'browser exit detail\\n' >&2\nexit 47\n",
            false,
            "browser exited:",
        ),
        (
            "#!/bin/sh\nprintf 'browser waiting detail\\n' >&2\nexec /usr/bin/tail -f /dev/null\n",
            true,
            "suite stdin closed",
        ),
    ] {
        let mut stand_in = StandIn::new(script);
        if close_suite {
            // The launcher's PID record is written after opening the stderr log.
            stand_in.group();
            let diagnostics = stand_in.owner.diagnostics();
            assert!(diagnostics.contains("status=Ok(None)"), "{diagnostics}");
            assert!(
                diagnostics.contains("shutdown=not recorded yet"),
                "{diagnostics}"
            );
            drop(stand_in.owner.leash.stdin.take());
        }
        // Child::wait closes its stdin. Hold the suite's lifetime pipe separately so
        // observing a browser exit cannot itself request an EOF shutdown.
        let lifetime = stand_in.owner.leash.stdin.take();
        let status = stand_in.owner.leash.wait().unwrap();
        drop(lifetime);
        assert_eq!(status.code(), Some(if close_suite { 0 } else { 47 }));
        assert!(!stand_in.owner.profile.exists());
        let diagnostics = stand_in.owner.diagnostics();
        assert!(diagnostics.contains(expected), "{diagnostics}");
        if !close_suite {
            assert!(diagnostics.contains("47"), "{diagnostics}");
            assert!(diagnostics.contains("browser exit detail"), "{diagnostics}");
        }
        assert!(diagnostics.contains("members=Ok([])"), "{diagnostics}");
        let exit = stand_in.owner.profile.with_extension("exit");
        let stderr = stand_in.owner.profile.with_extension("stderr");
        assert!(!exit.exists());
        assert!(!stderr.exists());
        // An arbitrarily long Chrome log must not turn one socket failure into huge output.
        stand_in.owner.stderr.rewind().unwrap();
        stand_in
            .owner
            .stderr
            .write_all(format!("discarded{}kept", "x".repeat(9_000)).as_bytes())
            .unwrap();
        let diagnostics = stand_in.owner.diagnostics();
        assert!(!diagnostics.contains("discarded"));
        assert!(diagnostics.ends_with("kept"));
        assert!(diagnostics.len() < 9_000);
        drop(stand_in);
        assert!(!exit.exists());
        assert!(!stderr.exists());
    }
}

#[test]
fn an_unexpected_protocol_reset_fails_with_the_browser_exit_evidence() {
    let binary = next_profile();
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'reset exit detail\\n' >&2\nexit 47\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = Owner::launch(binary.to_str().unwrap());
    let lifetime = owner.leash.stdin.take();
    assert_eq!(owner.leash.wait().unwrap().code(), Some(47));
    drop(lifetime);
    std::fs::remove_file(binary).unwrap();

    // A peer dropping TCP without a WebSocket close exercises the reported transport
    // boundary without requiring a real Chrome crash.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    drop(peer);
    let mut chrome = Chrome {
        _owner: owner,
        socket: WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(stream),
            tungstenite::protocol::Role::Client,
            None,
        ),
        next_id: 0,
        session: None,
        observer: None,
        events: Vec::new(),
    };
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| chrome.read(false)))
        .expect_err("an unexpected reset must still fail the case");
    let failure = failure.downcast_ref::<String>().unwrap();
    assert!(failure.contains("reading from Chrome:"), "{failure}");
    assert!(failure.contains("browser exited:"), "{failure}");
    assert!(failure.contains("reset exit detail"), "{failure}");
}

#[test]
fn cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill() {
    if std::env::var_os("SF_CHROME_CLEANUP_SPINNER").is_none() {
        let output =
            suite_child("chrome::cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill")
                .env("SF_CHROME_CLEANUP_SPINNER", "1")
                .output()
                .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let report = stdout
            .lines()
            .find_map(|line| line.strip_prefix("CLEANUP_SPINNER "))
            .expect("cleanup fixture report");
        let (group, seconds): (u32, u64) = serde_json::from_str(report).unwrap();
        let lines = stderr
            .lines()
            .filter(|line| line.starts_with(&format!("Chrome cleanup group={group} ")))
            .count();
        assert!(lines > 0, "the live helper must be diagnosed");
        // Two cleanup calls: one CPU failure and one ESRCH wait after the final kill.
        assert!(
            u64::try_from(lines).unwrap() <= seconds + 2,
            "{lines} ESRCH diagnostics in {seconds} seconds"
        );
        return;
    }

    let mut stand_in = StandIn::new(
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
/usr/bin/tail -f /dev/null &
main=$!
(
    while :; do
        /bin/sleep 0.01 &
        i=0
        while test "$i" -lt 2000; do i=$((i+1)); done
        wait
    done
) &
helper=$!
printf '%s %s\n' "$main" "$helper" > "$profile/spinner-ready"
wait "$main"
wait "$helper"
"#,
    );
    let group = stand_in.group();
    let (main, helper) = stand_in
        .owner
        .ready("spinning helper", PROTOCOL_CPU, |profile| {
            let ready = std::fs::read_to_string(profile.join("spinner-ready")).ok()?;
            if !ready.ends_with('\n') {
                return None;
            }
            let pids: Vec<u32> = ready
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            (pids.len() == 2 && pids.iter().all(|pid| *pid > 0)).then(|| (pids[0], pids[1]))
        })
        .unwrap();
    // SAFETY: the owned fixture announced this positive main PID; killing it leaves
    // its helper running in the recorded group, under the separate leash's ownership.
    assert_eq!(
        unsafe { kill(std::ffi::c_int::try_from(main).unwrap(), 9) },
        0
    );
    assert!(
        process::members(group)
            .unwrap()
            .iter()
            .any(|member| member.pid == helper)
    );
    let (done, watcher) = cpu_watchdog(group, CLEANUP_CPU * 2);
    let started = Instant::now();
    let result = cleanup_profile(&stand_in.owner.profile, group);
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a missed spinning helper must fail cleanup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(error.to_string().contains("CPU after SIGKILL"), "{error}");
    eprintln!("spinning helper={helper} group={group}: {error}; watchdog killed={forced}");
    println!(
        "CLEANUP_SPINNER {}",
        json!([group, started.elapsed().as_secs()])
    );
}

/// Drain queued protocol messages until the peer closes the connection, with or without a
/// WebSocket close handshake. Other protocol and I/O errors are fixture failures.
fn wait_for_protocol_eof(
    socket: &mut WebSocket<impl std::io::Read + std::io::Write>,
) -> tungstenite::Result<()> {
    use tungstenite::Error;
    use tungstenite::error::ProtocolError;

    loop {
        match socket.read() {
            Ok(_) => {}
            Err(
                Error::ConnectionClosed
                | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
            ) => {
                return Ok(());
            }
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::BrokenPipe
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn protocol_eof_drains_messages_and_rejects_nonclosure_errors() {
    use std::io::Cursor;
    use tungstenite::protocol::Role;

    for ending in [b"".as_slice(), b"\x88\x00"] {
        // Two server text frames precede either TCP EOF or a WebSocket close followed by EOF.
        let frames = [b"\x81\x01a\x81\x01b".as_slice(), ending].concat();
        let mut socket = WebSocket::from_raw_socket(Cursor::new(frames), Role::Client, None);
        wait_for_protocol_eof(&mut socket).unwrap();
        assert!(matches!(
            socket.read(),
            Err(tungstenite::Error::AlreadyClosed)
        ));
    }
    // A reserved opcode is a protocol failure, not evidence that the browser exited.
    let mut socket =
        WebSocket::from_raw_socket(Cursor::new(b"\x83\x00".to_vec()), Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::InvalidOpcode(3)
        ))
    ));

    // An open but silent peer is not EOF; exercise WouldBlock without a timing deadline.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    let mut socket = WebSocket::from_raw_socket(stream, Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock
    ));

    struct FailedIo(ErrorKind);
    impl std::io::Read for FailedIo {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
    }
    impl std::io::Write for FailedIo {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(self.0.into())
        }
    }
    for kind in [
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
        ErrorKind::TimedOut,
    ] {
        let mut socket = WebSocket::from_raw_socket(FailedIo(kind), Role::Client, None);
        let result = wait_for_protocol_eof(&mut socket);
        if kind == ErrorKind::TimedOut {
            assert!(matches!(result, Err(tungstenite::Error::Io(error)) if error.kind() == kind));
        } else {
            result.unwrap();
        }
    }
}

const SUITE_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGHUP, libc::SIGTERM];

fn suite_signal_disposition(disposition: libc::sighandler_t) -> std::io::Result<()> {
    // SAFETY: sigemptyset and sigaction are async-signal-safe. The initialized action
    // contains no handler pointer and changes only the calling process's dispositions.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = disposition;
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for signal in SUITE_SIGNALS {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

fn suite_child(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture"]);
    // Background shells can pass SIG_IGN through exec. Every catchable signal in
    // the lifecycle matrix must terminate this child; SIGKILL cannot be ignored.
    // SAFETY: the hook changes only the forked child's dispositions through
    // async-signal-safe calls, before it executes the suite fixture.
    unsafe { command.pre_exec(|| suite_signal_disposition(libc::SIG_DFL)) };
    command
}

#[test]
fn suite_child_resets_inherited_ignored_signals() {
    const TEST: &str = "chrome::suite_child_resets_inherited_ignored_signals";
    const MODE: &str = "SF_CHROME_SUITE_SIGNAL_MODE";
    let mode = std::env::var(MODE).ok();
    if let Some(mode) = mode {
        let expected = match mode.as_str() {
            "ignored-parent" => libc::SIG_IGN,
            "reset-child" => libc::SIG_DFL,
            _ => panic!("unknown suite signal fixture mode: {mode}"),
        };
        for signal in SUITE_SIGNALS {
            // SAFETY: a null action queries this process's disposition into initialized storage.
            let action = unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut action), 0);
                action
            };
            assert_eq!(action.sa_sigaction, expected, "signal {signal} in {mode}");
        }
        if mode == "ignored-parent" {
            assert!(
                suite_child(TEST)
                    .env(MODE, "reset-child")
                    .status()
                    .unwrap()
                    .success(),
                "suite child must reset its ignored parent's signal dispositions"
            );
        }
        return;
    }

    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent
        .args(["--exact", TEST, "--nocapture"])
        .env(MODE, "ignored-parent");
    // SAFETY: only this forked parent's dispositions change, through the same
    // async-signal-safe calls. The concurrent test runner keeps its own dispositions.
    unsafe { parent.pre_exec(|| suite_signal_disposition(libc::SIG_IGN)) };
    assert!(parent.status().unwrap().success());
}

#[test]
fn chrome_dies_with_its_suite() {
    use std::io::{BufRead, Write};
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os("SF_CHROME_SUITE_CHILD").is_some() {
        let fake_binary = std::env::var("SF_SYMPHONY_FAKE_BROWSER").ok();
        let real = fake_binary
            .is_none()
            .then(|| Chrome::launch(&find().expect("real Chrome capability")));
        let mut fake = fake_binary.map(|binary| Owner::launch(&binary));
        let profile = real
            .as_ref()
            .map(|chrome| &chrome._owner.profile)
            .or_else(|| fake.as_ref().map(|owner| &owner.profile))
            .unwrap()
            .clone();
        let helper = fake.as_mut().map(|owner| {
            owner
                .ready("stand-in helper handshake", PROTOCOL_CPU, |profile| {
                    if let Ok(ready) = std::fs::read_to_string(profile.join("fake-ready"))
                        && ready.ends_with('\n')
                    {
                        let pids: Vec<u32> = ready
                            .split_whitespace()
                            .filter_map(|pid| pid.parse().ok())
                            .collect();
                        let recorded = std::fs::read_to_string(profile.join("browser.pid"))
                            .ok()
                            .filter(|text| text.ends_with('\n'))
                            .and_then(|text| text.trim().parse::<u32>().ok());
                        if pids.len() == 2 && recorded == Some(pids[0]) {
                            return Some(pids[1]);
                        }
                    }
                    None
                })
                .expect("stand-in helper handshake")
        });
        let endpoint = real
            .as_ref()
            .map(|_| std::fs::read_to_string(profile.join("DevToolsActivePort")).unwrap());
        let group: u32 = std::fs::read_to_string(profile.join("browser.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let leash = real
            .as_ref()
            .map(|chrome| chrome._owner.leash.id())
            .or_else(|| fake.as_ref().map(|owner| owner.leash.id()))
            .unwrap();
        println!(
            "CHROME_READY {}",
            json!([profile, endpoint, group, helper, leash])
        );
        std::io::stdout().flush().unwrap();
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        return;
    }
    struct Suite(Child);
    impl Drop for Suite {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Sweep(Option<u32>, PathBuf);
    impl Drop for Sweep {
        fn drop(&mut self) {
            if let Some(group) = self.0 {
                let _ = signal_group(group, 9);
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let scratch = Scratch(next_profile());
    std::fs::create_dir(&scratch.0).unwrap();
    let fake_browser = scratch.0.join("browser");
    std::fs::write(
        &fake_browser,
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
test -n "$profile" || exit 2
trap '' HUP TERM
(
    while :; do
        mkdir -p "$profile/writing/nested"
        printf '%s\n' writing > "$profile/writing/nested/data"
        rm -f "$profile/writing/nested/data"
    done
) 2>/dev/null &
helper=$!
trap 'exit 0' TERM
printf '%s %s\n' "$$" "$helper" > "$profile/fake-ready"
wait "$helper"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake_browser, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut variants = vec![Some(fake_browser)];
    if find().is_some() {
        variants.push(None);
    } else {
        eprintln!("no Chrome: real browser capability unavailable; stand-in still tested");
    }
    for fake_browser in variants {
        for (signal, expected, closed_stderr) in [
            ("normal", None, false),
            ("-TERM", Some(libc::SIGTERM), false),
            ("-HUP", Some(libc::SIGHUP), false),
            ("-INT", Some(libc::SIGINT), false),
            ("-KILL", Some(libc::SIGKILL), false),
            ("-KILL", Some(libc::SIGKILL), true),
        ] {
            let mut command = suite_child("chrome::chrome_dies_with_its_suite");
            if let Some(binary) = &fake_browser {
                command.env("SF_SYMPHONY_FAKE_BROWSER", binary);
            } else {
                command.env_remove("SF_SYMPHONY_FAKE_BROWSER");
            }
            let mut suite = Suite(
                command
                    .env("SF_CHROME_SUITE_CHILD", "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(if closed_stderr {
                        Stdio::piped()
                    } else {
                        Stdio::inherit()
                    })
                    .spawn()
                    .unwrap(),
            );
            let stdout = suite.0.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    if let Some(ready) = line.strip_prefix("CHROME_READY ") {
                        let _ =
                            send.send(
                                serde_json::from_str::<(
                                    PathBuf,
                                    Option<String>,
                                    u32,
                                    Option<u32>,
                                    u32,
                                )>(ready)
                                .unwrap(),
                            );
                    }
                }
            });
            let (profile, endpoint, group, helper, leash) =
                receive.recv().expect("suite protocol handshake");
            let mut sweep = Sweep(Some(group), profile.clone());
            assert!(
                signal_group(group, 0).is_ok(),
                "recorded browser group must be alive before stopping its suite"
            );
            if let Some(helper) = helper {
                // SAFETY: getpgid only queries the positive helper PID the owned fixture announced.
                let actual = unsafe { getpgid(std::ffi::c_int::try_from(helper).unwrap()) };
                assert_eq!(
                    actual,
                    std::ffi::c_int::try_from(group).unwrap(),
                    "the never-exiting helper must be alive in the browser's group"
                );
            }
            assert_eq!(
                std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let mut protocol = endpoint.map(|endpoint| {
                let mut lines = endpoint.lines();
                let port = lines.next().unwrap().to_string();
                let path = lines.next().unwrap();
                let (socket, _) =
                    tungstenite::connect(format!("ws://127.0.0.1:{port}{path}")).unwrap();
                (socket, port)
            });
            if closed_stderr {
                // The suite has announced its live browser and leash. Lose the output reader
                // before killing the suite, as when a piped cargo test loses its consumer.
                drop(suite.0.stderr.take().expect("piped suite stderr reader"));
            }
            let input = suite.0.stdin.take(); // Kept to the reap: Child::wait closes it first.
            if let Some(expected) = expected {
                assert!(
                    Command::new("/bin/kill")
                        .args([signal, &suite.0.id().to_string()])
                        .status()
                        .unwrap()
                        .success()
                );
                assert_eq!(suite.0.wait().unwrap().signal(), Some(expected));
            } else {
                drop(input);
                assert!(suite.0.wait().unwrap().success());
            }
            reader.join().unwrap();
            // Observe protocol closure, then require the whole recorded group to be gone below.
            // A closing WebSocket alone can precede the browser releasing its listening socket.
            if let Some((socket, _)) = &mut protocol {
                wait_for_protocol_eof(socket)
                    .unwrap_or_else(|error| panic!("browser protocol after {signal}: {error}"));
            }
            loop {
                // SAFETY: the suite announced this exact leash PID at startup. Signal zero
                // only queries it, including its zombie until the parent reaps it.
                if unsafe { kill(std::ffi::c_int::try_from(leash).unwrap(), 0) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(3) {
                        break;
                    }
                    assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");
                }
                std::thread::sleep(POLL);
            }
            // The leash's exit is the boundary under test. Waiting here for a leaked
            // helper/profile would hide a premature exit instead of failing the case.
            assert_eq!(
                signal_group(group, 0)
                    .err()
                    .and_then(|error| error.raw_os_error()),
                Some(3),
                "browser group outlived its leash after {signal}"
            );
            assert!(
                !profile.exists(),
                "profile outlived its leash after {signal}"
            );
            for extension in ["stderr", "exit"] {
                assert!(
                    !profile.with_extension(extension).exists(),
                    "{extension} log outlived its leash after {signal}"
                );
            }
            // An ended suite took its folder with it; the next run's sweep takes a killed one's.
            profiles::sweep();
            let folder = profile.parent().unwrap();
            assert!(!folder.exists(), "suite's folder outlived {signal}");
            sweep.0 = None;
            if let Some((_, port)) = &protocol {
                assert!(
                    TcpStream::connect(format!("127.0.0.1:{port}")).is_err(),
                    "DevTools survived {signal}"
                );
            }
        }
    }
}

impl Chrome {
    pub fn launch(binary: &str) -> Chrome {
        let mut owner = Owner::launch(binary);
        let (port, path) = owner
            .ready("DevTools port readiness", PROTOCOL_CPU, devtools_endpoint)
            .expect("Chrome DevTools readiness");
        let (socket, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}{path}"))
            .expect("connect to Chrome's DevTools");
        let mut chrome = Chrome {
            _owner: owner,
            socket,
            next_id: 0,
            session: None,
            observer: None,
            events: Vec::new(),
        };
        let targets = chrome.browser_call("Target.getTargets", json!({}));
        let blank = targets["targetInfos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["type"] == "page" && target["url"] == "about:blank")
            .expect("Chrome's launch target")["targetId"]
            .as_str()
            .unwrap()
            .to_owned();
        let observer = chrome.browser_call(
            "Target.attachToTarget",
            json!({"targetId": blank, "flatten": true}),
        );
        chrome.observer = Some(observer["sessionId"].as_str().unwrap().to_owned());
        let target = chrome.browser_call("Target.createTarget", json!({"url": "about:blank"}));
        let target_id = target["targetId"].as_str().unwrap().to_string();
        let attached = chrome.browser_call(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        );
        chrome.session = Some(attached["sessionId"].as_str().unwrap().to_string());
        chrome.call("Page.enable", json!({}));
        chrome.call("Network.enable", json!({}));
        chrome
    }

    /// Emulate a mid phone on a tailnet link: CPU slowed 4x, 10 Mbit/s each way, 40 ms round trip.
    pub fn mid_phone(&mut self) {
        self.call("Emulation.setCPUThrottlingRate", json!({"rate": 4}));
        let bytes_per_second = 10_000_000 / 8;
        self.call(
            "Network.emulateNetworkConditions",
            json!({"offline": false, "latency": 40, "downloadThroughput": bytes_per_second, "uploadThroughput": bytes_per_second}),
        );
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": 390, "height": 844, "deviceScaleFactor": 3, "mobile": true}),
        );
    }

    /// Ask to save data, as a browser in data saver mode does, on every page opened from now on:
    /// the shell then warms no module at idle, and a module's first use alone requests its files.
    /// A test that holds or fails such a request uses it, so no idle prefetch serves the file
    /// first.
    pub fn save_data(&mut self) {
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": "Object.defineProperty(Navigator.prototype, 'connection', {configurable: true, get: () => ({saveData: true, effectiveType: '4g'})});"}),
        );
    }

    /// Go to `url` and wait for its document to be parsed.
    pub fn open(&mut self, url: &str) {
        self.events.clear();
        self.call("Page.navigate", json!({"url": url}));
        self.wait_event("Page.domContentEventFired", Duration::from_secs(60));
    }

    /// Evaluate `expression` in the page (awaiting a promise), its value as JSON.
    pub fn eval(&mut self, expression: &str) -> Value {
        let mut budget = WaitBudget::new(PROTOCOL_CPU, &self._owner.members());
        self.eval_bounded(expression, &mut budget).unwrap_or_else(|| {
            panic!(
                "{expression}: no evaluation reply after {} completed observer polls and {:?} CPU",
                budget.polls,
                budget.cpu.used()
            )
        })
    }

    fn eval_bounded(&mut self, expression: &str, budget: &mut WaitBudget) -> Option<Value> {
        let id = self.send(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
            self.session.clone(),
        );
        let mut marker = self.observer_poll();
        loop {
            if let Some(value) = self.read(true) {
                if value["id"] == json!(id) {
                    if let Some(error) = value.get("error") {
                        panic!("{expression}: {error}");
                    }
                    let result = &value["result"];
                    if let Some(exception) = result.get("exceptionDetails") {
                        panic!("{expression} threw: {exception}");
                    }
                    return Some(result["result"]["value"].clone());
                }
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, budget);
                    if budget.exhausted(&self._owner.members()) {
                        return None;
                    }
                    marker = self.observer_poll();
                } else if value.get("id").is_none() {
                    self.events.push(value);
                }
            }
            if budget.exhausted(&self._owner.members()) {
                return None;
            }
        }
    }

    /// Wait until `expression` is truthy, bounded by browser CPU and completed polls,
    /// including observer turns while an evaluation's promise is still pending.
    pub fn wait_for(&mut self, expression: &str, cpu: Duration) -> bool {
        let mut budget = WaitBudget::new(cpu, &self._owner.members());
        loop {
            let Some(value) = self.eval_bounded(expression, &mut budget) else {
                eprintln!(
                    "Chrome evaluation still pending after {} polls and {:?} CPU: {expression}",
                    budget.polls,
                    budget.cpu.used()
                );
                return false;
            };
            if !(value.is_null()
                || value == json!(false)
                || value == json!(0)
                || value == json!(""))
            {
                return true;
            }
            if budget.poll(&self._owner.members()) {
                eprintln!(
                    "Chrome condition still false after {} polls and {:?} CPU: {expression}",
                    budget.polls,
                    budget.cpu.used()
                );
                return false;
            }
            self.pump(POLL);
        }
    }

    /// Observe protocol events while the real browser is paused at a fixture boundary.
    pub fn wait_for_protocol_events(
        &mut self,
        method: &str,
        count: usize,
        cpu: Duration,
    ) -> Vec<Value> {
        let mut budget = WaitBudget::new(cpu, &self._owner.members());
        loop {
            self.pump(POLL);
            let events: Vec<_> = self
                .events
                .iter()
                .filter(|event| event["method"] == method)
                .map(|event| event["params"].clone())
                .collect();
            if events.len() >= count || budget.poll(&self._owner.members()) {
                return events;
            }
        }
    }

    /// The requests seen since the last `open`, by Chrome's request id.
    pub fn requests(&mut self) -> Vec<Request> {
        self.pump(Duration::from_millis(50));
        requests_from(&self.events)
    }

    pub fn browser_call(&mut self, method: &str, params: Value) -> Value {
        self.session_call(method, params, None)
    }

    /// One DevTools command; its result. Events read meanwhile are kept.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        self.session_call(method, params, self.session.clone())
    }

    fn session_call(&mut self, method: &str, params: Value, session: Option<String>) -> Value {
        let mut budget = WaitBudget::new(PROTOCOL_CPU, &self._owner.members());
        let id = self.send(method, params, session);
        let mut marker = self.observer_poll();
        loop {
            if let Some(value) = self.read(true) {
                if value["id"] == json!(id) {
                    if let Some(error) = value.get("error") {
                        panic!("{method}: {error}");
                    }
                    return value["result"].clone();
                }
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, &mut budget);
                    if !budget.exhausted(&self._owner.members()) {
                        marker = self.observer_poll();
                    }
                }
                if value.get("id").is_none() {
                    self.events.push(value);
                }
            }
            assert!(
                !budget.exhausted(&self._owner.members()),
                "{method}: no reply after {} completed observer polls and {:?} CPU",
                budget.polls,
                budget.cpu.used()
            );
        }
    }

    fn send(&mut self, method: &str, params: Value, session: Option<String>) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(message.to_string()))
            .expect("send to Chrome");
        id
    }

    fn observer_poll(&mut self) -> u64 {
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

    fn complete_observer_poll(&self, value: &Value, budget: &mut WaitBudget) {
        assert!(
            value.get("error").is_none() && value["result"].get("exceptionDetails").is_none(),
            "observer poll: {value}"
        );
        budget.poll(&self._owner.members());
    }

    fn wait_event(&mut self, method: &str, cpu: Duration) {
        if self
            .events
            .iter()
            .any(|event| event["method"] == json!(method))
        {
            return;
        }
        let mut budget = WaitBudget::new(cpu, &self._owner.members());
        let mut marker = self.observer_poll();
        loop {
            if let Some(value) = self.read(true) {
                if value["id"] == json!(marker) {
                    self.complete_observer_poll(&value, &mut budget);
                    if !budget.exhausted(&self._owner.members()) {
                        marker = self.observer_poll();
                    }
                } else if value.get("id").is_none() {
                    let found = value["method"] == json!(method);
                    self.events.push(value);
                    if found {
                        return;
                    }
                }
            }
            assert!(
                !budget.exhausted(&self._owner.members()),
                "no {method} after {} completed observer polls and {:?} browser CPU",
                budget.polls,
                budget.cpu.used()
            );
        }
    }

    /// Drain through a browser-scheduled observation marker. Its protocol reply proves
    /// Chrome ran the observation, even when this driver waited for a core in between.
    pub fn pump(&mut self, window: Duration) {
        self.session_call("Runtime.evaluate", json!({
            "expression": format!("new Promise(resolve => setTimeout(resolve, {}))", window.as_millis()),
            "awaitPromise": true
        }), self.observer.clone());
    }

    /// One message, checking exit and CPU on each polling wake. A poll may yield to
    /// its caller; a socket wake alone never decides that Chrome failed.
    fn read(&mut self, poll: bool) -> Option<Value> {
        let mut budget = process::Budget::new(PROTOCOL_CPU, &self._owner.members());
        if let MaybeTlsStream::Plain(stream) = self.socket.get_mut() {
            stream.set_read_timeout(Some(POLL)).unwrap();
        }
        loop {
            assert!(
                !budget.exhausted(&self._owner.members()),
                "Chrome spent {:?} CPU without a DevTools message",
                budget.used()
            );
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return Some(serde_json::from_str(text.as_str()).expect("DevTools sends JSON"));
                }
                Ok(_) => continue,
                Err(tungstenite::Error::Io(err))
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    assert!(
                        self._owner.leash.try_wait().unwrap().is_none(),
                        "Chrome's leash exited while waiting for DevTools"
                    );
                    assert!(
                        !budget.exhausted(&self._owner.members()),
                        "Chrome spent {:?} CPU without a DevTools reply",
                        budget.used()
                    );
                    if poll {
                        return None;
                    }
                }
                Err(err) => panic!("reading from Chrome: {err}; {}", self._owner.diagnostics()),
            }
        }
    }
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

#[test]
fn startup_requires_complete_readiness_and_observes_leash_exit() {
    let profile = next_profile();
    std::fs::create_dir(&profile).unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", "read -r release"])
        .stdin(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id();
    let release = child.stdin.take().unwrap();
    let owner = Owner {
        leash: child,
        profile: profile.clone(),
        stderr: std::fs::File::open("/dev/null").unwrap(),
        shutdown: std::fs::File::open("/dev/null").unwrap(),
    };
    // These are actual partial/invalid writes at the file boundary Chrome uses.
    for (pid, port) in [
        (
            "123",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        ("123\n", "1234\n"),
        (
            "123\n",
            "0\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        (
            "123\n",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789ab",
        ),
    ] {
        std::fs::write(profile.join("browser.pid"), pid).unwrap();
        std::fs::write(profile.join("DevToolsActivePort"), port).unwrap();
        assert!(devtools_endpoint(&profile).is_none());
    }
    // The owned child is idle on its pipe; its wall wait cannot spend CPU credit.
    std::fs::write(profile.join("browser.pid"), format!("{group}\n")).unwrap();
    let waiter = std::thread::spawn(move || {
        let mut owner = owner;
        let endpoint = owner
            .ready("complete test port", PROTOCOL_CPU, devtools_endpoint)
            .unwrap();
        (owner, endpoint)
    });
    std::fs::write(
        profile.join("DevToolsActivePort"),
        "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
    )
    .unwrap();
    let (mut owner, endpoint) = waiter.join().unwrap();
    assert_eq!(
        endpoint,
        (
            "1234".into(),
            "/devtools/browser/12345678-1234-1234-1234-123456789abc".into()
        )
    );
    std::fs::remove_file(profile.join("DevToolsActivePort")).unwrap();
    drop(release);
    assert!(!owner.leash.wait().unwrap().success());
    let error = owner
        .ready("missing port after exit", PROTOCOL_CPU, devtools_endpoint)
        .unwrap_err();
    assert!(error.to_string().contains("exited"), "{error}");
    drop(owner);
    assert!(!profile.exists());

    // The real leash owns a stand-in that burns CPU without publishing a port.
    let mut stand_in = StandIn::new("#!/bin/sh\nwhile :; do :; done\n");
    let group = stand_in.group();
    let (done, watcher) = cpu_watchdog(group, Duration::from_secs(1));
    let result = stand_in.owner.ready(
        "missing stand-in port",
        Duration::from_millis(50),
        devtools_endpoint,
    );
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a spinning browser without a port must fail startup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(
        error
            .to_string()
            .contains("CPU without missing stand-in port"),
        "{error}"
    );
    eprintln!("spinning startup group={group}: {error}; watchdog killed={forced}");
}

#[test]
fn browser_waits_keep_events_and_bound_idle_polls_without_charging_observer_delay() {
    use tungstenite::protocol::Role;

    // Exercise the actual transport and kernel meter with an idle owned process.
    // Chrome's background startup work is legitimate CPU and cannot serve as an
    // idle fixture for a deliberately small allowance.
    let profile = next_profile();
    std::fs::create_dir(&profile).unwrap();
    let idle = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let peer = std::thread::spawn(move || {
        let mut socket = WebSocket::from_raw_socket(peer.try_clone().unwrap(), Role::Server, None);
        let (send, receive) = std::sync::mpsc::channel::<Value>();
        let writer = std::thread::spawn(move || {
            let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
            for value in receive {
                if socket.send(Message::text(value.to_string())).is_err() {
                    break;
                }
            }
        });
        let mut evaluations = 0;
        let mut idle_evaluations = 0;
        let mut pending_markers = 0;
        let mut scripted = Vec::<std::ops::RangeInclusive<u64>>::new();
        while let Ok(Message::Text(text)) = socket.read() {
            let command: Value = serde_json::from_str(text.as_str()).unwrap();
            assert_eq!(command["method"], "Runtime.evaluate");
            let id = command["id"].as_u64().unwrap();
            if scripted.iter().any(|range| range.contains(&id)) {
                assert_eq!(command["sessionId"], "observer");
                pending_markers += 1;
                continue;
            }
            if matches!(
                command["params"]["expression"].as_str(),
                Some("pending-wait" | "pending-eval")
            ) {
                let polls = if command["params"]["expression"] == "pending-wait" {
                    2
                } else {
                    2400
                };
                scripted.push(id + 1..=id + polls);
                send.send(json!({"method":"Fixture.observed", "params":command["params"]}))
                    .unwrap();
                // A finite wire transcript also fails promptly if the driver stops
                // observing: after these turns the pending expression becomes true.
                // Only the driver's allowance may reject it before that late reply.
                for marker in id + 1..=id + polls {
                    send.send(json!({"id":marker,"result":{"result":{"value":null}}}))
                        .unwrap();
                }
                send.send(json!({"id":id,"result":{"result":{"value":true}}}))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == "protocol-error" {
                send.send(json!({"id":id,"error":{"message":"fixture protocol error"}}))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == "evaluation-error" {
                send.send(json!({"id":id,"result":{"exceptionDetails":{"text":"fixture evaluation error"}}})).unwrap();
                continue;
            }
            let value = if command["params"]["expression"] == "ready" {
                evaluations += 1;
                if evaluations == 1 {
                    // Reply only after this owned timer process exits. The first
                    // result is always false after >100 ms wall time, with no
                    // work charged to the idle process's 100 ms CPU allowance.
                    assert!(
                        Command::new("/bin/sleep")
                            .arg("0.25")
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                json!(evaluations > 1)
            } else if command["params"]["expression"] == "never-ready" {
                idle_evaluations += 1;
                // Close the transport on excess retries so a regression fails promptly
                // even though the owned process stays idle and its CPU never advances.
                assert!(
                    idle_evaluations <= 3,
                    "idle wait exceeded its poll allowance"
                );
                json!(false)
            } else if command["params"]["expression"] == "after-timeout" {
                json!("usable")
            } else {
                // Acknowledge pump's observation marker between evaluations.
                Value::Null
            };
            send.send(json!({
                "id": command["id"], "result": {"result": {"value": value}}
            }))
            .unwrap();
        }
        drop(send);
        writer.join().unwrap();
        (evaluations, idle_evaluations, pending_markers)
    });
    let mut controlled = Chrome {
        _owner: Owner {
            leash: idle,
            profile,
            stderr: std::fs::File::open("/dev/null").unwrap(),
            shutdown: std::fs::File::open("/dev/null").unwrap(),
        },
        socket: WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
        next_id: 0,
        session: Some("page".into()),
        observer: Some("observer".into()),
        events: Vec::new(),
    };
    assert!(controlled.wait_for("ready", Duration::from_millis(100)));
    assert!(!controlled.wait_for("never-ready", Duration::ZERO));
    assert!(!controlled.wait_for("never-ready", Duration::from_millis(26)));
    assert!(!controlled.wait_for("pending-wait", Duration::from_millis(26)));
    let timeout = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controlled.eval("pending-eval")
    }))
    .expect_err("plain eval must bound a promise that outlives all observer turns");
    let why = timeout.downcast_ref::<String>().unwrap();
    assert!(why.contains("2400 completed observer polls"), "{why}");
    assert_eq!(controlled.eval("after-timeout"), json!("usable"));
    for (expression, expected) in [
        ("protocol-error", "fixture protocol error"),
        ("evaluation-error", "fixture evaluation error"),
    ] {
        let thrown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controlled.wait_for(expression, Duration::ZERO)
        }))
        .expect_err("an evaluation failure must not become a false condition");
        let why = thrown.downcast_ref::<String>().unwrap();
        assert!(why.contains(expected), "{why}");
    }
    assert!(
        controlled
            .events
            .iter()
            .any(|event| event["method"] == "Fixture.observed"
                && event["params"]["expression"] == "pending-eval")
    );
    drop(controlled);
    assert_eq!(peer.join().unwrap(), (2, 3, 2402));

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary);
    chrome.open("data:text/html,<p>ready</p>");
    // open consumed the event from the wire but kept it for later observers. Even
    // a zero CPU allowance must accept an event/condition that already succeeded.
    chrome.wait_event("Page.domContentEventFired", Duration::ZERO);
    assert!(chrome.wait_for("document.querySelector('p')", Duration::ZERO));
    assert!(!chrome.wait_for("new Promise(() => {})", Duration::from_millis(26)));
    assert!(!chrome.wait_for("Promise.resolve(false)", Duration::ZERO));
    assert!(chrome.wait_for("Promise.resolve(true)", Duration::ZERO));
    chrome.eval("window.idlePolls=0");
    assert!(!chrome.wait_for(
        "(window.idlePolls++, document.querySelector('#missing'))",
        Duration::from_millis(26)
    ));
    let polls = chrome.eval("window.idlePolls").as_u64().unwrap();
    assert!(
        (1..=2).contains(&polls),
        "idle page evaluated {polls} times"
    );
    // A false condition that itself does renderer work must also exhaust its allowance.
    assert!(!chrome.wait_for(
        "(() => { const until=performance.now()+50; while(performance.now()<until) {} return false; })()",
        Duration::from_millis(1)
    ));
    chrome.eval("window.observed=false; setTimeout(()=>{window.observed=true}, 5)");
    chrome.pump(Duration::from_millis(10));
    assert!(chrome.wait_for("window.observed === true", PROTOCOL_CPU));
    chrome.eval("setTimeout(() => location.href='about:blank#next', 10)");
    // Page navigation may discard its own promises. The observation marker's
    // session belongs to the stable blank target and must still answer.
    chrome.pump(Duration::from_millis(50));
    assert!(chrome.wait_for("location.href === 'about:blank#next'", PROTOCOL_CPU));
    chrome.open("data:text/html,<p>after</p>");
    assert_eq!(
        chrome.eval("document.querySelector('p').textContent"),
        json!("after")
    );
}

#[test]
fn navigation_and_protocol_replies_end_on_completed_observer_turns() {
    use tungstenite::protocol::Role;

    for (navigation, observer) in [(true, true), (false, true), (false, false)] {
        let profile = next_profile();
        std::fs::create_dir(&profile).unwrap();
        let idle = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        let peer = std::thread::spawn(move || {
            let mut reader =
                WebSocket::from_raw_socket(peer.try_clone().unwrap(), Role::Server, None);
            let (send, receive) = std::sync::mpsc::channel::<Value>();
            let writer = std::thread::spawn(move || {
                let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
                for value in receive {
                    if socket.send(Message::text(value.to_string())).is_err() {
                        break;
                    }
                }
            });
            let mut held = false;
            let mut polls = 0;
            while let Ok(Message::Text(text)) = reader.read() {
                let command: Value = serde_json::from_str(text.as_str()).unwrap();
                let id = command["id"].as_u64().unwrap();
                if !held {
                    assert_eq!(
                        command["method"],
                        if navigation {
                            "Page.navigate"
                        } else {
                            "Fixture.held"
                        }
                    );
                    held = true;
                    if navigation {
                        // Navigation is accepted, but its document never finishes parsing
                        // within the allowance. open must fail on the missing event.
                        send.send(json!({"id":id,"result":{}})).unwrap();
                    }
                    send.send(json!({"method":"Fixture.unrelated"})).unwrap();
                    // A finite transcript supplies the late success even if observation
                    // is removed. That regression returns successfully instead of hanging
                    // the test, and must fail the expect_err below. Replies are queued on
                    // a separate writer so observer requests cannot fill both TCP buffers.
                    for marker in id + 1..=id + 2401 {
                        send.send(json!({"id":marker,"result":{}})).unwrap();
                    }
                    send.send(if navigation {
                        json!({"method":"Page.domContentEventFired"})
                    } else {
                        json!({"id":id,"result":{}})
                    })
                    .unwrap();
                } else {
                    assert_eq!(
                        command["method"],
                        if observer {
                            "Runtime.evaluate"
                        } else {
                            "Target.getTargets"
                        }
                    );
                    if observer {
                        assert_eq!(command["sessionId"], "observer");
                    } else {
                        assert!(command.get("sessionId").is_none());
                    }
                    polls += 1;
                }
            }
            drop(send);
            writer.join().unwrap();
            (held, polls)
        });
        let mut chrome = Chrome {
            _owner: Owner {
                leash: idle,
                profile,
                stderr: std::fs::File::open("/dev/null").unwrap(),
                shutdown: std::fs::File::open("/dev/null").unwrap(),
            },
            socket: WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
            next_id: 0,
            session: Some("page".into()),
            observer: observer.then(|| "observer".into()),
            events: Vec::new(),
        };
        let started = Instant::now();
        let timeout = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if navigation {
                chrome.open("https://fixture.invalid/held");
            } else {
                chrome.call("Fixture.held", json!({}));
            }
        }))
        .expect_err("a late event or reply must exhaust completed observer turns");
        let why = timeout.downcast_ref::<String>().unwrap();
        assert!(
            why.contains(if navigation {
                "Page.domContentEventFired"
            } else {
                "Fixture.held"
            }),
            "{why}"
        );
        assert!(why.contains("2400 completed observer polls"), "{why}");
        assert!(
            chrome
                .events
                .iter()
                .any(|event| event["method"] == "Fixture.unrelated")
        );
        eprintln!(
            "navigation={navigation} observer={observer} bounded in {:?}: {why}",
            started.elapsed()
        );
        drop(chrome);
        assert_eq!(
            peer.join().unwrap(),
            (true, if navigation { 2401 } else { 2400 })
        );
    }
}

#[test]
fn cpu_exhaustion_stops_renderer_work_before_the_poll_allowance() {
    use tungstenite::protocol::Role;

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary);
    chrome.open("data:text/html,<p>CPU fixture</p>");
    chrome.eval("window.cpuPolls=0");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let mut renderer = std::mem::replace(
        &mut chrome.socket,
        WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
    );
    let observer = chrome.observer.clone().unwrap();
    let mut renderer_id = chrome.next_id;
    const WORK: &str = "(() => {window.cpuPolls++; let checksum=2166136261; for(let i=0;i<500000000;i++) {checksum=Math.imul(checksum ^ i,16777619);} window.cpuChecksum=checksum; return false;})()";
    let peer = std::thread::spawn(move || {
        let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
        let mut evaluations = 0;
        while let Ok(Message::Text(text)) = socket.read() {
            let mut command: Value = serde_json::from_str(text.as_str()).unwrap();
            let id = command["id"].clone();
            if command["sessionId"] == observer {
                // Observer replies wait behind the renderer's fixed work in this
                // transport. They cannot spend the allowance while work is pending.
                socket
                    .send(Message::text(
                        json!({"id":id,"result":{"result":{"value":null}}}).to_string(),
                    ))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == WORK {
                evaluations += 1;
                assert!(evaluations <= 4, "renderer exceeded its poll allowance");
            }
            renderer_id += 1;
            command["id"] = json!(renderer_id);
            renderer.send(Message::text(command.to_string())).unwrap();
            loop {
                let text = match renderer.read() {
                    Ok(Message::Text(text)) => text,
                    Ok(_) => continue,
                    Err(tungstenite::Error::Io(error))
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        continue;
                    }
                    Err(error) => panic!("renderer relay: {error}"),
                };
                let mut reply: Value = serde_json::from_str(text.as_str()).unwrap();
                if reply["id"] == json!(renderer_id) {
                    reply["id"] = id;
                    socket.send(Message::text(reply.to_string())).unwrap();
                    break;
                }
            }
        }
        evaluations
    });
    assert!(!chrome.wait_for(WORK, Duration::from_millis(100)));
    let polls = chrome.eval("window.cpuPolls").as_u64().unwrap();
    assert!(
        (1..4).contains(&polls),
        "CPU must end the wait before four polls, got {polls}"
    );
    assert_eq!(
        chrome.eval("typeof window.cpuChecksum === 'number'"),
        json!(true)
    );
    drop(chrome);
    assert_eq!(peer.join().unwrap(), polls);
}
