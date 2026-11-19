//! The copy of a whole board: a dump (every row, table by table, [`Dump`]) written in one
//! transaction, replacing what the board held, and read back in the same shape.

use super::postgres::{self, Pg};
use super::statement::both;
use super::surreal::copy as sql;
use super::transaction::Run;
use super::{Error, Store, values};
use crate::cbor;
use crate::rows::Dump;
use crate::server::Storage;
use serde_json::{Value, json};
use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

impl Store {
    /// Remove every copy's staging table (`copy_…`): what a copy killed while it staged leaves, or
    /// one whose cleanup failed. Every copy starts with it, so a copy made while another stages
    /// takes the other's staging away, and the other's swap, which counts what it staged before
    /// it writes ([`sql::swap`]), is refused: the board is left to the later copy, as two copies
    /// of one board at once were never both kept. A board on PostgreSQL has none: its copy is
    /// one request ([`postgres::copy`]).
    pub fn remove_staging(&self) -> Result<(), Error> {
        if self.db.is_postgresql() {
            return Ok(());
        }
        let tables: Vec<String> = self.retrying(|| self.transact(sql::TABLES, json!({})))?;
        let staged = tables.iter().map(String::as_str).filter(|table| {
            table.starts_with("copy_")
                && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        self.remove_each(staged)
    }

    /// Each of `tables` removed if it is there, a transaction each: one transaction must fit the
    /// server's memtable, and the staging of a copy that did not fit it would not either. Every
    /// table is tried; the first failure is the answer.
    fn remove_each<'a>(&self, tables: impl Iterator<Item = &'a str>) -> Result<(), Error> {
        let mut failed = Ok(());
        for table in tables {
            let removed =
                self.staging_write(&sql::remove_tables(std::iter::once(table)), json!({}));
            failed = failed.and(removed);
        }
        failed
    }

    /// Make the board `dump`, replacing whatever was there. The rows go up in requests the server takes
    /// and land in one transaction: a dump that cannot be written leaves the board as it was. That
    /// transaction replaces the board's recovery generation too ([`super::recovery`]), so a token
    /// read before the copy is refused after it and one read before a copy that failed still
    /// holds; the new generation is then published to this store's authority, when it has one.
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
        if let Some(wait) = self.db.postgresql_wait() {
            let limit = copy_limit(wait);
            let generation =
                self.copy_in_one(&tables, last, last_ts, last_task, last_ask, limit)?;
            return self.copied(&generation);
        }
        self.remove_staging().map_err(|error| {
            said_after(
                error,
                "; staging left by an earlier copy could not be removed",
            )
        })?;
        let staging = format!(
            "copy_{:016x}",
            RandomState::new().hash_one(std::process::id())
        );
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        let copied = self.stage(&staging, &tables).and_then(|staged| {
            let swap = sql::swap(&staging, &names);
            let sent: serde_json::Map<String, Value> = (tables.iter())
                .map(|(table, rows)| ((*table).to_string(), json!(rows.len())))
                .collect();
            let vars = json!({
                "last": last, "last_ts": last_ts, "last_task": last_task, "last_ask": last_ask,
                "sent": sent,
            });
            self.retrying(|| self.transact::<String>(&swap, vars.clone()))
                .map_err(|error| uncommitted(error, staged, &tables))
        });
        let generation = copied.map_err(|error| self.cleanup_staging(&staging, &names, error))?;
        self.copied(&generation)
    }

