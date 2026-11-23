//! Go to session over the routes: a pane is focused only for a page on this machine, any machine
//! the gate admits sees its screen, a subagent reaches its parent's session, a web address is the
//! page's own link, and nothing is ever typed into a pane. hc is a stand-in that keeps its calls.

use super::Hub;
use crate::front::{Shared, router};
use crate::gate::Gate;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed},
    repository, store_on,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use symphony_board::launch::LaunchError;
use symphony_board::launch::hc::Hc;
use tower::ServiceExt;

/// This machine's tailnet names, as `tailscale status` gives them.
const STATUS: &str =
    r#"{"Self": {"DNSName": "mac.tail.ts.net.", "TailscaleIPs": ["100.83.24.15"]}}"#;
const HERE: &str = "127.0.0.1:2";
const OWN_TAILNET: &str = "100.83.24.15:2";
const PHONE: &str = "100.83.24.99:2";
const LAN: &str = "192.168.1.5:2";
const SCREEN: &str = "❯ running the focused tests\n";
const WEB: &str = "https://claude.ai/code/session_01abc";

/// The wall-clock backstop of a wait here on its event: only a stall reaches it (RULES.md
/// `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(60));

/// What a wait that ran out says: `what` stalled, and the machine's load then.
fn stalled(what: &str) -> String {
    format!(
        "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
        test_time::Load::now()
    )
}

/// A response's status, headers and JSON (its text as a string when it is none).
async fn read(response: axum::response::Response) -> (StatusCode, HeaderMap, Value) {
    let (status, headers) = (response.status(), response.headers().clone());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, headers, body)
}

/// An hc and Herdr app that keep every call and answer each as the real ones would; while
/// [`Kept::hold`] holds them, a focus or a screen read waits to be answered until
/// [`Kept::release`].
#[derive(Clone, Default)]
struct Kept {
    calls: Arc<Mutex<Vec<String>>>,
    held: Arc<(Mutex<bool>, Condvar)>,
}

impl Kept {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn keep(&self, call: String) {
        self.calls.lock().unwrap().push(call);
        let (held, released) = &*self.held;
        let mut held = held.lock().unwrap();
        while *held {
            held = released.wait(held).unwrap();
        }
    }
    fn hold(&self) {
        *self.held.0.lock().unwrap() = true;
    }
    fn release(&self) {
        *self.held.0.lock().unwrap() = false;
        self.held.1.notify_all();
    }
    /// Wait until hc has been asked `call`.
    fn asked(&self, call: &str) {
        let asked = test_time::until_true(format!("hc to be asked {call}"), BACKSTOP, || {
            self.calls().iter().any(|c| c == call)
        });
        if let Err(stall) = asked {
            panic!("{stall}; asked {:?}", self.calls());
        }
    }
}

impl Hc for Kept {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        self.keep(args.join(" "));
        match args {
            ["read", "t7" | "t1", "--lines", "0"] => Ok(SCREEN.into()),
            ["read", t, ..] => Err(LaunchError(format!(
                "hc read {t}: not_found: no terminal \"{t}\""
            ))),
            _ => Err(LaunchError(format!(
                "{args:?} is no call a Go to session makes"
            ))),
        }
    }
    fn feed(&self, args: &[&str], _: &str) -> Result<String, LaunchError> {
        self.keep(format!("feed {}", args.join(" ")));
        Err(LaunchError("fed".into()))
    }
    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        self.keep(format!("app {}", args.join(" ")));
        // `t9` closed after the last tick saw it: the window shows nothing by it.
        Ok(if args.contains(&"t9") {
            r#"{"error":"no terminal t9"}"#
        } else {
            r#"{"ok":true}"#
        }
        .into())
    }
    fn raise(&self) -> Result<bool, LaunchError> {
        self.keep("raise".into());
        Ok(true)
    }
    fn caller(&self) -> Option<String> {
        None
    }
    fn board(&self) -> String {
        String::new()
    }
}

fn agent(handle: &str, more: Value) -> symphony_api::Agent {
    let mut agent = json!({
        "handle": handle, "kind": "worker", "model": "", "parent": null, "task": null,
        "role": null, "tasks": [], "worktree": "", "branch": "", "agent": null,
        "session_id": null, "pane": null, "state": "working", "state_detail": null,
        "health": "live", "last_tool_call": null, "last_message": null,
    });
    for (key, value) in more.as_object().unwrap() {
        agent[key] = value.clone();
    }
    serde_json::from_value(agent).unwrap()
}

/// The agents a tick saw, their ways in resolved as the tick resolves them with panes addressed.
fn agents() -> Vec<symphony_api::Agent> {
    let mut agents = vec![
        agent("amber", json!({"pane": "t7"})),
        agent("orch", json!({"kind": "orchestrator", "pane": "t1"})),
        agent("sub", json!({"parent": "orch", "in_process": true})),
        agent(
            "web",
            json!({"session_url": "https://claude.ai/code/session_01abc"}),
        ),
        agent("bare", json!({})),
        agent("gone", json!({"pane": "t9"})),
    ];
    symphony_api::resolve_sessions(&mut agents, true);
    agents
}

