//! The copy of a whole board: a dump (every row, table by table, [`Dump`]) written in one
//! transaction, replacing what the board held, and read back in the same shape.

use super::postgres::{self, Pg};
use super::statement::both;
use super::surreal::copy as sql;
use super::transaction::Run;
use super::{Error, Store, values};
use crate::cbor;
use crate::rows::Dump;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};

impl Store {
    /// Remove every copy's staging table (`copy_…`): what a copy killed while it staged leaves. Only for
    /// a caller that knows no copy runs meanwhile. A board on PostgreSQL has none: its copy is one
    /// request ([`postgres::copy`]).
    pub fn remove_staging(&self) -> Result<(), Error> {
        if self.db.is_postgresql() {
            return Ok(());
        }
        let tables: Vec<String> = self.retrying(|| self.transact(sql::TABLES, json!({})))?;
        let staged = tables.iter().map(String::as_str).filter(|table| {
            table.starts_with("copy_")
                && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        self.staging_write(&sql::remove_tables(staged), json!({}))
    }

    /// Make the board `dump`, replacing whatever was there. The rows go up in requests the server takes
    /// and land in one transaction: a dump that cannot be written leaves the board as it was.
    pub fn copy(&self, dump: &Dump) -> Result<(), Error> {
        let keyed = |rows: Vec<Value>, key: &dyn Fn(&Value) -> Value| -> Vec<Value> {
            rows.into_iter()
                .map(|mut row| {
                    row["id"] = key(&row);
                    row
                })
                .collect()
        };
        // SQLite's row id is the record's `sid`.
        let with_sid = |mut row: Value| {
            row["sid"] = row["id"].clone();
            row
        };
        let tables = [
            (
                "task",
                keyed(
                    values(&dump.tasks).into_iter().map(with_sid).collect(),
                    &|r| r["slug"].clone(),
                ),
            ),
            (
                "link",
                keyed(values(&dump.task_links), &|r| {
                    json!([r["slug"], r["kind"], r["target"]])
                }),
            ),
            (
                "session",
                keyed(values(&dump.sessions), &|r| r["handle"].clone()),
            ),
            (
                "role",
                keyed(values(&dump.roles), &|r| {
                    json!([r["slug"], r["handle"], r["role"]])
                }),
            ),
            (
                "message",
                keyed(
                    values(&dump.messages).into_iter().map(with_sid).collect(),
                    &|r| json!([r["slug"], r["sid"]]),
                ),
            ),
            (
                "delivery",
                keyed(values(&dump.deliveries), &|r| {
                    json!([r["message"], r["handle"]])
                }),
            ),
            (
                "barred_key",
                keyed(values(&dump.barred_keys), &|r| r["key"].clone()),
            ),
            (
                "task_session",
                keyed(values(&dump.task_sessions), &|r| {
                    json!([r["slug"], r["handle"]])
                }),
            ),
            (
                "delivery_key",
                keyed(values(&dump.delivery_keys), &|r| {
                    json!([r["slug"], r["key"]])
                }),
            ),
            (
                "ask",
                keyed(
                    values(&dump.asks).into_iter().map(with_sid).collect(),
                    &|r| r["sid"].clone(),
                ),
            ),
            (
                "usage",
                keyed(values(&dump.usages), &|r| {
                    json!([
                        r["handle"],
                        r["slug"],
                        r["role"],
                        r["model"],
                        r["speed"],
                        r["round"]
                    ])
                }),
            ),
        ];
        let last = dump.messages.iter().map(|m| m.id).max().unwrap_or(0);
        let last_task = dump.tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let last_ask = dump.asks.iter().map(|a| a.id).max().unwrap_or(0);
        let last_ts = dump.messages.iter().map(|m| m.ts).reduce(f64::max);
        if self.db.is_postgresql() {
            return self.copy_in_one(&tables, last, last_ts, last_task, last_ask);
        }
        let staging = format!(
            "copy_{:016x}",
            RandomState::new().hash_one(std::process::id())
        );
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        let copied = self.stage(&staging, &tables).and_then(|()| {
            let swap = sql::swap(&staging, &names);
            let vars = json!({
                "last": last, "last_ts": last_ts, "last_task": last_task, "last_ask": last_ask,
            });
            self.retrying(|| self.transact::<Value>(&swap, vars.clone()))
                .map(|_| ())
        });
        copied.map_err(|error| self.cleanup_staging(&sql::remove_staged(&staging, &names), error))
    }

    /// On PostgreSQL, `tables` (each a table and its rows) into the emptied board in one guarded
    /// transaction, with the counters the dump's rows end at: the copy of a board one request
    /// takes ([`postgres::copy`]).
    fn copy_in_one(
        &self,
        tables: &[(&str, Vec<Value>)],
        last: i64,
        last_ts: Option<f64>,
        last_task: i64,
        last_ask: i64,
    ) -> Result<(), Error> {
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        let mut vars = json!({
            "last": last, "last_ts": last_ts, "last_task": last_task, "last_ask": last_ask,
        });
        for (table, rows) in tables {
            let rows: Vec<Value> = (rows.iter().cloned())
                .map(|mut row| {
                    // A task row is made at its first version, as SurrealDB's field default makes it.
                    if *table == "task" {
                        row["version"] = json!(0);
                    }
                    row
                })
                .collect();
            vars[*table] = Value::Array(rows);
        }
        vars["copied_fields"] = Value::Array(
            tables
                .iter()
                .filter(|(table, _)| *table == "task")
                .flat_map(|(_, rows)| rows)
                .filter_map(|row| {
                    row.get("fields").map(|fields| {
                        json!({
                            "id": row["slug"], "original": fields.to_string(),
                        })
                    })
                })
                .collect(),
        );
        let copy = || both(sql::NOTHING_COPIED, postgres::copy::copy(&names));
        self.retrying(|| self.run_transaction::<Value>(copy(), vars.clone(), Run::Staging))
            .map(|_| ())
    }

    /// Every row of `tables` into schemaless tables named `{staging}_{table}`, each request at most
    /// `CHUNK_BYTES`: a board is bigger than the server takes in one. The copy's one transaction then
    /// moves them in. Each request starts with the schema guard in its transaction, so a migration
    /// between requests refuses the next batch before it writes.
    pub(super) fn stage(&self, staging: &str, tables: &[(&str, Vec<Value>)]) -> Result<(), Error> {
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        self.staging_write(&sql::define_staged(staging, &names), json!({}))?;
        for (table, rows) in tables {
            let insert = sql::insert_staged(staging, table);
            let (mut chunk, mut bytes) = (Vec::new(), 0);
            for row in rows {
                let size = cbor::encode(row).len();
                if bytes + size > CHUNK_BYTES && !chunk.is_empty() {
                    self.staging_write(&insert, json!({"rows": std::mem::take(&mut chunk)}))?;
                    bytes = 0;
                }
                chunk.push(json!({"row": row}));
                bytes += size;
            }
            if !chunk.is_empty() {
                self.staging_write(&insert, json!({"rows": chunk}))?;
            }
        }
        Ok(())
    }

    /// Staging has no callback work, but each mutation must be fenced in its own transaction.
    /// `request` is a staging request whole, as its statement's name gives it.
    fn staging_write(&self, request: &str, vars: Value) -> Result<(), Error> {
        self.retrying(|| self.run_transaction::<Value>(request, vars.clone(), Run::Staging))
            .map(|_| ())
    }

    /// Keep the original failure and report cleanup's failure too. A newer writer owns any staging
    /// left after a migration; this store may not remove it. `remove` is the staging request that
    /// removes what was staged.
    pub(super) fn cleanup_staging(&self, remove: &str, mut error: Error) -> Error {
        if let Err(cleanup) = self.staging_write(remove, json!({})) {
            let (Error::Refused(message) | Error::Db(message)) = &mut error;
            let _ = write!(message, "; staging cleanup failed: {cleanup}");
        }
        error
    }

    /// Every row of the board, in the shape the copy reads, each table in the order of its key: what a
    /// copy wrote, read back. An object inside a task's fields has its keys in their order as text,
    /// as SurrealDB keeps them, where PostgreSQL keeps the shorter first: a board's export is one text
    /// in either database. PostgreSQL also restores a copied field's numeric spelling while that
    /// value is unchanged: jsonb alone expands exponents and forgets negative zero. The original
    /// fields text in `copied_fields` is replaced with the copy and removed with the task.
    pub fn dump(&self) -> Result<Dump, Error> {
        let dump = || both(sql::DUMP, Pg::read(postgres::copy::DUMP.as_str()));
        let mut dump: Dump = if self.db.is_postgresql() {
            let (mut dump, copied): (Dump, Vec<(String, String, Value)>) =
                self.retrying(|| self.transact(dump(), json!({})))?;
            for (slug, original, normalized) in copied {
                let original: Value = serde_json::from_str(&original)
                    .map_err(|e| Error::Db(format!("copied fields: {e}")))?;
                if let Some(task) = dump.tasks.iter_mut().find(|task| task.slug == slug) {
                    for (namespace, fields) in &mut task.fields {
                        for (name, value) in fields {
                            restore_numbers(
                                value,
                                &original[namespace][name],
                                &normalized[namespace][name],
                            );
                        }
                    }
                }
            }
            dump
        } else {
            self.retrying(|| self.transact(dump(), json!({})))?
        };
        let fields = (dump.tasks.iter_mut()).flat_map(|task| task.fields.values_mut());
        for value in fields.flat_map(|namespace| namespace.values_mut()) {
            value.sort_all_objects();
        }
        Ok(dump)
    }
}

/// The most a copy sends in one request: the server takes 4 MiB (`SURREAL_HTTP_MAX_RPC_BODY_SIZE`).
pub(super) const CHUNK_BYTES: usize = 1 << 20;

/// Restore a copied number's spelling only while the current value is still its jsonb spelling.
/// Numbers live at arbitrary object/array depths; edits, removals and additions remain visible.
fn restore_numbers(current: &mut Value, original: &Value, normalized: &Value) {
    match current {
        Value::Number(_) if original.is_number() && current == normalized => {
            *current = original.clone()
        }
        Value::Object(fields) => {
            for (name, value) in fields {
                restore_numbers(value, &original[name], &normalized[name]);
            }
        }
        Value::Array(items) => {
            for (at, value) in items.iter_mut().enumerate() {
                restore_numbers(value, &original[at], &normalized[at]);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_restoration_preserves_edits_and_walks_arrays_and_objects() {
        let original: Value = serde_json::from_str(r#"{"same":1e30,"changed":1e30,"kind":1e30,"gone":1e30,"array":[1e18,-0.0,1e30],"old_object":{"x":1e30}}"#).unwrap();
        let normalized: Value = serde_json::from_str(r#"{"same":1000000000000000000000000000000,"changed":1000000000000000000000000000000,"kind":1000000000000000000000000000000,"gone":1000000000000000000000000000000,"array":[1000000000000000000,0.0,1000000000000000000000000000000],"old_object":{"x":1000000000000000000000000000000}}"#).unwrap();
        let mut current = normalized.clone();
        current["changed"] = json!(7);
        current["kind"] = json!("1e30");
        current.as_object_mut().unwrap().remove("gone");
        current["array"].as_array_mut().unwrap().truncate(2);
        current["array"].as_array_mut().unwrap().push(json!(false));
        current["array"].as_array_mut().unwrap().push(json!(8));
        current["added"] = json!({"new": [null, true, 4]});
        current["old_object"] = json!([1]);
        let mut expected = current.clone();
        expected["same"] = original["same"].clone();
        expected["array"][0] = original["array"][0].clone();
        expected["array"][1] = original["array"][1].clone();
        restore_numbers(&mut current, &original, &normalized);
        assert_eq!(current, expected);
    }
}
