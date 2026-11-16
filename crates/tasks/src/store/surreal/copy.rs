//! The statements of `store/copy.rs`.

use std::fmt::Write as _;

/// Every row of the board, in the shape the copy reads, each table in the order of its key.
pub(in crate::store) const DUMP: &str = "RETURN {
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

/// The names of the database's tables.
pub(in crate::store) const TABLES: &str = "RETURN object::keys((INFO FOR DB).tables);";

/// `body`, a staging request's statements, as the whole request: staging answers nothing.
pub(super) fn staging_request(body: &str) -> String {
    format!("{body}\nRETURN NONE;")
}

/// `tables` removed, each if it is there: what a copy killed while it staged leaves.
pub(in crate::store) fn remove_tables<'a>(tables: impl Iterator<Item = &'a str>) -> String {
    let remove: String = tables
        .map(|table| format!("REMOVE TABLE IF EXISTS {table};"))
        .collect();
    staging_request(&remove)
}

/// A schemaless staging table `{staging}_{table}` for each of `tables`.
pub(in crate::store) fn define_staged(staging: &str, tables: &[&str]) -> String {
    let define: String = tables
        .iter()
        .map(|table| format!("DEFINE TABLE {staging}_{table} SCHEMALESS;"))
        .collect();
    staging_request(&define)
}

/// `$rows` into `table`'s staging table.
pub(in crate::store) fn insert_staged(staging: &str, table: &str) -> String {
    staging_request(&format!("INSERT INTO {staging}_{table} $rows RETURN NONE;"))
}

/// Every staging table of `tables` removed, each if it is there.
pub(in crate::store) fn remove_staged(staging: &str, tables: &[&str]) -> String {
    let remove: String = tables
        .iter()
        .map(|table| format!("REMOVE TABLE IF EXISTS {staging}_{table};"))
        .collect();
    staging_request(&remove)
}

/// The copy's one transaction: each of `tables` emptied and filled from its staging table, which
/// is removed, the caches of the board it replaces dropped, and the counters set to `$last` and
/// `$last_ts` (messages), `$last_task` and `$last_ask`.
pub(in crate::store) fn swap(staging: &str, tables: &[&str]) -> String {
    let mut swap = String::new();
    // Each sender's last word goes with the messages, before them, so their deletion
    // finds no word to look past; the copied messages' events name them again.
    swap.push_str("DELETE said RETURN NONE;\n");
    for table in tables {
        let _ = writeln!(
            swap,
            "DELETE {table}; INSERT INTO {table} (SELECT VALUE row FROM {staging}_{table}) RETURN NONE; REMOVE TABLE {staging}_{table};"
        );
    }
    // These caches describe the replaced board and are rebuilt by catch-up. Keep the
    // reset in the swap transaction so a refused copy preserves the current index.
    swap.push_str("DELETE task_chunk RETURN NONE; DELETE task_embedding_source RETURN NONE; DELETE task_embedding_checkpoint RETURN NONE;\n");
    swap.push_str("UPSERT counter:message SET last = $last, ts = $last_ts RETURN NONE;\nUPSERT counter:task SET last = $last_task RETURN NONE;\nUPSERT counter:ask SET last = $last_ask RETURN NONE;\n");
    swap.push_str("RETURN NONE;");
    swap
}