/// A project whose `symphony.toml` says how its panes are addressed, on a board where `amber`
/// executes `tools/open`, observed with [`agents`], served with `kept` as its hc. Its answer to
/// a method, path and body from a peer: status and JSON.
struct Served {
    shared: Arc<Shared>,
    runtime: tokio::runtime::Runtime,
    app: axum::Router,
    #[expect(
        dead_code,
        reason = "keeps the board and its repository for the test's length"
    )]
    held: (Tree, Fixture),
}

impl Served {
    /// Observed with [`agents`], their ways in already resolved.
    fn new(panes: &str, kept: &Kept) -> Served {
        Served::observed(
            panes,
            Some(kept),
            Seed::default()
                .session("amber")
                .task("tools/open", "in_progress")
                .role("tools/open", "amber", "executor"),
            |hub, f| {
                hub.update_board(
                    &f.store,
                    f.store.board().unwrap(),
                    symphony_api::Agents {
                        machine_work: symphony_api::MachineWork::default(),
                        generated: NOW,
                        probes: symphony_api::AgentProbes {
                            panes: "ok".into(),
                            slots: "ok".into(),
                        },
                        agents: agents(),
                    },
                )
                .unwrap();
            },
        )
    }

    /// Observed by the server's own board tick ([`Hub::board_read`]): `amber` executes and `webby`
    /// reviews `tools/open`, `webby` gave its web address, and hc is whatever `HC_BIN` names. A
    /// focus or screen read asks `kept`, or, with none, the hc the server starts itself
    /// ([`Hub::new`]'s own `panes`).
    fn ticked(panes: &str, kept: Option<&Kept>) -> Served {
        Served::observed(
            panes,
            kept,
            Seed::default()
                .session("amber")
                .session("webby")
                .task("tools/open", "in_progress")
                .role("tools/open", "amber", "executor")
                .role("tools/open", "webby", "reviewer"),
            |hub, f| {
                f.store.set_session_url("webby", WEB).unwrap();
                hub.board_read(&f.store).unwrap();
            },
        )
    }

