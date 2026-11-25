//! Whose a card or a task is, as a page shows it to the one reading (docs/workflow/TASKS.md,
//! "Which group a thing belongs to"), and a card handed on. The board decides it
//! ([`symphony_board::people::ownership`]): a name written on the thing, a card's `*For:*`
//! ([`symphony_docs::syntax::card_for`]) or a task's `_board.group`, where the board holds a group
//! or a person by it; else the group its place maps to; else the board's default. This module
//! says that answer on the wire ([`symphony_api::Owner`]), with whom the reader may hand it to.
//!
//! On a board that names nobody nothing is said: every thing is the person at the machine's, and
//! the page draws as it did before there were groups. What waits on one reader of them, of a
//! page's queue, board and pending list, is [`theirs`].

use std::collections::BTreeMap;
use symphony_api::{CardOwners, Owner};
use symphony_board::people::People;
use symphony_board::people::ownership::{self, By, Kind, Map, Owning, Place, Thing};
use symphony_docs::syntax::card_for;

use crate::register::{Decision, State};

pub mod theirs;

/// `owning` as the wire says it to `actor` (`machine`: the person at the machine), `open` whether
/// the thing still waits on someone: whom they may hand it to, the board's groups, then its
/// people, then `owner`, never the name it is written to now; none where it is closed or they may
/// not ([`ownership::of_owner`]); and whether they may decide it ([`ownership::may_decide`]), so a
/// page draws its deciding controls for them alone.
pub fn owner(people: &People, owning: &Owning, actor: &str, machine: bool, open: bool) -> Owner {
    let hand_to = if open && ownership::of_owner(people, owning, actor, machine) {
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
        decides: ownership::may_decide(people, owning, actor, machine),
    }
}

/// The short name of the person a board named who asks as `actor`: the principal the board issues
/// them ([`symphony_board::workflow::person`]), as the machine's roster admits every visitor it
/// names and as the board grants a workflow's edges ([`symphony_board::Store::host_on`]). None for
/// the person at the machine, whatever principal their gate gives them, never told by a name.
pub fn person(actor: &symphony_plugin::Actor) -> Option<&str> {
    (actor.principal.issuer == symphony_board::workflow::PEOPLE).then(|| actor.name())
}

/// Why `actor` may not decide the thing in `places` (the deciding first) with `written` the name
/// written on it, on the board `store` ([`ownership::refused_decide`]): none for the person at
/// the machine, who is never asked and for whom nothing is read, none on a board that names
/// nobody, and none for one of its owner.
fn refused(
    store: &symphony_board::Store,
    actor: &symphony_plugin::Actor,
    written: Option<&str>,
    places: &[Place],
) -> Result<Option<String>, symphony_board::Error> {
    let Some(name) = person(actor) else {
        return Ok(None);
    };
    let people = store.people()?;
    if people.members.is_empty() {
        return Ok(None);
    }
    let owning = ownership::owning(&people, &store.ownership_map()?, written, places);
    Ok(ownership::refused_decide(
        &people, &owning, places, name, false,
    ))
}

/// Why `actor` may not decide the task `task` (move, prioritize, change a field of, cancel or
/// approve it): [`refused`].
pub fn refused_task(
    store: &symphony_board::Store,
    task: &symphony_board::Task,
    actor: &symphony_plugin::Actor,
) -> Result<Option<String>, symphony_board::Error> {
    let places = Thing::Task(task.slug.clone()).places();
    refused(store, actor, task.group(), &places)
}

/// Why `actor` may not answer the card `card` of the document of route `rel`: [`refused`].
pub fn refused_card(
    store: &symphony_board::Store,
    rel: &str,
    card: &Decision,
    actor: &symphony_plugin::Actor,
) -> Result<Option<String>, symphony_board::Error> {
    let places = thing(rel, card).places();
    refused(store, actor, card_for(&card.text), &places)
}

/// Why `actor` may not change what the Settings page changes: [`refused`] of its place.
pub fn refused_settings(
    store: &symphony_board::Store,
    actor: &symphony_plugin::Actor,
) -> Result<Option<String>, symphony_board::Error> {
    refused(store, actor, None, &[Place::Settings])
}

