//! What a plan the server gives (`EXPLAIN`'s text) says is wrong with a statement:
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

/// The tables no path reads whole to keep a few rows.
pub const WATCHED: [&str; 5] = ["message", "delivery", "role", "session", "callback_state"];

/// A finding of the check, or a plan it lists.
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// A whole table read to keep a few rows.
    FilteredScan(String),
    /// A subquery the server runs again for every row a predicate tests.
    SubqueryPerRow(String),
    /// A statement the server would not plan, or one reading a variable the check has no value for.
    NotPlanned,
}

/// What is wrong with `plan`, an `EXPLAIN`'s text. A scan of a whole index (no `access` range)
/// under a `Filter` is a filtered scan too: the server takes an index for its order alone (`ORDER
/// BY sid` through `message_sid`) when the one its condition needs is gone, and reads every row.
pub fn findings(plan: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut parent = "";
    let lines = plan.lines();
    for line in lines {
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
pub fn whole_read(plan: &str) -> Option<String> {
    let mut scans = (plan.lines())
        .filter(|line| line.trim_start().starts_with("TableScan") && !line.contains("predicate: "));
    scans.find_map(|line| {
        let table = line
            .split_once("table: ")?
            .1
            .split([',', ']'])
            .next()?
            .trim();
        WATCHED.contains(&table).then(|| table.to_string())
    })
}

#[cfg(test)]
mod reading {
    use super::*;

    /// None: what a plan with nothing wrong in it gives.
    fn none() -> Vec<Finding> {
        Vec::new()
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
        assert_eq!(findings(indexed), none());
        let filtered = "SelectProject [ctx: Db] [projections: key]
    Filter [ctx: Db] [predicate: delivered != true AND attempts < 3]
        IndexScan [ctx: Db] [index: callback_event_handle, access: ['x'], direction: Forward]";
        assert_eq!(findings(filtered), none());
        let whole = "SelectProject [ctx: Db] [projections: *, OMIT id]
    SortByKey [ctx: Db] [sort_keys: connected ASC, handle ASC]
        TableScan [ctx: Db] [table: session, direction: Forward]";
        assert_eq!(findings(whole), none());
        assert_eq!(whole_read(whole).as_deref(), Some("session"));
        let unwatched = "SelectProject [ctx: Db] [projections: *]
    TableScan [ctx: Db] [table: task, direction: Forward, predicate: status INSIDE ['ready'], pre_decode_filter: yes]";
        assert_eq!(findings(unwatched), none());
    }

    /// The plan a server gave a lower-case `select * from session where kind = 'reviewer'`
    /// (2026-11-07): the keyword's case changes nothing in it.
    #[test]
    fn a_lower_case_select_of_session_under_a_condition_is_a_filtered_scan() {
        let lower = "SelectProject [ctx: Db] [projections: *]
    TableScan [ctx: Db] [table: session, direction: Forward, predicate: kind = 'reviewer', pre_decode_filter: yes]";
        assert_eq!(findings(lower), [Finding::FilteredScan("session".into())]);
    }

    /// `to_owner` as a clone of the live board planned it with `message_recipient`, and with that
    /// index removed: the server walked all of `message_sid` for the order alone.
    #[test]
    fn a_whole_index_read_under_a_filter_is_a_finding_and_a_ranged_one_is_not() {
        let ranged = "SelectProject [ctx: Db] [projections: *, OMIT id]
    IndexScan [ctx: Db] [index: message_recipient, access: ['owner'], direction: Backward, limit: 40]";
        assert_eq!(findings(ranged), none());
        let dropped = "SelectProject [ctx: Db] [projections: *, OMIT id]
    Limit [ctx: Db] [limit: 40]
        Filter [ctx: Db] [predicate: recipient = 'owner']
            IndexScan [ctx: Db] [index: message_sid, access: , direction: Backward]";
        assert_eq!(findings(dropped), [Finding::FilteredScan("message".into())]);
        let bounded = "SelectProject [ctx: Db] [projections: handle]
    Filter [ctx: Db] [predicate: type::record('session', handle).left_at = NONE]
        IndexScan [ctx: Db] [index: callback_state_address, access: >NONE, direction: Forward]";
        assert_eq!(findings(bounded), none());
    }
}