    fn observed(
        panes: &str,
        kept: Option<&Kept>,
        seed: Seed,
        observe: impl FnOnce(&Hub, &Fixture),
    ) -> Served {
        let tree = Tree::new();
        let root = repository(&tree);
        std::fs::write(
            root.join("symphony.toml"),
            format!("[agents]\npanes = \"{panes}\"\n"),
        )
        .unwrap();
        let f = Fixture::new(seed);
        let mut hub = Hub::new(&root, &root.join("build/client"));
        let board = f.other_database();
        hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
        if let Some(kept) = kept {
            let handed = kept.clone();
            hub.panes = Box::new(move |_: &Path| Box::new(handed.clone()) as Box<dyn Hc>);
        }
        hub.refresh_page("", &root).unwrap();
        observe(&hub, &f);
        let peer_gate = Gate::new(
            Arc::new(|_| Ok(Some(r#"{"Node": {}}"#.into()))),
            Arc::new(Instant::now),
        )
        .with_status(Arc::new(|| Ok(STATUS.into())));
        let shared = Shared::new(1, 1, &root, None, peer_gate);
        assert!(shared.api.set(Arc::new(hub)).is_ok());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let app = router(shared.clone());
        Served {
            shared,
            runtime,
            app,
            held: (tree, f),
        }
    }

    fn ask(
        &self,
        peer: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let body = body.map(|b| b.to_string());
        let (status, _, body) = self.send(peer, method, path, &[], body);
        (status, body)
    }

    /// The request from `peer`, its Host this server's unless `headers` names another, and what
    /// it is answered: status, headers and JSON.
    fn send(
        &self,
        peer: &str,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<String>,
    ) -> (StatusCode, HeaderMap, Value) {
        let answer = self.start(peer, method, path, headers, body);
        self.runtime.block_on(async {
            let response = tokio::time::timeout(BACKSTOP, answer)
                .await
                .unwrap_or_else(|_| panic!("{}", stalled(&format!("{path} to answer"))))
                .unwrap()
                .unwrap();
            read(response).await
        })
    }

    /// The request [`Served::send`] makes, started on the server's runtime and left to answer.
    fn start(
        &self,
        peer: &str,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<String>,
    ) -> tokio::task::JoinHandle<Result<axum::response::Response, std::convert::Infallible>> {
        let app = self
            .app
            .clone()
            .layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        let mut request = Request::builder().method(method).uri(path);
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        {
            request = request.header("Host", "127.0.0.1:1");
        }
        request = request.header("Content-Type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = request
            .body(body.map_or_else(Body::empty, Body::from))
            .unwrap();
        self.runtime.spawn(app.oneshot(request))
    }

    fn focus(&self, peer: &str, handle: &str) -> (StatusCode, Value) {
        self.ask(
            peer,
            "POST",
            "/api/agent-focus",
            Some(json!({"handle": handle})),
        )
    }

    fn screen(&self, peer: &str, handle: &str) -> (StatusCode, Value) {
        self.ask(
            peer,
            "GET",
            &format!("/api/agent-screen?handle={handle}"),
            None,
        )
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

/// No call but showing a pane, bringing the app forward and reading a screen ever reached hc.
fn nothing_typed(kept: &Kept) {
    for call in kept.calls() {
        assert!(
            call.starts_with("app do agent.focus --terminal ")
                || call.starts_with("app do pane.focus --terminal ")
                || call == "raise"
                || call.starts_with("read "),
            "{call}"
        );
    }
}

#[test]
fn a_launched_agents_pane_is_focused_from_this_machine_and_its_screen_shown_elsewhere() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for peer in [HERE, OWN_TAILNET] {
        let (status, body) = s.focus(peer, "amber");
        assert_eq!(status, StatusCode::OK, "{peer}: {body}");
        assert_eq!(body["said"], "The Herdr app shows t7, in front.");
    }
    assert_eq!(
        kept.calls(),
        [
            "app do agent.focus --terminal t7",
            "raise",
            "app do agent.focus --terminal t7",
            "raise"
        ]
    );
    // A phone on the tailnet cannot focus anything on the Mac: it is told so and sees the screen.
    let (status, body) = s.focus(PHONE, "amber");
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], crate::session_go::NOT_HERE);
    assert_eq!(kept.calls().len(), 4, "a refused focus asks nothing");
    let (status, body) = s.screen(PHONE, "amber");
    assert_eq!(status, StatusCode::OK, "{body}");
    let screen: symphony_api::AgentScreen = serde_json::from_value(body).unwrap();
    assert_eq!(
        (
            screen.handle.as_str(),
            screen.pane.as_str(),
            screen.text.as_str()
        ),
        ("amber", "t7", SCREEN)
    );
    assert_eq!(kept.calls().last().unwrap(), "read t7 --lines 0");
    // An address the gate does not admit reaches neither.
    let (status, _) = s.screen(LAN, "amber");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = s.focus(LAN, "amber");
    assert_eq!(status, StatusCode::FORBIDDEN);
    nothing_typed(&kept);
}

#[test]
fn a_subagent_goes_to_its_parents_pane_and_says_so() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    let (status, body) = s.focus(HERE, "sub");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["said"],
        "sub runs inside orch's session. The Herdr app shows t1, in front."
    );
    let (status, body) = s.screen(PHONE, "sub");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["handle"].as_str(), body["pane"].as_str()),
        (Some("orch"), Some("t1"))
    );
    assert_eq!(
        kept.calls(),
        [
            "app do agent.focus --terminal t1",
            "raise",
            "read t1 --lines 0"
        ]
    );
}

#[test]
fn a_web_address_an_unknown_session_or_a_missing_handle_reaches_no_pane() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for (handle, why) in [
        ("web", "web's session has no pane, only its web address"),
        (
            "bare",
            "nothing is known of bare's session: no web address, no pane",
        ),
        ("ghost", "no agent ghost is connected"),
    ] {
        for (status, body) in [s.focus(HERE, handle), s.screen(HERE, handle)] {
            assert_eq!(status, StatusCode::NOT_FOUND, "{handle}: {body}");
            assert_eq!(body["error"], why);
        }
    }
    let (status, body) = s.ask(HERE, "POST", "/api/agent-focus", Some(json!({})));
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("handle is required"))
    );
    let (status, body) = s.ask(HERE, "GET", "/api/agent-screen", None);
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("handle is required"))
    );
    let (status, _) = s.ask(HERE, "POST", "/api/agent-focus", None);
    assert_eq!(status, StatusCode::BAD_REQUEST, "no body");
    let (status, body) = s.ask(
        HERE,
        "POST",
        "/api/agent-focus",
        Some(json!(["handle", "amber"])),
    );
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("handle is required")),
        "a body that is no object names no handle"
    );
    assert!(kept.calls().is_empty(), "{:?}", kept.calls());
}

#[test]
fn a_pane_gone_since_the_last_tick_is_refused_as_hc_said_it() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    let (status, body) = s.focus(HERE, "gone");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(
        body["error"],
        "the Herdr app did not show t9: herdr-app do agent.focus: no terminal t9; \
         herdr-app do pane.focus: no terminal t9"
    );
    let (status, body) = s.screen(PHONE, "gone");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "hc knows no terminal t9: its pane is gone");
    assert_eq!(
        kept.calls(),
        [
            "app do agent.focus --terminal t9",
            "app do pane.focus --terminal t9",
            "read t9 --lines 0"
        ],
        "a window that showed nothing is not brought forward"
    );
    nothing_typed(&kept);
}

