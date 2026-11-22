//! The one server's projects: every project the machine folder's registry holds, each served under
//! `/<id>/` by a router of its own, so each keeps its own watcher, feed cursor, board snapshot and
//! branch scan (its own [`Shared`] and hub) and nothing of one is visible from another's pages.
//!
//! Before any project the server answers `/api/front` (itself, with the ids it serves),
//! `/api/projects` (each project's counts and task asks, as data), `/` (the
//! project picker), `/<id>` (to `/<id>/`) and `/<id>/theme.css` (the
//! project's theme tokens). A path of today, under no project, is the project's whose page asked
//! for it (a same-origin Referer under `/<id>/`), else the starter's, when that project answers it
//! (a route of the Symphony's or of its plugins', one under its entrypoints, or a file of its
//! checkout a document names root-relative): a page load is redirected there, so a bookmark opens
//! under `/<id>/`, and anything else is answered by that project in place; a project this server
//! refuses answers its refusal on either way in. An id spelled with escapes (`/%61lpha/`) is
//! redirected to its plain address. Any other path, an unknown project id say, is the project
//! list's page saying so, never another project's.
//! Every route but `/api/front`, which loopback's name alone reaches, answers a Host naming this
//! machine alone.
//! A project's state is its registered root's under its id: until the server stops, the registry
//! is read again whenever its file changes, at a request or at the server's own look, which a
//! watcher of the machine folder wakes ([`Projects::watch`]), and a changed root or id is served
//! from new state.
//!
//! A project this build cannot serve by the host rule (`symphony::machine::host`) is refused, its
//! pages saying why and what serves it: for a release's server, the `[plugins.process]` command a
//! linked plugin needs, or the release `current` must name. While a project's build serves and a
//! release is installed, each project naming a linked plugin says on its pages that it holds the
//! machine on a project's build (`symphony::machine::switch`), read again with the registry.
//!
//! Each project's state holds the maintenance of its board, its idle sweep
//! ([`crate::front::maintenance`]), on a schedule of its own: started once the
//! server's generation is live, its socket hand-off committed, as the starter's own are, or when
//! the project is first served after that; stopped when it is retired or the server stops. The
//! starter's checkout is maintained by the server's own workers, under whatever id it is
//! registered with.

use crate::lock::Held as _;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Json, Response};
use notify::Watcher;
use symphony::entrypoint::below;
use symphony::machine::{Entry, PROJECTS, Registry, switch};
use tower::ServiceExt;

use crate::backstop::{self, Backstop, Wake, Waker};
use crate::front::Shared;
use crate::host::{Build, Lack};
use crate::live::{Signal, Standing};

mod retired;

pub(crate) use retired::{Stopped, close_saves};

/// The thread name of the server's look at the registry ([`Projects::watch`]).
pub(crate) const REGISTRY_WORKER: &str = "review project registry";

/// What starts the maintenance of a project served beside the starter's, the workers its state
/// holds until it stops ([`Shared::maintained`]).
pub(crate) type Maintain = dyn Fn(&Arc<Shared>) -> Vec<std::thread::JoinHandle<()>> + Send + Sync;

/// Whether a watcher's `event` in the machine folder can have changed what the server's look reads:
/// the registry ([`PROJECTS`]) or the `current` link, which says what each project's pages say of
/// the machine's server ([`switch`]), written, renamed into place or removed; a rescan; or the
/// watcher's own error.
fn registry_event(event: &notify::Result<notify::Event>) -> bool {
    event.as_ref().map_or(true, |event| {
        event.need_rescan()
            || (!matches!(event.kind, notify::EventKind::Access(_))
                && event.paths.iter().any(|path| {
                    path.file_name().is_some_and(|name| {
                        name == PROJECTS || name == symphony::machine::release::CURRENT
                    })
                }))
    })
}

/// A watcher `W` of `machine`'s own entries, kept while the server looks, which wakes `look` at
/// each [`registry_event`]. A watcher that cannot start, or cannot watch the folder, is said in the
/// log and wakes nothing: the backstop's period then reads the registry alone.
fn watch_registry<W: Watcher>(machine: &Path, look: Waker) -> Option<W> {
    let handler = move |event: notify::Result<notify::Event>| {
        if registry_event(&event) {
            look.wake();
        }
    };
    W::new(handler, notify::Config::default())
        .and_then(|mut watcher| {
            watcher.watch(machine, notify::RecursiveMode::NonRecursive)?;
            Ok(watcher)
        })
        .inspect_err(|why| {
            eprintln!(
                "· Symphony projects: cannot watch {} for the registry: {why}; it is read once a \
                 period",
                machine.display()
            );
        })
        .ok()
}

