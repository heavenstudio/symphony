//! Which statuses a move goes from, decided against the status its own write reads: any the
//! pipeline allows, open work alone for a hold, or the statuses a caller names, so a move decided on
//! an earlier read leaves a status another move set since as it is.

use crate::slots::OPEN;
use crate::store::{Error, Store, Task, refused};

/// What a move ([`Store::moving`]) may go from.
#[derive(Clone, Copy)]
pub(super) enum Moving<'a> {
    /// Any status the pipeline goes from to the move's own ([`super::can_transition`]).
    Any,
    /// Open work alone, to blocked, keeping its slot ([`Store::hold_blocked`]).
    Hold,
    /// One of these statuses alone ([`Store::move_from`]).
    From(&'a [&'a str]),
}

impl Moving<'_> {
    /// Whether the move holds its task's slot ([`Store::hold_blocked`]).
    pub(super) fn holds(self) -> bool {
        matches!(self, Moving::Hold)
    }

    /// Refused when `slug`, at `current`, is not where this move goes from.
    pub(super) fn allows(self, slug: &str, current: &str) -> Result<(), Error> {
        match self {
            Moving::Hold if !OPEN.contains(&current) => Err(refused(format!(
                "{slug} is {current}: only work in progress or in review is held"
            ))),
            Moving::From(from) if !from.contains(&current) => Err(refused(format!(
                "{slug} is {current}, not {}: it stays {current}",
                from.join(" or ")
            ))),
            _ => Ok(()),
        }
    }
}

impl Store {
    /// [`Store::move_to`] by a session, only from a status in `from`, as the move's own write reads
    /// it. A caller that decided the move on an earlier read (a changes verdict sending work in
    /// review back to in progress) so leaves a status another move set since (blocked, say) as it
    /// is, where `move_to` would take the task from there too.
    pub fn move_from(
        &self,
        slug: &str,
        from: &[&str],
        status: &str,
        handle: &str,
    ) -> Result<Task, Error> {
        self.moving(slug, status, handle, Moving::From(from))
    }
}
