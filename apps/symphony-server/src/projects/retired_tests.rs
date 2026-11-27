//! The one server's stop: every other project's saves and every retired project's state a request
//! still holds closed in its last drain, and nothing served after it but the state it drained.

use std::net::SocketAddr;
use std::sync::Mutex;

use axum::body::Body;
use axum::extract::Request;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Method, StatusCode, header};
use symphony::machine::{Entry, Registry};
use tower::ServiceExt;

use super::*;
use crate::projects::router;
use crate::projects::tests::{Machine, text};

/// A comment on beta's topic, which the fixture's tests write before they post it.
const COMMENT: &[u8] =
    br#"{"path":"docs/TOPIC.md","after_line":1,"kind":"comment","title":"Late","body":"After the close"}"#;

#[test]
fn a_stop_closes_every_other_projects_saves_naming_each_wait() {
    let machine = Machine::new();
    let served = machine.projects.served();
    let beta = served
        .iter()
        .find(|served| served.entry.id == "beta")
        .unwrap();
    let saves = beta
        .shared
        .saves
        .get_or_init(|| symphony::save::Saves::start(&beta.entry.root));
    let stopped = machine.projects.stop();
    let ids: Vec<&str> = stopped.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["beta"], "the starter's stop is the server's own");
    assert_eq!(
        (beta.shared.stopping(), machine.projects.starter.stopping()),
        (true, false),
        "(beta stopping, starter stopping)"
    );
    let said = Mutex::new(Vec::new());
    close_saves(&stopped, &|stage| {
        said.lock().unwrap().push(stage.to_string())
    });
    machine.tree.write("beta/docs/TOPIC.md", "# Topic\n");
    let payload =
        serde_json::json!({"path": "docs/TOPIC.md", "slug": "a-card", "state": "approved"});
    let refused = saves.post("", "/api/decision", &payload).err().unwrap();
    assert_eq!(
        (refused.status, refused.error.as_str()),
        (503, "document saves are closing")
    );
    let said = said.into_inner().unwrap();
    assert_eq!(
        said.first().map(String::as_str),
        Some("beta: the save in flight"),
        "{said:?}"
    );
}

/// A comment request of beta's reading its body while beta is renamed gamma: beta's state is
/// retired, and the server's last drain closes its saves too, so the request is refused rather
/// than saved after the close.
#[tokio::test]
async fn a_request_holding_a_retired_projects_state_is_refused_after_the_last_close() {
    let machine = Machine::new();
    let beta = machine.projects.served().remove(1);
    assert_eq!(beta.entry.id, "beta");
    machine.tree.write("beta/docs/TOPIC.md", "# Topic\n");
    let (entered, reading) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let body = Body::from_stream(futures_util::stream::once(async move {
        let _ = entered.send(());
        let _ = released.await;
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(COMMENT))
    }));
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/comment")
        .header(header::HOST, "127.0.0.1:1")
        .body(body)
        .unwrap();
    let old = beta.shared.clone();
    drop(beta);
    let request = crate::testing::at_the_machine(&old, request);
    let pending = tokio::spawn(async move { crate::write::answer(&old, request).await });
    reading.await.unwrap();
    machine.tree.write(
        "beta/symphony.toml",
        "[project]\nname = \"Gamma\"\nid = \"gamma\"\n",
    );
    Registry::change(&machine.tree.path("machine"), |registry| {
        registry.add(Entry::of(&machine.tree.path("beta")).unwrap())
    })
    .unwrap();
    let ids: Vec<String> = machine
        .projects
        .served()
        .into_iter()
        .map(|s| s.entry.id)
        .collect();
    assert_eq!(ids, ["alpha", "gamma"]);
    let stopped = machine.projects.stop();
    let ids: Vec<&str> = stopped.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["gamma", "beta"],
        "the retired state is drained with the rest"
    );
    close_saves(&stopped, &|_| {});
    release.send(()).unwrap();
    let response = pending.await.unwrap();
    let status = response.status();
    let said = text(response).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{said}");
    assert!(said.contains("document saves are closing"), "{said}");
}

/// A project registered after the server stopped and drained: the projects served stay the state
/// it drained, nothing new beside them, so a late write to one is refused rather than saved after
/// the last close.
#[tokio::test]
async fn a_registry_changed_after_the_stop_serves_only_the_drained_state() {
    let machine = Machine::new();
    let stopped = machine.projects.stop();
    close_saves(&stopped, &|_| {});
    machine.tree.write("delta/docs/README.md", "# Docs\n");
    machine.tree.write(
        "delta/symphony.toml",
        "[project]\nname = \"delta works\"\nid = \"delta\"\n",
    );
    Registry::change(&machine.tree.path("machine"), |registry| {
        registry.add(Entry::of(&machine.tree.path("delta")).unwrap())
    })
    .unwrap();
    let served = machine.projects.served();
    let seen: Vec<(&str, bool)> = served
        .iter()
        .map(|s| (s.entry.id.as_str(), s.shared.stopping()))
        .collect();
    assert_eq!(
        seen,
        [("alpha", false), ("beta", true)],
        "(id, stopping): the starter's stop is the server's own"
    );
    assert!(
        Arc::ptr_eq(&served[1].shared, &stopped[0].shared),
        "beta is served from the state the stop drained"
    );
    machine.tree.write("beta/docs/TOPIC.md", "# Topic\n");
    let request = Request::builder()
        .method(Method::POST)
        .uri("/beta/api/comment")
        .header(header::HOST, "127.0.0.1:1")
        .body(Body::from(COMMENT))
        .unwrap();
    let response = router(machine.projects.clone())
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let said = text(response).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{said}");
    assert!(said.contains("document saves are closing"), "{said}");
}
