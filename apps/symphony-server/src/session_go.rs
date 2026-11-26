//! Go to session (docs/workflow/TASKS.md, "Going to a session"). Each agent's way into its session is
//! resolved on the board's tick ([`symphony_api::resolve_sessions`]); a page links a web address
//! itself. What reaches a pane comes here: from this machine, `POST /api/agent-focus` has the
//! Herdr app show it ([`symphony_board::launch::go::focus`]); from any machine the gate admits
//! (this one, or a confirmed tailnet peer), `GET /api/agent-screen` reads its visible screen.
//! Neither types into it. Both ask hc only where the project addresses its panes
//! (`[agents] panes = "hc"`), each within one deadline for all its calls
//! ([`symphony_board::launch::go::DEADLINE`]), and one at a time for a session.

use crate::api::error;
use crate::front::Shared;
use crate::live::Hub;
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, Request};
use axum::http::StatusCode;
use axum::response::{Json, Response};
use serde_json::Value;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use symphony_api::{AgentFocus, AgentScreen, SessionGo};
use symphony_board::launch::go as herdr;
use symphony_board::lock::Recovered as _;
use symphony_board::settings::Stored;

/// The largest focus request read: a handle in a JSON object.
const FOCUS_BYTES: usize = 4096;

/// What a request from another machine is told when it asks for a focus.
pub(crate) const NOT_HERE: &str = "a pane is shown in the Herdr app only for a page on the \
     machine its panes run on; from this one, Go to session shows the pane's screen";

/// The address `request` came from, as its connection gave it.
fn peer<B>(request: &axum::http::Request<B>) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

/// Whether `request` came from this machine ([`crate::gate::Gate::is_this_machine`]); one whose
/// address is unknown did not, nor one from a visitor who is not the person at the machine: a
/// signed visitor whose sign-in service runs on this machine reaches it by loopback. The address
/// is read before the answer is awaited, so the wait holds no borrow of the request (whose body is
/// not `Sync`).
pub(crate) fn local<B>(
    shared: &Arc<Shared>,
    request: &axum::http::Request<B>,
) -> impl std::future::Future<Output = bool> + Send + 'static {
    let elsewhere = crate::hosted::admission(request).is_some()
        || !crate::front::Admitted::machine(request.extensions());
    let ip = peer(request);
    let shared = shared.clone();
    async move {
        if elsewhere {
            return false;
        }
        let Some(ip) = ip else {
            return false;
        };
        crate::spawn::blocking(move || shared.gate.is_this_machine(ip))
            .await
            .unwrap_or(false)
    }
}

/// `handle`'s way into its session as the last observation resolved it, when it reaches a pane hc
/// can be asked about: refused when the project does not address its panes, when nothing is known
/// of the session, or when it is reached by its web address alone.
fn paned(hub: &Hub, handle: &str) -> Result<(SessionGo, String), (StatusCode, String)> {
    let project = symphony::project::Project::of(&hub.root);
    // The board's store's setting over the project file's, read on the board the page reads share
    // ([`Hub::on_reader`]), never one opened anew; a board that does not answer holds none, and the
    // file's is in force.
    let stored = match hub.on_reader(symphony_board::Store::stored_entries) {
        Ok(entries) => Stored::of(&entries),
        Err(why) => Stored::unread(&why),
    };
    let panes = stored.panes(&project.agents.panes);
    if panes != "hc" {
        return Err((
            StatusCode::FORBIDDEN,
            format!("this project does not address its agents' panes ([agents] panes = {panes:?})"),
        ));
    }
    let go = hub
        .session_of(handle)
        .map_err(|why| (StatusCode::NOT_FOUND, why))?;
    let pane = go.pane.clone().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("{}'s session has no pane, only its web address", go.handle),
        )
    })?;
    Ok((go, pane))
}

/// What going to `handle`'s session by `go` says first: that a subagent's is its parent's.
fn via(handle: &str, go: &SessionGo) -> String {
    if go.via_parent(handle) {
        format!("{handle} runs inside {}'s session. ", go.handle)
    } else {
        String::new()
    }
}

/// One focus, or one screen read, of a session under way ([`Hub::going`]): while it is held, another
/// of the same kind for that session is refused, so a page pressed again, or open twice, starts no
/// second chain of hc calls beside the first. It lets go when the call ends, answered or not, which
/// the deadline every call is given bounds ([`herdr::DEADLINE`]).
struct Going<'a> {
    hub: &'a Hub,
    key: String,
}

impl Going<'_> {
    /// Hold `what` (`focus`, `screen read`) of `go`'s session, or say that one is under way.
    fn hold<'a>(
        hub: &'a Hub,
        what: &str,
        go: &SessionGo,
    ) -> Result<Going<'a>, (StatusCode, String)> {
        let key = format!("{what} {}", go.handle);
        let fresh = hub.going.lock().recover().insert(key.clone());
        if fresh {
            Ok(Going { hub, key })
        } else {
            Err((
                StatusCode::CONFLICT,
                format!(
                    "a {what} of {}'s session is under way: its answer comes within {} s",
                    go.handle,
                    herdr::DEADLINE.as_secs()
                ),
            ))
        }
    }
}

impl Drop for Going<'_> {
    fn drop(&mut self) {
        self.hub.going.lock().recover().remove(&self.key);
    }
}

/// Show `handle`'s session's pane in the Herdr app on this machine and bring the app forward.
pub(crate) fn focus(hub: &Hub, handle: &str) -> Result<AgentFocus, (StatusCode, String)> {
    let (go, pane) = paned(hub, handle)?;
    let _going = Going::hold(hub, "focus", &go)?;
    let hc = (hub.panes)(&hub.root);
    let focused = herdr::focus(&*hc, &pane).map_err(|e| (StatusCode::BAD_GATEWAY, e.0))?;
    Ok(AgentFocus {
        said: format!("{}{}", via(handle, &go), focused.said()),
    })
}

/// What `handle`'s session's pane shows now, read only.
pub(crate) fn screen(hub: &Hub, handle: &str) -> Result<AgentScreen, (StatusCode, String)> {
    let (go, pane) = paned(hub, handle)?;
    let _going = Going::hold(hub, "screen read", &go)?;
    let hc = (hub.panes)(&hub.root);
    let text = herdr::screen(&*hc, &pane).map_err(|e| (StatusCode::BAD_GATEWAY, e.0))?;
    Ok(AgentScreen {
        handle: go.handle,
        pane,
        text,
        read_at: (hub.clock)(),
    })
}

/// `POST /api/agent-focus` with `{"handle": H}`, its Host and Origin already checked: refused for
/// a page on another machine before its body is read.
pub(crate) async fn answer_focus(shared: &Arc<Shared>, request: Request) -> Response {
    if !local(shared, &request).await {
        return error(StatusCode::FORBIDDEN, NOT_HERE);
    }
    let bytes = match to_bytes(request.into_body(), FOCUS_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => return error(StatusCode::BAD_REQUEST, format!("bad request: {e}")),
    };
    let handle = match serde_json::from_slice::<Value>(&bytes) {
        Ok(payload) => payload["handle"].as_str().unwrap_or("").trim().to_string(),
        Err(e) => return error(StatusCode::BAD_REQUEST, format!("bad JSON: {e}")),
    };
    if handle.is_empty() {
        return error(StatusCode::BAD_REQUEST, "handle is required");
    }
    let hub = match shared.hub() {
        Ok(hub) => hub,
        Err(starting) => return *starting,
    };
    let focused = crate::spawn::blocking(move || focus(&hub, &handle).map(Json)).await;
    crate::api::answered(focused, "")
}