/// The registered projects and the server that serves them.
pub(crate) struct Projects {
    machine: PathBuf,
    starter: Arc<Shared>,
    /// The id the starter's state is served under, its base without the slash.
    starter_id: String,
    /// The starter's checkout as the registry names a root: canonical.
    starter_root: PathBuf,
    build: Build,
    warming: Warming,
    /// The maintenance each other project's state runs: the server's ([`crate::front::maintenance`]);
    /// none in the dispatch's tests.
    maintain: Option<Box<Maintain>>,
    held: Mutex<Held>,
}

/// When a project read from the registry has its read data warmed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Warming {
    /// As it is read, so the list counts it: the server's.
    AsRead,
    /// On its first request: the dispatch's tests', which read the dispatch alone.
    #[cfg(test)]
    OnRequest,
}

#[derive(Default)]
struct Held {
    /// The registry file's modification time and length when it was last read; none before.
    read: Option<Option<(SystemTime, u64)>>,
    /// The projects in the registry's order, the starter's among them.
    served: Vec<Served>,
    /// The state of the projects it no longer names as they were.
    retired: Vec<retired::Retired>,
    /// Whether the server stopped them: the projects served stay as they are from then on.
    stopped: bool,
    /// Whether the server's generation is live, its socket hand-off committed
    /// ([`Projects::watch`]): each other project's maintenance starts from then, never before, as
    /// a hand-off that fails ends this generation by a kill that stops no state.
    live: bool,
}

/// One project as this server serves it.
#[derive(Clone)]
pub(crate) struct Served {
    pub entry: Entry,
    pub shared: Arc<Shared>,
    router: Router,
    /// Why this server does not serve it, when it does not.
    pub refused: Option<String>,
}

impl Projects {
    /// The projects the registry in `machine` holds, served beside `starter`'s, each warmed as it is
    /// read and maintained once the server's generation is live ([`Projects::watch`]).
    pub(crate) fn new(machine: &Path, starter: &Arc<Shared>) -> Arc<Projects> {
        let home = machine.to_path_buf();
        let maintain = move |shared: &Arc<Shared>| crate::front::maintenance(shared, Some(&home));
        Projects::with(machine, starter, Warming::AsRead, Some(Box::new(maintain)))
    }

    #[cfg(test)]
    fn made(machine: &Path, starter: &Arc<Shared>, warming: Warming) -> Arc<Projects> {
        Projects::with(machine, starter, warming, None)
    }

    fn with(
        machine: &Path,
        starter: &Arc<Shared>,
        warming: Warming,
        maintain: Option<Box<Maintain>>,
    ) -> Arc<Projects> {
        let projects = Arc::new(Projects {
            machine: machine.into(),
            starter: starter.clone(),
            starter_id: starter.base.trim_start_matches('/').to_string(),
            starter_root: starter
                .root
                .canonicalize()
                .unwrap_or_else(|_| starter.root.clone()),
            build: Build::this(&starter.root),
            warming,
            maintain,
            held: Mutex::default(),
        });
        drop(projects.read());
        projects
    }

    /// The server's generation is live, its socket hand-off committed: start the maintenance of
    /// every other project it serves now, and of each it serves from now on, then look at the
    /// registry until the starter stops, as a request reads it, so a project registered is served,
    /// warmed and maintained, and one removed retired and its maintenance stopped, though no page
    /// asks. The look runs on a thread of its own, within the runtime it is started in, which warms
    /// what it reads: once as it starts, then at each event of a watcher of the machine folder that
    /// names the registry or `current` ([`registry_event`]), and once a backstop period, as an
    /// event can be lost.
    pub(crate) fn watch(self: &Arc<Self>) -> std::thread::JoinHandle<()> {
        self.watch_with::<notify::RecommendedWatcher>(backstop::PERIOD)
    }

    /// [`Projects::watch`] by watcher `W`, the registry read on its own every `period`.
    fn watch_with<W: Watcher>(self: &Arc<Self>, period: Duration) -> std::thread::JoinHandle<()> {
        self.go_live();
        let projects = Arc::clone(self);
        let runtime = tokio::runtime::Handle::try_current().ok();
        crate::front::worker(REGISTRY_WORKER, move || {
            let _within = runtime.as_ref().map(tokio::runtime::Handle::enter);
            let mut backstop = Backstop::new(period);
            projects.starter.wake_at_stop(backstop.waker());
            // Watched before the first look, so no change after it goes unheard.
            let _watcher = watch_registry::<W>(&projects.machine, backstop.waker());
            while !projects.starter.stopping() {
                drop(projects.read());
                projects.tell_switch();
                // A registry written and renamed into place is one change: read once both are done.
                if backstop.wait() == Wake::Event && !projects.starter.stopping() {
                    backstop.debounce(backstop::QUIET, backstop::QUIET);
                }
            }
        })
    }

