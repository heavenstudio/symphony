//! Axum serves the Symphony client, JSON API and project media directly. Every request is judged by
//! client address first; read and save handlers also check Host and Origin.

use crate::backstop::{Waker, Wakers, Wakes};
use crate::lock::Held as _;
use crate::{cli::Serve, gate::Gate, sys};
use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use futures_util::StreamExt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use symphony_board::{launch::reap::REAP_SECONDS, sys::Command};
use tokio::sync::watch;
use tower_http::compression::predicate::{DefaultPredicate, Predicate};

mod connections;
mod port_file;

pub(crate) use port_file::port_file;
use port_file::{clear_port, record_port};

/// Allow a cold launcher build, but end a stuck command before the next five-minute sweep.
pub(crate) const SWEEP_TIMEOUT: Duration = Duration::from_secs(240);
/// How far past its interval a round may start before the log says the worker was stalled.
pub(crate) const LATE_SWEEP: Duration = Duration::from_secs(30);
/// Existing connection-shutdown grace, also used to bound a rejected socket's read-side drain.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long a replaced generation lets the requests it accepted finish before it ends anyway, as
/// long as a replacement is given to start: a save's body, a download. A peer that stops reading
/// a response (a browser's suspended page, a connection left on a sleeping network) holds it no
/// longer, and the generation, the replacement's parent, ends.
const REPLACED_DRAIN: Duration = Duration::from_secs(120);
/// Names the test process that started this server, which then ends with it, however it ends: a test
/// killed by a signal runs no cleanup, and a started server is in a session of its own, out of the
/// runner's process group (`tests/app.rs` sets it on everything it runs).
pub const TEST_PID: symphony_env::Var = symphony_env::TEST_PID;

/// The thread names of the workers a server owns and joins as it ends; the stop's stage lines
/// and a sample of a stuck shutdown name the one it waits on.
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
    /// Whether the client is the one installed beside the machine's one server: true for that
    /// server alone ([`Shared::one_server`]).
    pub(crate) installed_ui: bool,
    pub(crate) embedded_ui: bool,
    /// Where the one server serves this project's pages, `/<id>`; empty for a server of its own,
    /// which serves its one project at `/`.
    pub(crate) base: String,
    common: Option<PathBuf>,
    pub(crate) gate: Gate,
    tailnet: Mutex<Option<String>>,
    /// Set once, when the server stops: whoever waits on it sees it, however late it looks.
    stopped: watch::Sender<bool>,
    pub(crate) lifecycle: Mutex<()>,
    /// What the stop wakes ([`Shared::wake_at_stop`]). Never taken with `lifecycle`, which hub
    /// startup holds across SurrealDB calls.
    stop_wakers: Wakers,
    /// The write ends of the pipes whose read ends see their end at the stop
    /// ([`Shared::stop_pipe`]): a wait on a descriptor ends there.
    stop_pipes: Mutex<Vec<std::io::PipeWriter>>,
    pub(crate) replaced: AtomicBool,
    /// A starter asked for the hand-off ([`Shared::ask_handover`]); the reload worker takes it.
    pub(crate) handover: AtomicBool,
    /// What a hand-off request wakes ([`Shared::wake_at_handover`]).
    handover_wakers: Wakers,
    /// What the project's hub wakes as its board is found or lost ([`crate::live::Hub::has_board`]),
    /// given to the hub as it starts ([`crate::api::start`]).
    pub(crate) board_wakers: Arc<Wakers>,
    pub(crate) api: tokio::sync::OnceCell<Arc<crate::live::Hub>>,
    /// When this server generation's hubs record, the one gate of every project it serves and of
    /// both their recorders, the agents' and the board's health hours: a replacement's opens once
    /// its hand-off commits, the generation it replaces closes just before it commits
    /// ([`symphony_board::recording`]).
    pub(crate) recording: Arc<symphony_board::recording::Gate>,
    pub(crate) saves: std::sync::OnceLock<symphony::save::Saves>,
    /// The workers that maintain this project's board, its idle sweep,
    /// when the one server serves it beside its starter's ([`crate::projects`]): joined as it stops,
    /// so none, nor any child of theirs, outlives its state. The starter's own are the server's
    /// ([`Background`]).
    maintenance: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

/// What [`Shared::made`] makes a project's state with beside its process, port, root and base:
/// the client it serves, the gate its peers pass and the gate its recorders keep.
struct Serving {
    ui_dist: PathBuf,
    installed_ui: bool,
    embedded_ui: bool,
    gate: Gate,
    recording: Arc<symphony_board::recording::Gate>,
}

impl Shared {
    /// The project's hub, or the answer a request gets before the server has started one, boxed
    /// as a response is large to carry in a `Result`.
    pub(crate) fn hub(&self) -> Result<Arc<crate::live::Hub>, Box<Response>> {
        self.api.get().cloned().ok_or_else(|| {
            Box::new(crate::api::error(
                StatusCode::SERVICE_UNAVAILABLE,
                "the Symphony server is starting",
            ))
        })
    }

    /// Whether this is the machine's one server, started with its install folder (`--home`): the
    /// server that alone does what its machine's projects share, as a private one must not (the
    /// chat journal and the board's records, [`crate::live::Hub::start`]; a cancel's cleanup).
    pub(crate) fn one_server(&self) -> bool {
        self.installed_ui
    }

