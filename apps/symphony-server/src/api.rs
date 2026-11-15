//! Native reads, media and root SPA, including compatibility bookmark routes.

use std::collections::VecDeque;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::front::Shared;
use crate::live::Hub;

mod failure;
pub(crate) mod reports;
mod validated;
mod warm;

use failure::{task_failure, unavailable};
pub(crate) use warm::{start, warm};

/// Where a plugin's routes answer: `/x/<route>/<rest>`.
const PLUGIN_ROUTES: &str = "/x/";

pub(crate) const READS: &[&str] = &[
    "/api/board",
    "/api/queue",
    "/api/postponed",
    "/api/tasks",
    "/api/task-board",
    "/api/machine-work",
    "/api/task-page",
    "/api/task-brief",
    "/api/task-offer",
    symphony_api::TASK_GATES_PATH,
    symphony_api::TASK_TIMELINE_PATH,
    "/api/task-history",
    "/api/agent-page",
    symphony_api::AGENT_SCREEN_PATH,
    "/api/task-thread",
    "/api/agents",
    "/api/activity",
    "/api/pending",
    "/api/feed",
    "/queue",
    "/api/document",
    "/api/folder",
    "/api/section",
    "/api/history",
    "/api/versions",
    "/api/diff",
    "/api/asks",
    "/api/ask",
    "/api/health",
    symphony_api::REPORTS_PATH,
];

type Request = axum::http::Request<()>;

/// What the server reads answer for `request`. Every JSON read leaves here with its validator
/// (`validated`), so a read added later has one too.
pub(crate) async fn answer(shared: &Arc<Shared>, request: Request) -> Option<Response> {
    let asked: Vec<_> = request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .cloned()
        .collect();
    let response = read(shared, request).await?;
    Some(validated::validated(&asked, response).await)
}