    /// Tell each project served whose hub is up what the machine's server is to it
    /// ([`switch`]): while this server is a project's build and the machine has a release
    /// installed, each project naming a linked plugin holds the machine on a project's build, and
    /// its pages say so; nothing otherwise. A release's server refuses such a project, whose page
    /// says why ([`Projects::refused`]).
    fn tell_switch(&self) {
        let served = self.served();
        let installed = (self.build.release.is_none())
            .then(|| symphony::machine::release::current_installed(&self.machine))
            .flatten();
        for project in &served {
            let Some(hub) = project.shared.api.get() else {
                continue;
            };
            hub.tell_machine(holding(installed.as_ref(), &project.entry));
        }
    }

    /// Start the maintenance of each other project served now, once: none once the server stopped
    /// them or its starter is stopping, as nothing would join it.
    fn go_live(&self) {
        let mut held = self.held.lock().held();
        if held.live || held.stopped || self.starter.stopping() {
            return;
        }
        held.live = true;
        for served in &held.served {
            self.maintain(&served.entry, &served.shared);
        }
    }

    /// Start the maintenance of `shared`, the state `entry` is served from, unless it is the
    /// starter's checkout, under whatever id, whose board the server's own workers maintain.
    fn maintain(&self, entry: &Entry, shared: &Arc<Shared>) {
        if entry.root != self.starter_root
            && let Some(maintain) = &self.maintain
        {
            shared.maintained(maintain(shared));
        }
    }

    /// The projects served now, the registry read again when its file changed.
    pub(crate) fn served(&self) -> Vec<Served> {
        self.read().served.clone()
    }

    /// What is served now, held, the registry read again when its file changed.
    fn read(&self) -> std::sync::MutexGuard<'_, Held> {
        // A panic while reading leaves the list before it whole: the new one is stored last, in
        // one assignment with the stamp it was read at.
        let mut held = self.held.lock().held();
        let stamp = fs::metadata(self.machine.join(PROJECTS)).ok().map(|meta| {
            (
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
            )
        });
        if held.stopped || held.read == Some(stamp) {
            return held;
        }
        let registry = Registry::read(&self.machine).unwrap_or_else(|why| {
            eprintln!("· Symphony projects: {why}; serving the starter's project alone");
            Registry::default()
        });
        let live = held.live;
        let mut next: Vec<Served> = Vec::new();
        for entry in registry.projects {
            if next.iter().any(|served| served.entry.id == entry.id) {
                continue;
            }
            let kept = held
                .served
                .iter()
                .find(|served| served.entry.id == entry.id && served.entry.root == entry.root);
            next.push(match kept {
                Some(kept) => Served {
                    refused: self.refused(&entry),
                    entry,
                    ..kept.clone()
                },
                None => self.serve(entry, live),
            });
        }
        // The starter's checkout, unregistered, is served under the id it started with, unless
        // another checkout holds that id now.
        if !next.iter().any(|served| {
            served.entry.root == self.starter_root || served.entry.id == self.starter_id
        }) {
            let entry = Entry::of(&self.starter.root).unwrap_or_else(|_| Entry {
                id: self.starter_id.clone(),
                name: self.starter_id.clone(),
                root: self.starter_root.clone(),
                manifest: None,
                plugins: Vec::new(),
                pin: self.build.pin,
                schema: self.build.schema,
            });
            next.insert(
                0,
                self.serve(
                    Entry {
                        id: self.starter_id.clone(),
                        ..entry
                    },
                    live,
                ),
            );
        }
        let gone: Vec<Stopped> = held
            .served
            .iter()
            .filter(|old| {
                !Arc::ptr_eq(&old.shared, &self.starter)
                    && !next
                        .iter()
                        .any(|served| Arc::ptr_eq(&served.shared, &old.shared))
            })
            .map(|old| Stopped {
                id: old.entry.id.clone(),
                shared: old.shared.clone(),
            })
            .collect();
        retired::retire(gone, &mut held.retired);
        held.read = Some(stamp);
        held.served = next;
        held
    }

    /// `entry` served: the starter's own state while the entry is the starter's checkout under the
    /// id it started with, a new one beside it for any other root or id. A new state starts the
    /// maintenance of its board while the server is `live` ([`Projects::maintain`]), which waits
    /// while the project has no board.
    fn serve(&self, entry: Entry, live: bool) -> Served {
        let shared = if entry.root == self.starter_root && entry.id == self.starter_id {
            self.starter.clone()
        } else {
            self.starter.beside(&entry.root, &format!("/{}", entry.id))
        };
        if live {
            self.maintain(&entry, &shared);
        }
        let refused = self.refused(&entry);
        if self.warming == Warming::AsRead
            && refused.is_none()
            && !Arc::ptr_eq(&shared, &self.starter)
            && tokio::runtime::Handle::try_current().is_ok()
        {
            let warming = shared.clone();
            crate::spawn::task(async move {
                if let Err(why) = crate::api::warm(&warming).await {
                    eprintln!("· {}: read data could not be warmed: {why}", warming.base);
                }
            });
        }
        Served {
            router: crate::front::router(shared.clone()),
            refused,
            shared,
            entry,
        }
    }

    /// Why this server cannot serve `entry`, naming what fixes it by the projects' ids: the page
    /// that says it names no folder of this machine.
    fn refused(&self, entry: &Entry) -> Option<String> {
        if entry.root != self.starter_root && !entry.root.is_dir() {
            return Some("its folder is gone: run `just symphony` in its new folder".to_string());
        }
        lacking(&self.build, &self.starter_id, entry)
    }

    /// The project today's paths belong to: the starter's checkout under whatever id it is
    /// registered with now, else the project that holds the id it started with.
    fn started<'a>(&self, served: &'a [Served]) -> Option<&'a Served> {
        served
            .iter()
            .find(|served| served.entry.root == self.starter_root)
            .or_else(|| {
                served
                    .iter()
                    .find(|served| served.entry.id == self.starter_id)
            })
    }
}

