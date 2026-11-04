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

/// Where a plugin's routes answer: `/x/<route>/<rest>`.
const PLUGIN_ROUTES: &str = "/x/";

const READS: &[&str] = &[
    "/api/board",
    "/api/queue",
    "/api/postponed",
    "/api/tasks",
    "/api/task-board",
    "/api/task-page",
    "/api/task-brief",
    "/api/task-offer",
    "/api/task-history",
    "/api/agent-page",
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
];

type Request = axum::http::Request<()>;

pub(crate) async fn warm(shared: &Arc<Shared>) -> Result<Arc<Hub>, String> {
    if !shared.stopping()
        && let Some(hub) = shared.api.get()
    {
        return Ok(hub.clone());
    }
    let shared = shared.clone();
    tokio::task::spawn_blocking(move || {
        let _held = shared.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if shared.stopping() {
            return Err("the Symphony server is stopping".into());
        }
        if let Some(hub) = shared.api.get() {
            return Ok(hub.clone());
        }
        // Only --home servers hold the shared lease. A preview must never send the owner's
        // journal to its board, even when that board is a copied scratch database.
        let hub = Hub::start(&shared.root, &shared.ui_dist, shared.installed_ui)?;
        assert!(
            shared.api.set(hub.clone()).is_ok(),
            "hub startup is serialized"
        );
        Ok(hub)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn answer(shared: &Arc<Shared>, request: Request) -> Option<Response> {
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
    let static_file = client_page(&project_view(&shared.root), path)
        || path.starts_with("/assets/")
        || alias.is_some();
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
    ) && query(&request, "slug").is_none_or(|slug| slug.trim().is_empty())
    {
        return Some(error(StatusCode::BAD_REQUEST, "slug is required"));
    }
    if !scope.is_empty() && shared.api.get().and_then(|hub| hub.tree(scope)).is_none() {
        return Some(error(StatusCode::NOT_FOUND, "no worktree for this branch"));
    }
    // Uploads are the server's own, the same file for every branch's pages: a branch card's
    // picture names `/@tree/artifacts/uploads/…` and is read from this checkout's store.
    if let Some(file) = path.strip_prefix(symphony::uploads::ROUTE) {
        return Some(crate::media_files::upload(shared, file, &request).await);
    }
    if let Some(target) = alias {
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
    if media_file && !plugin_route && !thumbnail {
        let root = if scope.is_empty() {
            hub.root.clone()
        } else {
            hub.tree(scope).expect("scope was checked")
        };
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
                    Ok(Err(why)) => return Some(error(StatusCode::SERVICE_UNAVAILABLE, why)),
                    Err(why) => {
                        return Some(error(StatusCode::INTERNAL_SERVER_ERROR, why.to_string()));
                    }
                }
            } else {
                std::collections::BTreeMap::new()
            };
            match hub.task_snapshot() {
                Ok(mut tasks) => {
                    if path == "/api/task-board" {
                        let offset = query(&request, "offset")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let cursor = match query(&request, "cursor")
                            .map(|text| serde_json::from_str::<(String, i64, f64, String)>(&text))
                            .transpose()
                        {
                            Ok(cursor) => cursor,
                            Err(error) => {
                                return Some(self::error(
                                    StatusCode::BAD_REQUEST,
                                    format!("invalid task cursor: {error}"),
                                ));
                            }
                        };
                        let all = query(&request, "all").as_deref() == Some("1");
                        let status = query(&request, "status");
                        let mut signoff = hub
                            .task_signoff
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        if !search.is_empty() {
                            signoff.retain(|slug| scores.contains_key(slug));
                        }
                        let counts_only = query(&request, "counts").as_deref() == Some("1");
                        let older = query(&request, "older").as_deref() == Some("1");
                        let days = if query(&request, "days").as_deref() == Some("7") {
                            7.0
                        } else {
                            1.0
                        };
                        let cutoff = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64()
                            - days * 86_400.0;
                        let parent = query(&request, "parent");
                        let holder = query(&request, "holder");
                        let role = query(&request, "role");
                        let open_only = query(&request, "open").as_deref() == Some("1");
                        let roots = query(&request, "roots").as_deref() == Some("1");
                        let ids: std::collections::BTreeSet<_> =
                            tasks.tasks.iter().map(|t| t.slug.clone()).collect();
                        let parents: std::collections::BTreeSet<_> = tasks
                            .tasks
                            .iter()
                            .filter_map(|t| t.parent.clone())
                            .collect();
                        tasks.tasks.retain(|task| {
                            (parent
                                .as_ref()
                                .is_none_or(|p| task.parent.as_ref() == Some(p)))
                                && (!open_only
                                    || !["done", "cancelled"].contains(&task.status.as_str()))
                                && holder.as_ref().is_none_or(|handle| {
                                    task.roles.iter().any(|r| {
                                        r.handle == *handle
                                            && role.as_ref().is_none_or(|role| r.role == *role)
                                    })
                                })
                                && (!roots
                                    || (parents.contains(&task.slug)
                                        && task.parent.as_ref().is_none_or(|p| !ids.contains(p))))
                                && (all
                                    || older
                                    || !search.is_empty()
                                    || !["done", "cancelled"].contains(&task.status.as_str())
                                    || task.updated >= cutoff)
                                && (search.is_empty() || scores.contains_key(&task.slug))
                        });
                        tasks.tasks.sort_by(|a, b| {
                            if !search.is_empty() {
                                return scores[&b.slug]
                                    .total_cmp(&scores[&a.slug])
                                    .then(a.slug.cmp(&b.slug));
                            }
                            if parent.is_some() || roots || holder.is_some() {
                                return (a.priority, &a.slug).cmp(&(b.priority, &b.slug));
                            }
                            a.status.cmp(&b.status).then_with(|| {
                                if ["done", "cancelled"].contains(&a.status.as_str()) {
                                    b.updated.total_cmp(&a.updated).then(a.slug.cmp(&b.slug))
                                } else {
                                    (a.priority, &a.slug).cmp(&(b.priority, &b.slug))
                                }
                            })
                        });
                        let mut counts = std::collections::BTreeMap::<String, usize>::new();
                        let mut parent_index = 0_usize;
                        tasks.tasks.retain(|task| {
                            let count = counts.entry(task.status.clone()).or_default();
                            let index = *count;
                            *count += 1;
                            let selected = status.as_ref().is_none_or(|s| {
                                task.status == *s
                                    || (s == "signoff" && signoff.contains(&task.slug))
                            });
                            let after =
                                cursor
                                    .as_ref()
                                    .is_none_or(|(state, priority, updated, slug)| {
                                        if !search.is_empty() {
                                            return scores[&task.slug] < *updated
                                                || (scores[&task.slug] == *updated
                                                    && task.slug > *slug);
                                        }
                                        if parent.is_some() || roots || holder.is_some() {
                                            return (task.priority, &task.slug) > (*priority, slug);
                                        }
                                        task.status > *state
                                            || (task.status == *state
                                                && if ["done", "cancelled"]
                                                    .contains(&state.as_str())
                                                {
                                                    task.updated < *updated
                                                        || (task.updated == *updated
                                                            && task.slug > *slug)
                                                } else {
                                                    (task.priority, &task.slug) > (*priority, slug)
                                                })
                                    });
                            let index = if cursor.is_some() {
                                let index = parent_index;
                                if selected && after {
                                    parent_index += 1;
                                }
                                index
                            } else if parent.is_some()
                                || roots
                                || holder.is_some()
                                || (status.as_deref() == Some("signoff") && selected)
                            {
                                let index = parent_index;
                                parent_index += 1;
                                index
                            } else {
                                index
                            };
                            !counts_only
                                && selected
                                && after
                                && (all
                                    || if cursor.is_some() {
                                        index < 30
                                    } else {
                                        (offset..offset.saturating_add(30)).contains(&index)
                                    })
                        });
                        for task in &mut tasks.tasks {
                            task.body.clear();
                        }
                        let next = tasks.tasks.last().map(|task| {
                            serde_json::to_string(&(
                                &task.status,
                                task.priority,
                                if search.is_empty() {
                                    task.updated
                                } else {
                                    scores[&task.slug]
                                },
                                &task.slug,
                            ))
                            .expect("a task paging key serializes")
                        });
                        no_store(Json(symphony_api::TaskListing {
                            statuses: symphony_board::STATUSES
                                .iter()
                                .map(|s| (*s).to_string())
                                .collect(),
                            next,
                            tasks: tasks.tasks,
                            signoff,
                            counts: counts.into_iter().collect(),
                        }))
                    } else {
                        no_store(Json(tasks))
                    }
                }
                Err(e) => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("the task board cannot be reached: {e}"),
                ),
            }
        }
        "/api/agents" | "/api/agent-page" | "/api/task-thread" | "/api/task-page"
        | "/api/task-brief" | "/api/task-offer" | "/api/task-history" => {
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
            match tokio::task::spawn_blocking(move || {
                let store = symphony::taskview::connect(&root)
                    .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
                if agents {
                    let mut view = symphony::taskview::agents(&store, &root)
                        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
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
                        let (mut tasks, _) = symphony::taskview::snapshot(&store)
                            .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
                        let mut shown = std::collections::BTreeSet::new();
                        for agent in &view.agents {
                            let mut finished: Vec<_> = tasks
                                .tasks
                                .iter()
                                .filter(|t| {
                                    ["done", "cancelled"].contains(&t.status.as_str())
                                        && t.roles.iter().any(|r| r.handle == agent.handle)
                                })
                                .collect();
                            finished.sort_by(|a, b| b.updated.total_cmp(&a.updated));
                            shown.extend(finished.into_iter().take(5).map(|t| t.slug.clone()));
                            shown.extend(
                                tasks
                                    .tasks
                                    .iter()
                                    .filter(|t| {
                                        !["done", "cancelled"].contains(&t.status.as_str())
                                            && t.roles.iter().any(|r| {
                                                r.handle == agent.handle && r.role != "orchestrator"
                                            })
                                    })
                                    .map(|t| t.slug.clone()),
                            );
                        }
                        tasks.tasks.retain(|t| shown.contains(&t.slug));
                        for task in &mut tasks.tasks {
                            task.body.clear();
                        }
                        Ok(no_store(Json(symphony_api::AgentPage {
                            view,
                            tasks: tasks.tasks,
                            total,
                            next,
                        })))
                    } else {
                        Ok(no_store(Json(view)))
                    }
                } else if offer {
                    store.get(&slug).map_err(task_failure)?;
                    let plugins = symphony::plugins::Plugins::of(&root);
                    symphony::taskview::owner_offer(&store, &plugins, &slug, &root)
                        .map(|offer| no_store(Json(offer)))
                        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))
                } else if brief {
                    let record = store.get(&slug).map_err(task_failure)?;
                    let from = query_from.unwrap_or(0);
                    Ok(no_store(Json(symphony::taskview::brief(
                        &record, from, &root,
                    ))))
                } else if detail {
                    let record = store.get(&slug).map_err(task_failure)?;
                    symphony::taskview::page(&store, &record, &root)
                        .map(Json)
                        .map(no_store)
                        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))
                } else {
                    symphony::taskview::history(&store, &slug, before, after, &root)
                        .map(Json)
                        .map(no_store)
                        .map_err(task_failure)
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

fn task_failure(error: symphony_board::Error) -> (StatusCode, String) {
    let status = match error {
        symphony_board::Error::Refused(_) => StatusCode::NOT_FOUND,
        symphony_board::Error::Db(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, error.to_string())
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
        return match ask_response(&render, rel, scope, request, path == "/api/asks") {
            Ok(value) => no_store(Json(value)),
            Err((status, why)) => error(status, why),
        };
    }
    let result = match path {
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
    let recipient = symphony::taskview::connect(render.root())
        .ok()
        .and_then(|board| board.chat_recipient().ok().flatten());
    let listening = recipient.is_some();
    let typed = query(request, "format").as_deref() == Some("blocks");
    let rendered = threads
        .into_iter()
        .map(|thread| {
            let mut ask: symphony_api::Ask =
                serde_json::from_value(json!(thread)).expect("ask wire shape");
            for message in &mut ask.messages {
                // An answer is an agent's Markdown; the owner's words stay literal but for the
                // lines naming the files they attached.
                let mut fragment: symphony_api::Fragment =
                    serde_json::from_value(json!(match (message.kind == "answer", typed) {
                        (true, true) => render.typed_reply(&message.text, &ask.path),
                        (true, false) => render.reply(&message.text, &ask.path),
                        (false, true) => render.typed_owner(&message.text, &ask.path),
                        (false, false) => render.owner(&message.text, &ask.path),
                    }))
                    .expect("fragment wire shape");
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
            crate::media_files::contained(shared, &base, &file, request).await
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
    request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .any(|value| {
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
                        let summary = value
                            .get("text")
                            .and_then(|text| text.as_str())
                            .unwrap_or_default()
                            .chars()
                            .take(140)
                            .collect::<String>();
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

async fn static_response(shared: &Shared, path: &str, request: &Request) -> Response {
    let relative = if client_page(&project_view(&shared.root), path) {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if Path::new(relative)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return error(StatusCode::NOT_FOUND, "no such app file");
    }
    let root = match tokio::fs::canonicalize(&shared.ui_dist).await {
        Ok(root) => root,
        Err(_) => return client_unbuilt(shared),
    };
    let file = match tokio::fs::canonicalize(root.join(relative)).await {
        // The shipped client lives inside the common directory, deliberately. Only this static
        // handler may serve it, and canonical containment still rejects links outside its bundle.
        Ok(file)
            if file.starts_with(&root)
                && file.is_file()
                && (shared.installed_ui || !shared.is_metadata(&file)) =>
        {
            file
        }
        Err(e) if relative == "index.html" && e.kind() == std::io::ErrorKind::NotFound => {
            return client_unbuilt(shared);
        }
        _ => return error(StatusCode::NOT_FOUND, "no such app file"),
    };
    let mut bytes = match tokio::fs::read(&file).await {
        Ok(bytes) => bytes,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if relative == "index.html" {
        let shell = String::from_utf8_lossy(&bytes);
        let body = shell
            .find("<body")
            .and_then(|at| shell[at..].find('>').map(|end| at + end + 1));
        // The project's name and navigation ride in the page's body, so the first paint asks for
        // nothing; a shell without a body is served as it is, the client then drawing no nav.
        let element = body.map_or_else(String::new, |_| {
            symphony::project::Project::element(
                &symphony::project::Project::of(&shared.root).view(),
            )
        });
        let banner = shared.orphan_banner().unwrap_or_default();
        if !element.is_empty() || !banner.is_empty() {
            let at = body.unwrap_or(0);
            bytes = format!("{}{element}{banner}{}", &shell[..at], &shell[at..]).into_bytes();
        }
    }
    let extension = file.extension().and_then(|s| s.to_str()).unwrap_or("");
    let content_type = match extension {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let hash: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let etag = format!("\"{hash}\"");
    let cached = matches_etag(request, &etag);
    let length = bytes.len();
    let mut response = if cached {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
    };
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().expect("hex etag is a header"));
    if !cached {
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, length.into());
    }
    let immutable =
        relative.starts_with("assets/") && relative.split('-').any(|part| part.starts_with("dxh"));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    response
}

use sha2::Digest;

fn client_unbuilt(shared: &Shared) -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        if shared.installed_ui {
            "the Symphony client has not been built: run just symphony --stop, then just symphony from the main checkout"
        } else {
            "the Symphony client has not been built: run just symphony-ui-dist"
        },
    )
}

/// The project at `root` as the client reads it: its plugins' navigation and its entrypoints.
pub(crate) fn project_view(root: &Path) -> symphony_api::ProjectView {
    symphony::project::Project::of(root).view()
}

/// Whether `path` is a page the client draws: its own, one of the project's navigation pages, or
/// a document or folder page of one of its entrypoints.
pub(crate) fn client_page(project: &symphony_api::ProjectView, path: &str) -> bool {
    let page = path.trim_end_matches('/');
    matches!(page, "" | "/tasks" | "/agents" | "/queue")
        || project
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

fn encode_location(path: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_draws_the_symphonys_pages_and_each_entrypoints_documents() {
        let convention = symphony_api::ProjectView {
            nav: vec![symphony_api::NavItem {
                label: "▤ shelf".into(),
                title: "Shelf items".into(),
                href: "/shelf".into(),
                document: "docs/SHELF.md".into(),
            }],
            ..symphony_api::ProjectView::default()
        };
        let root = symphony_api::ProjectView {
            entrypoints: vec![String::new()],
            ..convention.clone()
        };
        for (path, by_convention, at_root) in [
            ("/", true, true),
            ("/tasks", true, true),
            ("/task/tools/x", true, true),
            ("/shelf/", true, true),
            ("/docs/", true, true),
            ("/docs/game/SPEC.md", true, true),
            ("/index.md", false, true),
            ("/guide/", false, true),
            ("/docs/v1.0/", true, true),
            ("/docs/v1.0", false, false),
            ("/v1.0/", false, true),
            ("/tasks/", true, true),
            ("/api/", false, false),
            ("/media/a.png", false, false),
            ("/api/document", false, false),
            ("/assets/app.js", false, false),
        ] {
            assert_eq!(client_page(&convention, path), by_convention, "{path}");
            assert_eq!(client_page(&root, path), at_root, "{path} at /");
        }
    }

    #[test]
    fn document_feed_defers_bodies_and_reserves_only_current_anchors() {
        let event = json!({"version":4,"changes":[{"region":"document","data":{
            "path":"@design~policy/docs/POLICY.md",
            "sections":[{"key":"policy","blocks":[{"type":"heading","anchor":"policy","spans":[]}],
                "comparison":{"kind":"changed","before":[{"type":"code","language":"","text":"Old policy"}]}},
                {"key":"removed:policy","blocks":[{"type":"heading","anchor":"policy","spans":[]}],
                "comparison":{"kind":"removed"}}]
        }}]}).to_string();
        let filter = ("@design~policy/docs/POLICY.md".to_string(), Vec::new());
        let unloaded: serde_json::Value =
            serde_json::from_str(&filtered_document_event(&event, &filter)).unwrap();
        let section = &unloaded["changes"][0]["data"]["sections"][0];
        assert_eq!(section["comparison"]["kind"], "changed");
        assert_eq!(section["blocks"], json!([]));
        assert_eq!(section["comparison"]["before"], json!([]));
        assert_eq!(
            unloaded["changes"][0]["data"]["anchors"]["policy"],
            json!(["policy"])
        );
        assert_eq!(
            unloaded["changes"][0]["data"]["anchors"]["removed:policy"],
            json!([])
        );
        let loaded: serde_json::Value = serde_json::from_str(&filtered_document_event(
            &event,
            &(filter.0, vec!["policy".into(), "removed:policy".into()]),
        ))
        .unwrap();
        let section = &loaded["changes"][0]["data"]["sections"][0];
        assert_eq!(section["blocks"].as_array().unwrap().len(), 1);
        assert_eq!(section["comparison"]["before"][0]["text"], "Old policy");
        assert_eq!(
            loaded["changes"][0]["data"]["sections"][1]["blocks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            loaded["changes"][0]["data"]["anchors"]["removed:policy"],
            json!([])
        );
    }

    #[test]
    fn matches_etag_handles_whitespace_empty_values_and_invalid_headers() {
        let cases: &[(&str, &[&[u8]], bool)] = &[
            ("absent", &[], false),
            ("empty", &[b""], false),
            ("whitespace", &[b" \t "], false),
            ("empty list entries", &[b", ,\t,"], false),
            ("empty quoted tag", &[b"\"\""], false),
            ("strong", &[b"\"fresh\""], true),
            ("padded strong", &[b" \t\"fresh\"\t "], true),
            (
                "padded weak in list",
                &[b" , \"stale\" ,\tW/\"fresh\" , "],
                true,
            ),
            ("padded wildcard", &[b" \t*\t "], true),
            ("weak wildcard is not a wildcard", &[b"W/*"], false),
            ("quoted wildcard is a tag", &[b"\"*\""], false),
            ("different tag", &[b"W/\"stale\""], false),
            ("case sensitive tag", &[b"\"FRESH\""], false),
            ("invalid header bytes", &[b"\xff"], false),
            ("invalid list value", &[b"\xff, \"fresh\""], false),
            ("invalid then valid header", &[b"\xff", b"\"fresh\""], true),
            ("empty then valid header", &[b"", b"W/\"fresh\""], true),
        ];
        for &(case, values, expected) in cases {
            let mut request = Request::new(());
            for value in values {
                request.headers_mut().append(
                    header::IF_NONE_MATCH,
                    header::HeaderValue::from_bytes(value).unwrap(),
                );
            }
            assert_eq!(matches_etag(&request, "\"fresh\""), expected, "{case}");
        }
    }
}
