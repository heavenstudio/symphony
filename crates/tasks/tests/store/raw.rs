//! A board's rows read and set as they are (`Store::raw`): what the tests set up past the rules,
//! asked by table, id and equal fields and never as a statement of the test's own, on either
//! database.

use crate::common::{Fixture, Seed};
use serde_json::{Value, json};

fn board() -> Fixture {
    Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("tools/one", "ready")
            .task("tools/two", "ready"),
    )
}

#[test]
fn one_row_is_read_whole_by_its_fields_and_by_one_field() {
    let f = board();
    let raw = f.store.raw();
    let row = raw.row("task", "tools/one").get().unwrap();
    assert_eq!(row["slug"], json!("tools/one"));
    assert_eq!(row.get("id"), None, "a row read by its id comes without it");
    assert_eq!(
        raw.row("task", "tools/one")
            .fields(&["slug", "status"])
            .unwrap(),
        json!({"slug": "tools/one", "status": row["status"]})
    );
    assert_eq!(
        raw.row("task", "tools/one").field("slug").unwrap(),
        json!("tools/one")
    );
    assert!(raw.row("task", "tools/one").exists().unwrap());
}

#[test]
fn a_row_that_is_not_there_reads_as_null_and_is_not_made_by_a_set() {
    let f = board();
    let raw = f.store.raw();
    let gone = || raw.row("task", "tools/none");
    assert_eq!(gone().get().unwrap(), Value::Null);
    assert_eq!(gone().field("slug").unwrap(), Value::Null);
    assert_eq!(gone().fields(&["slug"]).unwrap(), Value::Null);
    assert!(!gone().exists().unwrap());
    gone().set(json!({"title": "made by a set"})).unwrap();
    gone().add("priority", 1).unwrap();
    gone().delete().unwrap();
    assert!(!gone().exists().unwrap());
    assert_eq!(raw.table("task").count().unwrap(), 2);
}

#[test]
fn a_set_changes_the_fields_named_unsets_a_null_and_leaves_the_rest() {
    let f = board();
    let raw = f.store.raw();
    let before = raw.row("task", "tools/one").get().unwrap();
    raw.row("task", "tools/one")
        .set(json!({"title": "another", "fields._board.red": {"by": "owner"}}))
        .unwrap();
    let after = raw.row("task", "tools/one").get().unwrap();
    assert_eq!(after["title"], json!("another"));
    assert_eq!(after["fields"]["_board"]["red"], json!({"by": "owner"}));
    assert_eq!(after["body"], before["body"]);
    assert_eq!(
        raw.row("task", "tools/two").field("title").unwrap(),
        json!("Task tools/two"),
        "the other row is as it was"
    );
    raw.row("task", "tools/one")
        .set(json!({"fields._board.red": null}))
        .unwrap();
    let unset = raw.row("task", "tools/one").field("fields._board.red");
    assert_eq!(unset.unwrap(), Value::Null);
}

#[test]
fn a_field_moves_by_what_it_is_given() {
    let f = board();
    let raw = f.store.raw();
    let was = raw.row("task", "tools/one").field("updated").unwrap();
    raw.row("task", "tools/one").add("updated", 90).unwrap();
    raw.row("task", "tools/one").add("updated", -30.5).unwrap();
    let now = raw.row("task", "tools/one").field("updated").unwrap();
    assert_eq!(now.as_f64().unwrap(), was.as_f64().unwrap() + 59.5);
    // A field within a typed one, and a schemaless table's, from nothing when it is not set.
    let task = || raw.row("task", "tools/one");
    task().add("fields.count", 3).unwrap();
    task().add("fields.count", 2).unwrap();
    assert_eq!(task().field("fields.count").unwrap(), json!(5));
    let lease = || raw.row("callback_lease", json!(["dispatch", "this-host"]));
    lease().put(json!({"asked": 1})).unwrap();
    lease().add("asked", 2).unwrap();
    lease().add("tries", 1).unwrap();
    assert_eq!(lease().get().unwrap(), json!({"asked": 3, "tries": 1}));
}

