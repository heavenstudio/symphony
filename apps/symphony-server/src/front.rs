//! Axum serves the Symphony client, JSON API and project media directly. Every request is judged by
//! client address first; read and save handlers also check Host and Origin.

use crate::cli::Serve;
use crate::gate::Gate;
use crate::{orphans, sys};
use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use futures_util::StreamExt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::watch;

mod connections;
mod embeddings;

/// How often the process table is read for leftovers.
const ORPHAN_SECONDS: Duration = Duration::from_secs(60);
/// Allow a cold launcher build, but end a stuck command before the next five-minute sweep.
const SWEEP_TIMEOUT: Duration = Duration::from_secs(240);
/// How far past its interval a round may start before the log says the worker was stalled.
const LATE_SWEEP: Duration = Duration::from_secs(30);
/// Existing connection-shutdown grace, also used to bound a rejected socket's read-side drain.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// The server's address for local tools that link to the app.
const PORT_FILE: &str = "artifacts/docs-app.port";
/// Names the test process that started this server, which then ends with it, however it ends: a test
/// killed by a signal runs no cleanup, and a started server is in a session of its own, out of the
/// runner's process group (`tests/app.rs` sets it on everything it runs).
pub const TEST_PID: &str = "SF_SYMPHONY_TEST_PID";

/// The thread names of the workers a server owns and joins as it ends; a sample of a stuck
/// shutdown shows the one it waits on.
const TAILNET_WORKER: &str = "review tailnet name";
const SWEEP_WORKER: &str = "review idle-session sweep";
pub(crate) const RELOAD_WORKER: &str = "Symphony source reload";
const STARTER_WORKER: &str = "review starter watch";
const TEST_WORKER: &str = "review test watch";

/// What the routes and watchers share.
pub struct Shared {
    pub pid: i64,
    pub port: u16,
    pub root: PathBuf,
    /// One release client for every worktree, installed beside the shared server or built locally.
    pub(crate) ui_dist: PathBuf,
    pub(crate) installed_ui: bool,
    common: Option<PathBuf>,
    gate: Gate,
    orphans: AtomicUsize,
    /// Set as the first census ends, whether it counted or the process table could not be read.
    counted: AtomicBool,
    census_pending: AtomicBool,
    tailnet: Mutex<Option<String>>,
    /// Set once, when the server stops: whoever waits on it sees it, however late it looks.
    stopped: watch::Sender<bool>,
    pub(crate) lifecycle: Mutex<()>,
    /// Pairs with `wake` alone. Background timers must never queue on `lifecycle`, which hub
    /// startup holds across SurrealDB calls.
    sleep: Mutex<()>,
    wake: Condvar,
    pub(crate) replaced: AtomicBool,
    pub(crate) api: tokio::sync::OnceCell<Arc<crate::live::Hub>>,
    pub(crate) saves: std::sync::OnceLock<symphony::save::Saves>,
}

impl Shared {
    pub fn new(pid: i64, port: u16, root: &Path, home: Option<&Path>, gate: Gate) -> Arc<Shared> {
        // --home is carried into the detached process as an argument. Its installed client does
        // not depend on the starter's environment, which Home deliberately drops. Foreground
        // servers retain their root's build and SF_SYMPHONY_UI_DIST override.
        let ui_dist = home.map_or_else(|| crate::live::dist(root), |h| h.join("client/public"));
        Arc::new(Shared {
            pid,
            port,
            root: root.into(),
            ui_dist,
            installed_ui: home.is_some(),
            common: crate::repo::checkout(root)
                .ok()
                .map(|checkout| checkout.common),
            gate,
            orphans: AtomicUsize::new(0),
            counted: AtomicBool::new(false),
            census_pending: AtomicBool::new(false),
            tailnet: Mutex::new(None),
            stopped: watch::Sender::new(false),
            lifecycle: Mutex::new(()),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
            replaced: AtomicBool::new(false),
            api: tokio::sync::OnceCell::new(),
            saves: std::sync::OnceLock::new(),
        })
    }

    pub(crate) fn stopping(&self) -> bool {
        *self.stopped.borrow()
    }

    pub(crate) fn allows_host(&self, host: &str) -> bool {
        self.gate.allows_host(host)
    }

    pub(crate) fn orphan_banner(&self) -> Option<String> {
        let count = self.orphans.load(Ordering::SeqCst);
        (count > 0).then(|| orphans::banner(count))
    }

    /// A canonical file path must stay out of repository metadata, including a common directory
    /// outside the served checkout and any nested checkout's `.git` file or directory.
    pub(crate) fn is_metadata(&self, path: &Path) -> bool {
        self.common
            .as_ref()
            .is_some_and(|common| path.starts_with(common))
            || path.components().any(|part| {
                part.as_os_str()
                    .to_str()
                    .is_some_and(|part| part.eq_ignore_ascii_case(".git"))
            })
    }

    /// Stop the server and finish its live refresh before returning. The lifecycle lock also
    /// covers hub initialization, so a concurrent first request cannot start a worker after stop.
    pub fn stop(&self) {
        {
            let _asleep = self.sleep.lock().unwrap_or_else(|e| e.into_inner());
            self.stopped.send_replace(true);
            self.wake.notify_all();
        }
        let _held = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hub) = self.api.get() {
            hub.stop();
        }
    }

    /// A background timer wakes immediately on shutdown, including when stop preceded the wait.
    fn wait(&self, duration: Duration) -> bool {
        let held = self.sleep.lock().unwrap_or_else(|e| e.into_inner());
        let _held = self
            .wake
            .wait_timeout_while(held, duration, |()| !self.stopping())
            .unwrap_or_else(|e| e.into_inner());
        self.stopping()
    }

    /// Until the server stops.
    async fn until_stopped(&self) {
        let mut stopped = self.stopped.subscribe();
        let _ = stopped.wait_for(|stopped| *stopped).await;
    }

    fn status(&self) -> symphony_api::Front {
        // Read before the count: the census stores its count first, so a status that says counted
        // carries that count.
        let uncounted = !self.counted.load(Ordering::SeqCst);
        symphony_api::Front {
            pid: self.pid,
            port: self.port,
            root: self.root.display().to_string(),
            ready: self.api.get().is_some(),
            app: None,
            tailnet: self
                .tailnet
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            orphans: self.orphans.load(Ordering::SeqCst),
            uncounted,
            census_pending: self.census_pending.load(Ordering::SeqCst),
        }
    }
}

/// The server's routes.
pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route(symphony_api::FRONT_PATH, get(front))
        .fallback(answer)
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(middleware::from_fn(head))
        .layer(middleware::from_fn_with_state(shared.clone(), judge))
        .with_state(shared)
}

