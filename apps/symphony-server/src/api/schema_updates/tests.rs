//! The machine's schema updates as the route answers them: every open one and the newest, newest
//! first, each said with its times and their sources, its waits in the owner's words, its failures
//! and its task's page on this server; a ledger that cannot be read, or no machine folder, said in
//! place of any; and the route answering through the hub, with nothing read of a board.

use super::*;
use crate::testing::{Tree, repository};
use std::path::Path;
use std::sync::Arc;
use symphony::machine::updates::{Failure, Task, VERSION};

/// 2026-11-19, 00:00 UTC.
const DAY: f64 = 1_795_046_400.0;

/// The record of a board's first store at its schema, as a refresh writes it.
const SEEN: &str = "schema-seen.log: the first store that saw the board at this schema; the move \
                    was then or just before";

/// The record of the server's first write at the schema, as a refresh writes it.
const HANDED: &str = "the server's hand-off: its reload's line in server.log";

/// The record of a release's install, as a refresh writes it: no colon in it.
const MANIFEST: &str = "release-2026-11-19's manifest, written by its install";

fn stamp(at: f64, source: &str) -> Stamp {
    Stamp {
        at,
        source: source.into(),
    }
}

/// A hub of the project at `root`, served under `/alpha` by the one server, reading the machine
/// folder `machine`.
fn hub(root: &Path, machine: Option<&Path>) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    hub.machine = machine.map(Path::to_path_buf);
    hub.serve_at("/alpha");
    hub
}

/// The ledger in `machine` holding `updates`, oldest first.
fn write(machine: &Path, updates: Vec<Update>) {
    Ledger {
        version: VERSION,
        updates,
    }
    .write(machine)
    .unwrap();
}

/// `alpha`'s board at schema 23, reached at 08:05:09, its release installed at 09:34:29 and served
/// by the machine's server at 12:51:19 on 2026-11-19.
fn served() -> Update {
    let mut update = Update::new("alpha", 23, 1, Some(stamp(DAY + 29_109.0, SEEN)));
    update.release = Some("release-2026-11-19".into());
    update.available = Some(stamp(DAY + 34_469.0, MANIFEST));
    update.served = Some(stamp(DAY + 46_279.0, HANDED));
    update
}

/// `beta`'s board at schema 24, open, failed twice, its task on the `symphony` board.
fn failing() -> Update {
    let mut update = Update::new("beta", 24, 2, Some(stamp(DAY + 50_000.0, SEEN)));
    update.available = Some(stamp(DAY + 50_100.0, MANIFEST));
    for (at, why) in [
        (DAY + 50_200.0, "the server reads the board at 23"),
        (DAY + 53_800.0, "the server still reads the board at 23"),
    ] {
        update.failures.push(Failure {
            at,
            run: "release".into(),
            why: why.into(),
        });
    }
    update.task = Some(Task {
        project: "symphony".into(),
        ours: true,
        slug: "tests/release-red-update-beta-24-g2".into(),
        closed: false,
    });
    update
}

