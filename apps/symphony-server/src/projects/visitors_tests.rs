//! Who reaches which project on the one server: a node under another Tailscale login, or a tagged
//! node, reaches no board that has not added them, and each such board answers them as one the
//! server does not hold, byte for byte; a board that has added a visitor lets them reach it and no
//! other. The person at the machine reaches every board, as before.

use super::tests::Machine;
use super::*;
use crate::gate::{Gate, Roster, Visitor};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use std::time::Instant;
use symphony_plugin::execution::{Principal, PrincipalKind};
use symphony_plugin::{Actor, Sender};

/// The Host a peer names. A project beside the starter checks Host with a gate of its own, on
/// this machine's real Tailscale, which knows no name of these fakes: loopback's name passes it,
/// as the one server's own check.
const HOST: &str = "127.0.0.1:1";
const STATUS: &str = r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 11, "TailscaleIPs": ["100.83.24.15"]}}"#;
const PHONE: &str = "100.101.102.103:1";
const LAPTOP: &str = "100.101.102.104:1";
const SERVER: &str = "100.101.102.105:1";

/// A gate whose whois knows the owner's phone (under this machine's user, 11), a colleague's
/// laptop (`ana@example.com`, user 12) and a tagged node, and whose status names this machine,
/// asking `roster` of anyone but the machine.
fn gate(roster: Roster) -> Gate {
    Gate::new(
        Arc::new(|address: &str| {
            let (name, user, login, tags) = match address {
                "100.101.102.103" => ("phone", 11, "owner@example.com", "[]"),
                "100.101.102.104" => ("laptop", 12, "ana@example.com", "[]"),
                "100.101.102.105" => ("ci", 99, "tagged-devices", r#"["tag:ci"]"#),
                _ => return Ok(None),
            };
            Ok(Some(format!(
                r#"{{"Node": {{"Name": "{name}.tail.ts.net.", "User": {user}, "Tags": {tags}}}, "UserProfile": {{"ID": {user}, "LoginName": "{login}", "DisplayName": "{name}"}}}}"#
            )))
        }),
        Arc::new(Instant::now),
    )
    .with_status(Arc::new(|| Ok(STATUS.to_string())))
    .with_roster(roster)
}

/// The roster of today: no board has added anyone.
fn nobody() -> Roster {
    Arc::new(|_, _| None)
}

/// Ana, as a board that added her names her.
fn ana() -> Actor {
    Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        Principal::new(
            PrincipalKind::Human,
            "tailscale".into(),
            "ana@example.com".into(),
        ),
    )
}

/// A roster on which the board of the project folder `id` has added Ana, and no board anyone
/// else.
fn ana_on(id: &'static str) -> Roster {
    Arc::new(move |root: &Path, visitor: &Visitor| {
        let her = matches!(visitor, Visitor::Login(login) if login.name == "ana@example.com");
        (her && root.ends_with(id)).then(ana)
    })
}

/// What `projects`' server answers `method` of `path` from `client`, naming [`HOST`], with `headers`,
/// `body` and the headers a page's save sends: status, headers and body, whole.
async fn exchange(
    projects: &Arc<Projects>,
    client: &str,
    (method, path, headers, body): Asked,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if method == "POST" {
        request = request
            .header(header::ORIGIN, format!("http://{HOST}"))
            .header(header::CONTENT_TYPE, "application/json");
    }
    let response = router(projects.clone())
        .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()))
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, parts.headers, bytes.to_vec())
}

/// One request: its method, path, headers beside Host and body.
type Asked = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static str,
);

/// A page of `beta`'s, as a request its page sends names it in its Referer.
const FROM_BETA: (&str, &str) = ("referer", "http://127.0.0.1:1/beta/tasks");
/// A browser loading a page.
const PAGE: (&str, &str) = ("sec-fetch-mode", "navigate");

/// Every way a visitor asks for `beta`: its address with and without the slash, its pages, a
/// document, the board and the pending cards as data, the feed, the theme, a respelled id, paths
/// of today and the address's icon whose Referer names it (a read, a page, a document), and a
/// save.
const ASKED: &[Asked] = &[
    ("GET", "/beta", &[], ""),
    ("GET", "/beta/", &[], ""),
    ("GET", "/beta/tasks", &[], ""),
    ("GET", "/beta/docs/README.md", &[], ""),
    ("GET", "/beta/api/task-board?counts=1&work=0", &[], ""),
    ("GET", "/beta/api/pending", &[], ""),
    ("GET", "/beta/api/feed?since=0", &[], ""),
    ("GET", "/beta/theme.css", &[], ""),
    ("HEAD", "/beta/docs/README.md", &[], ""),
    ("GET", "/%62eta/tasks", &[], ""),
    ("GET", "/api/task-board?counts=1&work=0", &[FROM_BETA], ""),
    ("GET", "/tasks", &[FROM_BETA, PAGE], ""),
    ("GET", "/docs/README.md", &[FROM_BETA], ""),
    ("GET", "/favicon.ico", &[FROM_BETA], ""),
    ("POST", "/beta/api/decision", &[], DECISION),
    ("GET", "/", &[], ""),
    ("GET", "/api/projects", &[], ""),
];

const DECISION: &str =
    r#"{"path":"docs/README.md","slug":"one","state":"question","note":"Which one?"}"#;