async fn read(shared: &Arc<Shared>, request: Request) -> Option<Response> {
    let Some(decoded) = decode_path(request.uri().path()) else {
        return Some(error(StatusCode::BAD_REQUEST, "invalid URL path"));
    };
    let (uri_scope, path) = scope_path(&decoded);
    // Existing page scripts pass @tree/docs/X.md in path; SPA callers may scope the route itself.
    let rel = query(&request, "path").unwrap_or_default();
    let (query_scope, rel) = if let Some(scoped) = rel.strip_prefix('@') {
        scoped.split_once('/').unwrap_or(("", &rel))
    } else {
        ("", rel.as_str())
    };
    if !uri_scope.is_empty() && !query_scope.is_empty() && uri_scope != query_scope {
        return Some(error(
            StatusCode::BAD_REQUEST,
            "conflicting worktree scopes",
        ));
    }
    let scope = if query_scope.is_empty() {
        uri_scope
    } else {
        query_scope
    };
    let thumbnail = path.starts_with("/thumb/");
    let plugin_route = path.starts_with(PLUGIN_ROUTES);
    let json_queue = path == "/queue"
        && request
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.contains("application/json"));
    let alias = if json_queue { None } else { legacy_page(path) };
    let view = project_view(&shared.root);
    // An HTML document's route is the app's page, whatever headers ask for it; asked for with the
    // file's query, the address its frame loads, it is the file itself in its sandbox
    // (`symphony_api::file_address`, `media_files::html_document`).
    let html_file = view.is_html_route(path.trim_start_matches('/'))
        && request
            .uri()
            .query()
            .is_some_and(symphony_api::asks_for_file);
    let static_file =
        !html_file && (client_page(&view, path) || path.starts_with("/assets/") || alias.is_some());
    // Typed document links can point to any project source or data file. Keep those raw downloads
    // behind the same canonical containment, metadata exclusion and sandbox as embedded media.
    let media_file = crate::media_files::handles(path)
        || (!static_file && !path.starts_with("/api/") && !READS.contains(&path));
    if !static_file && !thumbnail && !plugin_route && !media_file && !READS.contains(&path) {
        return None;
    }
    if path == "/api/ask" && request.method() != Method::GET && request.method() != Method::HEAD {
        return None; // the write slice still owns POST /api/ask
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let check = shared.clone();
    let allowed = tokio::task::spawn_blocking(move || check.allows_host(&host))
        .await
        .unwrap_or(false);
    if !allowed {
        return Some(error(
            StatusCode::FORBIDDEN,
            "Forbidden: the Host header does not name this machine",
        ));
    }
    if path.starts_with("/api/")
        && request
            .headers()
            .get(header::ORIGIN)
            .is_some_and(|v| v == "null")
    {
        return Some(error(
            StatusCode::FORBIDDEN,
            "Forbidden: a sandboxed page has no access to the app API",
        ));
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return Some(error(
            StatusCode::METHOD_NOT_ALLOWED,
            "this route takes GET or HEAD",
        ));
    }
    if matches!(
        path,
        "/api/task-page"
            | "/api/task-brief"
            | "/api/task-history"
            | "/api/task-thread"
            | "/api/task-offer"
            | symphony_api::TASK_GATES_PATH
            | symphony_api::TASK_TIMELINE_PATH
    ) && let Err(why) = argument(&request, "slug")
    {
        return Some(error(StatusCode::BAD_REQUEST, why));
    }
    if path == symphony_api::AGENT_SCREEN_PATH
        && let Err(why) = argument(&request, "handle")
    {
        return Some(error(StatusCode::BAD_REQUEST, why));
    }
    if matches!(
        path,
        "/api/document"
            | "/api/folder"
            | "/api/section"
            | "/api/history"
            | "/api/versions"
            | "/api/diff"
            | "/api/asks"
    ) && let Err(why) = document_argument(&request)
    {
        return Some(error(StatusCode::BAD_REQUEST, why));
    }
    // A status no listing selects is refused before the board is read, slow or down.
    if path == "/api/task-board"
        && let Err(why) = task_board::status_argument(&request)
    {
        return Some(error(StatusCode::BAD_REQUEST, why));
    }
    if !scope.is_empty() && shared.api.get().and_then(|hub| hub.tree(scope)).is_none() {
        // A link to a branch's page outlives its worktree once the branch merges: a page says so.
        if !uri_scope.is_empty() && !crate::pages::data(path, request.headers()) {
            return Some(crate::pages::gone(
                &shared.root,
                &shared.base,
                scope,
                path,
                request.uri().query(),
            ));
        }
        return Some(error(StatusCode::NOT_FOUND, "no worktree for this branch"));
    }
    // The board's health is the hub's memory: it waits on no board read, the first one included,
    // so a board too slow to answer is still said.
    if path == "/api/health" {
        return Some(match shared.api.get() {
            Some(hub) => no_store(Json(hub.health.report(hub.now()))),
            None => error(
                StatusCode::SERVICE_UNAVAILABLE,
                "the Symphony server is starting",
            ),
        });
    }
    // The reports are the project's day files and its git alone, the board never read: a board
    // slow or down leaves them as they are.
    if path == symphony_api::REPORTS_PATH {
        let asked = reports::asked(
            query(&request, "grain").as_deref(),
            query(&request, "days").as_deref(),
        );
        let (grain, days) = match asked {
            Ok(asked) => asked,
            Err(why) => return Some(error(StatusCode::BAD_REQUEST, why)),
        };
        let Some(hub) = shared.api.get().cloned() else {
            return Some(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "the Symphony server is starting",
            ));
        };
        return Some(
            match tokio::task::spawn_blocking(move || reports::answer(&hub, grain, days)).await {
                Ok(page) => no_store(Json(page)),
                Err(e) => error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("the reports could not be read: {e}"),
                ),
            },
        );
    }
    // Uploads are the server's own, the same file for every branch's pages: a branch card's
    // picture names `/@tree/artifacts/uploads/…` and is read from this checkout's store.
    if let Some(file) = path.strip_prefix(symphony::uploads::ROUTE) {
        return Some(crate::media_files::upload(shared, file, &request).await);
    }
    if let Some(target) = alias {
        // A `.` or `..` part (`/history/..%2f..%2fbeta%2fx.js`) would point the redirect out of
        // the project, past any policy that bounds a page's loads by path: no such page.
        if target.split('/').any(|part| part == "." || part == "..") {
            return Some(StatusCode::NOT_FOUND.into_response());
        }
        // Encode decoded path components again: spaces, percent signs and question marks in a
        // document name must remain path text. Keep the caller's query exactly as supplied.
        let target = if scope.is_empty() {
            target
        } else {
            format!("/@{scope}{target}")
        };
        let mut location = encode_location(&target);
        if let Some(query) = request.uri().query() {
            location.push('?');
            location.push_str(query);
        }
        return Some(axum::response::Redirect::permanent(&location).into_response());
    }
    if static_file && !json_queue {
        return Some(static_response(shared, path, &request).await);
    }
    let hub = match warm(shared).await {
        Ok(hub) => hub,
        Err(e) => return Some(error(StatusCode::SERVICE_UNAVAILABLE, e)),
    };
    let check = hub.clone();
    let selected = scope.to_string();
    if let Err(e) = tokio::task::spawn_blocking(move || check.ensure_page(&selected))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
    {
        return Some(error(StatusCode::NOT_FOUND, e));
    }
    if let Some(problem) = hub.problem(scope) {
        return Some(error(StatusCode::SERVICE_UNAVAILABLE, problem));
    }
    if path == symphony_api::AGENT_SCREEN_PATH {
        let handle = query(&request, "handle").unwrap_or_default();
        return Some(
            match tokio::task::spawn_blocking(move || crate::session_go::screen(&hub, &handle))
                .await
            {
                Ok(Ok(screen)) => no_store(Json(screen)),
                Ok(Err((code, why))) => error(code, why),
                Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            },
        );
    }
    if media_file && !plugin_route && !thumbnail {
        let root = if scope.is_empty() {
            hub.root.clone()
        } else {
            hub.tree(scope).expect("scope was checked")
        };
        if html_file {
            return Some(
                crate::media_files::html_document(shared, &root, path, scope, &request).await,
            );
        }
        return Some(crate::media_files::answer(shared, &root, path, &request).await);
    }
    if plugin_route {
        let root = if scope.is_empty() {
            hub.root.clone()
        } else {
            hub.tree(scope).expect("scope was checked")
        };
        return Some(plugin_response(shared, &root, path, &request).await);
    }
    if thumbnail
        || matches!(
            path,
            "/api/document"
                | "/api/folder"
                | "/api/section"
                | "/api/history"
                | "/api/versions"
                | "/api/diff"
                | "/api/asks"
                | "/api/ask"
        )
    {
        let root = if scope.is_empty() {
            hub.root.clone()
        } else {
            hub.tree(scope).expect("scope was checked")
        };
        let hub = hub.clone();
        let shared = shared.clone();
        let scope = scope.to_string();
        let path = path.to_string();
        let rel = rel.to_string();
        let response = tokio::task::spawn_blocking(move || {
            document_response(&shared, &hub, &root, &scope, &path, &rel, &request)
        })
        .await;
        return Some(match response {
            Ok(response) => response,
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        });
    }
    let response = match path {
        "/api/feed" => feed(hub, scope, &request),
        "/api/tasks" | "/api/task-board" => {
            let search = query(&request, "q")
                .unwrap_or_default()
                .trim()
                .to_lowercase();
            let scores = if path == "/api/task-board" && !search.is_empty() {
                let root = hub.root.clone();
                let needle = search.clone();
                match tokio::task::spawn_blocking(move || {
                    let store = symphony::taskview::connect(&root)?;
                    store.search(&needle).map_err(|error| error.to_string())
                })
                .await
                {
                    Ok(Ok(hits)) => hits
                        .into_iter()
                        .map(|hit| (hit.slug, hit.score))
                        .collect::<std::collections::BTreeMap<_, _>>(),
                    Ok(Err(why)) => {
                        let (status, said) = unavailable(&why, "");
                        return Some(error(status, said));
                    }
                    Err(why) => {
                        return Some(error(StatusCode::INTERNAL_SERVER_ERROR, why.to_string()));
                    }
                }
            } else {
                std::collections::BTreeMap::new()
            };
            match hub.task_snapshot() {
                Ok(tasks) => {
                    if path == "/api/task-board" {
                        task_board::listing(&hub, &request, tasks, &search, &scores)
                    } else {
                        no_store(Json(tasks))
                    }
                }
                Err(e) => {
                    let (status, said) = unavailable(&e, "the task board cannot be reached: ");
                    error(status, said)
                }
            }
        }
        // The machine's work alone, for a page that reads the board's listings without it
        // (`/api/task-board?work=0`): its recorders' heartbeats move it every sample.
        "/api/machine-work" => no_store(Json(hub.machine_snapshot())),
        "/api/agents"
        | "/api/agent-page"
        | "/api/task-thread"
        | "/api/task-page"
        | "/api/task-brief"
        | "/api/task-offer"
        | symphony_api::TASK_GATES_PATH
        | symphony_api::TASK_TIMELINE_PATH
        | "/api/task-history" => {
            let root = if scope.is_empty() {
                hub.root.clone()
            } else {
                hub.tree(scope).unwrap_or_else(|| hub.root.clone())
            };
            let slug = query(&request, "slug").unwrap_or_default();
            let agents = path == "/api/agents" || path == "/api/agent-page";
            let agent_page = path == "/api/agent-page";
            let before = query(&request, "before").and_then(|id| id.parse().ok());
            let after = query(&request, "after").and_then(|id| id.parse().ok());
            let offset = query(&request, "offset")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            let lost = query(&request, "health").as_deref() == Some("lost");
            let agent_after = query(&request, "cursor");
            let detail = path == "/api/task-page";
            let brief = path == "/api/task-brief";
            let query_from = query(&request, "from").and_then(|n| n.parse::<usize>().ok());
            let offer = path == "/api/task-offer";
            // Whether a pane is focused or its screen shown depends on where the page was asked.
            let local = (agent_page || detail) && crate::session_go::local(shared, &request).await;
            let gates = path == symphony_api::TASK_GATES_PATH;
            let timeline = path == symphony_api::TASK_TIMELINE_PATH;
            match tokio::task::spawn_blocking(move || {
                if agents {
                    // The agents and their work as the hub's board tick last read them: while the
                    // board cannot be read, neither can they, as with its tasks.
                    let board = hub.tasks_read().map_err(|e| unavailable(&e, ""))?;
                    let mut view = hub.agent_snapshot().map_err(|e| unavailable(&e, ""))?;
                    if agent_page {
                        view.agents.retain(|a| {
                            if lost {
                                a.health == "lost"
                            } else {
                                a.health != "lost" && a.health != "left"
                            }
                        });
                        let total = view.agents.len();
                        view.agents.sort_by(|a, b| a.handle.cmp(&b.handle));
                        if let Some(after) = &agent_after {
                            view.agents.retain(|a| a.handle > *after);
                        }
                        view.agents = view.agents.into_iter().skip(offset).take(20).collect();
                        let next = view
                            .agents
                            .last()
                            .filter(|_| view.agents.len() == 20)
                            .map(|a| a.handle.clone());
                        let tasks = agent_page::tasks(
                            &board.tasks,
                            view.agents.iter().map(|a| a.handle.as_str()),
                        );
                        Ok(no_store(Json(symphony_api::AgentPage {
                            view,
                            tasks,
                            total,
                            next,
                            local,
                        })))
                    } else {
                        Ok(no_store(Json(view)))
                    }
                } else {
                    let store = hub.reader().map_err(|e| unavailable(&e, ""))?;
                    let read = if offer {
                        store.get(&slug).map_err(task_failure).and_then(|_| {
                            let plugins = symphony::plugins::Plugins::of(&root);
                            symphony::taskview::owner_offer(&store, &plugins, &slug, &root)
                                .map(|offer| no_store(Json(offer)))
                                .map_err(|e| unavailable(&e, ""))
                        })
                    } else if gates {
                        // A plugin that errs or is slow is an unknown gate, never a failed answer:
                        // only the board's own reads fail it.
                        store.get(&slug).map_err(task_failure).and_then(|record| {
                            let pending = hub.pending();
                            symphony::gates::answer(&store, &record, &root, &pending, &hub.gates)
                                .map(|gates| no_store(Json(gates)))
                                .map_err(|e| unavailable(&e, ""))
                        })
                    } else if timeline {
                        // The task's own thread and run records, and its gates as the gates
                        // answer gives them: gates that cannot be read leave their lanes out.
                        store.get(&slug).map_err(task_failure).and_then(|record| {
                            let pending = hub.pending();
                            symphony::timeline::answer(&store, &record, &root, &pending, &hub.gates)
                                .map(|timeline| {
                                    // Tagged without its clock, so a minute's read is a 304.
                                    let mut value = serde_json::to_value(&timeline)
                                        .expect("a timeline serializes");
                                    let tag = validated::without_now(&mut value);
                                    let mut response = no_store(Json(value));
                                    response.extensions_mut().insert(validated::Content(tag));
                                    response
                                })
                                .map_err(|e| unavailable(&e, ""))
                        })
                    } else if brief {
                        store.get(&slug).map_err(task_failure).map(|record| {
                            let from = query_from.unwrap_or(0);
                            no_store(Json(symphony::taskview::brief(&record, from, &root)))
                        })
                    } else if detail {
                        store.get(&slug).map_err(task_failure).and_then(|record| {
                            symphony::taskview::page(&store, &record, &root)
                                .map(|page| {
                                    let mut page = hub.observed_page(page);
                                    page.local = local;
                                    // The room its gates held last time, from what is kept: the
                                    // page never waits on them.
                                    page.gates = hub.gates.seen(&root, &record.slug);
                                    no_store(Json(page))
                                })
                                .map_err(|e| unavailable(&e, ""))
                        })
                    } else {
                        symphony::taskview::history(&store, &slug, before, after, &root)
                            .map(|history| no_store(Json(history)))
                            .map_err(task_failure)
                    };
                    // The board's server may have gone, or moved: the next read opens it anew.
                    if read.as_ref().is_err_and(|(status, _)| {
                        [StatusCode::SERVICE_UNAVAILABLE, StatusCode::GATEWAY_TIMEOUT]
                            .contains(status)
                    }) {
                        hub.reader_failed(&store);
                    }
                    read
                }
            })
            .await
            {
                Ok(Ok(response)) => response,
                Ok(Err((code, e))) => error(code, e),
                Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        _ => {
            let pages = hub.pages.read().unwrap_or_else(|e| e.into_inner());
            match pages.get(scope) {
                Some(page) => match path {
                    "/api/board" => no_store(Json(&page.board)),
                    "/api/queue" | "/queue" => no_store(Json(&page.queue)),
                    "/api/postponed" => no_store(Json(
                        page.queue
                            .waiting
                            .iter()
                            .filter(|card| card.postponed)
                            .filter(|card| {
                                !card.doc.starts_with('@')
                                    || card
                                        .doc
                                        .strip_prefix('@')
                                        .and_then(|p| p.split_once('/'))
                                        .is_some_and(|(card_scope, _)| card_scope == scope)
                            })
                            .map(|card| &card.slug)
                            .collect::<Vec<_>>(),
                    )),
                    "/api/pending" => no_store(Json(&page.pending)),
                    "/api/activity" => no_store(Json(&page.activity)),
                    _ => error(StatusCode::NOT_FOUND, "no such API route"),
                },
                None => error(StatusCode::NOT_FOUND, "this worktree was removed"),
            }
        }
    };
    Some(response)
}

fn document_response(
    shared: &Shared,
    hub: &Hub,
    root: &Path,
    scope: &str,
    path: &str,
    rel: &str,
    request: &Request,
) -> Response {
    if path.starts_with("/thumb/") {
        return thumbnail(shared, root, path);
    }
    if path == "/api/ask"
        && query(request, "id")
            .and_then(|s| s.parse::<i64>().ok())
            .is_none()
    {
        return error(StatusCode::BAD_REQUEST, "id must be a number");
    }
    let project = symphony::project::Project::of(root);
    if !matches!(path, "/api/folder" | "/api/ask")
        && (symphony::asks::check_path(rel).is_err()
            || project
                .serving(rel)
                .is_none_or(|(entry, inside)| !entry.page(inside)))
    {
        return error(StatusCode::BAD_REQUEST, project.not_a_document());
    }
    if root
        .join(symphony::project::Project::of(root).file(rel))
        .canonicalize()
        .is_ok_and(|target| shared.is_metadata(&target))
    {
        return error(StatusCode::NOT_FOUND, "no such document");
    }
    let render = symphony::render::Render::new(root);
    if matches!(path, "/api/asks" | "/api/ask") {
        // The hub keeps the board's chat recipient from its board reads; a hub that has not read
        // the board yet leaves this read to connect for it.
        let recipient = hub.chat_recipient().unwrap_or_else(|| {
            symphony::taskview::connect(root)
                .ok()
                .and_then(|board| board.chat_recipient().ok().flatten())
        });
        return match ask_response(&render, rel, scope, request, path == "/api/asks", recipient) {
            Ok(value) => no_store(Json(value)),
            Err((status, why)) => error(status, why),
        };
    }
    let result = match path {
        // What a page reads ahead of a document it links to: its first part, within the cap, or
        // nothing when its head alone is over it. Read ahead, the document is not followed.
        "/api/document" if query(request, "view").as_deref() == Some("first") => {
            match hub.document_ahead(scope, rel).map(symphony::docview::first) {
                Ok(Some(first)) => Ok(json!(first)),
                Ok(None) => {
                    return error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "the document's head alone is over the read ahead's cap",
                    );
                }
                Err(why) => Err(why),
            }
        }
        "/api/document" => hub.document(scope, rel).map(|doc| {
            if query(request, "view").as_deref() == Some("outline") {
                json!(symphony::docview::outline(doc))
            } else {
                json!(doc)
            }
        }),
        "/api/folder" => hub.folder(scope, rel).map(|mut folder| {
            if query(request, "view").as_deref() == Some("outline") {
                folder.intro = folder
                    .intro
                    .map(|doc| symphony::docview::outline(doc).document);
            }
            json!(folder)
        }),
        "/api/section" if query(request, "keys").is_some() => {
            let keys = match serde_json::from_str::<Vec<String>>(
                &query(request, "keys").unwrap_or_default(),
            ) {
                Ok(keys) => keys,
                Err(_) => {
                    return error(
                        StatusCode::BAD_REQUEST,
                        "keys must be a JSON list of section keys",
                    );
                }
            };
            hub.document_sections(scope, rel, &keys)
                .map(|parts| json!(parts))
        }
        "/api/section" => {
            let anchor = query(request, "anchor").unwrap_or_default();
            if query(request, "format").as_deref() == Some("blocks") {
                symphony::docview::section(&render, rel, &anchor, scope)
                    .map(|fragment| json!(fragment))
            } else {
                symphony::docview::legacy_section(&render, rel, &anchor, scope).map(|fragment| {
                    let html = if query(request, "format").as_deref() == Some("fragments") {
                        fragment.html.clone()
                    } else {
                        symphony::docview::legacy_html(&fragment)
                    };
                    json!({"html": html, "media": fragment.media})
                })
            }
        }
        "/api/history" => {
            symphony::docview::history(&render, rel, scope).map(|threads| json!(threads))
        }
        "/api/versions" => symphony::docview::target(root, rel, true)
            .map(|_| json!({"versions":symphony::versions::versions(root, rel, 0)})),
        "/api/diff" => symphony::docview::target(root, rel, true)
            .and_then(|_| {
                symphony::versions::diff(root, rel, &query(request, "sha").unwrap_or_default())
                    .ok_or_else(|| "no such document version".into())
            })
            .map(|diff| json!(diff)),
        _ => Err("no such route".into()),
    };
    match result {
        // A document, its sections and a folder's intro carry the feed's cursor, which is not their
        // content: their tag leaves it out, and a header names it, on the `304` too.
        Ok(mut value) if matches!(path, "/api/document" | "/api/section" | "/api/folder") => {
            let tagged = validated::without_cursor(&mut value);
            let mut response =
                no_store(([(header::CONTENT_TYPE, "application/json")], tagged.body));
            response
                .extensions_mut()
                .insert(validated::Content(tagged.tag));
            if let Some(cursor) = tagged.cursor {
                response.headers_mut().insert(
                    symphony_api::VERSION_HEADER,
                    header::HeaderValue::from(cursor),
                );
            }
            response
        }
        Ok(value) => no_store(Json(value)),
        Err(e) => error(StatusCode::NOT_FOUND, e),
    }
}

