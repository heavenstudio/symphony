//! A board one schema ahead of this build's, through the server's own reach of it
//! ([`symphony::taskview::connect`], a store opened to read): when the step that moved it is
//! declared to keep this build writing ([`symphony_board::schema::COMPATIBLE`]), its tick flushes
//! the owner's chat journal, closes the owner's answered asks, dispatches, judges and gives as at
//! its own schema, says nothing of the schema on any page, and a page's save is written; when it
//! is not, each of those is left as it was, the pages say the board is read only, and the save is
//! refused. The tick's store is opened before the move, the save's after it.
//!
//! Each case runs in a child of this test with the board's address in its environment, as the
//! journal's flush and a save reach the board from the process's own.

use super::Hub;
use super::owner_asks::Kept;
use crate::front::Shared;
use crate::testing::board::{Fixture, HOST, NOW, Seed};
use crate::testing::{Tree, repository_holding};
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::Request;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use symphony_board::rpc::Client;
use symphony_board::sys::Command;
use symphony_board::{Opening, Schema, Store};

const CHILD: &str = "SYMPHONY_COMPATIBLE_AHEAD_TEST";
const TEST: &str = "live::compatible_ahead_tests::a_board_one_ahead_is_written_by_the_tick_and_a_save_only_when_its_step_keeps_this_build_writing";
const MESSAGE: &str = "From the owner, while the board is one schema ahead.";
const DOC: &str = "docs/DECISIONS.md";

/// The decisions register, `answer` under the one card the owner rejected.
fn register(answer: &str) -> String {
    format!(
        "# Decisions\n\n<!-- register:start -->\n\
         - [-] **lamp-a** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — too pale.\n{answer}\
         <!-- register:end -->\n"
    )
}