    /// On PostgreSQL, `tables` (each a table and its rows) into the emptied board in one guarded
    /// transaction, with the counters the dump's rows end at: the copy of a board one request
    /// takes ([`postgres::copy`]), which the server ends at `limit`. Its answer is the recovery
    /// generation it made.
    fn copy_in_one(
        &self,
        tables: &[(&str, Vec<Value>)],
        last: i64,
        last_ts: Option<f64>,
        last_task: i64,
        last_ask: i64,
        limit: Duration,
    ) -> Result<String, Error> {
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
                .map(|row| {
                    json!({
                        "id": row["slug"], "original": row.get("fields")
                            .map_or_else(|| "{}".to_string(), Value::to_string),
                    })
                })
                .collect(),
        );
        vars["copy_limit"] = json!(format!("{:.0} s", limit.as_secs_f64()));
        let millis = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
        let copy = || both(sql::NOTHING_COPIED, postgres::copy::copy(&names, millis));
        self.retrying(|| self.run_transaction(copy(), vars.clone(), Run::Staging))
    }

    /// Every row of `tables` into schemaless tables named `{staging}_{table}`, each request at most
    /// `CHUNK_BYTES`: a board is bigger than the server takes in one. The copy's one transaction then
    /// moves them in. Each request starts with the schema guard in its transaction, so a migration
    /// between requests refuses the next batch before it writes. Its answer is the bytes it staged,
    /// the rows as they are sent.
    pub(super) fn stage(&self, staging: &str, tables: &[(&str, Vec<Value>)]) -> Result<u64, Error> {
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        self.staging_write(&sql::define_staged(staging, &names), json!({}))?;
        let mut staged = 0;
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
                staged += size as u64;
            }
            if !chunk.is_empty() {
                self.staging_write(&insert, json!({"rows": chunk}))?;
            }
        }
        Ok(staged)
    }

    /// Staging has no callback work, but each mutation must be fenced in its own transaction.
    /// `request` is a staging request whole, as its statement's name gives it.
    fn staging_write(&self, request: &str, vars: Value) -> Result<(), Error> {
        self.retrying(|| self.run_transaction::<Value>(request, vars.clone(), Run::Staging))
            .map(|_| ())
    }

    /// Keep the original failure and report cleanup's failure too, in the same line. A newer
    /// writer owns any staging left after a migration; this store may not remove it. What was
    /// staged is removed a table at a time ([`Store::remove_each`]); what could not be is cleared
    /// by the next copy, before it stages.
    pub(super) fn cleanup_staging(&self, staging: &str, tables: &[&str], error: Error) -> Error {
        let staged: Vec<String> = sql::staged(staging, tables);
        match self.remove_each(staged.iter().map(String::as_str)) {
            Ok(()) => error,
            Err(cleanup) => said_after(error, &format!("; staging cleanup failed: {cleanup}")),
        }
    }

    /// Every row of the board, in the shape the copy reads, each table in the order of its key: what a
    /// copy wrote, read back. An object inside a task's fields has its keys in their order as text,
    /// as SurrealDB keeps them, where PostgreSQL keeps the shorter first: a board's export is one text
    /// in either database. PostgreSQL also restores a copied field's numeric spelling while that
    /// value agrees with its saved JSON: jsonb alone expands exponents and forgets negative zero.
    /// Every task-field setter updates `copied_fields` at the assigned path; replacements do not
    /// inherit the old value's spelling. A copy replaces the text, and task deletion removes it.
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

/// The tables a move of the board to another engine carries beside the dump's
/// ([`Store::carried`]): what the board's commands keep for sessions and programs and no copy
/// derives, each keyed alike on both engines. `counter` is carried for its task holds alone
/// ([`HELD`]): its other rows are fences a command reads and moves within itself, and each
/// engine's own marker.
pub const CARRIED: [&str; 7] = [
    "callback_state",
    "callback_event",
    "chat_event",
    "writer",
    "artifact_ledger",
    "unanswered_restore",
    "counter",
];

/// What begins the id of a task's hold in `counter` (`counter:['hold', SLUG]`).
const HELD: &str = "hold";

/// A carried row's id as either engine keys it: SurrealDB lists a record's id as its table and
/// its key, PostgreSQL as the key alone.
fn key(table: &str, id: &Value) -> Value {
    match id.as_array().map(Vec::as_slice) {
        Some([named, key]) if named == table => key.clone(),
        _ => id.clone(),
    }
}

/// Whether `id`, a key of `counter`, is a task's hold.
fn hold(id: &Value) -> bool {
    id.get(0).and_then(Value::as_str) == Some(HELD)
}

impl Store {
    /// Every row of the tables a move carries beside the dump ([`CARRIED`]), by table, each
    /// with its key as `id` and in the order of its key as text: what [`Store::carry`] writes
    /// on the other engine, and what is compared with its reading there.
    pub fn carried(&self) -> Result<Value, Error> {
        let raw = self.raw();
        let mut tables = serde_json::Map::new();
        for table in CARRIED {
            let listed = raw.table(table).get()?;
            let mut rows: Vec<Value> = (listed.as_array().into_iter().flatten())
                .map(|row| {
                    let mut row = row.clone();
                    row["id"] = key(table, &row["id"]);
                    row.sort_all_objects();
                    row
                })
                .filter(|row| table != "counter" || hold(&row["id"]))
                .collect();
            crate::order::sort_by(&mut rows, &|a, b| {
                a["id"].to_string().cmp(&b["id"].to_string())
            });
            tables.insert(table.to_string(), Value::Array(rows));
        }
        Ok(Value::Object(tables))
    }