fn ask_response(
    render: &symphony::render::Render,
    rel: &str,
    scope: &str,
    request: &Request,
    list: bool,
    recipient: Option<String>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    if list {
        symphony::docview::target(render.root(), rel, true)
            .map_err(|why| (StatusCode::NOT_FOUND, why))?;
    }
    let named = std::env::var("ASKS_DB").ok();
    let db = symphony::asks::default_path(render.root(), named.as_deref());
    let store = symphony::asks::AskStore::open(&db).map_err(ask_error)?;
    let threads = if list {
        store.for_doc(rel)
    } else {
        let id = query(request, "id")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "id must be a number".into()))?;
        store.thread(id).map(|thread| vec![thread])
    }
    .map_err(ask_error)?;
    let listening = recipient.is_some();
    let typed = query(request, "format").as_deref() == Some("blocks");
    let rendered = threads
        .into_iter()
        .map(|thread| {
            let mut ask = symphony_api::Ask::from(thread);
            for message in &mut ask.messages {
                // An answer is an agent's Markdown; the owner's words stay literal but for the
                // lines naming the files they attached.
                let mut fragment =
                    symphony_api::Fragment::from(match (message.kind == "answer", typed) {
                        (true, true) => render.typed_reply(&message.text, &ask.path),
                        (true, false) => render.reply(&message.text, &ask.path),
                        (false, true) => render.typed_owner(&message.text, &ask.path),
                        (false, false) => render.owner(&message.text, &ask.path),
                    });
                symphony::docview::prepare_fragment(
                    &mut fragment.html,
                    &mut fragment.media,
                    render.root(),
                    &ask.path,
                    scope,
                );
                if typed {
                    let path = if scope.is_empty() {
                        ask.path.clone()
                    } else {
                        format!("@{scope}/{}", ask.path)
                    };
                    message.blocks =
                        symphony::blocks::parse(&fragment.html, &fragment.media, &path);
                    message.html.clear();
                    message.media.clear();
                } else {
                    message.html = if query(request, "format").as_deref() == Some("fragments") {
                        fragment.html.clone()
                    } else {
                        symphony::docview::legacy_html(&fragment)
                    };
                    message.media = fragment.media;
                }
            }
            ask
        })
        .collect::<Vec<_>>();
    Ok(if list {
        json!({"asks":rendered, "listening":listening, "recipient":recipient})
    } else {
        json!({"ask":rendered[0], "listening":listening, "recipient":recipient})
    })
}

