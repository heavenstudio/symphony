//! The tokens sessions spent ([`crate::usage`]): added by a beat in its own transaction
//! ([`SPEND`]), read back by task and by session.

use super::{Error, Store};
use crate::health::FINISHED;
use crate::rows::UsageRow;
use crate::stages::{ARROW, MOVED};
use crate::usage::{Spent, Tokens};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// After [`super::sessions`]' beat, which found `$s`, the session bound to the agent: `$spent`
/// added to its row. The task is the one the agents view shows it working (an open task it holds
/// a role on, bar orchestrating: an executor's before a reviewer's before a reservation, then by
/// priority and slug); with none, no task, and its kind for the role. The round is 1 and the moves
/// into review the task's thread holds (`HANDLE moved it FROM → review`), read through the
/// thread's range of keys; 0 with no task.
pub(super) const SPEND: &str = "
IF $s != NONE {
    LET $working = (SELECT slug, role,
            (IF role = 'executor' { 0 } ELSE IF role = 'reviewer' { 1 } ELSE { 2 }) AS rank,
            (type::record('task', slug).priority ?? 2) AS priority,
            type::record('task', slug).status AS status
        FROM role WHERE handle = $s.handle AND role != 'orchestrator');
    LET $on = (SELECT slug, role, rank, priority FROM $working
        WHERE status != NONE AND status NOT IN $finished ORDER BY rank, priority, slug LIMIT 1)[0];
    LET $slug = $on.slug ?? '';
    LET $role = $on.role ?? $s.kind;
    LET $round = IF $on = NONE { 0 } ELSE {
        array::len(SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..]
            WHERE kind = 'system' AND string::contains(text, $moved) AND string::ends_with(text, $into_review)) + 1
    };
    FOR $x IN $spent {
        UPSERT type::record('usage', [$s.handle, $slug, $role, $x.model, $x.speed, $round]) SET
            handle = $s.handle, slug = $slug, role = $role, model = $x.model, speed = $x.speed,
            round = $round,
            input = (input ?? 0) + $x.tokens.input,
            cache_write_5m = (cache_write_5m ?? 0) + $x.tokens.cache_write_5m,
            cache_write_1h = (cache_write_1h ?? 0) + $x.tokens.cache_write_1h,
            cache_read = (cache_read ?? 0) + $x.tokens.cache_read,
            output = (output ?? 0) + $x.tokens.output,
            calls = (calls ?? 0) + $x.tokens.calls,
            first = first ?? $now, last = $now
        RETURN NONE;
    };
};
";

/// [`SPEND`]'s variables, beside the beat's own.
pub(super) fn vars(vars: &mut Value, spent: &[Spent]) {
    vars["spent"] = json!(spent);
    vars["finished"] = json!(FINISHED);
    vars["moved"] = json!(MOVED);
    vars["into_review"] = json!(format!("{ARROW}review"));
}

impl Store {
    /// The rows of the tokens spent on `slugs`, by handle, task, role, model, speed and round.
    pub fn spent_on(&self, slugs: &[&str]) -> Result<Vec<UsageRow>, Error> {
        if slugs.is_empty() {
            return Ok(Vec::new());
        }
        let read = "SELECT * OMIT id FROM usage WHERE slug IN $slugs
            ORDER BY handle, slug, role, model, speed, round;";
        self.retrying(|| self.read(read, json!({"slugs": slugs})))
    }

    /// The tokens each of `handles` spent, all told; one that spent none is left out.
    pub fn spent_by(&self, handles: &[&str]) -> Result<BTreeMap<String, Tokens>, Error> {
        if handles.is_empty() {
            return Ok(BTreeMap::new());
        }
        let read = "SELECT * OMIT id FROM usage WHERE handle IN $handles;";
        let rows: Vec<UsageRow> =
            self.retrying(|| self.read(read, json!({"handles": handles})))?;
        let mut by = BTreeMap::new();
        for row in &rows {
            by.entry(row.handle.clone())
                .or_insert_with(Tokens::default)
                .add(&row.tokens());
        }
        Ok(by)
    }
}