    /// `carried`, another board's ([`Store::carried`]), written here row by row under its keys,
    /// replacing what those tables held: the task holds alone of `counter`. A table it does not
    /// know is refused before anything is written.
    pub fn carry(&self, carried: &Value) -> Result<(), Error> {
        let tables = carried.as_object().cloned().unwrap_or_default();
        if let Some(unknown) = tables
            .keys()
            .find(|table| !CARRIED.contains(&table.as_str()))
        {
            return Err(Error::Refused(format!(
                "{unknown} is no table a move carries: nothing was written"
            )));
        }
        let raw = self.raw();
        for (table, rows) in &tables {
            if table == "counter" {
                let listed = raw.table(table).get()?;
                for row in listed.as_array().into_iter().flatten() {
                    let id = key(table, &row["id"]);
                    if hold(&id) {
                        raw.row(table, id).delete()?;
                    }
                }
            } else {
                raw.table(table).delete()?;
            }
            for row in rows.as_array().into_iter().flatten() {
                let mut fields = row.clone();
                let id = fields
                    .as_object_mut()
                    .and_then(|fields| fields.remove("id"))
                    .unwrap_or(Value::Null);
                raw.row(table, id).create(fields)?;
            }
        }
        Ok(())
    }
}

impl Store {
    /// The board's marker ([`Store::marker`]) moved past `other`, the marker of the board this
    /// one was copied from: what it is then. A reader that kept that board's rows under its
    /// marker, and reaches this one where that one was, never meets the same number over other
    /// rows, however few commits this board has seen.
    pub fn marker_past(&self, other: u64) -> Result<u64, Error> {
        let now = self.marker()?;
        if now > other {
            return Ok(now);
        }
        // Each engine keeps the part of its marker no session owns in a row of its own.
        let id = if self.database().is_postgresql() {
            serde_json::json!(["board"])
        } else {
            serde_json::json!("board_structure")
        };
        let (raw, more) = (self.raw(), other - now + 1);
        let row = raw.row("counter", id);
        if row.exists()? {
            row.add("last", more)?;
        } else {
            row.create(serde_json::json!({"last": more}))?;
        }
        let moved = self.marker()?;
        if moved <= other {
            return Err(Error::Refused(format!(
                "the board's marker reads {moved} after it was moved past {other}"
            )));
        }
        Ok(moved)
    }
}

/// `error` with `more` after its words.
fn said_after(mut error: Error, more: &str) -> Error {
    let (Error::Refused(message) | Error::Db(message)) = &mut error;
    message.push_str(more);
    error
}

/// What SurrealDB answers for every statement of a transaction it did not commit, when none of
/// them said why: the commit itself failed.
const UNCOMMITTED: &str = "The query was not executed due to a failed transaction";

/// `error`, the swap's, said for what it is when the server gave no reason of a statement's: the
/// copy's one transaction was not committed, which is how SurrealKV ends one that does not fit
/// its memtable ([`Storage`]), so the line names the rows and the bytes the copy `staged`, the
/// setting and what the board's server runs under. The board is as it was either way; any other
/// refusal keeps its own words.
fn uncommitted(error: Error, staged: u64, tables: &[(&str, Vec<Value>)]) -> Error {
    if !matches!(&error, Error::Db(said) if said == UNCOMMITTED) {
        return error;
    }
    let rows: Vec<String> = (tables.iter())
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(table, rows)| format!("{} of {table}", rows.len()))
        .collect();
    Error::Db(format!(
        "the server did not commit the copy's one transaction, and the board is as it was: the \
         copy staged {} of rows ({}), and a transaction must fit the memtable of the server it \
         runs on, which lays one out in several times its rows' bytes \
         (SURREAL_SURREALKV_MAX_MEMTABLE_SIZE, {} on a server the board starts): copy it into a \
         server started with a larger memtable",
        crate::size(staged),
        rows.join(", "),
        crate::size(Storage::BOARD.memtable),
    ))
}

/// How long the server gives a copy that is one request, of a client that waits `wait` for it:
/// thirty seconds short of the wait, or half of a wait under a minute, so the server's own word
/// that it ended the copy reaches the caller before the caller stops waiting. A copy still
/// running when its caller gives up would commit afterwards; what is left is a copy that
/// finished inside the limit and whose commit and answer then took longer than the difference.
fn copy_limit(wait: Duration) -> Duration {
    wait - (wait / 2).min(Duration::from_secs(30))
}

/// The most a copy sends in one request: the server takes 4 MiB (`SURREAL_HTTP_MAX_RPC_BODY_SIZE`).
pub(super) const CHUNK_BYTES: usize = 1 << 20;