/// `visitor` asks every route of [`ASKED`] of a machine holding `alpha` and `beta`, and of a
/// server beside it over the same checkouts holding `alpha` alone, the roster `roster` in both:
/// each answer is the same, byte for byte.
async fn answered_as_not_held(visitor: &str, roster: fn() -> Roster) {
    let holding = Machine::with_gate(gate(roster()));
    let lacking = holding.beside("lacking", gate(roster()), &["alpha"]);
    for &asked @ (method, path, headers, _) in ASKED {
        let held = exchange(&holding.projects, visitor, asked).await;
        let not_held = exchange(&lacking, visitor, asked).await;
        assert_eq!(
            (held.0, &held.1),
            (not_held.0, &not_held.1),
            "{visitor} {method} {path} {headers:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&held.2),
            String::from_utf8_lossy(&not_held.2),
            "{visitor} {method} {path} {headers:?}"
        );
    }
    let readme = std::fs::read_to_string(holding.tree.path("beta/docs/README.md")).unwrap();
    assert_eq!(readme, "# Docs\n", "no write was taken");
}

#[tokio::test]
async fn another_login_reaches_every_board_as_one_the_server_does_not_hold() {
    answered_as_not_held(LAPTOP, nobody).await;
}

#[tokio::test]
async fn a_tagged_node_reaches_every_board_as_one_the_server_does_not_hold() {
    answered_as_not_held(SERVER, nobody).await;
}

#[tokio::test]
async fn a_board_that_has_not_added_a_visitor_answers_as_not_held_beside_one_that_has() {
    answered_as_not_held(LAPTOP, || ana_on("alpha")).await;
}

#[tokio::test]
async fn a_visitor_no_board_added_is_shown_no_project_and_none_of_the_machines_own() {
    let machine = Machine::with_gate(gate(nobody()));
    for visitor in [LAPTOP, SERVER] {
        let (status, _, body) = exchange(
            &machine.projects,
            visitor,
            ("GET", "/api/projects", &[], ""),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list: symphony_api::ProjectList = serde_json::from_slice(&body).unwrap();
        assert!(list.projects.is_empty(), "{visitor}: {list:?}");
        assert_eq!(list.installation, None, "{visitor}");
        let (status, _, body) = exchange(&machine.projects, visitor, ("GET", "/", &[], "")).await;
        assert_eq!(status, StatusCode::OK);
        let page = String::from_utf8(body).unwrap();
        for id in ["/alpha/", "/beta/", "alpha works", "beta works"] {
            assert!(!page.contains(id), "{visitor} is shown {id}: {page}");
        }
        let front = machine
            .named(visitor, "127.0.0.1:1", symphony_api::FRONT_PATH, &[])
            .await;
        assert_eq!(front.status(), StatusCode::FORBIDDEN, "{visitor}");
        assert!(!super::tests::text(front).await.contains("alpha"));
    }
}

#[tokio::test]
async fn the_owners_phone_reaches_every_board_and_the_machines_own_as_the_machine_does() {
    let machine = Machine::with_gate(gate(nobody()));
    let (_, _, body) = exchange(&machine.projects, PHONE, ("GET", "/api/projects", &[], "")).await;
    let list: symphony_api::ProjectList = serde_json::from_slice(&body).unwrap();
    let ids: Vec<_> = list
        .projects
        .iter()
        .map(|listed| listed.id.as_str())
        .collect();
    assert_eq!(ids, ["alpha", "beta"]);
    assert!(list.installation.is_some());
    for path in ["/beta/docs/README.md", "/alpha/theme.css"] {
        let (status, _, _) = exchange(&machine.projects, PHONE, ("GET", path, &[], "")).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
    let front = machine
        .named(PHONE, "127.0.0.1:1", symphony_api::FRONT_PATH, &[])
        .await;
    assert_eq!(front.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_board_that_added_a_visitor_lets_them_reach_it_and_no_other() {
    // Beta's own router judges with a gate of its own, which knows no fake peer: Ana reaches beta
    // only as the dispatch admitted her there.
    let machine = Machine::with_gate(gate(ana_on("beta")));
    let (status, _, body) = exchange(
        &machine.projects,
        LAPTOP,
        ("GET", "/beta/docs/README.md", &[], ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (_, _, body) = exchange(&machine.projects, LAPTOP, ("GET", "/api/projects", &[], "")).await;
    let list: symphony_api::ProjectList = serde_json::from_slice(&body).unwrap();
    let ids: Vec<_> = list
        .projects
        .iter()
        .map(|listed| listed.id.as_str())
        .collect();
    assert_eq!(ids, ["beta"]);
    let (_, _, page) = exchange(&machine.projects, LAPTOP, ("GET", "/", &[], "")).await;
    let page = String::from_utf8(page).unwrap();
    assert!(
        page.contains("/beta/") && !page.contains("/alpha/"),
        "{page}"
    );
    let (status, _, page) = exchange(
        &machine.projects,
        LAPTOP,
        ("GET", "/alpha/docs/README.md", &[], ""),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        String::from_utf8(page)
            .unwrap()
            .contains("No project is called"),
        "alpha is not there for her"
    );
    for (visitor, why) in [(SERVER, "a tagged node"), (PHONE, "the owner's phone")] {
        let (status, _, _) = exchange(
            &machine.projects,
            visitor,
            ("GET", "/alpha/docs/README.md", &[], ""),
        )
        .await;
        let reached = status == StatusCode::OK;
        assert_eq!(reached, visitor == PHONE, "{why}");
    }
    // The server's own state stays the machine's, even through a board that added her.
    for path in ["/beta/api/front", symphony_api::FRONT_PATH] {
        let (status, _, body) = exchange(&machine.projects, LAPTOP, ("GET", path, &[], "")).await;
        assert_eq!(
            (status, String::from_utf8(body).unwrap().as_str()),
            (
                StatusCode::FORBIDDEN,
                "Forbidden: this route answers the person at this machine alone\n"
            ),
            "{path}"
        );
        let (status, _, _) = exchange(&machine.projects, PHONE, ("GET", path, &[], "")).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
}
