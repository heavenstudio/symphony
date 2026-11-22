//! A realistic board ([`crate::realistic`]) from SurrealDB, on the board's own engine, to
//! PostgreSQL and back: SurrealDB's export, copied into PostgreSQL, reads back as that export,
//! table by table and as one text, with the rows the copy derives from it (the counters, each
//! sender's last word), and searches alike; PostgreSQL's is SurrealDB's text, which SurrealDB
//! took. An import into PostgreSQL cut short leaves the board as it was, and made again ends where
//! one never cut short does.

use crate::common::{Backend, BoardServer, Fixture, Seed, postgresql};
use crate::copy::everything;
use crate::realistic;
use crate::search::answered;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::time::Duration;
use symphony_board::pg::{self, Step};
use symphony_board::rows::Dump;
use symphony_board::{Database, Error, Store};

/// Where `a` and `b` first differ, row by row: none when they are equal.
fn first_difference<T: PartialEq + Debug>(a: &[T], b: &[T]) -> Option<String> {
    let at = (0..a.len().max(b.len())).find(|&at| a.get(at) != b.get(at))?;
    Some(format!(
        "{} rows against {}; row {at}: {:?} against {:?}",
        a.len(),
        b.len(),
        a.get(at),
        b.get(at)
    ))
}

/// `a` is `b`, every table of the dump, a difference named by its table and first row.
fn same(a: &Dump, b: &Dump, what: &str) {
    macro_rules! each {
        ($($table:ident),+) => {$(
            if let Some(difference) = first_difference(&a.$table, &b.$table) {
                panic!("{what}: its {} differ: {difference}", stringify!($table));
            }
        )+};
    }
    each!(
        tasks,
        task_links,
        sessions,
        roles,
        messages,
        deliveries,
        barred_keys,
        task_sessions,
        delivery_keys,
        asks,
        usages
    );
    assert!(a == b, "{what}");
}

/// The rows a copy derives from `board`: the counters it sets and each sender's last word (its
/// newest `msg` or `verdict` line), each row read by its id, so as each database reads it alike.
fn derived(store: &Store, board: &Dump) -> Value {
    let raw = store.raw();
    let counter = |id: &str| {
        let field = |name| raw.row("counter", id).field(name).unwrap();
        json!({"last": field("last"), "ts": field("ts")})
    };
    let senders: BTreeSet<&str> = (board.messages.iter())
        .filter(|m| matches!(&*m.kind, "msg" | "verdict"))
        .map(|m| &*m.sender)
        .collect();
    let said: Vec<Value> = (senders.iter())
        .map(|sender| raw.row("said", json!([sender])).get().unwrap())
        .collect();
    assert_eq!(raw.table("said").count().unwrap(), senders.len() as u64);
    json!({
        "message": counter("message"), "task": counter("task"), "ask": counter("ask"),
        "said": said,
    })
}

/// What is searched on the board copied, each found on the live board and the generated one: a
/// common word, rare ones, a slug's parts, a path, markup and text outside ASCII.
const SEARCHED: [&str; 12] = [
    "the",
    "board",
    "postgresql review",
    "orchestrator verdict changes",
    "tools/board",
    "copy.rs",
    "store copy",
    "12 µs",
    "release client build",
    "a_b",
    "commit tip",
    "main",
];

/// What each request of a test that copies the realistic board waits within: six times the
/// clients' own wait (300 s, the same on either database), a backstop only a stall reaches
/// ([`test_time::backstop`]). A copy is one request, so its client's wait is its whole bound, and
/// these tests are of what a copy gives, not how soon (RULES.md `tests/no-test-fails-on-load`).
fn patient() -> Duration {
    test_time::backstop(postgresql().wait())
}

