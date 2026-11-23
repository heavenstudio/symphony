//! The reads a write, a recipient or a tick sends that PostgreSQL planned with a sequential scan of
//! session, message or callback_state until each was given an index to read by: a session by its
//! handle (`session_handle`, the second additive step on schema 28), the landings by the word in
//! `words`, the channels by `callback_state_address` and the session addresses by their ids' range;
//! and those that read the rows of a list (`jsonb_array_elements`), which the server guesses holds
//! a hundred and so hashes the whole table for, by all of their keys in one condition (`= ANY`).
//!
//! Each is planned as the store sends it ([`request`]) with sequential scans off, so the server
//! takes an index wherever one serves the condition, whatever the board's size: a scan left is one
//! no index serves, as the board crate's `perf` tests read a plan.

use super::{Pg, Request, callbacks, messages, reading, request, session_urls, work, write};
use crate::Database;
use crate::pg::Step;
use crate::store::messages::DELIVERED;
use crate::store::reading::native_bounds;
use crate::store_fixture::{Backend, Fixture, Seed};
use serde_json::{Value, json};

/// The tables no read of a write, a recipient or a tick scans to keep a few rows.
const WATCHED: [&str; 3] = ["session", "message", "callback_state"];

/// Each node of `node`'s plan that reads a watched table by no index to keep the rows a condition
/// picks: a sequential scan, or an index walked from end to end (no `Index Cond`), with a `Filter`,
/// as its node type and table. A read of every row, with no condition, is what its caller asks
/// for, and no index makes it smaller.
fn unindexed(node: &Value) -> Vec<String> {
    let kind = node["Node Type"].as_str().unwrap_or_default();
    let table = node["Relation Name"].as_str().unwrap_or_default();
    let walked = match kind {
        "Seq Scan" => true,
        "Index Scan" | "Index Only Scan" => node.get("Index Cond").is_none(),
        _ => false,
    };
    let mut found = Vec::new();
    if walked && WATCHED.contains(&table) && node.get("Filter").is_some() {
        found.push(format!("{kind} on {table}"));
    }
    for child in node["Plans"].as_array().into_iter().flatten() {
        found.extend(unindexed(child));
    }
    found
}

/// Whether `node`'s plan reads `table` by an index condition of `= ANY`: every key of a list in one
/// condition, on an index scan of it or, of a bitmap's, as the condition its heap scan rechecks.
fn by_any(node: &Value, table: &str) -> bool {
    let any = |field: &str| node[field].as_str().is_some_and(|c| c.contains("= ANY "));
    let here =
        node["Relation Name"].as_str() == Some(table) && (any("Index Cond") || any("Recheck Cond"));
    let under = node["Plans"].as_array().into_iter().flatten();
    here || under.into_iter().any(|child| by_any(child, table))
}

/// The plan PostgreSQL gives the read `statement` with `vars`, as the store sends it, on `f`'s
/// board, with sequential scans off for its transaction alone.
fn planned(f: &Fixture, statement: &Pg, vars: &Value) -> Value {
    let Some(Request::Steps { steps, .. }) = request(statement, false) else {
        panic!("a read is steps: {statement:?}");
    };
    let [(sql, true)] = steps.as_slice() else {
        panic!("a read is one statement binding its variables: {steps:?}");
    };
    let Database::PostgreSql(client) = f.other_database() else {
        panic!("this board is on PostgreSQL");
    };
    let explain = format!("EXPLAIN (FORMAT JSON) {sql}");
    let vars = vars.to_string();
    let answers = client
        .request(&[
            Step::bare("SELECT set_config('enable_seqscan', 'off', true)"),
            Step::with(&explain, &vars),
        ])
        .unwrap();
    let plan: Value = serde_json::from_str(answers[1].value().unwrap()).unwrap();
    plan[0]["Plan"].clone()
}

/// A board on PostgreSQL with a row for each read: a channel, a session's address and a landing.
fn board() -> Fixture {
    let seed = Seed::default()
        .session("amber-heron")
        .session("swift-hare")
        .task("sim/jump", "in_progress");
    let f = Fixture::on(Backend::PostgreSql, seed);
    f.store
        .callback("amber-heron", Some("codex:0a1b2c3d"), false, false)
        .unwrap();
    f.store
        .set_session_url("swift-hare", "https://claude.ai/code/session_01abc")
        .unwrap();
    f.store
        .say(
            "sim/jump",
            "amber-heron",
            "landed sim/jump on main at 4300a80",
            None,
        )
        .unwrap();
    f
}

