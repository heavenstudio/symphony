//! The tokens sessions spent ([`crate::usage`]): added by a beat in its own transaction
//! ([`sql::SPEND`]), read back by task and by session.

use super::postgres::spending as pg;
use super::statement::both;
use super::surreal::spending as sql;
use super::{Error, Store};
use crate::health::FINISHED;
use crate::rows::UsageRow;
use crate::stages::{ARROW, MOVED};
use crate::usage::{Prices, Spend, Spent};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// [`sql::SPEND`]'s variables, beside the beat's own.
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
        self.retrying(|| self.read(both(sql::SPENT_ON, pg::spent_on()), json!({"slugs": slugs})))
    }

    /// The tokens each of `handles` spent, all told, and what they cost at the prices every build
    /// embeds, each row priced by its own model and speed; one that spent none is left out.
    pub fn spent_by(&self, handles: &[&str]) -> Result<BTreeMap<String, Spend>, Error> {
        if handles.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows: Vec<UsageRow> = self.retrying(|| {
            self.read(
                both(sql::SPENT_BY, pg::spent_by()),
                json!({"handles": handles}),
            )
        })?;
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
