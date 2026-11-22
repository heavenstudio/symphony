//! A project with no documents yet: the hub starts once over an empty document set, its board,
//! tasks, agents and health answer, its docs page says how to add documents, the log names the
//! missing folder once, and documents that arrive later are read.

use super::Hub;
use crate::front::{Shared, router};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository_holding, request,
};
use axum::body::to_bytes;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::StatusCode;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tower::ServiceExt;

const CHILD: &str = "SYMPHONY_NO_DOCUMENTS_CHILD";
const TEST: &str = "live::no_documents_tests::a_project_without_documents_serves_its_board_tasks_agents_and_health_and_says_so_once";

const SAID: &str = "This project has no documents yet: create a docs/ folder with a Markdown file \
                    in it, or name the folder that holds them in symphony.toml's [[entrypoints]].";

/// A repository with a README at its root and no `docs/`, nor any `[[entrypoints]]`.
fn without_documents(tree: &Tree) -> PathBuf {
    repository_holding(
        tree,
        "main",
        "README.md",
        "# A project with no documents yet\n",
    )
}

#[test]
fn a_project_without_documents_serves_its_board_tasks_agents_and_health_and_says_so_once() {
    if std::env::var_os(CHILD).is_none() {
        let fixture = Fixture::new(Seed::default().task("tools/without-docs", "ready"));
        let board = fixture.store.client();
        let (_, err) = crate::testing::child_on(board, CHILD, TEST);
        let said = |line: &str| err.lines().filter(|l| l.contains(line)).count();
        assert_eq!(
            said("→ documents: no docs/ directory under "),
            1,
            "the missing folder is named once: {err}"
        );
        assert!(
            err.contains("; the board and agents are served without documents"),
            "{err}"
        );
        assert_eq!(
            said("chat journal: left untouched by this private server"),
            1,
            "the hub starts once: {err}"
        );
        return;
    }

    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = without_documents(&tree);
    let shared = Shared::new(1, 1, &root, None, crate::testing::open_gate());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // As the server does before it announces its URL.
        crate::api::start(&shared).await.unwrap();
        let app = router(shared.clone()).layer(MockConnectInfo(
            "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
        ));
        let ask = |path: &'static str| {
            let app = app.clone();
            async move {
                let backstop = test_time::backstop(Duration::from_secs(60));
                let response = tokio::time::timeout(backstop, app.oneshot(request(path)))
                    .await
                    .unwrap_or_else(|_| {
                        panic!(
                            "{path} to answer: stalled: the wait's wall-clock backstop of \
                             {backstop:?} ran out; {}",
                            test_time::Load::now()
                        )
                    })
                    .unwrap();
                let status = response.status();
                let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                (
                    status,
                    serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                )
            }
        };
        // Each read answers at once, again and again: no read restarts the hub.
        for _ in 0..2 {
            for path in [
                "/api/health",
                "/api/board",
                "/api/tasks",
                "/api/agents",
                "/api/machine-work",
            ] {
                let (status, body) = ask(path).await;
                assert_eq!(status, StatusCode::OK, "{path}: {body}");
            }
        }
        let (_, work) = ask("/api/machine-work").await;
        serde_json::from_value::<symphony_api::MachineWork>(work).unwrap();
        let (_, board) = ask("/api/board").await;
        let board: symphony_api::Board = serde_json::from_value(board).unwrap();
        assert_eq!((board.counts.waiting, board.progress.docs), (0, 0));
        let (_, tasks) = ask("/api/tasks").await;
        let tasks: symphony_api::Tasks = serde_json::from_value(tasks).unwrap();
        assert_eq!(
            tasks
                .tasks
                .iter()
                .map(|t| t.slug.as_str())
                .collect::<Vec<_>>(),
            ["tools/without-docs"]
        );

        let (status, folder) = ask("/api/folder?path=docs/&view=outline").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(folder, serde_json::json!({"error": SAID}));
        let (status, document) = ask("/api/document?path=docs/README.md&view=outline").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(document, serde_json::json!({"error": SAID}));
        shared.stop();
    });
}

#[test]
fn documents_that_arrive_after_the_start_are_read_within_a_period() {
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = without_documents(&tree);
    let period = Duration::from_millis(500);
    let hub = Hub::start_watching::<notify::NullWatcher>(
        &root,
        &root.join("build/client"),
        false,
        false,
        period,
        |_| Err("no board in this test".into()),
    )
    .unwrap();
    hub.start_initial_board();
    let waiting = || hub.pages.read().unwrap()[""].board.counts.waiting;
    assert_eq!(waiting(), 0);

    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **first-card** — Arrived after the start.\n",
    );
    test_time::until_true(
        "the new folder's documents to be read",
        test_time::backstop(period + Duration::from_secs(30)),
        || waiting() == 1,
    )
    .unwrap();
    assert_eq!(
        hub.folder("", "docs/").unwrap().docs,
        1,
        "the docs page lists them"
    );
    hub.stop();
}
