//! What a plan PostgreSQL gives says is wrong with a statement. The plan check has the server send
//! the plan of every statement its commands run, each a block or a trigger runs too, to the
//! session that ran it as a notice (`auto_explain` with nested statements, its format JSON:
//! `Board::explained`), which the relay keeps (`crate::proxy`); [`plans_of`] reads the plans out of
//! the notices, and [`findings`] reads what is wrong out of each:
//!
//! - a scan of message, delivery, role, session or callback_state ([`WATCHED`]) under a condition
//!   no index serves: a `Seq Scan` with a `Filter`, the whole table read to keep a few rows; or an
//!   index read from end to end (an `Index Scan` or `Index Only Scan` with no `Index Cond`) under
//!   one, which reads every row as well, in the index's order: a walk of `message_sid` from the
//!   newest message down for one sender's word is the 2026-11-07 look back;
//! - a subquery the server runs again for every row it is asked of (a `SubPlan` it does not hash)
//!   that reads a table by no index, as `session WHERE handle IN (SELECT … FROM role)` read the
//!   role table again for each of 2.6k sessions. One that probes an index for each row is a join's
//!   inner side, and no finding; one the server hashes (`hashed SubPlan`) or runs once
//!   (`InitPlan`) reads its rows once.
//!
//! The plans are made with sequential scans disabled (`enable_seqscan = off`), so the server takes
//! an index wherever one serves the condition, whatever the table's size: callback_state's hundred
//! rows, which a scan reads faster than any index, still show whether an index would serve the
//! condition on a board that has grown. A scan left is one no index serves.
//!
//! A scan with no condition reads every row because the caller wants every row (the board's
//! snapshot of its sessions and roles): no index makes that smaller, and it is listed
//! ([`whole_reads`]), not failed. A plan the check cannot read is kept as it came
//! ([`Logged::unread`]), for the check to report, never skipped.

use serde_json::Value;
use std::collections::BTreeSet;

/// The tables no path reads whole to keep a few rows (AGENTS.md, "Queries keep their indexes").
pub const WATCHED: [&str; 5] = ["message", "delivery", "role", "session", "callback_state"];

/// A finding of the check.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Finding {
    /// A watched table read whole, or one of its indexes from end to end, to keep a few rows.
    FilteredScan(String),
    /// A subquery the server runs again for each row it is asked of, reading this table by no
    /// index each time.
    SubPlanPerRow { subplan: String, table: String },
}

/// One plan the server gave: the statement's text, as the server ran it (a block's or a
/// trigger's statement as it is written there), and its plan, the tree `EXPLAIN (FORMAT JSON)`
/// gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub query: String,
    pub plan: Value,
}

/// What the server's plans were: each plan read, and each notice that says it is a plan but did
/// not read as one, as it came.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Logged {
    pub plans: Vec<Plan>,
    pub unread: Vec<String>,
}

impl Logged {
    /// Another run's plans and unread entries after these.
    pub fn extend(&mut self, other: Logged) {
        self.plans.extend(other.plans);
        self.unread.extend(other.unread);
    }
}

/// What `auto_explain` said in `notices`, each what one notice said: a plan's says
/// `duration: … ms  plan:` and then, on its next line, the plan's JSON. One that says that and then
/// no plan is kept unread; a notice of anything else is no plan's.
pub fn plans_of(notices: &[String]) -> Logged {
    let mut logged = Logged::default();
    for notice in notices {
        let (head, body) = notice.split_once('\n').unwrap_or((notice.as_str(), ""));
        if !(head.starts_with("duration: ") && head.ends_with("plan:")) {
            continue;
        }
        match read_plan(body) {
            Some(plan) => logged.plans.push(plan),
            None => logged.unread.push(notice.clone()),
        }
    }
    logged
}

/// A notice's JSON as a plan: its `Query Text` and its `Plan`.
fn read_plan(body: &str) -> Option<Plan> {
    let mut entry: Value = serde_json::from_str(body).ok()?;
    let query = entry.get("Query Text")?.as_str()?.to_owned();
    let plan = entry.get_mut("Plan")?.take();
    plan.is_object().then_some(Plan { query, plan })
}

/// The nodes under `node`, in order.
fn children(node: &Value) -> impl Iterator<Item = &Value> {
    (node.get("Plans").and_then(Value::as_array))
        .into_iter()
        .flatten()
}

fn text<'a>(node: &'a Value, field: &str) -> Option<&'a str> {
    node.get(field).and_then(Value::as_str)
}