/// Why `build`, the server `starter_id` started, cannot serve `entry`, naming what fixes it; none
/// when it can. A release's server is replaced by no start, so its fixes are a plugin's process
/// command and a release that serves the project.
fn lacking(build: &Build, starter_id: &str, entry: &Entry) -> Option<String> {
    let lack = build.lacks(entry)?;
    let fix = match (&lack, &build.release) {
        (Lack::Plugins(_), None) => format!(
            "run `just symphony` in {}'s checkout, whose build holds them",
            entry.id
        ),
        (Lack::Plugins(names), Some(_)) => format!(
            "a [plugins.process] command for {} in {}, then `just symphony` in its checkout, lets \
             this server serve it (docs/PLUGIN_PROTOCOL.md)",
            names.join(", "),
            entry.root.join(symphony::project::FILE).display()
        ),
        (Lack::Pin(_) | Lack::Schema(_), None) => format!(
            "{starter_id}, which started this Symphony, must bump its Symphony, then run `just \
             symphony` there"
        ),
        (Lack::Pin(_) | Lack::Schema(_), Some(_)) => format!(
            "this server serves {} once the machine's `current` names a release that does, which \
             its reload follows",
            entry.id
        ),
    };
    Some(format!(
        "this Symphony {}: {fix}",
        lack.said(build, &entry.id)
    ))
}

/// What the pages of `entry` say of the machine's server while it is a project's build and
/// `installed` is the release the machine's `current` names: that `entry` holds it there, when it
/// names a linked plugin ([`switch::held`]), its project file named as its own
/// ([`switch::Hold::said_on_its_pages`]); nothing otherwise.
fn holding(installed: Option<&symphony::machine::release::Manifest>, entry: &Entry) -> String {
    let Some(installed) = installed else {
        return String::new();
    };
    let held = switch::held(&Registry {
        projects: vec![entry.clone()],
    });
    held.map(|hold| {
        format!(
            "The machine's Symphony stays a project's build, not the release {}: {}.",
            installed.release,
            hold.said_on_its_pages()
        )
    })
    .unwrap_or_default()
}

/// The one server's routes: every request, before any project's. Its own JSON answers say
/// `nosniff` as a project's do ([`crate::front::nosniff`]).
pub(crate) fn router(projects: Arc<Projects>) -> Router {
    Router::new()
        .fallback(dispatch)
        .layer(axum::middleware::map_response(crate::front::nosniff))
        .with_state(projects)
}

