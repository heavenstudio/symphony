//! The record the pipeline alarm replays ([`crate::alarm`]): the lines a change of status writes,
//! read through `message_ts` from the start of its window; and from a task's own thread, a range of
//! keys read back from its newest a page at a time, its newest such lines before the window and
//! when it last went into review.

use super::{Error, Store};
use crate::alarm::{self, ARROW, CLAIMED, Line, MOVED, Step, UNDER, WAITS_SET};
use crate::stages::{BLOCK_LINE, UNBLOCK_LINE};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The lines since `$since` that may say a status changed: every verdict, and the system lines of a
/// move, a claim, a block by a cancelled dependency and its end, of a change to what a task's
/// landing waits on, and of a move in the tree.
const STATUS_LINES: &str = "SELECT slug, sid, ts, kind, text FROM message
    WHERE ts >= $since AND kind IN ['system', 'verdict']
    AND (kind = 'verdict' OR string::contains(text, $moved) OR string::contains(text, $claimed)
        OR string::starts_with(text, $block) OR string::starts_with(text, $unblock)
        OR string::contains(text, $waits) OR string::contains(text, $under));";

/// A line before `$since` that may say a status changed, as [`STATUS_LINES`] reads one.
const STEP_BEFORE: &str = "ts < $since AND kind IN ['system', 'verdict']
    AND (kind = 'verdict' OR string::contains(text, $moved) OR string::contains(text, $claimed)
        OR string::starts_with(text, $block) OR string::starts_with(text, $unblock))";

/// A line before `$since` that may record what a task's landing waits on, as [`STATUS_LINES`] reads
/// one.
const WAITS_BEFORE: &str = "ts < $since AND kind = 'system' AND string::contains(text, $waits)";

/// A line that may have taken a task into review: a move, or the end of a block that returns it
/// there.
const INTO_REVIEW: &str = "kind = 'system'
    AND (string::ends_with(text, $moved_in) OR string::starts_with(text, $unblocked_in))";

/// How many lines one read of a thread takes ([`Store::newest`]).
const PAGE: usize = 4;

/// The read, of each thread `$from` names, of its newest [`PAGE`] lines `filter` matches before the
/// line of its `sid`: a range of the thread's keys.
fn page_query(filter: &str) -> String {
    format!(
        "RETURN $from.map(|$line| (SELECT slug, sid, ts, kind, text
            FROM message:[$line.slug, NONE]..[$line.slug, $line.sid]
            WHERE {filter} ORDER BY sid DESC LIMIT {PAGE}));"
    )
}

/// What [`STATUS_LINES`], [`STEP_BEFORE`] and [`WAITS_BEFORE`] match a line by, from `since`.
fn status_vars(since: f64) -> Value {
    json!({
        "since": since,
        "moved": MOVED,
        "claimed": CLAIMED,
        "block": BLOCK_LINE,
        "unblock": UNBLOCK_LINE,
        "waits": WAITS_SET,
        "under": UNDER,
    })
}

impl Store {
    /// The current waits occurrence from historical boards whose assessment has no sequence.
    /// Match the standing text as well, so malformed or unrelated evidence supplies no identity.
    pub(crate) fn waits_record(&self, task: &crate::Task) -> Result<Option<Line>, Error> {
        let vars = status_vars(f64::MAX);
        let mut records = self.newest(&[&task.slug], WAITS_BEFORE, &vars, |line| {
            alarm::waits_set(line).is_some()
        })?;
        Ok(records.remove(&task.slug).filter(|line| {
            alarm::Recorded::of(line).is_some()
                && alarm::waits_set(line)
                    == task
                        .fields
                        .get(crate::red::FIELDS)
                        .and_then(|board| board.get(alarm::WAITS))
                        .and_then(Value::as_str)
        }))
    }

