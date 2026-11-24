//! Who a save is from, through the real router: the person the local host's gate admits
//! ([`crate::gate::Gate::person`]), never one the payload names. A test host's gate admitting
//! another person sees that person's name on the card's line; this machine's own gate, the person
//! at it, whose line reads `owner` as it always has.

use crate::front::{Shared, router};
use crate::testing::{Board, LAPTOP, Tree, laptop_with, open_gate, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use symphony_board::people::{Holder, Power};
use symphony_plugin::execution::{Principal, PrincipalKind};
use symphony_plugin::{Actor, Sender};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8123";
const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n";

/// The card's question saved through the router of `shared`, its payload naming another actor,
/// which no save takes: the page as it then reads.
async fn asked(shared: &Arc<Shared>, root: &Path) -> String {
    let (status, body) = asked_from(shared, "127.0.0.1:5000").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap()
}

/// The card's question saved through the router of `shared` from `client`: the status and body
/// it is answered.
async fn asked_from(shared: &Arc<Shared>, client: &str) -> (StatusCode, String) {
    let app = router(shared.clone()).layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
    let payload = r#"{"path":"docs/TOPIC.md","slug":"one","state":"question","note":"Which one?","actor":"mallory"}"#;
    let request = Request::post("/api/decision")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// A gate whose whois names a colleague's laptop at [`LAPTOP`], under a login other than this
/// machine's, and whose roster adds that login to the board at `added` alone, as Ana.
fn laptop_gate(added: &Path, ana: &Actor) -> crate::gate::Gate {
    let (added, ana) = (added.to_path_buf(), ana.clone());
    laptop_with(Arc::new(
        move |root: &Path, visitor: &crate::gate::Visitor| {
            let her = matches!(visitor, crate::gate::Visitor::Login(login) if login.name == "ana@example.com");
            (her && root == added).then(|| ana.clone())
        },
    ))
}

/// A visitor under another login whom the board added, holding decide, saves as the actor the board
/// names them, never as the person at the machine; on a board that has not added them, a server of
/// its own refuses them before any route, and nothing is written.
#[tokio::test(flavor = "multi_thread")]
async fn a_visitor_a_board_added_saves_as_themselves_and_one_it_did_not_writes_nothing() {
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        Principal::new(
            PrincipalKind::Human,
            "tailscale".into(),
            "ana@example.com".into(),
        ),
    );
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = Board::holding(&root, &[("ana", &[Power::Decide])]);
    let shared = Shared::new(1, 8123, &root, None, laptop_gate(&root, &ana));
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert!(
        page.lines()
            .any(|l| l.ends_with("] ana · question — Which one?")),
        "{page}"
    );
    assert!(!page.contains("owner ·"), "{page}");

    let elsewhere = Tree::new();
    let other = repository_holding(&elsewhere, "main", "docs/TOPIC.md", PAGE);
    let shared = Shared::new(1, 8123, &other, None, laptop_gate(&root, &ana));
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            "Forbidden: the Symphony answers only this machine and its tailnet\n"
        )
    );
    assert_eq!(
        std::fs::read_to_string(other.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );
}

/// A person the board gives speak and no decide is refused a decision with nothing written, the
/// refusal naming the power; given decide, the same save is hers; and a comment, which speaks to
/// the agents, is refused her while the board holds no words for agents from anyone.
#[tokio::test(flavor = "multi_thread")]
async fn a_person_is_refused_each_write_whose_power_she_lacks_and_nothing_is_written() {
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        symphony_board::workflow::person("ana"),
    );
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = Board::holding(&root, &[("ana", &[Power::Speak])]);
    let shared = Shared::new(1, 8123, &root, None, laptop_gate(&root, &ana));
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden: ana may not decide on this board"}"#
        )
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );
    let comment = r#"{"path":"docs/TOPIC.md","slug":"one","text":"Wider?"}"#;
    let (status, body) = posted(&shared, "/api/comment", comment).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden: this board holds no words for agents from ana yet"}"#
        )
    );
    // Powers are read as each write arrives: given decide now, the same save is hers.
    let store = Board::store(&root);
    store
        .grant(Holder::Person("ana"), &[Power::Decide], "owner")
        .unwrap();
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert!(
        page.lines()
            .any(|l| l.ends_with("] ana · question — Which one?")),
        "{page}"
    );
    // And once she holds it no more, the next is refused again.
    store
        .revoke(Holder::Person("ana"), &[Power::Decide], "owner")
        .unwrap();
    let (status, _) = asked_from(&shared, LAPTOP).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// The machine's own roster ([`crate::roster::of`]): Ana's login is her profile's sign-in in the
