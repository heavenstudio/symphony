//! The copy of a whole board on PostgreSQL: the dump's rows into the emptied board in one
//! transaction (`store/copy.rs`). Its staging, for a dump bigger than one request takes, and the
//! dump read back are the copy's own slice's.

use super::Pg;
use std::fmt::Write as _;

/// The copy's one transaction: each of `tables` emptied and filled from `v -> table`, its rows as
/// the dump gives them with their ids, each sender's last word made again by the messages'
/// trigger, and the counters set to `last` and `last_ts` (messages), `last_task` and `last_ask`.
pub(in crate::store) fn copy(tables: &[&str]) -> Pg<'static> {
    let mut block = String::from("DELETE FROM said;\n");
    for table in tables {
        let _ = writeln!(
            block,
            "DELETE FROM {table};
INSERT INTO {table} SELECT r.* FROM jsonb_populate_recordset(NULL::{table}, v -> '{table}') AS r;"
        );
    }
    block.push_str(
        "INSERT INTO counter (id, last, ts) VALUES ('\"message\"', (v ->> 'last')::bigint, (v ->> 'last_ts')::double precision)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last, ts = EXCLUDED.ts;
INSERT INTO counter (id, last) VALUES ('\"task\"', (v ->> 'last_task')::bigint)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last;
INSERT INTO counter (id, last) VALUES ('\"ask\"', (v ->> 'last_ask')::bigint)
    ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last;",
    );
    Pg::script(block)
}
