//! The copy of a whole board on PostgreSQL (`store/copy.rs`): the dump's rows into the emptied
//! board in one transaction, and the board read back in the dump's shape ([`DUMP`]). A board's
//! dump is one request here, the largest a board holds being far below what one takes, so a copy
//! stages nothing: one cut short leaves the board as it was, and is made again whole. What it
//! costs is its messages' terms, counted a row at a time by the search's trigger and written to
//! their index: time in proportion to the text, most of a copy's
//! ([the numbers](../../../../../docs/operations/analysis/board-store-baseline.md)).
//!
//! `copied_fields` keeps imported fields as text beside their task: jsonb cannot preserve a JSON
//! number's spelling or distinguish an exponent float from an integer. The dump gives both that
//! text and its jsonb normalization to `Store::dump`. Every task-field assignment updates its
//! exact path in the saved JSON, before jsonb loses the incoming number's spelling; an assigned
//! array or object replaces its whole subtree. The text is removed with a task and replaced with
//! a copy, in the same transaction. These definitions support fresh scratch PostgreSQL
//! boards; the version-only opening guard does not upgrade an earlier same-version shape.

use super::schema::{self, Kind};
use super::{Pg, row};
use std::fmt::Write as _;

/// The copy's one transaction: each of `tables` emptied and filled from `v -> table`, its rows as
/// the dump gives them with their ids (a schemaless table's each `{id, doc}`), each sender's last word made again by the messages'
/// trigger, the counters set to `last` and `last_ts` (messages), `last_task` and `last_ask`, and
/// the board's recovery generation replaced ([`super::recovery::ROTATE`]), which is its answer.
///
/// The server ends it once it has run `millis`, which the store sets short of its client's wait
/// (`Store::copy`): the request is sent whole, its end with it, so a copy its caller stopped
/// waiting for would otherwise run on and replace the board after the caller was told it failed.
/// Ended, it is rolled back whole and says so in one line (`v ->> 'copy_limit'` the time it was
/// given, in words), with the rows of each table it had copied and the table it was at.
pub(in crate::store) fn copy(tables: &[&str], millis: u64) -> Pg<'static> {
    let mut block = String::from(
        "DECLARE
    copied_ text := '';
    at_ text;
    rows_ bigint;
BEGIN
DELETE FROM said;
DELETE FROM workflow;\n",
    );
    for table in tables {
        let _ = writeln!(
            block,
            "at_ := '{table}';
DELETE FROM {table};
INSERT INTO {table} SELECT r.* FROM jsonb_populate_recordset(NULL::{table}, v -> '{table}') AS r;
GET DIAGNOSTICS rows_ = ROW_COUNT;
copied_ := copied_ || ' ' || rows_ || ' of {table},';"
        );
    }
    block.push_str(
        "at_ := NULL;
DELETE FROM copied_fields;
INSERT INTO copied_fields SELECT r.* FROM jsonb_populate_recordset(NULL::copied_fields, v -> 'copied_fields') AS r;
INSERT INTO counter (id, last, ts) VALUES ('\"message\"', (v ->> 'last')::bigint, (v ->> 'last_ts')::double precision)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last, ts = EXCLUDED.ts;
INSERT INTO counter (id, last) VALUES ('\"task\"', (v ->> 'last_task')::bigint)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last;
INSERT INTO counter (id, last) VALUES ('\"ask\"', (v ->> 'last_ask')::bigint)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last;\n",
    );
    block.push_str(super::recovery::ROTATE);
    block.push_str(
        "\nboard_answer := (SELECT to_jsonb(generation) FROM recovery WHERE id = '\"board\"');
EXCEPTION WHEN query_canceled THEN
    RAISE EXCEPTION '%', 'the copy did not finish in ' || (v ->> 'copy_limit')
        || ' and was ended, the board left as it was: it had copied'
        || CASE WHEN copied_ = '' THEN ' no table' ELSE rtrim(copied_, ',') END
        || coalesce(' and was at the ' || jsonb_array_length(v -> at_) || ' of ' || at_,
            ' and was at its counters');
END;",
    );
    Pg::Within(millis, block.into())
}

/// Every table of the dump: its name in the dump, its table, and the order of its key (a
/// schemaless table's in any, which `Store::dump` puts in its own).
pub(in crate::store) const DUMPED: [(&str, &str, &str); 16] = [
    ("tasks", "task", "sid"),
    ("task_links", "link", "slug, kind, target"),
    ("sessions", "session", "handle"),
    ("roles", "role", "slug, handle, role"),
    ("messages", "message", "sid"),
    ("deliveries", "delivery", "message, handle"),
    ("barred_keys", "barred_key", "key"),
    ("task_sessions", "task_session", "slug, handle"),
    ("delivery_keys", "delivery_key", "slug, key"),
    ("asks", "ask", "sid"),
    ("usages", "usage", "handle, slug, role, model, speed, round"),
    ("execution_attempts", "execution_attempt", "id"),
    ("execution_facts", "execution_fact", "id"),
    ("execution_fences", "execution_fence", "id"),
    ("execution_artifacts", "execution_artifact", "id"),
    ("workflow", "workflow", "id"),
];

/// Every row of the board, in the shape the copy reads, each table in the order of its key: a task
/// without its version, which a copy makes anew.
pub(in crate::store) static DUMP: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let tables: Vec<String> = DUMPED
        .iter()
        .map(|(name, table, order)| {
            let mut read = whole("d", table);
            if *table == "task" {
                read.push_str(" - 'version'");
            }
            // A workflow record is named by its id, which the task fence finds it by.
            if *table == "workflow" {
                read = "jsonb_build_object('key', d.id, 'kind', d.kind, 'body', d.body, 'at', d.at)"
                    .to_string();
            }
            format!(
                "'{name}', coalesce((SELECT jsonb_agg({read} ORDER BY {order}) FROM {table} d), '[]'::jsonb)"
            )
        })
        .collect();
    format!(
        "jsonb_build_array(jsonb_build_object({}),
        coalesce((SELECT jsonb_agg(jsonb_build_array(id, original, original::jsonb))
            FROM copied_fields), '[]'::jsonb))",
        tables.join(",\n    ")
    )
});

/// The row `alias` of `table` read as [`row`] reads one, but for its objects and arrays, kept
/// whole: a null inside one is a value the row holds, where the read of a row's own unset fields
/// would take it out.
fn whole(alias: &str, table: &str) -> String {
    if schema::table(table).is_some_and(|table| table.columns.is_empty()) {
        // A schemaless row is its document, whole.
        return format!("{alias}.doc");
    }
    let nested: Vec<&str> = (schema::table(table).map_or(&[][..], |table| table.columns))
        .iter()
        .filter(|column| matches!(column.kind, Kind::Object | Kind::Array))
        .map(|column| column.name)
        .collect();
    if nested.is_empty() {
        return row(alias);
    }
    let mut read = format!("jsonb_strip_nulls(to_jsonb({alias}) - 'id'");
    for name in &nested {
        let _ = write!(read, " - '{name}'");
    }
    read.push(')');
    for name in &nested {
        let _ = write!(
            read,
            " || CASE WHEN {alias}.{name} IS NULL THEN '{{}}'::jsonb \
             ELSE jsonb_build_object('{name}', {alias}.{name}) END"
        );
    }
    format!("({read})")
}
