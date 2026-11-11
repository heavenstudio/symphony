//! Go to session over the routes: a pane is focused only for a page on this machine, any machine
//! the gate admits sees its screen, a subagent reaches its parent's session, a web address is the
//! page's own link, and nothing is ever typed into a pane. hc is a stand-in that keeps its calls.

use super::Hub;
use crate::front::{Shared, router};
use crate::gate::Gate;
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    repository,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::launch::LaunchError;
use symphony_board::launch::hc::Hc;
use symphony_board::rpc::Client;
use tower::ServiceExt;

/// This machine's tailnet names, as `tailscale status` gives them.
const STATUS: &str =
    r#"{"Self": {"DNSName": "mac.tail.ts.net.", "TailscaleIPs": ["100.83.24.15"]}}"#;
const HERE: &str = "127.0.0.1:2";
const OWN_TAILNET: &str = "100.83.24.15:2";
const PHONE: &str = "100.83.24.99:2";
const LAN: &str = "192.168.1.5:2";
const SCREEN: &str = "❯ running the focused tests\n";

/// An hc and Herdr app that keep every call and answer each as the real ones would.
#[derive(Clone, Default)]
struct Kept(Arc<Mutex<Vec<String>>>);

impl Kept {
    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn keep(&self, call: String) {
        self.0.lock().unwrap().push(call);
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
    fn new(panes: &str, kept: &Kept) -> Served {
        let tree = Tree::new();
        let root = repository(&tree);
        std::fs::write(
            root.join("symphony.toml"),
            format!("[agents]\npanes = \"{panes}\"\n"),
        )
        .unwrap();
        let f = Fixture::new(
            Seed::default()
                .session("amber")
                .task("tools/open", "in_progress")
                .role("tools/open", "amber", "executor"),
        );
        let mut hub = Hub::new(&root, &root.join("build/client"));
        let board = f.other_client();
        hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
        let handed = kept.clone();
        hub.panes = Box::new(move |_: &Path| Box::new(handed.clone()) as Box<dyn Hc>);
        hub.refresh_page("", &root).unwrap();
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
        let app = self
            .app
            .clone()
            .layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("Host", "127.0.0.1:1")
            .header("Content-Type", "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        self.runtime.block_on(async {
            let response = tokio::time::timeout(Duration::from_secs(60), app.oneshot(request))
                .await
                .unwrap_or_else(|_| panic!("{path} did not answer"))
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
            (status, body)
        })
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

/// A store on `client`'s board at the fixture's time and host.
fn store_on(client: &Client) -> symphony_board::Store {
    symphony_board::Store::with(
        Client::new(client.addr(), "sf", client.database()),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
    )
    .unwrap()
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
