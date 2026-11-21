//! Stale copy writers are fenced on each staging and cleanup transaction, including a migration
//! between batches. The relay schedules a real second writer on a private board, without sleeps.

use crate::common::{Fixture, NOW, Seed, refused, task};
use crate::relay::Relay;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use symphony_board::rows::Dump;
use symphony_board::rpc::Client;
use symphony_board::{Error, Opening, Raw, Schema, Store};

fn opened(client: Client, how: &Opening) -> Store {
    Store::opening(
        client,
        Box::new(|| NOW),
        Box::new(|_| false),
        "fixture",
        how,
    )
    .unwrap()
}

fn migrate(client: Client) -> Store {
    opened(
        client,
        &Opening::write()
            .at(Schema::this_build().then("DEFINE TABLE copy_schema_probe SCHEMALESS;")),
    )
}

fn count(client: &Client, table: &str) -> Value {
    json!(Raw::on(client).table(table).count().unwrap())
}

fn tables(client: &Client) -> Vec<String> {
    let found = client
        .query("RETURN object::keys((INFO FOR DB).tables);", json!({}))
        .unwrap();
    serde_json::from_value(found.last().cloned().unwrap()).unwrap()
}

fn assert_table(client: &Client, table: &str) {
    let found = tables(client);
    assert!(
        found.iter().any(|name| name == table),
        "missing {table}: {found:?}"
    );
}

fn assert_no_copy_tables(client: &Client) {
    let found = tables(client);
    assert!(
        found.iter().all(|name| !name.starts_with("copy_")),
        "{found:?}"
    );
}

fn refusal(why: &str) {
    let own = Schema::this_build().version();
    assert!(
        why.contains(&format!("the alpha board is at schema {}", own + 1)),
        "{why}"
    );
    assert!(why.contains("`just symphony` in alpha"), "{why}");
}

#[test]
fn guarded_staging_preserves_nested_nulls_in_copied_plugin_fields() {
    let mut dump = crate::copy::everything();
    dump.tasks[0].fields = [(
        "fixture".into(),
        [(
            "settings".into(),
            json!({"empty": null, "items": [null, {"value": null}]}),
        )]
        .into_iter()
        .collect(),
    )]
    .into_iter()
    .collect();
    let f = Fixture::new(Seed { dump: dump.clone() });
    assert_eq!(f.store.dump().unwrap(), dump);
}

#[test]
fn stale_remove_staging_preserves_newer_tables_even_when_migration_follows_the_table_list() {
    let f = Fixture::new(Seed::default().task("old/one", "ready"));
    let before = f.store.dump().unwrap();
    // What a copy killed while it staged leaves, and a table of a build ahead: each defined, as
    // the board is STRICT.
    for table in ["copy_abandoned", "unrelated"] {
        (f.store.client())
            .query(&format!("DEFINE TABLE {table} SCHEMALESS;"), json!({}))
            .unwrap();
        (f.store.raw().row(table, "one"))
            .create(json!({"kept": true}))
            .unwrap();
    }
    let next = f.other_client();
    let mut pending = Some(next);
    let relay = Relay::with_before(
        f.store.client().addr(),
        0,
        Some(Box::new(move |sql| {
            if sql.contains("REMOVE TABLE IF EXISTS copy_")
                && let Some(client) = pending.take()
            {
                migrate(client);
            }
        })),
    );
    let writer = opened(
        relay.client("sf", f.store.client().database()),
        &Opening::write().of(Some("alpha")),
    );
    let why = refused(writer.remove_staging());
    refusal(&why);
    assert_eq!(refused(writer.remove_staging()), why);
    // Its copy, which clears what an earlier one left before it stages, is refused the same
    // way and says what it was at.
    let (Error::Refused(copying) | Error::Db(copying)) = writer.copy(&before).unwrap_err();
    assert_eq!(
        copying,
        format!("{why}; staging left by an earlier copy could not be removed")
    );
    assert_eq!(count(f.store.client(), "copy_abandoned"), json!(1));
    assert_table(f.store.client(), "copy_schema_probe");
    // A writer at the new version can clean both staging tables, preserving unrelated data.
    migrate(f.other_client()).remove_staging().unwrap();
    assert_no_copy_tables(f.store.client());
    assert_eq!(count(f.store.client(), "unrelated"), json!(1));
    assert_eq!(f.store.dump().unwrap(), before);
}

