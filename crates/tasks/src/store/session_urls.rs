//! A session's web address ([`crate::session_url`]): `callback_state:['session_url', HANDLE]`,
//! never a field of the `session` row. Its `url` is no field the board marker's event watches, so
//! setting one moves no board marker; the agents view reads every address each time it is built.

use super::postgres::session_urls as pg;
use super::statement::Statement;
use super::{Error, Store, refused};
use crate::maps::Collect as _;
use serde_json::{Value, json};
use std::collections::BTreeMap;

impl Store {
    /// Give `handle`'s session the web address `url`, checked as [`crate::session_url::checked`]
    /// checks it (an empty one clears it), as that session: refused for an address it refuses, and
    /// for a session that is unknown or left.
    pub fn set_session_url(&self, handle: &str, url: &str) -> Result<Option<String>, Error> {
        let url = crate::session_url::checked(url).map_err(refused)?;
        let vars = self.acting(handle, (self.clock)(), json!({"url": url}));
        self.retrying(|| self.transact::<Value>(Statement::from(pg::set()), vars.clone()))?;
        Ok(url)
    }

    /// The web address of every session that has one and has not left, by handle: each checked
    /// again as [`crate::session_url::checked`] checks one being set, so a row written any other
    /// way (a copied dump, a hand write, another build) that holds an address it refuses, a
    /// `javascript:` one say, is left out, and no page is handed it to link.
    pub fn session_urls(&self) -> Result<BTreeMap<String, String>, Error> {
        let found: Vec<(String, Value)> =
            self.retrying(|| self.read(Statement::from(pg::read()), json!({})))?;
        Ok(found
            .into_iter()
            .filter_map(|(handle, url)| {
                let kept = crate::session_url::checked(url.as_str()?).ok()??;
                Some((handle, kept))
            })
            .collect_map())
    }
}
