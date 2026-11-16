//! The copy of a whole board: a dump (every row, table by table, [`Dump`]) written in one
//! transaction, replacing what the board held, and read back in the same shape.

use super::transaction::Run;
use super::{Error, Store, values};
use crate::cbor;
use crate::rows::Dump;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};

impl Store {
    /// Remove every copy's staging table (`copy_…`): what a copy killed while it staged leaves. Only for
    /// a caller that knows no copy runs meanwhile.
    pub fn remove_staging(&self) -> Result<(), Error> {
        let tables: Vec<String> = self
            .retrying(|| self.transact("RETURN object::keys((INFO FOR DB).tables);", json!({})))?;
        let remove: String = tables
            .iter()
            .filter(|table| {
                table.starts_with("copy_")
                    && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .map(|table| format!("REMOVE TABLE IF EXISTS {table};"))
            .collect();
        self.staging_write(&remove, json!({}))
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
        ];
        let last = dump.messages.iter().map(|m| m.id).max().unwrap_or(0);
        let last_task = dump.tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let last_ask = dump.asks.iter().map(|a| a.id).max().unwrap_or(0);
        let last_ts = dump.messages.iter().map(|m| m.ts).reduce(f64::max);
        let staging = format!(
            "copy_{:016x}",
            RandomState::new().hash_one(std::process::id())
        );
        let copied = self.stage(&staging, &tables).and_then(|()| {
            let mut swap = String::new();
            // Each sender's last word goes with the messages, before them, so their deletion
            // finds no word to look past; the copied messages' events name them again.
            swap.push_str("DELETE said RETURN NONE;\n");
            for (table, _) in &tables {
                let _ = writeln!(
                    swap,
                    "DELETE {table}; INSERT INTO {table} (SELECT VALUE row FROM {staging}_{table}) RETURN NONE; REMOVE TABLE {staging}_{table};"
                );
            }
            // What the deleted search by meaning left describes the replaced board: emptied in the
            // swap transaction, so a refused copy leaves those rows as they were.
            swap.push_str("DELETE task_chunk RETURN NONE; DELETE task_embedding_source RETURN NONE; DELETE task_embedding_checkpoint RETURN NONE;\n");
            swap.push_str("UPSERT counter:message SET last = $last, ts = $last_ts RETURN NONE;\nUPSERT counter:task SET last = $last_task RETURN NONE;\nUPSERT counter:ask SET last = $last_ask RETURN NONE;\n");
            swap.push_str("RETURN NONE;");
            let vars = json!({
                "last": last, "last_ts": last_ts, "last_task": last_task, "last_ask": last_ask,
            });
            self.retrying(|| self.transact::<Value>(&swap, vars.clone())).map(|_| ())
        });
        copied.map_err(|error| {
            let remove: String = tables
                .iter()
                .map(|(table, _)| format!("REMOVE TABLE IF EXISTS {staging}_{table};"))
                .collect();
            self.cleanup_staging(&remove, error)
        })
    }

    /// Every row of `tables` into schemaless tables named `{staging}_{table}`, each request at most
    /// `CHUNK_BYTES`: a board is bigger than the server takes in one. The copy's one transaction then
    /// moves them in. Each request starts with the schema guard in its transaction, so a migration
    /// between requests refuses the next batch before it writes.
    pub(super) fn stage(&self, staging: &str, tables: &[(&str, Vec<Value>)]) -> Result<(), Error> {
        let define: String = tables
            .iter()
            .map(|(table, _)| format!("DEFINE TABLE {staging}_{table} SCHEMALESS;"))
            .collect();
        self.staging_write(&define, json!({}))?;
        for (table, rows) in tables {
            let insert = format!("INSERT INTO {staging}_{table} $rows RETURN NONE;");
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
    fn staging_write(&self, body: &str, vars: Value) -> Result<(), Error> {
        self.retrying(|| {
            self.run_transaction::<Value>(
                &format!("{body}\nRETURN NONE;"),
                vars.clone(),
                Run::Staging,
            )
        })
        .map(|_| ())
    }

    /// Keep the original failure and report cleanup's failure too. A newer writer owns any staging
    /// left after a migration; this store may not remove it.
    pub(super) fn cleanup_staging(&self, remove: &str, mut error: Error) -> Error {
        if let Err(cleanup) = self.staging_write(remove, json!({})) {
            let (Error::Refused(message) | Error::Db(message)) = &mut error;
            let _ = write!(message, "; staging cleanup failed: {cleanup}");
        }
        error
    }

    /// Every row of the board, in the shape the copy reads, each table in the order of its key: what a
    /// copy wrote, read back.
    pub fn dump(&self) -> Result<Dump, Error> {
        self.retrying(|| self.transact(DUMP, json!({})))
    }
}

/// The most a copy sends in one request: the server takes 4 MiB (`SURREAL_HTTP_MAX_RPC_BODY_SIZE`).
pub(super) const CHUNK_BYTES: usize = 1 << 20;

const DUMP: &str = "RETURN {
    tasks: (SELECT * OMIT id, version FROM task ORDER BY sid),
    task_links: (SELECT * OMIT id FROM link ORDER BY slug, kind, target),
    sessions: (SELECT * OMIT id FROM session ORDER BY handle),
    roles: (SELECT * OMIT id FROM role ORDER BY slug, handle, role),
    messages: (SELECT * OMIT id FROM message ORDER BY sid),
    deliveries: (SELECT * OMIT id FROM delivery ORDER BY message, handle),
    barred_keys: (SELECT * OMIT id FROM barred_key ORDER BY key),
    task_sessions: (SELECT * OMIT id FROM task_session ORDER BY slug, handle),
    delivery_keys: (SELECT * OMIT id FROM delivery_key ORDER BY slug, key),
    asks: (SELECT * OMIT id FROM ask ORDER BY sid),
};";
