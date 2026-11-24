//! Native owner writes. Validate the browser's origin before reading or dispatching its body.
//!
//! Each save is asked by the actor the local host's gate admitted its request as
//! ([`crate::front::Admitted`]: the person at the machine, or a visitor a board added), the actor
//! its save takes ([`symphony::save::Saves::post_as`]), never one its payload names. A
//! request hosted admission let in is refused here: a hosted visitor's saves are its host's
//! dispatch ([`crate::hosted::Host::dispatch`]) until hosted saves go through these.

use crate::api::error;
use crate::front::Shared;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};
use std::sync::Arc;

/// JSON saves carry text and references; uploaded file bytes have their own route and cap.
const JSON_BYTES: usize = 2 * 1024 * 1024;

// Why a write was refused, as its log line says.
const HOST_REFUSED: &str = "its Host is not this machine's";
const ORIGIN_REFUSED: &str = "its Origin is not http:// and its Host";
const CHECK_FAILED: &str = "its Host check did not finish";

/// Why a save hosted admission let in is refused here.
const HOSTED_SAVE: &str =
    "a hosted visitor's saves are its host's to dispatch, never the local owner's";

/// The server log's line for a refused write: its path, why, and its Host and Origin as sent, or
/// `absent`. A refused write's body is never read.
fn refusal(path: &str, why: &str, headers: &HeaderMap) -> String {
    let sent = |name: header::HeaderName| {
        headers
            .get(name)
            .map_or_else(|| "absent".to_string(), |value| format!("{value:?}"))
    };
    format!(
        "· refused a write to {path}: {why}; Host {}, Origin {}",
        sent(header::HOST),
        sent(header::ORIGIN)
    )
}

pub(crate) fn handles(request: &Request) -> bool {
    methods(request.uri().path()).is_some() && request.method() == Method::POST
}

/// The methods the save route at `path` takes, as an `Allow` header names them: POST, and the
/// reads of the one route that is also read (`/api/ask`); none for a path no save answers.
pub(crate) fn methods(path: &str) -> Option<&'static str> {
    let (_, route) = crate::api::scope_path(path);
    if route == "/api/ask" {
        Some("GET, HEAD, POST")
    } else if symphony::save::ROUTES.contains(&route)
        || route == symphony_api::UPLOAD_PATH
        || route == symphony_api::AGENT_FOCUS_PATH
        || route == symphony_api::ATTEMPT_CONTROL_ROUTE
        || route == symphony_api::ATTEMPT_VIEW_ROUTE
        || route == symphony_api::PEOPLE_CHANGE_PATH
        || route == symphony_api::PROFILE_PATH
    {
        Some("POST")
    } else {
        None
    }
}