async fn dispatch(
    State(projects): State<Arc<Projects>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    if !crate::front::peer_allowed(&projects.starter, peer.ip()).await {
        return crate::front::peer_refused();
    }
    let path = request.uri().path().to_string();
    // `/api/front` holds to loopback's name alone; every other route to a name of this machine.
    if path != symphony_api::FRONT_PATH {
        let host = request
            .headers()
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .unwrap_or("")
            .to_string();
        let starter = projects.starter.clone();
        let named = crate::spawn::blocking(move || starter.allows_host(&host))
            .await
            .unwrap_or(false);
        if !named {
            return (
                StatusCode::FORBIDDEN,
                "Forbidden: the Host header does not name this machine\n",
            )
                .into_response();
        }
    }
    let reading = projects.clone();
    let Ok(served) = crate::spawn::blocking(move || reading.served()).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if path == symphony_api::FRONT_PATH {
        return front(&projects, &served, request.headers());
    }
    if path == symphony_api::PROJECTS_PATH {
        return listing(&projects, &served);
    }
    if path == "/" {
        return home(&served);
    }
    let (first, rest) = path[1..]
        .split_once('/')
        .map_or((&path[1..], None), |(first, rest)| (first, Some(rest)));
    if let Some(project) = served.iter().find(|served| served.entry.id == first) {
        let Some(rest) = rest else {
            return redirect(&format!("{path}/"), request.uri());
        };
        if rest == "theme.css" {
            return theme(project, request.headers());
        }
        if let Some(why) = &project.refused {
            return refusal(
                &served,
                project,
                why,
                &format!("/{rest}"),
                request.headers(),
            );
        }
        return forward(project, &format!("/{rest}"), request).await;
    }
    if let Some(plain) = respelled(&served, &path) {
        return redirect(&plain, request.uri());
    }
    // The address's own icon, asked for by no project's page (a file opened raw, the address
    // itself), is the Symphony's, never the started project's.
    if path == "/favicon.ico" && referred(request.headers(), &served).is_none() {
        return crate::mark::answer(request.uri().query());
    }
    // A path of today is its project's only when that project answers it: a first segment no
    // project holds and no route of the Symphony's or that project's is a project id mistyped.
    let Some(owner) = referred(request.headers(), &served)
        .or_else(|| projects.started(&served))
        .filter(|owner| answers(owner, &path))
    else {
        return unknown(&served, &path);
    };
    if let Some(why) = &owner.refused {
        return refusal(&served, owner, why, &path, request.headers());
    }
    if navigation(request.method(), request.headers()) {
        return redirect(&format!("/{}{path}", owner.entry.id), request.uri());
    }
    forward(owner, &path, request).await
}

/// A redirect to `path`, keeping `uri`'s query.
fn redirect(path: &str, uri: &Uri) -> Response {
    let location = match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, location)],
    )
        .into_response()
}

/// Whether a request loads a page: the browser says so, or an older one asks for HTML.
pub(crate) fn navigation(method: &Method, headers: &HeaderMap) -> bool {
    let read = |name| {
        headers
            .get(name)
            .and_then(|value: &HeaderValue| value.to_str().ok())
    };
    (method == Method::GET || method == Method::HEAD)
        && read("sec-fetch-mode").map_or_else(
            || read(header::ACCEPT.as_str()).is_some_and(|accept| accept.contains("text/html")),
            |mode| mode == "navigate",
        )
}

/// Whether `project` answers `path`, a path of today under no project's prefix, behind a branch's
/// scope or not: a route the Symphony or one of the project's plugins answers, a route under one
/// of its entrypoints (under one served at the project's root, only a name its folder holds), or
/// a file of its checkout it serves as media, as a document's picture or link names one
/// root-relative (`/look/icon.svg`, `/src/main.rs`).
fn answers(project: &Served, path: &str) -> bool {
    let decoded = crate::api::decode_path(path);
    let (scope, route) = crate::api::scope_path(decoded.as_deref().unwrap_or(path));
    let route = route.trim_start_matches('/');
    if route.is_empty() {
        // A branch's own board, `/@tree/`.
        return !scope.is_empty();
    }
    let root = &project.entry.root;
    let project = symphony::project::Project::of(root);
    project
        .reserved()
        .iter()
        .any(|reserved| below(reserved, route).is_some())
        || project.entrypoints.iter().any(|entry| {
            if entry.url.is_empty() {
                let first = route.split('/').next().unwrap_or_default();
                project.reachable(root, entry, first).is_some()
            } else {
                below(&entry.url, route).is_some()
            }
        })
        || holds(root, &project, route)
}

