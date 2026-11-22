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
    /// catalogue lacks, an import named with no tip or not as a whole commit, a task cancelled or
    /// done, and a writer that executed the task or work under it, orchestrates or orchestrated
    /// it, or filed it ([`Held::unfresh`]). A task its reviewer approved and no landing has closed
    /// ([`crate::orchestration::unlanded`]) is assessed as open work is: it reads done, and its
    /// land is still to make, of a head an assessor may not have read yet.
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
        let named = |field: &Option<String>| {
            (field.as_deref())
                .map(str::trim)
                .filter(|named| !named.is_empty())
                .map(str::to_string)
        };
        let (tip, import) = (named(&needs.tip), named(&needs.import));
        if let Some(import) = &import {
            if tip.is_none() {
                return Err(refused(
                    "an import is named at the head the assessment read (--tip TIP): it binds \
                     that head to the upstream commit it imports"
                        .into(),
                ));
            }
            if import.len() != 40 || !import.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(refused(format!(
                    "an import names its upstream commit whole, not {import:?}"
                )));
            }
        }
        let written = Needs {
            stamps,
            tip,
            by: handle.into(),
            why: why.into(),
            import,
        };
        let value = serde_json::to_value(&written).expect("needs are JSON");
        let held: Held = self.ever_held(slug)?;
        let defaults = catalogue.defaults();
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            let unlanded =
                crate::orchestration::marked_unlanded(&task.status, &task.fields).is_some();
            if task.status == "cancelled" || (task.status == "done" && !unlanded) {
                return Err(refused(format!(
                    "{slug} is {}: only an open task, or one approved and not landed, is assessed",
                    task.status
                )));
            }
            let orchestrates = view.holds(handle, "orchestrator");
            if let Some(why) = held.unfresh(handle, &task.created_by, orchestrates) {
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
