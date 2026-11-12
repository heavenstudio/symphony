//! The legacy scalar is retired once; it never aliases current cached-input identities.

use crate::common::{Fixture, Seed, refused};
use serde_json::{Value, json};
use symphony_board::{Opening, Schema, Store, schema};

fn value(f: &Fixture, sql: &str) -> Value {
    f.store.client().query(sql, json!({})).unwrap().remove(0)
}

fn legacy_board() -> Fixture {
    let f = Fixture::new(
        Seed::default()
            .session("legacy")
            .task("tools/held", "ready"),
    );
    let before = value(&f, &format!("RETURN {};", schema::BOARD_MARKER));
    f.store
        .client()
        .query(
            "BEGIN TRANSACTION;
         UPDATE counter:board SET last = $marker_before;
         DELETE counter:board_structure;
         REMOVE TABLE session_revision;
         DEFINE EVENT OVERWRITE board_changed ON session WHEN $before != $after THEN {
            IF record::exists(counter:board) { UPDATE counter:board SET last += 1 RETURN NONE; }
            ELSE { CREATE counter:board SET last = 1 RETURN NONE; };
         };
         UPDATE meta:schema SET version = 14;
         COMMIT TRANSACTION;",
            json!({"marker_before":before}),
        )
        .unwrap();
    f
}

#[test]
fn migration_preserves_the_live_marker_and_retires_the_scalar_once() {
    let f = legacy_board();
    let original = f.store.dump().unwrap();
    let before = value(&f, "RETURN counter:board.last;");
    let upgraded = f.other();
    assert_eq!(value(&f, "RETURN counter:board.last;"), json!(i64::MIN));
    assert_eq!(value(&f, "RETURN counter:board_structure.last;"), before);
    assert_eq!(
        value(&f, &format!("RETURN {};", schema::BOARD_MARKER)),
        before
    );
    assert_eq!(upgraded.dump().unwrap(), original);
    assert_eq!(
        value(&f, "RETURN meta:schema.version;"),
        json!(Schema::this_build().version())
    );
    let warm = f.other();
    assert_eq!(warm.client().requests(), 1, "a current open writes nothing");
    assert_eq!(value(&f, "RETURN counter:board_structure.last;"), before);
    assert_eq!(value(&f, "RETURN counter:board.last;"), json!(i64::MIN));
}

#[test]
fn a_failed_migration_rolls_back_both_marker_transfer_and_retirement() {
    let f = legacy_board();
    let before = value(&f, "RETURN counter:board.last;");
    let original = f.store.dump().unwrap();
    let stopped = Schema::this_build().then("THROW 'retirement-aborted';");
    let result = Store::opening(
        f.other_client(),
        Box::new(|| crate::common::NOW),
        Box::new(|_| false),
        crate::common::HOST,
        &Opening::write().at(stopped),
    );
    let error = result
        .err()
        .expect("the failing step must abort the migration");
    assert!(error.to_string().contains("retirement-aborted"), "{error}");
    assert_eq!(value(&f, "RETURN meta:schema.version;"), json!(14));
    assert_eq!(value(&f, "RETURN counter:board.last;"), before);
    assert_eq!(
        value(&f, "RETURN record::exists(counter:board_structure);"),
        json!(false)
    );
    assert_eq!(
        value(&f, "RETURN (INFO FOR DB).tables.session_revision ?? NULL;"),
        Value::Null
    );
    assert_eq!(f.store.dump().unwrap(), original);
    f.other();
    assert_eq!(value(&f, "RETURN counter:board_structure.last;"), before);
    assert_eq!(value(&f, "RETURN counter:board.last;"), json!(i64::MIN));
}

#[test]
fn a_retired_scalar_without_its_successor_refuses_instead_of_resetting_the_marker() {
    let f = Fixture::empty();
    f.store.client().query(
        "BEGIN TRANSACTION; DELETE counter:board_structure; UPDATE meta:schema SET version = 14; COMMIT TRANSACTION;",
        json!({}),
    ).unwrap();
    let result = Store::opening(
        f.other_client(),
        Box::new(|| crate::common::NOW),
        Box::new(|_| false),
        crate::common::HOST,
        &Opening::write(),
    );
    assert_eq!(
        refused(result.map(|_| ())),
        "structural marker missing after legacy marker retirement"
    );
    assert_eq!(value(&f, "RETURN meta:schema.version;"), json!(14));
    assert_eq!(value(&f, "RETURN counter:board.last;"), json!(i64::MIN));
    assert_eq!(
        value(&f, "RETURN record::exists(counter:board_structure);"),
        json!(false)
    );
}