/// machine folder, and the board at the project gives her decide, so her save is hers, shown as
/// her profile shows her; with her power revoked she reaches the board no more.
#[tokio::test(flavor = "multi_thread")]
async fn the_machines_roster_puts_a_profiles_login_on_a_board_that_gives_her_a_power() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let machine = tree.root.join("machine");
    symphony_board::people::Profiles::change(&machine, |profiles| {
        profiles.people.push(symphony_board::people::Profile {
            name: "ana".into(),
            display: "Ana Lima".into(),
            signins: vec![symphony_board::people::Signin::parse(
                "tailscale:ana@example.com",
            )?],
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
    let _board = Board::holding(&root, &[("ana", &[Power::Decide])]);
    let gate = laptop_with(crate::roster::of(machine));
    let visitor = crate::gate::Visitor::Login(crate::gate::Login {
        id: 12,
        name: "ana@example.com".into(),
        display: "whoever Tailscale says".into(),
    });
    let ana = gate.member(&root, &visitor).expect("on the board");
    assert_eq!(
        (ana.name(), ana.sender.display.as_str()),
        ("ana", "Ana Lima")
    );
    let shared = Shared::new(1, 8123, &root, None, gate);
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert!(
        page.lines()
            .any(|l| l.ends_with("] ana · question — Which one?")),
        "{page}"
    );
    let store = Board::store(&root);
    store
        .revoke(Holder::Person("ana"), &[Power::Decide], "owner")
        .unwrap();
    assert!(
        laptop_with(crate::roster::of(tree.root.join("machine")))
            .member(&root, &visitor)
            .is_none()
    );
}

/// A person's write on a project whose board cannot be read is refused as the board unreached,
/// 503, and nothing is written: no failed read lets a write through.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_write_where_the_board_cannot_be_read_is_refused_and_writes_nothing() {
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        symphony_board::workflow::person("ana"),
    );
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let shared = Shared::new(1, 8123, &root, None, laptop_gate(&root, &ana));
    let (status, body) = asked_from(&shared, LAPTOP).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        body.contains("the task board cannot be reached to read who may write"),
        "{body}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );
}

/// `body` posted at `route` through the router of `shared` from Ana's laptop: the status and body.
async fn posted(shared: &Arc<Shared>, route: &str, body: &str) -> (StatusCode, String) {
    let app = router(shared.clone()).layer(MockConnectInfo(LAPTOP.parse::<SocketAddr>().unwrap()));
    let request = Request::post(route)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn a_save_is_the_person_the_gate_admits_and_the_machines_own_reads_owner() {
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec!["ana-l".into()]),
        Principal::new(PrincipalKind::Human, "test-host".into(), "u-ana".into()),
    );
    for (gate, by) in [(open_gate().admitting(ana), "ana"), (open_gate(), "owner")] {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
        let shared = Shared::new(1, 8123, &root, None, gate);
        let page = asked(&shared, &root).await;
        let line = format!("] {by} · question — Which one?");
        assert!(page.lines().any(|l| l.ends_with(&line)), "{page}");
        assert!(!page.contains("mallory"), "{page}");
    }
}

#[tokio::test]
async fn a_save_hosted_admission_let_in_is_refused_here_and_writes_nothing() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let shared = Shared::new(1, 8123, &root, None, open_gate());
    let app = router(shared.clone()).layer(MockConnectInfo(
        "127.0.0.1:5000".parse::<SocketAddr>().unwrap(),
    ));
    let payload = r#"{"path":"docs/TOPIC.md","slug":"one","state":"approved"}"#;
    let mut request = Request::post("/api/decision")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap();
    request
        .extensions_mut()
        .insert(crate::hosted::tests::admitted().await);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("its host's to dispatch"),
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );
}

/// The server's own code of its writes and attempt controls: what may no longer stand where the
/// gate's person is in hand.
#[test]
fn no_server_write_takes_the_local_owner_where_the_gates_person_is_in_hand() {
    let actorless = [
        "local_owner()",
        "local_actor()",
        "saves.post(",
        "saves.post_with_order(",
        "saves.apply_workflow(",
        "Asker::Local =>",
        "Asker::Local;",
    ];
    for (file, code) in [
        ("write.rs", include_str!("../write.rs")),
        ("attempts.rs", include_str!("../attempts.rs")),
        ("api.rs", include_str!("../api.rs")),
    ] {
        let code = code.split("\n#[cfg(test)]\nmod tests").next().unwrap();
        assert!(code.lines().count() > 100, "{file} read whole");
        for (n, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for word in actorless {
                assert!(!line.contains(word), "{file}:{}: {word}: {line}", n + 1);
            }
        }
    }
}