fn ask_error(why: symphony::asks::AskError) -> (StatusCode, String) {
    let status = match why {
        symphony::asks::AskError::Refused(_) => StatusCode::NOT_FOUND,
        symphony::asks::AskError::Store(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, why.to_string())
}

fn thumbnail(shared: &Shared, root: &Path, path: &str) -> Response {
    let parts = path
        .trim_start_matches("/thumb/")
        .splitn(3, '/')
        .collect::<Vec<_>>();
    let Some(width) = parts.first().and_then(|s| s.parse::<u32>().ok()) else {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    };
    let Some(rel) = parts.get(2).and_then(|s| s.strip_suffix(".jpg")) else {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    };
    if Path::new(rel)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    }
    // A picture beside a document is named by its route, as the document is (`docs/…`).
    let file = symphony::project::Project::of(root).file(rel);
    if root
        .join(&file)
        .canonicalize()
        .is_ok_and(|target| shared.is_metadata(&target))
    {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    }
    let made = symphony::images::thumbnail(
        root,
        width,
        parts[1],
        rel,
        &symphony::images::sips_convert,
        symphony::images::sips_available(),
    );
    let (file, mime, current) = if let Some((file, current)) = made {
        (file, "image/jpeg", current)
    } else {
        let Ok(file) = root.join(&file).canonicalize() else {
            return error(StatusCode::NOT_FOUND, "no such thumbnail");
        };
        if !root.canonicalize().is_ok_and(|root| file.starts_with(root))
            || file
                .extension()
                .is_none_or(|s| !s.eq_ignore_ascii_case("png"))
        {
            return error(StatusCode::NOT_FOUND, "no such thumbnail");
        }
        (file, "image/png", false)
    };
    let Ok(file) = file.canonicalize() else {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    };
    if shared.is_metadata(&file) {
        return error(StatusCode::NOT_FOUND, "no such thumbnail");
    }
    match std::fs::read(file) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    if current {
                        "public, max-age=31536000, immutable"
                    } else {
                        "no-cache"
                    },
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => error(StatusCode::NOT_FOUND, "no such thumbnail"),
    }
}

