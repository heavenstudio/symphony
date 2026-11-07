//! The plan check. The commands of every per-write, per-recipient and per-tick path run on the
//! generated board through the recording relay ([`crate::proxy`]); each statement their requests
//! hold that reads rows under a condition ([`crate::sql`]: every `SELECT`, nested ones too, and
//! each `UPDATE` or `DELETE` of a table under a `WHERE` read as the `SELECT` of the rows it would
//! change, the server planning only a `SELECT`) is planned by the server (`EXPLAIN`) with the
//! request's own variables, and a variable an earlier statement of the request set given a value
//! of its kind ([`bound`]). The check fails on what [`crate::findings`] finds in a plan: a filtered
//! scan of message, delivery, role, session or callback_state, or a subquery in a predicate.

use crate::board::board;
use crate::findings::{Finding, findings, whole_read};
use crate::generated::{self, NOW};
use crate::proxy::{Proxy, Request};
use crate::sql::{statements, variables};
use crate::store;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use symphony_board::rpc::Client;

/// A value of the kind a variable set by an earlier statement of a request holds, by its name, for
/// planning a statement that reads it: the server picks an index by the value it compares, and an
/// unbound name reads as NONE, for which it picks none.
pub fn bound(name: &str) -> Option<Value> {
    let open = json!([generated::thread(), generated::slug(generated::TASKS - 1)]);
    Some(match name {
        "me" => json!({"connected": NOW - 5.0 * 86_400.0, "left_at": null}),
        "connected" => json!(NOW - 5.0 * 86_400.0),
        "slugs" | "open" | "waiting" => open,
        "tasks" => json!([{"slug": generated::thread()}]),
        "orchestrators" => json!([[generated::thread(), generated::orchestrator()]]),
        "sid" | "same" | "last" => json!(1),
        "ts" => json!(NOW),
        "counted" => json!({"last": 1, "ts": NOW}),
        "agent" | "binding" | "old" | "other" | "had" | "made" | "was" => Value::Null,
        "s" | "state" | "c" | "lease" | "recipient" | "channel" | "before" | "pull" => json!({}),
        "candidates" | "acked" | "wanted" | "pool" | "keys" | "waiting_keys" | "said" => json!([]),
        "line" => json!({"slug": generated::thread(), "sid": 1}),
        "address" => json!("uds:/tmp/perf.sock"),
        "dependent" => json!(generated::thread()),
        "prefix" | "pull_kept" => json!("pull-1"),
        _ => return None,
    })
}

/// One statement as planned: how often the requests sent it, its plan and what is wrong with it.
#[derive(Debug)]
struct Planned {
    sent: usize,
    plan: String,
    findings: Vec<Finding>,
}

/// `statement`, sent with `sent_with`, planned on the board `client` reaches.
fn plan_one(client: &Client, statement: &str, sent_with: &Value) -> Planned {
    let mut vars: Map<String, Value> = sent_with.as_object().cloned().unwrap_or_default();
    let mut unbound = Vec::new();
    let read = variables(statement);
    for name in read {
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
    let (plan, found) = if !unbound.is_empty() {
        (
            format!(
                "not planned: give ${} a value in plans::bound",
                unbound.join(", $")
            ),
            vec![Finding::NotPlanned],
        )
    } else {
        match client.query(&format!("EXPLAIN {statement};"), Value::Object(vars)) {
            Ok(mut answers) => {
                let plan = (answers.pop())
                    .and_then(|answer| answer.as_str().map(String::from))
                    .unwrap_or_default();
                let found = findings(&plan);
                (plan, found)
            }
            Err(why) => (format!("not planned: {why:?}"), vec![Finding::NotPlanned]),
        }
    };
    Planned {
        sent: 1,
        plan,
        findings: found,
    }
}

/// Each distinct statement of `requests`, planned on the board `client` reaches.
fn plan_all(client: &Client, requests: &[Request]) -> BTreeMap<String, Planned> {
    let mut planned: BTreeMap<String, Planned> = BTreeMap::new();
    for request in requests {
        let held = statements(&request.sql);
        for statement in held {
            if let Some(seen) = planned.get_mut(&statement) {
                seen.sent += 1;
                continue;
            }
            let one = plan_one(client, &statement, &request.vars);
            planned.insert(statement, one);
        }
    }
    planned
}

#[test]
fn every_query_a_write_a_recipient_or_a_tick_sends_is_planned_on_an_index() {
    let mut board = board();
    let relay = Proxy::to(&board.addr);
    let thread = generated::thread();
    let writer = generated::writer();
    let recipient = generated::recipient();
    let orchestrator = generated::orchestrator();
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
    for (statement, one) in &planned {
        let whole = whole_read(&one.plan).map_or(String::new(), |t| format!(" (reads {t} whole)"));
        eprintln!(
            "--- sent {}×{whole}: {statement}\n{}",
            one.sent,
            one.plan.trim_end()
        );
        if !one.findings.is_empty() {
            failed.push(format!(
                "{:?} in {statement}\n{}",
                one.findings,
                one.plan.trim_end()
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

/// A request spelled as the server also takes it, a lower-case `select`, or a `SELECT` after a
/// comment holding a letter outside ASCII, is planned as any other, and its read of session under
/// a condition, which no index serves, found: planned on a server of its own, not the generated
/// board, which the other tests hold.
#[test]
fn a_select_in_any_case_or_after_any_comment_is_planned_and_its_scan_found() {
    let fixture = store::Fixture::empty();
    fixture.session("reviewer");
    let spellings = [
        "select * from session where kind = 'reviewer'",
        "// unicode comment: café\nSELECT * FROM session WHERE kind = 'reviewer'",
    ];
    for sql in spellings {
        let request = Request {
            sql: sql.to_string(),
            vars: json!({}),
        };
        let planned = plan_all(fixture.store.client(), &[request]);
        let found: Vec<&Finding> = (planned.values()).flat_map(|one| &one.findings).collect();
        assert_eq!(
            found,
            [&Finding::FilteredScan("session".into())],
            "{sql}: {planned:#?}"
        );
    }
}