/// Whether the checkout at `root` holds what the media route `route` names
/// ([`crate::media_files::handles`]): its first part is there, where the project's router reads
/// it ([`symphony::project::Project::file`]), so that router answers it, a file it lacks in that
/// folder included, with the project's own miss.
fn holds(root: &std::path::Path, project: &symphony::project::Project, route: &str) -> bool {
    use std::path::{Component, Path};
    let first = route.split('/').next().unwrap_or_default();
    crate::media_files::handles(&format!("/{route}"))
        && matches!(
            Path::new(first).components().collect::<Vec<_>>()[..],
            [Component::Normal(_)]
        )
        && std::fs::symlink_metadata(root.join(project.file(first))).is_ok()
}

/// The plain address of `path` when its first segment spells a project's id with escapes
/// (`/%61lpha/tasks`), which names that project as `/alpha/tasks` does.
fn respelled(served: &[Served], path: &str) -> Option<String> {
    let decoded = crate::api::decode_path(path)?;
    let inside = |path: &str| path.strip_prefix('/').unwrap_or(path).to_string();
    let decoded = inside(&decoded);
    let (first, rest) = decoded.split_once('/').unwrap_or((&decoded, ""));
    let spelled = inside(path);
    let spelled = spelled.split('/').next().unwrap_or_default();
    (first != spelled && served.iter().any(|served| served.entry.id == first))
        .then(|| crate::api::encode_location(&format!("/{first}/{rest}")))
}

/// What a path under no project answers when no project answers it: the project list, saying no
/// project is called by its first segment, and naming one whose id differs from it in case alone.
fn unknown(served: &[Served], path: &str) -> Response {
    use symphony::markup::escape;
    let decoded = crate::api::decode_path(path).unwrap_or_else(|| path.to_string());
    let (first, rest) = decoded
        .trim_start_matches('/')
        .split_once('/')
        .unwrap_or((decoded.trim_start_matches('/'), ""));
    let mut said = format!(
        "<p><strong>No project is called <q>{}</q> here</strong>: this Symphony serves the \
         projects below.</p>",
        escape(first)
    );
    if let Some(near) = served
        .iter()
        .find(|served| served.entry.id.eq_ignore_ascii_case(first))
    {
        let href = crate::api::encode_location(&format!("/{}/{rest}", near.entry.id));
        said.push_str(&format!(
            "<p>Did you mean <a href=\"{}\">{}</a>?</p>",
            escape(&href),
            escape(&near.entry.id)
        ));
    }
    crate::pages::html(
        StatusCode::NOT_FOUND,
        crate::pages::machine(
            "Not found · Symphony",
            "not found",
            &crate::pages::notice("status", &said),
            &list(served),
        ),
    )
}

/// The project whose page a same-origin request came from.
pub(crate) fn referred<'a>(headers: &HeaderMap, served: &'a [Served]) -> Option<&'a Served> {
    let referer: Uri = headers.get(header::REFERER)?.to_str().ok()?.parse().ok()?;
    let host = headers.get(header::HOST)?.to_str().ok()?;
    if referer.authority()?.as_str() != host {
        return None;
    }
    let first = referer.path().trim_start_matches('/').split('/').next()?;
    served.iter().find(|served| served.entry.id == first)
}

/// `request` answered by `project`'s router at `inner`, its redirects kept under the project.
async fn forward(project: &Served, inner: &str, mut request: Request) -> Response {
    let target = match request.uri().query() {
        Some(query) => format!("{inner}?{query}"),
        None => inner.to_string(),
    };
    let Ok(uri) = target.parse::<Uri>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    *request.uri_mut() = uri;
    let Ok(mut response) = project.router.clone().oneshot(request).await;
    let base = &project.shared.base;
    if let Some(location) = response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .filter(|location| location.starts_with('/') && !location.starts_with("//"))
        && let Ok(rebased) = HeaderValue::from_str(&format!("{base}{location}"))
    {
        response.headers_mut().insert(header::LOCATION, rebased);
    }
    response
}

/// `GET /api/front`: the starter's status and the ids served, to loopback's name alone.
fn front(projects: &Projects, served: &[Served], headers: &HeaderMap) -> Response {
    if let Some(refused) = crate::front::loopback_refused(headers) {
        return refused;
    }
    let mut status = projects.starter.status();
    status.projects = served
        .iter()
        .map(|served| served.entry.id.clone())
        .collect();
    Json(status).into_response()
}

