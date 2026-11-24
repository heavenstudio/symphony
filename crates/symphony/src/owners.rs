//! Whose a card or a task is, as a page shows it to the one reading (docs/workflow/TASKS.md,
//! "Which group a thing belongs to"), and a card handed on. The board decides it
//! ([`symphony_board::people::ownership`]): a name written on the thing, a card's `*For:*`
//! ([`symphony_docs::syntax::card_for`]) or a task's `_board.group`, where the board holds a group
//! or a person by it; else the group its place maps to; else the board's default. This module
//! says that answer on the wire ([`symphony_api::Owner`]), with whom the reader may hand it to.
//!
//! On a board that names nobody nothing is said: every thing is the person at the machine's, and
//! the page draws as it did before there were groups.

use std::collections::BTreeMap;
use symphony_api::{CardOwners, Owner};
use symphony_board::people::People;
use symphony_board::people::ownership::{self, By, Kind, Map, Owning, Thing};
use symphony_docs::syntax::card_for;

use crate::register::{Decision, State};

/// `owning` as the wire says it to `actor` (`machine`: the person at the machine), `open` whether
/// the thing still waits on someone: whom they may hand it to, the board's groups, then its
/// people, then `owner`, never the name it is written to now; none where it is closed or they may
/// not ([`ownership::may_hand`]).
pub fn owner(people: &People, owning: &Owning, actor: &str, machine: bool, open: bool) -> Owner {
    let hand_to = if open && ownership::may_hand(people, owning, actor, machine) {
        (people.groups.iter().map(|group| group.name.as_str()))
            .chain(people.members.iter().map(|member| member.name.as_str()))
            .chain([ownership::MACHINE])
            .filter(|name| *name != owning.name)
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    Owner {
        name: owning.name.clone(),
        kind: match owning.kind {
            Kind::Group => "group",
            Kind::Person => "person",
            Kind::Machine => "machine",
        }
        .into(),
        by: match owning.by {
            By::Named => "named",
            By::Map => "map",
            By::Default => "default",
        }
        .into(),
        said: owning.said(),
        hand_to,
    }
}

/// Whose the task `task` is, said to `actor` (`machine`: the person at the machine): none on a
/// board that names nobody.
pub fn task(
    store: &symphony_board::Store,
    task: &symphony_board::Task,
    actor: &str,
    machine: bool,
) -> Result<Option<Owner>, symphony_board::Error> {
    let people = store.people()?;
    if people.members.is_empty() {
        return Ok(None);
    }
    let owning = store.task_owning(task)?;
    let open = !matches!(task.status.as_str(), "done" | "cancelled");
    Ok(Some(owner(&people, &owning, actor, machine, open)))
}

/// The owners of the cards `cards` of the document of route `rel`, by slug, said to `actor`
/// (`machine`: the person at the machine): none on a board that names nobody.
pub fn cards(
    store: &symphony_board::Store,
    rel: &str,
    cards: &[Decision],
    actor: &str,
    machine: bool,
) -> Result<CardOwners, symphony_board::Error> {
    let people = store.people()?;
    if people.members.is_empty() {
        return Ok(CardOwners::default());
    }
    let map = store.ownership_map()?;
    Ok(CardOwners {
        cards: owners_of(&people, &map, rel, cards, actor, machine),
    })
}

/// The owners of the cards of the document of route `rel` in the checkout at `root`, by slug, as
/// its file holds them, and on the project's own checkout with the saves still waiting on their
/// delivery merged onto it as its page shows it ([`crate::commit::pending::shown`]): [`cards`].
/// Refused as a document's read refuses a route no page is ([`crate::docview::target`]).
pub fn document(
    store: &symphony_board::Store,
    root: &std::path::Path,
    rel: &str,
    actor: &str,
    machine: bool,
) -> Result<CardOwners, String> {
    let rel = rel.trim_start_matches('/');
    let path = crate::docview::target(root, rel, true)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    let file = crate::project::Project::of(root).file(rel);
    let text = crate::commit::pending::shown(root, &file, text);
    let decisions = crate::register::parse_decisions(&crate::text::splitlines(&text));
    cards(store, rel, &decisions, actor, machine).map_err(|e| e.to_string())
}

/// [`cards`] on `people` and `map` as read.
fn owners_of(
    people: &People,
    map: &Map,
    rel: &str,
    cards: &[Decision],
    actor: &str,
    machine: bool,
) -> BTreeMap<String, Owner> {
    (cards.iter())
        .map(|card| {
            let owning = card_owning(people, map, rel, card);
            let open = waits(card.state);
            let said = owner(people, &owning, actor, machine, open);
            (card.slug.clone(), said)
        })
        .collect()
}

/// Whose the card `card` of the document of route `rel` is, on `people` and `map`.
pub fn card_owning(people: &People, map: &Map, rel: &str, card: &Decision) -> Owning {
    ownership::owning(
        people,
        map,
        card_for(&card.text),
        &thing(rel, card).places(),
    )
}

/// The card `card` of the document of route `rel`, as the board's log names it.
pub fn thing(rel: &str, card: &Decision) -> Thing {
    Thing::Card {
        slug: card.slug.clone(),
        route: rel.trim_start_matches('/').to_string(),
    }
}

/// Whether a card in `state` still waits on someone's answer, so is handed on: a pending card, or
/// a question.
pub fn waits(state: State) -> bool {
    matches!(state, State::Pending | State::Question)
}

#[cfg(test)]
#[path = "owners_tests.rs"]
mod tests;
