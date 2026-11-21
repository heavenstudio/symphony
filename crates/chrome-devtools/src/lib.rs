//! Headless Chrome, driven over the DevTools protocol: one browser per launch, on a profile of
//! its own, leashed so it dies with the process that launched it however that ends. The client's
//! browser tests (apps/symphony-web-tests/tests/browser) and `symphony shot` both drive it. One
//! page is under test at a time: the launch's own, in the profile, or one in a browser context of
//! its own that holds nothing of the pages before ([`Chrome::fresh_context`]).
//!
//! A native leash owns and waits for Chrome: the launching program run again ([`Leash`]), which
//! hands itself to [`leash_main`] first. EOF on its launcher's pipe terminates the browser even
//! after SIGKILL, then removes its private profile. Startup failures use the same cleanup.
//! Chrome works in that profile and wakes no updater, so what it starts outside its group (its
//! crash handler, double-forked into a group of its own) works in no launcher's folder.
//!
//! Every wait is bounded by the browser's own work, not the wall: the CPU its recorded process
//! group spends, and the observation turns it completes (a scheduler delay spends nothing). A wait
//! on the page under test counts only the turns that page took part in, answering a probe of its
//! own renderer: a loaded machine can leave that renderer without a single turn for seconds while
//! the rest of the browser runs on, and such a stall spends nothing. A wall-clock backstop, several
//! times the wait's allowance, still ends every wait, with an error naming the stall.

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
/// A wait ends at the latest this many times its allowance after it started, on the wall clock,
/// and never sooner than [`BACKSTOP_FLOOR`]: the stalls of a page's renderer seen on a loaded
/// machine lasted from 2 s to a minute and a half, and a slow observer turn stretches a running
/// wait well past its allowance, neither of which may end it.
const BACKSTOP_TIMES: u32 = 6;
const BACKSTOP_FLOOR: Duration = Duration::from_secs(120);
/// The longest each start of a launch waits on the wall for Chrome to open DevTools, by the rule
/// every wait here keeps: [`BACKSTOP_TIMES`] its CPU allowance ([`PROTOCOL_CPU`]), six minutes.
/// Chrome opens DevTools on about half a second of its group's CPU, in under a second on a quiet
/// machine, which a loaded machine's fair share stretches to tens of seconds: only a Chrome that
/// stalled, alive and spending next to nothing, reaches it.
const LAUNCH_BACKSTOP: Duration = PROTOCOL_CPU.saturating_mul(BACKSTOP_TIMES);
/// A probe of the page under test unanswered this long goes again, so no reply lost to a
/// navigation leaves the page's turns uncounted.
const PROBE_AGAIN: Duration = Duration::from_secs(1);

/// What Chrome answers an evaluation the page's navigation took from it, or one that found no
/// document to run in, each the `message` of its error reply: nothing was evaluated, and the
/// document the page went to can be asked again ([`Chrome::wait_for`]).
const LOST_TO_NAVIGATION: [&str; 3] = [
    // The page went on to a document in its own frame host, which tore the old one's context
    // down while the evaluation was in it.
    "Execution context was destroyed.",
    // The page went on in another frame host (another process, or a new frame while Chrome keeps
    // the page it left for Back): Chrome answers every command still in flight on the old one so.
    // A page closed under an evaluation is not answered at all: Chrome tells of its session
    // detached instead ([`Gone::Closed`]), and refuses each later command for want of a session.
    "Inspected target navigated or closed",
    // V8's answer to an evaluation of a frame with no document's context to run it in: asked
    // between two documents.
    "Cannot find default execution context",
];

/// How the page under test went away, as Chrome told of it: it answers no evaluation then, neither
/// the one in flight nor a probe, so a wait on it ends at once ([`Chrome::wait_for`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gone {
    /// Its session detached (`Target.detachedFromTarget`, naming it, on the browser's session):
    /// the page closed. Each command after it is refused for want of a session.
    Closed,
    /// Its renderer crashed or was killed (`Inspector.targetCrashed`, on its own session); a
    /// reload or a navigation starts another (`Inspector.targetReloadedAfterCrash`).
    Crashed,
}

impl fmt::Display for Gone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Gone::Closed => "the page under test closed: its session detached",
            Gone::Crashed => "the page under test's renderer crashed",
        })
    }
}

/// What went wrong driving Chrome, in one message that names what was waited for.
#[derive(Debug)]
pub struct Error {
    message: String,
    past_deadline: bool,
    stalled: bool,
    /// An evaluation's reply was one of [`LOST_TO_NAVIGATION`]'s.
    navigated: bool,
    /// An evaluation's expression threw.
    threw: bool,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            past_deadline: false,
            stalled: false,
            navigated: false,
            threw: false,
        }
    }

    fn deadline(what: &str) -> Self {
        Self {
            message: format!("{what}: no reply before the deadline"),
            past_deadline: true,
            stalled: false,
            navigated: false,
            threw: false,
        }
    }

    fn stall(what: &str, budget: &WaitBudget) -> Self {
        Self {
            message: format!("{what}: {}", budget.stall()),
            past_deadline: false,
            stalled: true,
            navigated: false,
            threw: false,
        }
    }

    /// The error reply `error` to the evaluation of `expression`, which says whether the page's
    /// navigation took the evaluation from it ([`LOST_TO_NAVIGATION`]).
    fn evaluation(expression: &str, error: &Value) -> Self {
        Self {
            navigated: error["message"]
                .as_str()
                .is_some_and(|message| LOST_TO_NAVIGATION.contains(&message)),
            ..Self::new(format!("{expression}: {error}"))
        }
    }

    /// A wait on `what` the page under test's end cut short, saying how it went ([`Gone`]).
    fn gone(what: &str, gone: Gone) -> Self {
        Self::new(format!("{what}: {gone}"))
    }

    /// Whether the wait ended because the deadline [`Chrome::set_deadline`] set had passed, rather
    /// than because Chrome failed.
    pub fn past_deadline(&self) -> bool {
        self.past_deadline
    }

    /// Whether the wait ended on its wall-clock backstop: the page under test took too few turns
    /// in it to spend its allowance, its renderer stalled, or Chrome stalled before it opened
    /// DevTools.
    pub fn stalled(&self) -> bool {
        self.stalled
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
/// scheduler delays spend nothing; an observer reply proves the browser had a turn. On a wait on
/// the page under test ([`WaitBudget::on_page`]) an observer turn counts only once that page has
/// answered a probe since the last turn counted, so a stalled renderer spends nothing while the
/// observer runs on. A caller's deadline, when there is one, ends the wait too, however little
/// the browser has spent, and the wall-clock backstop ends it at the latest.
struct WaitBudget {
    cpu: process::Budget,
    polls: u128,
    max_polls: u128,
    deadline: Option<Instant>,
    page: Option<PageTurns>,
    started: Instant,
    allowance: Duration,
    floor: Duration,
    end: Option<End>,
}

/// The check that ended a wait, the first [`WaitBudget::exhausted`] found true: what its error
/// says, whatever the clock reads by the time the error is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// The caller's deadline had passed.
    Deadline,
    /// The wall-clock backstop ran out: the browser, or the page under test, stalled.
    Backstop,
    /// The allowance was spent, in browser CPU or in completed turns.
    Allowance,
}