    /// A server's one project, at `/`. `install` is the one server's install folder, whose client
    /// it serves; none for a server of its own.
    pub fn new(
        pid: i64,
        port: u16,
        root: &Path,
        install: Option<&Path>,
        gate: Gate,
    ) -> Arc<Shared> {
        Shared::project(pid, port, root, install, gate, "")
    }

    /// [`Shared::new`] for a project the one server serves at `base`, `/<id>`.
    pub fn project(
        pid: i64,
        port: u16,
        root: &Path,
        install: Option<&Path>,
        gate: Gate,
        base: &str,
    ) -> Arc<Shared> {
        let (ui_dist, embedded_ui) = crate::client::selection(root, install);
        let serving = Serving {
            ui_dist,
            installed_ui: install.is_some(),
            embedded_ui,
            gate,
            recording: Arc::default(),
        };
        Shared::made(pid, port, root, serving, base)
    }

    /// The one server's state for another registered project at `root`, served at `base`: this
    /// server's process, port and client, that project's own hub, saves and gate.
    pub(crate) fn beside(&self, root: &Path, base: &str) -> Arc<Shared> {
        let serving = Serving {
            ui_dist: self.ui_dist.clone(),
            installed_ui: self.installed_ui,
            embedded_ui: self.embedded_ui,
            gate: Gate::tailscale(),
            recording: self.recording.clone(),
        };
        Shared::made(self.pid, self.port, root, serving, base)
    }

