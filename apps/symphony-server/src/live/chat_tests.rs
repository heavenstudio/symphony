//! Exercise the real hub startup choice with child-local environments and private stores.

use super::Hub;
use crate::front::Shared;
use crate::testing::{Tree, board, git, repository};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::sys::Command;

const CHILD: &str = "SYMPHONY_CHAT_JOURNAL_TEST";
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
            let board = board::database();
            // The board's command line makes the board; the Symphony, a reader, never does.
            crate::testing::store_on(&board);
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
    let posted = || store.raw().table("chat_event").fields(&["text"]).unwrap();
    assert_eq!(posted(), json!([]));

    let home = shared_server.then(|| root.parent().unwrap().join("installed"));
    let shared = Shared::new(
        i64::from(std::process::id()),
        0,
        &root,
        home.as_deref(),
        crate::testing::open_gate(),
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
    if shared_server {
        const RETRY_MESSAGE: &str = "Owner message saved during a board outage.";
        symphony::callbacks::publish(&root, RETRY_MESSAGE).unwrap();
        // The shared clock's journal handoff fails while this scratch board has an older schema.
        // The queued owner message must survive, even when a repair and new ticks arrive before
        // the retry's deadline. Dispatch/read success must not reset the chat operation's streak.
        let mut hub = Hub::new(&root, &root.join("build/client"));
        *hub.callbacks.get_mut().unwrap() = Some(symphony::callbacks::Flusher::new(&root));
        let clock = Arc::new(Mutex::new(Instant::now()));
        let held = clock.clone();
        hub.retry_clock = Box::new(move || *held.lock().unwrap());
        store
            .raw()
            .row("meta", "schema")
            .set(json!({"version": symphony_board::SCHEMA - 1}))
            .unwrap();
        hub.board_read(&store).unwrap();
        assert_eq!(queued(), [json!({"text": RETRY_MESSAGE})]);
        assert_eq!(posted(), json!([{ "text": MESSAGE }]));
        store
            .raw()
            .row("meta", "schema")
            .set(json!({"version": symphony_board::SCHEMA}))
            .unwrap();
        for _ in 0..3 {
            hub.board_read(&store).unwrap();
        }
        assert_eq!(
            queued(),
            [json!({"text": RETRY_MESSAGE})],
            "no false acknowledgement while deferred"
        );
        assert_eq!(posted(), json!([{ "text": MESSAGE }]));
        assert_eq!(
            hub.health.report(hub.now()).dispatch.ok,
            4,
            "chat failure did not defer dispatch"
        );
        *clock.lock().unwrap() += Duration::from_secs(1);
        hub.board_read(&store).unwrap();
        assert!(
            queued().is_empty(),
            "the repaired board accepted the pending message"
        );
        assert_eq!(
            posted(),
            json!([{ "text": MESSAGE }, { "text": RETRY_MESSAGE }])
        );
    }
}
