//! What waits on each reader, through the real router (docs/product/analysis/users-and-identity.md,
//! step 4 of "The way from one owner"), by visitors the board named who each hold decide: ana, of
//! design, whose group holds `docs/design`, and bo, of engineering, holding `docs/engineering`, are
//! each answered the cards they decide alone by `/api/board`, `/api/queue`, `/api/postponed`,
//! `/api/pending` and a folder's counts, and sent no other by the feed, while the person at the
//! machine is answered every card, as before. A Later, its Undo and a visit of a card are refused,
//! with nothing written, to a person not of its owner, in its verdict's words, and taken from one
//! who is, and from the person at the machine.

use super::decide_tests::{HOST, MACHINE, as_person, post, read, with_hub};
use crate::front::{Shared, router};
use crate::testing::{Board, LAPTOP, Tree, git, repository_holding};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::StreamExt as _;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use symphony_board::people::Power;
use symphony_board::people::ownership::Place;

const DESIGN: &str = "docs/design/palette.md";
const ENGINEERING: &str = "docs/engineering/build.md";
const PALETTE: &str = "# Palette\n\n<!-- register:start -->\n\
                       - [ ] **new-palette** — Which palette?\n<!-- register:end -->\n";
const BUILD: &str = "# Build\n\n<!-- register:start -->\n\
                     - [ ] **new-build** — Which build tool?\n<!-- register:end -->\n";
/// The words of design's card posted while bo's feed is followed.
const POSTED: &str = "Which brand colour is ours?";

/// A repository holding design's and engineering's documents, each one card waiting.
fn project(tree: &Tree) -> PathBuf {
    let root = repository_holding(tree, "main", DESIGN, PALETTE);
    tree.write(&format!("main/{ENGINEERING}"), BUILD);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "Engineering's document"]);
    root
}

/// The board at `root` naming ana of design and bo of engineering, each group holding speak and
/// decide and mapped to its folder.
fn named(root: &Path) -> Board {
    let board = Board::holding(root, &[]);
    let store = Board::store(root);
    for (name, group) in [("ana", "design"), ("bo", "engineering")] {
        (store.add_group(group, &[Power::Speak, Power::Decide], "owner")).unwrap();
        (store.add_person(name, &[group.into()], &[], "owner")).unwrap();
        let folder = Place::Folder(format!("docs/{group}"));
        store.map_place(&folder, group, "owner").unwrap();
    }
    board
}

/// The server of the project at `root` admitting the laptop as `name`, its hub's page read and
/// the board's people looked at.
fn served(root: &Path, name: &str) -> Arc<Shared> {
    let shared = with_hub(as_person(root, name), root);
    let hub = shared.api.get().unwrap();
    hub.refresh_page("", root).unwrap();
    hub.people_read(&Board::store(root));
    shared
}

/// The slugs of the cards `cards` lists.
fn slugs(cards: &Value) -> Vec<&str> {
    (cards.as_array().unwrap().iter())
        .map(|card| card["slug"].as_str().unwrap())
        .collect()
}

/// A Later's, an Undo's or a visit's body for the card `slug` of `path`.
fn postpone(path: &str, slug: &str, action: &str) -> Value {
    json!({"path": path, "slug": slug, "action": action})
}

