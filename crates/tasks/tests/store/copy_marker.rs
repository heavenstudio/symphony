//! The board marker under the copy: a copy advances `counter:board` once a row, and a board whose
//! schema's events UPSERT the marker has them redefined when this schema opens it, moving no row.

use crate::common::{Fixture, Seed};
use crate::copy::everything;

/// The tables whose writes advance `counter:board` (`board_changed`).
const MARKED_TABLES: [&str; 9] = [
    "task",
    "session",
    "role",
    "link",
    "message",
    "delivery",
    "task_session",
    "callback_state",
    "chat_event",
];

fn marker(f: &Fixture) -> serde_json::Value {
    f.store
        .client()
        .query("RETURN counter:board.last;", serde_json::json!({}))
        .unwrap()
        .remove(0)
}

#[test]
fn a_copy_advances_the_board_marker_once_a_row_from_a_board_without_one() {
    // 2 tasks, links, sessions, roles and messages, a delivery and a task session; barred keys and
    // delivery keys none.
    let f = Fixture::new(Seed { dump: everything() });
    assert_eq!(marker(&f), serde_json::json!(12));
    f.store.copy(&everything()).unwrap();
    assert_eq!(marker(&f), serde_json::json!(36), "12 removed, 12 written");
}

/// Schema 10's events UPSERT the marker: on a record that exists that tries a create and rolls it
/// back, and SurrealKV's rollback walks every key the transaction wrote, so the copy's one
/// transaction cost time quadratic in its rows. This schema's open redefines them, moving no row.
#[test]
fn a_board_of_schema_ten_has_its_marker_events_redefined_and_its_rows_kept() {
    let f = Fixture::new(Seed { dump: everything() });
    let events = || -> Vec<String> {
        MARKED_TABLES
            .iter()
            .map(|table| {
                let sql = format!("RETURN (INFO FOR TABLE {table}).events.board_changed;");
                let found = f.store.client().query(&sql, serde_json::json!({}));
                serde_json::from_value(found.unwrap().remove(0)).unwrap()
            })
            .collect()
    };
    let old: String = MARKED_TABLES
        .iter()
        .map(|table| format!("DEFINE EVENT OVERWRITE board_changed ON {table} WHEN $before != $after THEN {{ UPSERT counter:board SET last = (last ?? 0) + 1; }};"))
        .collect();
    f.store
        .client()
        .query(
            &format!(
                "BEGIN TRANSACTION; {old} UPSERT meta:schema SET version = 10; COMMIT TRANSACTION;"
            ),
            serde_json::json!({}),
        )
        .unwrap();
    assert!(events().iter().all(|e| e.contains("UPSERT counter:board")));
    let before = marker(&f);
    let reopened = f.other();
    let redefined = events();
    assert!(
        redefined
            .iter()
            .all(|e| e.contains("UPDATE counter:board") && !e.contains("UPSERT")),
        "{redefined:#?}"
    );
    assert_eq!(reopened.dump().unwrap(), everything());
    assert_eq!(marker(&f), before, "redefining moves no row");
    reopened.copy(&everything()).unwrap();
    assert_eq!(marker(&f), serde_json::json!(36));
}
