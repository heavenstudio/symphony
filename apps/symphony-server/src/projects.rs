//! The one server's projects: every project the machine folder's registry holds, each served under
//! `/<id>/` by a router of its own, so each keeps its own watcher, feed cursor, board snapshot and
//! branch scan (its own [`Shared`] and hub) and nothing of one is visible from another's pages.
//!
//! Before any project the server answers `/api/front` (itself, with the ids it serves), `/` (the
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
//! is read again whenever its file changes, and a changed root or id is served from new state.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Json, Response};
use symphony::entrypoint::below;
use symphony::machine::{Entry, PROJECTS, Registry};
use tower::ServiceExt;

use crate::front::Shared;
use crate::host::{Build, Lack};

mod retired;

pub(crate) use retired::{Stopped, close_saves};

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
    /// The projects the registry in `machine` holds, served beside `starter`'s, each warmed as
    /// it is read.
    pub(crate) fn new(machine: &Path, starter: &Arc<Shared>) -> Arc<Projects> {
        Projects::made(machine, starter, Warming::AsRead)
    }

    fn made(machine: &Path, starter: &Arc<Shared>, warming: Warming) -> Arc<Projects> {
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
            held: Mutex::default(),
        });
        projects.served();
        projects
    }

    /// The projects served now, the registry read again when its file changed.
    pub(crate) fn served(&self) -> Vec<Served> {
        // A panic while reading leaves the list before it whole: the new one is stored last, in
        // one assignment with the stamp it was read at.
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let stamp = fs::metadata(self.machine.join(PROJECTS)).ok().map(|meta| {
            (
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
            )
        });
        if held.stopped || held.read == Some(stamp) {
            return held.served.clone();
        }
        let registry = Registry::read(&self.machine).unwrap_or_else(|why| {
            eprintln!("· Symphony projects: {why}; serving the starter's project alone");
            Registry::default()
        });
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
                None => self.serve(entry),
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
                self.serve(Entry {
                    id: self.starter_id.clone(),
                    ..entry
                }),
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
        held.served.clone()
    }

    /// `entry` served: the starter's own state while the entry is the starter's checkout under the
    /// id it started with, a new one beside it for any other root or id.
    fn serve(&self, entry: Entry) -> Served {
        let shared = if entry.root == self.starter_root && entry.id == self.starter_id {
            self.starter.clone()
        } else {
            self.starter.beside(&entry.root, &format!("/{}", entry.id))
        };
        let refused = self.refused(&entry);
        if self.warming == Warming::AsRead
            && refused.is_none()
            && !Arc::ptr_eq(&shared, &self.starter)
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let warming = shared.clone();
            runtime.spawn(async move {
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
        let lack = self.build.lacks(entry)?;
        let fix = match lack {
            Lack::Plugins(_) => format!(
                "run `just symphony` in {}'s checkout, whose build holds them",
                entry.id
            ),
            Lack::Pin(_) | Lack::Schema(_) => format!(
                "{}, which started this Symphony, must bump its Symphony, then run `just \
                 symphony` there",
                self.starter_id
            ),
        };
        Some(format!(
            "this Symphony {}: {fix}",
            lack.said(&self.build, &entry.id)
        ))
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

/// The one server's routes: every request, before any project's.
pub(crate) fn router(projects: Arc<Projects>) -> Router {
    Router::new().fallback(dispatch).with_state(projects)
}

async fn dispatch(
    State(projects): State<Arc<Projects>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let ip = peer.ip();
    let starter = projects.starter.clone();
    let allowed = ip.is_loopback()
        || tokio::task::spawn_blocking(move || starter.gate.allows(ip))
            .await
            .unwrap_or(false);
    if !allowed {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: the Symphony answers only this machine and its tailnet\n",
        )
            .into_response();
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
        let named = tokio::task::spawn_blocking(move || starter.allows_host(&host))
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
    let Ok(served) = tokio::task::spawn_blocking(move || reading.served()).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if path == symphony_api::FRONT_PATH {
        return front(&projects, &served, request.headers());
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
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if !host.is_some_and(crate::front::names_loopback) {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: the Host header does not name this machine's loopback\n",
        )
            .into_response();
    }
    let mut status = projects.starter.status();
    status.projects = served
        .iter()
        .map(|served| served.entry.id.clone())
        .collect();
    Json(status).into_response()
}

/// `/`: always the project picker, in the app's look ([`crate::pages`]).
fn home(served: &[Served]) -> Response {
    crate::pages::html(
        StatusCode::OK,
        crate::pages::machine("Symphony", "", "", &list(served)),
    )
}

/// The project list: each registered project's counts, whether it has a board, and whether its
/// board is unwell, from the hub's memory: the list reads no board.
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
                let pages = hub.pages.read().unwrap_or_else(PoisonError::into_inner);
                pages.get("").map(|page| page.board.counts.clone())
            });
            let health = hub.map(|hub| hub.health_notice()).unwrap_or_default();
            let boardless = hub.is_some_and(|hub| hub.boardless());
            list_row(project, counts.as_ref(), &health, boardless)
        })
        .collect();
    format!(
        "<h1>Projects</h1><ul class=\"projects\">{}</ul>",
        rows.concat()
    )
}

/// One project in the list: its name and link, then its counts or why it is not served, that it
/// has no task board when it has none (`boardless`), said as plainly as its counts, and `health`,
/// what its hub says of its board while that is unwell.
fn list_row(
    project: &Served,
    counts: Option<&symphony_api::Counts>,
    health: &str,
    boardless: bool,
) -> String {
    use symphony::markup::escape;
    let name =
        symphony::theme::Package::of(&symphony::project::Project::of(&project.entry.root)).name;
    let detail = match (&project.refused, counts) {
        (Some(why), _) => format!("<span class=\"refused\">{}</span>", escape(why)),
        (None, Some(counts)) => {
            let tasks = match &counts.tasks {
                Some(tasks) => format!(
                    " · {} in progress · {} in review",
                    tasks.in_progress, tasks.in_review
                ),
                None if boardless => " · no task board".to_string(),
                None => String::new(),
            };
            format!(
                "<span class=\"counts\">{} waiting on you{tasks}</span>",
                counts.waiting
            )
        }
        (None, None) if boardless => "<span class=\"counts\">No task board</span>".to_string(),
        (None, None) => String::new(),
    };
    let health = if health.is_empty() {
        String::new()
    } else {
        format!(
            "<span class=\"health\" role=\"status\">⚠ {}</span>",
            escape(health)
        )
    };
    format!(
        "<li><a href=\"/{id}/\">{}</a>{detail}{health}</li>",
        escape(&name),
        id = escape(&project.entry.id)
    )
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
