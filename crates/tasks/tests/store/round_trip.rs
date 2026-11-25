//! A realistic board ([`crate::realistic`]) copied into a board and out again: its export reads
//! back as the board it was given, table by table, and copied into another board reads back as
//! the same text, with the rows the copy derives from it (the counters, each sender's last word)
//! and searches alike. An import cut short leaves the board as it was, and made again ends where
//! one never cut short does. A plugin's numbers keep the tokens they were copied or written in.

use crate::common::{Fixture, Seed, postgresql};
use crate::copy::everything;
use crate::realistic;
use crate::search::answered;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::time::Duration;
use symphony_board::pg::{self, Step};
use symphony_board::rows::Dump;
use symphony_board::{Error, Store};

/// A task's plugin fields, as a dump holds them.
type Fields = std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>;

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
/// newest `msg` or `verdict` line), each row read by its id.
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
/// clients' own wait (300 s), a backstop only a stall reaches
/// ([`test_time::backstop`]). A copy is one request, so its client's wait is its whole bound, and
/// these tests are of what a copy gives, not how soon (RULES.md `tests/no-test-fails-on-load`).
fn patient() -> Duration {
    test_time::backstop(postgresql().wait())
}

/// The realistic board copied into one board, then its export copied into another: each export
/// is the board it was given, both are one text, and the two boards derive the same rows from it
/// and find the same answers to every search.
#[test]
fn a_realistic_board_is_copied_whole_and_its_export_makes_the_same_board() {
    let board = realistic::board();
    let wait = patient();
    let first = Fixture::in_database(
        postgresql().waiting(wait).into(),
        Seed {
            dump: board.clone(),
        },
    );
    let exported = first.store.dump().unwrap();
    same(&exported, &board, "the export of the board it was given");
    let second = Fixture::in_database(
        postgresql().waiting(wait).into(),
        Seed {
            dump: exported.clone(),
        },
    );
    let back = second.store.dump().unwrap();
    same(&back, &exported, "the export of the export copied");
    assert_eq!(
        serde_json::to_string(&back).unwrap(),
        serde_json::to_string(&exported).unwrap(),
        "the two exports are one text"
    );
    assert_eq!(
        derived(&second.store, &board),
        derived(&first.store, &board)
    );
    for query in SEARCHED {
        let there = answered(&first.store, query);
        assert!(!there.0.is_empty(), "{query:?} finds nothing on this board");
        assert_eq!(answered(&second.store, query), there, "{query:?}");
    }
}

#[test]
fn a_board_copy_keeps_the_separate_artifact_ledger() {
    // Artifact-root export/restore is a separate contract: Dump contains core board rows.
    // Importing those rows must retain the destination's independently scoped ledger.
    let f = Fixture::new(Seed::default());
    let body = json!({"format": 1, "nested": [null, {"number": 1e18, "zero": -0.0}]});
    assert!(
        f.store
            .artifact_ledger_cas("tenant", "project", 0, &body)
            .unwrap()
    );
    let ledger = f.store.artifact_ledger("tenant", "project").unwrap();
    let board = everything();
    for _ in 0..2 {
        f.store.copy(&board).unwrap();
        assert_eq!(f.store.dump().unwrap(), board);
        assert_eq!(
            f.other().artifact_ledger("tenant", "project").unwrap(),
            ledger
        );
    }
    let mut refused = board.clone();
    refused.tasks[0].status = "not-a-status".into();
    assert!(f.store.copy(&refused).is_err());
    assert_eq!(f.store.dump().unwrap(), board);
    assert_eq!(
        f.store.artifact_ledger("tenant", "project").unwrap(),
        ledger
    );
    assert!(
        f.store
            .artifact_ledger_cas("tenant", "project", 1, &body)
            .unwrap()
    );
    assert_eq!(
        f.store
            .artifact_ledger("tenant", "project")
            .unwrap()
            .unwrap()["revision"],
        2
    );
}

/// The one value `sql` answers on `client`.
fn one(client: &pg::Client, sql: &str) -> Option<String> {
    let answers = client.request(&[Step::bare(sql)]).unwrap();
    answers[0].value().map(str::to_string)
}

/// The backends of this board's database that wait on a lock.
const WAITING: &str =
    "FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'";

/// Held on a scope whose thread a lock holds: when the scope's own body panics, every other
/// backend of this board's database is ended through `watcher`, before the scope joins the thread.
/// The holder's lock goes with its backend and the thread's request with its own, wherever it
/// waits, so a failure is said now rather than at the client's wait ([`patient`]).
struct EndsOthersOnPanic<'a>(&'a pg::Client);