#[test]
fn a_session_the_landings_the_channels_and_the_session_addresses_are_each_read_by_an_index() {
    let f = board();
    let mut inbox =
        json!({"handle": "swift-hare", "delivered": DELIVERED, "since": null, "pool": null});
    native_bounds(&mut inbox);
    let reads = [
        ("the inbox, its reader's session", messages::inbox(), inbox),
        (
            "the landings posted",
            messages::landings_posted(),
            json!({"delivered": DELIVERED}),
        ),
        ("the registrations", callbacks::registrations(), json!({})),
        ("the session addresses", session_urls::read(), json!({})),
        (
            "whether a session keeps a channel",
            Pg::read(reading::HAS_CALLBACKS),
            json!({}),
        ),
    ];
    let wrong: Vec<String> = (reads.iter())
        .filter_map(|(what, statement, vars)| {
            let plan = planned(&f, statement, vars);
            let found = unindexed(&plan);
            (!found.is_empty()).then(|| format!("{what}: {found:?} in {plan:#}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

#[test]
fn the_rows_a_list_names_are_read_by_all_of_its_keys_in_one_index_condition() {
    // A join over a list's elements is planned by the server's guess of a hundred of them: by a
    // probe of the index for each on a board this small, by hashing the whole table on one at the
    // live board's scale. `= ANY` of the list is one condition of the index at any size.
    let f = board();
    let reads = [
        (
            "the candidates that listen",
            work::LISTENING,
            json!({"candidates": ["amber-heron", "swift-hare"]}),
            "callback_state",
        ),
        // Two conditions of writes' blocks, read as the blocks read them, over the same `v`.
        (
            "a write's incoming holders gone",
            Pg::read(format!("({})", write::ADDED_GONE)),
            json!({"add": [{"handle": "amber-heron", "role": "executor"}]}),
            "session",
        ),
        (
            "a pass's receivers gone",
            Pg::read(format!("({})", work::RECEIVER_GONE)),
            json!({"receivers": ["amber-heron", "swift-hare"]}),
            "session",
        ),
    ];
    let wrong: Vec<String> = (reads.iter())
        .filter_map(|(what, statement, vars, table)| {
            let plan = planned(&f, statement, vars);
            let found = unindexed(&plan);
            let keyed = by_any(&plan, table);
            (!found.is_empty() || !keyed)
                .then(|| format!("{what}: {found:?}, {table} by = ANY: {keyed}, in {plan:#}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

#[test]
fn a_read_of_a_watched_table_by_no_index_is_found_however_deep_it_is_planned() {
    let f = board();
    // `kind` has no index on session, and a whole read of message under a subquery's condition.
    let scan = Pg::read(
        "(SELECT count(*) FROM session WHERE kind = v ->> 'kind' \
         AND EXISTS (SELECT 1 FROM message m WHERE strpos(m.text, v ->> 'kind') > 0))",
    );
    let mut found = unindexed(&planned(&f, &scan, &json!({"kind": "worker"})));
    found.sort();
    assert_eq!(found, ["Seq Scan on message", "Seq Scan on session"]);
    // An index walked whole for its order, under a condition it does not serve.
    let walk = Pg::read(
        "(SELECT jsonb_agg(sid) FROM (SELECT sid FROM message \
         WHERE strpos(text, v ->> 'kind') > 0 ORDER BY sid LIMIT 1) m)",
    );
    let found = unindexed(&planned(&f, &walk, &json!({"kind": "landed"})));
    assert_eq!(found, ["Index Scan on message"]);
    // A read of every row asks for them all, and a table no read is held to (task) is not watched.
    let whole = Pg::read("(SELECT jsonb_agg(handle) FROM session)");
    assert!(unindexed(&planned(&f, &whole, &json!({}))).is_empty());
    let task = Pg::read("(SELECT count(*) FROM task WHERE title = v ->> 'kind')");
    assert!(unindexed(&planned(&f, &task, &json!({"kind": "x"}))).is_empty());
}