#[test]
fn a_row_under_a_compound_id_is_made_changed_and_removed() {
    let f = board();
    let raw = f.store.raw();
    let lease = || raw.row("callback_lease", json!(["dispatch", "this-host"]));
    assert!(!lease().exists().unwrap());
    lease().put(json!({"until": 5, "asked": 1})).unwrap();
    assert_eq!(lease().get().unwrap(), json!({"until": 5, "asked": 1}));
    lease().put(json!({"until": 9})).unwrap();
    assert_eq!(lease().get().unwrap(), json!({"until": 9, "asked": 1}));
    let again = lease().create(json!({"until": 1}));
    assert!(again.is_err(), "a row that is there is not made again");
    assert_eq!(lease().field("until").unwrap(), json!(9));
    lease().delete().unwrap();
    assert!(!lease().exists().unwrap());
    lease().create(json!({"until": 1})).unwrap();
    assert_eq!(lease().get().unwrap(), json!({"until": 1}));
}

#[test]
fn a_tables_rows_are_listed_counted_set_and_removed_whole_or_by_equal_fields() {
    let f = board();
    let raw = f.store.raw();
    let slugs = |rows: Value| -> Vec<String> {
        (rows.as_array().unwrap().iter())
            .map(|row| row["slug"].as_str().unwrap().to_string())
            .collect()
    };
    let all = raw.table("task").get().unwrap();
    assert_eq!(slugs(all.clone()), ["tools/one", "tools/two"]);
    assert!(all[0].get("id").is_some(), "a listed row comes with its id");
    assert_eq!(raw.table("task").count().unwrap(), 2);

    let one = || raw.table("task").matching(json!({"slug": "tools/one"}));
    assert_eq!(slugs(one().get().unwrap()), ["tools/one"]);
    assert_eq!(one().count().unwrap(), 1);
    assert_eq!(
        one().fields(&["slug"]).unwrap(),
        json!([{"slug": "tools/one"}])
    );
    one().set(json!({"status": "blocked"})).unwrap();
    let blocked = || {
        raw.table("task")
            .matching(json!({"status": "blocked", "slug": "tools/one"}))
    };
    assert_eq!(blocked().count().unwrap(), 1);
    assert_eq!(
        (raw.table("task")
            .matching(json!({"status": "blocked", "slug": "tools/two"})))
        .count()
        .unwrap(),
        0,
        "every field named must be equal"
    );

    raw.table("task")
        .set(json!({"title": "every row"}))
        .unwrap();
    let titled = raw.table("task").matching(json!({"title": "every row"}));
    assert_eq!(titled.count().unwrap(), 2);
    raw.table("task").add("priority", 2).unwrap();

    // A null among the equal fields is a field not set.
    let parentless = raw.table("task").matching(json!({"parent": null}));
    assert_eq!(parentless.count().unwrap(), 2);

    blocked().delete().unwrap();
    assert_eq!(slugs(raw.table("task").get().unwrap()), ["tools/two"]);
    raw.table("task").delete().unwrap();
    assert_eq!(raw.table("task").count().unwrap(), 0);
    assert_eq!(raw.table("task").get().unwrap(), json!([]));
}

#[test]
#[should_panic(expected = "is not a plain name for a table")]
fn a_table_is_named_plainly() {
    let f = Fixture::empty();
    let _ = f.store.raw().table("task; DELETE task");
}

#[test]
#[should_panic(expected = "is not a plain name for a field")]
fn a_field_is_named_plainly() {
    let f = Fixture::empty();
    let _ = f.store.raw().row("task", "a").field("slug; DELETE task");
}

#[test]
#[should_panic(expected = "is not a plain name for a field")]
fn a_set_field_is_named_plainly() {
    let f = Fixture::empty();
    let _ = (f.store.raw().row("task", "a")).set(json!({"title = 1, slug": 2}));
}

#[test]
#[should_panic(expected = "fields are given as an object")]
fn fields_are_an_object() {
    let f = Fixture::empty();
    let _ = f.store.raw().row("task", "a").set(json!(["title"]));
}

#[test]
#[should_panic(expected = "asked of one row")]
fn one_field_is_asked_of_one_row() {
    let f = Fixture::empty();
    let _ = f.store.raw().table("task").field("slug");
}