/// `GET /api/projects`: the server and each project it serves as data, for the desktop app
/// ([`symphony_api::ProjectList`]): what the project list at `/` says of each, from the hubs'
/// memory, so it reads no board.
fn listing(projects: &Projects, served: &[Served]) -> Response {
    Json(symphony_api::ProjectList {
        installation: symphony::machine::installation_id_in(&projects.machine).ok(),
        pin: symphony::machine::PIN,
        projects: served.iter().map(listed).collect(),
    })
    .into_response()
}

/// `project` as [`listing`] lists it: its refusal alone while refused, else what its hub holds,
/// the version of its main page's feed read with that page's counts and with its task asks, the
/// line the project list draws under it while its board is not well ([`Hub::standing`]), and
/// whether this server writes its board ([`Hub::writes`]).
///
/// [`Hub::standing`]: crate::live::Hub::standing
/// [`Hub::writes`]: crate::live::Hub::writes
fn listed(project: &Served) -> symphony_api::ListedProject {
    let id = &project.entry.id;
    let mut listed = symphony_api::ListedProject {
        id: id.clone(),
        name: name(project),
        refused: project.refused.clone(),
        unwell: None,
        version: None,
        waiting: None,
        next: None,
        asks: None,
        writes: None,
    };
    let Some(hub) = project
        .shared
        .api
        .get()
        .filter(|_| project.refused.is_none())
    else {
        return listed;
    };
    listed.unwell = hub.standing().unwell();
    listed.writes = hub.writes();
    // A writer's panic leaves at worst a page between two of its regions' updates, as the list's.
    let pages = hub.pages.read().held();
    if let Some(page) = pages.get("") {
        listed.version = Some(page.board.version);
        listed.waiting = Some(page.board.counts.waiting);
        listed.next = page
            .board
            .next
            .first()
            .map(|card| symphony_api::ListedCard {
                title: card.title.clone(),
                href: format!("/{id}{}", card.href()),
            });
    }
    listed.asks = hub.task_asks();
    drop(pages);
    listed
}

/// The name a project's pages and the list call it: its theme package's.
fn name(project: &Served) -> String {
    symphony::theme::Package::of(&symphony::project::Project::of(&project.entry.root)).name
}

/// `/`: always the project picker, in the app's look ([`crate::pages`]).
fn home(served: &[Served]) -> Response {
    crate::pages::html(
        StatusCode::OK,
        crate::pages::machine("Symphony", "", "", &list(served)),
    )
}

/// The project list: each registered project's counts, whether it has a board, and how its board
/// stands, from the hub's memory: the list reads no board.
fn list(served: &[Served]) -> String {
    // Each project is warmed as the registry is read, so its counts are the hub's at hand; one
    // still warming shows none yet.
    let rows: Vec<String> = served
        .iter()
        .map(|project| {
            let hub = project.shared.api.get();
            let counts = hub.and_then(|hub| {
                // A writer's panic leaves at worst a page between two of its regions' updates:
                // these counts are shown, and the project's next delta corrects them.
                let pages = hub.pages.read().held();
                pages.get("").map(|page| page.board.counts.clone())
            });
            let standing = hub.map(|hub| hub.standing());
            let blocked = (hub.and_then(|hub| hub.tasks_read().ok()))
                .map(|tasks| Blocked::of(&tasks.tasks))
                .unwrap_or_default();
            let boardless = hub.is_some_and(|hub| hub.boardless());
            list_row(
                project,
                counts.as_ref(),
                standing.as_ref(),
                blocked,
                boardless,
            )
        })
        .collect();
    format!(
        "<h1>Projects</h1><ul class=\"projects\">{}</ul>",
        rows.concat()
    )
}

