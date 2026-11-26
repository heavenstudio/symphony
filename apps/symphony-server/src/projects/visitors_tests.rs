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

/// [`ana_on`]`(id)`, each ask counted by the project folder it names.
fn counted(id: &'static str, asks: Arc<std::sync::Mutex<Vec<String>>>) -> Roster {
    let roster = ana_on(id);
    Arc::new(move |root: &Path, visitor: &Visitor| {
        let named = root.file_name().unwrap().to_string_lossy().into_owned();
        asks.lock().unwrap().push(named);
        roster(root, visitor)
    })
}

/// The roster asks `exchange` of `path` from `client` made, emptied after.
async fn asked_of(
    machine: &Machine,
    asks: &std::sync::Mutex<Vec<String>>,
    client: &str,
    path: &'static str,
) -> Vec<String> {
    asks.lock().unwrap().clear();
    let (status, _, body) = exchange(&machine.projects, client, ("GET", path, &[], "")).await;
    assert_ne!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{path}: {}",
        String::from_utf8_lossy(&body)
    );
    std::mem::take(&mut *asks.lock().unwrap())
}

#[tokio::test]
async fn a_request_under_a_boards_id_that_added_the_visitor_asks_that_board_alone() {
    let asks = Arc::new(std::sync::Mutex::new(Vec::new()));
    let machine = Machine::with_gate(gate(counted("beta", asks.clone())));
    for path in ["/beta/docs/README.md", "/beta/api/pending", "/beta"] {
        assert_eq!(
            asked_of(&machine, &asks, LAPTOP, path).await,
            ["beta"],
            "{path}"
        );
    }
    // A board that has not added her is asked once, then every other, so she is answered as for
    // a project the server does not hold.
    assert_eq!(
        asked_of(&machine, &asks, LAPTOP, "/alpha/docs/README.md").await,
        ["alpha", "beta"]
    );
    // The project list, the machine's front, a path no project's id begins and a respelled id ask
    // every board.
    for path in ["/api/projects", "/", "/tasks", "/%62eta/tasks"] {
        assert_eq!(
            asked_of(&machine, &asks, LAPTOP, path).await,
            ["alpha", "beta"],
            "{path}"
        );
    }
    // The person at the machine is on every board, and no board is asked of her.
    for path in ["/beta/docs/README.md", "/api/projects"] {
        assert!(
            asked_of(&machine, &asks, PHONE, path).await.is_empty(),
            "{path}"
        );
    }
}

/// A board that added a visitor and names them lists them, of its project, what waits on them: its
/// count and first card are of the cards they decide, as its row on the machine's page is; the
/// owner's phone, as the person at the machine, is listed every card.
#[tokio::test]
async fn the_list_counts_a_visitor_the_cards_they_decide_and_the_machine_every_card() {
    use crate::testing::board::{Fixture, Seed};
    use symphony_board::people::Power;
    use symphony_board::people::ownership::Place;
    let machine = Machine::with_gate(gate(ana_on("alpha")));
    let root = crate::testing::repository_holding(
        &machine.tree,
        "alpha",
        "docs/engineering/build.md",
        "# Build\n\n- [ ] **new-build** — Which build tool?\n",
    );
    machine.tree.write(
        "alpha/docs/design/palette.md",
        "# Palette\n\n- [ ] **new-palette** — Which palette?\n",
    );
    crate::testing::git(&root, &["add", "."]);
    crate::testing::git(&root, &["commit", "-qm", "Design's document"]);
    let board = Fixture::new(Seed::default());
    for (name, group) in [("ana", "design"), ("bo", "engineering")] {
        let store = &board.store;
        (store.add_group(group, &[Power::Speak, Power::Decide], "owner")).unwrap();
        (store.add_person(name, &[group.into()], &[], "owner")).unwrap();
        let folder = Place::Folder(format!("docs/{group}"));
        store.map_place(&folder, group, "owner").unwrap();
    }
    let hub = Arc::new(crate::live::Hub::new(&root, &root.join("build/client")));
    hub.refresh_page("", &root).unwrap();
    hub.people_read(&board.store);
    assert!(machine.projects.starter.api.set(hub).is_ok());
    let listed = |body: &[u8]| {
        let list: symphony_api::ProjectList = serde_json::from_slice(body).unwrap();
        let alpha = list.projects.into_iter().find(|p| p.id == "alpha").unwrap();
        (alpha.waiting, alpha.next.map(|card| card.href))
    };
    let projects = ("GET", "/api/projects", &[][..], "");
    let (_, _, body) = exchange(&machine.projects, LAPTOP, projects).await;
    assert_eq!(
        listed(&body),
        (
            Some(1),
            Some("/alpha/docs/design/palette.md#new-palette".into())
        )
    );
    let (_, _, page) = exchange(&machine.projects, LAPTOP, ("GET", "/", &[], "")).await;
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("1 decision card waiting on you"), "{page}");
    let (_, _, body) = exchange(&machine.projects, PHONE, projects).await;
    assert_eq!(listed(&body).0, Some(2), "the person at the machine's");
    let (_, _, page) = exchange(&machine.projects, PHONE, ("GET", "/", &[], "")).await;
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("2 decision cards waiting on you"), "{page}");
}

