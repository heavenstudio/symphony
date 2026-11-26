//! A comment held on a document in the review queue (docs/product/analysis/users-and-identity.md,
//! card `held-on-document-waits-where`, B), through the real router: cy, of design, who may speak
//! and may not direct agents, leaves a question on design's document. ana, of design, who may
//! direct agents, is the one its notice tells: her queue lists it, linking to it on its page, her
//! board counts it, and her feed moves her counts. Its author, dee of design, who may not pass it
//! on, and bo of engineering, who may but is not told while design has a passer, are shown nothing
//! of it, by a read or by the feed. The person at the machine is shown it, and a hosted visitor,
//! who passes nothing on, is not. Passed on, it leaves every queue.

use super::decide_tests::{HOST, MACHINE, as_person, post, read};
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
const PALETTE: &str = "# Palette\n\nWarm or cold lamps.\n\n<!-- register:start -->\n\
                       - [ ] **new-palette** — Which palette?\n<!-- register:end -->\n";
const BUILD: &str = "# Build\n\n<!-- register:start -->\n\
                     - [ ] **new-build** — Which build tool?\n<!-- register:end -->\n";
/// A word of cy's question, which no reader's queue or feed carries.
const WORDS: &str = "quartzlamp";

/// A repository holding design's and engineering's documents, each one card waiting.
fn project(tree: &Tree) -> PathBuf {
    let root = repository_holding(tree, "main", DESIGN, PALETTE);
    tree.write(&format!("main/{ENGINEERING}"), BUILD);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "Engineering's document"]);
    root
}

/// The board at `root`: design and engineering, each speaking and deciding and mapped to its
/// folder; ana of design and bo of engineering directing agents, cy and dee of design not.
fn named(root: &Path) -> Board {
    let board = Board::holding(root, &[]);
    let store = Board::store(root);
    for group in ["design", "engineering"] {
        (store.add_group(group, &[Power::Speak, Power::Decide], "owner")).unwrap();
        let folder = Place::Folder(format!("docs/{group}"));
        store.map_place(&folder, group, "owner").unwrap();
    }
    for (name, group, powers) in [
        ("ana", "design", &[Power::DirectAgents][..]),
        ("bo", "engineering", &[Power::DirectAgents]),
        ("cy", "design", &[]),
        ("dee", "design", &[]),
    ] {
        (store.add_person(name, &[group.into()], powers, "owner")).unwrap();
    }
    board
}

/// A server admitting the laptop as each of `names`, all sharing one hub, its pages read and
/// the board's people looked at, so each reader's feed counts the same versions.
fn readers(root: &Path, names: &[&str]) -> (Vec<Arc<Shared>>, Arc<crate::live::Hub>) {
    let hub = Arc::new(crate::live::Hub::new(root, &root.join("build/client")));
    hub.refresh_page("", root).unwrap();
    hub.people_read(&Board::store(root));
    let servers = (names.iter())
        .map(|name| {
            let shared = as_person(root, name);
            assert!(shared.api.set(Arc::clone(&hub)).is_ok());
            shared
        })
        .collect();
    (servers, hub)
}

/// cy's question after the paragraph on line 3 of design's document.
fn question() -> Value {
    json!({"path": DESIGN, "kind": "question", "after_line": 3,
        "text": format!("Too many {WORDS}s\nShow the thirty that matter.")})
}

