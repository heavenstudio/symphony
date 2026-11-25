//! Who decides a thing (docs/product/analysis/users-and-identity.md, "Who in a group decides",
//! "Who acts on a task", "Who changes settings"), through the real router, by visitors the board
//! named who each hold decide: ana, of design, whose group owns the documents, the area `app` and
//! the Settings page, answers a card, moves, prioritizes, approves and cancels a task, and changes
//! a setting; bo, of engineering, holding decide as well, is refused each with nothing written, in
//! the board's words, reads that he decides none of them and is offered no setting to write; the
//! person at the machine acts as before. On a board that names people and maps nothing, every
//! thing is the person at the machine's, and a person's refusal says the way out.

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

pub(super) const HOST: &str = "127.0.0.1:8123";
/// The person at the machine, as the gate admits loopback.
pub(super) const MACHINE: &str = "127.0.0.1:5000";
const DOC: &str = "docs/TOPIC.md";
const PAGE: &str = "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n- [ ] **two** — \
                    another.\n<!-- register:end -->\n";

/// What `request` is answered through the router of `shared` from `client`: its status and body.
pub(super) async fn sent(shared: &Arc<Shared>, client: &str, request: Request<Body>) -> (StatusCode, String) {
    let app = router(shared.clone()).layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// A read of `uri` from `client`, its body as JSON where it is.
pub(super) async fn read(shared: &Arc<Shared>, client: &str, uri: &str) -> (StatusCode, Value) {
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
pub(super) async fn post(
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

/// The board at `root` naming ana of design and bo of engineering, each group holding speak and
/// decide, with `mapped` places given to design.
fn named(root: &Path, mapped: &[Place]) -> Board {
    let board = Board::holding(root, &[]);
    let store = Board::store(root);
    for group in ["design", "engineering"] {
        (store.add_group(group, &[Power::Speak, Power::Decide], "owner")).unwrap();
    }
    for (name, group) in [("ana", "design"), ("bo", "engineering")] {
        store
            .add_person(name, &[group.into()], &[], "owner")
            .unwrap();
    }
    for place in mapped {
        store.map_place(place, "design", "owner").unwrap();
    }
    board
}

/// Every place design owns here: the documents, the area `app` and the Settings page.
fn designs() -> Vec<Place> {
    vec![
        Place::Folder("docs".into()),
        Place::Area("app".into()),
        Place::Settings,
    ]
}

/// The server of the project at `root`, whose gate admits the laptop's login as `name`, and
/// loopback as the person at the machine.
pub(super) fn as_person(root: &Path, name: &str) -> Arc<Shared> {
    let (at, person) = (
        root.to_path_buf(),
        Actor::new(
            Sender::new(name, name, vec![]),
            symphony_board::workflow::person(name),
        ),
    );
    let gate = laptop_with(Arc::new(move |root: &Path, _: &crate::gate::Visitor| {
        (root == at).then(|| person.clone())
    }));
    Shared::new(1, 8123, root, None, gate)
}

/// `shared` with the hub its reads of a project's pages go through.
fn with_hub(shared: Arc<Shared>, root: &Path) -> Arc<Shared> {
    let hub = crate::live::Hub::new(root, &root.join("build/client"));
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    shared
}

/// A verdict on the card `slug` of [`DOC`].
fn verdict(slug: &str, state: &str) -> Value {
    json!({"path": DOC, "slug": slug, "state": state, "note": ""})
}

/// The refusal a non-member of design meets, in the board's words.
fn not_designs(name: &str) -> String {
    format!(
        "Forbidden: {name} is no member of the group design, whose it is, so may not decide it: \
         nothing was written"
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_answers_a_card_of_their_group_and_a_non_member_holding_decide_is_refused() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root, &designs());
    let bo = as_person(&root, "bo");
    let (_, owners) = read(&bo, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(owners["cards"]["one"]["name"], "design");
    assert_eq!(owners["cards"]["one"]["decides"], false);
    let (status, body) = post(&bo, LAPTOP, "/api/decision", verdict("one", "approved")).await;
    assert_eq!(
        (status, body),
        (
            StatusCode::FORBIDDEN,
            json!({"error": not_designs("bo")}).to_string()
        )
    );
    bo.saves.get().unwrap().close();
    assert_eq!(std::fs::read_to_string(root.join(DOC)).unwrap(), PAGE);

    let ana = as_person(&root, "ana");
    let (_, owners) = read(&ana, LAPTOP, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(owners["cards"]["one"]["decides"], true);
    let (status, body) = post(&ana, LAPTOP, "/api/decision", verdict("one", "approved")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The person at the machine answers every card, as before.
    let (status, body) = post(&ana, MACHINE, "/api/decision", verdict("two", "rejected")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ana.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    assert!(page.contains("] ana · approved"), "{page}");
    assert!(page.contains("] owner · rejected"), "{page}");
    let (_, owners) = read(&ana, MACHINE, "/api/card-owners?path=docs/TOPIC.md").await;
    assert_eq!(
        owners["cards"]["two"]["decides"], true,
        "the machine's person"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_moves_a_task_of_their_group_and_a_non_member_is_refused_with_nothing_written() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root, &designs());
    let store = Board::store(&root);
    for slug in ["app/x", "app/y"] {
        let filed = symphony_board::NewTask {
            status: "ready".into(),
            ..symphony_board::NewTask::new(slug, "X", "owner")
        };
        store.create(&filed).unwrap();
    }
    let before = (store.get("app/x").unwrap(), store.thread("app/x").unwrap());
    let approve = |slug: &str| {
        json!({"slug": slug, "action": symphony::gates::stamps::OWNER_APPROVE,
               "payload": "c0ffee1", "note": ""})
    };
    let bo = as_person(&root, "bo");
    let (_, page) = read(&bo, LAPTOP, "/api/task-page?slug=app/x").await;
    assert_eq!(page["owner"]["decides"], false);
    for (route, body) in [
        (
            "/api/task-priority",
            json!({"slug": "app/x", "priority": 0}),
        ),
        (
            "/api/task-field",
            json!({"slug": "app/x", "name": "effort", "value": 3}),
        ),
        (symphony_api::TASK_ACTION_ROUTE, approve("app/x")),
        ("/api/task-cancel", json!({"slug": "app/x"})),
    ] {
        let (status, said) = post(&bo, LAPTOP, route, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{route}: {said}");
        assert!(said.contains(&not_designs("bo")), "{route}: {said}");
    }
    let task = store.get("app/x").unwrap();
    assert_eq!(
        (task.status.as_str(), task.priority),
        ("ready", before.0.priority)
    );
    assert_eq!(store.thread("app/x").unwrap().len(), before.1.len());

    let ana = as_person(&root, "ana");
    let (_, page) = read(&ana, LAPTOP, "/api/task-page?slug=app/x").await;
    assert_eq!(page["owner"]["decides"], true);
    for (route, body) in [
        (
            "/api/task-priority",
            json!({"slug": "app/x", "priority": 0}),
        ),
        (symphony_api::TASK_ACTION_ROUTE, approve("app/x")),
        ("/api/task-cancel", json!({"slug": "app/x"})),
    ] {
        let (status, said) = post(&ana, LAPTOP, route, body).await;
        assert_eq!(status, StatusCode::OK, "{route}: {said}");
    }
    let task = store.get("app/x").unwrap();
    assert_eq!((task.status.as_str(), task.priority), ("cancelled", 0));
    let thread = store.thread("app/x").unwrap();
    assert!(
        thread
            .iter()
            .any(|line| line.sender == "ana" && line.text == "approve c0ffee1"),
        "{thread:?}"
    );
    // The person at the machine moves every task, as before.
    let (status, said) = post(
        &ana,
        MACHINE,
        "/api/task-priority",
        json!({"slug": "app/y", "priority": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{said}");
    assert_eq!(store.get("app/y").unwrap().priority, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workflow_edge_on_a_task_of_another_group_is_refused_before_anything_is_asked() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root, &designs());
    let store = Board::store(&root);
    store
        .create(&symphony_board::NewTask::new("app/x", "X", "owner"))
        .unwrap();
    let revision =
        json!({"format": 1, "digest": "d", "source": {"commit": "c", "project_blob": "b"}});
    let request = json!({
        "project": "symphony", "task": "app/x", "edge": "start",
        "expected": {"workflow": revision, "activation_generation": 1, "task_revision": 1,
                     "at": {"phase": "build", "state": "ready"}},
        "input_revisions": {}, "idempotency_key": "k", "inputs": {}
    });
    let body = json!({"slug": "app/x", "request": request, "attestations": []});
    let route = symphony_api::workflow::WORKFLOW_APPLY_ROUTE;
    let before = store.thread("app/x").unwrap().len();
    let bo = as_person(&root, "bo");
    let (status, said) = post(&bo, LAPTOP, route, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
    assert!(said.contains(&not_designs("bo")), "{said}");
    assert_eq!(store.thread("app/x").unwrap().len(), before);
    // A member passes the group and meets the board's own judgment of the request.
    let ana = as_person(&root, "ana");
    let (status, said) = post(&ana, LAPTOP, route, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    assert!(
        said.contains("unknown_id: the request is for project symphony"),
        "the board's own refusal: {said}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_settings_pages_group_changes_a_setting_and_another_reads_its_values_alone() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root, &designs());
    let store = Board::store(&root);
    let write = json!({"key": "limits.low_volume", "action": "store", "value": 3});

    let bo = with_hub(as_person(&root, "bo"), &root);
    let (status, page) = read(&bo, LAPTOP, symphony_api::SETTINGS_PATH).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        page.get("stored"),
        None,
        "no preference offered to write: {page}"
    );
    assert!(
        page["sections"].as_array().is_some_and(|s| !s.is_empty()),
        "its values are read"
    );
    let (status, said) = post(&bo, LAPTOP, symphony_api::SETTING_WRITE_PATH, write.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
    assert!(said.contains(&not_designs("bo")), "{said}");
    assert!(
        store.stored_entries().unwrap().is_empty(),
        "nothing was written"
    );

    let ana = with_hub(as_person(&root, "ana"), &root);
    let (_, page) = read(&ana, LAPTOP, symphony_api::SETTINGS_PATH).await;
    let stored = page["stored"].as_array().cloned().unwrap_or_default();
    assert!(
        stored.iter().any(|item| item["key"] == "limits.low_volume"),
        "{page}"
    );
    let (status, said) = post(&ana, LAPTOP, symphony_api::SETTING_WRITE_PATH, write).await;
    assert_eq!(status, StatusCode::OK, "{said}");
    assert_eq!(store.stored_entries().unwrap().len(), 1);
    // The person at the machine is offered every preference, as before.
    let (_, page) = read(&ana, MACHINE, symphony_api::SETTINGS_PATH).await;
    assert_eq!(page["stored"].as_array().map(Vec::len), Some(stored.len()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_board_naming_people_and_mapping_nothing_leaves_every_thing_to_the_person_at_the_machine()
{
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root, &[]);
    let ana = as_person(&root, "ana");
    let (status, body) = post(&ana, LAPTOP, "/api/decision", verdict("one", "approved")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body.contains(
            "Forbidden: it is the person at the machine's, as this board maps no group it keeps \
             to the folder docs or to the board's default, so ana may not decide it until an \
             admin maps one (just tasks-people map PLACE GROUP): nothing was written"
        ),
        "{body}"
    );
    let (status, body) = post(&ana, MACHINE, "/api/decision", verdict("one", "approved")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ana.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    assert!(page.contains("] owner · approved"), "{page}");
    assert!(!page.contains("ana ·"), "{page}");
}
