//! A board's word on a visitor who is not the person at the machine ([`crate::gate::Roster`]),
//! from what the machine and the board keep (docs/product/analysis/users-and-identity.md, "Where
//! people are kept"): the machine's profile whose sign-in is the visitor's login
//! ([`symphony_board::people::Profiles::signed_in`]), and that board's row of the person, who is
//! on it while they hold a power there ([`symphony_board::people::People::powers_of`]). They then
//! write as their short name, shown as their profile shows them, their git handles theirs, a
//! person the board named ([`symphony_board::workflow::person`]).
//!
//! It is read on each request a visitor sends, so a person added, removed or given a power is
//! seen at their next. A tagged node, a login no profile signs in, a profile no row of the board
//! names or names with no power, and a board or a profile that cannot be read are none: on that
//! board the visitor is nobody, and its project is one the server does not hold. A failed read is
//! said in the server's log by the board's folder alone, never by the visitor's login.

use crate::gate::{Roster, Visitor};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_board::people::{Profiles, TAILSCALE};
use symphony_plugin::{Actor, Sender};

/// The roster of the people the machine folder `machine` keeps the profiles of.
pub(crate) fn of(machine: PathBuf) -> Roster {
    Arc::new(move |root: &Path, visitor: &Visitor| {
        member(&machine, root, visitor, |root| {
            symphony::taskview::connect(root)?
                .people()
                .map_err(|e| e.to_string())
        })
    })
}

/// The machine folder this process's environment names, whose roster [`of`] is: none where it
/// names none, said in the server's log, and then no visitor but the person at the machine is on
/// any board.
pub(crate) fn this_machine() -> Option<PathBuf> {
    symphony_board::project::machine_folder(
        |name| std::env::var_os(name),
        cfg!(target_os = "macos"),
    )
    .inspect_err(|why| {
        eprintln!("· no visitor but the person at the machine is on any board: {why}");
    })
    .ok()
}

/// `visitor` on the board at `root`, its people read by `people`, as the actor their writes there
/// take: none where it has not added them.
fn member(
    machine: &Path,
    root: &Path,
    visitor: &Visitor,
    people: impl FnOnce(&Path) -> Result<symphony_board::people::People, String>,
) -> Option<Actor> {
    let Visitor::Login(login) = visitor else {
        return None;
    };
    let profiles = match Profiles::read(machine) {
        Ok(profiles) => profiles,
        Err(why) => {
            eprintln!(
                "· the machine's profiles were not read, so no visitor is on any board: {why}"
            );
            return None;
        }
    };
    let profile = profiles.signed_in(TAILSCALE, &login.name)?;
    let held = match people(root) {
        Ok(people) => people.powers_of(&profile.name),
        Err(why) => {
            eprintln!(
                "· the people of the board at {} were not read, so {} is not on it: {why}",
                root.display(),
                profile.name
            );
            return None;
        }
    };
    held.any().then(|| {
        let sender = Sender::new(&profile.name, profile.shown(), profile.git.clone());
        Actor::new(sender, symphony_board::workflow::person(&profile.name))
    })
}

#[cfg(test)]
#[path = "roster_tests.rs"]
mod tests;
