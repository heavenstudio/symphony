//! Queue expiry backs off on store errors; passive visits use the cached decision snapshot.

use super::{Hub, OrderExpiry};
use crate::front::{Shared, router};
use crate::testing::{Tree, git, repository};
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::Request;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

#[test]
fn expiry_backs_off_failed_prunes_and_resumes_after_recovery() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — First.\n- [ ] **second** — Second.\n",
    );
    let hub = Hub::new(&root, &root.join("client/public"));
    hub.refresh_docs(None).unwrap();
    let before = hub.pages.read().unwrap()[""].board.clone();
    let postpone = || {
        assert!(
            symphony::postpone::act(&root, "owner", "docs/TOPIC.md", "initial-card", "later", 0)
                .unwrap()
        );
    };
    postpone();
    let expires = 3 * 86_400;
    assert!(symphony::postpone::expired(&root, expires));
    let persisted = std::fs::read(symphony::postpone::order_file(&root)).unwrap();
    // The store is readable, but atomic replacement cannot write its temporary file.
    let blocked = root.join(format!(
        "artifacts/review-order/postponed.{}.tmp",
        std::process::id()
    ));
    std::fs::create_dir(&blocked).unwrap();
    let mut expiry = OrderExpiry::default();
    let mut refreshes = 0;
    for tick in 0..=300 {
        expiry.tick(&root, expires + tick, || {
            refreshes += 1;
            let result = hub.refresh_order("");
            assert!(result.is_err(), "the store cannot be pruned");
            result
        });
    }
    assert_eq!(
        refreshes, 9,
        "301 ticks must not refresh on every expiry check"
    );
    assert_eq!(
        std::fs::read(symphony::postpone::order_file(&root)).unwrap(),
        persisted
    );
    assert_eq!(hub.pages.read().unwrap()[""].board, before);
    assert!(symphony::postpone::expired(&root, expires + 300));

    std::fs::remove_dir(&blocked).unwrap();
    for tick in 301..=302 {
        expiry.tick(&root, expires + tick, || {
            refreshes += 1;
            hub.refresh_order("")
        });
    }
    assert_eq!(refreshes, 10, "one retry prunes the recovered store");
    assert!(!symphony::postpone::expired(&root, expires + 302));

    // A fresh failure streak starts with the short delay, independent of the prior backoff.
    postpone();
    std::fs::create_dir(&blocked).unwrap();
    for tick in 303..=305 {
        expiry.tick(&root, expires + tick, || {
            refreshes += 1;
            let result = hub.refresh_order("");
            assert!(result.is_err());
            result
        });
    }
    assert_eq!(refreshes, 12, "recovery resets the retry delay");
    std::fs::remove_dir(&blocked).unwrap();
    // An external undo also clears a failure streak, even without a timer refresh.
    symphony::postpone::act(&root, "owner", "docs/TOPIC.md", "initial-card", "undo", 0).unwrap();
    expiry.tick(&root, expires + 309, || {
        refreshes += 1;
        hub.refresh_order("")
    });
    assert_eq!(refreshes, 12);
    postpone();
    std::fs::create_dir(&blocked).unwrap();
    for tick in 310..=312 {
        expiry.tick(&root, expires + tick, || {
            refreshes += 1;
            let result = hub.refresh_order("");
            assert!(result.is_err());
            result
        });
    }
    assert_eq!(refreshes, 14, "an external undo resets the retry delay");
    std::fs::remove_dir(&blocked).unwrap();
    expiry.tick(&root, expires + 316, || {
        refreshes += 1;
        hub.refresh_order("")
    });
    assert_eq!(refreshes, 15, "the next retry prunes the recovered store");
    assert!(!symphony::postpone::expired(&root, expires + 316));
}