/// SurrealDB's side is a server on the board's own engine and memtable ([`BoardServer`]), as
/// PostgreSQL's is the board's own server: its export is the one a board's server gives, and its
/// copy of a whole board is the work a board's is, not the in-memory engine's many times over.
#[test]
fn a_realistic_board_goes_from_surrealdb_to_postgresql_and_back_whole() {
    let board = realistic::board();
    let wait = patient();
    let server = BoardServer::start();
    let surreal = Fixture::in_database(
        server.client().waiting(wait).into(),
        Seed {
            dump: board.clone(),
        },
    );
    let exported = surreal.store.dump().unwrap();
    same(
        &exported,
        &board,
        "SurrealDB's export of the board it was given",
    );
    let postgres = Fixture::in_database(
        postgresql().waiting(wait).into(),
        Seed {
            dump: exported.clone(),
        },
    );
    let back = postgres.store.dump().unwrap();
    same(&back, &exported, "PostgreSQL's export of SurrealDB's");
    // One text, so SurrealDB takes PostgreSQL's export back as it took the board.
    assert_eq!(
        serde_json::to_string(&back).unwrap(),
        serde_json::to_string(&exported).unwrap(),
        "the two exports are one text"
    );
    assert_eq!(
        derived(&postgres.store, &board),
        derived(&surreal.store, &board)
    );
    for query in SEARCHED {
        let there = answered(&surreal.store, query);
        assert!(!there.0.is_empty(), "{query:?} finds nothing on this board");
        assert_eq!(answered(&postgres.store, query), there, "{query:?}");
    }
}

on_both! {
    fn a_board_copy_keeps_the_separate_artifact_ledger(backend: Backend) {
        // Artifact-root export/restore is a separate contract: Dump contains core board rows.
        // Importing those rows must retain the destination's independently scoped ledger.
        let f = Fixture::on(backend, Seed::default());
        let body = json!({"format": 1, "nested": [null, {"number": 1e18, "zero": -0.0}]});
        assert!(f.store.artifact_ledger_cas("tenant", "project", 0, &body).unwrap());
        let ledger = f.store.artifact_ledger("tenant", "project").unwrap();
        let board = everything();
        for _ in 0..2 {
            f.store.copy(&board).unwrap();
            assert_eq!(f.store.dump().unwrap(), board);
            assert_eq!(f.other().artifact_ledger("tenant", "project").unwrap(), ledger);
        }
        let mut refused = board.clone();
        refused.tasks[0].status = "not-a-status".into();
        assert!(f.store.copy(&refused).is_err());
        assert_eq!(f.store.dump().unwrap(), board);
        assert_eq!(f.store.artifact_ledger("tenant", "project").unwrap(), ledger);
        assert!(f.store.artifact_ledger_cas("tenant", "project", 1, &body).unwrap());
        assert_eq!(f.store.artifact_ledger("tenant", "project").unwrap().unwrap()["revision"], 2);
    }
}

/// A PostgreSQL client of its own on the board `f` is on.
fn connection(f: &Fixture) -> pg::Client {
    match f.other_database() {
        Database::PostgreSql(client) => client,
        Database::SurrealDb(_) => panic!("this board is on SurrealDB"),
    }
}

/// The one value `sql` answers on `client`.
fn one(client: &pg::Client, sql: &str) -> Option<String> {
    let answers = client.request(&[Step::bare(sql)]).unwrap();
    answers[0].value().map(str::to_string)
}

/// The backends of this board's database that wait on a lock.
const WAITING: &str =
    "FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'";

#[test]
fn an_import_cut_short_leaves_the_board_as_it_was_and_made_again_ends_whole() {
    let board = realistic::board();
    let mut before = everything();
    before.tasks[0].fields = serde_json::from_value(json!({"probe": {"number": 1e30}})).unwrap();
    let f = Fixture::in_database(
        postgresql().waiting(patient()).into(),
        Seed {
            dump: before.clone(),
        },
    );
    let (holder, watcher) = (connection(&f), connection(&f));
    // The copy's last table: the import waits there, every other table emptied and written.
    holder
        .simple("BEGIN; LOCK TABLE usage IN SHARE MODE")
        .unwrap();
    let importer = f.other();
    std::thread::scope(|scope| {
        let importing = scope.spawn(|| importer.copy(&board));
        let backstop = test_time::backstop(Duration::from_secs(120));
        let waits = || one(&watcher, &format!("SELECT count(*) {WAITING}")) == Some("1".into());
        test_time::until_true("the import waits on the table held", backstop, waits).unwrap();
        let ended = one(
            &watcher,
            &format!("SELECT bool_and(pg_terminate_backend(pid)) {WAITING}"),
        );
        assert_eq!(ended.as_deref(), Some("t"));
        let cut_short = importing.join().unwrap();
        assert!(matches!(cut_short, Err(Error::Db(_))), "{cut_short:?}");
    });
    holder.simple("ROLLBACK").unwrap();
    assert_eq!(f.store.dump().unwrap(), before, "nothing of it stayed");
    f.other().copy(&board).unwrap();
    same(&f.store.dump().unwrap(), &board, "the import made again");
}