#[tokio::test(flavor = "multi_thread")]
async fn each_person_reads_the_cards_they_decide_and_the_person_at_the_machine_every_card() {
    let tree = Tree::new();
    let root = project(&tree);
    let _board = named(&root);
    let bo = served(&root, "bo");
    let (status, board) = read(&bo, LAPTOP, "/api/board").await;
    assert_eq!(status, StatusCode::OK, "{board}");
    assert_eq!(slugs(&board["next"]), ["new-build"]);
    assert_eq!(board["counts"]["waiting"], 1);
    assert_eq!(board["counts"]["docs_waiting"], 1);
    assert_eq!(board["progress"]["decisions_waiting"], 1);
    let (_, queue) = read(&bo, LAPTOP, "/api/queue").await;
    assert_eq!(slugs(&queue["waiting"]), ["new-build"]);
    let (_, pending) = read(&bo, LAPTOP, "/api/pending").await;
    assert_eq!(
        pending["items"],
        json!([{"path": ENGINEERING, "slug": "new-build",
                "href": format!("/{ENGINEERING}#new-build")}])
    );
    let (_, folder) = read(&bo, LAPTOP, "/api/folder?path=docs").await;
    let waiting = |folder: &Value, path: &str| {
        (folder["entries"].as_array().unwrap().iter())
            .find(|entry| entry["path"].as_str().unwrap().ends_with(path))
            .map(|entry| entry["waiting"].clone())
            .unwrap()
    };
    assert_eq!(
        (
            waiting(&folder, "docs/design"),
            waiting(&folder, "docs/engineering")
        ),
        (json!(0), json!(1))
    );
    // The person at the machine reads every card, as before.
    let (_, board) = read(&bo, MACHINE, "/api/board").await;
    assert_eq!(board["counts"]["waiting"], 2);
    let (_, queue) = read(&bo, MACHINE, "/api/queue").await;
    let mut all = slugs(&queue["waiting"]);
    all.sort_unstable();
    assert_eq!(all, ["new-build", "new-palette"]);
    let (_, folder) = read(&bo, MACHINE, "/api/folder?path=docs").await;
    assert_eq!(waiting(&folder, "docs/design"), 1);
    // What is postponed is the board's one order, each reader told of theirs.
    let (status, body) = post(
        &bo,
        LAPTOP,
        "/api/postpone",
        postpone(ENGINEERING, "new-build", "later"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    bo.api.get().unwrap().refresh_page("", &root).unwrap();
    let (_, postponed) = read(&bo, LAPTOP, "/api/postponed").await;
    assert_eq!(postponed, json!(["new-build"]));
    let ana = served(&root, "ana");
    let (_, postponed) = read(&ana, LAPTOP, "/api/postponed").await;
    assert_eq!(postponed, json!([]), "not hers");
    let (_, queue) = read(&ana, LAPTOP, "/api/queue").await;
    assert_eq!(slugs(&queue["waiting"]), ["new-palette"]);
    let (_, postponed) = read(&ana, MACHINE, "/api/postponed").await;
    assert_eq!(postponed, json!(["new-build"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_its_undo_and_a_visit_are_refused_to_one_not_of_the_cards_owner_and_write_nothing()
{
    let tree = Tree::new();
    let root = project(&tree);
    let _board = named(&root);
    let bo = served(&root, "bo");
    let order = symphony::postpone::order_file(&root);
    for action in ["later", "undo", "seen"] {
        let (status, body) = post(
            &bo,
            LAPTOP,
            "/api/postpone",
            postpone(DESIGN, "new-palette", action),
        )
        .await;
        assert_eq!(
            (status, body),
            (
                StatusCode::FORBIDDEN,
                json!({"error": "Forbidden: bo is no member of the group design, whose it is, so \
                                 may not decide it: nothing was written"})
                .to_string()
            ),
            "{action}"
        );
        assert!(!order.exists(), "{action}: nothing was written");
    }
    let ana = served(&root, "ana");
    let (status, body) = post(
        &ana,
        LAPTOP,
        "/api/postpone",
        postpone(DESIGN, "new-palette", "later"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &ana,
        LAPTOP,
        "/api/postpone",
        postpone(DESIGN, "new-palette", "undo"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The person at the machine takes a Later of any card.
    let (status, body) = post(
        &bo,
        MACHINE,
        "/api/postpone",
        postpone(DESIGN, "new-palette", "later"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ana.api.get().unwrap().refresh_page("", &root).unwrap();
    let (_, postponed) = read(&ana, LAPTOP, "/api/postponed").await;
    assert_eq!(postponed, json!(["new-palette"]));
}

/// What the feed of `shared`'s main page sends `client` from `since` up to its version now, each
/// Server-Sent Event's text in turn.
async fn fed(shared: &Arc<Shared>, client: &str, since: u64) -> Vec<String> {
    let until = shared.api.get().unwrap().cursor("").unwrap();
    let app = router(shared.clone()).layer(axum::extract::connect_info::MockConnectInfo(
        client.parse::<std::net::SocketAddr>().unwrap(),
    ));
    let request = Request::get(format!("/api/feed?since={since}"))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut frames = Vec::new();
    let mut text = String::new();
    let wait = test_time::backstop(Duration::from_secs(10));
    tokio::time::timeout(wait, async {
        loop {
            let chunk = body.next().await.expect("the feed goes on").unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some((frame, rest)) = text.split_once("\n\n") {
                let frame = frame.to_string();
                text = rest.to_string();
                let last = frame.contains(&format!("id: {until}\n"));
                if frame.starts_with(&format!("event: {}\n", symphony_api::FEED_EVENT)) {
                    frames.push(frame);
                }
                if last {
                    return;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the feed stalled before {until}; {}",
            test_time::Load::now()
        )
    });
    frames
}

#[tokio::test(flavor = "multi_thread")]
async fn the_feed_sends_a_person_no_byte_of_a_card_they_do_not_decide_and_the_machine_all() {
    let tree = Tree::new();
    let root = project(&tree);
    let _board = named(&root);
    let bo = served(&root, "bo");
    let hub = bo.api.get().unwrap();
    let (_, board) = read(&bo, LAPTOP, "/api/board").await;
    let since = board["version"].as_u64().unwrap();
    tree.write(
        &format!("main/{DESIGN}"),
        &PALETTE.replace(
            "<!-- register:end -->",
            &format!("- [ ] **brand-new** — {POSTED}\n<!-- register:end -->"),
        ),
    );
    hub.refresh_page("", &root).unwrap();
    let his = fed(&bo, LAPTOP, since).await.concat();
    for leak in ["brand-new", POSTED, "palette", "\"region\":\"counts\""] {
        assert!(!his.contains(leak), "{leak} in {his}");
    }
    let machine = fed(&bo, MACHINE, since).await.concat();
    assert!(
        machine.contains("brand-new") && machine.contains(POSTED),
        "{machine}"
    );
    // Each event is sent him at its version, so his cursor stays the page's.
    let until = hub.cursor("").unwrap();
    assert!(his.contains(&format!("id: {until}\n")), "{his}");
    // Ana decides it: hers says it.
    let ana = served(&root, "ana");
    let (_, board) = read(&ana, LAPTOP, "/api/board").await;
    assert_eq!(slugs(&board["next"]), ["new-palette", "brand-new"]);
}
