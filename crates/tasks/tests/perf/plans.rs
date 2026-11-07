//! The plan check. The commands of every per-write, per-recipient and per-tick path run on the
//! generated board through the recording relay ([`crate::proxy`]); each `SELECT` their requests
//! hold, nested ones too, and each `UPDATE` or `DELETE` of a table under a `WHERE` (read as the
//! `SELECT` of the rows it would change: the server plans only a `SELECT`), is planned by the
//! server (`EXPLAIN`) with the request's own variables, and a variable an earlier statement of the
//! request set given a value of its kind ([`bound`]). The check fails on:
//!
//! - a `TableScan` with a predicate over message, delivery, role, session or callback_state: the
//!   whole table read to keep a few rows, which an index serves; and a whole index of one of them
//!   (an `IndexScan` with no `access` range) under a `Filter`, which reads every row as well;
//! - a predicate holding a subquery (EXPLAIN shows it as `(Operator)`): the server runs it again
//!   for every row it tests, as `session WHERE handle IN (SELECT … FROM role)` ran the role scan
//!   once for each of 2.6k sessions.
//!
//! A scan with no predicate reads every row because the caller wants every row (the board's
//! snapshot of its sessions and roles): no index makes that smaller, and it is listed, not failed.

use crate::board::{self, NOW, board};
use crate::proxy::{Proxy, Request};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use symphony_board::rpc::Client;

/// The tables no path reads whole to keep a few rows.
const WATCHED: [&str; 5] = ["message", "delivery", "role", "session", "callback_state"];

/// Names SurrealQL binds itself, and the parameters of a closure, which a statement uses unbound.
const BUILT_IN: [&str; 7] = [
    "parent", "this", "value", "before", "after", "event", "input",
];

/// Each statement `sql` holds that reads rows of a table under a condition, as a `SELECT`: every
/// `SELECT`, nested ones too, and every `UPDATE TABLE … WHERE …` and `DELETE TABLE WHERE …` as
/// the `SELECT id` of the rows it would change. A `SELECT` ends where the bracket it opened in
/// closes, or at a `;` outside any bracket.
pub fn statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut found = Vec::new();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        let starts = |word: &str| {
            sql[i..].starts_with(word)
                && (i == 0 || !is_word(bytes[i - 1]))
                && bytes
                    .get(i + word.len())
                    .is_some_and(u8::is_ascii_whitespace)
        };
        if starts("SELECT") {
            found.push(sql[i..statement_end(sql, i)].trim().to_string());
        } else if (starts("UPDATE") || starts("DELETE"))
            && let Some(read) = as_select(&sql[i..statement_end(sql, i)])
        {
            found.push(read);
        }
        i += 1;
    }
    found
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c == b':'
}

/// Where the statement starting at `from` ends: the bracket closing the one it is in, a `;`
/// outside its own brackets, or the end of `sql`.
fn statement_end(sql: &str, from: usize) -> usize {
    let bytes = sql.as_bytes();
    let (mut depth, mut quote, mut i) = (0i32, None::<u8>, from);
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(_) if c == b'\\' => i += 1,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => return i,
                b')' | b']' | b'}' => depth -= 1,
                b';' if depth == 0 => return i,
                _ => {}
            },
        }
        i += 1;
    }
    bytes.len()
}

/// `UPDATE TABLE SET … WHERE COND` or `DELETE TABLE WHERE COND` (a table by its name, not one
/// record) as `SELECT id FROM TABLE WHERE COND`; none for any other.
fn as_select(statement: &str) -> Option<String> {
    let rest = statement.get(7..)?.trim_start();
    let table: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let after = rest.get(table.len()..)?;
    if table.is_empty() || !after.starts_with(char::is_whitespace) {
        return None;
    }
    let at = top_level(after, " WHERE ")?;
    let condition = &after[at + " WHERE ".len()..];
    let condition = top_level(condition, " RETURN ").map_or(condition, |end| &condition[..end]);
    Some(format!("SELECT id FROM {table} WHERE {}", condition.trim()))
}

