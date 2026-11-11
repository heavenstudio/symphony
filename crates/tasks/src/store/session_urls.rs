//! A session's web address ([`crate::session_url`]): `callback_state:['session_url', HANDLE]`,
//! never a field of the `session` row. Its `url` is no field the board marker's event watches, so
//! setting one moves no board marker; the agents view reads every address each time it is built.

use super::{ACT, Error, Store, acting, refused};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Every address of a session that has not left, by handle: one read of the `session_url` rows,
/// by their ids' range.
const READ: &str = "RETURN SELECT VALUE [record::id(id)[1], url]
    FROM callback_state:['session_url', NONE]..['session_url', ..]
    WHERE url != NONE AND type::record('session', record::id(id)[1]).left_at = NONE;";

impl Store {
    /// Give `handle`'s session the web address `url`, checked as [`crate::session_url::checked`]
    /// checks it (an empty one clears it), as that session: refused for an address it refuses, and
    /// for a session that is unknown or left.
    pub fn set_session_url(&self, handle: &str, url: &str) -> Result<Option<String>, Error> {
        let url = crate::session_url::checked(url).map_err(refused)?;
        let body = format!(
            "{ACT}
LET $key = type::record('callback_state', ['session_url', $actor]);
IF $url = NONE {{ DELETE $key RETURN NONE; }}
ELSE {{ UPSERT $key SET url = $url RETURN NONE; }};
RETURN NONE;"
        );
        let vars = acting(handle, (self.clock)(), json!({"url": url}));
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))?;
        Ok(url)
    }

    /// The web address of every session that has one and has not left, by handle.
    pub fn session_urls(&self) -> Result<BTreeMap<String, String>, Error> {
        let found: Vec<(String, String)> = self.retrying(|| self.read(READ, json!({})))?;
        Ok(found.into_iter().collect())
    }
}
