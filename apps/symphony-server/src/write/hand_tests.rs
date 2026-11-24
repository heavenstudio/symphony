//! Whose a card or a task is and its hand-over, through the real router, by a visitor the board
//! named (docs/workflow/TASKS.md, "Which group a thing belongs to"): ana, of design, reads whose a
//! card and a task are and whom she may hand each to, and hands each on; a thing of a group she is
//! no member of is refused her with nothing written; and on a board that names nobody the person
//! at the machine reads no owner at all.

use crate::front::{Shared, router};
use crate::testing::{Board, LAPTOP, Tree, laptop_with, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use symphony_board::people::Power;
use symphony_board::people::ownership::Place;
use symphony_plugin::{Actor, Sender};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8123";
const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n";

/// What `request` is answered through the router of `shared` from `client`: its status and body.
async fn sent(shared: &Arc<Shared>, client: &str, request: Request<Body>) -> (StatusCode, String) {
    let app = router(shared.clone()).layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// A read of `uri` from `client`, its body as JSON where it is.
async fn read(shared: &Arc<Shared>, client: &str, uri: &str) -> (StatusCode, Value) {
    let request = Request::get(uri)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let (status, body) = sent(shared, client, request).await;
    (
        status,
        serde_json::from_str(&body).unwrap_or(Value::String(body)),
    )
}

/// A save of `body` to `route` from `client`.
async fn post(
    shared: &Arc<Shared>,
    client: &str,
    route: &str,
    body: Value,
) -> (StatusCode, String) {
    let request = Request::post(route)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    sent(shared, client, request).await
}

/// The board at `root` naming ana of design and bo of engineering, each holding speak alone.
fn named(root: &Path) -> Board {
    let board = Board::holding(root, &[]);
    let store = Board::store(root);
    for group in ["design", "engineering"] {
        store.add_group(group, &[], "owner").unwrap();
    }
    for (name, group) in [("ana", "design"), ("bo", "engineering")] {
        (store.add_person(name, &[group.into()], &[Power::Speak], "owner")).unwrap();
    }
    board
}

/// A gate admitting the laptop's login to the board at `root` as ana.
fn ana_at(root: &Path) -> crate::gate::Gate {
    let (root, ana) = (
        root.to_path_buf(),
        Actor::new(
            Sender::new("ana", "Ana Lima", vec![]),
            symphony_board::workflow::person("ana"),
        ),
    );
    laptop_with(Arc::new(move |at: &Path, _: &crate::gate::Visitor| {
        (at == root).then(|| ana.clone())
    }))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_reads_whose_a_card_is_and_hands_it_on_and_a_card_of_another_group_is_refused() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = named(&root);
    let store = Board::store(&root);
    store
        .map_place(&Place::Folder("docs".into()), "design", "owner")
        .unwrap();
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let (status, owners) = read(&shared, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(status, StatusCode::OK, "{owners}");
    assert_eq!(
        owners["cards"]["one"],
        json!({"name": "design", "kind": "group", "by": "map", "said": "from the folder docs",
               "hand_to": ["engineering", "ana", "bo", "owner"]})
    );
    let hand = json!({"path": "docs/TOPIC.md", "slug": "one", "to": "engineering"});
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", hand.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert!(
        page.contains("- [ ] **one** — *For:* engineering. proposal."),
        "{page}"
    );
    let handed = store.ownership_log().unwrap();
    let last = handed.last().unwrap();
    assert_eq!(
        (
            last.by.as_str(),
            last.thing.as_deref(),
            last.name.as_deref()
        ),
        ("ana", Some("card one"), Some("engineering"))
    );
    // Engineering's now, whose member ana is not: she reads it, may hand it to no one, and a
    // hand-over back is refused her with nothing written.
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let (_, owners) = read(&shared, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(owners["cards"]["one"]["name"], "engineering");
    assert_eq!(owners["cards"]["one"]["by"], "named");
    assert_eq!(owners["cards"]["one"].get("hand_to"), None);
    let back = json!({"path": "docs/TOPIC.md", "slug": "one", "to": "design"});
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", back).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("ana is no member of the group engineering, whose it is"),
        "{body}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        page
    );
    assert_eq!(store.ownership_log().unwrap().len(), handed.len());
    let (status, body) = read(&shared, LAPTOP, "/api/card-owners?path=docs/NONE.md").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_reads_whose_a_task_is_on_its_page_and_hands_it_on_once() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = named(&root);
    let store = Board::store(&root);
    store
        .create(&symphony_board::NewTask::new("app/x", "X", "owner"))
        .unwrap();
    store
        .map_place(&Place::Area("app".into()), "design", "owner")
        .unwrap();
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let (status, page) = read(&shared, LAPTOP, "/api/task-page?slug=app/x").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["owner"]["name"], "design");
    assert_eq!(page["owner"]["said"], "from the area app");
    assert_eq!(page["owner"]["hand_to"][0], "engineering");
    let hand = json!({"slug": "app/x", "to": "bo"});
    let (status, body) = post(&shared, LAPTOP, "/api/task-hand", hand).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(store.get("app/x").unwrap().group(), Some("bo"));
    let (_, page) = read(&shared, LAPTOP, "/api/task-page?slug=app/x").await;
    assert_eq!(
        (&page["owner"]["name"], &page["owner"]["kind"]),
        (&json!("bo"), &json!("person"))
    );
    assert_eq!(page["owner"].get("hand_to"), None, "bo's, not ana's");
    let again = json!({"slug": "app/x", "to": "design"});
    let (status, body) = post(&shared, LAPTOP, "/api/task-hand", again).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("ana is no member of bo, whose it is"),
        "{body}"
    );
    assert_eq!(store.get("app/x").unwrap().group(), Some("bo"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_board_that_names_nobody_says_no_owner_to_the_person_at_the_machine() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = Board::holding(&root, &[]);
    let store = Board::store(&root);
    store
        .create(&symphony_board::NewTask::new("app/x", "X", "owner"))
        .unwrap();
    let shared = Shared::new(1, 8123, &root, None, crate::testing::open_gate());
    let (status, page) = read(&shared, "127.0.0.1:5000", "/api/task-page?slug=app/x").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page.get("owner"), None);
    let (status, owners) = read(
        &shared,
        "127.0.0.1:5000",
        "/api/card-owners?path=docs/TOPIC.md",
    )
    .await;
    assert_eq!((status, owners), (StatusCode::OK, json!({"cards": {}})));
    let hand = json!({"slug": "app/x", "to": "owner"});
    let (status, body) = post(&shared, "127.0.0.1:5000", "/api/task-hand", hand).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("this board names nobody"), "{body}");
}