/// `/x/<route>/<rest>`: what the project's plugin route of that name answers. A file it names is
/// served as every project file is, only from inside the checkout and outside its metadata.
async fn plugin_response(shared: &Shared, root: &Path, path: &str, request: &Request) -> Response {
    let Some((route, rest)) = path
        .strip_prefix(PLUGIN_ROUTES)
        .and_then(|routed| routed.split_once('/'))
    else {
        return error(StatusCode::NOT_FOUND, "no such route");
    };
    let (checkout, route, rest) = (root.to_path_buf(), route.to_string(), rest.to_string());
    let reply = tokio::task::spawn_blocking(move || {
        symphony::plugins::Plugins::of(&checkout)
            .route(&route)?
            .serve(&checkout, &rest)
    })
    .await
    .ok()
    .flatten();
    match reply {
        None => error(StatusCode::NOT_FOUND, "no such file"),
        Some(symphony::plugins::Reply::Json(body)) => {
            no_store(([(header::CONTENT_TYPE, "application/json")], body))
        }
        Some(symphony::plugins::Reply::File(file)) => {
            let Ok(base) = tokio::fs::canonicalize(root).await else {
                return error(StatusCode::NOT_FOUND, "no such file");
            };
            crate::media_files::contained(shared, &base, &base, &file, request).await
        }
    }
}