impl Drop for EndsOthersOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let others = "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                 WHERE datname = current_database() AND pid <> pg_backend_pid()";
            let _ = self.0.request(&[Step::bare(others)]);
        }
    }
}

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
    let (holder, watcher) = (f.postgresql(), f.postgresql());
    // The copy's last table: the import waits there, every other table emptied and written.
    holder
        .simple("BEGIN; LOCK TABLE usage IN SHARE MODE")
        .unwrap();
    let importer = f.other();
    std::thread::scope(|scope| {
        let importing = scope.spawn(|| importer.copy(&board));
        let _stalled = EndsOthersOnPanic(&watcher);
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

/// `value` with every object's keys in their order, at every depth: two documents' texts compared
/// so differ only where a value does, a number's spelling among them, not where their keys' order
/// does.
fn keys_in_order(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            symphony_board::order::sort_by(&mut entries, &|a, b| a.0.cmp(&b.0));
            Value::Object(
                (entries.into_iter())
                    .map(|(key, value)| (key, keys_in_order(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(keys_in_order).collect()),
        other => other,
    }
}

/// A dump's text, its keys in order ([`keys_in_order`]).
fn text_of(dump: &Dump) -> String {
    serde_json::to_string(&keys_in_order(serde_json::to_value(dump).unwrap())).unwrap()
}

/// jsonb expands exponents and forgets negative zero. The copied numeric tokens, including
/// integers above f64's exact range and exponent floats which are also valid i64s, survive.
#[test]
fn numeric_fields_keep_the_copied_representation() {
    let mut board = everything();
    board.tasks[0].fields = serde_json::from_value(json!({"probe": {"numbers": {
        "large": 1e30, "small": 1e-30, "fraction": 1.234_567_890_123_456_7,
        "integer": 9_007_199_254_740_993_i64, "exponent_in_i64": 1e18,
        "zero": -0.0, "values": [1.0, -1e30, 1e-100, {"deep": 1e100}],
        "text": "1e+30", "null": null, "bool": true
    }}}))
    .unwrap();
    let postgres = Fixture::new(Seed {
        dump: board.clone(),
    });
    let mut expected = board;
    let check = |expected: &Dump| {
        let back = postgres.store.dump().unwrap();
        assert_eq!(&back, expected);
        assert_eq!(text_of(&back), text_of(expected));
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
/// identities. An equal-valued raw document update changes nothing, while a write that changes the
/// document keeps its new numeric forms and the untouched siblings' old forms. Each step's fields
/// are read back as text, so a number's spelling counts as well as its value.
#[test]
fn edited_numeric_fields_keep_the_written_tokens() {
    let mut board = everything();
    board.tasks[0].fields = serde_json::from_value(json!({"probe": {
        "values": [1e18, 1_000_000_000_000_000_000_i64],
        "object": {"leaf": 1e18}, "leaf": 1e18, "sibling": 1e18
    }}))
    .unwrap();
    let f = Fixture::new(Seed {
        dump: board.clone(),
    });
    let store = &f.store;
    let slug = board.tasks[0].slug.as_str();
    let fields_of = |slug: &str| {
        let task = (store.dump().unwrap().tasks.into_iter())
            .find(|task| task.slug == slug)
            .unwrap();
        task.fields
    };
    let fields = || fields_of(slug);
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut note = |step: String, fields: &Fields| {
        seen.push((step, serde_json::to_string(fields).unwrap()));
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
        store.copy(&board).unwrap();
        store.raw().row("task", slug).set(patch.clone()).unwrap();
        note(format!("set {patch}"), &fields());
    }
    // A failed task write rolls back the saved text too, even after its field setter ran.
    store.copy(&board).unwrap();
    let before = fields();
    assert!(
        store
            .raw()
            .row("task", slug)
            .set(json!({
                "fields.probe.values": [1_000_000_000_000_000_000_i64], "title": null
            }))
            .is_err()
    );
    assert_eq!(fields(), before);
    // An upsert of the existing task updates the same saved JSON as an ordinary assignment.
    let mut replacement = store.raw().row("task", slug).get().unwrap();
    replacement["title"] = json!("Upserted");
    replacement["fields"]["probe"]["values"] = json!([1_000_000_000_000_000_000_i64]);
    store.raw().row("task", slug).put(replacement).unwrap();
    note("upsert".into(), &fields());
    // The public setter's MERGE is separate from its version bump: an equal-valued MERGE keeps
    // the old document, its existing number's spelling among it.
    store.copy(&board).unwrap();
    let leaf = json!(1_000_000_000_000_000_000_i64);
    store
        .set_field(slug, "probe", "leaf", &leaf, "owner")
        .unwrap();
    assert_eq!(fields()["probe"]["leaf"], json!(1e18));
    note("set_field leaf to an equal integer".into(), &fields());
    store
        .set_field(slug, "probe", "leaf", &json!(1e30), "owner")
        .unwrap();
    assert_eq!(fields()["probe"]["leaf"], json!(1e30));
    note("set_field leaf 1e30".into(), &fields());
    store
        .raw()
        .row("task", slug)
        .add("fields.probe.leaf", json!(0))
        .unwrap();
    note("add 0 to leaf".into(), &fields());
    let text = json!("1e+18 is text");
    store
        .set_field(slug, "probe", "leaf", &text, "owner")
        .unwrap();
    assert_eq!(fields()["probe"]["leaf"], text);
    note("set_field leaf to text".into(), &fields());
    // Tasks imported without fields also retain numeric tokens when a plugin adds one later.
    let empty_slug = board.tasks[1].slug.as_str();
    store
        .set_field(empty_slug, "probe", "new", &json!(1e18), "owner")
        .unwrap();
    assert_eq!(fields_of(empty_slug)["probe"]["new"], json!(1e18));
    note(
        "set_field on a task without fields".into(),
        &fields_of(empty_slug),
    );
    // Removing all fields and then adding one must not reuse a deleted token or lose the
    // new input's form on a task whose fields have already been tracked.
    let raw = store.raw();
    raw.row("task", slug).set(json!({"fields": null})).unwrap();
    (raw.row("task", slug))
        .set(json!({"fields.probe.leaf": 1e18}))
        .unwrap();
    assert_eq!(fields()["probe"]["leaf"], json!(1e18));
    note("fields removed, then a leaf set".into(), &fields());
    let expected: Vec<(String, String)> = EDITED
        .iter()
        .map(|(step, text)| (step.to_string(), text.to_string()))
        .collect();
    assert_eq!(seen, expected, "{seen:#?}");
}

/// What [`edited_numeric_fields_keep_the_written_tokens`] reads back after each step.
const EDITED: [(&str, &str); 18] = [
    (
        r#"set {"fields.probe.values":[1000000000000000000]}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.values":[1e+18]}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18]}}"#,
    ),
    (
        r#"set {"fields.probe.values":[1000000000000000000,1e+18]}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.object":{"leaf":1000000000000000000}}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.object.leaf":1000000000000000000}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.object.leaf":1000000000000000000,"title":"Changed"}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1000000000000000000},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.values":[1000000000000000000,1e+18],"fields.probe.added":"new"}"#,
        r#"{"probe":{"added":"new","leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1000000000000000000,1e+18]}}"#,
    ),
    (
        r#"set {"fields":{"probe":{"values":[1000000000000000000],"leaf":1000000000000000000,"sibling":1e+18}}}"#,
        r#"{"probe":{"leaf":1000000000000000000,"sibling":1e+18,"values":[1000000000000000000]}}"#,
    ),
    (
        r#"set {"fields.probe.object.leaf":null,"fields.probe.values":[]}"#,
        r#"{"probe":{"leaf":1e+18,"object":{},"sibling":1e+18,"values":[]}}"#,
    ),
    (
        r#"set {"fields.probe.values":[1e-30,{"n":1e+30}]}"#,
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e-30,{"n":1e+30}]}}"#,
    ),
    (r#"set {"fields":null}"#, "{}"),
    (
        "upsert",
        r#"{"probe":{"leaf":1000000000000000000,"object":{"leaf":1000000000000000000},"sibling":1000000000000000000,"values":[1000000000000000000]}}"#,
    ),
    (
        "set_field leaf to an equal integer",
        r#"{"probe":{"leaf":1e+18,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        "set_field leaf 1e30",
        r#"{"probe":{"leaf":1e+30,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        "add 0 to leaf",
        r#"{"probe":{"leaf":1e+30,"object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        "set_field leaf to text",
        r#"{"probe":{"leaf":"1e+18 is text","object":{"leaf":1e+18},"sibling":1e+18,"values":[1e+18,1000000000000000000]}}"#,
    ),
    (
        "set_field on a task without fields",
        r#"{"probe":{"new":1e+18}}"#,
    ),
    (
        "fields removed, then a leaf set",
        r#"{"probe":{"leaf":1e+18}}"#,
    ),
];
