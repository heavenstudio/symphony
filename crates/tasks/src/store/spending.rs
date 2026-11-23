//! The tokens sessions spent ([`crate::usage`]): added by a beat in its own transaction
//! ([`pg::SPEND`]), read back by task and by session.

use super::postgres::spending as pg;
use super::statement::Statement;
use super::{Error, Store};
use crate::health::FINISHED;
use crate::rows::UsageRow;
use crate::stages::{ARROW, MOVED};
use crate::usage::{Prices, Spend, Spent};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// [`pg::SPEND`]'s variables, beside the beat's own.
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
        self.retrying(|| self.read(Statement::from(pg::spent_on()), json!({"slugs": slugs})))
    }

    /// What each of `slugs` took, its subtasks' with it as its page counts them, and what that
    /// cost at the prices every build embeds: one read however many are asked, which the board's
    /// cards make for those drawn. One that took none is left out; a subtask asked for beside its
    /// parent counts in both.
    pub fn spent_under(&self, slugs: &[&str]) -> Result<BTreeMap<String, Spend>, Error> {
        if slugs.is_empty() {
            return Ok(BTreeMap::new());
        }
        let answer: Value = self.retrying(|| {
            self.read_together(Statement::from(pg::spent_under()), json!({"slugs": slugs}))
        })?;
        let unread = || Error::Db("the tokens' read is not in the board's shape".into());
        let rows: Vec<UsageRow> =
            crate::objects::read(answer.get("rows").ok_or_else(unread)?).map_err(|_| unread())?;
        let mut parents = BTreeMap::new();
        for under in answer
            .get("under")
            .and_then(Value::as_array)
            .ok_or_else(unread)?
        {
            let text = |name: &str| under.get(name).and_then(Value::as_str);
            parents.insert(
                text("slug").ok_or_else(unread)?,
                text("parent").ok_or_else(unread)?,
            );
        }
        let prices = Prices::embedded();
        let mut by: BTreeMap<String, Spend> = BTreeMap::new();
        for row in &rows {
            let spend = row.spend(prices);
            let parent = parents.get(row.slug.as_str()).copied();
            for slug in [Some(row.slug.as_str()), parent].into_iter().flatten() {
                if slugs.contains(&slug) {
                    by.entry(slug.to_owned()).or_default().add(&spend);
                }
            }
        }
        Ok(by)
    }

    /// The tokens each of `handles` spent, all told, and what they cost at the prices every build
    /// embeds, each row priced by its own model and speed; one that spent none is left out.
    pub fn spent_by(&self, handles: &[&str]) -> Result<BTreeMap<String, Spend>, Error> {
        if handles.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows: Vec<UsageRow> = self
            .retrying(|| self.read(Statement::from(pg::spent_by()), json!({"handles": handles})))?;
        let prices = Prices::embedded();
        let mut by = BTreeMap::new();
        for row in &rows {
            by.entry(row.handle.clone())
                .or_insert_with(Spend::default)
                .add(&row.spend(prices));
        }
        Ok(by)
    }
}