/// Let routing and compression see GET's representation before discarding a HEAD body.
async fn head(mut request: Request, next: Next) -> Response {
    if request.method() != Method::HEAD {
        return next.run(request).await;
    }
    *request.method_mut() = Method::GET;
    let response = next.run(request).await;
    let (mut parts, body) = response.into_parts();
    if !parts.status.is_informational()
        && !matches!(
            parts.status,
            StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED
        )
        && !parts.headers.contains_key(header::CONTENT_LENGTH)
    {
        let length = if parts.headers.contains_key(header::CONTENT_ENCODING) {
            // Compression streams and removes the identity length. Count its bytes without
            // retaining a second copy of a potentially large model or download.
            let mut stream = body.into_data_stream();
            let mut length = 0_u64;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(chunk) => length += chunk.len() as u64,
                    Err(error) => {
                        eprintln!("could not encode HEAD representation: {error}");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
            }
            Some(length)
        } else {
            // In particular, do not poll an open-ended event feed to discover its length.
            body.size_hint().exact()
        };
        if let Some(length) = length {
            parts.headers.insert(header::CONTENT_LENGTH, length.into());
        }
    }
    // An empty stream has no exact size hint: when GET omitted the length (304 or SSE),
    // Axum must not infer Content-Length: 0 from the discarded HEAD body.
    Response::from_parts(
        parts,
        Body::from_stream(futures_util::stream::empty::<Result<Vec<u8>, std::io::Error>>()),
    )
}

