//! A task's needs, written by its assessor ([`crate::stamps`]): the board's own field
//! `fields._board.needs`, each change a line on the task's thread, in one write.
//!
//! The assessor is a role in name only, never a role row: the write itself holds the rule a
//! reviewer's verdict is held to ([`Held::unfresh`], with no fast fix's exception), so a session
//! that executed the task or work under it, orchestrates or orchestrated it, or filed it, writes
//! none, and any other session may.

use crate::stamps::{Catalogue, Needs};
use crate::store::{Change, Error, Say, Store, Task, refused};
use crate::{Held, Unfresh};

/// The write of [`Change::needs`] after the rule's own ([`super::super::WRITE`]).
pub(in crate::store) const NEEDS_WRITTEN: &str =
    "\nUPDATE type::record('task', $slug) SET fields._board.needs = $needs RETURN NONE;";

/// The refusal of `handle`'s needs on `slug`, for what makes it no assessor of it (`why`).
fn unfresh(slug: &str, handle: &str, why: Unfresh) -> String {
    let did = match why {
        Unfresh::Executed => "executed",
        Unfresh::Orchestrated => "orchestrated",
        Unfresh::Filed => "filed",
    };
    format!(
        "{handle} {did} {slug}: its needs are its assessor's, a session that never executed, \
         orchestrated or filed it, which its orchestrator starts as a session of its own"
    )
}

impl Store {
    /// Writes `slug`'s needs (`needs`, written by `needs.by`), the stamps it names checked against
    /// `catalogue` ([`Catalogue::needs`]: [`crate::stamps::NONE`] alone for none), with a line on its
    /// thread saying them, the tip read and what it needed before: its last assessment's stamps,
    /// else the catalogue's defaults ([`Needs::line`]). Refused: an empty reason, a stamp the
    /// catalogue lacks, a task done or cancelled, and a writer that executed the task or work under
    /// it, orchestrates or orchestrated it, or filed it ([`Held::unfresh`]).
    pub fn set_needs(
        &self,
        slug: &str,
        catalogue: &Catalogue,
        needs: &Needs,
    ) -> Result<Task, Error> {
        let handle = needs.by.as_str();
        let why = needs.why.trim();
        if why.is_empty() {
            return Err(refused(
                "an assessment says why (--why WHY): one line for each stamp it left out or added"
                    .into(),
            ));
        }
        let stamps = catalogue.needs(&needs.stamps).map_err(refused)?;
        let tip = (needs.tip.as_deref())
            .map(str::trim)
            .filter(|tip| !tip.is_empty())
            .map(str::to_string);
        let written = Needs {
            stamps,
            tip,
            by: handle.into(),
            why: why.into(),
        };
        let value = serde_json::to_value(&written).expect("needs are JSON");
        let held: Held = self.ever_held(slug)?;
        let defaults = catalogue.defaults();
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!(
                    "{slug} is {}: only an open task is assessed",
                    task.status
                )));
            }
            let orchestrates = view.holds(handle, "orchestrator");
            if let Some(why) = held.unfresh(handle, &task.created_by, orchestrates, false) {
                return Err(refused(unfresh(slug, handle, why)));
            }
            let was = Needs::of(&task.fields).map_or_else(|| defaults.clone(), |n| n.stamps);
            Ok(Change {
                needs: Some(value.clone()),
                say: Say::system(written.line(&was)),
                ..Change::default()
            })
        })
    }
}