#[test]
fn a_project_that_does_not_address_its_panes_asks_hc_nothing() {
    let kept = Kept::default();
    let s = Served::new("none", &kept);
    for (status, body) in [s.focus(HERE, "amber"), s.screen(PHONE, "amber")] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(
            body["error"],
            "this project does not address its agents' panes ([agents] panes = \"none\")"
        );
    }
    assert!(kept.calls().is_empty());
}

#[test]
fn the_pages_say_whether_they_were_asked_here_and_where_each_role_holder_goes() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for (peer, local) in [(HERE, true), (OWN_TAILNET, true), (PHONE, false)] {
        let (status, page) = s.ask(peer, "GET", "/api/agent-page", None);
        assert_eq!(status, StatusCode::OK, "{page}");
        let page: symphony_api::AgentPage = serde_json::from_value(page).unwrap();
        assert_eq!(page.local, local, "{peer}");
        let go = |handle: &str| {
            page.view
                .agents
                .iter()
                .find(|a| a.handle == handle)
                .and_then(|a| a.go.clone())
        };
        assert_eq!(go("amber").and_then(|g| g.pane), Some("t7".into()));
        assert_eq!(go("sub").map(|g| g.handle), Some("orch".into()));
        assert_eq!(go("bare"), None);
        let (status, task) = s.ask(peer, "GET", "/api/task-page?slug=tools/open", None);
        assert_eq!(status, StatusCode::OK, "{task}");
        let task: symphony_api::TaskPage = serde_json::from_value(task).unwrap();
        assert_eq!(task.local, local, "{peer}");
        assert_eq!(
            task.sessions.keys().collect::<Vec<_>>(),
            ["amber"],
            "the task's one role holder"
        );
        assert_eq!(task.sessions["amber"].pane.as_deref(), Some("t7"));
    }
    assert!(kept.calls().is_empty(), "reading a page asks hc nothing");
}

/// A focus of `amber`'s pane from this machine, with `headers` beside the request's own.
fn focus_with(s: &Served, peer: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Value) {
    s.send(
        peer,
        "POST",
        "/api/agent-focus",
        headers,
        Some(json!({"handle": "amber"}).to_string()),
    )
}

#[test]
fn a_page_of_another_site_or_a_sandbox_or_a_rebound_host_reaches_no_pane() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    const WRITE_REFUSED: &str = "Forbidden: the Host or Origin header does not name this app";
    // A focus from this machine, refused only for the Host or Origin its page sent: the write's
    // check comes before the route's own.
    for headers in [
        &[("Origin", "https://evil.example")][..],
        &[("Origin", "http://127.0.0.1:3000")],
        &[("Origin", "http://localhost:1")],
        &[("Origin", "null")],
        &[("Host", "evil.example")],
        &[
            ("Host", "evil.example:1"),
            ("Origin", "http://evil.example:1"),
        ],
    ] {
        let (status, answered, body) = focus_with(&s, HERE, headers);
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}: {body}");
        assert_eq!(body["error"], WRITE_REFUSED, "{headers:?}");
        assert_eq!(answered[header::CACHE_CONTROL], "no-store");
    }
    // A screen, from this machine or a phone: the read's Host check, then its sandbox check.
    for peer in [HERE, PHONE] {
        for (headers, why) in [
            (
                &[("Host", "evil.example")][..],
                "Forbidden: the Host header does not name this machine",
            ),
            (
                &[("Host", "evil.example:1")],
                "Forbidden: the Host header does not name this machine",
            ),
            (
                &[("Origin", "null")],
                "Forbidden: a sandboxed page has no access to the app API",
            ),
        ] {
            let (status, _, body) =
                s.send(peer, "GET", "/api/agent-screen?handle=amber", headers, None);
            assert_eq!(status, StatusCode::FORBIDDEN, "{peer} {headers:?}: {body}");
            assert_eq!(body["error"], why, "{peer} {headers:?}");
        }
    }
    assert!(kept.calls().is_empty(), "{:?}", kept.calls());
    // The same requests with this app's own Host and Origin reach the pane.
    let (status, _, body) = focus_with(&s, HERE, &[("Origin", "http://127.0.0.1:1")]);
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, body) = s.send(PHONE, "GET", "/api/agent-screen?handle=amber", &[], None);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        kept.calls(),
        [
            "app do agent.focus --terminal t7",
            "raise",
            "read t7 --lines 0"
        ]
    );
}

