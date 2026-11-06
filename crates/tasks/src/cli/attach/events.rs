//! The event lines for what changed between two looks ([`diff`]): ROLE, STATUS, NEW, and STALE,
//! LOST or LIVENESS UNKNOWN for a quiet child, each said once.

use super::Seen;
use crate::health::{Health, LIVE_SECONDS};
use crate::orchestration::{moved, told};
use crate::{Error, Message, Store};
use std::collections::BTreeSet;

fn latest_status_action<'a>(messages: &'a [Message], slug: &str) -> Option<&'a Message> {
    messages.iter().rev().find(|m| {
        m.slug == slug
            && (m.kind == "verdict"
                || (m.kind == "system"
                    && (m.text.starts_with("created by ")
                        || m.text.contains(" moved it ")
                        || m.text.contains(" claimed it")
                        || m.text.contains(" released "))))
    })
}

/// The lines for what changed between two looks.
pub fn diff(
    store: &Store,
    before: &Seen,
    now: &Seen,
    handle: &str,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    let through = now.latest_message.unwrap_or_default();
    let since = before.latest_message.unwrap_or(through);
    // The tasks this session's orchestrator role came to or left since the last look.
    let orchestrating = |seen: &Seen| -> BTreeSet<String> {
        (seen.roles.iter())
            .filter(|(_, who, role)| who == handle && role == "orchestrator")
            .map(|(slug, _, _)| slug.clone())
            .collect()
    };
    let flipped: BTreeSet<String> = orchestrating(before)
        .symmetric_difference(&orchestrating(now))
        .cloned()
        .collect();
    let needs_actions = before.statuses != now.statuses
        || !flipped.is_empty()
        || (now.ready.keys()).any(|slug| !before.ready.contains_key(slug));
    let messages = if needs_actions && since < through {
        store.messages_after(since, through)?
    } else {
        Vec::new()
    };
    let mut announced = BTreeSet::new();
    let moves = role_moves(store, now, handle, &flipped, &messages, emit)?;
    announced.extend(moves.iter().map(|(slug, _)| *slug));
    for (slug, who) in role_changes(before, now, handle, &moves) {
        let roles: Vec<&str> = (now.roles.iter())
            .filter(|(s, h, _)| s == slug && h == who)
            .map(|(_, _, role)| role.as_str())
            .collect();
        let roles = if roles.is_empty() {
            "none".to_string()
        } else {
            roles.join(", ")
        };
        let status = &now.statuses[slug];
        emit(&format!("ROLE {slug} {who} {roles}, {status}"));
        announced.insert(slug);
    }
    for (slug, status) in &now.statuses {
        let decided = |m: &Message| {
            m.sender == handle
                || (m.kind == "verdict"
                    && (now.verdict_tasks.contains(slug)
                        || (m.sender == "owner"
                            && (now.roles.iter()).any(|(s, who, _)| s == slug && who == handle))))
        };
        // Work nobody orchestrates is said only as it blocks; its digest rows say the rest.
        let heard = !now.unclaimed.contains(slug) || status == "blocked";
        if let Some(old) = before.statuses.get(slug)
            && old != status
            && heard
            && !announced.contains(slug.as_str())
            && !latest_status_action(&messages, slug).is_some_and(decided)
        {
            emit(&format!("STATUS {slug} {old} → {status}"));
        }
    }
    for (slug, title) in &now.ready {
        if !before.ready.contains_key(slug)
            && !latest_status_action(&messages, slug).is_some_and(|m| m.sender == handle)
            && !announced.contains(slug.as_str())
        {
            emit(&format!("NEW {slug} ready — {title}"));
        }
    }
    quiet_children(store, before, now, handle, emit)
}

/// A move of the orchestrator role: the session it left and the one it reached each hear it once,
/// from the line the move wrote on the thread. Returns each (slug, session) a move named.
fn role_moves<'m>(
    store: &Store,
    now: &Seen,
    handle: &str,
    flipped: &BTreeSet<String>,
    messages: &'m [Message],
    emit: &mut dyn FnMut(&str),
) -> Result<BTreeSet<(&'m str, &'m str)>, Error> {
    let mut parties: BTreeSet<(&str, &str)> = BTreeSet::new();
    let on_flipped = |m: &&Message| m.kind == "system" && flipped.contains(&m.slug);
    let lines = messages.iter().filter(on_flipped);
    for message in lines {
        let Some((from, to)) = moved(&message.text) else {
            continue;
        };
        if to != handle && !from.contains(&handle) {
            continue;
        }
        let status = match now.statuses.get(&message.slug) {
            Some(status) => status.clone(),
            None => store.get(&message.slug)?.status,
        };
        let line = format!("{to} orchestrator, {status} (from {})", from.join(", "));
        emit(&format!("ROLE {} {line}", message.slug));
        parties.extend(
            from.iter()
                .chain([&to])
                .map(|who| (message.slug.as_str(), *who)),
        );
    }
    Ok(parties)
}

/// The (slug, session) pairs whose roles changed on a task this session still holds a role on, but
/// for its own, those revealed by its joining, and those a role move already said.
fn role_changes<'s>(
    before: &'s Seen,
    now: &'s Seen,
    handle: &str,
    moves: &BTreeSet<(&str, &str)>,
) -> BTreeSet<(&'s str, &'s str)> {
    // Joining a task reveals its existing participants. Those roles are context, not new actions
    // for the joining recipient (and must not echo its own join back through the other roles).
    let held_before = |slug: &str| {
        (before.roles.iter()).any(|(old_slug, old_who, _)| old_slug == slug && old_who == handle)
    };
    let joined: BTreeSet<&str> = (now.roles.iter())
        .filter(|(slug, who, _)| who == handle && !held_before(slug))
        .map(|(slug, _, _)| slug.as_str())
        .collect();
    let mut changed: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (slug, who, _) in now.roles.symmetric_difference(&before.roles) {
        if who != handle
            && now.roles.iter().any(|(s, _, _)| s == slug)
            && !joined.contains(slug.as_str())
            && !moves.contains(&(slug.as_str(), who.as_str()))
        {
            changed.insert((slug, who));
        }
    }
    changed
}

/// STALE, LOST or LIVENESS UNKNOWN for each child gone quiet since the last look that holds work
/// this session is told of; another orchestrator's tasks stay that one's.
fn quiet_children(
    store: &Store,
    before: &Seen,
    now: &Seen,
    handle: &str,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    let confirmed = store.confirmed_callbacks()?;
    let mut board = None;
    for (child, &health) in &now.children {
        if confirmed.contains(child)
            || before.children.get(child) == Some(&health)
            || matches!(health, Health::Live | Health::Left)
        {
            continue;
        }
        if board.is_none() {
            board = Some(store.board()?);
        }
        let board = board.as_ref().expect("read just above");
        let held = told(board, child, handle, store.claimable(child)?);
        if held.is_empty() {
            continue; // a child holding nothing another could take ends quietly
        }
        let line = match health {
            Health::Unknown => format!(
                "LIVENESS UNKNOWN {child} — native presence sweep overdue or failed; work remains held"
            ),
            Health::Stale => format!("STALE {child} — no sign of it for {LIVE_SECONDS} s"),
            _ => format!("LOST {child} — its work is claimable: {}", held.join(", ")),
        };
        emit(&line);
    }
    Ok(())
}
