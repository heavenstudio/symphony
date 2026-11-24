//! The record the pipeline alarm replays ([`crate::alarm`]): the lines a change of status writes,
//! read through `message_ts` from the start of its window; and from a task's own thread, a range of
//! keys read back from its newest a page at a time, its newest such lines before the window and
//! when it last went into review.

use super::postgres::{Pg, history as pg};
use super::statement::Statement;
use super::{Error, Store};
use crate::alarm::{self, ARROW, CLAIMED, Line, MOVED, Step, UNDER, WAITS_SET};
use crate::json_object::object;
use crate::stages::{BACK_TO, BLOCK_LINE, UNBLOCKED};
use serde_json::{Value, json};
use std::collections::HashMap;

/// How many lines one read of a thread takes ([`Store::newest`]).
pub(super) const PAGE: usize = 4;

/// What [`pg::status_lines`], [`pg::steps_before`] and [`pg::waits_before`] match a line by,
/// from `since`.
fn status_vars(since: f64) -> Value {
    object(vec![
        ("since", Value::from(since)),
        ("moved", Value::from(MOVED)),
        ("claimed", Value::from(CLAIMED)),
        ("block", Value::from(BLOCK_LINE)),
        ("unblock", Value::from(UNBLOCKED)),
        ("waits", Value::from(WAITS_SET)),
        ("under", Value::from(UNDER)),
    ])
}

impl Store {
    /// The current waits occurrence from historical boards whose assessment has no sequence.
    /// Match the standing text as well, so malformed or unrelated evidence supplies no identity.
    pub(crate) fn waits_record(&self, task: &crate::Task) -> Result<Option<Line>, Error> {
        let vars = status_vars(f64::MAX);
        let query = pg::waits_before();
        let mut records = self.newest(&[&task.slug], &query, &vars, |line| {
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
        let Some(first) = value.and_then(|value| crate::objects::read::<alarm::Tip>(value).ok())
        else {
            return Ok(None);
        };
        if !first.at.is_finite() || first.commit.is_empty() {
            return Ok(None);
        }
        let vars = json!({ "slug": task.slug, "at": record.at, "waits": WAITS_SET });
        let lines: Vec<Line> =
            self.retrying(|| self.read(Statement::from(pg::waits_at()), vars.clone()))?;
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
        let mut lines: Vec<Line> =
            self.retrying(|| self.read(Statement::from(pg::status_lines()), vars.clone()))?;
        crate::order::sort_by(&mut lines, &|a, b| (&a.slug, a.sid).cmp(&(&b.slug, b.sid)));
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
        let steps = pg::steps_before();
        let steps = self.newest(slugs, &steps, &vars, changed)?;
        let recorded = |line: &Line| alarm::waits_set(line).is_some();
        let records = pg::waits_before();
        let records = self.newest(slugs, &records, &vars, recorded)?;
        let mut said: HashMap<String, Vec<Line>> = HashMap::new();
        let both = steps.into_iter().chain(records);
        for (slug, line) in both {
            said.entry(slug).or_default().push(line);
        }
        for lines in said.values_mut() {
            crate::order::sort_by_key(lines, &|line| line.sid);
        }
        Ok(said)
    }

    /// When each of `slugs` last went into review, by its thread; one whose thread never says is
    /// left out.
    pub fn entered_review(&self, slugs: &[&str]) -> Result<HashMap<String, f64>, Error> {
        let vars = json!({
            "moved_in": format!("{ARROW}review"),
            "unblock": UNBLOCKED,
            "back_in": format!("{BACK_TO}review"),
        });
        let into = |line: &Line| matches!(alarm::step(line), Step::Between(_, "review"));
        let found = self.newest(slugs, &pg::into_review(), &vars, into)?;
        Ok((found.into_iter())
            .map(|(slug, line)| (slug, line.ts))
            .collect())
    }

    /// Of each of `slugs`' threads, its newest line `query`'s filter matches (with `vars`) that
    /// `said` takes, read back from its newest [`PAGE`] lines at a time, so that lines the filter
    /// lets through and `said` refuses (a field's text that reads like a line of the board's) hide
    /// none older; one with none is left out.
    fn newest(
        &self,
        slugs: &[&str],
        query: &Pg<'static>,
        vars: &Value,
        said: fn(&Line) -> bool,
    ) -> Result<HashMap<String, Line>, Error> {
        let mut found = HashMap::new();
        let mut reading: Vec<(&str, i64)> = slugs.iter().map(|slug| (*slug, i64::MAX)).collect();
        while !reading.is_empty() {
            let from: Vec<Value> = (reading.iter())
                .map(|(slug, sid)| {
                    object(vec![
                        ("slug", Value::from(*slug)),
                        ("sid", Value::from(*sid)),
                    ])
                })
                .collect();
            let mut vars = vars.clone();
            vars["from"] = Value::Array(from);
            let pages: Vec<Vec<Line>> =
                self.retrying(|| self.read(Statement::from(query.clone()), vars.clone()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_variables_keep_every_field_its_order_and_numeric_value() {
        for since in [0.0, -0.0, f64::MAX] {
            let expected = json!({
                "since": since, "moved": MOVED, "claimed": CLAIMED,
                "block": BLOCK_LINE, "unblock": UNBLOCKED,
                "waits": WAITS_SET, "under": UNDER,
            });
            assert_eq!(status_vars(since).to_string(), expected.to_string());
        }
    }
}