#[test]
fn a_phone_cannot_pass_for_this_machine_by_a_forwarding_header() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for headers in [
        &[("X-Forwarded-For", "127.0.0.1")][..],
        &[("X-Real-IP", "127.0.0.1")],
        &[("Forwarded", "for=127.0.0.1")],
        &[("X-Forwarded-For", "100.83.24.15")],
        &[
            ("X-Forwarded-For", "127.0.0.1"),
            ("X-Real-IP", "::1"),
            ("Forwarded", "for=\"[::1]\""),
            ("X-Forwarded-Host", "127.0.0.1:1"),
        ],
    ] {
        let (status, _, body) = focus_with(&s, PHONE, headers);
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}: {body}");
        assert_eq!(body["error"], crate::session_go::NOT_HERE, "{headers:?}");
        let (status, _, page) = s.send(PHONE, "GET", "/api/agent-page", headers, None);
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["local"], false, "{headers:?}");
        // From an address the gate does not admit, they open nothing either.
        let (status, _, body) = focus_with(&s, LAN, headers);
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}: {body}");
        let (status, _, _) = s.send(LAN, "GET", "/api/agent-screen?handle=amber", headers, None);
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}");
    }
    assert!(kept.calls().is_empty(), "{:?}", kept.calls());
}

#[test]
fn the_focus_takes_a_small_post_naming_an_agent_and_nothing_else_reaches_hc() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for method in ["GET", "HEAD", "PUT", "DELETE"] {
        let (status, answered, _) = s.send(HERE, method, "/api/agent-focus", &[], None);
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(answered[header::ALLOW], "POST", "{method}");
    }
    // A body past the few kilobytes a handle needs is refused before it is read as JSON.
    let padded = json!({"handle": "amber", "padding": "x".repeat(10_000)}).to_string();
    let (status, _, body) = s.send(HERE, "POST", "/api/agent-focus", &[], Some(padded));
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap().starts_with("bad request: "),
        "{body}"
    );
    // A handle is only ever looked up among the agents: one shaped as a flag, a command or a
    // terminal's address names none.
    for handle in [
        "--json t7;rm",
        "--json",
        "t7",
        "amber; rm -rf /",
        "../amber",
        "AMBER",
    ] {
        let (status, body) = s.focus(HERE, handle);
        assert_eq!(status, StatusCode::NOT_FOUND, "{handle:?}: {body}");
        let encoded = handle
            .bytes()
            .map(|b| format!("%{b:02X}"))
            .collect::<String>();
        let (status, _) = s.screen(PHONE, &encoded);
        assert_eq!(status, StatusCode::NOT_FOUND, "{handle:?}");
    }
    let (status, _, body) = s.send(
        HERE,
        "POST",
        "/api/agent-focus",
        &[],
        Some("{\"handle\": ".into()),
    );
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(kept.calls().is_empty(), "{:?}", kept.calls());
}

#[test]
fn a_screen_and_a_focus_are_never_kept_by_a_cache() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    for peer in [HERE, PHONE] {
        let (status, answered, body) =
            s.send(peer, "GET", "/api/agent-screen?handle=amber", &[], None);
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(answered[header::CACHE_CONTROL], "no-store", "{peer}");
        assert_eq!(body["text"], SCREEN);
    }
    let (status, answered, body) = focus_with(&s, HERE, &[]);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answered[header::CACHE_CONTROL], "no-store");
    let (status, answered, _) = s.send(PHONE, "GET", "/api/agent-screen?handle=ghost", &[], None);
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(answered[header::CACHE_CONTROL], "no-store");
}

#[test]
fn a_second_focus_or_screen_read_of_a_session_under_way_is_refused_until_it_answers() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    kept.hold();
    let focusing = s.start(
        HERE,
        "POST",
        "/api/agent-focus",
        &[],
        Some(json!({"handle": "amber"}).to_string()),
    );
    let reading = s.start(PHONE, "GET", "/api/agent-screen?handle=amber", &[], None);
    kept.asked("app do agent.focus --terminal t7");
    kept.asked("read t7 --lines 0");
    let (status, body) = s.focus(HERE, "amber");
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        format!(
            "a focus of amber's session is under way: its answer comes within {} s",
            symphony_board::launch::go::DEADLINE.as_secs()
        )
    );
    let (status, body) = s.screen(PHONE, "amber");
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("a screen read of amber's session is under way"),
        "{body}"
    );
    assert_eq!(kept.calls().len(), 2, "{:?}", kept.calls());
    kept.release();
    for answer in [focusing, reading] {
        let (status, _, body) = s
            .runtime
            .block_on(async { read(answer.await.unwrap().unwrap()).await });
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    // Once each has answered, the next is asked as any other.
    assert_eq!(s.focus(HERE, "amber").0, StatusCode::OK);
    assert_eq!(s.screen(PHONE, "amber").0, StatusCode::OK);
    // Another session is never held up by this one's.
    assert_eq!(s.focus(HERE, "sub").0, StatusCode::OK);
}