/// Whether `node` reads its table's rows by no index: a scan of the whole table, or a walk of one
/// of its indexes with no condition on it.
fn unindexed(node: &Value) -> bool {
    match text(node, "Node Type") {
        Some("Seq Scan") => true,
        Some("Index Scan" | "Index Only Scan") => node.get("Index Cond").is_none(),
        _ => false,
    }
}

/// The subplans `plan` hashes, each named as the plan names it (`SubPlan 2`): a condition reads one
/// as `(hashed SubPlan 2)`, or `(hashed SubPlan 2).col1` where it compares a column of it.
fn hashed(plan: &Value) -> BTreeSet<String> {
    fn strings<'a>(value: &'a Value, into: &mut Vec<&'a str>) {
        match value {
            Value::String(text) => into.push(text),
            Value::Array(items) => items.iter().for_each(|item| strings(item, into)),
            Value::Object(fields) => fields.values().for_each(|field| strings(field, into)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    strings(plan, &mut texts);
    let mut found = BTreeSet::new();
    for text in texts {
        for (at, _) in text.match_indices("hashed SubPlan ") {
            let digits: String = (text[at + "hashed ".len() + "SubPlan ".len()..].chars())
                .take_while(char::is_ascii_digit)
                .collect();
            found.insert(format!("SubPlan {digits}"));
        }
    }
    found
}

/// What is wrong with `plan`, a plan's tree as `EXPLAIN (FORMAT JSON)` gives it.
pub fn findings(plan: &Value) -> Vec<Finding> {
    let hashed = hashed(plan);
    let mut found = Vec::new();
    walk(plan, &hashed, None, &mut found);
    found.sort();
    found.dedup();
    found
}

/// The findings of `node` and every node under it, `per_row` the subplan run for each row that
/// holds it, if one does.
fn walk(node: &Value, hashed: &BTreeSet<String>, per_row: Option<&str>, found: &mut Vec<Finding>) {
    let table = text(node, "Relation Name");
    if let Some(table) = table
        && unindexed(node)
    {
        if WATCHED.contains(&table) && node.get("Filter").is_some() {
            found.push(Finding::FilteredScan(table.to_owned()));
        }
        if let Some(subplan) = per_row {
            found.push(Finding::SubPlanPerRow {
                subplan: subplan.to_owned(),
                table: table.to_owned(),
            });
        }
    }
    for child in children(node) {
        let subplan = (text(child, "Parent Relationship") == Some("SubPlan"))
            .then(|| text(child, "Subplan Name"))
            .flatten();
        // A hashed subplan's table is built once, however often the subplan holding it runs, only
        // while it reads no value of an outer row: PostgreSQL builds it again each time a
        // parameter it reads changes. The plan does not say which it reads, and the walk passes
        // a hashed subplan as one built once.
        let per_row = match subplan {
            Some(name) if hashed.contains(name) => None,
            Some(name) => Some(name),
            None => per_row,
        };
        walk(child, hashed, per_row, found);
    }
}

/// The watched tables `plan` reads whole, with no condition: listed, not failed.
pub fn whole_reads(plan: &Value) -> Vec<String> {
    fn walk(node: &Value, into: &mut Vec<String>) {
        if let Some(table) = text(node, "Relation Name")
            && WATCHED.contains(&table)
            && text(node, "Node Type") == Some("Seq Scan")
            && node.get("Filter").is_none()
        {
            into.push(table.to_owned());
        }
        children(node).for_each(|child| walk(child, into));
    }
    let mut found = Vec::new();
    walk(plan, &mut found);
    found.sort();
    found.dedup();
    found
}

/// The indexes `plan` reads, by name.
pub fn indexes(plan: &Value) -> BTreeSet<String> {
    fn walk(node: &Value, into: &mut BTreeSet<String>) {
        if let Some(index) = text(node, "Index Name") {
            into.insert(index.to_owned());
        }
        children(node).for_each(|child| walk(child, into));
    }
    let mut found = BTreeSet::new();
    walk(plan, &mut found);
    found
}

/// `plan` as an outline to read, a node a line, indented by its depth: its type, its table and its
/// index, and its conditions.
pub fn outline(plan: &Value) -> String {
    fn walk(node: &Value, depth: usize, into: &mut String) {
        let mut line = format!(
            "{:indent$}{}",
            "",
            text(node, "Node Type").unwrap_or("?"),
            indent = depth * 2
        );
        for (field, said) in [
            ("Subplan Name", " "),
            ("Index Name", " using "),
            ("Relation Name", " on "),
            ("Index Cond", "  Index Cond: "),
            ("Recheck Cond", "  Recheck Cond: "),
            ("Hash Cond", "  Hash Cond: "),
            ("Join Filter", "  Join Filter: "),
            ("Filter", "  Filter: "),
        ] {
            if let Some(value) = text(node, field) {
                line.push_str(said);
                line.push_str(value);
            }
        }
        into.push_str(&line);
        into.push('\n');
        children(node).for_each(|child| walk(child, depth + 1, into));
    }
    let mut out = String::new();
    walk(plan, 0, &mut out);
    out
}

#[cfg(test)]
mod read {
    use super::*;
    use serde_json::json;

    /// A plan's node of `kind` on `table`, with `fields` beside.
    fn node(kind: &str, table: &str, fields: Value) -> Value {
        let mut node = json!({"Node Type": kind, "Relation Name": table, "Alias": table});
        node.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        node
    }

    /// A scan of the whole message table to keep one recipient's messages is found; a read by
    /// `message_recipient` is not; nor is a scan of a table no path guards.
    #[test]
    fn a_scan_of_message_under_a_condition_is_found_and_an_index_scan_passes() {
        let scan = node(
            "Seq Scan",
            "message",
            json!({"Disabled": true, "Filter": "(recipient = 'perf-0007'::text)"}),
        );
        assert_eq!(findings(&scan), [Finding::FilteredScan("message".into())]);
        let indexed = node(
            "Index Scan",
            "message",
            json!({"Index Name": "message_recipient", "Index Cond": "(recipient = 'perf-0007'::text)",
                "Filter": "(kind = 'msg'::text)"}),
        );
        assert_eq!(findings(&indexed), []);
        let task = node(
            "Seq Scan",
            "task",
            json!({"Filter": "(status = 'ready'::text)"}),
        );
        assert_eq!(findings(&task), []);
    }

    /// A walk of `message_sid` from the newest message down, keeping one sender's words, is found:
    /// an index read from end to end under a condition reads every row; the same index read from
    /// a key on is not.
    #[test]
    fn an_index_walked_from_end_to_end_under_a_condition_is_found() {
        let walk = json!({"Node Type": "Limit", "Plans": [node(
            "Index Scan",
            "message",
            json!({"Parent Relationship": "Outer", "Scan Direction": "Backward",
                "Index Name": "message_sid", "Filter": "(sender = 'perf-rare-sender'::text)"}),
        )]});
        assert_eq!(findings(&walk), [Finding::FilteredScan("message".into())]);
        let ranged = node(
            "Index Only Scan",
            "message",
            json!({"Index Name": "message_sender", "Index Cond": "(sender = 'perf-rare-sender'::text)"}),
        );
        assert_eq!(findings(&ranged), []);
    }

    /// A subquery the server hashes is read once, and passes, whatever it reads, under a subquery
    /// run for each row too; one it runs again for each row and that reads a table by no index
    /// each time is found (a watched one's scan under its condition with it); one that probes an
    /// index for each row passes; and one run once, before the rest, passes too.
    #[test]
    fn a_hashed_subplan_passes_and_one_scanning_for_each_row_is_found() {
        let session = |filter: &str, under: Value| {
            node(
                "Index Scan",
                "session",
                json!({"Index Name": "session_handle", "Index Cond": "(handle = 'perf-0001'::text)",
                    "Filter": filter, "Plans": [under]}),
            )
        };
        let subplan = |kind: &str, table: &str, fields: Value| {
            let mut under = node(kind, table, fields);
            under["Parent Relationship"] = json!("SubPlan");
            under["Subplan Name"] = json!("SubPlan 1");
            under
        };
        let tasks = subplan(
            "Seq Scan",
            "task",
            json!({"Filter": "(status = 'review'::text)"}),
        );
        let hashed = session("(ANY (handle = (hashed SubPlan 1).col1))", tasks.clone());
        assert_eq!(findings(&hashed), []);
        // The dispatch's due lines: an outbox read for each recipient, and under it the chats
        // delivered, hashed.
        let mut delivered = subplan(
            "Seq Scan",
            "chat_event",
            json!({"Filter": "((doc -> 'delivered'::text) = 'true'::jsonb)"}),
        );
        delivered["Subplan Name"] = json!("SubPlan 2");
        let mut outbox = subplan(
            "Bitmap Heap Scan",
            "callback_event",
            json!({"Recheck Cond": "((doc ->> 'handle'::text) = (r.r ->> 'handle'::text))",
                "Filter": "(NOT (ANY (k = (hashed SubPlan 2).col1)))",
                "Plans": [delivered.clone(), node("Bitmap Index Scan", "callback_event",
                    json!({"Index Name": "callback_event_handle"}))]}),
        );
        assert_eq!(
            findings(&session("((SubPlan 1) IS NOT NULL)", outbox.clone())),
            []
        );
        // The same chats read for each line instead, not hashed: found, under the outbox's read.
        outbox["Filter"] = json!("(NOT (SubPlan 2))");
        assert_eq!(
            findings(&session("((SubPlan 1) IS NOT NULL)", outbox)),
            [Finding::SubPlanPerRow {
                subplan: "SubPlan 2".into(),
                table: "chat_event".into()
            }]
        );
        let per_row = session("((SubPlan 1) > 0)", tasks);
        assert_eq!(
            findings(&per_row),
            [Finding::SubPlanPerRow {
                subplan: "SubPlan 1".into(),
                table: "task".into()
            }]
        );
        let roles = subplan(
            "Seq Scan",
            "role",
            json!({"Disabled": true, "Filter": "(role = s.kind)"}),
        );
        assert_eq!(
            findings(&session("((SubPlan 1) > 0)", roles)),
            [
                Finding::FilteredScan("role".into()),
                Finding::SubPlanPerRow {
                    subplan: "SubPlan 1".into(),
                    table: "role".into()
                },
            ]
        );
        let probe = subplan(
            "Index Scan",
            "role",
            json!({"Index Name": "role_handle", "Index Cond": "(handle = s.handle)"}),
        );
        assert_eq!(findings(&session("((SubPlan 1) > 0)", probe)), []);
        let once = node(
            "Index Scan",
            "task",
            json!({"Index Name": "task_slug", "Index Cond": "(slug = (InitPlan 1).col1)",
                "Plans": [node("Seq Scan", "counter", json!({"Parent Relationship": "InitPlan",
                "Subplan Name": "InitPlan 1"}))]}),
        );
        assert_eq!(findings(&once), []);
        assert_eq!(
            hashed_names("(ANY (handle = (hashed SubPlan 12).col1)) OR (hashed SubPlan 3)"),
            ["SubPlan 12", "SubPlan 3"]
        );
    }

    /// The subplans a condition `text` hashes.
    fn hashed_names(text: &str) -> Vec<String> {
        hashed(&json!({"Filter": text})).into_iter().collect()
    }

    /// A scan with no condition is a whole read, listed and not found.
    #[test]
    fn a_whole_read_is_listed_not_found() {
        let whole = json!({"Node Type": "Sort", "Plans": [node("Seq Scan", "session",
            json!({"Parent Relationship": "Outer", "Disabled": true}))]});
        assert_eq!(findings(&whole), []);
        assert_eq!(whole_reads(&whole), ["session"]);
        assert_eq!(indexes(&whole), BTreeSet::new());
        assert_eq!(outline(&whole), "Sort\n  Seq Scan on session\n");
    }

    /// A plan's notice is read; one cut short, or with no plan after its first line, is kept as
    /// it came, and a notice of anything else is passed over.
    #[test]
    fn the_notices_plans_are_read_and_one_that_is_not_whole_is_kept() {
        let whole = "duration: 0.383 ms  plan:
{
  \"Query Text\": \"SELECT count(*) FROM session WHERE kind = 'x';\",
  \"Plan\": {
    \"Node Type\": \"Aggregate\",
    \"Plans\": [
      {
        \"Node Type\": \"Seq Scan\",
        \"Parent Relationship\": \"Outer\",
        \"Relation Name\": \"session\",
        \"Filter\": \"(kind = 'x'::text)\"
      }
    ]
  }
}";
        let cut = "duration: 0.032 ms  plan:
{
  \"Query Text\": \"SELECT 1\",
  \"Plan\": {";
        let notices = [
            whole,
            "relation \"said\" already exists, skipping",
            cut,
            "duration: 1.000 ms  plan:",
        ]
        .map(String::from);
        let logged = plans_of(&notices);
        assert_eq!(logged.plans.len(), 1, "{logged:#?}");
        assert_eq!(
            logged.plans[0].query,
            "SELECT count(*) FROM session WHERE kind = 'x';"
        );
        assert_eq!(
            findings(&logged.plans[0].plan),
            [Finding::FilteredScan("session".into())]
        );
        assert_eq!(
            logged.unread,
            [cut, "duration: 1.000 ms  plan:"],
            "{logged:#?}"
        );
    }
}
