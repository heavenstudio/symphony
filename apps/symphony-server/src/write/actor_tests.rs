//! Who a save is from, through the real router: the person the local host's gate admits
//! ([`crate::gate::Gate::person`]), never one the payload names. A test host's gate admitting
//! another person sees that person's name on the card's line; this machine's own gate, the person
//! at it, whose line reads `owner` as it always has.

use crate::front::{Shared, router};
use crate::testing::{Tree, open_gate, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
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
    crate::gate::Gate::new(
        Arc::new(|address: &str| {
            Ok((address == LAPTOP_IP).then(|| {
                r#"{"Node": {"Name": "laptop"}, "UserProfile": {"ID": 12, "LoginName": "ana@example.com"}}"#.to_string()
            }))
        }),
        Arc::new(std::time::Instant::now),
    )
    .with_status(Arc::new(|| {
        Ok(r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 11}}"#.to_string())
    }))
    .with_roster(Arc::new(move |root: &Path, visitor: &crate::gate::Visitor| {
        let her = matches!(visitor, crate::gate::Visitor::Login(login) if login.name == "ana@example.com");
        (her && root == added).then(|| ana.clone())
    }))
}

const LAPTOP_IP: &str = "100.101.102.104";
const LAPTOP: &str = "100.101.102.104:5000";

/// A visitor under another login whom the board added saves as the actor the board names them,
/// never as the person at the machine; on a board that has not added them, a server of its own
/// refuses them before any route, and nothing is written.
#[tokio::test]
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