#[test]
fn a_focus_or_screen_read_that_fails_lets_its_session_go() {
    let kept = Kept::default();
    let s = Served::new("hc", &kept);
    let hub = s.shared.api.get().unwrap().clone();
    kept.hold();
    // `gone`'s pane closed after the last tick: hc and the app answer that they know no `t9`.
    let focusing = s.start(
        HERE,
        "POST",
        "/api/agent-focus",
        &[],
        Some(json!({"handle": "gone"}).to_string()),
    );
    let reading = s.start(PHONE, "GET", "/api/agent-screen?handle=gone", &[], None);
    kept.asked("app do agent.focus --terminal t9");
    kept.asked("read t9 --lines 0");
    assert_eq!(s.focus(HERE, "gone").0, StatusCode::CONFLICT);
    assert_eq!(s.screen(PHONE, "gone").0, StatusCode::CONFLICT);
    assert_eq!(hub.going.lock().unwrap().len(), 2);
    kept.release();
    for answer in [focusing, reading] {
        let (status, _, body) = s
            .runtime
            .block_on(async { read(answer.await.unwrap().unwrap()).await });
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    }
    assert!(
        hub.going.lock().unwrap().is_empty(),
        "{:?}",
        hub.going.lock().unwrap()
    );
    // Refused by hc, each lets its session go: the next is asked as any other, and refused alike.
    let (status, body) = s.focus(HERE, "gone");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    let (status, body) = s.screen(PHONE, "gone");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(hub.going.lock().unwrap().is_empty());
    let asked = |call: &str| kept.calls().iter().filter(|c| *c == call).count();
    assert_eq!(asked("app do agent.focus --terminal t9"), 2);
    assert_eq!(asked("app do pane.focus --terminal t9"), 2);
    assert_eq!(asked("read t9 --lines 0"), 2);
    assert_eq!(kept.calls().len(), 6, "{:?}", kept.calls());
}

/// What `hc` is asked by the server's own board tick in the re-run of
/// [`the_ticks_own_view_gives_each_agent_its_way_in_as_the_project_addresses_panes`]: one live
/// terminal, named for `amber`, as `hc ls --json` lists it, each call kept in `calls` beside it.
const LISTING: &str = r#"{"terminals": [{"address": "t7", "name": "amber", "agent": "claude-code", "status": "idle", "cwd": "/"}]}"#;

