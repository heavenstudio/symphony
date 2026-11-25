//! Whose a card or a task is and its hand-over, through the real router, by a visitor the board
//! named (docs/workflow/TASKS.md, "Which group a thing belongs to"): ana, of design, reads whose a
//! card and a task are and whom she may hand each to, and hands each on; a thing of a group she is
//! no member of is refused her with nothing written; a card handed again to the name written on it
//! writes nothing, and one whose file is not written leaves no line on the board, as one whose save
//! for delivery by pull request is not kept leaves nothing for a restart to take up; a hosted
//! visitor is offered no one to hand to; and on a board that names nobody the person at the machine
//! reads no owner at all.

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
               "hand_to": ["engineering", "ana", "bo", "owner"], "decides": false})
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
        body.contains(
            "it is bo's, so ana may not hand it on: only bo or the person at the machine does"
        ),
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

/// A card handed to the name already written on it writes nothing, on the page or the board, and
/// says it was a repeat; a hosted visitor, who saves nothing here, reads whose it is and is
/// offered no one to hand it to.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_handed_again_to_its_name_writes_nothing_and_a_hosted_visitor_is_offered_no_one() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = named(&root);
    let store = Board::store(&root);
    store
        .map_place(&Place::Folder("docs".into()), "design", "owner")
        .unwrap();
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let to_ana = json!({"path": "docs/TOPIC.md", "slug": "one", "to": "ana"});
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", to_ana.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    shared.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert!(page.contains("**one** — *For:* ana. proposal."), "{page}");
    let lines = store.ownership_log().unwrap().len();
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", to_ana).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"ok": true, "repeat": true})
    );
    shared.saves.get().unwrap().close();
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        page
    );
    assert_eq!(store.ownership_log().unwrap().len(), lines);
    let mut request = Request::get("/api/card-owners?path=docs/TOPIC.md")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(crate::hosted::tests::admitted().await);
    let (status, body) = sent(&shared, LAPTOP, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let owners: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        owners["cards"]["one"],
        json!({"name": "ana", "kind": "person", "by": "named", "said": "named on it",
               "decides": false})
    );
}

/// A card's hand-over whose file cannot be written (the page made read-only) keeps nothing: the
/// board's line is taken back, the page reads as it did, and whose the card is is unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_handed_whose_file_is_not_written_leaves_no_line_on_the_board() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
    let _board = named(&root);
    let store = Board::store(&root);
    store
        .map_place(&Place::Folder("docs".into()), "design", "owner")
        .unwrap();
    let lines = store.ownership_log().unwrap();
    let page = root.join("docs/TOPIC.md");
    let set = |mode: u32| {
        std::fs::set_permissions(&page, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set(0o444);
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let hand = json!({"path": "docs/TOPIC.md", "slug": "one", "to": "engineering"});
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", hand).await;
    set(0o644);
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("ermission denied"), "{body}");
    assert_eq!(store.ownership_log().unwrap(), lines, "the line taken back");
    assert_eq!(std::fs::read_to_string(&page).unwrap(), PAGE);
    let (_, owners) = read(&shared, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(
        (
            &owners["cards"]["one"]["name"],
            &owners["cards"]["one"]["by"]
        ),
        (&json!("design"), &json!("map"))
    );
}

/// The branches of the repository at `root`, one full name a line, sorted.
fn heads(root: &Path) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["for-each-ref", "--format=%(refname)", "refs/heads"])
            .current_dir(root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8_lossy(&done.stdout).into_owned()
}

/// A card's hand-over in a project that delivers by pull request writes no file: it is kept as a
/// save for delivery (`symphony::commit::pending`). One that cannot be kept (the journal that
/// holds those saves made read-only) keeps nothing. The board's line is taken back, no save waits
/// for delivery, the page reads as it did, and a restarted server has nothing to take up again:
/// it starts no committer, makes no branch, and the card is still its first group's by the map.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_handed_whose_save_is_not_kept_for_delivery_leaves_nothing_to_take_up_again() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    let root = super::pending_tests::delivering(&tree);
    let _board = named(&root);
    let store = Board::store(&root);
    store
        .map_place(&Place::Folder("docs".into()), "design", "owner")
        .unwrap();
    let (lines, main, branches) = (
        store.ownership_log().unwrap(),
        super::pending_tests::tip(&root, "main"),
        heads(&root),
    );
    let journal = (root.join(".git").join(symphony_board::folders::SERVER))
        .join("verdict-publications");
    std::fs::create_dir_all(&journal).unwrap();
    let set = |mode: u32| {
        std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set(0o555);
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    let hand = json!({"path": "docs/TOPIC.md", "slug": "one", "to": "engineering"});
    let (status, body) = post(&shared, LAPTOP, "/api/card-hand", hand).await;
    set(0o755);
    shared.saves.get().unwrap().close();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("ermission denied"), "{body}");
    assert_eq!(store.ownership_log().unwrap(), lines, "the line taken back");
    let kept: Vec<_> = std::fs::read_dir(&journal).unwrap().collect();
    assert!(kept.is_empty(), "nothing kept, not even in part: {kept:?}");
    assert!(!symphony::commit::pending::kept(&root));
    assert_eq!(symphony::commit::pending::note(&root, "docs/TOPIC.md"), None);
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );

    // The server again, as it starts: what follows its first read takes up the saves kept, and
    // there are none.
    drop(shared);
    let shared = Shared::new(1, 8123, &root, None, ana_at(&root));
    crate::api::settling(&shared)();
    assert!(shared.saves.get().is_none(), "nothing taken up again");
    assert_eq!(super::pending_tests::tip(&root, "main"), main, "main moved");
    assert_eq!(heads(&root), branches, "no branch made for a save");
    assert_eq!(store.ownership_log().unwrap(), lines);
    let (_, owners) = read(&shared, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(
        (
            &owners["cards"]["one"]["name"],
            &owners["cards"]["one"]["by"]
        ),
        (&json!("design"), &json!("map"))
    );
}