#[test]
fn each_update_is_said_with_its_times_their_sources_its_waits_its_failures_and_its_task() {
    let tree = Tree::new();
    let root = repository(&tree);
    let machine = tree.path("machine");
    std::fs::create_dir_all(&machine).unwrap();
    let hub = hub(&root, Some(&machine));
    assert_eq!(answer(&hub), SchemaUpdates::default(), "no ledger yet");
    let mut left = Update::new("gamma", 23, 1, None);
    left.left = Some(stamp(DAY + 40_000.0, "a refresh's look"));
    write(&machine, vec![served(), left, failing()]);
    assert_eq!(
        answer(&hub).updates[0].task,
        None,
        "its task's project not registered: no page of the one server holds it"
    );
    crate::testing::registered(&machine, &["alpha", "beta", "symphony"]);
    let said = answer(&hub);
    assert_eq!(said.unread, None);
    let ids: Vec<&str> = said
        .updates
        .iter()
        .map(|update| update.id.as_str())
        .collect();
    assert_eq!(ids, ["beta@24.2", "gamma@23", "alpha@23"], "newest first");
    assert_eq!(
        said.updates[2],
        SchemaUpdate {
            id: "alpha@23".into(),
            project: "alpha".into(),
            schema: 23,
            generation: 1,
            state: UpdateState::Served,
            reached: Some(UpdateTime {
                at: DAY + 29_109.0,
                source: "schema-seen.log".into(),
                words: SEEN.into(),
            }),
            available: Some(UpdateTime {
                at: DAY + 34_469.0,
                source: MANIFEST.into(),
                words: MANIFEST.into(),
            }),
            served: Some(UpdateTime {
                at: DAY + 46_279.0,
                source: "the server's hand-off".into(),
                words: HANDED.into(),
            }),
            total: Some(17_170.0),
            total_said: Some("4 h 46 min".into()),
            after_available: Some(11_810.0),
            after_available_said: Some("3 h 17 min".into()),
            failures: 0,
            failure: None,
            task: None,
        }
    );
    let open = &said.updates[0];
    assert_eq!(
        (open.state, open.total, open.total_said.as_deref()),
        (UpdateState::Open, None, None),
        "not served: no wait to say yet"
    );
    assert_eq!(
        (open.failures, open.failure.as_deref()),
        (2, Some("the server still reads the board at 23")),
        "the count, and the last one's words"
    );
    assert_eq!(
        open.task.as_deref(),
        Some("/symphony/task/tests/release-red-update-beta-24-g2"),
        "its task's page under the project whose board holds it"
    );
    let gone = &said.updates[1];
    assert_eq!(
        (gone.state, gone.reached.as_ref(), gone.after_available),
        (UpdateState::Left, None, None)
    );
}

#[test]
fn the_answer_holds_the_twenty_newest_and_every_open_one_older_than_them() {
    let tree = Tree::new();
    let root = repository(&tree);
    let machine = tree.path("machine");
    std::fs::create_dir_all(&machine).unwrap();
    let mut updates = vec![failing()];
    for schema in 0..(SCHEMA_UPDATES_SHOWN as i64 + 5) {
        let mut update = served();
        update.schema = schema;
        update.id = updates::name("alpha", schema, 1);
        updates.push(update);
    }
    write(&machine, updates);
    let said = answer(&hub(&root, Some(&machine)));
    assert_eq!(said.updates.len(), SCHEMA_UPDATES_SHOWN + 1);
    assert_eq!(said.updates[0].id, "alpha@24", "the newest");
    assert_eq!(said.updates[SCHEMA_UPDATES_SHOWN - 1].id, "alpha@5");
    assert_eq!(
        said.updates[SCHEMA_UPDATES_SHOWN].id, "beta@24.2",
        "open, older than them"
    );
}

#[test]
fn a_ledger_that_cannot_be_read_or_no_machine_folder_is_said_in_place_of_any_update() {
    let tree = Tree::new();
    let root = repository(&tree);
    let said = answer(&hub(&root, None));
    assert_eq!(said.updates, []);
    assert_eq!(
        said.unread.as_deref(),
        Some("this server finds no machine folder")
    );
    let machine = tree.path("machine");
    std::fs::create_dir_all(&machine).unwrap();
    let ledger = updates::path(&machine);
    std::fs::write(&ledger, "{\"version\": 9, \"updates\": []}").unwrap();
    let said = answer(&hub(&root, Some(&machine)));
    assert_eq!(said.updates, []);
    let why = said.unread.unwrap();
    assert!(
        why.starts_with(&ledger.display().to_string()) && why.contains("a later build's"),
        "{why}"
    );
}

#[tokio::test]
async fn the_route_answers_from_the_hubs_machine_folder_once_there_is_a_hub() {
    use axum::http::StatusCode;
    let tree = Tree::new();
    let root = repository(&tree);
    let machine = tree.path("machine");
    std::fs::create_dir_all(&machine).unwrap();
    write(&machine, vec![served()]);
    let shared = crate::front::Shared::new(1, 1234, &root, None, crate::testing::open_gate());
    let ask = || {
        let request = axum::http::Request::builder()
            .uri(symphony_api::SCHEMA_UPDATES_PATH)
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        crate::api::answer(&shared, request)
    };
    let response = ask().await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "no hub yet"
    );
    assert!(shared.api.set(Arc::new(hub(&root, Some(&machine)))).is_ok());
    let response = ask().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let said: SchemaUpdates = serde_json::from_slice(&body).unwrap();
    assert_eq!(said.updates.len(), 1);
    assert_eq!(said.updates[0].total_said.as_deref(), Some("4 h 46 min"));
}