#[test]
fn each_staging_batch_refuses_a_migration_before_inserting_or_cleaning_any_more_rows() {
    for committed in 0..3 {
        let f = Fixture::new(Seed::default().task("old/one", "ready"));
        let before = f.store.dump().unwrap();
        // Each task needs a request of its own; the last batch exercises the final flush too.
        let dump = Dump {
            tasks: (0..3)
                .map(|id| symphony_board::rows::TaskRow {
                    id: id + 1,
                    body: "x".repeat(600_000),
                    ..task(&format!("new/{id}"), "ready")
                })
                .collect(),
            ..Dump::default()
        };
        // The relay's callback runs on its own thread; the test reads its chosen table after it.
        let table = Arc::new(Mutex::new(None));
        let staged = Arc::clone(&table);
        let mut next = Some(f.other_client());
        let mut batches = 0;
        let relay = Relay::with_before(
            f.store.client().addr(),
            0,
            Some(Box::new(move |sql| {
                if let Some((_, after)) = sql.split_once("INSERT INTO copy_") {
                    if batches == committed {
                        *staged.lock().unwrap() =
                            Some(format!("copy_{}", after.split_whitespace().next().unwrap()));
                        migrate(next.take().unwrap());
                    }
                    batches += 1;
                }
            })),
        );
        let writer = opened(
            relay.client("sf", f.store.client().database()),
            &Opening::write().of(Some("alpha")),
        );
        let why = refused(writer.copy(&dump));
        refusal(&why);
        assert!(why.contains("staging cleanup failed:"), "{why}");
        let table = table.lock().unwrap().clone().expect("the migration ran");
        assert_eq!(count(f.store.client(), &table), json!(committed));
        assert_table(f.store.client(), "copy_schema_probe");
        assert_eq!(f.store.dump().unwrap(), before);
        migrate(f.other_client()).remove_staging().unwrap();
        assert_no_copy_tables(f.store.client());
    }
}

#[test]
fn cleanup_refused_after_a_failed_swap_reports_both_causes_and_preserves_staging() {
    let f = Fixture::new(Seed::default().task("old/one", "ready"));
    let before = f.store.dump().unwrap();
    let mut next = Some(f.other_client());
    let relay = Relay::with_before(
        f.store.client().addr(),
        0,
        Some(Box::new(move |sql| {
            if sql.contains("REMOVE TABLE IF EXISTS copy_")
                && let Some(client) = next.take()
            {
                migrate(client);
            }
        })),
    );
    let writer = opened(
        relay.client("sf", f.store.client().database()),
        &Opening::write().of(Some("alpha")),
    );
    let mut bad = crate::copy::everything();
    bad.messages[0].kind = "shout".into();
    let Error::Db(why) = writer.copy(&bad).unwrap_err() else {
        panic!("the swap's database failure remains the primary error");
    };
    assert!(why.contains("shout"), "{why}");
    assert!(why.contains("staging cleanup failed:"), "{why}");
    refusal(&why);
    let staging: Vec<_> = tables(f.store.client())
        .into_iter()
        .filter(|t| t.starts_with("copy_") && t.ends_with("_task"))
        .collect();
    assert_eq!(staging.len(), 1);
    assert_eq!(count(f.store.client(), &staging[0]), json!(2));
    assert_table(f.store.client(), "copy_schema_probe");
    assert_eq!(f.store.dump().unwrap(), before);
}

/// A copy whose staging another copy's start removed (`remove_staging`, which every copy begins
/// with) is refused at its swap and leaves the board whole: between two requests of its first
/// table, where the database makes the staging table again for the rows sent after and the
/// swap would otherwise put in the later rows alone, and once everything was staged.
#[test]
fn a_copy_whose_staging_another_copy_took_is_refused_and_leaves_the_board() {
    for (at, held) in [
        (
            "INSERT INTO copy_",
            "its task holds 2 of the 3 rows it sent",
        ),
        (
            "DELETE said",
            "its task holds no table of the 3 rows it sent",
        ),
    ] {
        let f = Fixture::new(Seed::default().task("old/one", "ready"));
        let before = f.store.dump().unwrap();
        // Each task needs a request of its own.
        let dump = Dump {
            tasks: (0..3)
                .map(|id| symphony_board::rows::TaskRow {
                    id: id + 1,
                    body: "x".repeat(600_000),
                    ..task(&format!("new/{id}"), "ready")
                })
                .collect(),
            ..Dump::default()
        };
        let mut other = Some(f.other_client());
        let mut seen = 0;
        let relay = Relay::with_before(
            f.store.client().addr(),
            0,
            Some(Box::new(move |sql| {
                if sql.contains(at) {
                    seen += 1;
                    // The second request of the first table, or the swap.
                    if seen == if at == "DELETE said" { 1 } else { 2 }
                        && let Some(client) = other.take()
                    {
                        opened(client, &Opening::write()).remove_staging().unwrap();
                    }
                }
            })),
        );
        let writer = opened(
            relay.client("sf", f.store.client().database()),
            &Opening::write(),
        );
        assert_eq!(
            refused(writer.copy(&dump)),
            format!(
                "the copy's staging was taken while it staged, as a copy of the board started \
                 meanwhile takes it, and the board is as it was: {held}"
            ),
            "{at}"
        );
        assert_eq!(f.store.dump().unwrap(), before, "{at}");
        assert_no_copy_tables(f.store.client());
    }
}