/// A gate holding the sign-in service's key in the machine folder `keys` and asking `roster` of
/// anyone but the machine.
fn signed_gate(keys: &crate::testing::Tree, roster: Roster) -> Gate {
    crate::testing::SignIn::new(1).named_in(&keys.path(""));
    gate(nobody())
        .with_machine(keys.path(""))
        .with_roster(roster)
}

/// A roster on which the board of the project folder `id` has added the person Google signs in as
/// `ana-sub`, as Ana, and no board anyone else.
fn signed_ana_on(id: &'static str) -> Roster {
    Arc::new(move |root: &Path, visitor: &Visitor| {
        let her = matches!(visitor, Visitor::Signed(signed)
            if signed.service == "google" && signed.subject == "ana-sub");
        (her && root.ends_with(id)).then(ana)
    })
}

/// What `projects`' server answers a GET of `path` from `client`, naming `host`, with the word
/// `word` when one is given: status and body.
async fn signed_get(
    projects: &Arc<Projects>,
    client: &str,
    host: &str,
    path: &str,
    words: &[&str],
) -> (StatusCode, String) {
    let mut request = Request::get(path).header(header::HOST, host);
    for word in words {
        request = request.header(symphony::signed_in::HEADER, *word);
    }
    let response = router(projects.clone())
        .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// Through the one server, a signed visitor reaches the board that added them under the service's
/// name, from outside the tailnet, and no other: the list names that board alone, and nothing of
/// the machine's own. A word naming nobody is answered as for a project the server does not hold,
/// and one that does not verify is refused before any route.
#[tokio::test]
async fn a_signed_visitor_reaches_the_board_that_added_them_and_no_other_through_the_one_server() {
    use crate::testing::SignIn;
    let keys = crate::testing::Tree::new();
    let machine = Machine::with_gate(signed_gate(&keys, signed_ana_on("beta")));
    let service = SignIn::new(1);
    let ana_word = service.word("google", "ana-sub");
    const AWAY: &str = "10.0.0.5:1";
    let (status, body) = signed_get(
        &machine.projects,
        AWAY,
        SignIn::HOST,
        "/beta/docs/README.md",
        &[&ana_word],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = signed_get(
        &machine.projects,
        AWAY,
        SignIn::HOST,
        "/api/projects",
        &[&ana_word],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list: symphony_api::ProjectList = serde_json::from_str(&body).unwrap();
    let ids: Vec<_> = (list.projects.iter())
        .map(|listed| listed.id.as_str())
        .collect();
    assert_eq!(ids, ["beta"]);
    assert_eq!(list.installation, None, "the machine's own is not hers");
    let (status, body) = signed_get(
        &machine.projects,
        AWAY,
        SignIn::HOST,
        "/alpha/docs/README.md",
        &[&ana_word],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = signed_get(
        &machine.projects,
        AWAY,
        SignIn::HOST,
        symphony_api::FRONT_PATH,
        &[&ana_word],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let nobody = service.word("google", "bob-sub");
    let (status, body) = signed_get(
        &machine.projects,
        AWAY,
        SignIn::HOST,
        "/api/projects",
        &[&nobody],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list: symphony_api::ProjectList = serde_json::from_str(&body).unwrap();
    assert!(list.projects.is_empty(), "{list:?}");

    for words in [
        vec![SignIn::new(2).word("google", "ana-sub")],
        vec![ana_word.clone(), ana_word.clone()],
    ] {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        for client in [AWAY, "127.0.0.1:1"] {
            let (status, body) = signed_get(
                &machine.projects,
                client,
                SignIn::HOST,
                "/beta/docs/README.md",
                &words,
            )
            .await;
            assert_eq!(
                (status, body.as_str()),
                (
                    StatusCode::FORBIDDEN,
                    "Forbidden: this request's signed sign-in is not taken\n"
                ),
                "{client}"
            );
        }
    }
}

/// The one server's own Host check takes the service's name from a signed request alone.
#[tokio::test]
async fn the_one_server_takes_the_services_name_in_a_host_from_a_signed_request_alone() {
    use crate::testing::SignIn;
    let keys = crate::testing::Tree::new();
    let machine = Machine::with_gate(signed_gate(&keys, signed_ana_on("beta")));
    let (status, body) = signed_get(
        &machine.projects,
        "127.0.0.1:1",
        SignIn::HOST,
        "/api/projects",
        &[],
    )
    .await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            "Forbidden: the Host header does not name this machine\n"
        )
    );
    let word = SignIn::new(1).word("google", "ana-sub");
    let (status, body) = signed_get(
        &machine.projects,
        "127.0.0.1:1",
        "other.example.com",
        "/api/projects",
        &[&word],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = signed_get(
        &machine.projects,
        "127.0.0.1:1",
        SignIn::HOST,
        "/api/projects",
        &[&word],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