fn no_store(value: impl IntoResponse) -> Response {
    let mut response = value.into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

/// GET and HEAD use weak comparison, including a list of validators or the wildcard.
pub(crate) fn matches_etag(request: &Request, etag: &str) -> bool {
    names_etag(request.headers().get_all(header::IF_NONE_MATCH), etag)
}

/// Whether one of the `If-None-Match` values names `etag` (quoted, without `W/`), as
/// [`matches_etag`] reads them.
fn names_etag<'a>(values: impl IntoIterator<Item = &'a header::HeaderValue>, etag: &str) -> bool {
    values.into_iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value.split(',').any(|tag| {
                let tag = tag.trim();
                tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
            })
        })
    })
}

fn error(code: StatusCode, why: impl AsRef<str>) -> Response {
    no_store((code, Json(json!({"error":why.as_ref()}))))
}

pub(crate) fn scope_path(path: &str) -> (&str, &str) {
    path.strip_prefix("/@")
        .and_then(|p| p.find('/').map(|at| (&p[..at], &p[at..])))
        .unwrap_or(("", path))
}

fn query(request: &Request, key: &str) -> Option<String> {
    query_in(request.uri(), key)
}

/// The query parameter `key`, which a route needs: refused when it is missing or blank, or when it
/// does not decode (its percent-encoding is not UTF-8), saying which.
fn argument(request: &Request, key: &str) -> Result<String, String> {
    match query(request, key) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(format!("{key} is required")),
        None if raw_query(request.uri(), key).is_some() => Err(format!(
            "{key} is not valid: its percent-encoding is not UTF-8"
        )),
        None => Err(format!("{key} is required")),
    }
}