/// jsonb expands exponents and forgets negative zero. The exported numeric tokens, including
/// integers above f64's exact range and exponent floats which are also valid i64s, survive.
#[test]
fn numeric_fields_keep_the_surrealdb_exports_representation() {
    let mut board = everything();
    board.tasks[0].fields = serde_json::from_value(json!({"probe": {"numbers": {
        "large": 1e30, "small": 1e-30, "fraction": 1.234_567_890_123_456_7,
        "integer": 9_007_199_254_740_993_i64, "exponent_in_i64": 1e18,
        "zero": -0.0, "values": [1.0, -1e30, 1e-100, {"deep": 1e100}],
        "text": "1e+30", "null": null, "bool": true
    }}}))
    .unwrap();
    let surreal = Fixture::on(Backend::SurrealDb, Seed { dump: board });
    let exported = surreal.store.dump().unwrap();
    let postgres = Fixture::on(
        Backend::PostgreSql,
        Seed {
            dump: exported.clone(),
        },
    );
    let mut expected = exported;
    let check = |expected: &Dump| {
        let back = postgres.store.dump().unwrap();
        assert_eq!(&back, expected);
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(expected).unwrap()
        );
    };
    check(&expected);
    let slug = expected.tasks[0].slug.clone();
    let raw = postgres.store.raw();
    // A write beside the copied numbers must not discard them; a changed number is not restored.
    raw.row("task", slug.as_str())
        .set(json!({"fields.probe.added": "new", "fields.probe.numbers.large": 42}))
        .unwrap();
    expected.tasks[0]
        .fields
        .get_mut("probe")
        .unwrap()
        .insert("added".into(), json!("new"));
    expected.tasks[0]
        .fields
        .get_mut("probe")
        .unwrap()
        .get_mut("numbers")
        .unwrap()["large"] = json!(42);
    check(&expected);
    // Replacing the copy updates its saved forms in the same transaction.
    postgres.store.copy(&expected).unwrap();
    check(&expected);
    raw.row("task", slug.as_str()).delete().unwrap();
    assert_eq!(raw.table("copied_fields").count().unwrap(), 1);
    let fresh = symphony_board::NewTask::new(&slug, "New task at the same slug", "owner");
    postgres.store.create(&fresh).unwrap();
    raw.row("task", slug.as_str())
        .set(json!({"fields": {"probe": {"numbers": 99}}}))
        .unwrap();
    let back = postgres.store.dump().unwrap();
    assert_eq!(
        back.tasks
            .iter()
            .find(|task| task.slug == slug)
            .unwrap()
            .fields,
        serde_json::from_value(json!({"probe": {"numbers": 99}})).unwrap()
    );
}