/// Whether the reader `reader`, a person a board named (none: the person at the machine), may
/// change what the Settings page changes on the board `store`: the person at the machine always,
/// and on a board that names nobody; else while they hold decide and are of the page's owner
/// ([`ownership::may_decide`]). Not where the board cannot be read.
pub fn changes_settings(store: &symphony_board::Store, reader: Option<&str>) -> bool {
    let Some(name) = reader else {
        return true;
    };
    let read = || -> Result<bool, symphony_board::Error> {
        let people = store.people()?;
        if people.members.is_empty() {
            return Ok(true);
        }
        let map = store.ownership_map()?;
        let owning = ownership::owning(&people, &map, None, &[Place::Settings]);
        Ok(ownership::may_decide(&people, &owning, name, false))
    };
    read().unwrap_or(false)
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
    let places = Thing::Task(task.slug.clone()).places();
    let owning = ownership::owning(&people, &store.ownership_map()?, task.group(), &places);
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

/// Whose each of the waiting cards `cards` is, in their order, on `people` and `map`: each the
/// review queue's document (`docs/…/X.md`, or `@scope/docs/…/X.md` for a branch's proposal) and its
/// slug, owned as the file a verdict on it reads holds the card ([`card_owning`]), a name written
/// on it (`*For:*`) included: a branch's proposal as its worktree's file holds it, as its page and
/// a verdict there read it, and any other card, or a proposal its worktree does not hold, as main's
/// file holds it, with the saves still waiting on their delivery merged onto it. A card neither
/// holds (a document gone, a branch whose worktree went) by its places alone, a name written on it
/// unread. The worktrees are listed once, where a proposal waits, and each document is read once.
/// What a notice of the card tells ([`ownership::told`]) and what waits on each reader
/// ([`theirs`]).
pub fn waiting(
    people: &People,
    map: &Map,
    root: &std::path::Path,
    cards: &[(String, String)],
) -> Vec<Owning> {
    let mut read: BTreeMap<(String, String), Vec<Decision>> = BTreeMap::new();
    let mut trees: Option<Vec<crate::branches::Tree>> = None;
    let mut owners = Vec::new();
    for (doc, slug) in cards {
        let (scope, route) = split_route(doc);
        let mut held = |scope: &str| -> Option<Owning> {
            let decisions = read
                .entry((scope.to_string(), route.to_string()))
                .or_insert_with(|| {
                    if scope.is_empty() {
                        return decisions_on_main(root, route);
                    }
                    let trees = trees.get_or_insert_with(|| crate::branches::trees(root));
                    (trees.iter().find(|tree| tree.id() == scope))
                        .map(|tree| decisions_in(&tree.path, route))
                        .unwrap_or_default()
                });
            (decisions.iter())
                .find(|card| card.slug == *slug)
                .map(|card| card_owning(people, map, route, card))
        };
        let proposed = (!scope.is_empty()).then(|| held(scope)).flatten();
        let owning = proposed.or_else(|| held("")).unwrap_or_else(|| {
            let thing = Thing::Card {
                slug: slug.clone(),
                route: route.to_string(),
            };
            ownership::owning(people, map, None, &thing.places())
        });
        owners.push(owning);
    }
    owners
}

/// The cards of the document of route `route` in main's checkout at `root`, with the saves still
/// waiting on their delivery merged onto its file as its page shows it; none where it is no page.
fn decisions_on_main(root: &std::path::Path, route: &str) -> Vec<Decision> {
    let Some(text) = page_text(root, route) else {
        return Vec::new();
    };
    let file = crate::project::Project::of(root).file(route);
    let text = crate::commit::pending::shown(root, &file, text);
    crate::register::parse_decisions(&crate::text::splitlines(&text))
}

/// The cards of the document of route `route` as the checkout at `root` (a branch's worktree)
/// holds its file; none where it is no page there.
fn decisions_in(root: &std::path::Path, route: &str) -> Vec<Decision> {
    let Some(text) = page_text(root, route) else {
        return Vec::new();
    };
    crate::register::parse_decisions(&crate::text::splitlines(&text))
}

/// The text of the file the route `route` names in the checkout at `root`, as a page reads it.
fn page_text(root: &std::path::Path, route: &str) -> Option<String> {
    let path = crate::docview::target(root, route, true).ok()?;
    std::fs::read_to_string(path).ok()
}

/// The queue's document `doc` as the branch it is on and its route: a branch proposal's `@scope/`
/// taken off and named, and main's (no scope) for any other.
fn split_route(doc: &str) -> (&str, &str) {
    let doc = doc.trim_start_matches('/');
    match doc.split_once('/') {
        Some((scope, rest)) if scope.starts_with('@') => (&scope[1..], rest),
        _ => ("", doc),
    }
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