#[test]
fn a_board_one_ahead_is_written_by_the_tick_and_a_save_only_when_its_step_keeps_this_build_writing()
{
    let Ok(mode) = std::env::var(CHILD) else {
        for mode in ["listed", "unlisted"] {
            let tree = Tree::new();
            let root = repository_holding(&tree, "main", DOC, &register(""));
            // Each on this machine, as the server's reach judges a session by its process here:
            // this test's, which runs while its child does.
            let orchestrator = |handle: &str, seen: f64| symphony_board::rows::SessionRow {
                handle: handle.into(),
                kind: "orchestrator".into(),
                connected: NOW - 3_600.0,
                last_seen: seen,
                host: symphony_board::sys::hostname(),
                pid: Some(i64::from(std::process::id())),
                ..Default::default()
            };
            // A give is due: amber went an hour ago holding tools/quiet, and cobalt stays.
            let fixture = Fixture::new(
                Seed::default()
                    .session_row(orchestrator("amber-heron", NOW - 3_600.0))
                    .session_row(orchestrator("cobalt-otter", NOW))
                    .task("tools/quiet", "ready")
                    .role("tools/quiet", "amber-heron", "orchestrator"),
            );
            // The owner's ask on a card the agents have since answered in the register.
            let card = symphony::verdicts::open_cards(&root).unwrap()[0].ask();
            fixture.store.open_ask(&card).unwrap();
            std::fs::write(
                root.join(DOC),
                register("  [2026-10-06 10:30] agent — Paler now.\n"),
            )
            .unwrap();
            let held = fixture.store.client();
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, mode)
                .env("TASKS_SERVER", held.addr())
                .env("TASKS_DATABASE", held.database())
                .env("TASKS_NOW", NOW.to_string())
                .env_remove("ASKS_DB")
                .env_remove("TASKS_SESSION")
                .env_remove("SYMPHONY_UI_DIST")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE")
                .current_dir(&root);
            let done = symphony_board::sys::output(&mut child).unwrap();
            let out = String::from_utf8_lossy(&done.stdout);
            let err = String::from_utf8_lossy(&done.stderr);
            assert!(done.status.success(), "{mode}: {out}\n{err}");
            assert!(out.contains("1 passed"), "child ran no test: {out}");
        }
        return;
    };
    let listed = mode == "listed";
    let root = std::env::current_dir().unwrap();
    let client = || {
        Client::new(
            &std::env::var("TASKS_SERVER").unwrap(),
            "sf",
            &std::env::var("TASKS_DATABASE").unwrap(),
        )
    };
    let mut hub = Hub::new(&root, &root.join("build/client"));
    *hub.callbacks.get_mut().unwrap() = Some(symphony::callbacks::Flusher::new(&root));
    *hub.owner_asks.get_mut().unwrap() = Some(Kept::default());
    hub.refresh_docs(None).unwrap();
    let hub = Arc::new(hub);
    // The tick's store, the server's own reach, opened while the board is at this build's schema.
    let tick = symphony::taskview::connect(&root).unwrap();
    // A newer build moves the board one schema on, its step declared to keep this build writing
    // or not.
    let next = Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;");
    let next = if listed { next.keeping_writes() } else { next };
    let at = |how: &Opening| {
        Store::opening(client(), Box::new(|| NOW), Box::new(|_| false), HOST, how).unwrap()
    };
    at(&Opening::write().at(next));
    let board = at(&Opening::read());
    assert_eq!(
        board.board_version().unwrap(),
        Some(symphony_board::SCHEMA + 1)
    );
    symphony::callbacks::publish(&root, MESSAGE).unwrap();
    let journal = symphony::asks::default_path(&root, None);
    let queued = || {
        let done = symphony_board::sys::output(
            Command::new("sqlite3")
                .arg("-json")
                .arg(&journal)
                .arg("SELECT text FROM callbacks ORDER BY id;"),
        )
        .unwrap();
        assert!(done.status.success(), "{done:?}");
        match done.stdout.is_empty() {
            true => Vec::new(),
            false => serde_json::from_slice::<Vec<Value>>(&done.stdout).unwrap(),
        }
    };
    let posted = || {
        symphony_board::Raw::on(&client())
            .table("chat_event")
            .fields(&["text"])
            .unwrap()
    };
    assert_eq!(queued(), [json!({ "text": MESSAGE })]);

    hub.board_read(&tick).unwrap();
    let asks = board.asks(None).unwrap();
    let quiet = board.get("tools/quiet").unwrap();
    let holders = quiet.holders("orchestrator");
    let dispatched = hub.health.report(hub.now()).dispatch.ok;
    let notice = hub.server_notice.borrow().clone();
    let writes = hub.writes();
    let (status, said) = saved(&hub, json!({ "slug": "tools/quiet", "priority": 1 }));
    let priority = board.get("tools/quiet").unwrap().priority;
    if listed {
        assert!(queued().is_empty(), "the journal was handed to the board");
        assert_eq!(posted(), json!([{ "text": MESSAGE }]));
        assert!(asks.is_empty(), "the answered ask was closed: {asks:?}");
        assert_eq!(
            holders,
            ["cobalt-otter"],
            "the gone holder's task was given"
        );
        assert_eq!(dispatched, 1, "the tick dispatched");
        assert_eq!(notice, "", "the pages say nothing of the schema");
        assert_eq!(writes, Some(true), "/api/projects says it writes the board");
        assert_eq!((status, said), (200, json!({ "ok": true })));
        assert_eq!(priority, 1);
    } else {
        assert_eq!(
            queued(),
            [json!({ "text": MESSAGE })],
            "the journal keeps it"
        );
        assert_eq!(posted(), json!([]));
        assert_eq!(asks.len(), 1, "the ask stays open");
        assert_eq!(holders, ["amber-heron"], "nothing was given");
        assert_eq!(dispatched, 0, "the tick dispatched nothing");
        assert_eq!(
            notice,
            super::health::read_only(symphony_board::SCHEMA + 1, None)
        );
        assert_eq!(
            writes,
            Some(false),
            "/api/projects says it only reads the board"
        );
        assert_ne!(status, 200, "{said}");
        let said = said.to_string();
        assert!(
            said.contains(&format!("schema {}", symphony_board::SCHEMA + 1)),
            "the save names the board's schema: {said}"
        );
        assert_ne!(priority, 1, "the save wrote nothing");
    }
}

/// What a page's save of `payload` as the task's priority answers, past front's check: the status,
/// and the JSON said, or else the text.
fn saved(hub: &Arc<Hub>, payload: Value) -> (u16, Value) {
    let shared = Shared::new(1, 1234, &hub.root, None, crate::testing::open_gate());
    assert!(shared.api.set(Arc::clone(hub)).is_ok());
    let request = Request::builder()
        .method("POST")
        .uri("/api/task-priority")
        .header("Host", "localhost:1234")
        .header("Content-Type", "application/json")
        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
        .body(Body::from(payload.to_string()))
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let response = crate::write::answer(&shared, request).await;
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let said = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, said)
    })
}