pub(crate) async fn answer(shared: &Arc<Shared>, request: Request) -> Response {
    let (scope, route) = crate::api::scope_path(request.uri().path());
    let scope = scope.to_string();
    let route = route.to_string();
    let actor = crate::front::Admitted::actor(shared, request.extensions());
    let machine = crate::front::Admitted::machine(request.extensions());
    if let Err(why) = allowed(shared, &request).await {
        eprintln!("{}", refusal(request.uri().path(), why, request.headers()));
        return error(
            StatusCode::FORBIDDEN,
            "Forbidden: the Host or Origin header does not name this app",
        );
    }
    if !machine
        && let Some(refused) = unpermitted(shared, request.uri().path(), &route, &actor).await
    {
        return refused;
    }
    if route == symphony_api::UPLOAD_PATH {
        return upload(shared, request).await;
    }
    if route == symphony_api::AGENT_FOCUS_PATH {
        return crate::session_go::answer_focus(shared, request).await;
    }
    if route == symphony_api::ATTEMPT_CONTROL_ROUTE || route == symphony_api::ATTEMPT_VIEW_ROUTE {
        return attempt(shared, &route, request, &actor).await;
    }
    if crate::hosted::admission(&request).is_some() {
        return error(StatusCode::FORBIDDEN, HOSTED_SAVE);
    }
    let payload = match json_body(request.into_body()).await {
        Ok(payload) => payload,
        Err((status, why)) => return error(status, why),
    };
    if route == symphony_api::workflow::WORKFLOW_APPLY_ROUTE {
        return workflow_apply(shared, payload, actor).await;
    }
    if route == symphony_api::PEOPLE_CHANGE_PATH || route == symphony_api::PROFILE_PATH {
        let asker = crate::people::Asker {
            name: actor.name().to_string(),
            machine,
        };
        return crate::people::write(shared, &scope, &route, payload, asker).await;
    }
    let shared = shared.clone();
    match crate::spawn::blocking(move || {
        let saves = shared
            .saves
            .get_or_init(|| symphony::save::Saves::start(&shared.root));
        let actor = &actor;
        let result = if route == "/api/postpone" {
            match shared
                .api
                .get()
                .ok_or_else(|| "Symphony data is not ready".to_string())
                .and_then(|hub| hub.queue_order())
            {
                Ok(order) => saves.post_with_order_as(actor, &scope, &route, &payload, &order),
                Err(error) => Err(symphony::save::Failure { status: 503, error }),
            }
        } else {
            saves.post_as(actor, &scope, &route, &payload)
        };
        if result.as_ref().is_ok_and(|value| value["changed"] == true)
            && route == "/api/postpone"
            && let Some(hub) = shared.api.get()
            && let Err(error) = hub.refresh_order(&scope)
        {
            return Err(symphony::save::Failure { status: 503, error });
        }
        // A save kept for delivery by pull request wrote no file, so no watcher tells the pages:
        // they are read again here, and the feed brings the owner's line and the pending note.
        if result
            .as_ref()
            .is_ok_and(|value| value["delivery"] == "pending")
            && let Some(hub) = shared.api.get()
            && let Err(error) = hub.refresh_page("", &shared.root)
        {
            eprintln!("· the pages were not read again after a pending save: {error}");
        }
        if result.is_ok() && route == "/api/task-cancel" && shared.one_server() {
            let root = shared.root.clone();
            let slug = payload["slug"].as_str().unwrap_or("").to_string();
            let worker_root = root.clone();
            let worker_slug = slug.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("task-cancel-cleanup".into())
                .spawn(move || finish_cancel(&worker_root, &worker_slug))
            {
                cancel_failed(&root, &slug, &format!("could not start cleanup: {error}"));
            }
        }
        result
    })
    .await
    {
        Ok(Ok(value)) => ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response(),
        Ok(Err(e)) => error(
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST),
            e.error,
        ),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// The refusal of `actor`'s write to `route` (at `path`), a visitor a board named and not the
/// person at the machine, where they lack what it needs ([`crate::powers`]): none where they hold
/// it. Their powers are read from the board's rows now, before the write's body is read, so a
/// power revoked since a page was opened holds at once, and a board that cannot be read refuses.
/// The server's log line names them by their short name alone.
async fn unpermitted(
    shared: &Arc<Shared>,
    path: &str,
    route: &str,
    actor: &symphony_plugin::Actor,
) -> Option<Response> {
    let needs = crate::powers::needs(route);
    let (root, name) = (shared.root.clone(), actor.name().to_string());
    let asked = name.clone();
    let held = crate::spawn::blocking(move || {
        let people = symphony::taskview::connect(&root)?.people();
        people
            .map(|people| people.powers_of(&asked))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|held| held);
    let held = match held {
        Ok(held) => held,
        Err(why) => {
            eprintln!(
                "· refused a write to {path} by {name}: the board's people were not read: {why}"
            );
            return Some(error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("the task board cannot be reached to read who may write: {why}"),
            ));
        }
    };
    let refused = crate::powers::refused(&name, &held, needs)?;
    eprintln!("· refused a write to {path} by {name}: {refused}");
    Some(error(StatusCode::FORBIDDEN, refused))
}

/// The owner's workflow operation (`symphony::save::WORKFLOW_APPLY`): its result, or its refusal
/// as every save's is answered, with the guard that refused it beside the words as `guard` where
/// one did ([`symphony_api::workflow::GuardRefusal`]), which a task's page draws by its word.
async fn workflow_apply(
    shared: &Arc<Shared>,
    payload: serde_json::Value,
    actor: symphony_plugin::Actor,
) -> Response {
    let shared = shared.clone();
    let applied = crate::spawn::blocking(move || {
        let saves = shared
            .saves
            .get_or_init(|| symphony::save::Saves::start(&shared.root));
        saves.apply_workflow_as(&actor, &payload)
    })
    .await;
    match applied {
        Ok(Ok(value)) => ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response(),
        Ok(Err(refused)) => {
            let status = StatusCode::from_u16(refused.failure.status);
            let status = status.unwrap_or(StatusCode::BAD_REQUEST);
            let Some(guard) = refused.guard else {
                return error(status, refused.failure.error);
            };
            (
                status,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error": refused.failure.error, "guard": guard})),
            )
                .into_response()
        }
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// A control or a view of an execution attempt (`crate::attempts`), `actor`'s, whom the gate
/// admitted: never a hosted visitor's, whose posts are its host's dispatch. A view's address is
/// answered `no-store` and said nowhere else, this server's log included.
async fn attempt(
    shared: &Arc<Shared>,
    route: &str,
    request: Request,
    actor: &symphony_plugin::Actor,
) -> Response {
    if crate::hosted::admission(&request).is_some() {
        return error(
            StatusCode::FORBIDDEN,
            "an attempt's controls are the local owner's; a hosted visitor's are its host's",
        );
    }
    let payload = match json_body(request.into_body()).await {
        Ok(payload) => payload,
        Err((status, why)) => return error(status, why),
    };
    let hub = match shared.hub() {
        Ok(hub) => hub,
        Err(starting) => return *starting,
    };
    let view = route == symphony_api::ATTEMPT_VIEW_ROUTE;
    let asker = crate::attempts::Asker::Local(actor.principal.clone());
    let answered = crate::spawn::blocking(move || {
        if view {
            crate::attempts::view(&hub, payload, &asker).map(|link| json!(link))
        } else {
            crate::attempts::control(&hub, payload, &asker).map(|attempts| json!(attempts))
        }
    })
    .await;
    crate::api::answered(answered.map(|answer| answer.map(Json)), "")
}

/// Collect no more than the JSON cap, even for a chunked body with no length header. Once a
/// chunk exceeds the remaining space, refuse it without collecting it or polling the rest.
async fn json_body(body: Body) -> Result<Value, (StatusCode, String)> {
    use futures_util::StreamExt;
    let mut body = body.into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| (StatusCode::BAD_REQUEST, format!("bad JSON: {e}")))?;
        if chunk.len() > JSON_BYTES - bytes.len() {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "JSON saves are at most 2 MiB; send files through the upload route".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("bad JSON: {e}")))
    }
}

