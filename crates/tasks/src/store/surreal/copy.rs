//! The statements of `store/copy.rs`.

use std::fmt::Write as _;

/// Every row of the board, in the shape the copy reads, each table in the order of its key; the
/// execution tables' in any, which `Store::dump` puts in theirs.
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
    usages: (SELECT * OMIT id FROM usage ORDER BY handle, slug, role, model, speed, round),
    execution_attempts: (SELECT * OMIT id FROM execution_attempt),
    execution_facts: (SELECT * OMIT id FROM execution_fact),
    execution_fences: (SELECT * OMIT id FROM execution_fence),
    execution_artifacts: (SELECT * OMIT id FROM execution_artifact),
    workflow: (SELECT record::id(id) AS key, kind, body, at FROM workflow),
};";

/// What the copy in one transaction says in SurrealQL: nothing, as a board on SurrealDB is copied
/// through its staging tables ([`swap`]).
pub(in crate::store) const NOTHING_COPIED: &str = "RETURN NONE;";

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

/// `$rows` into `table`'s staging table, defined again first if another copy removed it meanwhile:
/// the board is STRICT, so an undefined table takes no row, and the swap's count of what each
/// staging table holds is what refuses a copy whose staging was taken ([`swap`]).
pub(in crate::store) fn insert_staged(staging: &str, table: &str) -> String {
    staging_request(&format!(
        "DEFINE TABLE IF NOT EXISTS {staging}_{table} SCHEMALESS;\n\
         INSERT INTO {staging}_{table} $rows RETURN NONE;"
    ))
}

/// The names of `tables`' staging tables.
pub(in crate::store) fn staged(staging: &str, tables: &[&str]) -> Vec<String> {
    (tables.iter())
        .map(|table| format!("{staging}_{table}"))
        .collect()
}

/// The copy's one transaction: each of `tables`' staging tables found holding the rows the copy
/// sent it (`$sent`, a count a table), else refused before anything is written, as a staging
/// request defines its table again for rows sent after another copy removed it
/// ([`insert_staged`]) and the swap would otherwise put in a part of the board; then each table emptied and filled from its staging
/// table, which is removed, the caches of the board it replaces dropped, the counters set to `$last` and
/// `$last_ts` (messages), `$last_task` and `$last_ask`, and the board's recovery generation
/// replaced ([`super::recovery::REBIND`]), which is its answer: the numbers and versions the copy
/// gives name other tasks than the ones a token was read on, in the same transaction or not at
/// all. The database `database` is made STRICT again in it, as a board put back by other means
/// than a move may have lost that ([`super::opening::strict`]).
pub(in crate::store) fn swap(staging: &str, tables: &[&str], database: &str) -> String {
    let mut swap = super::opening::strict(database);
    swap.push('\n');
    // Each sender's last word goes with the messages, before them, so their deletion
    // finds no word to look past; the copied messages' events name them again.
    swap.push_str("LET $there = object::keys((INFO FOR DB).tables);\n");
    for table in tables {
        let _ = writeln!(
            swap,
            "LET $held = IF '{staging}_{table}' IN $there {{ (SELECT count() FROM {staging}_{table} GROUP ALL)[0].count ?? 0 }} ELSE {{ 'no table' }};
IF $held != $sent.{table} {{ THROW string::concat(\"refused: the copy's staging was taken while it staged, as a copy of the board started meanwhile takes it, and the board is as it was: its {table} holds \", <string>$held, ' of the ', <string>$sent.{table}, ' rows it sent'); }};"
        );
    }
    // A workflow's records go whether or not the dump has any: a dump with none lists no table
    // for them, so the copy says what it said before workflows, and leaves no record behind.
    swap.push_str("DELETE said RETURN NONE;\nDELETE workflow RETURN NONE;\n");
    for table in tables {
        let _ = writeln!(
            swap,
            "DELETE {table}; INSERT INTO {table} (SELECT VALUE row FROM {staging}_{table}) RETURN NONE; REMOVE TABLE {staging}_{table};"
        );
    }
    // What the deleted search by meaning left describes the replaced board: emptied in the
    // swap transaction, so a refused copy leaves those rows as they were.
    swap.push_str("DELETE task_chunk RETURN NONE; DELETE task_embedding_source RETURN NONE; DELETE task_embedding_checkpoint RETURN NONE;\n");
    swap.push_str("UPSERT counter:message SET last = $last, ts = $last_ts RETURN NONE;\nUPSERT counter:task SET last = $last_task RETURN NONE;\nUPSERT counter:ask SET last = $last_ask RETURN NONE;\n");
    swap.push_str(super::recovery::REBIND);
    swap
}