/// What `client` of `shared` reads at `uri`, its status OK.
async fn ok(shared: &Arc<Shared>, client: &str, uri: &str) -> Value {
    let (status, value) = read(shared, client, uri).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

/// What a hosted visitor the fixture host let in reads at `uri` of `shared`.
async fn hosted(shared: &Arc<Shared>, uri: &str) -> Value {
    let app = router(shared.clone()).layer(axum::extract::connect_info::MockConnectInfo(
        MACHINE.parse::<std::net::SocketAddr>().unwrap(),
    ));
    let mut request = Request::get(uri)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    (request.extensions_mut()).insert(crate::hosted::tests::admitted().await);
    let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The feed of `shared`'s main page as `client` is sent it from `since` up to its version now,
/// each Server-Sent Event's text in turn.
async fn fed(shared: &Arc<Shared>, client: &str, since: u64) -> String {
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
    let mut text = String::new();
    let wait = test_time::backstop(Duration::from_secs(10));
    tokio::time::timeout(wait, async {
        while !text.contains(&format!("id: {until}\n")) {
            let chunk = body.next().await.expect("the feed goes on").unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the feed stalled before {until}; {}",
            test_time::Load::now()
        )
    });
    text
}

/// What the queues an agent reads say of the project at `root`: its open comments
/// (`just symphony-status --open`), its cards (`--json`) and `symphony-cli triage queue`.
fn agents_queues(root: &Path) -> Vec<String> {
    let root = root.to_str().unwrap();
    let said = |run: fn(&[String], &mut dyn std::io::Write, &mut dyn std::io::Write) -> i32,
                args: &[&str]| {
        let args: Vec<String> = ["--root", root]
            .iter()
            .chain(args)
            .map(|a| a.to_string())
            .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            run(&args, &mut out, &mut err),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
        String::from_utf8(out).unwrap()
    };
    vec![
        said(symphony::cli::status_main, &["--open"]),
        said(symphony::cli::status_main, &["--json"]),
        said(symphony::cli::triage_main, &["queue", "10"]),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_comment_waits_in_the_queue_of_those_its_notice_tells_until_it_is_passed_on() {
    let tree = Tree::new();
    let root = project(&tree);
    let _board = named(&root);
    let (servers, hub) = readers(&root, &["ana", "bo", "cy", "dee"]);
    let [ana, bo, cy, dee] = &servers[..] else {
        unreachable!()
    };
    let since = ok(ana, LAPTOP, "/api/board").await["version"]
        .as_u64()
        .unwrap();
    let (status, body) = post(cy, LAPTOP, "/api/comment", question()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = serde_json::from_str::<Value>(&body).unwrap()["held"]
        .as_i64()
        .unwrap();
    let href = format!("/{DESIGN}#held-{id}");
    let row = json!([{"id": id, "href": href, "doc_title": "Palette", "author": "cy"}]);
    // ana, of design and directing agents, is the one its notice tells.
    assert_eq!(ok(ana, LAPTOP, "/api/queue").await["held"], row);
    assert_eq!(ok(ana, LAPTOP, "/api/board").await["counts"]["held"], 1);
    // Its author, one who may not pass it on, and one of another group are shown nothing of it,
    // and their cards are as before.
    for (shared, name, card) in [(cy, "cy", "new-palette"), (dee, "dee", "new-palette")]
        .into_iter()
        .chain([(bo, "bo", "new-build")])
    {
        let queue = ok(shared, LAPTOP, "/api/queue").await;
        assert_eq!(queue.get("held"), None, "{name}: {queue}");
        assert_eq!(queue["waiting"][0]["slug"], card, "{name}");
        let board = ok(shared, LAPTOP, "/api/board").await;
        assert_eq!(board["counts"].get("held"), None, "{name}: {board}");
        assert_eq!(board["counts"]["waiting"], 1, "{name}");
    }
    // The person at the machine may pass every one on, and is shown it.
    assert_eq!(ok(ana, MACHINE, "/api/queue").await["held"], row);
    assert_eq!(ok(ana, MACHINE, "/api/board").await["counts"]["held"], 1);
    // A hosted visitor passes none on: the queue and the board as the machine's, but it.
    let queue = hosted(ana, "/api/queue").await;
    assert_eq!(queue.get("held"), None, "{queue}");
    assert_eq!(queue["waiting"].as_array().unwrap().len(), 2);
    let board = hosted(ana, "/api/board").await;
    assert_eq!(board["counts"].get("held"), None, "{board}");
    // No reader's queue carries its words, the person at the machine's included.
    let machine = ok(ana, MACHINE, "/api/queue").await.to_string();
    assert!(!machine.contains(WORDS), "{machine}");

    // The feed moves ana's counts, and tells the others only that something held moved.
    let hers = fed(ana, LAPTOP, since).await;
    assert!(hers.contains(r#""region":"held""#), "{hers}");
    assert!(hers.contains(r#""held":1"#), "her count: {hers}");
    for (shared, name) in [(cy, "cy"), (dee, "dee"), (bo, "bo")] {
        let theirs = fed(shared, LAPTOP, since).await;
        assert!(theirs.contains(r#""region":"held","data":{}"#), "{name}");
        for leak in [r#""held":1"#, r#""region":"counts""#, WORDS, "Palette"] {
            assert!(!theirs.contains(leak), "{name}: {leak} in {theirs}");
        }
    }
    let machine = fed(ana, MACHINE, since).await;
    assert!(machine.contains(r#""held":1"#), "{machine}");
    assert!(!machine.contains(WORDS), "{machine}");
    // An agent's queues are the documents': the open comments say none, the status and the
    // triage list the cards, and none says anything of it.
    let [open, status, triage] = &agents_queues(&root)[..] else {
        unreachable!()
    };
    assert_eq!(open.trim(), "no open comments");
    for said in [open, status, triage] {
        assert!(said == open || said.contains("new-palette"), "{said}");
        for leak in [WORDS, "#held-", "\"author\""] {
            assert!(!said.contains(leak), "{leak} in {said}");
        }
    }

    // Passed on, it leaves every queue and count.
    let (status, body) = post(ana, LAPTOP, "/api/document-pass", json!({"id": id})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ana.saves.get().unwrap().close();
    for client in [LAPTOP, MACHINE] {
        assert_eq!(ok(ana, client, "/api/queue").await.get("held"), None);
        let board = ok(ana, client, "/api/board").await;
        assert_eq!(board["counts"].get("held"), None, "{board}");
    }
    // The file it was passed into is read again as the watcher would: the queue stays clear.
    hub.refresh_page("", &root).unwrap();
    assert_eq!(ok(ana, MACHINE, "/api/queue").await.get("held"), None);
    // Passed on, its words are the file's, where an agent's open comments read them.
    let open = &agents_queues(&root)[0];
    assert!(open.contains(WORDS), "{open}");
}

#[tokio::test(flavor = "multi_thread")]
async fn where_none_of_its_group_may_pass_it_on_every_person_who_may_is_shown_it() {
    let tree = Tree::new();
    let root = project(&tree);
    let _board = named(&root);
    // Engineering's document: its group's bo may pass it on; dee's question there waits on him.
    let (servers, _hub) = readers(&root, &["ana", "bo", "dee"]);
    let [ana, bo, dee] = &servers[..] else {
        unreachable!()
    };
    let question = json!({"path": ENGINEERING, "slug": "new-build", "text": "Why this one?"});
    let (status, body) = post(dee, LAPTOP, "/api/message", question).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ok(bo, LAPTOP, "/api/board").await["counts"]["held"], 1);
    assert_eq!(ok(ana, LAPTOP, "/api/queue").await.get("held"), None);
    // With bo no longer directing agents, nobody of engineering may: every person who may is told.
    let store = Board::store(&root);
    let bo_himself = symphony_board::people::Holder::Person("bo");
    store
        .revoke(bo_himself, &[Power::DirectAgents], "owner")
        .unwrap();
    let hub = ana.api.get().unwrap();
    hub.people_read(&store);
    let queue = ok(ana, LAPTOP, "/api/queue").await;
    let at = queue["held"][0]["href"].as_str().unwrap();
    assert!(at.starts_with(&format!("/{ENGINEERING}#held-")), "{queue}");
    assert_eq!(queue["held"][0]["author"], "dee");
    assert_eq!(ok(bo, LAPTOP, "/api/queue").await.get("held"), None);
}