    /// Adopt a timestamp-keyed historical end only when the thread proves it names this single
    /// occurrence. Ambiguous times never lend another assessment their answer.
    pub(crate) fn legacy_fix(
        &self,
        task: &crate::Task,
        record: alarm::Written<'_>,
    ) -> Result<Option<alarm::Tip>, Error> {
        let value = task
            .fields
            .get(crate::red::FIELDS)
            .and_then(|board| board.get(alarm::FIXED))
            .and_then(|fixed| fixed.get(record.tip))
            .and_then(|ends| ends.get(alarm::written_key(record.at)));
        let Some(first) =
            value.and_then(|value| serde_json::from_value::<alarm::Tip>(value.clone()).ok())
        else {
            return Ok(None);
        };
        if !first.at.is_finite() || first.commit.is_empty() {
            return Ok(None);
        }
        let vars = json!({ "slug": task.slug, "at": record.at, "waits": WAITS_SET });
        let query = "SELECT slug, sid, ts, kind, text FROM message WHERE slug = $slug AND ts = $at
            AND kind = 'system' AND string::contains(text, $waits);";
        let lines: Vec<Line> = self.retrying(|| self.read(query, vars.clone()))?;
        let records: Vec<_> = lines
            .iter()
            .filter_map(alarm::Recorded::of)
            .filter(|line| line.written.tip == record.tip)
            .collect();
        Ok((records.len() == 1 && records[0].written == record).then_some(first))
    }

    /// The lines written since `since` that may say a task's status changed
    /// ([`crate::alarm::step`]), each thread's in the order written.
    pub fn status_lines(&self, since: f64) -> Result<Vec<Line>, Error> {
        let vars = status_vars(since);
        let mut lines: Vec<Line> = self.retrying(|| self.read(STATUS_LINES, vars.clone()))?;
        lines.sort_by(|a, b| (&a.slug, a.sid).cmp(&(&b.slug, b.sid)));
        Ok(lines)
    }

    /// Each of `slugs`' newest line written before `since` that says its status changed
    /// ([`alarm::step`]) and its newest that records what its landing waits on
    /// ([`alarm::WAITS`]), in the order written; one with neither is left out.
    pub fn lines_before(
        &self,
        slugs: &[&str],
        since: f64,
    ) -> Result<HashMap<String, Vec<Line>>, Error> {
        let vars = status_vars(since);
        let changed = |line: &Line| alarm::step(line) != Step::Unchanged;
        let steps = self.newest(slugs, STEP_BEFORE, &vars, changed)?;
        let recorded = |line: &Line| alarm::waits_set(line).is_some();
        let records = self.newest(slugs, WAITS_BEFORE, &vars, recorded)?;
        let mut said: HashMap<String, Vec<Line>> = HashMap::new();
        let both = steps.into_iter().chain(records);
        for (slug, line) in both {
            said.entry(slug).or_default().push(line);
        }
        for lines in said.values_mut() {
            lines.sort_by_key(|line| line.sid);
        }
        Ok(said)
    }

    /// When each of `slugs` last went into review, by its thread; one whose thread never says is
    /// left out.
    pub fn entered_review(&self, slugs: &[&str]) -> Result<HashMap<String, f64>, Error> {
        let vars = json!({
            "moved_in": format!("{ARROW}review"),
            "unblocked_in": format!("{UNBLOCK_LINE}review"),
        });
        let into = |line: &Line| matches!(alarm::step(line), Step::Between(_, "review"));
        let found = self.newest(slugs, INTO_REVIEW, &vars, into)?;
        Ok((found.into_iter())
            .map(|(slug, line)| (slug, line.ts))
            .collect())
    }

    /// Of each of `slugs`' threads, its newest line `filter` matches (with `vars`) that `said`
    /// takes, read back from its newest [`PAGE`] lines at a time, so that lines the filter lets
    /// through and `said` refuses (a field's text that reads like a line of the board's) hide none
    /// older; one with none is left out.
    fn newest(
        &self,
        slugs: &[&str],
        filter: &str,
        vars: &Value,
        said: fn(&Line) -> bool,
    ) -> Result<HashMap<String, Line>, Error> {
        let query = page_query(filter);
        let mut found = HashMap::new();
        let mut reading: Vec<(&str, i64)> = slugs.iter().map(|slug| (*slug, i64::MAX)).collect();
        while !reading.is_empty() {
            let from: Vec<Value> = (reading.iter())
                .map(|(slug, sid)| json!({"slug": slug, "sid": sid}))
                .collect();
            let mut vars = vars.clone();
            vars["from"] = json!(from);
            let pages: Vec<Vec<Line>> = self.retrying(|| self.read(&query, vars.clone()))?;
            let mut further = Vec::new();
            let paged = reading.into_iter().zip(pages);
            for ((slug, _), page) in paged {
                // A page short of full is the thread's last.
                let older = (page.last().map(|line| line.sid)).filter(|_| page.len() == PAGE);
                match page.into_iter().find(said) {
                    Some(line) => {
                        found.insert(slug.to_string(), line);
                    }
                    None => further.extend(older.map(|sid| (slug, sid))),
                }
            }
            reading = further;
        }
        Ok(found)
    }
}
