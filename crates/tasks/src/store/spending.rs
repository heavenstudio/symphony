//! The tokens sessions spent ([`crate::usage`]): added by a beat in its own transaction
//! ([`sql::SPEND`]), read back by task and by session.

use super::surreal::spending as sql;
use super::{Error, Store};
use crate::health::FINISHED;
use crate::rows::UsageRow;
use crate::stages::{ARROW, MOVED};
use crate::usage::{Spent, Tokens};
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
        self.retrying(|| self.read(sql::SPENT_ON, json!({"slugs": slugs})))
    }

    /// The tokens each of `handles` spent, all told; one that spent none is left out.
    pub fn spent_by(&self, handles: &[&str]) -> Result<BTreeMap<String, Tokens>, Error> {
        if handles.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows: Vec<UsageRow> =
            self.retrying(|| self.read(sql::SPENT_BY, json!({"handles": handles})))?;
        let mut by = BTreeMap::new();
        for row in &rows {
            by.entry(row.handle.clone())
                .or_insert_with(Tokens::default)
                .add(&row.tokens());
        }
        Ok(by)
    }
}