/// One project in the list: the dot of how its board stands (`standing`, none while its hub is
/// not up or it is refused), its name and link, then its counts or why it is not served, that it
/// has no task board when it has none (`boardless`), said as plainly as its counts, its `blocked`
/// tasks, and, while its board is not well, the line saying why in the dot's colour.
fn list_row(
    project: &Served,
    counts: Option<&symphony_api::Counts>,
    standing: Option<&Standing>,
    blocked: Blocked,
    boardless: bool,
) -> String {
    use symphony::markup::escape;
    let name = name(project);
    let standing = standing.filter(|_| project.refused.is_none());
    let detail = match (&project.refused, counts) {
        (Some(why), _) => format!("<span class=\"refused\">{}</span>", escape(why)),
        (None, Some(counts)) => {
            // The status's count, which the board splits into In progress, Build and Test by the
            // machine's work: said as such, so it never reads as the board's In progress column.
            let tasks = match &counts.tasks {
                Some(tasks) => format!(
                    " · {} in progress (Build and Test included) · {} in review",
                    tasks.in_progress, tasks.in_review
                ),
                None if boardless => " · no task board".to_string(),
                None => String::new(),
            };
            let cards = if counts.waiting == 1 { "card" } else { "cards" };
            format!(
                "<span class=\"counts\">{} decision {cards} waiting on you{tasks}{}</span>",
                counts.waiting,
                blocked.said()
            )
        }
        (None, None) if boardless => "<span class=\"counts\">No task board</span>".to_string(),
        (None, None) => String::new(),
    };
    let dot = standing.map_or_else(String::new, |standing| {
        format!(
            "<span class=\"dot {}\" role=\"img\" aria-label=\"{said}\" title=\"{said}\"></span>",
            standing.signal.name(),
            said = standing.signal.said()
        )
    });
    let line = standing
        .filter(|standing| standing.signal != Signal::Green)
        .map_or_else(String::new, |standing| {
            format!(
                "<span class=\"health {}\" role=\"status\">{}</span>",
                standing.signal.name(),
                escape(&standing.line)
            )
        });
    format!(
        "<li>{dot}<a href=\"/{id}/\">{}</a>{detail}{line}</li>",
        escape(&name),
        id = escape(&project.entry.id)
    )
}

/// The blocked tasks of a board's last read, as the list counts them: the tasks (not their
/// subtasks, as the list's other counts) blocked on the wire, where a block that names nothing is
/// sent in the status it stopped in and so is not counted (nor one sent blocked for want of that
/// status), and those of them that wait on a decision of the owner's
/// (`symphony_board::waits_on::DECISION`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Blocked {
    tasks: usize,
    on_decision: usize,
}

impl Blocked {
    fn of(tasks: &[symphony_api::Task]) -> Blocked {
        use symphony_board::waits_on::{NO_REASON, NOBODY};
        // One the listing could not send in the stage it stopped in still names nothing.
        let named = |task: &&symphony_api::Task| {
            !(task.waits_on.as_ref()).is_some_and(|w| w.kind == NOBODY && w.why == NO_REASON)
        };
        let blocked: Vec<&symphony_api::Task> =
            symphony_api::tasks_in_status(tasks, symphony_api::TaskStatus::Blocked)
                .filter(named)
                .collect();
        Blocked {
            tasks: blocked.len(),
            on_decision: (blocked.iter())
                .filter(|task| {
                    (task.waits_on.as_ref())
                        .is_some_and(|waits| waits.kind == symphony_board::waits_on::DECISION)
                })
                .count(),
        }
    }

    /// The counts' last part, in bold: ` · 41 blocked, 9 of them waiting on your decision`, the
    /// decision's part only when one waits on it; nothing while none is blocked.
    fn said(self) -> String {
        if self.tasks == 0 {
            return String::new();
        }
        let decision = if self.on_decision == 0 {
            String::new()
        } else {
            format!(", {} of them waiting on your decision", self.on_decision)
        };
        format!(" · <strong>{} blocked{decision}</strong>", self.tasks)
    }
}

/// `/<id>/theme.css`: the project's theme tokens, revalidated by their ETag.
fn theme(project: &Served, headers: &HeaderMap) -> Response {
    let package =
        symphony::theme::Package::of(&symphony::project::Project::of(&project.entry.root));
    let etag = package.etag();
    let cached = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|tags| tags.split(','))
        .any(|tag| {
            let tag = tag.trim();
            tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
        });
    let mut response = if cached {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            package.css(),
        )
            .into_response()
    };
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// A registered project this server does not serve, asked for `path` (the project's route, or a
/// path of today): why, and what fixes it, as an API error under `/api/`, else as the project
/// list's page, the refusal its notice.
fn refusal(
    served: &[Served],
    project: &Served,
    why: &str,
    path: &str,
    headers: &HeaderMap,
) -> Response {
    use symphony::markup::escape;
    let id = &project.entry.id;
    let decoded = crate::api::decode_path(path);
    let (_, route) = crate::api::scope_path(decoded.as_deref().unwrap_or(path));
    if crate::pages::data(route, headers) {
        return crate::pages::json(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!("The Symphony does not serve {id} now: {why}"),
        );
    }
    let said = format!(
        "<p><strong>The Symphony does not serve {} now</strong>: {}</p>",
        escape(id),
        escape(why)
    );
    crate::pages::html(
        StatusCode::SERVICE_UNAVAILABLE,
        crate::pages::machine(
            "Not served · Symphony",
            "not served",
            &crate::pages::notice("alert", &said),
            &list(served),
        ),
    )
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "projects/maintained_tests.rs"]
mod maintained_tests;