/// Where `word` first stands in `text` outside any bracket or quote.
fn top_level(text: &str, word: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut depth, mut quote) = (0i32, None::<u8>);
    for i in 0..bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => continue,
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ if depth == 0 && text[i..].starts_with(word) => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// The variables `statement` reads (`$name`), but a closure's parameters and SurrealQL's own.
fn variables(statement: &str) -> Vec<String> {
    let name_at = |at: usize| -> String {
        statement[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    };
    let mut names: Vec<String> = (statement.match_indices('$'))
        .map(|(at, _)| name_at(at))
        .filter(|name| !name.is_empty())
        .collect();
    // A closure's head: `|$m|` or `|$a, $b|`.
    let params: Vec<String> = statement
        .split('|')
        .skip(1)
        .step_by(2)
        .filter(|head| {
            head.split(',').all(|param| {
                param.trim().strip_prefix('$').is_some_and(|n| {
                    !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            })
        })
        .flat_map(|head| head.split(',').map(|param| param.trim()[1..].to_string()))
        .collect();
    names.sort();
    names.dedup();
    names.retain(|n| !params.contains(n) && !BUILT_IN.contains(&n.as_str()));
    names
}

/// A value of the kind a variable set by an earlier statement of a request holds, by its name, for
/// planning a statement that reads it: the server picks an index by the value it compares, and an
/// unbound name reads as NONE, for which it picks none.
pub fn bound(name: &str) -> Option<Value> {
    let open = json!([board::thread(), board::slug(board::TASKS - 1)]);
    Some(match name {
        "me" => json!({"connected": NOW - 5.0 * 86_400.0, "left_at": null}),
        "connected" => json!(NOW - 5.0 * 86_400.0),
        "slugs" | "open" | "waiting" => open,
        "tasks" => json!([{"slug": board::thread()}]),
        "orchestrators" => json!([[board::thread(), board::orchestrator()]]),
        "sid" | "same" | "last" => json!(1),
        "ts" => json!(NOW),
        "counted" => json!({"last": 1, "ts": NOW}),
        "agent" | "binding" | "old" | "other" | "had" | "made" | "was" => Value::Null,
        "s" | "state" | "c" | "lease" | "recipient" | "channel" | "before" | "pull" => json!({}),
        "candidates" | "acked" | "wanted" | "pool" | "keys" | "waiting_keys" | "said" => json!([]),
        "line" => json!({"slug": board::thread(), "sid": 1}),
        "address" => json!("uds:/tmp/perf.sock"),
        "dependent" => json!(board::thread()),
        "prefix" | "pull_kept" => json!("pull-1"),
        _ => return None,
    })
}

/// A finding of the check, or a plan it lists.
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// A whole table read to keep a few rows.
    FilteredScan(String),
    /// A subquery the server runs again for every row a predicate tests.
    SubqueryPerRow(String),
    /// A statement the server would not plan, or one reading a variable [`bound`] has no value for.
    NotPlanned,
}

/// What is wrong with `plan`, an `EXPLAIN`'s text. A scan of a whole index (no `access` range)
/// under a `Filter` is a filtered scan too: the server takes an index for its order alone (`ORDER
/// BY sid` through `message_sid`) when the one its condition needs is gone, and reads every row.
pub fn findings(plan: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut parent = "";
    for line in plan.lines() {
        let attribute = |name: &str| {
            line.split_once(&format!("{name}: ")).map(|(_, rest)| {
                rest.split([',', ']'])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
        };
        let node = line.trim_start();
        if node.starts_with("TableScan")
            && let Some(table) = attribute("table")
            && WATCHED.contains(&table.as_str())
            && line.contains("predicate: ")
        {
            found.push(Finding::FilteredScan(table));
        }
        if node.starts_with("IndexScan")
            && parent.starts_with("Filter")
            && attribute("access").is_some_and(|range| range.is_empty())
            && let Some(index) = attribute("index")
            && let Some(table) = WATCHED.iter().find(|t| index.starts_with(&format!("{t}_")))
        {
            found.push(Finding::FilteredScan(table.to_string()));
        }
        if !node.is_empty() {
            parent = node;
        }
        if let Some((_, predicate)) = line.split_once("predicate: ")
            && let Some(operator) = subplan(predicate)
        {
            found.push(Finding::SubqueryPerRow(operator));
        }
    }
    found
}

/// The first `(Operator)` a predicate holds: how EXPLAIN shows a subquery in it.
fn subplan(predicate: &str) -> Option<String> {
    let mut rest = predicate;
    while let Some(open) = rest.find('(') {
        let inner = &rest[open + 1..];
        let name: String = inner
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        if name.starts_with(|c: char| c.is_ascii_uppercase())
            && inner[name.len()..].starts_with(')')
        {
            return Some(name);
        }
        rest = inner;
    }
    None
}

/// Whether `plan` reads a watched table whole, with no predicate: listed, not failed.
fn whole_read(plan: &str) -> Option<String> {
    plan.lines()
        .filter(|line| line.trim_start().starts_with("TableScan") && !line.contains("predicate: "))
        .find_map(|line| {
            let table = line
                .split_once("table: ")?
                .1
                .split([',', ']'])
                .next()?
                .trim();
            WATCHED.contains(&table).then(|| table.to_string())
        })
}

/// One statement as planned: how often the requests sent it, its plan and what is wrong with it.
struct Planned {
    sent: usize,
    plan: String,
    findings: Vec<Finding>,
}

/// Each distinct statement of `requests`, planned on the board `client` reaches.
fn plan_all(client: &Client, requests: &[Request]) -> BTreeMap<String, Planned> {
    let mut planned: BTreeMap<String, Planned> = BTreeMap::new();
    for request in requests {
        for statement in statements(&request.sql) {
            if let Some(seen) = planned.get_mut(&statement) {
                seen.sent += 1;
                continue;
            }
            let mut vars: Map<String, Value> =
                request.vars.as_object().cloned().unwrap_or_default();
            let mut unbound = Vec::new();
            for name in variables(&statement) {
                if vars.contains_key(&name) {
                    continue;
                }
                match bound(&name) {
                    Some(value) => {
                        vars.insert(name, value);
                    }
                    None => unbound.push(name),
                }
            }
            let (plan, findings) = if unbound.is_empty() {
                match client.query(&format!("EXPLAIN {statement};"), Value::Object(vars)) {
                    Ok(mut said) => {
                        let plan = said
                            .pop()
                            .and_then(|p| p.as_str().map(String::from))
                            .unwrap_or_default();
                        let found = findings(&plan);
                        (plan, found)
                    }
                    Err(why) => (format!("not planned: {why:?}"), vec![Finding::NotPlanned]),
                }
            } else {
                (
                    format!(
                        "not planned: give ${} a value in plans::bound",
                        unbound.join(", $")
                    ),
                    vec![Finding::NotPlanned],
                )
            };
            planned.insert(
                statement,
                Planned {
                    sent: 1,
                    plan,
                    findings,
                },
            );
        }
    }
    planned
}

#[test]
fn every_query_a_write_a_recipient_or_a_tick_sends_is_planned_on_an_index() {
    let mut board = board();
    let relay = Proxy::to(&board.addr);
    let (thread, writer, recipient, orchestrator) = (
        board::thread(),
        board::writer(),
        board::recipient(),
        board::orchestrator(),
    );
    let claimed = board.unclaimed();
    let api = |method: &str, args: Value| json!({"method": method, "args": args}).to_string();
    let commands: Vec<(Vec<&str>, String)> = vec![
        (
            vec![
                "say",
                &thread,
                "a post to its recipient",
                "--to",
                &recipient,
                "--as",
                &writer,
            ],
            String::new(),
        ),
        (
            vec![
                "say",
                &thread,
                "a post to the orchestrator",
                "--to",
                "@orchestrator",
                "--as",
                &writer,
            ],
            String::new(),
        ),
        (vec!["claim", &claimed, "--as", &writer], String::new()),
        (vec!["inbox", "--as", &orchestrator], String::new()),
        (vec!["inbox", "--as", &recipient], String::new()),
        (vec!["dispatch"], String::new()),
    ];
    let calls = [
        api("get", json!({"slug": thread})),
        api("tasks", json!({"status": "review"})),
        api("ever_executed", json!({"slug": thread})),
        api("sessions", json!({})),
        api("session", json!({"handle": writer})),
        api("ancestors", json!({"slug": thread})),
        api("thread", json!({"slug": thread})),
        api(
            "say",
            json!({"slug": thread, "sender": writer, "text": "a keyed notice", "to": "@orchestrator", "key": "perf"}),
        ),
        api("landings_posted", json!({})),
        api("last_messages", json!({})),
        api("to_owner", json!({"limit": 40})),
        api("inbox", json!({"handle": orchestrator})),
    ];
    for (args, input) in &commands {
        let ran = board.run(&relay.addr, args, input);
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
    }
    for call in &calls {
        let ran = board.run(&relay.addr, &["api"], call);
        assert_eq!(ran.code, 0, "{call}: {}", ran.err);
        assert!(ran.out.contains("\"ok\""), "{call}: {}", ran.out);
    }
    let mut requests = board.first_looks.clone();
    requests.extend(relay.take());
    let planned = plan_all(&board.client(), &requests);
    let mut failed = Vec::new();
    for (statement, p) in &planned {
        let whole = whole_read(&p.plan).map_or(String::new(), |t| format!(" (reads {t} whole)"));
        eprintln!(
            "--- sent {}×{whole}: {statement}\n{}",
            p.sent,
            p.plan.trim_end()
        );
        if !p.findings.is_empty() {
            failed.push(format!(
                "{:?} in {statement}\n{}",
                p.findings,
                p.plan.trim_end()
            ));
        }
    }
    eprintln!(
        "perf: {} requests, {} distinct statements planned, {} with a finding",
        requests.len(),
        planned.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

mod reading {
    use super::*;

    #[test]
    fn every_select_is_found_nested_ones_too_and_ends_where_its_bracket_closes() {
        let sql = "LET $me = (SELECT connected FROM ONLY type::record('session', $handle));
RETURN { a: (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'), b: IF $x { (SELECT * FROM task) } ELSE { [] } };
RETURN SELECT * OMIT id FROM message WHERE recipient = 'owner; SELECT not' ORDER BY sid DESC LIMIT $limit;";
        assert_eq!(
            statements(sql),
            [
                "SELECT connected FROM ONLY type::record('session', $handle)",
                "SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'",
                "SELECT * FROM task",
                "SELECT * OMIT id FROM message WHERE recipient = 'owner; SELECT not' ORDER BY sid DESC LIMIT $limit",
            ]
        );
    }

    #[test]
    fn a_subquery_in_a_select_is_its_own_statement_as_well() {
        let sql = "SELECT * FROM session WHERE handle IN (SELECT VALUE handle FROM role WHERE role = 'orchestrator');";
        assert_eq!(
            statements(sql),
            [
                "SELECT * FROM session WHERE handle IN (SELECT VALUE handle FROM role WHERE role = 'orchestrator')",
                "SELECT VALUE handle FROM role WHERE role = 'orchestrator'",
            ]
        );
    }

    #[test]
    fn an_update_or_delete_of_a_table_under_a_condition_reads_as_the_select_of_its_rows() {
        let sql = "UPDATE callback_event SET attempts = 0 WHERE handle = $actor AND delivered != true RETURN NONE;
DELETE callback_event WHERE handle = $handle AND string::starts_with(key, 'pull-') RETURN NONE;
UPDATE type::record('callback_lease', $handle) SET until = 0 WHERE revision = $revision RETURN NONE;
UPDATE counter:board SET last = 1;
DELETE task_chunk RETURN NONE;";
        assert_eq!(
            statements(sql),
            [
                "SELECT id FROM callback_event WHERE handle = $actor AND delivered != true",
                "SELECT id FROM callback_event WHERE handle = $handle AND string::starts_with(key, 'pull-')",
            ]
        );
    }

    #[test]
    fn a_closure_parameter_and_surreals_own_names_are_not_variables_to_bind() {
        assert_eq!(
            variables(
                "SELECT * FROM $pool.filter(|$m| $m.sid > $since).map(|$a, $b| $a) WHERE x = $parent.handle"
            ),
            ["pool", "since"]
        );
    }

    /// The plans the server gave on the board of 2026-11-07, before and after its indexes.
    #[test]
    fn a_filtered_scan_of_a_watched_table_and_a_subquery_in_a_predicate_are_findings() {
        let per_row = "SelectProject [ctx: Db] [projections: *]
    TableScan [ctx: Db] [table: session, direction: Forward, predicate: handle INSIDE (ProjectValue), pre_decode_filter: no (unsupported predicate)]";
        assert_eq!(
            findings(per_row),
            [
                Finding::FilteredScan("session".into()),
                Finding::SubqueryPerRow("ProjectValue".into())
            ]
        );
        let scanned = "ProjectValue [ctx: Db] [expr: [<string>  message, NULL]]
    TableScan [ctx: Db] [table: delivery, direction: Forward, predicate: handle = 'x' AND message > 49000, pre_decode_filter: yes]";
        assert_eq!(
            findings(scanned),
            [Finding::FilteredScan("delivery".into())]
        );
        let indexed = "ProjectValue [ctx: Db] [expr: [<string>  message, NULL]]
    IndexScan [ctx: Db] [index: delivery_handle, access: ['x'] MoreThan 49000, direction: Forward]";
        assert!(findings(indexed).is_empty());
        let filtered = "SelectProject [ctx: Db] [projections: key]
    Filter [ctx: Db] [predicate: delivered != true AND attempts < 3]
        IndexScan [ctx: Db] [index: callback_event_handle, access: ['x'], direction: Forward]";
        assert!(findings(filtered).is_empty());
        let whole = "SelectProject [ctx: Db] [projections: *, OMIT id]
    SortByKey [ctx: Db] [sort_keys: connected ASC, handle ASC]
        TableScan [ctx: Db] [table: session, direction: Forward]";
        assert!(findings(whole).is_empty());
        assert_eq!(whole_read(whole).as_deref(), Some("session"));
        let unwatched = "SelectProject [ctx: Db] [projections: *]
    TableScan [ctx: Db] [table: task, direction: Forward, predicate: status INSIDE ['ready'], pre_decode_filter: yes]";
        assert!(findings(unwatched).is_empty());
    }

    /// `to_owner` as a clone of the live board planned it with `message_recipient`, and with that
    /// index removed: the server walked all of `message_sid` for the order alone.
    #[test]
    fn a_whole_index_read_under_a_filter_is_a_finding_and_a_ranged_one_is_not() {
        let ranged = "SelectProject [ctx: Db] [projections: *, OMIT id]
    IndexScan [ctx: Db] [index: message_recipient, access: ['owner'], direction: Backward, limit: 40]";
        assert!(findings(ranged).is_empty());
        let dropped = "SelectProject [ctx: Db] [projections: *, OMIT id]
    Limit [ctx: Db] [limit: 40]
        Filter [ctx: Db] [predicate: recipient = 'owner']
            IndexScan [ctx: Db] [index: message_sid, access: , direction: Backward]";
        assert_eq!(findings(dropped), [Finding::FilteredScan("message".into())]);
        let bounded = "SelectProject [ctx: Db] [projections: handle]
    Filter [ctx: Db] [predicate: type::record('session', handle).left_at = NONE]
        IndexScan [ctx: Db] [index: callback_state_address, access: >NONE, direction: Forward]";
        assert!(findings(bounded).is_empty());
    }
}
