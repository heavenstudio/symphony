//! A task's kind given or changed after it was filed ([`crate::kind`]): an orchestrator names one
//! for a task the owner filed from a page as it briefs it, as it sets a priority.

use super::{Error, OWNER, REFUSED, Store, Task, no_task, pg, refused};
use crate::store::postgres::Pg;
use crate::store::statement::Statement;
use serde_json::json;

impl Store {
    /// Gives the open task `slug` the kind `kind` ([`crate::kind::KINDS`]) by `handle`, a session
    /// or the owner, saying on its thread the kind it gave and the one it replaced. A task of that
    /// kind already is left as it is, its thread unchanged; a done or cancelled one keeps what it
    /// was filed with, refused.
    pub fn set_kind(&self, slug: &str, kind: &str, handle: &str) -> Result<Task, Error> {
        let kind = crate::kind::named(kind).map_err(refused)?;
        // The kind it had is read in the write, so the line names the one it replaced.
        let acts = handle != OWNER;
        let body = || Statement::from(Pg::script(pg::set_kind(acts))).posting();
        self.retrying(|| {
            let vars = self.acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug, "kind": kind,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                    "closed": format!("{REFUSED}{slug} is "),
                }),
            );
            self.written(body(), vars, slug)
        })
    }
}
