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
        self.copy_limited(dump, copy_limit)
    }

    /// [`Store::copy`], the server ending a PostgreSQL board's copy at `limit` of its client's
    /// wait: [`copy_limit`] for every copy the board takes. A test gives a limit of its own, so a
    /// copy it holds is ended soon while its client still waits on the server as any board's does.
    fn copy_limited(
        &self,
        dump: &Dump,
        limit: impl FnOnce(Duration) -> Duration,
    ) -> Result<(), Error> {
        let tables = copied_tables(dump);
        let last = dump.messages.iter().map(|m| m.id).max().unwrap_or(0);
        let last_task = dump.tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let last_ask = dump.asks.iter().map(|a| a.id).max().unwrap_or(0);
        let last_ts = dump.messages.iter().map(|m| m.ts).reduce(f64::max);
        if let Some(wait) = self.db.postgresql_wait() {
            let limit = limit(wait);
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
        let counters = json!({
            "last": last, "last_ts": last_ts, "last_task": last_task, "last_ask": last_ask,
        });
        let copied = self.stage(&staging, &tables).and_then(|staged| {
            self.swap_staged(&staging, &tables, counters)
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
            let schemaless =
                postgres::schema::table(table).is_some_and(|table| table.columns.is_empty());
            let rows: Vec<Value> = (rows.iter().cloned())
                .map(|mut row| {
                    // A task row is made at its first version, as SurrealDB's field default makes it.
                    if *table == "task" {
                        row["version"] = json!(0);
                    }
                    // A schemaless row is its id beside its document.
                    if schemaless && let Value::Object(fields) = &mut row {
                        let id = fields.remove("id").unwrap_or(Value::Null);
                        row = json!({"id": id, "doc": row});
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

    /// On SurrealDB, the copy's one transaction: what [`Store::stage`] put under `staging` replaces
    /// each of `tables` on the board, the counters are set as `counters` says and the recovery
    /// generation is replaced ([`sql::swap`]). Its answer is the generation it made. It is held to
    /// the store's own schema whoever opened the store ([`Run::Own`]), as its staging is: a
    /// reader's store, which may write a board one schema ahead whose step keeps it writing, never
    /// lands a copy staged at its own schema on a board a migration moved meanwhile. It posts no
    /// message, so it dispatches nothing.
    pub(super) fn swap_staged(
        &self,
        staging: &str,
        tables: &[(&str, Vec<Value>)],
        counters: Value,
    ) -> Result<String, Error> {
        let names: Vec<&str> = tables.iter().map(|(table, _)| *table).collect();
        let swap = sql::swap(staging, &names, self.db.database());
        let sent: serde_json::Map<String, Value> = (tables.iter())
            .map(|(table, rows)| ((*table).to_string(), json!(rows.len())))
            .collect();
        let mut vars = counters;
        vars["sent"] = Value::Object(sent);
        self.retrying(|| self.run_transaction::<String>(&swap, vars.clone(), Run::Own))
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
    ///
    /// Refused on a board of its contract with other additive steps than its own, whose tables and
    /// fields it would leave out ([`Store::with_own_steps`]).
    pub fn dump(&self) -> Result<Dump, Error> {
        self.with_own_steps(|| self.dump_read())
    }

    fn dump_read(&self) -> Result<Dump, Error> {
        let dump = || both(sql::DUMP, Pg::read(postgres::copy::DUMP.as_str()));
        let mut dump: Dump = if self.db.is_postgresql() {
            let (mut dump, copied): (Dump, Vec<(String, String, Value)>) =
                read_strictly(self.retrying(|| self.transact(dump(), json!({})))?)?;
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
            read_strictly(self.retrying(|| self.transact(dump(), json!({})))?)?
        };
        // The execution tables' rows in the order of their ids, and each row's fields in one order
        // at every depth, as the two databases neither return rows nor order a document's fields
        // alike (PostgreSQL's jsonb keeps the shorter keys first).
        let rows = [
            &mut dump.execution_attempts,
            &mut dump.execution_facts,
            &mut dump.execution_fences,
            &mut dump.execution_artifacts,
        ];
        rows.into_iter().flatten().for_each(Value::sort_all_objects);
        let text = |value: &Value| value.to_string();
        dump.execution_attempts
            .sort_by_key(|row| text(&row["attempt"]));
        dump.execution_facts
            .sort_by_key(|row| (text(&row["attempt"]), row["seq"].as_i64()));
        dump.execution_fences.sort_by_key(|row| text(&row["key"]));
        dump.execution_artifacts
            .sort_by_key(|row| (text(&row["attempt"]), text(&row["name"])));
        // Each database orders compound ids its own way (PostgreSQL's jsonb puts a shorter array
        // first): one order, so a board's export is one text in either.
        dump.workflow
            .sort_by(|a, b| crate::store::workflow::key_order(&a.key, &b.key));
        let fields = (dump.tasks.iter_mut()).flat_map(|task| task.fields.values_mut());
        for value in fields.flat_map(|namespace| namespace.values_mut()) {
            value.sort_all_objects();
        }
        Ok(dump)
    }
}

/// `read`, the board's rows as the dump reads them, as a `T`, refused where a row holds a field the
/// dump does not know, which a copy of it would drop: read exactly ([`crate::objects::read_exact`]),
/// where the store's answers hand an open table's row only its own ([`crate::rows::open_row`]).
fn read_strictly<T: serde::de::DeserializeOwned>(read: Value) -> Result<T, Error> {
    crate::objects::read_exact(&read)
        .map_err(|why| Error::Refused(format!("the board's rows are not the dump's: {why}")))
}

/// The tables a move of the board to another engine carries beside the dump's
/// ([`Store::carried`]): what the board's commands keep for sessions and programs and no copy
/// derives, each keyed alike on both engines. Of `counter` every row is carried but the ones
/// each engine or the copy makes for itself ([`own`]): the task holds, each recipient's place in
/// its outbox (the number its next line is keyed by), and the fences, which only ever count up.
pub const CARRIED: [&str; 9] = [
    "callback_state",
    "callback_lease",
    "callback_event",
    "chat_event",
    "writer",
    "artifact_ledger",
    "unanswered_restore",
    "counter",
    "setting",
];

/// How far a restored lease's revision is moved on where the cluster it replaces could not be
/// read: more takes than any holder of one lease ever made.
const UNREAD: i64 = 1 << 32;

/// The leases a snapshot put back over its board is given: `restored`, the snapshot's carried
/// tables ([`Store::carried`]), read beside `replaced`, those of the cluster it replaces, or
/// none where that one could not be read. Every lease of either, ended, its revision past the
/// greater of the two and of what its recipient last committed (`callback_state`), which a
/// recipient's next lease is taken past as well. A holder that took its lease after the
/// snapshot holds a revision the snapshot never saw, and the next taker would be handed the same
/// one: this is what makes it past both. Unread, each is moved on by [`UNREAD`] instead.
pub fn leases_past(restored: &Value, replaced: Option<&Value>) -> Vec<Value> {
    let rows = |of: &Value, table: &str| -> Vec<Value> {
        of.get(table)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let revision = |of: &Value, table: &str, id: &Value| -> i64 {
        (rows(of, table).iter())
            .find(|row| &row["id"] == id)
            .and_then(|row| row["revision"].as_i64())
            .unwrap_or(0)
    };
    let known = |of: &Value, id: &Value| {
        revision(of, "callback_lease", id).max(revision(of, "callback_state", id))
    };
    let mut leases = rows(restored, "callback_lease");
    for lease in replaced
        .map(|replaced| rows(replaced, "callback_lease"))
        .unwrap_or_default()
    {
        if !leases.iter().any(|kept| kept["id"] == lease["id"]) {
            leases.push(serde_json::json!({"id": lease["id"]}));
        }
    }
    for lease in &mut leases {
        let id = lease["id"].clone();
        let past = match replaced {
            Some(replaced) => known(restored, &id).max(known(replaced, &id)),
            None => known(restored, &id).saturating_add(UNREAD),
        };
        lease["until"] = serde_json::json!(0);
        lease["revision"] = serde_json::json!(past.saturating_add(1));
    }
    leases
}

/// The counters the copy sets from the rows it copies.
const COPIED: [&str; 3] = ["message", "task", "ask"];

/// A carried row's id as either engine keys it: SurrealDB lists a record's id as its table and
/// its key, PostgreSQL as the key alone.
fn key(table: &str, id: &Value, postgresql: bool) -> Value {
    match id.as_array().map(Vec::as_slice) {
        Some([named, key]) if !postgresql && named == table => key.clone(),
        _ => id.clone(),
    }
}

/// Whether `id`, a key of `counter`, is a row a board has of its own and no move carries: the
/// counters the copy sets ([`COPIED`]), and the part of the marker no session owns, which each
/// engine keeps its own way (`board_structure`, or `['board']` and one row a connection).
fn own(id: &Value) -> bool {
    match id {
        Value::String(name) => {
            // `board` is the marker of boards before `board_structure`, kept retired.
            ["board_structure", "board"].contains(&name.as_str()) || COPIED.contains(&name.as_str())
        }
        other => other.get(0).and_then(Value::as_str) == Some("board"),
    }
}

/// `carried`, a board's ([`Store::carried`]), as a move writes it on the other engine: every
/// lease ended and its revision moved on by one. The revision of a lease is the token its
/// holder's writes carry, so a holder from before the move meets a lease that is no longer its
/// own, and the next one taken is past both; a lease that stood would outlive the hold the move
/// ran under.
pub fn leases_ended(mut carried: Value) -> Value {
    let leases = carried
        .get_mut("callback_lease")
        .and_then(Value::as_array_mut);
    for lease in leases.into_iter().flatten() {
        if lease.get("until").is_some() {
            lease["until"] = serde_json::json!(0);
        }
        if let Some(revision) = lease.get("revision").and_then(Value::as_i64) {
            lease["revision"] = serde_json::json!(revision + 1);
        }
    }
    carried
}

impl Store {
    /// Every row of the tables a move carries beside the dump ([`CARRIED`]), by table, each
    /// with its key as `id` and in the order of its key as text: what [`Store::carry`] writes
    /// on the other engine, its leases ended first ([`leases_ended`]), and what is compared with
    /// its reading there.
    pub fn carried(&self) -> Result<Value, Error> {
        let (raw, postgresql) = (self.raw(), self.database().is_postgresql());
        let mut tables = serde_json::Map::new();
        for table in CARRIED {
            // A table of a step the board lacks holds nothing to carry.
            if self.lacks(table)? {
                tables.insert(table.to_string(), Value::Array(Vec::new()));
                continue;
            }
            let listed = raw.table(table).get()?;
            let mut rows: Vec<Value> = (listed.as_array().into_iter().flatten())
                .map(|row| {
                    let mut row = row.clone();
                    row["id"] = key(table, &row["id"], postgresql);
                    row.sort_all_objects();
                    row
                })
                .filter(|row| table != "counter" || !own(&row["id"]))
                .collect();
            crate::order::sort_by(&mut rows, &|a, b| {
                a["id"].to_string().cmp(&b["id"].to_string())
            });
            tables.insert(table.to_string(), Value::Array(rows));
        }
        Ok(Value::Object(tables))
    }

    /// Whether `table` is one an additive step defines that this store may not use on its board
    /// ([`Store::additions`]): one the board, or this build, lacks.
    fn lacks(&self, table: &str) -> Result<bool, Error> {
        let Some(step) =
            (crate::schema::ADDED.iter()).position(|step| step.tables.contains(&table))
        else {
            return Ok(false);
        };
        Ok(self.additions()? <= i64::try_from(step).unwrap_or(i64::MAX))
    }

    /// `carried`, another board's ([`Store::carried`]), written here row by row under its keys,
    /// replacing what those tables held: of `counter`, all but the rows a board has of its own.
    /// A table it does not know is refused before anything is written.
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
        let (raw, postgresql) = (self.raw(), self.database().is_postgresql());
        for (table, rows) in &tables {
            if self.lacks(table)? {
                if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                    return Err(Error::Refused(format!(
                        "{table} is a table of a step this board lacks: its rows cannot be \
                         carried here"
                    )));
                }
                continue;
            }
            if table == "counter" {
                let listed = raw.table(table).get()?;
                for row in listed.as_array().into_iter().flatten() {
                    let id = key(table, &row["id"], postgresql);
                    if !own(&id) {
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

/// The tables a copy of `dump` writes ([`Store::copy`]), in the order it writes them, each with its
/// rows as the board keeps them: each row's record id (`id`) made from its key, and on a table
/// whose SQLite row id the record's `sid` was, `sid` set to the dump's `id`.
fn copied_tables(dump: &Dump) -> Vec<(&'static str, Vec<Value>)> {
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
    // A workflow's records go in before the tasks: the task fence finds each managed task's
    // transition among them as the task is written (`crate::schema`'s `workflow_position`).
    // A dump with none lists no table for them, so its copy says what it said before
    // workflows; the copy empties the board's records either way. What the board accepted
    // from an approval authority is never copied: a copy or a restore may be older than the
    // authority's last report, so its guards are refused until the authority reports again
    // (`crate::workflow::guard`).
    let workflow: Vec<Value> = (dump.workflow.iter())
        .filter(|row| row.kind != "authority")
        .map(|row| json!({ "id": row.key, "kind": row.kind, "body": row.body, "at": row.at }))
        .collect();
    let workflow = (!workflow.is_empty()).then_some(("workflow", workflow));
    workflow
        .into_iter()
        .chain([
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
            (
                "execution_attempt",
                keyed(dump.execution_attempts.clone(), &|r| r["attempt"].clone()),
            ),
            (
                "execution_fact",
                keyed(dump.execution_facts.clone(), &|r| {
                    json!([r["attempt"], r["seq"]])
                }),
            ),
            (
                "execution_fence",
                keyed(dump.execution_fences.clone(), &|r| r["key"].clone()),
            ),
            (
                "execution_artifact",
                keyed(dump.execution_artifacts.clone(), &|r| {
                    json!([r["attempt"], r["name"]])
                }),
            ),
        ])
        .collect()
}

/// The tables a move leaves, each with why nothing of the board is lost with it.
#[cfg(test)]
const LEFT: [(&str, &str); 6] = [
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
];

#[cfg(test)]
mod placed {
    use super::*;

    /// The tables a move takes by the dump ([`Store::dump`], [`Store::copy`]): those the copy of
    /// a dump that holds a workflow writes.
    fn dumped() -> Vec<&'static str> {
        let workflow = crate::rows::WorkflowRow {
            key: json!("definition"),
            kind: "definition".into(),
            body: serde_json::Map::new(),
            at: 0.0,
        };
        let dump = Dump {
            workflow: vec![workflow],
            ..Dump::default()
        };
        copied_tables(&dump)
            .into_iter()
            .map(|(table, _)| table)
            .collect()
    }

    /// A table added to the board is copied, carried or left on purpose: one in none of the
    /// three would be dropped by a move of the board to another engine without a word.
    #[test]
    fn every_table_of_the_board_is_copied_carried_or_left_with_its_reason() {
        let placed: Vec<&str> = (dumped().into_iter())
            .chain(CARRIED)
            .chain(LEFT.iter().map(|(table, _)| *table))
            .collect();
        let contract = crate::store::postgres::schema::TABLES
            .iter()
            .map(|table| table.name);
        let added = (crate::schema::ADDED.iter()).flat_map(|step| step.tables.iter().copied());
        let tables: Vec<&str> = contract.chain(added).collect();
        for table in &tables {
            assert_eq!(
                placed.iter().filter(|name| *name == table).count(),
                1,
                "{table} is placed once among copied_tables, CARRIED and LEFT (store/copy.rs): a table a \
                 move neither copies nor carries is lost by it"
            );
        }
        assert_eq!(placed.len(), tables.len());
        assert!(LEFT.iter().all(|(_, why)| !why.is_empty()));
    }

    #[test]
    fn a_copy_writes_every_table_the_dump_reads_workflow_first_and_only_where_a_record_is_copied() {
        let names = |tables: &[(&str, Vec<Value>)]| -> Vec<String> {
            tables
                .iter()
                .map(|(table, _)| (*table).to_string())
                .collect()
        };
        let bare = copied_tables(&Dump::default());
        assert!(bare.iter().all(|(_, rows)| rows.is_empty()));
        assert_eq!(names(&bare), &dumped()[1..]);
        assert_eq!(dumped()[0], "workflow");
        // An authority's report is never copied: a dump holding nothing else lists no workflow.
        let report = |kind: &str| crate::rows::WorkflowRow {
            key: json!(["workflow", kind]),
            kind: kind.into(),
            body: serde_json::Map::from_iter([("said".to_string(), json!(kind))]),
            at: 7.0,
        };
        let reported = Dump {
            workflow: vec![report("authority")],
            ..Dump::default()
        };
        assert_eq!(names(&copied_tables(&reported)), names(&bare));
        let kept = Dump {
            workflow: vec![report("authority"), report("definition")],
            ..Dump::default()
        };
        let tables = copied_tables(&kept);
        assert_eq!(names(&tables), dumped());
        assert_eq!(
            tables[0].1,
            vec![json!({
                "id": ["workflow", "definition"], "kind": "definition",
                "body": {"said": "definition"}, "at": 7.0,
            })]
        );
        // The tables the dump reads back are those it writes.
        let mut read: Vec<&str> = (postgres::copy::DUMPED.iter())
            .map(|(_, table, _)| *table)
            .collect();
        let mut written = dumped();
        read.sort_unstable();
        written.sort_unstable();
        assert_eq!(read, written);
    }

    #[test]
    fn a_copied_row_is_keyed_as_the_board_keys_its_record_and_sid_is_its_dump_id() {
        use crate::rows::{AskRow, LinkRow, MessageRow, TaskRow};
        let dump = Dump {
            tasks: vec![TaskRow {
                id: 4,
                slug: "sim/jump".into(),
                ..TaskRow::default()
            }],
            task_links: vec![LinkRow {
                slug: "sim/jump".into(),
                kind: "dep".into(),
                target: "sim/land".into(),
            }],
            messages: vec![MessageRow {
                id: 9,
                slug: "sim/jump".into(),
                ..MessageRow::default()
            }],
            asks: vec![AskRow {
                id: 3,
                ..AskRow::default()
            }],
            execution_facts: vec![json!({"attempt": "a1", "seq": 2})],
            ..Dump::default()
        };
        let tables = copied_tables(&dump);
        let row = |table: &str| -> &Value {
            let (_, rows) = (tables.iter().find(|(name, _)| *name == table)).unwrap();
            assert_eq!(rows.len(), 1, "{table}");
            &rows[0]
        };
        let task = row("task");
        assert_eq!((&task["id"], &task["sid"]), (&json!("sim/jump"), &json!(4)));
        assert_eq!(row("link")["id"], json!(["sim/jump", "dep", "sim/land"]));
        let message = row("message");
        assert_eq!(
            (&message["id"], &message["sid"]),
            (&json!(["sim/jump", 9]), &json!(9))
        );
        let ask = row("ask");
        assert_eq!((&ask["id"], &ask["sid"]), (&json!(3), &json!(3)));
        assert_eq!(
            row("execution_fact"),
            &json!({"attempt": "a1", "seq": 2, "id": ["a1", 2]})
        );
    }

    #[test]
    fn a_carried_key_reads_alike_from_both_engines_and_a_boards_own_counters_are_not_carried() {
        use serde_json::json;
        let compound = json!(["writer", ["host", "/bin"]]);
        assert_eq!(key("writer", &compound, false), json!(["host", "/bin"]));
        // PostgreSQL lists the key alone, so one that begins with its table's name is whole.
        assert_eq!(key("writer", &compound, true), compound);
        assert_eq!(
            key("writer", &json!(["host", "/bin"]), true),
            json!(["host", "/bin"])
        );
        assert_eq!(key("chat_event", &json!("7"), false), json!("7"));
        for carried in [
            json!(["hold", "sim/jump"]),
            json!(["callback-event", "amber-heron"]),
            json!(["slots", "h"]),
            json!(["agent", "a"]),
            json!("tree"),
        ] {
            assert!(!own(&carried), "{carried}");
        }
        for kept in [
            json!("message"),
            json!("task"),
            json!("ask"),
            json!("board_structure"),
            json!("board"),
            json!(["board"]),
            json!(["board", 7]),
        ] {
            assert!(own(&kept), "{kept}");
        }
    }

    #[test]
    fn a_restored_lease_is_ended_past_the_cluster_it_replaces() {
        use serde_json::json;
        let restored = json!({
            "callback_lease": [
                {"id": "amber-heron", "until": 9.5, "revision": 4, "pull": 3},
                {"id": ["session", "amber-heron"], "until": 5, "revision": 1, "doing": "close"},
                {"id": "only-in-the-snapshot", "revision": 2},
                {"id": "state-ahead", "until": 1, "revision": 2},
            ],
            "callback_state": [
                {"id": "amber-heron", "revision": 3},
                {"id": "state-ahead", "revision": 6},
            ],
        });
        let replaced = json!({
            "callback_lease": [
                {"id": "amber-heron", "until": 99, "revision": 7},
                {"id": ["session", "amber-heron"], "until": 0, "revision": 1},
                {"id": ["dispatch", "this-host"], "until": 99, "revision": 5},
                {"id": "taken-since", "until": 99},
            ],
            "callback_state": [
                {"id": "amber-heron", "revision": 9},
                {"id": "taken-since", "revision": 11},
            ],
        });
        assert_eq!(
            leases_past(&restored, Some(&replaced)),
            vec![
                // Past the replaced cluster's committed revision, the greatest of the four.
                json!({"id": "amber-heron", "until": 0, "revision": 10, "pull": 3}),
                json!({"id": ["session", "amber-heron"], "until": 0, "revision": 2, "doing": "close"}),
                json!({"id": "only-in-the-snapshot", "until": 0, "revision": 3}),
                json!({"id": "state-ahead", "until": 0, "revision": 7}),
                // A lease taken since the snapshot is made, ended, past its holder's.
                json!({"id": ["dispatch", "this-host"], "until": 0, "revision": 6}),
                json!({"id": "taken-since", "until": 0, "revision": 12}),
            ]
        );
        assert_eq!(
            leases_past(&restored, None)[..2],
            [
                json!({"id": "amber-heron", "until": 0, "revision": UNREAD + 5, "pull": 3}),
                json!({"id": ["session", "amber-heron"], "until": 0, "revision": UNREAD + 2, "doing": "close"}),
            ]
        );
        assert_eq!(
            leases_past(&json!({}), Some(&json!({}))),
            Vec::<Value>::new()
        );
        assert_eq!(leases_past(&json!({}), None), Vec::<Value>::new());
    }

    #[test]
    fn a_carried_lease_is_ended_and_its_revision_moved_on() {
        use serde_json::json;
        let carried = json!({
            "callback_lease": [
                {"id": "amber-heron", "until": 1_900_000_000.5, "revision": 12, "pull": 3},
                {"id": ["session", "amber-heron"], "until": 5, "revision": 1, "doing": "close"},
                {"id": "no-lease-yet", "asked": 2},
            ],
            "counter": [{"id": ["hold", "sim/jump"], "last": 0, "until": 9, "revision": 4}],
        });
        assert_eq!(
            leases_ended(carried),
            json!({
                "callback_lease": [
                    {"id": "amber-heron", "until": 0, "revision": 13, "pull": 3},
                    {"id": ["session", "amber-heron"], "until": 0, "revision": 2, "doing": "close"},
                    {"id": "no-lease-yet", "asked": 2},
                ],
                "counter": [{"id": ["hold", "sim/jump"], "last": 0, "until": 9, "revision": 4}],
            })
        );
        assert_eq!(leases_ended(json!({})), json!({}));
    }
}

#[cfg(test)]
#[path = "copy_tests.rs"]
mod held;