/// Restore numbers from the saved JSON, which field setters maintain at the assigned paths.
/// Agreement with its jsonb form is a consistency check, not proof of element identity: an
/// assignment must first replace the saved subtree, including an entire assigned array.
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
    fn a_copy_is_ended_thirty_seconds_before_its_callers_wait_or_at_half_a_short_one() {
        let seconds = |wait: u64| copy_limit(Duration::from_secs(wait)).as_secs_f64();
        assert_eq!(seconds(300), 270.0);
        assert_eq!(seconds(60), 30.0);
        assert_eq!(seconds(4), 2.0);
        assert_eq!(seconds(0), 0.0);
        assert_eq!(
            copy_limit(Duration::from_millis(500)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn a_swap_the_server_did_not_commit_says_the_rows_and_bytes_staged_and_the_setting() {
        let tables = [
            ("task", vec![json!({}); 3]),
            ("link", vec![]),
            ("message", vec![json!({}); 40]),
        ];
        let Error::Db(said) = uncommitted(Error::Db(UNCOMMITTED.into()), 300 << 20, &tables) else {
            panic!("a failure of the database's");
        };
        assert_eq!(
            said,
            "the server did not commit the copy's one transaction, and the board is as it was: \
             the copy staged 300.0 MiB of rows (3 of task, 40 of message), and a transaction must \
             fit the memtable of the server it runs on, which lays one out in several times its \
             rows' bytes (SURREAL_SURREALKV_MAX_MEMTABLE_SIZE, 256.0 MiB on a server the board \
             starts): copy it into a server started with a larger memtable"
        );
        assert!(!said.contains('\n'));
        // A refusal that says why keeps its words, whoever's it is.
        for kept in [
            Error::Db("Found 'shout' for field `kind`".into()),
            Error::Refused("the board moved to schema 23".into()),
        ] {
            let was = format!("{kept:?}");
            assert_eq!(format!("{:?}", uncommitted(kept, 1, &tables)), was);
        }
    }

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

/// The tables a move takes by the dump ([`Store::dump`], [`Store::copy`]).
#[cfg(test)]
const DUMPED: [&str; 11] = [
    "task",
    "link",
    "session",
    "role",
    "message",
    "delivery",
    "delivery_key",
    "barred_key",
    "task_session",
    "ask",
    "usage",
];

/// The tables a move leaves, each with why nothing of the board is lost with it.
#[cfg(test)]
const LEFT: [(&str, &str); 7] = [
    (
        "recovery",
        "the copy gives the new board a generation of its own",
    ),
    (
        "meta",
        "the opening that makes the new board writes its schema",
    ),
    (
        "copied_fields",
        "written by the copy, from the rows it copies",
    ),
    ("said", "written by the copy: each sender's last word"),
    ("words", "written by the copy: the search indexes"),
    (
        "session_revision",
        "a session's part of the marker, which the move raises past the old board's",
    ),
    (
        "callback_lease",
        "a dispatcher's lease of seconds, taken again; no dispatcher runs across a hold",
    ),
];

#[cfg(test)]
mod placed {
    use super::*;

    /// A table added to the board is copied, carried or left on purpose: one in none of the
    /// three would be dropped by a move of the board to another engine without a word.
    #[test]
    fn every_table_of_the_board_is_copied_carried_or_left_with_its_reason() {
        let placed: Vec<&str> = (DUMPED.into_iter())
            .chain(CARRIED)
            .chain(LEFT.iter().map(|(table, _)| *table))
            .collect();
        for table in crate::store::postgres::schema::TABLES {
            assert_eq!(
                placed.iter().filter(|name| **name == table.name).count(),
                1,
                "{} is placed once among DUMPED, CARRIED and LEFT (store/copy.rs): a table a \
                 move neither copies nor carries is lost by it",
                table.name
            );
        }
        assert_eq!(placed.len(), crate::store::postgres::schema::TABLES.len());
        assert!(LEFT.iter().all(|(_, why)| !why.is_empty()));
    }

    #[test]
    fn a_carried_key_reads_alike_from_both_engines_and_only_a_hold_is_carried_of_counter() {
        use serde_json::json;
        assert_eq!(
            key("writer", &json!(["writer", ["host", "/bin"]])),
            json!(["host", "/bin"])
        );
        assert_eq!(
            key("writer", &json!(["host", "/bin"])),
            json!(["host", "/bin"])
        );
        assert_eq!(key("chat_event", &json!("7")), json!("7"));
        assert!(hold(&json!(["hold", "sim/jump"])));
        for other in [
            json!(["slots", "h"]),
            json!("message"),
            json!(["board", 7]),
            json!(null),
        ] {
            assert!(!hold(&other), "{other}");
        }
    }
}