/// A document read's `path`, which may be empty (the folder of an entrypoint at the project's
/// root) but must be given, decode, and hold no control character, which no document's name does.
fn document_argument(request: &Request) -> Result<(), String> {
    match query(request, "path") {
        Some(path) if path.chars().any(char::is_control) => {
            Err("path is not valid: it holds a control character".into())
        }
        Some(_) => Ok(()),
        None if raw_query(request.uri(), "path").is_some() => {
            Err("path is not valid: its percent-encoding is not UTF-8".into())
        }
        None => Err("path is required".into()),
    }
}

/// The query parameter `key` as sent, undecoded.
fn raw_query<'a>(uri: &'a axum::http::Uri, key: &str) -> Option<&'a str> {
    uri.query()?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == key).then_some(value)
    })
}

/// A query parameter, decoded (`+` is a space).
pub(crate) fn query_in(uri: &axum::http::Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == key).then(|| decode(value)).flatten()
    })
}

fn decode(value: &str) -> Option<String> {
    decode_component(value, true)
}

pub(crate) fn decode_path(value: &str) -> Option<String> {
    decode_component(value, false)
}

fn decode_component(value: &str, plus_space: bool) -> Option<String> {
    let mut out = Vec::new();
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        out.push(match b {
            b'+' if plus_space => b' ',
            b'%' => {
                let a = char::from(bytes.next()?).to_digit(16)?;
                let b = char::from(bytes.next()?).to_digit(16)?;
                u8::try_from(a * 16 + b).ok()?
            }
            other => other,
        });
    }
    String::from_utf8(out).ok()
}

fn events_since(
    hub: &Hub,
    scope: &str,
    since: u64,
) -> Result<VecDeque<crate::live::Event>, String> {
    if let Some(problem) = hub.problem(scope) {
        return Err(problem);
    }
    let pages = hub.pages.read().unwrap_or_else(|e| e.into_inner());
    let page = pages.get(scope).ok_or("this worktree was removed")?;
    if since > page.board.version
        || page
            .history
            .front()
            .is_some_and(|e| since.saturating_add(1) < e.version)
        || (since < page.board.version && page.history.is_empty())
    {
        return Err("the feed snapshot expired; fetch /api/board again".into());
    }
    Ok(page
        .history
        .iter()
        .filter(|e| e.version > since)
        .cloned()
        .collect())
}

fn feed(hub: Arc<Hub>, scope: &str, request: &Request) -> Response {
    use futures_util::StreamExt;
    let Some(since) = query(request, "since").and_then(|v| v.parse::<u64>().ok()) else {
        return error(
            StatusCode::BAD_REQUEST,
            "the feed needs ?since=VERSION from /api/board",
        );
    };
    let receiver = hub.changed.subscribe(); // before reading the history: no subscribe/read gap
    let pending = match events_since(&hub, scope, since) {
        Ok(events) => events,
        Err(e) => return error(StatusCode::CONFLICT, e),
    };
    let scope = scope.to_string();
    let document_filter = {
        let path = query(request, "document").unwrap_or_default();
        let keys = query(request, "sections")
            .and_then(|keys| serde_json::from_str::<Vec<String>>(&keys).ok())
            .unwrap_or_default();
        (path, keys)
    };
    let ping_after = Duration::from_millis(u64::from(symphony_api::PING_MAX_MS));
    let mut draining = hub.draining.subscribe();
    let notices = hub.server_notice.subscribe();
    let notices =
        futures_util::stream::unfold((notices, true), |(mut notices, first)| async move {
            if !first && notices.changed().await.is_err() {
                return None;
            }
            let message = notices.borrow_and_update().clone();
            Some((
                Ok::<_, std::io::Error>(format!(
                    "event: {}\ndata: {}\n\n",
                    symphony_api::SERVER_EVENT,
                    serde_json::to_string(&message).expect("a notice serializes")
                )),
                (notices, false),
            ))
        });
    let stream = futures_util::stream::unfold(
        (
            hub,
            scope,
            since,
            receiver,
            pending,
            tokio::time::Instant::now() + ping_after,
            document_filter,
        ),
        move |(hub, scope, mut since, mut receiver, mut pending, mut deadline, document_filter)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    since = event.version;
                    let item: Result<String, std::io::Error> = Ok(format!(
                        "event: {}\nid: {since}\ndata: {}\n\n",
                        symphony_api::FEED_EVENT,
                        filtered_document_event(&event.json, &document_filter)
                    ));
                    deadline = tokio::time::Instant::now() + ping_after;
                    return Some((
                        item,
                        (
                            hub,
                            scope,
                            since,
                            receiver,
                            pending,
                            deadline,
                            document_filter,
                        ),
                    ));
                }
                match tokio::time::timeout_at(deadline, receiver.changed()).await {
                    Ok(Ok(())) => match events_since(&hub, &scope, since) {
                        Ok(events) => pending = events,
                        Err(_) => return None,
                    },
                    Ok(Err(_)) => return None,
                    Err(_) => {
                        return Some((
                            // Event::data("") writes no data field, which a browser never
                            // dispatches. A blank data line is required for the named ping.
                            Ok(format!("event: {}\ndata:\n\n", symphony_api::PING_EVENT)),
                            (
                                hub,
                                scope,
                                since,
                                receiver,
                                pending,
                                tokio::time::Instant::now() + ping_after,
                                document_filter,
                            ),
                        ));
                    }
                }
            }
        },
    );
    let mut response = no_store((
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(
            // A terminal marker from the main subscription closes the whole response. Notices
            // must not keep an expired or removed subscription open without its normal pings.
            futures_util::stream::select(
                stream
                    .map(Some)
                    .chain(futures_util::stream::once(std::future::ready(None))),
                notices.map(Some),
            )
            .take_while(|event| std::future::ready(event.is_some()))
            .filter_map(std::future::ready)
            .take_until(async move {
                let _ = draining.wait_for(|draining| *draining).await;
            }),
        ),
    ));
    response
        .headers_mut()
        .insert("x-accel-buffering", header::HeaderValue::from_static("no"));
    response
}

