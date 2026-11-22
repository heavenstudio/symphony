//! A save in a project that delivers by pull request, through the real router and the server's
//! own wiring: the pending answer reads the pages again, as no file changed for a watcher to
//! tell, and what the hub calls after a refresh takes a kept save up again.

use crate::front::{Shared, router};
use crate::live::Hub;
use crate::testing::{Tree, git, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8123";
const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n";

/// A repository that delivers by pull request, one page with one card committed.
fn delivering(tree: &Tree) -> PathBuf {
    let root = repository_holding(tree, "main", "docs/TOPIC.md", PAGE);
    tree.write(
        "main/symphony.toml",
        "[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n",
    );
    git(&root, &["add", "symphony.toml"]);
    git(&root, &["commit", "-qm", "deliver by pull request"]);
    root
}

fn serving(root: &Path) -> Arc<Shared> {
    let gate = crate::testing::open_gate();
    Shared::new(1, 8123, root, None, gate)
}

/// The commit `name` names in the repository at `root`, empty where it names none.
fn tip(root: &Path, name: &str) -> String {
    symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["rev-parse", "--verify", "-q", name])
            .current_dir(root),
    )
    .ok()
    .filter(|done| done.status.success())
    .map(|done| String::from_utf8_lossy(&done.stdout).trim().to_string())
    .unwrap_or_default()
}

fn branch(root: &Path, name: &str) -> bool {
    !tip(root, name).is_empty()
}

#[tokio::test]
async fn a_pending_answer_reads_the_pages_again_so_the_feed_brings_the_save() {
    let tree = Tree::new();
    let root = delivering(&tree);
    let shared = serving(&root);
    let hub = Arc::new(Hub::new(&root, &root.join("build/client")));
    hub.refresh_page("", &root).unwrap();
    assert!(shared.api.set(hub.clone()).is_ok());
    let before = hub.cursor("").unwrap();
    let first = hub.document("", "docs/TOPIC.md").unwrap();
    assert!(first.sections.iter().all(|s| s.key != "pending-delivery"));

    let app = router(shared.clone()).layer(MockConnectInfo(
        "127.0.0.1:5000".parse::<SocketAddr>().unwrap(),
    ));
    let payload = r#"{"path":"docs/TOPIC.md","slug":"one","state":"approved","note":"","tap":"t"}"#;
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
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let answer: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(answer, serde_json::json!({"ok":true,"delivery":"pending"}));

    // No file changed, and the answer is out: the pages were read again before it, so the feed
    // has moved and the page the hub holds carries the note and the owner's answer.
    assert_eq!(
        std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap(),
        PAGE
    );
    assert!(hub.cursor("").unwrap() > before, "the feed did not move");
    let page = hub.document("", "docs/TOPIC.md").unwrap();
    assert_eq!(page.sections[0].key, "pending-delivery");
    let html: String = page.sections.iter().map(|s| s.html.as_str()).collect();
    assert!(html.contains("approved"), "{html}");
    shared.saves.get().unwrap().close();
}

/// What the server gives its hub for after a refresh ([`crate::api::settling`]) starts
/// the committer for a save the journal keeps and has it taken up: its branch is made, with no
/// new save. A project that keeps none starts nothing.
#[test]
fn what_follows_a_refresh_takes_a_kept_save_up_again() {
    let tree = Tree::new();
    let root = delivering(&tree);
    let main = tip(&root, "main");
    let shared = serving(&root);
    let settle = crate::api::settling(&shared);
    settle();
    assert!(
        shared.saves.get().is_none(),
        "nothing kept, nothing started"
    );

    let answered = PAGE.replace("[ ]", "[x]");
    let entry = serde_json::json!({
        "edit": {"rel": "docs/TOPIC.md", "base": PAGE, "new": answered},
        "publication": null,
        "delivery": {},
    });
    tree.write(
        &format!(
            "main/.git/symphony/server/verdict-publications/{:020}.json",
            4
        ),
        &entry.to_string(),
    );
    settle();
    let made = test_time::until_true(
        "the kept save's branch to be made",
        test_time::backstop(Duration::from_secs(30)),
        || branch(&root, "refs/heads/docs/owner-save-4"),
    );
    if let Err(stall) = made {
        panic!("{stall}");
    }
    shared.saves.get().unwrap().close();
    assert_eq!(tip(&root, "main"), main, "main moved");
    assert_eq!(tip(&root, "docs/owner-save-4^"), main);
    drop(settle);
}
