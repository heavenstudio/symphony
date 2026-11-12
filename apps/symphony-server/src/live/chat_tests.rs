//! Exercise the real hub startup choice with child-local environments and private stores.

use crate::front::Shared;
use crate::gate::Gate;
use crate::testing::{Tree, board, git, repository};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;
use symphony_board::sys::Command;

const CHILD: &str = "SF_CHAT_JOURNAL_TEST";
const TEST: &str = "live::chat_tests::only_the_shared_server_drains_the_chat_journal";
const MESSAGE: &str = "Pending owner message must survive a private preview.";

#[test]
fn only_the_shared_server_drains_the_chat_journal() {
    let Ok(mode) = std::env::var(CHILD) else {
        for mode in [
            "private-default",
            "private-named",
            "shared-default",
            "shared-named",
        ] {
            let tree = Tree::new();
            let root = repository(&tree);
            git(
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    "preview",
                    tree.path("preview").to_str().unwrap(),
                ],
            );
            let board = board::client();
            // The board's command line makes the board; the Symphony, a reader, never does.
            symphony_board::Store::open(symphony_board::rpc::Client::new(
                board.addr(),
                "sf",
                board.database(),
            ))
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, mode)
                .env("TASKS_SERVER", board.addr())
                .env("TASKS_DATABASE", board.database())
                .env_remove("ASKS_DB")
                .env_remove("TASKS_SESSION")
                .env_remove("SYMPHONY_UI_DIST")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE")
                .current_dir(tree.path("preview"));
            if mode.ends_with("named") {
                child.env("ASKS_DB", tree.path("private-asks.db"));
            }
            let done = symphony_board::sys::output(&mut child).unwrap();
            let out = String::from_utf8_lossy(&done.stdout);
            let err = String::from_utf8_lossy(&done.stderr);
            assert!(done.status.success(), "{mode}: {out}\n{err}");
            assert!(out.contains("1 passed"), "child ran no test: {out}");
            let notice = if mode.starts_with("shared") {
                "chat journal: draining enabled for the shared server"
            } else {
                "chat journal: left untouched by this private server"
            };
            assert!(err.contains(notice), "{mode}: {err}");
        }
        return;
    };

    let shared_server = mode.starts_with("shared");
    let root = std::env::current_dir().unwrap();
    let named = std::env::var("ASKS_DB").ok();
    let journal = symphony::asks::default_path(&root, named.as_deref());
    if mode.ends_with("default") {
        assert!(named.is_none(), "the regression requires ASKS_DB unset");
        assert!(!journal.exists(), "the fixture journal starts absent");
        assert_eq!(
            journal,
            root.parent().unwrap().join("main/artifacts/asks/asks.db"),
            "a preview's default journal belongs to its temporary main checkout"
        );
    }
    symphony::callbacks::publish(&root, MESSAGE).unwrap();
    let queued = || {
        let done = symphony_board::sys::output(
            Command::new("sqlite3")
                .arg("-json")
                .arg(&journal)
                .arg("SELECT text FROM callbacks ORDER BY id;"),
        )
        .unwrap();
        assert!(done.status.success(), "{done:?}");
        if done.stdout.is_empty() {
            Vec::new()
        } else {
            serde_json::from_slice::<Vec<Value>>(&done.stdout).unwrap()
        }
    };
    assert_eq!(queued(), [json!({"text": MESSAGE})]);
    let store = symphony::taskview::connect(&root).unwrap();
    let board = symphony_board::rpc::Client::new(
        &std::env::var("TASKS_SERVER").unwrap(),
        "sf",
        &std::env::var("TASKS_DATABASE").unwrap(),
    );
    let posted = || {
        board
            .query("SELECT text FROM chat_event;", json!({}))
            .unwrap()[0]
            .clone()
    };
    assert_eq!(posted(), json!([]));

    let home = shared_server.then(|| root.parent().unwrap().join("installed"));
    let shared = Shared::new(
        i64::from(std::process::id()),
        0,
        &root,
        home.as_deref(),
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let hub = crate::api::warm(&shared).await.unwrap();
        // Startup and recurring board reads must both respect the journal ownership choice.
        for _ in 0..2 {
            hub.board_read(&store).unwrap();
        }
        assert!(hub.task_snapshot().is_ok(), "the scratch board was read");
        shared.stop();
    });

    if shared_server {
        assert!(
            queued().is_empty(),
            "shared delivery removes the queued entry"
        );
        assert_eq!(
            posted(),
            json!([{"text": MESSAGE}]),
            "delivery is idempotent"
        );
    } else {
        assert_eq!(queued(), [json!({"text": MESSAGE})]);
        assert_eq!(posted(), json!([]), "a preview posts no journal entry");
    }
}