/// Whether a write names this app in its Host, and a browser's write also in its Origin; when it
/// does not, why, as its log line says. The headers are read before the check waits, so the
/// request body is not held across it, and every write, an upload too, is checked before its
/// body is read.
fn allowed(
    shared: &Arc<Shared>,
    request: &Request,
) -> impl Future<Output = Result<(), &'static str>> + use<> {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .map(|v| v.to_str().unwrap_or("").to_string());
    let check = shared.clone();
    async move {
        crate::spawn::blocking(move || {
            if !check.allows_host(&host) {
                return Err(HOST_REFUSED);
            }
            if origin
                .as_deref()
                .is_some_and(|o| !o.eq_ignore_ascii_case(&format!("http://{host}")))
            {
                return Err(ORIGIN_REFUSED);
            }
            Ok(())
        })
        .await
        .unwrap_or(Err(CHECK_FAILED))
    }
}

/// `POST /api/upload?name=FILE`: the file is the raw body, read to at most the cap (a larger one
/// is refused without reading the rest), then stored by `symphony::uploads` whatever branch the page
/// is on: the server's own checkout holds every upload.
async fn upload(shared: &Arc<Shared>, request: Request) -> Response {
    use futures_util::StreamExt;
    let max = symphony::uploads::MAX_BYTES;
    let too_large = |bytes: usize| {
        error(
            StatusCode::PAYLOAD_TOO_LARGE,
            symphony::uploads::Refusal::TooLarge { bytes }.to_string(),
        )
    };
    if let Some(length) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|length| *length > max)
    {
        return too_large(length);
    }
    let name = crate::api::query_in(request.uri(), "name").unwrap_or_default();
    let mut body = request.into_body().into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(chunk) if bytes.len() + chunk.len() > max => {
                return too_large(bytes.len() + chunk.len());
            }
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(e) => {
                return error(
                    StatusCode::BAD_REQUEST,
                    format!("the upload broke off: {e}"),
                );
            }
        }
    }
    let root = shared.root.clone();
    match crate::spawn::blocking(move || symphony::uploads::store(&root, &name, &bytes)).await {
        Ok(Ok(stored)) => {
            ([(header::CACHE_CONTROL, "no-store")], Json(stored.wire())).into_response()
        }
        Ok(Err(e)) => error(
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST),
            e.error,
        ),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// A preview's copied board may hold real pane identities. Only the shared installation