/// Actual edits keep the addressed replacement's tokens: array positions are not element
/// identities. An equal-valued raw document update is a no-op on SurrealDB, while a write that
/// changes the document keeps its new numeric forms and the untouched siblings' old forms.
#[test]
fn edited_numeric_fields_keep_the_written_tokens_on_both_databases() {
    let mut board = everything();
    board.tasks[0].fields = serde_json::from_value(json!({"probe": {
        "values": [1e18, 1_000_000_000_000_000_000_i64],
        "object": {"leaf": 1e18}, "leaf": 1e18, "sibling": 1e18
    }}))
    .unwrap();
    let (surreal, postgres) = (
        Fixture::on(
            Backend::SurrealDb,
            Seed {
                dump: board.clone(),
            },
        ),
        Fixture::on(
            Backend::PostgreSql,
            Seed {
                dump: board.clone(),
            },
        ),
    );
    let slug = board.tasks[0].slug.as_str();
    let fields = |store: &Store| {
        store
            .dump()
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.slug == slug)
            .unwrap()
            .fields
    };
    let cases = [
        json!({"fields.probe.values": [1_000_000_000_000_000_000_i64]}),
        json!({"fields.probe.values": [1e18]}),
        json!({"fields.probe.values": [1_000_000_000_000_000_000_i64, 1e18]}),
        json!({"fields.probe.object": {"leaf": 1_000_000_000_000_000_000_i64}}),
        json!({"fields.probe.object.leaf": 1_000_000_000_000_000_000_i64}),
        json!({"fields.probe.object.leaf": 1_000_000_000_000_000_000_i64, "title": "Changed"}),
        json!({"fields.probe.values": [1_000_000_000_000_000_000_i64, 1e18], "fields.probe.added": "new"}),
        json!({"fields": {"probe": {"values": [1_000_000_000_000_000_000_i64], "leaf": 1_000_000_000_000_000_000_i64, "sibling": 1e18}}}),
        json!({"fields.probe.object.leaf": null, "fields.probe.values": []}),
        json!({"fields.probe.values": [1e-30, {"n": 1e30}]}),
        json!({"fields": null}),
    ];
    for patch in cases {
        for store in [&surreal.store, &postgres.store] {
            store.copy(&board).unwrap();
            store.raw().row("task", slug).set(patch.clone()).unwrap();
        }
        let expected = fields(&surreal.store);
        let back = fields(&postgres.store);
        assert_eq!(back, expected, "{patch}");
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(&expected).unwrap(),
            "{patch}"
        );
    }
    // A failed task write rolls back the saved text too, even after its field setter ran.
    for store in [&surreal.store, &postgres.store] {
        store.copy(&board).unwrap();
        let before = fields(store);
        assert!(
            store
                .raw()
                .row("task", slug)
                .set(json!({
                    "fields.probe.values": [1_000_000_000_000_000_000_i64], "title": null
                }))
                .is_err()
        );
        assert_eq!(fields(store), before);
    }
    // An upsert of the existing task updates the same saved JSON as an ordinary assignment.
    let mut replacement = surreal.store.raw().row("task", slug).get().unwrap();
    replacement["title"] = json!("Upserted");
    replacement["fields"]["probe"]["values"] = json!([1_000_000_000_000_000_000_i64]);
    for store in [&surreal.store, &postgres.store] {
        store
            .raw()
            .row("task", slug)
            .put(replacement.clone())
            .unwrap();
    }
    assert_eq!(fields(&postgres.store), fields(&surreal.store));
    // The public setter's MERGE is separate from its version bump: SurrealDB keeps the old
    // document on an equal-valued MERGE, including its existing number's spelling.
    for store in [&surreal.store, &postgres.store] {
        store.copy(&board).unwrap();
        store
            .set_field(
                slug,
                "probe",
                "leaf",
                &json!(1_000_000_000_000_000_000_i64),
                "owner",
            )
            .unwrap();
    }
    let expected = fields(&surreal.store);
    assert_eq!(expected["probe"]["leaf"], json!(1e18));
    assert_eq!(fields(&postgres.store), expected);
    for store in [&surreal.store, &postgres.store] {
        store
            .set_field(slug, "probe", "leaf", &json!(1e30), "owner")
            .unwrap();
    }
    let expected = fields(&surreal.store);
    assert_eq!(expected["probe"]["leaf"], json!(1e30));
    assert_eq!(fields(&postgres.store), expected);
    for store in [&surreal.store, &postgres.store] {
        store
            .raw()
            .row("task", slug)
            .add("fields.probe.leaf", json!(0))
            .unwrap();
    }
    assert_eq!(fields(&postgres.store), fields(&surreal.store));
    for store in [&surreal.store, &postgres.store] {
        store
            .set_field(slug, "probe", "leaf", &json!("1e+18 is text"), "owner")
            .unwrap();
    }
    let expected = fields(&surreal.store);
    assert_eq!(expected["probe"]["leaf"], json!("1e+18 is text"));
    assert_eq!(fields(&postgres.store), expected);
    // Tasks imported without fields also retain numeric tokens when a plugin adds one later.
    let empty_slug = board.tasks[1].slug.as_str();
    for store in [&surreal.store, &postgres.store] {
        store
            .set_field(empty_slug, "probe", "new", &json!(1e18), "owner")
            .unwrap();
    }
    let empty_fields = |store: &Store| {
        store
            .dump()
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.slug == empty_slug)
            .unwrap()
            .fields
    };
    assert_eq!(empty_fields(&surreal.store)["probe"]["new"], json!(1e18));
    assert_eq!(empty_fields(&postgres.store), empty_fields(&surreal.store));
    // Removing all fields and then adding one must not reuse a deleted token or lose the
    // new input's form on a task whose fields have already been tracked.
    for store in [&surreal.store, &postgres.store] {
        store
            .raw()
            .row("task", slug)
            .set(json!({"fields": null}))
            .unwrap();
        store
            .raw()
            .row("task", slug)
            .set(json!({"fields.probe.leaf": 1e18}))
            .unwrap();
    }
    let expected = fields(&surreal.store);
    assert_eq!(expected["probe"]["leaf"], json!(1e18));
    assert_eq!(fields(&postgres.store), expected);
}