/// The shell script `body` as the executable `NAME` in `tree`.
fn stand_in(tree: &Tree, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = tree.write(name, &format!("#!/bin/sh\n{body}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Runs this binary's test `TEST` alone with `RERUN` set and the stand-ins `hc` and `app` as hc and
/// the Herdr app, which the server reads from its process environment: the re-run's own, set
/// before any of its threads starts. It must pass.
fn rerun(test: &str, rerun: &str, hc: &Path, app: &Path) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("live::session_go_tests::{test}")])
            .env(rerun, "1")
            .env("HC_BIN", hc)
            .env("HERDR_APP_BIN", app),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The tick resolves each agent's way in itself (`machine_work.rs`, `Hub::board_read`), from
/// what hc lists and the web address a session gave: run in a re-run of this test whose `HC_BIN`
/// names a stand-in, so the tick's hc is the one production starts from its environment.
#[test]
fn the_ticks_own_view_gives_each_agent_its_way_in_as_the_project_addresses_panes() {
    const RERUN: &str = "SYMPHONY_SESSION_GO_TICK";
    if std::env::var_os(RERUN).is_some() {
        ticked_ways_in();
        return;
    }
    let tree = Tree::new();
    let calls = tree.path("calls");
    let hc = stand_in(
        &tree,
        "hc",
        &format!(
            "echo \"$*\" >> '{}'\nprintf '%s\\n' '{LISTING}'\n",
            calls.display()
        ),
    );
    rerun(
        "the_ticks_own_view_gives_each_agent_its_way_in_as_the_project_addresses_panes",
        RERUN,
        &hc,
        Path::new("/usr/bin/false"),
    );
    // Each tick listed hc's terminals, and asked it nothing else.
    let asked = std::fs::read_to_string(&calls).unwrap_or_default();
    assert!(!asked.is_empty(), "the tick never asked hc");
    for line in asked.lines() {
        assert_eq!(line, "ls --json");
    }
}

/// The re-run's half: with panes addressed, `amber`'s way in is its pane and `webby`'s its web
/// address; with them not, `amber` has none and `webby` keeps its address.
fn ticked_ways_in() {
    for panes in ["hc", "none"] {
        let kept = Kept::default();
        let s = Served::ticked(panes, Some(&kept));
        let addressed = panes == "hc";
        for (peer, local) in [(HERE, true), (PHONE, false)] {
            let (status, page) = s.ask(peer, "GET", "/api/agent-page", None);
            assert_eq!(status, StatusCode::OK, "{page}");
            let page: symphony_api::AgentPage = serde_json::from_value(page).unwrap();
            assert_eq!(page.local, local, "{panes} {peer}");
            let agent = |handle: &str| {
                page.view
                    .agents
                    .iter()
                    .find(|a| a.handle == handle)
                    .unwrap_or_else(|| panic!("{handle} is not listed"))
                    .clone()
            };
            // The view finds amber's pane whatever the setting: only the way in depends on it.
            assert_eq!(agent("amber").pane.as_deref(), Some("t7"), "{panes}");
            let amber = agent("amber").go;
            let webby = agent("webby").go;
            let control = |go: &Option<symphony_api::SessionGo>| {
                go.as_ref()
                    .and_then(|go| go.control(page.local))
                    .map(|c| format!("{c:?}"))
            };
            let expected = match (addressed, local) {
                (true, true) => Some("Focus(\"t7\")".to_string()),
                (true, false) => Some("Screen(\"t7\")".to_string()),
                (false, _) => None,
            };
            assert_eq!(control(&amber), expected, "{panes} {peer}");
            assert_eq!(
                control(&webby),
                Some(format!("Link({WEB:?})")),
                "{panes} {peer}"
            );
            let (status, task) = s.ask(peer, "GET", "/api/task-page?slug=tools/open", None);
            assert_eq!(status, StatusCode::OK, "{task}");
            let task: symphony_api::TaskPage = serde_json::from_value(task).unwrap();
            assert_eq!(task.sessions.get("amber"), amber.as_ref(), "{panes}");
            assert_eq!(
                task.sessions.get("webby").and_then(|go| go.url.as_deref()),
                Some(WEB),
                "{panes}"
            );
        }
        assert!(kept.calls().is_empty(), "reading a page asks hc nothing");
    }
}

/// The allowance past [`DEADLINE`](symphony_board::launch::go::DEADLINE) of the wait for a hung
/// focus or screen read's answer, whose backstop only a stall reaches. Which limit ended the call
/// is the answer's own words: "no answer within the 10 s it is given" only where the deadline's
/// time was the shorter (`hc::Binary::answered`), never for an hc call given its 120 s alone.
const SLACK: Duration = Duration::from_secs(20);

/// The server's own hc ([`Hub::new`]'s `panes`) asks a hung hc and Herdr app under the deadline:
/// run in a re-run of this test whose `HC_BIN` names a stand-in that lists `amber`'s terminal and
/// never answers anything else, and whose `HERDR_APP_BIN` never answers at all.
#[test]
fn the_servers_own_hc_answers_a_hung_focus_and_screen_read_within_the_deadline() {
    const RERUN: &str = "SYMPHONY_SESSION_GO_HUNG";
    if std::env::var_os(RERUN).is_some() {
        hung_ways_in_answer_within_the_deadline();
        return;
    }
    let tree = Tree::new();
    // Each call but the tick's listing adds its words to `calls` and its pid to `pids`, then
    // becomes a `sleep` that outlasts any wait, in the same process.
    let hung = |prefix: &str| {
        format!(
            "echo \"{prefix}$*\" >> '{}'\necho $$ >> '{}'\nexec sleep 600\n",
            tree.path("calls").display(),
            tree.path("pids").display()
        )
    };
    let hc = stand_in(
        &tree,
        "hc",
        &format!(
            "if [ \"$1\" = ls ]; then printf '%s\\n' '{LISTING}'; exit 0; fi\n{}",
            hung("")
        ),
    );
    let app = stand_in(&tree, "herdr-app", &hung("app "));
    rerun(
        "the_servers_own_hc_answers_a_hung_focus_and_screen_read_within_the_deadline",
        RERUN,
        &hc,
        &app,
    );
}

/// The re-run's half: a focus from this machine and a screen read from a phone, together, each
/// answered within the deadline that hc did not answer within, its child killed and reaped and its
/// session let go; nothing past the call that took the whole deadline asked.
fn hung_ways_in_answer_within_the_deadline() {
    let stand_ins = Path::new(&std::env::var_os("HC_BIN").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let s = Served::ticked("hc", None);
    let hub = s.shared.api.get().unwrap().clone();
    let started = Instant::now();
    let focusing = s.start(
        HERE,
        "POST",
        "/api/agent-focus",
        &[],
        Some(json!({"handle": "amber"}).to_string()),
    );
    let reading = s.start(PHONE, "GET", "/api/agent-screen?handle=amber", &[], None);
    let given = symphony_board::launch::go::DEADLINE.as_secs();
    let backstop = test_time::backstop(symphony_board::launch::go::DEADLINE + SLACK);
    let by = started + backstop;
    for (answer, why) in [
        (
            focusing,
            format!(
                "the Herdr app did not show t7: herdr-app do agent.focus: no answer within the \
                 {given} s it is given; herdr-app do pane.focus: not asked: the {given} s it is \
                 given are spent"
            ),
        ),
        (
            reading,
            format!("hc read t7: no answer within the {given} s it is given"),
        ),
    ] {
        let (status, _, body) = s.runtime.block_on(async {
            let left = by.saturating_duration_since(Instant::now());
            let response = tokio::time::timeout(left, answer)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "an answer within the {given} s it is given: stalled: the wait's \
                         wall-clock backstop of {backstop:?} ran out; {}; {why}",
                        test_time::Load::now()
                    )
                })
                .unwrap()
                .unwrap();
            read(response).await
        });
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], why.as_str());
    }
    assert!(
        hub.going.lock().unwrap().is_empty(),
        "{:?}",
        hub.going.lock().unwrap()
    );
    // What was asked, if its stand-in was up before the kill: only the first call of each.
    let asked = std::fs::read_to_string(stand_ins.join("calls")).unwrap_or_default();
    for line in asked.lines() {
        assert!(
            ["app do agent.focus --terminal t7", "read t7 --lines 0"].contains(&line),
            "{asked}"
        );
    }
    let pids = std::fs::read_to_string(stand_ins.join("pids")).unwrap_or_default();
    for pid in pids.lines() {
        let pid: i64 = pid.parse().unwrap();
        test_time::until_true(
            format!("{pid} to be killed and reaped"),
            test_time::backstop(Duration::from_secs(30)),
            || !symphony_board::sys::pid_alive(pid),
        )
        .unwrap();
    }
}