/// A document subscription always follows metadata and positions, but receives bodies and
/// threads only for sections its viewport has requested. Other streams receive no documents.
fn filtered_document_event(json: &str, filter: &(String, Vec<String>)) -> String {
    let (path, keys) = filter;
    let Ok(mut event) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.into();
    };
    if let Some(changes) = event["changes"].as_array_mut() {
        changes.retain_mut(|change| {
            if change["region"] != "document" {
                return true;
            }
            if change["data"]["path"] != *path {
                return false;
            }
            let data = &mut change["data"];
            let mut anchors = serde_json::Map::new();
            if let Some(sections) = data["sections"].as_array_mut() {
                for section in sections {
                    let key = section["key"].as_str().unwrap_or_default().to_string();
                    let ids = if section["comparison"]["kind"] == "removed" {
                        Vec::new()
                    } else {
                        typed_anchors(&section["blocks"])
                    };
                    anchors.insert(key.clone(), json!(ids));
                    if !keys.contains(&key) {
                        section["blocks"] = json!([]);
                        if section["comparison"].is_object() {
                            section["comparison"]["before"] = json!([]);
                        }
                    }
                }
            }
            data["anchors"] = anchors.into();
            if let Some(threads) = data["threads"].as_object_mut() {
                threads.retain(|slug, _| keys.contains(&format!("card:{slug}")));
            }
            if let Some(cards) = data["cards"].as_array_mut() {
                for card in cards {
                    if !keys.contains(&format!(
                        "card:{}",
                        card["key"].as_str().unwrap_or_default()
                    )) && let Some(value) = card["value"].as_object_mut()
                    {
                        let summary = symphony::docview::card_line(
                            value
                                .get("text")
                                .and_then(|text| text.as_str())
                                .unwrap_or_default(),
                        );
                        value.insert("text".into(), json!(summary));
                    }
                }
            }
            true
        });
    }
    serde_json::to_string(&event).unwrap_or_else(|_| json.into())
}

fn typed_anchors(value: &serde_json::Value) -> Vec<String> {
    fn collect(value: &serde_json::Value, ids: &mut Vec<String>) {
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, ids);
                }
            }
            serde_json::Value::Object(object) => {
                let id = match object.get("type").and_then(|kind| kind.as_str()) {
                    Some("heading") => object.get("anchor"),
                    Some("card") => object.get("slug"),
                    _ => None,
                }
                .and_then(|id| id.as_str());
                if let Some(id) = id.filter(|id| !id.is_empty()) {
                    ids.push(id.into());
                }
                if let Some(id) = object
                    .get("presentation")
                    .and_then(|p| p.get("id"))
                    .and_then(|id| id.as_str())
                    .filter(|id| !id.is_empty())
                {
                    ids.push(id.into());
                }
                for child in object.values() {
                    collect(child, ids);
                }
            }
            _ => {}
        }
    }
    let mut ids = Vec::new();
    collect(value, &mut ids);
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) use client::compressed_per_request;
use client::static_response;
mod client;

/// The project at `root` as the client reads it: its plugins' navigation, its entrypoints and its
/// review guide.
pub(crate) fn project_view(root: &Path) -> symphony_api::ProjectView {
    symphony::project::Project::of(root).view_at(root)
}

/// Whether `path` is a page the client draws: its own, one of the project's navigation pages, or
/// a document or folder page of one of its entrypoints.
pub(crate) fn client_page(project: &symphony_api::ProjectView, path: &str) -> bool {
    let page = path.trim_end_matches('/');
    // `/task` with no slug is the board, as the shell's routes read it.
    matches!(
        page,
        "" | "/tasks" | "/task" | "/agents" | "/queue" | "/reports"
    ) || project
        .nav
        .iter()
        .any(|nav| nav.href.trim_end_matches('/') == page)
        || path.starts_with("/task/")
        || project.is_document_route(path.trim_start_matches('/'))
}

/// Compatibility shell prefixes and version pages redirect to the document view.
fn legacy_page(path: &str) -> Option<String> {
    for prefix in ["/app", "/doc"] {
        if path == prefix {
            return Some("/".into());
        }
        if let Some(rest) = path.strip_prefix(&format!("{prefix}/")) {
            return Some(format!("/{}", rest.trim_start_matches('/')));
        }
    }
    if let Some(rest) = path.strip_prefix("/history/") {
        return Some(format!("/{}", rest.trim_start_matches('/')));
    }
    path.strip_prefix("/diff/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, document)| format!("/{}", document.trim_start_matches('/')))
}

/// `path` as a URL path: every byte but letters, digits and `/-._~@` percent-encoded.
pub(crate) fn encode_location(path: &str) -> String {
    path.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/-._~@".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

mod agent_page;

#[cfg(test)]
mod tests;

mod task_board;