/// The page under test's part in a wait's observer turns: the probes of it not answered yet and
/// when the last went, whether it answered one since the last turn counted, when it last
/// answered, and how many observer turns passed without it.
struct PageTurns {
    probes: Vec<u64>,
    asked: Instant,
    answered: bool,
    last: Instant,
    passed: u128,
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
            page: None,
            started: Instant::now(),
            allowance: cpu,
            floor: BACKSTOP_FLOOR,
            end: None,
        }
    }

    fn until(self, deadline: Option<Instant>) -> Self {
        Self { deadline, ..self }
    }

    /// The backstop at `floor` at least, [`BACKSTOP_TIMES`] the allowance where that is longer.
    fn floor(self, floor: Duration) -> Self {
        Self { floor, ..self }
    }

    /// Count only the observer turns the page under test took part in.
    fn on_page(self) -> Self {
        let now = Instant::now();
        Self {
            page: Some(PageTurns {
                probes: Vec::new(),
                asked: now,
                answered: false,
                last: now,
                passed: 0,
            }),
            ..self
        }
    }

    fn backstop(&self) -> Duration {
        self.floor
            .max(self.allowance.saturating_mul(BACKSTOP_TIMES))
    }

    fn past_deadline(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn past_backstop(&self) -> bool {
        self.started.elapsed() >= self.backstop()
    }

    /// Whether the wait is over: past the caller's deadline, past the backstop, or its allowance
    /// spent, checked in that order. The first check found true is the wait's [`End`], kept for
    /// its error.
    fn exhausted(&mut self, members: &[process::Member]) -> bool {
        if self.end.is_none() {
            self.end = if self.past_deadline() {
                Some(End::Deadline)
            } else if self.past_backstop() {
                Some(End::Backstop)
            } else if self.cpu.exhausted(members) || self.polls >= self.max_polls {
                Some(End::Allowance)
            } else {
                None
            };
        }
        self.end.is_some()
    }

    /// Whether the backstop ended the wait: its renderer, or the browser, stalled.
    fn stalled(&self) -> bool {
        self.end == Some(End::Backstop)
    }

    /// One turn the page took part in, as the reply to an evaluation in it proves.
    fn poll(&mut self, members: &[process::Member]) -> bool {
        self.polls += 1;
        self.exhausted(members)
    }

    /// An observer turn completed: it counts unless the wait is on a page that has not answered
    /// a probe since the last turn counted.
    fn turn(&mut self, members: &[process::Member]) -> bool {
        match &mut self.page {
            Some(page) if !page.answered => page.passed += 1,
            Some(page) => {
                page.answered = false;
                self.polls += 1;
            }
            None => self.polls += 1,
        }
        self.exhausted(members)
    }

    /// Whether the reply `id` answers a probe of the page; if so, the page took its turn.
    fn answered(&mut self, id: &Value) -> bool {
        let Some(page) = &mut self.page else {
            return false;
        };
        if !id.as_u64().is_some_and(|id| page.probes.contains(&id)) {
            return false;
        }
        page.probes.clear();
        page.answered = true;
        page.last = Instant::now();
        true
    }

    /// Whether to probe the page now: it has not answered since the last turn counted, and no
    /// probe is in flight or the last went [`PROBE_AGAIN`] ago.
    fn probe_due(&self) -> bool {
        self.page.as_ref().is_some_and(|page| {
            !page.answered && (page.probes.is_empty() || page.asked.elapsed() >= PROBE_AGAIN)
        })
    }

    fn probed(&mut self, id: u64) {
        if let Some(page) = &mut self.page {
            page.probes.push(id);
            page.asked = Instant::now();
        }
    }

    /// What the wait spent, as an exhausted wait's error says it.
    fn spent(&self) -> String {
        format!(
            "{} completed observer polls and {:?} CPU",
            self.polls,
            self.cpu.used()
        )
    }

    /// Why the backstop ended the wait, with the page's part in its turns.
    fn stall(&self) -> String {
        let took = self.started.elapsed();
        let load = load_said(load_average());
        match &self.page {
            Some(page) => format!(
                "the page under test stalled: its renderer took part in {} of {} observer turns \
                 and in none for the last {:?}, and the wait's wall-clock backstop of {:?} ran \
                 out after {took:?} and {:?} CPU; {load}",
                self.polls,
                self.polls + page.passed,
                page.last.elapsed(),
                self.backstop(),
                self.cpu.used()
            ),
            None => format!(
                "the browser stalled: the wait's wall-clock backstop of {:?} ran out after \
                 {took:?}, {}; {load}",
                self.backstop(),
                self.spent()
            ),
        }
    }

    /// Why a wait on `what` ended without what it waited for, as its [`End`] says: past the
    /// caller's deadline, a stall that ran to the backstop, or else its allowance spent (`spent`,
    /// naming it). A wait whose allowance ran out is no deadline, however late its error is made.
    fn ended(&self, what: &str, spent: String) -> Error {
        match self.end {
            Some(End::Deadline) => Error::deadline(what),
            Some(End::Backstop) => Error::stall(what, self),
            Some(End::Allowance) | None => Error::new(spent),
        }
    }
}

/// Whether `targets`, a `Target.getTargets` answer, lists the target `id`.
fn listed(targets: &Value, id: &str) -> bool {
    targets["targetInfos"]
        .as_array()
        .is_some_and(|infos| infos.iter().any(|info| info["targetId"] == id))
}

const CHROMES: [&str; 4] = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "google-chrome",
    "chromium",
    "chrome",
];