    fn made(pid: i64, port: u16, root: &Path, serving: Serving, base: &str) -> Arc<Shared> {
        let Serving {
            ui_dist,
            installed_ui,
            embedded_ui,
            gate,
            recording,
        } = serving;
        Arc::new(Shared {
            pid,
            port,
            root: root.into(),
            ui_dist,
            installed_ui,
            embedded_ui,
            base: base.into(),
            common: crate::repo::checkout(root)
                .ok()
                .map(|checkout| checkout.common),
            gate,
            tailnet: Mutex::new(None),
            stopped: watch::Sender::new(false),
            lifecycle: Mutex::new(()),
            stop_wakers: Wakers::default(),
            stop_pipes: Mutex::new(Vec::new()),
            replaced: AtomicBool::new(false),
            handover: AtomicBool::new(false),
            handover_wakers: Wakers::default(),
            board_wakers: Arc::default(),
            api: tokio::sync::OnceCell::new(),
            saves: std::sync::OnceLock::new(),
            recording,
            maintenance: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn stopping(&self) -> bool {
        *self.stopped.borrow()
    }

    pub(crate) fn allows_host(&self, host: &str) -> bool {
        self.gate.allows_host(host)
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
    /// The project's maintenance ends first ([`Shared::maintained`]).
    pub fn stop(&self) {
        self.stopped.send_replace(true);
        self.stop_wakers.wake();
        self.stop_pipes.lock().held().clear();
        self.end_maintenance(&mut self.maintenance.lock().held());
        let _held = self.lifecycle.lock().held();
        if let Some(hub) = self.api.get() {
            hub.stop();
        }
    }

    /// Keep `workers`, the maintenance of this project's board, until it stops, when each is
    /// joined: each sees the stop at its next look and ends the child it runs. Workers kept once it
    /// stopped are joined at once.
    pub(crate) fn maintained(&self, workers: Vec<std::thread::JoinHandle<()>>) {
        let mut held = self.maintenance.lock().held();
        held.extend(workers);
        if self.stopping() {
            self.end_maintenance(&mut held);
        }
    }

    /// How many maintenance workers it holds: none once a stop joined them, as a stop joins them
    /// with the list held.
    #[cfg(test)]
    pub(crate) fn maintaining(&self) -> usize {
        self.maintenance.lock().held().len()
    }

    /// Join every worker of `held`, the maintenance list, held so a second stop meanwhile waits
    /// for these joins rather than return before they end.
    fn end_maintenance(&self, held: &mut Vec<std::thread::JoinHandle<()>>) {
        for worker in held.drain(..) {
            let name = worker.thread().name().unwrap_or("maintenance").to_owned();
            if worker.join().is_err() {
                tell(format_args!(
                    "· {}{} worker panicked while stopping",
                    name,
                    of_project(self)
                ));
            }
        }
    }

    /// Wake `waker` as the server stops; one given once it stopped is woken at once. The stop is
    /// set before its wakes are sent, so a loop that gives its waker and then looks whether the
    /// server stops misses no stop.
    pub(crate) fn wake_at_stop(&self, waker: Waker) {
        self.stop_wakers.add(waker.clone());
        if self.stopping() {
            waker.wake();
        }
    }

    /// Wake `waker` at each hand-off a starter asks for ([`Shared::ask_handover`]).
    pub(crate) fn wake_at_handover(&self, waker: Waker) {
        self.handover_wakers.add(waker.clone());
        if self.handover.load(Ordering::SeqCst) {
            waker.wake();
        }
    }

    /// A starter asked for the hand-off: the reload worker takes it at its wake.
    pub(crate) fn ask_handover(&self) {
        self.handover.store(true, Ordering::SeqCst);
        self.handover_wakers.wake();
    }

    /// Wake `waker` as the project's hub finds or loses its board ([`has_board`]).
    pub(crate) fn wake_at_board(&self, waker: Waker) {
        self.board_wakers.add(waker);
    }

    /// The read end of a pipe whose write end the server keeps until it stops: a read, or a poll
    /// of it, sees its end then, and at once for one taken once it stopped. What waits on a
    /// descriptor (a process's end, [`sys::until_ends`]) waits on this beside it.
    pub(crate) fn stop_pipe(&self) -> std::io::Result<std::io::PipeReader> {
        let (read, write) = symphony_board::sys::pipe()?;
        let mut held = self.stop_pipes.lock().held();
        if !self.stopping() {
            held.push(write);
        }
        Ok(read)
    }

    /// Until the server stops, on a thread: the stop's wake, and no length.
    #[cfg(test)]
    pub(crate) fn wait_for_stop(&self) {
        let wakes = Wakes::new();
        self.wake_at_stop(wakes.waker());
        while !self.stopping() {
            wakes.wait(None);
        }
    }

    /// Until the server stops.
    async fn until_stopped(&self) {
        let mut stopped = self.stopped.subscribe();
        let _ = stopped.wait_for(|stopped| *stopped).await;
    }

    /// The server as `/api/front` says it, the projects it serves left to the one server's
    /// dispatch ([`crate::projects`]).
    pub(crate) fn status(&self) -> symphony_api::Front {
        let build = crate::host::Build::this(&self.root);
        symphony_api::Front {
            plugins: build.plugins,
            pin: build.pin,
            schema: build.schema,
            projects: Vec::new(),
            release: build.release,
            pid: self.pid,
            port: self.port,
            root: self.root.display().to_string(),
            ready: self.api.get().is_some(),
            app: None,
            tailnet: self.tailnet.lock().held().clone(),
            orphans: 0,
        }
    }
}

/// The server's routes.
pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route(symphony_api::FRONT_PATH, get(front))
        .fallback(answer)
        .layer(
            tower_http::compression::CompressionLayer::new()
                .compress_when(DefaultPredicate::new().and(crate::api::compressed_per_request)),
        )
        .layer(middleware::from_fn(head))
        .layer(middleware::from_fn_with_state(shared.clone(), judge))
        .layer(middleware::map_response(nosniff))
        .with_state(shared)
}

/// `response`, saying `X-Content-Type-Options: nosniff` when it is JSON or the feed
/// ([`sniffless`]), whichever route made it, its refusals and misses among them. A browser then
/// runs or applies none of them as a script or a stylesheet: a page that loads an answer of the
/// app's (an HTML document an entrypoint at the project's root serves can name any of its
/// addresses) gets the same refusal whether the route answered or not, and learns nothing from
/// the load. A project's own files and uploads say it themselves ([`crate::media_files`]); its
/// router and the one server's dispatch ([`crate::projects::router`]) each end with this.
pub(crate) async fn nosniff(mut response: Response) -> Response {
    if sniffless(response.headers()) {
        response.headers_mut().insert(
            header::X_CONTENT_TYPE_OPTIONS,
            header::HeaderValue::from_static("nosniff"),
        );
    }
    response
}

/// Whether `headers` name a type [`nosniff`] marks: the API's `application/json` or the event
/// feed's `text/event-stream`, its parameters and letters' case aside.
fn sniffless(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|kind| kind.split(';').next())
        .is_some_and(|essence| {
            let essence = essence.trim();
            essence.eq_ignore_ascii_case("application/json")
                || essence.eq_ignore_ascii_case("text/event-stream")
        })
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
    mut request: Request,
    next: Next,
) -> Response {
    // The peer as judged, for the routes that ask where a request came from: the extractor reads
    // a connection's address or a test's mock alike, a plain extension read only the first.
    request.extensions_mut().insert(ConnectInfo(peer));
    if crate::hosted::admission(&request).is_none() && !peer_allowed(&shared, peer.ip()).await {
        return peer_refused();
    }
    let Some(path) = crate::api::decode_path(request.uri().path()) else {
        return missing(&shared, &request, StatusCode::BAD_REQUEST);
    };
    if path
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return missing(&shared, &request, StatusCode::NOT_FOUND);
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
        let project = symphony::project::Project::of(&root);
        for relative in [Some(relative), document].into_iter().flatten() {
            if let Ok(target) = tokio::fs::canonicalize(root.join(project.file(relative))).await
                && shared.is_metadata(&target)
            {
                return missing(&shared, &request, StatusCode::NOT_FOUND);
            }
        }
    }
    next.run(request).await
}

/// Whether a request from `ip` may be answered: this machine's, or a peer on its tailnet that
/// `shared`'s gate confirms ([`Gate::allows`], asked on the blocking pool, as it may wait on
/// `tailscale`). A gate that panics refuses.
pub(crate) async fn peer_allowed(shared: &Arc<Shared>, ip: IpAddr) -> bool {
    ip.is_loopback() || {
        let shared = shared.clone();
        crate::spawn::blocking(move || shared.gate.allows(ip))
            .await
            .unwrap_or(false)
    }
}

/// What a peer [`peer_allowed`] refuses is answered.
pub(crate) fn peer_refused() -> Response {
    (
        StatusCode::FORBIDDEN,
        "Forbidden: the Symphony answers only this machine and its tailnet\n",
    )
        .into_response()
}

/// What a miss at `request`'s path answers in this project ([`crate::pages::missing`]).
fn missing(shared: &Shared, request: &Request, status: StatusCode) -> Response {
    crate::pages::missing(
        &shared.root,
        &shared.base,
        request.uri().path(),
        request.headers(),
        status,
    )
}

/// `GET /api/front`: this server and its app, for the starters on this machine, which ask by
/// loopback's name. A page another site rebinds to this machine names that site in its
/// Host, and reads nothing (the app's routes are the app's to judge by Host, as it does).
async fn front(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Some(refused) = loopback_refused(&headers) {
        return refused;
    }
    Json(shared.status()).into_response()
}

/// `/api/front`'s refusal of a request whose Host does not name loopback ([`names_loopback`]),
/// none given or one that is no text among them; none for a request whose Host does.
pub(crate) fn loopback_refused(headers: &HeaderMap) -> Option<Response> {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    (!host.is_some_and(names_loopback)).then(|| {
        (
            StatusCode::FORBIDDEN,
            "Forbidden: the Host header does not name this machine's loopback\n",
        )
            .into_response()
    })
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

/// Saves own their request body; reads need only its metadata. Unknown routes end here, and so
/// does every miss a read answers with no body (a file nothing serves, say): each is the one page
/// or API error a miss is ([`crate::pages::missing`]). A project whose folder no longer holds it
/// ([`crate::live::Hub::holds_its_project`]) is answered nothing more: each read or save would reach
/// the board of whatever checkout holds its folder, and start it.
async fn answer(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    if let Some(hub) = shared.api.get()
        && !hub.holds_its_project()
    {
        return lost(&request, &hub.gone());
    }
    if crate::write::handles(&request) {
        return crate::write::answer(&shared, request).await;
    }
    let mut metadata = axum::http::Request::new(());
    *metadata.method_mut() = request.method().clone();
    *metadata.uri_mut() = request.uri().clone();
    *metadata.headers_mut() = request.headers().clone();
    // Where the request came from (a Go to session focuses a pane only for this machine's own) and
    // the connection's upgrade, which a feed asked for on a WebSocket takes over.
    *metadata.extensions_mut() = request.extensions().clone();
    let response = match crate::api::answer(&shared, metadata).await {
        Some(response) => response,
        None => match crate::write::methods(request.uri().path()) {
            // A save route asked to read: say which methods it takes.
            Some(allowed) => {
                let mut refused = crate::pages::json(
                    StatusCode::METHOD_NOT_ALLOWED,
                    &format!("this route takes {allowed}"),
                );
                refused
                    .headers_mut()
                    .insert(header::ALLOW, header::HeaderValue::from_static(allowed));
                refused
            }
            None => StatusCode::NOT_FOUND.into_response(),
        },
    };
    if matches!(
        response.status(),
        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
    ) && !response.headers().contains_key(header::CONTENT_TYPE)
    {
        return missing(&shared, &request, response.status());
    }
    response
}

/// What a project whose folder no longer holds it answers every request with: `why`, as a page to a
/// browser loading one and as JSON to a read or a save.
fn lost(request: &Request, why: &str) -> Response {
    if !crate::projects::navigation(request.method(), request.headers()) {
        return crate::pages::json(StatusCode::SERVICE_UNAVAILABLE, why);
    }
    let said = format!(
        "<p><strong>The Symphony does not serve this project now</strong>: {}</p>",
        symphony::markup::escape(why)
    );
    crate::pages::html(
        StatusCode::SERVICE_UNAVAILABLE,
        crate::pages::machine(
            "Not served · Symphony",
            "not served",
            &crate::pages::notice("alert", &said),
            "",
        ),
    )
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

/// A thread `name` that stops the server once the process `pid` ends, saying `why`: a wait for
/// that end itself ([`sys::until_ends`]), beside the stop's ([`Shared::stop_pipe`]). `ended` says
/// whether it has ended, read once the wait is set, so an end before it, the pid given to another
/// process since, counts too. A process that cannot be followed is said, and the server then ends
/// at its stop alone.
fn end_when(
    shared: &Arc<Shared>,
    name: &str,
    why: &'static str,
    pid: i64,
    ended: impl Fn() -> bool + Send + 'static,
) -> std::thread::JoinHandle<()> {
    let shared = shared.clone();
    worker(name, move || {
        let followed = shared
            .stop_pipe()
            .and_then(|stop| sys::until_ends(pid, &stop, &ended));
        match followed {
            Ok(sys::End::Ended) => {
                tell(format_args!("· Symphony server: {why}: stopping"));
                shared.stop();
            }
            Ok(sys::End::Stopped) => {}
            Err(problem) => tell(format_args!(
                "· Symphony server: cannot follow pid {pid}'s end, so only its stop ends it: \
                 {problem}"
            )),
        }
    })
}

/// Includes early startup failures: no background command may outlive this server invocation.
struct Background {
    shared: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Background {
    /// Stop the server, finishing its data refresh, then join each worker, naming each wait
    /// through `stage` as it begins.
    fn join(&mut self, stage: &dyn Fn(&str)) {
        stage("the data refresh");
        self.shared.stop();
        for worker in self.workers.drain(..) {
            let name = worker
                .thread()
                .name()
                .unwrap_or("review background")
                .to_owned();
            stage(&format!("the {name} worker"));
            if worker.join().is_err() {
                tell(format_args!("· {name} worker panicked while stopping"));
            }
        }
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.join(&|_| {});
    }
}

/// Names each wait of a stopping server as it begins, so a stop that never ends names its wait.
type Stages = Arc<dyn Fn(&str) + Send + Sync>;

/// [`Stages`] writing each stage's line to `log`, naming the generation `pid` that stops: a
/// generation can reload into the next while the one before it still drains, and every
/// generation writes to the one log it inherited.
fn stages_of(pid: i64, log: impl Fn(String) + Send + Sync + 'static) -> Stages {
    Arc::new(move |stage| log(format!("· Symphony server pid {pid} stopping: {stage}")))
}

/// [`Stages`] of generation `pid` on stderr, where the shared server's log stamps each line
/// with its time.
fn stages_on_stderr(pid: i64) -> Stages {
    stages_of(pid, |line| tell(format_args!("{line}")))
}

/// `line` on stderr, a failed write ignored: a stopping server says what it does on the way,
/// and a closed or broken stderr must never end a stop before every save closes and every
/// worker joins (`eprintln!` panics on a failed write).
pub(crate) fn tell(line: std::fmt::Arguments<'_>) {
    #[cfg(test)]
    if told::kept(line) {
        return;
    }
    let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{line}\n"));
}

/// What [`tell`] says on a test's own thread, kept for the test instead of written to stderr.
#[cfg(test)]
pub(crate) mod told {
    use std::cell::RefCell;

    thread_local! {
        static LINES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    }

    /// What `run` returns, and each line told on this thread while it ran.
    pub(crate) fn during<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
        LINES.with(|lines| *lines.borrow_mut() = Some(Vec::new()));
        let result = run();
        let lines = LINES.with(|lines| lines.borrow_mut().take());
        (result, lines.unwrap_or_default())
    }

    /// Keep `line` when this thread's lines are kept: whether it was.
    pub(super) fn kept(line: std::fmt::Arguments<'_>) -> bool {
        LINES.with(|lines| {
            let mut lines = lines.borrow_mut();
            lines
                .as_mut()
                .map(|lines| lines.push(line.to_string()))
                .is_some()
        })
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
    crate::spawn::task(async move {
        signal.await;
        let _ = crate::spawn::blocking(move || shared.stop()).await;
    })
}

/// A listening server and the workers it owns until it returns.
struct Started {
    serve: Serve,
    shared: Arc<Shared>,
    /// The one server's other projects, stopped with it; none for a server of its own.
    projects: Option<Arc<crate::projects::Projects>>,
    background: Background,
    serving: Vec<tokio::task::JoinHandle<()>>,
    /// This server wrote the port record, and clears it as it ends.
    record: bool,
    /// Where the stop names its waits ([`stages_on_stderr`] outside tests).
    stages: Stages,
    /// How long its open requests are waited for once it was replaced ([`REPLACED_DRAIN`]
    /// outside tests).
    drain: Duration,
}

/// Listen, start the owned workers and print the URL; a failed start is its exit status.
async fn start(mut serve: Serve, open: bool) -> Result<Started, i32> {
    // The release this server is, read once, now: what it serves and follows for its life.
    if let Some((_, manifest)) = symphony::machine::release::this() {
        eprintln!("· Symphony: the server of the release {}", manifest.release);
    }
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
    // The bound port is discoverable during warmup. A private preview on main leaves the shared
    // server's hint alone.
    let record = serve.home.is_some()
        || crate::repo::checkout(&serve.root).map_or(true, |checkout| checkout.linked());
    if record {
        record_port(&serve.root, port);
    }
    // The one server serves its starter's project at `/<id>/`, beside every other registered one.
    let base = if serve.home.is_some() {
        format!("/{}", symphony::project::Project::of(&serve.root).id)
    } else {
        String::new()
    };
    let shared = Shared::project(
        i64::from(std::process::id()),
        port,
        &serve.root,
        serve.installed(),
        Gate::tailscale(),
        &base,
    );
    if reload.is_replacement() {
        // The generation it replaces records until the hand-off commits, its agents and its
        // board's hours; this one from then on.
        shared.recording.hold();
    }
    let mut background = Background {
        shared: shared.clone(),
        workers: Vec::new(),
    };
    {
        let shared = shared.clone();
        background.workers.push(worker(TAILNET_WORKER, move || {
            *shared.tailnet.lock().held() = crate::gate::magicdns_name();
        }));
    }
    if serve.home.is_none() {
        // A server of its own port is a test's, or a terminal's: it ends with the process that started it.
        // Its parent's end gives it another parent at once, before the end is reaped.
        let parent = sys::parent();
        background.workers.push(end_when(
            &shared,
            STARTER_WORKER,
            "the process that started it ended",
            parent,
            move || sys::parent() != parent,
        ));
    }
    if let Some(test) = TEST_PID
        .text()
        .and_then(|pid| pid.parse::<i64>().ok())
        .filter(|&pid| pid > 1)
    {
        // Taken by its start, so a pid ended, unreaped or given again reads as ended.
        let taken = sys::Taken::of(test);
        background.workers.push(end_when(
            &shared,
            TEST_WORKER,
            "the test that started it ended",
            test,
            move || !taken.alive(),
        ));
    }
    // Documents are ready before the URL. The first board read runs in the hub's owned worker;
    // data requests await it, and a replacement keeps the same barrier before handover.
    let warm = if reload.is_replacement() {
        crate::api::warm(&shared).await
    } else {
        crate::api::start(&shared).await
    };
    if let Err(why) = warm {
        eprintln!("· native read data could not be warmed: {why}");
        if reload.is_replacement() {
            return Err(2);
        }
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
    let projects = serve
        .home
        .as_deref()
        .map(|machine| crate::projects::Projects::new(machine, &shared));
    let app = projects
        .clone()
        .map_or_else(|| router(shared.clone()), crate::projects::router);
    let mut serving = Vec::new();
    for listener in listeners {
        let shared = shared.clone();
        let app = app.clone();
        serving.push(crate::spawn::task(async move {
            connections::serve(listener, app, shared).await;
        }));
    }
    let ready = crate::spawn::blocking(move || {
        reload.ready()?;
        Ok::<_, String>(reload)
    })
    .await;
    match ready {
        Ok(Ok(reload)) => {
            // Committed: a replacement records from here (a gate never held stays as it was).
            shared.recording.open(symphony_board::sweep::trigger::now());
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
    // The generation is live from here, and only from here does any project's maintenance start:
    // a hand-off that fails ends a replacement by a kill that stops no state, and would leave the
    // children of a maintenance started before it. The starter's project is maintained by workers
    // of the server's own; each other project it serves by workers its state holds, started by
    // the projects' watch ([`crate::projects::Projects::watch`]), which then reads the registry
    // again on its own so a project registered or removed starts or stops its maintenance with
    // no request.
    background
        .workers
        .extend(maintenance(&shared, serve.home.as_deref()));
    if let Some(projects) = &projects {
        background.workers.push(projects.watch());
    }
    if serve.home.is_some() {
        handover_when_asked(&shared);
    }
    let url = format!("http://127.0.0.1:{port}/");
    println!("→ Symphony at {url}  (pid {})", shared.pid);
    if let Some(hub) = shared.api.get() {
        hub.start_initial_board();
    }
    if open {
        crate::cli::open(&url);
    }
    let stages = stages_on_stderr(shared.pid);
    Ok(Started {
        serve,
        shared,
        projects,
        background,
        serving,
        record,
        stages,
        drain: REPLACED_DRAIN,
    })
}

/// A starter whose build must replace this server asks with SIGUSR1, its request in the machine
/// folder ([`crate::host::Handover`]); the reload worker, woken, reads it at once.
fn handover_when_asked(shared: &Arc<Shared>) {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut asked) = signal(SignalKind::user_defined1()) else {
        eprintln!("· Symphony server: cannot watch for hand-off requests");
        return;
    };
    let shared = shared.clone();
    crate::spawn::task(async move {
        loop {
            tokio::select! {
                () = shared.until_stopped() => return,
                received = asked.recv() => {
                    if received.is_none() {
                        return;
                    }
                    shared.ask_handover();
                }
            }
        }
    });
}

impl Started {
    /// Serve until the server stops, on `signal` or for any other reason, then drain and join
    /// every owned worker, naming each wait as it begins.
    async fn serve_until_stopped(self, signal: impl Future<Output = ()> + Send + 'static) -> i32 {
        let Started {
            serve,
            shared,
            projects,
            mut background,
            serving,
            record,
            stages,
            drain,
        } = self;
        let signals = stop_when(&shared, signal);
        shared.until_stopped().await;
        signals.abort();
        let others = match projects {
            Some(projects) => {
                stages("the other projects");
                crate::spawn::blocking(move || projects.stop())
                    .await
                    .unwrap_or_default()
            }
            None => Vec::new(),
        };
        stages("the open requests");
        // A replaced generation finishes what it accepted, within `drain` in all; what is still
        // open then is dropped as this process ends, and its peer asks the replacement again.
        let drained = tokio::time::Instant::now() + drain;
        for task in serving {
            if shared.replaced.load(Ordering::SeqCst) {
                if tokio::time::timeout_at(drained, task).await.is_err() {
                    stages(&format!(
                        "the open requests outlasted {} s: dropping what is still open",
                        drain.as_secs_f64()
                    ));
                    break;
                }
            } else {
                let _ = tokio::time::timeout(SHUTDOWN_GRACE, task).await;
            }
        }
        let draining = shared.clone();
        let closing = stages.clone();
        let _ = crate::spawn::blocking(move || {
            if let Some(saves) = draining.saves.get() {
                saves.close_saying(|stage| closing(stage));
            }
            crate::projects::close_saves(&others, &*closing);
        })
        .await;
        background.join(&*stages);
        drop(background);
        if record && !shared.replaced.load(Ordering::SeqCst) {
            clear_port(&serve.root, serve.port);
        }
        tell(format_args!("· Symphony server stopped"));
        0
    }
}

/// What the machine's one server (`home`, its machine folder) runs for each project it serves, on
/// the wall clock: the idle sweep ([`session_maintenance`]), a worker of its own, so one project's
/// hung or failing round never holds up another's. A private preview (no `home`) runs none.
pub(crate) fn maintenance(
    shared: &Arc<Shared>,
    home: Option<&Path>,
) -> Vec<std::thread::JoinHandle<()>> {
    session_maintenance(shared, home, MachineClock)
        .into_iter()
        .collect()
}

/// Whether the project `shared` serves has a board to maintain, as its hub last read it
/// ([`crate::live::Hub::has_board`]). A round waits while it has none, or while that is not known
/// yet (no hub, or its first board read not done), so maintenance never makes a board in a
/// registered project that has none.
fn has_board(shared: &Shared) -> bool {
    shared.api.get().and_then(|hub| hub.has_board()) == Some(true)
}

/// How the log names the project a maintenance line is about: ` of ID` for a project the one
/// server serves under `/ID`, nothing for a server of its own.
fn of_project(shared: &Shared) -> String {
    match shared.base.trim_start_matches('/') {
        "" => String::new(),
        id => format!(" of {id}"),
    }
}

/// A private preview may use a copied board, whose old pane identities must never be used to
/// clean up real agent terminals. Only the machine's shared installation starts this worker,
/// which reads the time from `clock` ([`MachineClock`] outside tests).
fn session_maintenance(
    shared: &Arc<Shared>,
    home: Option<&Path>,
    clock: impl Clock,
) -> Option<std::thread::JoinHandle<()>> {
    let home = home?;
    Some(maintenance_worker(
        Arc::clone(shared),
        Duration::from_secs_f64(REAP_SECONDS),
        SWEEP_TIMEOUT,
        clock,
        crate::held::Retrying::of_this_server(home),
    ))
}

/// A moment on both clocks. `Instant` stops while the machine sleeps, but the board's presence
/// caps run on wall time, so the schedule reads both: after a sleep the wall clock makes a round
/// due at once, before a live pane's Unknown cap expires.
#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    mono: Instant,
    wall: SystemTime,
}

impl Stamp {
    pub(crate) fn now() -> Stamp {
        Stamp {
            mono: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// The wall clock's time, in seconds since the epoch.
    pub(crate) fn wall_seconds(self) -> f64 {
        self.wall
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64())
    }

    /// Both clocks, as a command's deadline reads them ([`sys::output_while`]).
    pub(crate) fn both(self) -> (Instant, SystemTime) {
        (self.mono, self.wall)
    }

    /// Wall time since `earlier`; zero if the wall clock was set back.
    fn wall_since(self, earlier: Stamp) -> Duration {
        self.wall
            .duration_since(earlier.wall)
            .unwrap_or(Duration::ZERO)
    }
}

/// The clock a maintenance worker times its rounds by and waits on for the next: the machine's
/// ([`MachineClock`]), or a test's, which wakes the wait as the test moves it.
pub(crate) trait Clock: Send + 'static {
    /// The time now, on both clocks.
    fn now(&self) -> Stamp;

    /// Until `wakes` is woken, or until `left` has passed on this clock.
    fn wait(&self, wakes: &Wakes, left: Duration);
}

/// The machine's clocks, as [`Stamp::now`] reads them.
#[derive(Clone, Copy)]
pub(crate) struct MachineClock;

impl Clock for MachineClock {
    fn now(&self) -> Stamp {
        Stamp::now()
    }

    fn wait(&self, wakes: &Wakes, left: Duration) {
        wakes.wait(Some(left));
    }
}

/// The longest a maintenance worker waits for its next round before it reads its clocks again: a
/// machine that slept stopped `Instant` while its wall clock went on, which no event says, so a
/// round that the wall clock made due during a sleep comes within this of the wake, before a live
/// pane's Unknown cap expires. It is the backstop's period, the check for what no event says.
const AFTER_A_SLEEP: Duration = crate::backstop::PERIOD;

/// A round is due when either clock says `interval` has passed since the last one started, so a
/// wake from sleep runs one at once and a wall clock set back cannot hold it off.
fn sweep_due(last: Option<Stamp>, now: Stamp, interval: Duration) -> bool {
    last.is_none_or(|at| {
        now.mono.saturating_duration_since(at.mono) >= interval || now.wall_since(at) >= interval
    })
}

/// How long until the round after one that started at `last` is due ([`sweep_due`]): the sooner
/// of the two clocks' ends of `interval`, none once either has passed it.
fn sweep_left(last: Stamp, now: Stamp, interval: Duration) -> Duration {
    let mono = interval.saturating_sub(now.mono.saturating_duration_since(last.mono));
    mono.min(interval.saturating_sub(now.wall_since(last)))
}

/// How far past its interval, on the wall clock, a round starts; `None` within [`LATE_SWEEP`].
fn sweep_late(last: Stamp, now: Stamp, interval: Duration) -> Option<Duration> {
    let late = now.wall_since(last).checked_sub(interval)?;
    (late > LATE_SWEEP).then_some(late)
}

/// The build of `tasks` that runs the board for the checkout at `root`, as its recipes would from its
/// main checkout ([`symphony_board::builds::of_main`]): the main checkout's `.target/bin/tasks`, else
/// the machine's release's, else `tasks` on this server's `PATH`, never a linked worktree's build of branch code. None found is an
/// error saying where one goes.
pub(crate) fn board_build(root: &Path) -> Result<PathBuf, String> {
    board_build_on(root, &symphony_board::builds::Beyond::of_process())
}

/// [`board_build`] past the main checkout's link in `beyond`.
fn board_build_on(root: &Path, beyond: &symphony_board::builds::Beyond) -> Result<PathBuf, String> {
    use symphony_board::builds;
    let main = builds::main_of(root);
    builds::of_main(&main, "tasks", beyond).ok_or_else(|| builds::missing(&main, "tasks"))
}

/// A round every `interval`, whose reap child is ended after `timeout`, both measured on `clock`,
/// in the checkout of the project `shared` serves: its board, its build of `tasks`, its `reap.json`
/// and its health record. Before its reap, a round tries the project's held update again when
/// `retrying` names this server's machine folder and release ([`crate::held`]), each command of
/// that within `timeout` too. Between two it waits on `clock` until the next is due, the board
/// found or lost, or the stop; a round due waits while the project has no board ([`has_board`]).
/// No wait is longer than [`AFTER_A_SLEEP`].
pub(crate) fn maintenance_worker(
    shared: Arc<Shared>,
    interval: Duration,
    timeout: Duration,
    clock: impl Clock,
    retrying: Option<crate::held::Retrying>,
) -> std::thread::JoinHandle<()> {
    worker(SWEEP_WORKER, move || {
        let wakes = Wakes::new();
        shared.wake_at_stop(wakes.waker());
        shared.wake_at_board(wakes.waker());
        let of = of_project(&shared);
        let mut last: Option<Stamp> = None;
        // A round due that waited for a board: late for that, not for a sleep.
        let mut board_waited = false;
        while !shared.stopping() {
            let now = clock.now();
            let due = sweep_due(last, now, interval);
            if !due || !has_board(&shared) {
                board_waited |= due;
                // A round not due yet is waited for on the clock; one due, on the board. Either
                // looks again within AFTER_A_SLEEP, for what no wake says: a sleep, and the
                // project's folder, which `has_board` reads besides the hub's board.
                let left = match last {
                    Some(at) if !due => sweep_left(at, now, interval).min(AFTER_A_SLEEP),
                    _ => AFTER_A_SLEEP,
                };
                clock.wait(&wakes, left);
                continue;
            }
            let started = Instant::now();
            if let Some(late) = last
                .filter(|_| !board_waited)
                .and_then(|at| sweep_late(at, now, interval))
            {
                eprintln!(
                    "· idle-session sweep{of} started {:.0}s late: the machine slept or the worker was not running",
                    late.as_secs_f64()
                );
            }
            last = Some(now);
            board_waited = false;
            // A held update first: a board moved now is swept in this same round.
            if let Some(retrying) = &retrying {
                for line in crate::held::round(&shared, retrying, timeout, &clock) {
                    eprintln!("· held update{of}: {line}");
                }
            }
            // The board's nonblocking reap lock admits one sweeper across all servers.
            // Log before waiting on output: a hung child must still leave its actual start time.
            eprintln!("· idle-session sweep{of} started");
            // The tick waits after a sleep for a round begun after the wake (`live::presence`).
            if let Some(hub) = shared.api.get() {
                hub.sweep_began();
            }
            let swept = match crate::live::measured(&format!("idle-session sweep{of}"), || {
                let mut command = Command::new(board_build(&shared.root)?);
                command.arg("reap").current_dir(&shared.root);
                sys::output_while(
                    command,
                    timeout,
                    || !shared.stopping(),
                    || {
                        let at = clock.now();
                        (at.mono, at.wall)
                    },
                )
            }) {
                Ok(done) if done.status.success() => {
                    if !done.stderr.is_empty() {
                        eprintln!(
                            "· idle-session sweep{of} says:\n{}",
                            String::from_utf8_lossy(&done.stderr).trim()
                        );
                    }
                    Some(Ok(()))
                }
                Ok(done) => {
                    let why = format!(
                        "{}: {}",
                        done.status,
                        String::from_utf8_lossy(&done.stderr).trim()
                    );
                    eprintln!("· idle-session sweep{of} failed: {why}");
                    Some(Err(why))
                }
                Err(_) if shared.stopping() => {
                    eprintln!("· idle-session sweep{of} cancelled");
                    None
                }
                Err(e) => {
                    eprintln!("· idle-session sweep{of} failed: {e}");
                    Some(Err(e.to_string()))
                }
            };
            eprintln!(
                "· idle-session sweep{of} ended after {:.3}s",
                started.elapsed().as_secs_f64()
            );
            // Answered or failed, the round has looked at the listeners it could: the tick may judge.
            if let (Some(_), Some(hub)) = (&swept, shared.api.get()) {
                hub.sweep_ended();
            }
            // A cancelled sweep says nothing of the board, nor does one that found no board
            // ([`symphony_board::board_health::absent`]); the hub is there once the server is. The
            // record keeps the error on one line, cut when long; the log above keeps it whole.
            let swept = swept.filter(|outcome| {
                outcome
                    .as_ref()
                    .err()
                    .is_none_or(|why| !symphony_board::board_health::absent(why))
            });
            if let (Some(outcome), Some(hub)) = (swept, shared.api.get()) {
                let now = hub.now();
                hub.health
                    .swept(now, outcome.as_ref().copied().map_err(String::as_str));
                hub.tell_health(now);
            }
            // The next round is due `interval` after this one began: the next look waits for it.
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
mod tests;

#[cfg(test)]
mod sweep_tests;

#[cfg(test)]
mod head_tests;

#[cfg(test)]
mod nosniff_tests;

#[cfg(test)]
mod broken_stderr_tests;

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod peers_tests;

#[cfg(test)]
pub(crate) mod test_clock;