/// Every request, before any route: this machine or a confirmed tailnet peer, and no repository
/// metadata, whether the path would reach a native file handler or the legacy app.
async fn judge(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let ip = peer.ip();
    let allowed = ip.is_loopback() || {
        let shared = shared.clone();
        tokio::task::spawn_blocking(move || shared.gate.allows(ip))
            .await
            .unwrap_or(false)
    };
    if !allowed {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: the Symphony answers only this machine and its tailnet\n",
        )
            .into_response();
    }
    let Some(path) = crate::api::decode_path(request.uri().path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if path
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (scope, relative) = crate::api::scope_path(&path);
    let root = if scope.is_empty() {
        Some(shared.root.clone())
    } else {
        shared.api.get().and_then(|hub| hub.tree(scope))
    };
    if let Some(root) = root {
        let relative = relative.trim_start_matches('/');
        // The legacy document views name the same file after their route prefix.
        let document = relative.strip_prefix("history/").or_else(|| {
            relative
                .strip_prefix("diff/")
                .and_then(|rest| rest.split_once('/').map(|(_, path)| path))
        });
        for relative in [Some(relative), document].into_iter().flatten() {
            if let Ok(target) = tokio::fs::canonicalize(root.join(relative)).await
                && shared.is_metadata(&target)
            {
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    }
    next.run(request).await
}

/// `GET /api/front`: this server, its app, and the leftovers it sees, for the starters on this machine,
/// which ask by loopback's name. A page another site rebinds to this machine names that site in its
/// Host, and reads nothing (the app's routes are the app's to judge by Host, as it does).
async fn front(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if !host.is_some_and(names_loopback) {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: the Host header does not name this machine's loopback\n",
        )
            .into_response();
    }
    Json(shared.status()).into_response()
}

/// Whether a Host header names loopback: 127.0.0.1, [::1] or localhost, with any port.
fn names_loopback(host: &str) -> bool {
    let suffix = if let Some(suffix) = host.strip_prefix("[::1]") {
        suffix
    } else {
        let (name, suffix) = host.find(':').map_or((host, ""), |at| host.split_at(at));
        if !["127.0.0.1", "localhost"]
            .iter()
            .any(|own| name.eq_ignore_ascii_case(own))
        {
            return false;
        }
        suffix
    };
    suffix.is_empty()
        || suffix.strip_prefix(':').is_some_and(|port| {
            !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok()
        })
}

/// Saves own their request body; reads need only its metadata. Unknown routes end here.
async fn answer(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    if crate::write::handles(&request) {
        return crate::write::answer(&shared, request).await;
    }
    let mut metadata = axum::http::Request::new(());
    *metadata.method_mut() = request.method().clone();
    *metadata.uri_mut() = request.uri().clone();
    *metadata.headers_mut() = request.headers().clone();
    if let Some(response) = crate::api::answer(&shared, metadata).await {
        return response;
    }
    StatusCode::NOT_FOUND.into_response()
}

/// A thread named `name`, running `body`.
pub(crate) fn worker(
    name: &str,
    body: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(body)
        .unwrap_or_else(|problem| panic!("cannot start the {name} thread: {problem}"))
}

/// The census of leftovers, a round every [`ORPHAN_SECONDS`] until the server stops, each one's count
/// from `scan` ([`orphans::scan`] outside tests; `None` when the process table cannot be read).
/// A pending scan never holds a starter or shutdown. The server's stop is the one way the task
/// ends: it drops a pending scan, and with it the scan's owned child.
fn census_worker<F: Future<Output = Option<usize>> + Send>(
    shared: Arc<Shared>,
    scan: impl Fn() -> F + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    // Publish the start event before returning the task, even if it has not been scheduled yet.
    shared.census_pending.store(true, Ordering::SeqCst);
    tokio::spawn(async move {
        let mut said = 0;
        while !shared.stopping() {
            shared.census_pending.store(true, Ordering::SeqCst);
            // Timed until the scan answers or the stop drops it, so a profile shows how long a
            // round that never answers was pending.
            let scanning = scan();
            let round = crate::live::measured_async("orphan census", async {
                tokio::select! {
                    biased;
                    () = shared.until_stopped() => None,
                    count = scanning => Some(count),
                }
            })
            .await;
            let Some(count) = round else { break };
            if let Some(count) = count {
                shared.orphans.store(count, Ordering::SeqCst);
                if count > said {
                    eprintln!(
                        "· {count} Symphony processes from test runs run with no parent (none is stopped)"
                    );
                }
                said = count;
            }
            shared.census_pending.store(false, Ordering::SeqCst);
            shared.counted.store(true, Ordering::SeqCst);
            tokio::select! {
                () = shared.until_stopped() => break,
                () = tokio::time::sleep(ORPHAN_SECONDS) => {},
            }
        }
        shared.census_pending.store(false, Ordering::SeqCst);
    })
}

/// A thread `name` that stops the server once `ended` says so, polled ten times a second, saying
/// `why`.
fn end_when(
    shared: &Arc<Shared>,
    name: &str,
    why: &'static str,
    ended: impl Fn() -> bool + Send + 'static,
) -> std::thread::JoinHandle<()> {
    let shared = shared.clone();
    worker(name, move || {
        while !shared.stopping() {
            if ended() {
                eprintln!("· Symphony server: {why}: stopping");
                shared.stop();
                return;
            }
            if shared.wait(Duration::from_millis(100)) {
                break;
            }
        }
    })
}

/// Includes early startup failures: no background command may outlive this server invocation.
struct Background {
    shared: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
    /// Ends on the stop, as the threads do; a started server waits for it as it returns.
    census: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Background {
    fn drop(&mut self) {
        self.shared.stop();
        for worker in self.workers.drain(..) {
            let name = worker
                .thread()
                .name()
                .unwrap_or("review background")
                .to_owned();
            if worker.join().is_err() {
                eprintln!("· {name} worker panicked while stopping");
            }
        }
    }
}

fn records(root: &Path, port: u16) -> bool {
    std::fs::read_to_string(root.join(PORT_FILE)).is_ok_and(|said| said.trim() == port.to_string())
}

fn record_port(root: &Path, port: u16) {
    let path = root.join(PORT_FILE);
    if let Some(folder) = path.parent()
        && std::fs::create_dir_all(folder).is_ok()
    {
        let _ = std::fs::write(&path, format!("{port}\n"));
    }
}

/// Remove the port record, unless another app has written its own over it since.
fn clear_port(root: &Path, port: u16) {
    if records(root, port) {
        let _ = std::fs::remove_file(root.join(PORT_FILE));
    }
}

/// A listener on `host`, reusing an address a restart left in TIME_WAIT; on IPv6 that side alone, so
/// "::" and "0.0.0.0" do not collide on one port.
fn listen(host: &str, port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let address: SocketAddr = format!(
        "{}:{port}",
        if host.contains(':') {
            format!("[{host}]")
        } else {
            host.into()
        }
    )
    .parse()
    .map_err(|_| std::io::Error::other(format!("{host} is not an address")))?;
    let socket = if address.is_ipv6() {
        let socket = tokio::net::TcpSocket::new_v6()?;
        let on: libc::c_int = 1;
        // SAFETY: a socket this function owns; the option is an int, as given.
        let set = unsafe {
            libc::setsockopt(
                std::os::fd::AsRawFd::as_raw_fd(&socket),
                libc::IPPROTO_IPV6,
                libc::IPV6_V6ONLY,
                (&on as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if set != 0 {
            return Err(std::io::Error::last_os_error());
        }
        socket
    } else {
        tokio::net::TcpSocket::new_v4()?
    };
    socket.set_reuseaddr(true)?;
    socket.bind(address)?;
    socket.listen(128)
}

/// The addresses a server on `hosts` binds. Every interface is loopback's two addresses first, then
/// the wildcards: outside Linux a server bound to loopback alone may sit beside a wildcard bind on the
/// same port and take every request to 127.0.0.1 from it, and holding loopback itself makes such a
/// server fail to start instead (as a start beside one fails at once). Linux refuses that bind already.
fn listened(hosts: &[String]) -> Vec<&str> {
    let mut listened: Vec<&str> = hosts.iter().map(String::as_str).collect();
    if !cfg!(target_os = "linux") && hosts == ["0.0.0.0", "::"] {
        listened.splice(0..0, ["127.0.0.1", "::1"]);
    }
    listened
}

/// Acquire every address as one set. Only automatically selected ports may be retried: a sibling
/// address can already hold the port the first address picked. Each failed set is dropped before
/// asking the OS for another port.
fn listen_all(
    hosts: &[String],
    requested: u16,
    bind: impl FnMut(&str, u16) -> std::io::Result<tokio::net::TcpListener>,
) -> std::io::Result<(u16, Vec<tokio::net::TcpListener>)> {
    bind_all(hosts, requested, bind, |listener| {
        listener.local_addr().map(|address| address.port())
    })
}

/// Own each acquired resource until the set succeeds. Keeping port lookup separate from binding
/// preserves its terminal errors and lets ownership be observed without reclaiming a shared port.
fn bind_all<L>(
    hosts: &[String],
    requested: u16,
    mut bind: impl FnMut(&str, u16) -> std::io::Result<L>,
    local_port: impl Fn(&L) -> std::io::Result<u16>,
) -> std::io::Result<(u16, Vec<L>)> {
    const ATTEMPTS: usize = 16;
    let everywhere = hosts == ["0.0.0.0", "::"];
    let hosts = listened(hosts);
    let attempts = if requested == 0 { ATTEMPTS } else { 1 };
    for attempt in 1..=attempts {
        let mut port = requested;
        let mut listeners = Vec::new();
        let mut failure = None;
        for host in &hosts {
            match bind(host, port) {
                Ok(listener) => {
                    port = local_port(&listener)?;
                    listeners.push(listener);
                }
                Err(problem)
                    if everywhere
                        && host.contains(':')
                        && !listeners.is_empty()
                        && problem.kind() != std::io::ErrorKind::AddrInUse => {} // no IPv6 here
                Err(problem) => {
                    let retry = requested == 0
                        && !listeners.is_empty()
                        && problem.kind() == std::io::ErrorKind::AddrInUse;
                    let why = format!("could not listen on {host} port {port}: {problem}");
                    if !retry || attempt == attempts {
                        return Err(std::io::Error::new(
                            problem.kind(),
                            if retry {
                                format!(
                                    "{why} (automatic port selection exhausted {attempts} attempts)"
                                )
                            } else {
                                why
                            },
                        ));
                    }
                    failure = Some(problem);
                    break;
                }
            }
        }
        if failure.is_none() {
            return Ok((port, listeners));
        }
    }
    unreachable!("the final failed attempt returns its error")
}

/// `serve`: the server a start runs, detached.
pub fn serve(serve: Serve) -> i32 {
    run(serve, false)
}

/// The server, until it is stopped (SIGTERM, SIGINT, SIGHUP) or, for a server of its own port, until
/// the process that started it ends. `open` opens its page once it listens. A server of its own
/// is refused the one server's port before it binds anything.
pub fn run(serve: Serve, open: bool) -> i32 {
    if serve.home.is_none()
        && let Some(why) = crate::cli::refused_port(serve.port)
    {
        eprintln!("✖ Symphony server: {why}");
        return 2;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(problem) => {
            eprintln!("✖ Symphony server could not start: {problem}");
            return 2;
        }
    };
    runtime.block_on(async {
        match start(serve, open).await {
            Ok(started) => started.serve_until_stopped(signalled()).await,
            Err(status) => status,
        }
    })
}

/// SIGTERM, SIGINT or SIGHUP; never, when they cannot be watched.
async fn signalled() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
        _ = hup.recv() => {}
    }
}

/// Stop the server once `signal` resolves. Stop waits for the hub's tick in flight, so it runs on
/// the blocking pool: the runtime's workers stay free to answer the drain's 503s.
fn stop_when(
    shared: &Arc<Shared>,
    signal: impl Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    let shared = shared.clone();
    tokio::spawn(async move {
        signal.await;
        let _ = tokio::task::spawn_blocking(move || shared.stop()).await;
    })
}

/// A listening server and the workers it owns until it returns.
struct Started {
    serve: Serve,
    shared: Arc<Shared>,
    background: Background,
    serving: Vec<tokio::task::JoinHandle<()>>,
    /// This server wrote the port record, and clears it as it ends.
    record: bool,
}

/// Listen, start the owned workers and print the URL; a failed start is its exit status.
async fn start(mut serve: Serve, open: bool) -> Result<Started, i32> {
    let mut reload = match crate::reload::setup(&serve) {
        Ok(setup) => setup,
        Err(why) => {
            eprintln!("✖ Symphony server could not inherit sockets or take its lease: {why}");
            return Err(2);
        }
    };
    let mut listeners = std::mem::take(&mut reload.listeners);
    let mut port = serve.port;
    if listeners.is_empty() {
        match listen_all(&serve.hosts, port, listen) {
            Ok((selected, bound)) => {
                port = selected;
                listeners = bound;
            }
            Err(problem) => {
                eprintln!(
                    "✖ Symphony server {problem}{}",
                    if problem.kind() == std::io::ErrorKind::AddrInUse {
                        " (another program holds it: a Symphony started before the one-server lock? \
                         stop it, then run `just symphony` again)"
                    } else {
                        ""
                    }
                );
                return Err(2);
            }
        }
    }
    serve.port = port;
    let shared = Shared::new(
        i64::from(std::process::id()),
        port,
        &serve.root,
        serve.home.as_deref(),
        Gate::tailscale(),
    );
    let mut background = Background {
        shared: shared.clone(),
        workers: Vec::new(),
        census: Some(census_worker(shared.clone(), orphans::scan)),
    };
    {
        let shared = shared.clone();
        background.workers.push(worker(TAILNET_WORKER, move || {
            *shared.tailnet.lock().unwrap_or_else(|e| e.into_inner()) =
                crate::gate::magicdns_name();
        }));
    }
    if serve.home.is_none() {
        // A server of its own port is a test's, or a terminal's: it ends with the process that started it.
        let parent = sys::parent();
        background.workers.push(end_when(
            &shared,
            STARTER_WORKER,
            "the process that started it ended",
            move || sys::parent() != parent,
        ));
    }
    if let Some(test) = std::env::var(TEST_PID)
        .ok()
        .and_then(|pid| pid.parse::<i64>().ok())
        .filter(|&pid| pid > 1)
    {
        background.workers.push(end_when(
            &shared,
            TEST_WORKER,
            "the test that started it ended",
            move || !sys::pid_alive(test),
        ));
    }
    // Make the document/board snapshots and gallery content ready before printing or opening
    // the URL: the first visitor must not pay the read hub's initialization cost.
    if let Err(why) = crate::api::warm(&shared).await {
        eprintln!("· native read data could not be warmed: {why}");
        if reload.is_replacement() {
            return Err(2);
        }
    }
    // A private server on the main checkout must not replace the shared app's discovery record.
    let record = serve.home.is_some()
        || crate::repo::checkout(&serve.root)
            .map_or(true, |checkout| checkout.top != checkout.main);
    if record {
        record_port(&serve.root, port);
    }
    // Keep copies before moving listeners into axum. The watcher starts only after the replacement
    // has committed its own handover, below.
    let reload_sockets = listeners
        .iter()
        .map(|listener| {
            use std::os::fd::AsFd;
            listener.as_fd().try_clone_to_owned()
        })
        .collect::<std::io::Result<Vec<_>>>();
    let reload_sockets = match reload_sockets {
        Ok(sockets) => sockets,
        Err(why) => {
            eprintln!("✖ Symphony server could not retain sockets: {why}");
            return Err(2);
        }
    };
    let app = router(shared.clone());
    let mut serving = Vec::new();
    for listener in listeners {
        let shared = shared.clone();
        let app = app.clone();
        serving.push(tokio::spawn(async move {
            connections::serve(listener, app, shared).await;
        }));
    }
    let ready = tokio::task::spawn_blocking(move || {
        reload.ready()?;
        Ok::<_, String>(reload)
    })
    .await;
    match ready {
        Ok(Ok(reload)) => {
            background.workers.extend(crate::reload::watch(
                serve.clone(),
                reload,
                shared.clone(),
                reload_sockets,
            ));
        }
        problem => {
            eprintln!("✖ review socket handover failed: {problem:?}");
            shared.stop();
        }
    }
    background.workers.extend(session_maintenance(
        &shared,
        serve.home.as_deref(),
        Stamp::now,
    ));
    background.workers.extend(embeddings::start(
        &shared,
        serve.home.as_deref(),
        Stamp::now,
    ));
    let url = format!("http://127.0.0.1:{port}/");
    println!("→ Symphony at {url}  (pid {})", shared.pid);
    if open {
        crate::cli::open(&url);
    }
    Ok(Started {
        serve,
        shared,
        background,
        serving,
        record,
    })
}

impl Started {
    /// Serve until the server stops, on `signal` or for any other reason, then drain and join
    /// every owned worker.
    async fn serve_until_stopped(self, signal: impl Future<Output = ()> + Send + 'static) -> i32 {
        let Started {
            serve,
            shared,
            mut background,
            serving,
            record,
        } = self;
        let signals = stop_when(&shared, signal);
        shared.until_stopped().await;
        signals.abort();
        // The stop ends the census, dropping a pending scan's child; this waits for that end.
        if let Some(census) = background.census.take()
            && let Err(problem) = census.await
        {
            eprintln!("· review orphan census failed while stopping: {problem}");
        }
        for task in serving {
            if shared.replaced.load(Ordering::SeqCst) {
                let _ = task.await;
            } else {
                let _ = tokio::time::timeout(SHUTDOWN_GRACE, task).await;
            }
        }
        let draining = shared.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Some(saves) = draining.saves.get() {
                saves.close();
            }
        })
        .await;
        drop(background);
        if record && !shared.replaced.load(Ordering::SeqCst) {
            clear_port(&serve.root, serve.port);
        }
        eprintln!("· Symphony server stopped");
        0
    }
}

/// A private preview may use a copied board, whose old pane identities must never be used to
/// clean up real agent terminals. Only the machine's shared installation starts this worker,
/// which reads the time from `clock` ([`Stamp::now`] outside tests).
fn session_maintenance(
    shared: &Arc<Shared>,
    home: Option<&Path>,
    clock: impl Fn() -> Stamp + Send + 'static,
) -> Option<std::thread::JoinHandle<()>> {
    home?;
    Some(maintenance_worker(
        Arc::clone(shared),
        Duration::from_secs_f64(tasks::launch::reap::REAP_SECONDS),
        SWEEP_TIMEOUT,
        clock,
    ))
}

/// A moment on both clocks. `Instant` stops while the machine sleeps, but the board's presence
/// caps run on wall time, so the schedule reads both: after a sleep the wall clock makes a round
/// due at once, before a live pane's Unknown cap expires.
#[derive(Clone, Copy)]
struct Stamp {
    mono: Instant,
    wall: SystemTime,
}

impl Stamp {
    fn now() -> Stamp {
        Stamp {
            mono: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// Wall time since `earlier`; zero if the wall clock was set back.
    fn wall_since(self, earlier: Stamp) -> Duration {
        self.wall
            .duration_since(earlier.wall)
            .unwrap_or(Duration::ZERO)
    }
}

/// A round is due when either clock says `interval` has passed since the last one started, so a
/// wake from sleep runs one at once and a wall clock set back cannot hold it off.
fn sweep_due(last: Option<Stamp>, now: Stamp, interval: Duration) -> bool {
    last.is_none_or(|at| {
        now.mono.saturating_duration_since(at.mono) >= interval || now.wall_since(at) >= interval
    })
}

/// How far past its interval, on the wall clock, a round starts; `None` within [`LATE_SWEEP`].
fn sweep_late(last: Stamp, now: Stamp, interval: Duration) -> Option<Duration> {
    let late = now.wall_since(last).checked_sub(interval)?;
    (late > LATE_SWEEP).then_some(late)
}

/// A round every `interval`, whose reap child is ended after `timeout`, both measured on `clock`.
fn maintenance_worker(
    shared: Arc<Shared>,
    interval: Duration,
    timeout: Duration,
    clock: impl Fn() -> Stamp + Send + 'static,
) -> std::thread::JoinHandle<()> {
    worker(SWEEP_WORKER, move || {
        let mut last: Option<Stamp> = None;
        while !shared.stopping() {
            let now = clock();
            if !sweep_due(last, now, interval) {
                if shared.wait(Duration::from_millis(250)) {
                    break;
                }
                continue;
            }
            let started = Instant::now();
            if let Some(late) = last.and_then(|at| sweep_late(at, now, interval)) {
                eprintln!(
                    "· idle-session sweep started {:.0}s late: the machine slept or the worker was not running",
                    late.as_secs_f64()
                );
            }
            last = Some(now);
            // The board's nonblocking reap lock admits one sweeper across all servers.
            // Log before waiting on output: a hung child must still leave its actual start time.
            eprintln!("· idle-session sweep started");
            match crate::live::measured("idle-session sweep", || {
                let mut command = tasks::sys::Command::new(shared.root.join("tools/rs-bin.sh"));
                command
                    .args(["tasks", "reap"])
                    .current_dir(&shared.root)
                    .env_remove("TASKS_SESSION")
                    .env_remove("HERDR_PANE_ID");
                sys::output_while(
                    command,
                    timeout,
                    || !shared.stopping(),
                    || {
                        let at = clock();
                        (at.mono, at.wall)
                    },
                )
            }) {
                Ok(done) if done.status.success() => {
                    if !done.stderr.is_empty() {
                        eprintln!("{}", String::from_utf8_lossy(&done.stderr).trim());
                    }
                }
                Ok(done) => eprintln!(
                    "· idle-session sweep failed: {}",
                    String::from_utf8_lossy(&done.stderr).trim()
                ),
                Err(_) if shared.stopping() => eprintln!("· idle-session sweep cancelled"),
                Err(e) => eprintln!("· idle-session sweep failed: {e}"),
            }
            eprintln!(
                "· idle-session sweep ended after {:.3}s",
                started.elapsed().as_secs_f64()
            );
            if shared.wait(Duration::from_millis(250)) {
                break;
            }
        }
    })
}

#[cfg(test)]
mod shutdown_tests;

#[cfg(test)]
mod listen_tests;

#[cfg(test)]
mod compression_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use tower::ServiceExt;

    #[tokio::test]
    async fn head_keeps_empty_responses_and_event_streams_bodyless() {
        for (status, content_type, stream, length) in [
            (StatusCode::CONTINUE, "text/plain", false, None),
            (StatusCode::NOT_MODIFIED, "text/plain", false, None),
            (StatusCode::NO_CONTENT, "text/plain", false, None),
            (StatusCode::OK, "text/plain", false, Some("0")),
            (StatusCode::OK, "text/event-stream", true, None),
        ] {
            let app = Router::new()
                .fallback(move || async move {
                    let body = if stream {
                        Body::from_stream(futures_util::stream::poll_fn(
                            |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
                                panic!("HEAD must not poll the event stream")
                            },
                        ))
                    } else {
                        Body::empty()
                    };
                    (status, [(header::CONTENT_TYPE, content_type)], body)
                })
                .layer(tower_http::compression::CompressionLayer::new())
                .layer(middleware::from_fn(head));
            let response = app
                .oneshot(
                    Request::builder()
                        .method(Method::HEAD)
                        .header(header::ACCEPT_ENCODING, "gzip")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_LENGTH)
                    .map(|v| v.to_str().unwrap()),
                length,
                "{status}: {content_type}"
            );
            assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
            assert!(
                axum::body::to_bytes(response.into_body(), 0)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[tokio::test]
    async fn head_reports_a_body_encoding_failure_without_success_headers() {
        let app = Router::new()
            .fallback(|| async {
                Body::from_stream(futures_util::stream::once(async {
                    Err::<Vec<u8>, _>(std::io::Error::other("download failed"))
                }))
            })
            .layer(tower_http::compression::CompressionLayer::new())
            .layer(middleware::from_fn(head));
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
        assert!(
            axum::body::to_bytes(response.into_body(), 0)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A confirmed tailnet peer, the one address the gate's `tailscale whois` knows.
    const PEER: &str = "100.101.102.103";

    #[test]
    fn loopback_hosts_accept_exact_names_and_numeric_ports() {
        for name in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]"] {
            for suffix in ["", ":0", ":1", ":80", ":65535", ":00080"] {
                let host = format!("{name}{suffix}");
                assert!(names_loopback(&host), "{host:?}");
            }
        }
    }

    #[test]
    fn loopback_hosts_refuse_foreign_names_and_incomplete_authorities() {
        for host in [
            "",
            "foreign.example",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "localhost.",
            "127.1",
            "127.0.0.2",
            "::1",
            "[::1",
            "[::2]",
            "[0:0:0:0:0:0:0:1]",
            "[localhost]",
            "[127.0.0.1]",
            "[[::1]]",
            "http://localhost",
            "user@localhost",
            " localhost",
            "\tlocalhost",
            "ｌocalhost",
        ] {
            assert!(!names_loopback(host), "{host:?}");
        }
        for name in ["localhost", "127.0.0.1", "[::1]"] {
            for suffix in [
                ".evil.example",
                "]",
                " ",
                "\0",
                "/path",
                "?query",
                "#fragment",
                "@evil.example",
                ":",
                ":+1",
                ":-1",
                ": 80",
                ":80 ",
                ":80:90",
                ":65536",
                ":999999999999999999999999",
                ":0x50",
                ":８０",
                ":80/path",
                ":80?query",
                ":80#fragment",
                ":80.evil.example",
                ":80@evil.example",
            ] {
                let host = format!("{name}{suffix}");
                assert!(!names_loopback(&host), "{host:?}");
            }
        }
    }

    #[test]
    fn idle_session_maintenance_starts_only_for_the_shared_installation() {
        let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
        let home = Path::new("/private-preview-fixture");
        let shared = Shared::new(1, 1, home, Some(home), gate);
        // Stop is sticky, so a started worker exits before any wait or external command.
        shared.stop();
        let private = session_maintenance(&shared, None, Stamp::now);
        let installed = session_maintenance(&shared, Some(home), Stamp::now);
        let private_started = private.is_some();
        let installed_started = installed.is_some();
        for worker in private.into_iter().chain(installed) {
            worker.join().unwrap();
        }
        assert!(
            !private_started,
            "a private preview must not start a reaper"
        );
        assert!(
            installed_started,
            "the shared installation must start its reaper"
        );
    }

    #[test]
    fn leaving_the_server_scope_joins_its_owned_workers() {
        let shared = Shared::new(
            1,
            1,
            Path::new("/background-scope-fixture"),
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        std::thread::scope(|scope| {
            let (entered, stopping) = std::sync::mpsc::channel();
            let (release, finish) = std::sync::mpsc::channel();
            let (finished, completed) = std::sync::mpsc::channel();
            let (returned, stopped) = std::sync::mpsc::channel();
            let working = shared.clone();
            let worker = std::thread::spawn(move || {
                working.wait(Duration::from_secs(60));
                entered.send(working.stopping()).unwrap();
                // Closing release also lets an early assertion unwind without stranding a worker.
                let _ = finish.recv();
                finished.send(()).unwrap();
            });
            let background = Background {
                shared,
                workers: vec![worker],
                census: None,
            };
            scope.spawn(move || {
                drop(background);
                returned.send(()).unwrap();
            });
            assert!(stopping.recv_timeout(Duration::from_secs(30)).unwrap());
            let early = stopped.recv_timeout(Duration::from_millis(100));
            release.send(()).unwrap();
            if early.is_err() {
                stopped.recv_timeout(Duration::from_secs(30)).unwrap();
            }
            completed.recv_timeout(Duration::from_secs(30)).unwrap();
            assert!(
                matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
                "the server scope returned before its worker finished: {early:?}"
            );
        });
    }

    #[tokio::test]
    async fn the_status_is_uncounted_until_the_first_census_ends_counted_or_not() {
        // None is a census that could not read the process table: it ends the wait all the same.
        for count in [Some(3), None] {
            let shared = Shared::new(
                1,
                1,
                Path::new("/census-fixture"),
                None,
                Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
            );
            // Each round answers what this test sends, and waits until it does.
            let (send, rounds) = tokio::sync::mpsc::unbounded_channel::<Option<usize>>();
            let rounds = Arc::new(tokio::sync::Mutex::new(rounds));
            let background = Background {
                shared: shared.clone(),
                workers: vec![],
                census: Some(census_worker(shared.clone(), move || {
                    let rounds = rounds.clone();
                    async move { rounds.lock().await.recv().await.flatten() }
                })),
            };
            let held = shared.status();
            assert!(held.uncounted, "counted before its census ended: {held:?}");
            assert!(held.census_pending, "the starter needs the pending event");
            assert_eq!(held.orphans, 0);
            send.send(count).unwrap();
            let began = Instant::now();
            let counted = loop {
                let status = shared.status();
                if !status.uncounted {
                    break status;
                }
                assert!(
                    began.elapsed() < Duration::from_secs(15),
                    "{count:?}: the first census never ended"
                );
                tokio::task::yield_now().await;
            };
            assert_eq!(counted.orphans, count.unwrap_or(0), "{count:?}");
            assert!(!counted.census_pending);
            drop(send);
            drop(background);
        }
    }

    #[tokio::test]
    async fn a_server_stopped_before_the_census_starts_launches_no_scan() {
        let shared = Shared::new(
            1,
            1,
            Path::new("/stopped-census-fixture"),
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        shared.stop();
        census_worker(shared.clone(), || async {
            panic!("a stopped server must not launch a census")
        })
        .await
        .unwrap();
        let status = shared.status();
        assert!(status.uncounted);
        assert!(!status.census_pending);
    }

    #[tokio::test]
    async fn stopping_a_pending_census_cancels_its_scan_without_a_result() {
        let shared = Shared::new(
            1,
            1,
            Path::new("/cancelled-census-fixture"),
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        let entered = Arc::new(tokio::sync::Notify::new());
        let started = entered.clone();
        let census = census_worker(shared.clone(), move || {
            let started = started.clone();
            async move {
                started.notify_one();
                std::future::pending().await
            }
        });
        entered.notified().await;
        assert!(shared.status().census_pending);
        shared.stop();
        census.await.unwrap();
        let status = shared.status();
        assert!(status.uncounted, "the scan returned no result");
        assert!(
            !status.census_pending,
            "the stop must cancel the pending scan"
        );
    }

    struct SweepFixture {
        root: PathBuf,
        shared: Arc<Shared>,
        background: Option<Background>,
    }

    impl SweepFixture {
        fn with_embedding_launcher(script: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let fixture = Self::new(script);
            let launcher = fixture.root.join("tools/tasks.sh");
            std::fs::write(&launcher, format!("#!/bin/sh\n{script}\n")).unwrap();
            assert!(std::fs::symlink_metadata(&launcher).unwrap().is_file());
            std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o644)).unwrap();
            fixture
        }

        /// A shared installation whose launcher runs `script`; its sweep starts with `start`.
        fn new(script: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "sf-sweep-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
            let shared = Shared::new(i64::from(std::process::id()), 1, &root, Some(&root), gate);
            let fixture = Self {
                root,
                shared,
                background: None,
            };
            std::fs::create_dir(fixture.root.join("tools")).unwrap();
            let program = fixture.root.join("tools/rs-bin.sh");
            crate::fixture_script::write(&program, &format!("#!/bin/sh\n{script}\n"), false);
            fixture
        }

        /// Own `worker` as the server does, until the fixture is dropped.
        fn start(&mut self, worker: std::thread::JoinHandle<()>) {
            self.background = Some(Background {
                shared: self.shared.clone(),
                workers: vec![worker],
                census: None,
            });
        }

        fn wait_for(&self, file: &str, expected: &str) {
            let began = Instant::now();
            while std::fs::read_to_string(self.root.join(file))
                .ok()
                .as_deref()
                != Some(expected)
            {
                assert!(
                    began.elapsed() < Duration::from_secs(15),
                    "sweep did not write {file}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for SweepFixture {
        fn drop(&mut self) {
            drop(self.background.take());
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A clock only the test moves. Every read is counted, so the test knows when the worker has
    /// looked at a time it set and acted on it.
    #[derive(Clone)]
    struct TestClock {
        start: Stamp,
        /// How far past `start` the clock reads, and how often it was read.
        state: Arc<(Mutex<(Duration, u64)>, Condvar)>,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                start: Stamp::now(),
                state: Arc::default(),
            }
        }

        fn reader(&self) -> impl Fn() -> Stamp + Send + 'static {
            let clock = self.clone();
            move || {
                let (state, read) = &*clock.state;
                let mut state = state.lock().unwrap();
                state.1 += 1;
                read.notify_all();
                Stamp {
                    mono: clock.start.mono + state.0,
                    wall: clock.start.wall + state.0,
                }
            }
        }

        /// Set the clock to `elapsed` past its start, when given, then wait for `more` reads.
        fn read_at(&self, elapsed: Option<Duration>, more: u64) {
            let (state, read) = &*self.state;
            let mut state = state.lock().unwrap();
            if let Some(elapsed) = elapsed {
                state.0 = elapsed;
            }
            let seen = state.1;
            let (_state, waited) = read
                .wait_timeout_while(state, Duration::from_secs(30), |state| {
                    state.1 < seen + more
                })
                .unwrap();
            assert!(!waited.timed_out(), "the sweep stopped reading its clock");
        }

        /// Move the clock to `elapsed` past its start. The first read after decides on that time;
        /// the second comes only once the worker has acted on the decision.
        fn set(&self, elapsed: Duration) {
            self.read_at(Some(elapsed), 2);
        }
    }

    #[test]
    fn the_shared_installation_ends_a_hung_reap_child_at_its_sweep_timeout() {
        // The deadline is the shared installation's own SWEEP_TIMEOUT, read on a clock this test
        // moves: the child below never ends by itself.
        let clock = TestClock::new();
        let mut fixture = SweepFixture::new("printf '%s\\n' \"$$\" > entered; exec sleep 86400");
        let announced = crate::testing::fifo(&fixture.root.join("entered"));
        fixture.start(
            session_maintenance(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap(),
        );
        let first = crate::testing::entered(&announced);
        // The child may announce itself before its sweep reads the start of the deadline; the
        // next read is that start or comes after it.
        clock.read_at(None, 1);
        clock.set(SWEEP_TIMEOUT - Duration::from_secs(1));
        assert!(
            sys::pid_alive(first),
            "the reap child was ended before its timeout"
        );
        clock.set(SWEEP_TIMEOUT);
        assert!(
            !sys::pid_alive(first),
            "the reap child outlived its {SWEEP_TIMEOUT:?} timeout"
        );
        // The next round starts at the reap interval, with a child of its own.
        clock.set(Duration::from_secs_f64(tasks::launch::reap::REAP_SECONDS));
        let second = crate::testing::entered(&announced);
        assert_ne!(first, second);
        drop(fixture);
        assert!(!sys::pid_alive(second), "shutdown ends the round in flight");
    }

    #[test]
    fn the_shared_installation_sweeps_at_startup_before_its_normal_interval() {
        let mut fixture = SweepFixture::new("printf '%s' \"$*\" > started; exec sleep 30");
        use std::os::unix::fs::MetadataExt;
        let other = SweepFixture::new("exit 0");
        let first = std::fs::metadata(fixture.root.join("tools/rs-bin.sh")).unwrap();
        let second = std::fs::metadata(other.root.join("tools/rs-bin.sh")).unwrap();
        assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
        fixture
            .start(session_maintenance(&fixture.shared, Some(&fixture.root), Stamp::now).unwrap());
        fixture.wait_for("started", "tasks reap");
        let stopping = Instant::now();
        drop(fixture);
        assert!(
            stopping.elapsed() < Duration::from_secs(5),
            "shutdown must cancel an active sweep instead of waiting for its four-minute deadline"
        );
    }

    #[test]
    fn a_private_preview_never_starts_embedding_maintenance() {
        let fixture = SweepFixture::with_embedding_launcher("echo wrong > started");
        assert!(embeddings::start(&fixture.shared, None, Stamp::now).is_none());
        assert!(!fixture.root.join("started").exists());
    }

    /// Subprocess entry: measure the launched process rather than reconstructing its command.
    #[test]
    fn embedding_child_resource_probe() {
        let Ok(args) = std::env::var("SF_EMBEDDING_RESOURCE_PROBE") else {
            return;
        };
        // SAFETY: getpriority reads this process's scheduling priority without mutating it.
        let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        std::fs::write("resources", format!("{priority}\n{args}\n")).unwrap();
        std::fs::write("resources-ready", "yes").unwrap();
    }

    #[test]
    fn embedding_maintenance_runs_a_non_executable_launcher_niced_with_one_inference_thread() {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: getpriority reads this process's inherited scheduling priority.
        let inherited = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        let exe = std::env::current_exe().unwrap();
        let exe = exe.to_str().unwrap().replace('\'', "'\\''");
        let script = format!(
            "SF_EMBEDDING_RESOURCE_PROBE=\"$*\" exec '{exe}' --exact front::tests::embedding_child_resource_probe --nocapture"
        );
        let mut fixture = SweepFixture::with_embedding_launcher(&script);
        assert_eq!(
            std::fs::metadata(fixture.root.join("tools/tasks.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
        fixture.start(embeddings::start(&fixture.shared, Some(&fixture.root), Stamp::now).unwrap());
        fixture.wait_for("resources-ready", "yes");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("resources")).unwrap(),
            format!(
                "{}\nembed --maintenance --threads 1\n",
                (inherited + 10).min(19)
            ),
            "the actual child must inherit nice +10 and select exactly one inference thread"
        );
    }

    #[test]
    fn embedding_maintenance_starts_at_startup_and_retries_a_failed_pass_each_minute() {
        let clock = TestClock::new();
        let mut fixture = SweepFixture::with_embedding_launcher(
            "printf '%s\\n' \"$*\" >> calls; if [ ! -f attempted ]; then touch attempted; exit 1; fi",
        );
        fixture.start(
            embeddings::start(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap(),
        );
        fixture.wait_for("calls", "embed --maintenance --threads 1\n");
        clock.set(embeddings::INTERVAL - Duration::from_secs(1));
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("calls")).unwrap(),
            "embed --maintenance --threads 1\n"
        );
        clock.set(embeddings::INTERVAL);
        fixture.wait_for(
            "calls",
            "embed --maintenance --threads 1\nembed --maintenance --threads 1\n",
        );
    }

    #[test]
    fn embedding_maintenance_bounds_a_hung_pass_and_cancels_its_owned_child_on_shutdown() {
        let clock = TestClock::new();
        let mut fixture = SweepFixture::with_embedding_launcher(
            "printf '%s\\n' \"$$\" > entered; exec sleep 86400",
        );
        let announced = crate::testing::fifo(&fixture.root.join("entered"));
        fixture.start(
            embeddings::start(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap(),
        );
        let first = crate::testing::entered(&announced);
        clock.read_at(None, 1);
        clock.set(embeddings::TIMEOUT - Duration::from_secs(1));
        assert!(
            sys::pid_alive(first),
            "a first backfill gets its own runtime budget"
        );
        clock.set(embeddings::TIMEOUT);
        assert!(!sys::pid_alive(first), "a hung pass is bounded");
        let second = crate::testing::entered(&announced);
        assert_ne!(first, second);
        drop(fixture);
        assert!(!sys::pid_alive(second), "shutdown ends the pass in flight");
    }

    #[test]
    fn a_sweep_is_due_when_either_clock_passes_the_interval() {
        let interval = Duration::from_secs(300);
        let (mono, wall) = (
            Instant::now(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
        );
        let last = Stamp { mono, wall };
        let at = |awake: u64, elapsed: u64| Stamp {
            mono: mono + Duration::from_secs(awake),
            wall: wall + Duration::from_secs(elapsed),
        };
        assert!(sweep_due(None, last, interval), "the first round is due");
        assert!(!sweep_due(Some(last), at(299, 299), interval));
        assert!(sweep_due(Some(last), at(300, 300), interval));
        // A 32-minute sleep: the monotonic clock advanced one second, the wall clock 1920.
        assert!(
            sweep_due(Some(last), at(1, 1920), interval),
            "a wake runs a round at once"
        );
        // The wall clock set back must not hold a round off.
        let back = Stamp {
            mono: mono + Duration::from_secs(300),
            wall: wall - Duration::from_secs(3600),
        };
        assert!(sweep_due(Some(last), back, interval));
        assert!(!sweep_due(
            Some(last),
            Stamp {
                wall: wall - Duration::from_secs(5),
                ..at(10, 0)
            },
            interval
        ));
    }

    #[test]
    fn a_round_is_reported_late_by_its_whole_delay_past_the_threshold() {
        let interval = Duration::from_secs(300);
        let (mono, wall) = (
            Instant::now(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
        );
        let last = Stamp { mono, wall };
        let after = |secs: u64| Stamp {
            mono,
            wall: wall + Duration::from_secs(secs),
        };
        assert_eq!(sweep_late(last, after(300), interval), None);
        assert_eq!(
            sweep_late(last, after(330), interval),
            None,
            "30 s is the threshold itself"
        );
        assert_eq!(
            sweep_late(last, after(1920), interval),
            Some(Duration::from_secs(1620)),
            "a 32-minute sleep is 1620 s past the interval"
        );
        assert_eq!(sweep_late(last, after(10), interval), None);
    }

    #[test]
    fn a_held_lifecycle_lock_does_not_stall_the_sweep_rounds() {
        // Hub startup holds the lifecycle lock across SurrealDB calls; the rounds must go on.
        let mut fixture = SweepFixture::new(
            "if [ ! -f started ]; then printf first > started; exit 0; fi\nprintf again > retried",
        );
        fixture.start(maintenance_worker(
            fixture.shared.clone(),
            Duration::ZERO,
            Duration::from_secs(3),
            Stamp::now,
        ));
        let _hub_starting = fixture.shared.lifecycle.lock().unwrap();
        fixture.wait_for("started", "first");
        fixture.wait_for("retried", "again");
    }

    /// The status this server's routes give a GET of `path` from `client` with `host`, behind a gate
    /// whose `tailscale whois` knows [`PEER`] alone.
    async fn status_from(client: &str, path: &str, host: Option<&str>) -> StatusCode {
        let gate = Gate::new(
            Arc::new(|ip: &str| {
                Ok::<_, String>((ip == PEER).then(|| r#"{"Node": {"Name": "phone"}}"#.to_string()))
            }),
            Arc::new(Instant::now),
        );
        let shared = Shared::new(1, 1, Path::new("/nowhere"), None, gate);
        let app = router(shared).layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
        let mut request = Request::builder().uri(path);
        if let Some(host) = host {
            request = request.header(header::HOST, host);
        }
        let request = request.body(Body::empty()).unwrap();
        app.oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn a_lan_address_or_a_tailnet_stranger_is_refused_before_any_route() {
        let strangers = [
            "192.168.1.20:5000",
            "10.0.0.2:5000",
            "100.64.0.9:5000",
            "[fd7a:115c:a1e0::9]:5000",
            "[::ffff:192.168.1.20]:5000",
        ];
        for client in strangers {
            for path in ["/api/front", "/docs/A.md"] {
                let status = status_from(client, path, Some("127.0.0.1:1")).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{client} {path}");
            }
        }
    }

    #[tokio::test]
    async fn this_machine_and_a_confirmed_tailnet_peer_are_let_in() {
        for client in [
            "127.0.0.1:5000",
            "[::1]:5000",
            "[::ffff:127.0.0.1]:5000",
            "100.101.102.103:5000",
        ] {
            let status = status_from(client, "/api/front", Some("127.0.0.1:1")).await;
            assert_eq!(status, StatusCode::OK, "{client}");
        }
    }

    /// `/api/front`, the server's own route, is for the starters on this machine, which ask it by
    /// loopback's name: a page another site rebinds to loopback names that site instead, and reads
    /// nothing. The app's routes are judged by the app's own Host check, after this server.
    #[tokio::test]
    async fn the_servers_own_route_answers_only_a_host_naming_loopback() {
        for host in ["127.0.0.1:1", "localhost:1", "[::1]:1", "LOCALHOST"] {
            let status = status_from("127.0.0.1:5000", "/api/front", Some(host)).await;
            assert_eq!(status, StatusCode::OK, "{host}");
        }
        for host in [
            Some("evil.example:1"),
            Some("127.0.0.1.evil.example"),
            Some(""),
            None,
        ] {
            let status = status_from("127.0.0.1:5000", "/api/front", host).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{host:?}");
        }
    }
}