/// The Chrome binary on this machine: `SYMPHONY_CHROME`, else the first of `CHROMES` found.
pub fn find() -> Option<String> {
    if let Some(named) = symphony_env::CHROME.os() {
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
    /// Bytes on the wire, headers included: the finished total, or what arrived so far. An open
    /// event stream counts its response head and the body the page has read: Chrome reports a
    /// stream's transfer size one chunk late, so its last message would wait for the next one. A
    /// WebSocket ([`WEB_SOCKET`]) counts its handshake's answer and every frame it received.
    pub bytes: f64,
    /// It finished: its answer arrived whole, or a WebSocket closed.
    pub finished: bool,
    /// The keep-alive pings an event stream delivered, each named by Chrome's
    /// `Network.eventSourceMessageReceived`, or a WebSocket, each a frame whose message is named
    /// a ping; their bytes are in `bytes`.
    pub pings: usize,
    /// Why it failed (`net::ERR_…`), when it did; such a request never finishes.
    pub failed: Option<String>,
    /// The page itself gave it up (an abort, or the page going away), rather than the network.
    /// A request still open when its frame was removed from the page counts as given up too (a
    /// frame goes with the document that held it, when a frame around it goes to another
    /// document), and so does a frame's document still arriving when the frame went on to another
    /// document, one a followed frame's document ([`Chrome::follow_frames`]) made, still open when
    /// the frame left that document's process, one a followed worker made, still open when it
    /// ended, and a shared worker's script, whose answer goes to the worker; never a frame's own
    /// document as it moves between processes, which it goes on loading.
    pub canceled: bool,
    /// The frame it was made for, when that is not the page's own: the address of the document it
    /// was made for, a frame's own document request naming its own address (`about:srcdoc` for a
    /// `srcdoc` frame's). `None` for the page's own frame, or before the page's first navigation,
    /// and for a worker's request.
    pub frame: Option<String>,
    /// The worker that made it, by its script's address, when a worker followed
    /// ([`Chrome::follow_frames`]) made it: its own script's request, which its page or frame
    /// starts, among them.
    pub worker: Option<String>,
    /// A speculative `<link rel=prefetch>` (its `Purpose` or `Sec-Purpose` header says so): a
    /// later use's, warmed at idle, not the page's own.
    pub prefetch: bool,
}

/// The [`Request::kind`] of a WebSocket, as Chrome names the resource type.
pub const WEB_SOCKET: &str = "WebSocket";

impl Request {
    /// Whether it is a stream its page holds open for as long as it follows it, an event stream or
    /// a WebSocket: one never finished while open.
    pub fn streams(&self) -> bool {
        self.kind == "EventSource" || self.kind == WEB_SOCKET
    }
}

/// The bytes of a WebSocket's handshake answer (a DevTools `WebSocketResponse`): its head as sent,
/// or, when Chrome gives no text of it, its status line and headers as they are written.
fn handshake_bytes(response: &Value) -> f64 {
    if let Some(head) = response["headersText"].as_str() {
        return head.len() as f64;
    }
    let status = format!(
        "HTTP/1.1 {} {}\r\n",
        response["status"].as_i64().unwrap_or(0),
        response["statusText"].as_str().unwrap_or("")
    );
    let headers: usize = response["headers"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(name, values)| {
            // Chrome joins a header sent more than once with line feeds.
            let values = values.as_str().unwrap_or("");
            values
                .split('\n')
                .map(move |value| name.len() + value.len() + 4)
        })
        .sum();
    (status.len() + headers + 2) as f64
}

/// The bytes of a frame from a server carrying `payload` bytes: unmasked, with the shortest length
/// field that holds it (RFC 6455, 5.2).
fn frame_bytes(payload: usize) -> f64 {
    let head = match payload {
        0..=125 => 2,
        126..=65_535 => 4,
        _ => 10,
    };
    (head + payload) as f64
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

/// The variable a leash reads its browser and profile from, as JSON `[binary, profile]`. The
/// launching program sets it for itself run again, so no person sets it and no read says anything.
const LEASH: symphony_env::Var = symphony_env::CHROME_LEASH;

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
    let config = std::env::var(LEASH.name).ok()?;
    let Ok((binary, profile)) = serde_json::from_str::<(String, PathBuf)>(&config) else {
        leash_report(format_args!(
            "Chrome leash: unreadable {} {config:?}",
            LEASH.name
        ));
        return Some(2);
    };
    Some(leash(&binary, &profile))
}

fn leash(binary: &str, profile: &Path) -> i32 {
    let started = std::fs::File::create(profile.with_extension("stderr")).and_then(|stderr| {
        Command::new(binary)
            .args(chrome_args(profile))
            // Chrome works in its profile, never in its launcher's folder (a test's package, in a
            // release run's checkout): what it starts that leaves its group, as its crash handler
            // does (double-forked into a group of its own), works where it was started until it
            // ends, and a process working in a release run's checkout stops that run.
            .current_dir(profile)
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
        Ok(child) => BrowserGroup { child, done: false },
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
    let pid = browser.child.id();
    if let Err(error) = std::fs::write(profile.join("browser.pid"), format!("{pid}\n")) {
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
        match exited(pid) {
            Ok(true) => break browser.end().ok(),
            Ok(false) => {}
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
    drop(browser);
    let cleanup = cleanup_profile(profile, pid);
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
        // No updater. Chrome otherwise wakes Google's updater 19 s in, once its own checks of
        // the updater have run (`updater::SchedulePeriodicTasks`): `GoogleUpdater --wake-all`,
        // started in a process group of its own so no signal to Chrome's group reaches it, which
        // starts crash handlers of its own and outlives the leash by minutes, all of them working
        // where Chrome was started.
        "--disable-updater-scheduler",
        // A same-site visit keeps the page's frame and widget (no RenderDocument). Chrome
        // otherwise gives the visit a new frame and, a second later, tears the left document's
        // compositor down on the page's main thread, which waits there for that document's
        // raster tasks; its thread pool runs them at background priority, which a loaded machine
        // left unscheduled for up to a minute and a half, the page running no task meanwhile. A
        // left page Chrome keeps for Back still gets a new frame. Chrome takes only the last
        // --disable-features given, so every feature turned off is in this one.
        "--disable-features=Translate,OptimizationHints,MediaRouter,RenderDocument",
        // Motion as a page asks for it, whatever the machine's own setting: Chrome otherwise
        // reads the system's "reduce motion" (on, on a hosted macOS runner) into
        // `prefers-reduced-motion`, and a page is drawn and measured one way here and another
        // there. A caller that wants the reduced page emulates it (`Emulation.setEmulatedMedia`).
        "--force-prefers-no-reduced-motion",
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
/// leash and its browser keep no pipe or socket their launcher's own parent handed it. A copy in
/// this package, so the package crates.io gets builds alone; crates/symphony/tests/packages.rs
/// holds it equal to the script the fixtures run.
const CLOSE_FDS: &str = include_str!("fixture_process.sh");

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

    /// The browser's process id, which leads its group, once its leash has recorded it.
    fn browser(&self) -> Option<u32> {
        std::fs::read_to_string(self.profile.join("browser.pid"))
            .ok()
            .filter(|text| text.ends_with('\n'))
            .and_then(|text| text.trim().parse::<u32>().ok())
            .filter(|pid| *pid > 0)
    }

    fn snapshot(&self) -> std::io::Result<Vec<process::Member>> {
        process::members(self.browser().unwrap_or(self.leash.id()))
    }

    /// Success comes only from the handshake. The leash's exit (which the browser's own exit
    /// makes), `cpu` of the group's CPU spent, or the wall-clock `backstop` reached first by a
    /// browser alive that neither answered nor spent its CPU (a stall, [`StartStalled`]) fails it,
    /// saying what was seen.
    fn ready<T>(
        &mut self,
        what: &str,
        cpu: Duration,
        backstop: Duration,
        mut handshake: impl FnMut(&Path) -> Option<T>,
    ) -> std::io::Result<T> {
        let started = Instant::now();
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
            let browser = self.browser();
            let members = process::members(browser.unwrap_or(self.leash.id()))?;
            if budget.exhausted(&members) {
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    format!("Chrome spent {:?} CPU without {what}", budget.used()),
                ));
            }
            let waited = started.elapsed();
            if waited >= backstop {
                let watched = match browser {
                    Some(pid) => format!(
                        "Chrome (pid {pid}) has {} alive in its group, which",
                        match members.len() {
                            1 => "1 process".to_string(),
                            alive => format!("{alive} processes"),
                        }
                    ),
                    None => format!(
                        "its leash (pid {}) had not recorded Chrome's pid, and",
                        self.leash.id()
                    ),
                };
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    StartStalled(format!(
                        "Chrome stalled before {what}: {watched} spent {:?} CPU in the wait, \
                         and the launch's wall-clock backstop of {backstop:.1?} ran out after \
                         {waited:.1?}; {}",
                        budget.used(),
                        load_said(load_average())
                    )),
                ));
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again for the browser's answer, where its pipe is the event"
            )]
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
            .env(LEASH.name, json!([binary, profile]).to_string())
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

/// What [`Owner::ready`] fails with once its wall-clock backstop ran out, the browser alive and its
/// CPU allowance unspent: Chrome stalled at its start.
#[derive(Debug)]
struct StartStalled(String);

impl fmt::Display for StartStalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StartStalled {}

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

/// The browser, leading a group of its own with its helpers. It is reaped only after the SIGKILL
/// to its group, so the group's id is its own until then and the signal reaches no group numbered
/// after it.
struct BrowserGroup {
    child: Child,
    /// Set once its group is signalled no more: after its SIGKILL, or once its leader is reaped.
    done: bool,
}

impl BrowserGroup {
    /// SIGKILL to the group, its helpers with it (they must die before the leash finishes profile
    /// cleanup or exits), then the browser reaped: how it ended.
    fn end(&mut self) -> std::io::Result<std::process::ExitStatus> {
        if !self.done {
            let _ = signal_group(self.child.id(), 9); // SIGKILL, on Darwin and Linux
            self.done = true;
        }
        self.child.wait()
    }

    /// The browser waited for with no signal to its group, which is signalled no more after: a
    /// test's wait for a browser it lets end.
    #[cfg(test)]
    fn reap(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.done = true;
        self.child.wait()
    }
}

impl Drop for BrowserGroup {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// Whether the child `pid` has ended, left unreaped (`waitid` with `WNOWAIT`): until it is reaped
/// its pid, and the id of the group it leads, name it and no later process. A browser stopped (a
/// test stalls one) has not ended, which macOS answers as well though only `WEXITED` is asked.
fn exited(pid: u32) -> std::io::Result<bool> {
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: asks whether this process's child `pid` ended, writing `info`, reaping none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::from(pid),
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    if waited != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(info.si_signo == libc::SIGCHLD
        && matches!(
            info.si_code,
            libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
        ))
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

/// The machine's load average over 1, 5 and 15 minutes, as `getloadavg` reads it.
fn load_average() -> Option<[f64; 3]> {
    let mut load = [0.0f64; 3];
    // SAFETY: writes at most three doubles into `load`, which holds three.
    let read = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
    (read == 3).then_some(load)
}

/// A load average as a stall's error says it, in the words of the tests' own stalls
/// (`test_time::Load`).
fn load_said(load: Option<[f64; 3]>) -> String {
    match load {
        Some([one, five, fifteen]) => {
            format!("load average {one:.2} {five:.2} {fifteen:.2} over 1, 5 and 15 minutes")
        }
        None => "no load average to read".into(),
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
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for a process group's end, which no wait reports"
        )]
        std::thread::sleep(POLL);
    }
}

pub struct Chrome {
    owner: Owner,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    session: Option<String>,
    /// The browser context [`Chrome::fresh_context`] made for the page under test, and that
    /// page's target; `None` while the page is the launch's own, in the profile's context.
    context: Option<(String, String)>,
    /// The launcher's blank target survives navigation in the page under test.
    observer: Option<String>,
    /// Every event since the last `open`, in order.
    events: Vec<Value>,
    /// When each of `events` was read from the browser.
    arrivals: Vec<Instant>,
    /// When every call and wait must end by, if the caller set it.
    deadline: Option<Instant>,
    /// The least a wait's wall-clock backstop allows ([`BACKSTOP_FLOOR`]).
    backstop: Duration,
    /// The page's own frame, as its last navigation named it.
    frame: Option<String>,
    /// Whether the frames Chrome runs in processes of their own, and the workers, are followed
    /// ([`Chrome::follow_frames`]).
    following: bool,
    /// Whether every JavaScript dialog is answered as it opens ([`Chrome::dismiss_dialogs`]).
    dismissing: bool,
    /// What each frame followed turns on beyond its network and page events
    /// ([`Chrome::enable_in_frames`]).
    frame_domains: Vec<String>,
    /// The targets of the pages this driver opened itself: the observer's and the page under
    /// test's, which [`Chrome::keep_in_front`] tells from the pages the page opens.
    own_pages: Vec<String>,
    /// Whether the page under test is brought back in front as soon as another page appears or
    /// commits ([`Chrome::keep_in_front`]).
    fronting: bool,
    /// How the page under test went away, as the last of Chrome's messages that told of it said
    /// ([`Chrome::note_gone`]); `None` while it runs.
    gone: Option<Gone>,
}

/// A mid phone's screen, as the browser tests emulate one: 390x844 CSS pixels at 3x, mobile.
pub const PHONE: (u32, u32, u32) = (390, 844, 3);

impl Chrome {
    /// Chrome at `binary`, on a new profile, under a leash `leash` starts, with one blank page
    /// attached and its page and network events on. A Chrome that exits first or spends a minute
    /// of CPU without opening DevTools fails its launch with an error saying which. One that
    /// stalls, alive and opening none for six minutes on the wall, is ended and Chrome started
    /// once more, on a profile of its own; a second start that fails fails the launch, its error
    /// naming the stall before it.
    pub fn launch(binary: &str, leash: &Leash) -> Result<Chrome> {
        Self::launch_within(leash, [(binary, LAUNCH_BACKSTOP); 2])
    }

    /// [`Chrome::launch`], its first start running the binary `first` names and waiting at most
    /// its backstop on the wall for Chrome to open DevTools, and the start after a stall `again`'s.
    /// A start that stalled has run nothing of its caller's. A Chrome hung at its start, blocked
    /// rather than starved, stays hung while others start beside it on the same loaded machine:
    /// it is ended with its group and profile as the start returns, and the next starts afresh.
    fn launch_within(leash: &Leash, [first, again]: [(&str, Duration); 2]) -> Result<Chrome> {
        match Self::start(first.0, leash, first.1) {
            Err(stall) if stall.stalled => {
                leash_report(format_args!("{stall}; starting Chrome again"));
                Self::start(again.0, leash, again.1).map_err(|error| Error {
                    message: format!("{error}; the start before it: {stall}"),
                    ..error
                })
            }
            started => started,
        }
    }

    /// One start of [`Chrome::launch_within`]'s, waiting at most `backstop` on the wall for
    /// DevTools: a failure there that the backstop ended is [`Error::stalled`].
    fn start(binary: &str, leash: &Leash, backstop: Duration) -> Result<Chrome> {
        let mut owner = Owner::launch(binary, leash)?;
        let (port, path) = owner
            .ready(
                "DevTools port readiness",
                PROTOCOL_CPU,
                backstop,
                devtools_endpoint,
            )
            .map_err(|error| Error {
                stalled: error
                    .get_ref()
                    .is_some_and(|inner| inner.is::<StartStalled>()),
                ..Error::new(format!(
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
            context: None,
            observer: None,
            events: Vec::new(),
            arrivals: Vec::new(),
            deadline: None,
            backstop: BACKSTOP_FLOOR,
            frame: None,
            following: false,
            dismissing: false,
            frame_domains: Vec::new(),
            own_pages: Vec::new(),
            fronting: false,
            gone: None,
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
        chrome.own_pages = vec![blank];
        chrome.page_in(None)?;
        Ok(chrome)
    }

    /// A blank page in `context` (the profile's own with none) made the page under test, its page
    /// and network events on, as a launch leaves it; its target. The page before is only let go
    /// of, never closed.
    fn page_in(&mut self, context: Option<&str>) -> Result<String> {
        let mut params = json!({"url": "about:blank"});
        if let Some(context) = context {
            params["browserContextId"] = json!(context);
        }
        let target = self.browser_call("Target.createTarget", params)?;
        let target_id = target["targetId"]
            .as_str()
            .ok_or_else(|| Error::new(format!("Target.createTarget: no target in {target}")))?
            .to_string();
        let attached = self.browser_call(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        )?;
        self.session = Some(session_id(&attached)?);
        self.frame = None;
        self.gone = None;
        self.own_pages.truncate(1);
        self.own_pages.push(target_id.clone());
        self.call("Page.enable", json!({}))?;
        self.call("Network.enable", json!({}))?;
        Ok(target_id)
    }

    /// Leave the page under test for a blank one in a new browser context, as an incognito window
    /// is: no cookie, storage, cache entry or connection of any page before, in this browser, and
    /// kept in memory alone. The page is as a launch leaves it, so what was set on the page before
    /// is set again by its caller (an emulated screen, a script for each new document,
    /// [`Chrome::follow_frames`], [`Chrome::keep_in_front`], [`Chrome::dismiss_dialogs`],
    /// [`Chrome::enable_in_frames`]); a deadline stays ([`Chrome::set_deadline`]). The page
    /// before is closed, its context disposed of with every page in it where this made it, and
    /// its events are forgotten. One browser's cold visits each take one, where a launch each
    /// would only start Chrome again: every wait's allowance is still this browser's own work.
    pub fn fresh_context(&mut self) -> Result<()> {
        let left = self.own_pages.get(1).cloned();
        let before = self.context.take();
        if self.following || self.fronting {
            (self.following, self.fronting) = (false, false);
            self.discover()?;
        }
        self.dismissing = false;
        self.frame_domains.clear();
        let created = self.browser_call("Target.createBrowserContext", json!({}))?;
        let context = created["browserContextId"]
            .as_str()
            .ok_or_else(|| Error::new(format!("Target.createBrowserContext: none in {created}")))?
            .to_owned();
        let target = self.page_in(Some(&context))?;
        self.context = Some((context, target));
        // The page before is let go of first, so its end is not taken for this page's.
        match (before, left) {
            (Some((context, _)), _) => {
                self.browser_call(
                    "Target.disposeBrowserContext",
                    json!({"browserContextId": context}),
                )?;
            }
            (None, Some(page)) => {
                self.browser_call("Target.closeTarget", json!({"targetId": page}))?;
                self.wait_closed(&page)?;
            }
            (None, None) => {}
        }
        self.forget_events();
        Ok(())
    }

    /// Wait until the browser lists no target `page`: `Target.closeTarget` answers once the close
    /// is asked for, and the page goes after it, later the busier the machine. A context's
    /// disposal answers only once its pages are gone.
    fn wait_closed(&mut self, page: &str) -> Result<()> {
        let mut budget = self.budget(PROTOCOL_CPU)?;
        loop {
            let targets = self.browser_call("Target.getTargets", json!({}))?;
            if !listed(&targets, page) {
                return Ok(());
            }
            if budget.poll(&self.owner.members()?) {
                let what = format!("the closed page {page}");
                return Err(budget.ended(
                    &what,
                    format!("{what}: still a target after {}", budget.spent()),
                ));
            }
            // A turn of the browser's between two looks, as a wait on a page takes: looks back
            // to back would spend the allowance in a second and take the browser from the close.
            self.pump(POLL)?;
        }
    }

    /// From now on, every call and wait here ends by `deadline` at the latest, whatever its own
    /// allowance of browser CPU and observation turns, failing with an error that says so
    /// ([`Error::past_deadline`]); `None` lifts it. The waits are otherwise as they were.
    pub fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// A wait's allowance: `cpu` of browser work and as many observation turns, the deadline and
    /// the backstop.
    fn budget(&self, cpu: Duration) -> Result<WaitBudget> {
        Ok(WaitBudget::new(cpu, &self.owner.members()?)
            .until(self.deadline)
            .floor(self.backstop))
    }

    /// As [`Chrome::budget`], for a wait on the page under test: only the observation turns it
    /// took part in count, once it has a session.
    fn page_budget(&self, cpu: Duration) -> Result<WaitBudget> {
        let budget = self.budget(cpu)?;
        Ok(if self.session.is_some() {
            budget.on_page()
        } else {
            budget
        })
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

    /// From now on, follow each frame of the page that Chrome runs in a process of its own (a
    /// cross-site frame, a sandboxed one, and such frames within it), and each dedicated worker (of
    /// the page, of a frame, and the workers it starts): attached as it starts, and held there until
    /// its network events are on, so the answer to its own document or script and every request it
    /// makes are kept with the page's and end in [`Chrome::requests`] as the page's own do.
    /// Unfollowed, such a frame's document request, or a worker's script, is seen to start, on the
    /// page, and never to end: its answer goes to the frame's or the worker's own session. Anything
    /// else the page starts that way is let run as it is. What starts while nothing is asked of the
    /// browser waits for the next call or [`Chrome::pump`], which reads of it and lets it run.
    /// Only the page's own auto-attach is set, never the browser's: a page it opens (a popup) or a
    /// shared worker, which the browser runs, is left as Chrome leaves it. A shared worker is only
    /// discovered (`Target.targetCreated`), so that its script, whose answer goes to the worker,
    /// is given up ([`Chrome::requests`]) and what it loads is not followed.
    pub fn follow_frames(&mut self) -> Result<()> {
        self.following = true;
        self.call("Target.setAutoAttach", auto_attach())?;
        self.discover()
    }

    /// From now on, keep the page under test in front of every page it opens. Each page Chrome
    /// tells of that this driver did not open itself (`Target.targetCreated`: a page the page
    /// opened, by a link or a script; `Target.targetInfoChanged`: its document committed, or its
    /// title) has the page brought back in front at once, unwaited, as its event is read, in any
    /// call or wait: the browser does it without asking the page. A page behind another tab draws
    /// no frames, and Chrome closes the dialog of a tab behind another. A page of the page's own
    /// site opened with its opener runs in the page's renderer, and a dialog it shows in front
    /// holds that renderer, and every call and wait on the page with it, until it is answered:
    /// opened as a tab, its dialog is closed as the page comes back in front, or as it opens once
    /// that tab is behind. A window of its own (`window.open` with window features) stays in front
    /// in its own right whatever the page does, and keeps its dialog. Such a page is only told of
    /// (`Target.setDiscoverTargets`), never attached or held: it runs as Chrome leaves it, and its
    /// dialogs are not among [`Chrome::events`].
    pub fn keep_in_front(&mut self) -> Result<()> {
        self.fronting = true;
        self.discover()
    }

    /// Have the browser tell of the targets that are only told of, never attached to or held:
    /// shared workers once frames are followed ([`Chrome::follow_frames`]), which the browser
    /// runs, and pages once the page is kept in front ([`Chrome::keep_in_front`]), and of none
    /// once neither is ([`Chrome::fresh_context`]). A Chrome that refuses tells of none, and
    /// nothing else is lost.
    fn discover(&mut self) -> Result<()> {
        let filter: Vec<Value> = [(self.following, SHARED_WORKER), (self.fronting, PAGE)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, kind)| json!({ "type": kind }))
            .collect();
        let discover = json!({"discover": !filter.is_empty(), "filter": filter});
        match self.browser_call("Target.setDiscoverTargets", discover) {
            Err(error) if error.past_deadline() => Err(error),
            _ => Ok(()),
        }
    }

    /// From now on, each frame followed ([`Chrome::follow_frames`]) that starts turns on `domains`
    /// too (each an `enable` method, `Runtime.enable` for its console), as the page's own session
    /// may, before it runs: their events come on the frame's own session.
    pub fn enable_in_frames(&mut self, domains: &[&str]) {
        self.frame_domains = domains.iter().map(|domain| (*domain).to_string()).collect();
    }

    /// From now on, answer every JavaScript dialog the page or a frame followed
    /// ([`Chrome::follow_frames`]) opens, as it opens: an `alert`, `confirm` or `prompt` dismissed
    /// (its OK, Cancel and nothing, as the script reads them), a `beforeunload` accepted (the page
    /// left, as what started its navigation asked). Unanswered, a dialog holds its document's
    /// renderer, and every wait on it, until it is answered. Each stays among [`Chrome::events`]
    /// as `Page.javascriptDialogOpening`, which names its kind, its text and its document.
    pub fn dismiss_dialogs(&mut self) {
        self.dismissing = true;
    }

    /// Go to `url` and wait for its document to be parsed.
    pub fn open(&mut self, url: &str) -> Result<()> {
        self.forget_events();
        let navigated = self.call("Page.navigate", json!({"url": url}))?;
        self.frame = navigated["frameId"].as_str().map(str::to_owned);
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
        self.frame = navigated["frameId"].as_str().map(str::to_owned);
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

    /// Evaluate `expression` in the page (awaiting a promise), its value as JSON. It is asked once,
    /// as it may act: an evaluation the page's navigation took from it is an error naming that.
    pub fn eval(&mut self, expression: &str) -> Result<Value> {
        let mut budget = self.page_budget(PROTOCOL_CPU)?;
        self.eval_bounded(expression, &mut budget)?.ok_or_else(|| {
            budget.ended(
                expression,
                format!("{expression}: no evaluation reply after {}", budget.spent()),
            )
        })
    }

    fn eval_bounded(&mut self, expression: &str, budget: &mut WaitBudget) -> Result<Option<Value>> {
        self.evaluate(expression, budget, false)
    }

    /// `expression` evaluated in the page within `budget` (awaiting a promise): its value as JSON,
    /// or `None` once the budget ran out. With `ends_with_the_page`, the page under test going away
    /// before its reply ends it at once with an error saying how ([`Gone`]), as Chrome never
    /// answers it then.
    fn evaluate(
        &mut self,
        expression: &str,
        budget: &mut WaitBudget,
        ends_with_the_page: bool,
    ) -> Result<Option<Value>> {
        let id = self.send(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
            self.session.clone(),
        )?;
        self.watch(budget, |chrome, value| {
            if let Some(gone) = chrome.gone.filter(|_| ends_with_the_page) {
                return Err(Error::gone(expression, gone));
            }
            if value["id"] != json!(id) {
                return Ok(None);
            }
            if let Some(error) = value.get("error") {
                return Err(Error::evaluation(expression, error));
            }
            let result = &value["result"];
            if let Some(exception) = result.get("exceptionDetails") {
                return Err(Error {
                    threw: true,
                    ..Error::new(format!("{expression} threw: {exception}"))
                });
            }
            Ok(Some(result["result"]["value"].clone()))
        })
    }

    /// Wait until `expression` is truthy, bounded by browser CPU and the page's completed turns,
    /// including observer turns while an evaluation's promise is still pending. A page whose
    /// renderer stalls until the wall-clock backstop is an error naming the stall, not a false
    /// condition: it never had the turns to make the condition true. An evaluation the page's
    /// navigation took from it ([`LOST_TO_NAVIGATION`]) is a turn whose condition is not met yet,
    /// and the document the page went to is asked again, and so is a condition that threw on a
    /// document with no element yet ([`Chrome::rootless`]); any other error reply ends the wait. A
    /// page that closed, or whose renderer crashed, before the wait or under it, ends it at once
    /// with an error saying which ([`Gone`]): Chrome answers none of its evaluations then.
    pub fn wait_for(&mut self, expression: &str, cpu: Duration) -> Result<bool> {
        let mut budget = self.page_budget(cpu)?;
        let state = loop {
            if let Some(gone) = self.gone {
                return Err(Error::gone(expression, gone));
            }
            let (met, state) = match self.evaluate(expression, &mut budget, true) {
                Ok(Some(value)) => (
                    !(value.is_null()
                        || value == json!(false)
                        || value == json!(0)
                        || value == json!("")),
                    "condition still false",
                ),
                Ok(None) => break "evaluation still pending",
                Err(error) if error.navigated => (false, "evaluation lost to a navigation"),
                Err(error) if error.threw && self.rootless(&mut budget)? => {
                    (false, "the document has no element yet")
                }
                Err(error) => return Err(error),
            };
            if met {
                return Ok(true);
            }
            // The evaluation's reply is the page's own turn, or the browser's on its behalf as it
            // navigated.
            if budget.poll(&self.owner.members()?) {
                break state;
            }
            self.pump(POLL)?;
        };
        if budget.stalled() {
            return Err(Error::stall(expression, &budget));
        }
        eprintln!("Chrome {state} after {}: {expression}", budget.spent());
        Ok(false)
    }

    /// Whether the page's document has no root element yet, or the page went on while it was
    /// asked: a document the page has just gone to and not begun to read, which a busy machine
    /// leaves so for a while. A condition that reads the root (`document.documentElement.dataset`)
    /// throws there, and says nothing of the page it waits for.
    fn rootless(&mut self, budget: &mut WaitBudget) -> Result<bool> {
        match self.evaluate("document.documentElement === null", budget, true) {
            Ok(rootless) => Ok(rootless != Some(json!(false))),
            Err(error) if error.navigated => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// Observe protocol events while the real browser is paused at a fixture boundary: those of
    /// `method` seen since the last `open`, once there are `count`, or when the page under test
    /// has spent `cpu` of browser work or as many turns.
    pub fn wait_for_protocol_events(
        &mut self,
        method: &str,
        count: usize,
        cpu: Duration,
    ) -> Result<Vec<Value>> {
        let mut budget = self.page_budget(cpu)?;
        let seen = |events: &[Value]| {
            events
                .iter()
                .filter(|event| event["method"] == method)
                .count()
        };
        // What has arrived already counts, whatever the allowance.
        self.pump(POLL)?;
        if seen(&self.events) < count {
            let found = self.watch(&mut budget, |chrome, value| {
                Ok((value.get("id").is_none()
                    && value["method"] == method
                    && seen(&chrome.events) + 1 >= count)
                    .then_some(()))
            })?;
            if found.is_none() && budget.stalled() {
                return Err(Error::stall(&format!("waiting for {method}"), &budget));
            }
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event["method"] == method)
            .map(|event| event["params"].clone())
            .collect())
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

    /// Keep `event`; one that says a frame or a worker followed ([`Chrome::follow_frames`]) has
    /// started is answered first ([`Chrome::follow`]), and so is a dialog opening, once dialogs
    /// are dismissed ([`Chrome::dismiss_dialogs`]), and one of a page the page opened, once the
    /// page is kept in front ([`Chrome::keep_in_front`]).
    fn keep(&mut self, event: Value) -> Result<()> {
        if self.following && event["method"] == "Target.attachedToTarget" {
            self.follow(&event["params"])?;
        }
        if self.fronting && self.opened_by_the_page(&event) {
            // Unwaited: the browser answers it, whatever holds the page's renderer.
            self.send("Page.bringToFront", json!({}), self.session.clone())?;
        }
        if self.dismissing && event["method"] == "Page.javascriptDialogOpening" {
            // Answered on the session it opened on, the page's or a followed frame's, unwaited:
            // a dialog already gone refuses it.
            let session = event["sessionId"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| self.session.clone());
            let leave = event["params"]["type"] == "beforeunload";
            self.send(
                "Page.handleJavaScriptDialog",
                json!({"accept": leave}),
                session,
            )?;
        }
        self.events.push(event);
        self.arrivals.push(Instant::now());
        Ok(())
    }

    /// A target the page started, attached and held (`attached`, as `Target.attachedToTarget`
    /// says it): a frame's network and page events turned on (the frames its document holds
    /// attached and removed), with what [`Chrome::enable_in_frames`] adds, and the frames within it
    /// followed, a worker's network events turned on and the workers it starts followed, then it
    /// is let run, in that order, as its session takes commands in turn. Their replies are not
    /// waited for: a target gone meanwhile refuses them, and nothing is left to follow. Anything
    /// else is only let run.
    fn follow(&mut self, attached: &Value) -> Result<()> {
        let Some(session) = attached["sessionId"].as_str().map(str::to_owned) else {
            return Ok(());
        };
        let domains: Vec<String> = match attached["targetInfo"]["type"].as_str() {
            Some("iframe") => ["Network.enable", "Page.enable"]
                .map(String::from)
                .into_iter()
                .chain(self.frame_domains.iter().cloned())
                .collect(),
            Some(WORKER) => vec!["Network.enable".to_string()],
            _ => Vec::new(),
        };
        for method in &domains {
            self.send(method, json!({}), Some(session.clone()))?;
        }
        if !domains.is_empty() {
            self.send("Target.setAutoAttach", auto_attach(), Some(session.clone()))?;
        }
        if attached["waitingForDebugger"] == true {
            self.send("Runtime.runIfWaitingForDebugger", json!({}), Some(session))?;
        }
        Ok(())
    }

    /// Whether `event` tells of a page this driver did not open itself, as it appears or commits
    /// its document: a page the page opened.
    fn opened_by_the_page(&self, event: &Value) -> bool {
        let target = &event["params"]["targetInfo"];
        matches!(
            event["method"].as_str(),
            Some("Target.targetCreated" | "Target.targetInfoChanged")
        ) && target["type"] == PAGE
            && target["targetId"]
                .as_str()
                .is_some_and(|id| !self.own_pages.iter().any(|own| own == id))
    }

    fn forget_events(&mut self) {
        self.events.clear();
        self.arrivals.clear();
    }

    /// The requests seen since the last `open`, by Chrome's request id: the page's own frame's,
    /// the frames in its process's, and with [`Chrome::follow_frames`] those of every frame.
    pub fn requests(&mut self) -> Result<Vec<Request>> {
        self.pump(Duration::from_millis(50))?;
        Ok(requests(&self.events, self.frame.as_deref()))
    }

    pub fn browser_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.session_call(method, params, None)
    }

    /// One DevTools command; its result. Events read meanwhile are kept.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.session_call(method, params, self.session.clone())
    }

    /// One DevTools command on another target's `session`, as `Target.attachToTarget` with
    /// `flatten` names it (another tab's, say); its result. Events read meanwhile are kept.
    pub fn call_on(&mut self, session: &str, method: &str, params: Value) -> Result<Value> {
        self.session_call(method, params, Some(session.to_owned()))
    }

    fn session_call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value> {
        self.session_call_within(method, params, session, PROTOCOL_CPU)
    }

    /// `method` on `session`, within `allowance`; a call on the page under test's session counts
    /// only the turns that page took part in.
    fn session_call_within(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
        allowance: Duration,
    ) -> Result<Value> {
        let mut budget = if session.is_some() && session == self.session {
            self.page_budget(allowance)?
        } else {
            self.budget(allowance)?
        };
        let id = self.send(method, params, session)?;
        let replied = self.watch(&mut budget, |_, value| {
            if value["id"] != json!(id) {
                return Ok(None);
            }
            if let Some(error) = value.get("error") {
                return Err(Error::new(format!("{method}: {error}")));
            }
            Ok(Some(value["result"].clone()))
        })?;
        replied.ok_or_else(|| {
            budget.ended(
                method,
                format!("{method}: no reply after {}", budget.spent()),
            )
        })
    }
}

/// Why a request still open when the frame it was made for was removed from the page (or a frame
/// around it was, or went on to another document, taking the frames its document held along)
/// never ends.
const FRAME_REMOVED: &str = "its frame was removed";

/// Why a frame's document request still open when the frame went on to another document (its
/// answer still arriving, or a navigation the frame dropped) never ends.
const DOCUMENT_LEFT: &str = "its frame went on to another document";

/// Why a request a followed frame's document made, still open when the frame's session ended,
/// never ends: the document went with the process the frame left (its removal, or its navigation
/// to another site's process or back into the page's own).
const DOCUMENT_GONE: &str = "its document went with the process its frame left";

/// Why a request a followed worker made, still open when the worker's session ended, never ends.
const WORKER_ENDED: &str = "its worker ended";

/// Why a shared worker's script never ends where it is seen to start: its answer goes to the
/// worker, which the browser runs and nothing follows.
const SHARED_WORKER_SCRIPT: &str = "its shared worker is not followed";

/// The kind of target that is a worker a page starts, which [`Chrome::follow_frames`] follows: a
/// dedicated worker, of the page, a frame or a worker.
const WORKER: &str = "worker";

/// The kind of target that is a shared worker, which the browser runs: only discovered
/// ([`Chrome::follow_frames`]), never followed.
const SHARED_WORKER: &str = "shared_worker";

/// The kind of target that is a page (a tab): one the page opens is only discovered
/// ([`Chrome::keep_in_front`]), never followed.
const PAGE: &str = "page";

/// The auto-attach that follows a page's, a followed frame's or a followed worker's frames in
/// processes of their own and workers ([`Chrome::follow_frames`]): each held as it starts, on a
/// session of its own beside the page's.
fn auto_attach() -> Value {
    json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true})
}

/// Whether `frame` is `ancestor` or lies within it, by `parents`, each frame's parent.
fn within<'a>(mut frame: &'a str, ancestor: &str, parents: &'a HashMap<String, String>) -> bool {
    // Each step goes up one frame; more steps than frames would be a loop.
    for _ in 0..=parents.len() {
        if frame == ancestor {
            return true;
        }
        match parents.get(frame) {
            Some(parent) => frame = parent,
            None => return false,
        }
    }
    false
}

/// Whether `frame` was removed from the page, it or a frame around it (by `parents`, each frame's
/// parent), among the frames `removed`: a frame removed takes the frames within it along, whether
/// or not each was told of.
fn removed_with(frame: &str, parents: &HashMap<String, String>, removed: &[String]) -> bool {
    removed.iter().any(|gone| within(frame, gone, parents))
}

/// One navigation of a frame, as `Page.frameNavigated` tells of it: the frame, the loader of the
/// document it went to (a navigation's request is named by its loader) and how many requests had
/// been seen by then.
struct Navigated {
    frame: String,
    loader: String,
    seen: usize,
}

impl Navigated {
    /// Whether the document request `id` of its frame, first seen `at` (`seen` names when each
    /// request was first seen), was left behind by it: not the document it went to, and seen
    /// before that one's request was, or, for a document no request was seen for (a `srcdoc`
    /// one), before the navigation was told of. A navigation started after the one told of goes
    /// on.
    fn left(&self, id: &str, at: usize, seen: &HashMap<&str, usize>) -> bool {
        id != self.loader && at < seen.get(self.loader.as_str()).copied().unwrap_or(self.seen)
    }
}

/// The requests `events` tell of, in the order each was first seen, by Chrome's request id, the
/// page's own frame being `frame`.
///
/// A frame is followed by its id, whichever session tells of it: one request may be told of on
/// two, as a followed frame's document starts on the page's session and is answered on the
/// frame's own, and a frame navigating into the page's own process, or out of it, has its
/// document answered on the session of the process it goes to.
///
/// A request still open counts as given up, as the page's own requests do when it goes away, once
/// the frame it was made for, or a frame around it, is removed from the page (`Page.frameDetached`
/// says `remove`, not a `swap` between processes: [`FRAME_REMOVED`]), and a frame going on to
/// another document (`Page.frameNavigated`) removes every frame its document held, whether or not
/// each removal is told of (one in a process of its own, whose parent leaves its process, is not).
/// A frame's document request still open when the frame went on to another document is given up
/// too ([`Navigated::left`], [`DOCUMENT_LEFT`]): its answer still arriving, or a navigation the
/// frame dropped. So is one a followed frame's document made, still open on the frame's session
/// when that session ends ([`DOCUMENT_GONE`]), but never a frame's own document: a frame whose
/// session ends as it navigates into another process is still loading that document there, and
/// its request ends where it is answered. And so is one a followed worker made, still open when
/// its session ends ([`WORKER_ENDED`]).
///
/// A worker followed is known by its session and by its target, whose id names the request for its
/// script: what is told of on its session, and its script, are its requests ([`Request::worker`]),
/// not a frame's. A shared worker discovered (`Target.targetCreated`) names its script's request
/// by its target too: that request is given up while open ([`SHARED_WORKER_SCRIPT`]), as its
/// answer goes to the worker, and what the worker loads is told of nowhere followed. A worker's
/// script no target names yet is waited for: a dedicated worker attaches only once its script's
/// answer starts.
fn requests(events: &[Value], frame: Option<&str>) -> Vec<Request> {
    let mut requests: HashMap<String, Request> = HashMap::new();
    // The sessions each request was told of on, and the frame each was made for.
    let mut told: HashMap<String, Vec<String>> = HashMap::new();
    let mut made_for: HashMap<String, String> = HashMap::new();
    // Each frame's parent, the frames removed from the page, and each frame's navigations.
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut removed: Vec<String> = Vec::new();
    let mut navigations: Vec<Navigated> = Vec::new();
    // The workers followed, each one's script's address, by its session and by its target, and the
    // shared workers discovered, by their target.
    let mut workers: HashMap<String, String> = HashMap::new();
    let mut scripts: HashMap<String, String> = HashMap::new();
    let mut shared: Vec<String> = Vec::new();
    let mut order = Vec::new();
    for event in events {
        let params = &event["params"];
        match event["method"].as_str().unwrap_or("") {
            "Target.targetCreated" => {
                let target = &params["targetInfo"];
                if let (true, Some(id)) =
                    (target["type"] == SHARED_WORKER, target["targetId"].as_str())
                {
                    shared.push(id.to_string());
                }
                continue;
            }
            "Target.attachedToTarget" => {
                let target = &params["targetInfo"];
                if let (Some(session), Some(id), true) = (
                    params["sessionId"].as_str(),
                    target["targetId"].as_str(),
                    target["type"] == WORKER,
                ) {
                    let script = target["url"].as_str().unwrap_or("").to_string();
                    workers.insert(session.to_string(), script.clone());
                    scripts.insert(id.to_string(), script);
                }
                continue;
            }
            "Target.detachedFromTarget" => {
                let gone = params["sessionId"].as_str();
                let why = if gone.is_some_and(|gone| workers.contains_key(gone)) {
                    WORKER_ENDED
                } else {
                    DOCUMENT_GONE
                };
                for (id, request) in &mut requests {
                    let mut on = told.get(id).into_iter().flatten().map(String::as_str);
                    if !request.finished
                        && request.failed.is_none()
                        && request.kind != "Document"
                        && on.any(|on| Some(on) == gone)
                    {
                        request.failed = Some(why.to_string());
                        request.canceled = true;
                    }
                }
                continue;
            }
            "Page.frameAttached" => {
                if let (Some(child), Some(parent)) =
                    (params["frameId"].as_str(), params["parentFrameId"].as_str())
                {
                    parents.insert(child.to_string(), parent.to_string());
                }
                continue;
            }
            "Page.frameDetached" if params["reason"] == "remove" => {
                if let Some(gone) = params["frameId"].as_str() {
                    removed.push(gone.to_string());
                }
                continue;
            }
            "Page.frameNavigated" => {
                let navigated = &params["frame"];
                if let (Some(id), Some(loader)) =
                    (navigated["id"].as_str(), navigated["loaderId"].as_str())
                {
                    // The frames the document it left held went with that document.
                    let gone: Vec<String> = parents
                        .keys()
                        .filter(|child| {
                            *child != id && within(child, id, &parents) && !removed.contains(child)
                        })
                        .cloned()
                        .collect();
                    removed.extend(gone);
                    navigations.push(Navigated {
                        frame: id.to_string(),
                        loader: loader.to_string(),
                        seen: order.len(),
                    });
                }
                continue;
            }
            _ => {}
        }
        let Some(id) = params["requestId"].as_str() else {
            continue;
        };
        if !requests.contains_key(id) {
            order.push(id.to_string());
        }
        if let Some(session) = event["sessionId"].as_str() {
            let on = told.entry(id.to_string()).or_default();
            if !on.iter().any(|on| on == session) {
                on.push(session.to_string());
            }
        }
        let request = requests.entry(id.to_string()).or_default();
        let stream = request.kind == "EventSource";
        match event["method"].as_str().unwrap_or("") {
            "Network.requestWillBeSent" => {
                request.url = params["request"]["url"].as_str().unwrap_or("").to_string();
                request.kind = params["type"].as_str().unwrap_or("").to_string();
                request.prefetch |= prefetch(&params["request"]["headers"]);
                if let Some(frame) = params["frameId"].as_str() {
                    made_for.insert(id.to_string(), frame.to_string());
                }
                request.frame = match (params["frameId"].as_str(), frame) {
                    (Some(made_for), Some(own)) if made_for != own => Some(
                        params["documentURL"]
                            .as_str()
                            .unwrap_or(&request.url)
                            .to_string(),
                    ),
                    _ => None,
                };
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
            // A WebSocket is told of apart from requests: made, its handshake answered, then each
            // frame it received, the feed's events among them, each one message whose name comes
            // first (`symphony_api`'s feed contract).
            "Network.webSocketCreated" => {
                request.url = params["url"].as_str().unwrap_or("").to_string();
                request.kind = WEB_SOCKET.to_string();
            }
            "Network.webSocketHandshakeResponseReceived" => {
                let response = &params["response"];
                request.status = response["status"].as_i64().unwrap_or(0);
                request.bytes += handshake_bytes(response);
            }
            "Network.webSocketFrameReceived" => {
                let payload = params["response"]["payloadData"].as_str().unwrap_or("");
                request.bytes += frame_bytes(payload.len());
                if payload.split_once('\n').map(|(name, _)| name) == Some(symphony_api::PING_EVENT)
                {
                    request.pings += 1;
                }
            }
            "Network.webSocketClosed" => request.finished = true,
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
    let seen: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(at, id)| (id.as_str(), at))
        .collect();
    for (id, request) in &mut requests {
        if let Some(script) = scripts.get(id).or_else(|| {
            told.get(id)
                .into_iter()
                .flatten()
                .find_map(|session| workers.get(session))
        }) {
            request.worker = Some(script.clone());
            request.frame = None;
        }
        if request.finished || request.failed.is_some() {
            continue;
        }
        if shared.contains(id) {
            request.failed = Some(SHARED_WORKER_SCRIPT.to_string());
            request.canceled = true;
            continue;
        }
        let (Some(frame), Some(at)) = (made_for.get(id), seen.get(id.as_str())) else {
            continue;
        };
        let why = if removed_with(frame, &parents, &removed) {
            FRAME_REMOVED
        } else if request.kind == "Document"
            && navigations
                .iter()
                .any(|navigated| navigated.frame == *frame && navigated.left(id, *at, &seen))
        {
            DOCUMENT_LEFT
        } else {
            continue;
        };
        request.failed = Some(why.to_string());
        request.canceled = true;
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

    /// A probe of the page under test: a reply to it proves the page's renderer ran a task. It
    /// evaluates a constant, so the page's own code sees nothing of it.
    fn page_probe(&mut self) -> Result<u64> {
        self.send(
            "Runtime.evaluate",
            json!({"expression": "0", "returnByValue": true}),
            self.session.clone(),
        )
    }

    /// Note what `message` tells of the page under test's end ([`Chrome::gone`]): its session
    /// detached, its renderer crashed, or a renderer of its started again after a crash. A frame or
    /// a worker followed ([`Chrome::follow_frames`]) detaches and crashes on a session of its own,
    /// which is not the page's.
    fn note_gone(&mut self, message: &Value) {
        let Some(page) = self.session.as_deref() else {
            return;
        };
        let own = message["sessionId"] == page;
        self.gone = match message["method"].as_str() {
            Some("Target.detachedFromTarget") if message["params"]["sessionId"] == page => {
                Some(Gone::Closed)
            }
            Some("Inspector.targetCrashed") if own => Some(Gone::Crashed),
            Some("Inspector.targetReloadedAfterCrash") if own => None,
            _ => return,
        };
    }

    /// Read the browser's messages until `found` makes something of one (that, or the error it
    /// makes of it), or `budget` runs out (`None`), keeping an observer turn in flight throughout
    /// and, on a wait on the page under test, a probe of that page whenever it has not answered
    /// since the last turn counted. What each message tells of the page's end is noted before
    /// `found` sees it ([`Chrome::note_gone`]). Events are kept, the one found among them.
    fn watch<T>(
        &mut self,
        budget: &mut WaitBudget,
        mut found: impl FnMut(&Self, &Value) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let mut marker = self.observer_poll()?;
        loop {
            if let Some(value) = self.read(true)? {
                self.note_gone(&value);
                let made = found(self, &value);
                if !matches!(made, Ok(None)) {
                    if value.get("id").is_none() {
                        self.keep(value)?;
                    }
                    return made;
                }
                if value["id"] == json!(marker) {
                    if value.get("error").is_some()
                        || value["result"].get("exceptionDetails").is_some()
                    {
                        return Err(Error::new(format!("observer poll: {value}")));
                    }
                    if budget.turn(&self.owner.members()?) {
                        return Ok(None);
                    }
                    // The probe goes first: a page that answers at once does so within the next
                    // observer turn, which takes a timer's delay.
                    if budget.probe_due() {
                        let probe = self.page_probe()?;
                        budget.probed(probe);
                    }
                    marker = self.observer_poll()?;
                } else if budget.answered(&value["id"]) {
                    // Any reply, an error too: the page's renderer, or the browser on its
                    // behalf as it navigated, answered.
                } else if value.get("id").is_none() {
                    self.keep(value)?;
                }
            }
            if budget.exhausted(&self.owner.members()?) {
                return Ok(None);
            }
        }
    }

    /// Wait for the page's own `method` event: one a followed frame's session sends (its own
    /// document's `Page.domContentEventFired`) is not the page's.
    fn wait_event(&mut self, method: &str, cpu: Duration) -> Result<()> {
        let page = self.session.clone();
        let own = move |event: &Value| {
            event["method"] == json!(method)
                && event
                    .get("sessionId")
                    .is_none_or(|session| session.as_str() == page.as_deref())
        };
        if self.events.iter().any(&own) {
            return Ok(());
        }
        let mut budget = self.page_budget(cpu)?;
        let found = self.watch(&mut budget, |_, value| {
            Ok((value.get("id").is_none() && own(value)).then_some(()))
        })?;
        found.ok_or_else(|| {
            budget.ended(
                &format!("waiting for {method}"),
                format!("no {method} after {}", budget.spent()),
            )
        })
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