async fn request_json(shared: &Arc<Shared>, method: &str, uri: &str, payload: Value) -> Value {
    let response = router(shared.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Host", "localhost:1234")
                .header("Content-Type", "application/json")
                .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 100_000).await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn cached_postponements_keep_distinct_proposals_independent() {
    let tree = Tree::new();
    let root = repository(&tree);
    let original = "# Topic\n\n<!-- register:start -->\n- [ ] **alpha** — Original proposal.\n- [ ] **other** — Other decision.\n<!-- register:end -->\n";
    tree.write("main/docs/TOPIC.md", original);
    git(&root, &["add", "docs/TOPIC.md"]);
    git(&root, &["commit", "-m", "Register the decisions"]);
    for (name, folder, text) in [
        ("design/a", "a", "Revision A."),
        ("design/b", "b", "Revision B."),
    ] {
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                name,
                tree.path(folder).to_str().unwrap(),
            ],
        );
        tree.write(
            &format!("{folder}/docs/TOPIC.md"),
            &original.replace("Original proposal.", text),
        );
        // a revision reaches the owner once its branch posts it
        git(&tree.path(folder), &["commit", "-qam", "Post the revision"]);
    }
    let hub = Arc::new(Hub::new(&root, &root.join("client/public")));
    hub.refresh_docs(None).unwrap();
    for scope in ["design~a", "design~b"] {
        hub.ensure_page(scope).unwrap();
    }
    let shared = Shared::new(1, 1234, &root, None, crate::testing::open_gate());
    assert!(shared.api.set(hub).is_ok());
    // Inherited main cards use one preference entry even when acted on from branch pages.
    let later = request_json(
        &shared,
        "POST",
        "/@design~a/api/postpone",
        json!({"path":"docs/TOPIC.md", "slug":"other", "action":"later"}),
    )
    .await;
    assert_eq!(later["changed"], true);
    for uri in [
        "/api/postponed",
        "/@design~a/api/postponed",
        "/@design~b/api/postponed",
    ] {
        assert_eq!(
            request_json(&shared, "GET", uri, Value::Null).await,
            json!(["other"])
        );
    }
    let undo = request_json(
        &shared,
        "POST",
        "/@design~b/api/postpone",
        json!({"path":"docs/TOPIC.md", "slug":"other", "action":"undo"}),
    )
    .await;
    assert_eq!(undo["changed"], true);
    assert_eq!(
        request_json(&shared, "GET", "/api/postponed", Value::Null).await,
        json!([])
    );
    // The watcher has not published these edits. Actions must use its kept decision snapshot,
    // including when a fresh main scan fails or a branch's live file no longer has the card.
    std::fs::write(root.join("docs/unrelated.md"), [0xff]).unwrap();
    tree.write("a/docs/TOPIC.md", "# An unpublished branch edit\n");
    assert!(symphony::board::scan(&root).is_err());
    request_json(
        &shared,
        "POST",
        "/@design~a/api/postpone",
        json!({"path":"docs/TOPIC.md", "slug":"alpha", "action":"later"}),
    )
    .await;
    assert_eq!(
        request_json(&shared, "GET", "/api/postponed", Value::Null).await,
        json!([])
    );
    assert_eq!(
        request_json(&shared, "GET", "/@design~a/api/postponed", Value::Null).await,
        json!(["alpha"])
    );
    assert_eq!(
        request_json(&shared, "GET", "/@design~b/api/postponed", Value::Null).await,
        json!([])
    );
    request_json(
        &shared,
        "POST",
        "/api/postpone",
        json!({"path":"@design~b/docs/TOPIC.md", "slug":"alpha", "action":"later"}),
    )
    .await;
    request_json(
        &shared,
        "POST",
        "/api/postpone",
        json!({"path":"@design~a/docs/TOPIC.md", "slug":"alpha", "action":"undo"}),
    )
    .await;
    assert_eq!(
        request_json(&shared, "GET", "/@design~a/api/postponed", Value::Null).await,
        json!([])
    );
    assert_eq!(
        request_json(&shared, "GET", "/@design~b/api/postponed", Value::Null).await,
        json!(["alpha"])
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        original
    );
    shared.stop();
}

#[tokio::test]
async fn visits_without_visible_changes_skip_scans_snapshots_and_hub_refreshes() {
    for postponed in [false, true] {
        let tree = Tree::new();
        let root = repository(&tree);
        tree.write("main/docs/TOPIC.md", "# Topic\n\n- [ ] **initial-card** — First.\n- [ ] **second** — Second.\n- [ ] **third** — Third.\n");
        if postponed {
            assert!(
                symphony::postpone::act(
                    &root,
                    "owner",
                    "docs/TOPIC.md",
                    "initial-card",
                    "later",
                    symphony::postpone::now()
                )
                .unwrap()
            );
        }
        let hub = Arc::new(Hub::new(&root, &root.join("client/public")));
        hub.refresh_docs(None).unwrap();
        let before = hub.pages.read().unwrap()[""].board.clone();
        let shared = Shared::new(1, 1234, &root, None, crate::testing::open_gate());
        assert!(shared.api.set(hub.clone()).is_ok());
        // The cached snapshot stays valid; a scan or a refresh would fail on this input.
        let bad = tree.path("main/docs/unrelated.md");
        std::fs::write(&bad, [0xff]).unwrap();
        assert!(symphony::board::scan(&root).is_err());
        for _ in 0..2 {
            let response = router(shared.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/postpone")
                        .header("Host", "localhost:1234")
                        .header("Content-Type", "application/json")
                        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
                        .body(Body::from(
                            json!({"path":"docs/TOPIC.md", "slug":"second", "action":"seen"})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let bytes = to_bytes(response.into_body(), 10_000).await.unwrap();
            let result: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(result, json!({"ok":true,"changed":false,"next":null}));
            assert_eq!(hub.pages.read().unwrap()[""].board, before);
        }
        if postponed {
            let order: Value = serde_json::from_slice(
                &std::fs::read(symphony::postpone::order_file(&root)).unwrap(),
            )
            .unwrap();
            assert_eq!(
                order["owners"]["owner"]["initial-card"]["remaining"],
                json!(["third"])
            );
        } else {
            assert!(!root.join("artifacts/review-order").exists());
        }
        shared.stop();
    }
}