#[test]
fn session_map_keeps_first_agent_lookup_duplicate_handles_and_complete_destinations() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    assert!(hub.sessions_of(["amber"]).is_empty());
    let mut observed = agents();
    let mut duplicate = observed[0].clone();
    duplicate.go = observed
        .iter()
        .find(|a| a.handle == "web")
        .unwrap()
        .go
        .clone();
    observed.push(duplicate);
    let mut later_bare = observed
        .iter()
        .find(|agent| agent.handle == "bare")
        .unwrap()
        .clone();
    later_bare.go = observed
        .iter()
        .find(|agent| agent.handle == "web")
        .unwrap()
        .go
        .clone();
    observed.push(later_bare);
    let handles = ["web", "missing", "bare", "amber", "sub", "web", "amber"];
    let expected: std::collections::BTreeMap<_, _> = handles
        .iter()
        .filter_map(|handle| {
            let agent = observed.iter().find(|agent| agent.handle == *handle)?;
            Some((handle.to_string(), agent.go.clone()?))
        })
        .collect();
    *hub.agents.lock().unwrap() = Some(symphony_api::Agents {
        machine_work: symphony_api::MachineWork::default(),
        generated: NOW,
        probes: symphony_api::AgentProbes {
            panes: "ok".into(),
            slots: "ok".into(),
        },
        agents: observed,
    });
    assert_eq!(
        serde_json::to_value(hub.sessions_of(handles)).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        hub.sessions_of(handles)
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["amber", "sub", "web"]
    );
    assert!(hub.sessions_of([]).is_empty());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = hub.agents.lock().unwrap();
        panic!("fixture poison");
    }));
    assert_eq!(
        serde_json::to_value(hub.sessions_of(handles)).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}

#[test]
fn native_map_insertions_keep_last_duplicate_payload_and_report_cost() {
    fn compare<V: Clone + std::fmt::Debug + serde::Serialize>(label: &str, rows: Vec<(String, V)>) {
        for count in [0, 1, 32, 512] {
            let pairs = rows[..count].to_vec();
            let old_input = pairs.clone();
            let new_input = pairs;
            let (old, old_cpu) = test_time::cpu::spent(|| {
                old_input
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>()
            });
            let (new, new_cpu) = test_time::cpu::spent(|| {
                let mut map = std::collections::BTreeMap::new();
                for (key, value) in new_input {
                    map.insert(key, value);
                }
                map
            });
            assert_eq!(
                serde_json::to_value(&new).unwrap(),
                serde_json::to_value(&old).unwrap()
            );
            assert_eq!(
                new.keys().collect::<Vec<_>>(),
                old.keys().collect::<Vec<_>>()
            );
            eprintln!(
                "{label} n={count}: standard_cpu={old_cpu:?}, insert_cpu={new_cpu:?}; no pair collection Vec/stable scratch; tree nodes only"
            );
        }
    }
    let destinations: Vec<_> = agents().into_iter().filter_map(|a| a.go).collect();
    compare(
        "SessionGo",
        (0..512)
            .map(|i| {
                (
                    format!("agent-{}", i % 37),
                    destinations[i % destinations.len()].clone(),
                )
            })
            .collect(),
    );
    compare(
        "tree paths",
        (0..512)
            .map(|i| {
                (
                    format!("branch-{}", i % 37),
                    std::path::PathBuf::from(format!("/tree/{i}")),
                )
            })
            .collect(),
    );
}