/// closes sessions and triggers the same detached sweep as the CLI's finished-task path, both by
/// the main checkout's build of `tasks` ([`crate::front::board_build`]); with none, neither runs and
/// the thread says where one goes.
fn finish_cancel(root: &std::path::Path, slug: &str) {
    let tasks = match crate::front::board_build(root) {
        Ok(tasks) => tasks,
        Err(missing) => return cancel_failed(root, slug, &missing),
    };
    match symphony_board::sys::output(
        symphony::children::command(tasks)
            .args(["finish", slug])
            .current_dir(root),
    ) {
        Ok(done) if done.status.success() => {
            // `tasks finish` preserves the completed task's success even when cleanup fails;
            // its failure is an arrow-prefixed warning on stdout, not a nonzero exit.
            let stdout = String::from_utf8_lossy(&done.stdout);
            if stdout.lines().any(|line| {
                line.trim_start()
                    .starts_with("→ could not clean up the sessions of ")
            }) {
                cancel_failed(root, slug, stdout.trim());
            } else if !stdout.trim().is_empty() {
                eprintln!("{}", stdout.trim());
            }
            if !done.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&done.stderr).trim());
            }
        }
        Ok(done) => cancel_failed(
            root,
            slug,
            &format!(
                "tasks finish {slug} exited with {}: {}",
                done.status,
                String::from_utf8_lossy(&done.stderr).trim()
            ),
        ),
        Err(error) => cancel_failed(root, slug, &format!("could not run tasks finish: {error}")),
    }
    let env = symphony::children::board_env();
    if symphony_board::sweep::trigger::enabled(&env)
        && !symphony_board::sweep::trigger::trigger(
            &env,
            root,
            &format!("{slug} is cancelled"),
            Some(slug),
        )
    {
        cancel_failed(root, slug, "could not start the cleanup sweep");
    }
}

/// Cleanup cannot change the successful cancellation response. Keep its failure on the task's
/// thread and in owner activity, using the app's identity rather than the starter's session.
fn cancel_failed(root: &std::path::Path, slug: &str, why: &str) {
    let text = format!("Task cancelled, but its cleanup failed: {why}");
    eprintln!("· {slug}: {text}");
    let report = || -> Result<(), String> {
        let board = symphony::taskview::connect(root)?;
        let actor = match board
            .sessions()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|session| session.row.kind == "app" && session.row.left_at.is_none())
        {
            Some(session) => session.row.handle,
            None => {
                board
                    .register(&symphony_board::NewSession {
                        model: "review-app".into(),
                        ..symphony_board::NewSession::new("app")
                    })
                    .map_err(|e| e.to_string())?
                    .row
                    .handle
            }
        };
        board
            .say(slug, &actor, &text, Some("owner"))
            .map_err(|e| e.to_string())?;
        Ok(())
    };
    if let Err(error) = report() {
        eprintln!("· {slug}: could not report cancellation cleanup failure to the owner: {error}");
    }
}

#[cfg(test)]
mod upload_tests;

#[cfg(test)]
mod body_tests;

#[cfg(test)]
mod pending_tests;

#[cfg(test)]
mod actor_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(header::HeaderName, &[u8])]) -> HeaderMap {
        pairs
            .iter()
            .map(|(name, value)| (name.clone(), HeaderValue::from_bytes(value).unwrap()))
            .collect()
    }

    #[test]
    fn a_refusal_names_the_path_why_and_the_host_and_origin_as_sent() {
        let sent = headers(&[
            (header::HOST, b"mac.taile6311e.ts.net:8765"),
            (header::ORIGIN, b"http://mac.taile6311e.ts.net:8765"),
            (header::CONTENT_LENGTH, b"42"),
        ]);
        assert_eq!(
            refusal("/@main/api/task-message", HOST_REFUSED, &sent),
            "· refused a write to /@main/api/task-message: its Host is not this machine's; \
             Host \"mac.taile6311e.ts.net:8765\", Origin \"http://mac.taile6311e.ts.net:8765\""
        );
    }

    #[test]
    fn a_refusal_says_absent_for_a_missing_header_and_escapes_bytes_outside_ascii() {
        let sent = headers(&[(header::ORIGIN, b"http://caf\xc3\xa9\".example")]);
        assert_eq!(
            refusal("/api/comment", ORIGIN_REFUSED, &sent),
            "· refused a write to /api/comment: its Origin is not http:// and its Host; \
             Host absent, Origin \"http://caf\\xc3\\xa9\\\".example\""
        );
        assert_eq!(
            refusal("/api/ask", CHECK_FAILED, &HeaderMap::new()),
            "· refused a write to /api/ask: its Host check did not finish; Host absent, Origin absent"
        );
    }
}
