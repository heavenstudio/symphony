//! The event lines for what changed between two looks ([`diff`]): ROLE, GIVEN, STATUS, NEW, and
//! STALE, LOST or LIVENESS UNKNOWN for a quiet child, each said once. A recipient never hears its own
//! writes, nor the roles its own children (sessions it registered, `parent`) take or let go, nor
//! the move a child's claim or release makes: it staffed them. Nor is the owner's move, or a
//! person's, said as a STATUS: their line itself reaches its inbox, as a `MOVE` (`Store::inbox`),
//! read here as the board marks it ([`MessageRow::by_person`]).

use super::Seen;
use crate::health::{Health, LIVE_SECONDS};
use crate::orchestration::{given, given_event, moved, moved_for, told};
use crate::rows::MessageRow;
use crate::stages::{CLAIMED, CREATED, MOVED, RELEASED};
use crate::{Board, Error, Store};
use std::collections::BTreeSet;

fn latest_status_action<'a>(messages: &'a [MessageRow], slug: &str) -> Option<&'a MessageRow> {
    messages.iter().rev().find(|m| {
        m.slug == slug
            && (m.kind == "verdict"
                || (m.kind == "system"
                    && (m.text.starts_with(CREATED)
                        || m.text.contains(MOVED)
                        || m.text.contains(CLAIMED)
                        || m.text.contains(RELEASED))))
    })
}

/// The lines for what changed between two looks, read against the board as it is now.
pub fn diff(
    store: &Store,
    before: &Seen,
    now: &Seen,
    handle: &str,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    diff_on(store, &store.board()?, before, now, handle, emit)
}

/// [`diff`] against `board`, the board `now` was taken from.
pub fn diff_on(
    store: &Store,
    board: &Board,
    before: &Seen,
    now: &Seen,
    handle: &str,
    emit: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    // The sessions it registered: what they take on and let go is its own staffing.
    let mut children: BTreeSet<&str> = BTreeSet::new();
    children.extend(
        (board.sessions.iter())
            .filter(|s| s.row.parent.as_deref() == Some(handle))
            .map(|s| s.row.handle.as_str()),
    );
    let through = now.latest_message.unwrap_or_default();
    let since = before.latest_message.unwrap_or(through);
    // The tasks this session's orchestrator role came to or left since the last look.
    let orchestrating = |seen: &Seen| -> BTreeSet<String> {
        let mut slugs = BTreeSet::new();
        slugs.extend(
            (seen.roles.iter())
                .filter(|(_, who, role)| who == handle && role == "orchestrator")
                .map(|(slug, _, _)| slug.clone()),
        );
        slugs
    };
    let mut flipped: BTreeSet<String> = BTreeSet::new();
    flipped.extend(
        orchestrating(before)
            .symmetric_difference(&orchestrating(now))
            .cloned(),
    );
    let needs_actions = before.statuses != now.statuses
        || !flipped.is_empty()
        || (now.ready.keys()).any(|slug| !before.ready.contains_key(slug));
    let messages = if needs_actions && since < through {
        store.messages_after_rows(since, through)?
    } else {
        Vec::new()
    };
    let mut announced = BTreeSet::new();
    let moves = role_moves(store, now, handle, &flipped, &messages, emit)?;
    announced.extend(moves.iter().map(|(slug, _)| *slug));
    for (slug, who) in role_changes(before, now, handle, &moves) {
        if children.contains(who) {
            continue;
        }
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
    // A child's claim or release: its own staffing, whose move and readiness are not said. Only
    // the move the claim or release made is its own: a claim starts ready work, a release of the
    // last executor makes work in progress ready again. A move between the looks that ends
    // anywhere else (a block the child made before it let the work go) is said.
    let staffing = |m: &MessageRow, old: Option<&String>, status: &str| {
        let made = match old.map(String::as_str) {
            Some("ready") => status == "in_progress" && m.text.contains(CLAIMED),
            Some("in_progress") => status == "ready" && m.text.contains(RELEASED),
            _ => false,
        };
        made && m.kind == "system" && children.contains(m.sender.as_str())
    };
    for (slug, status) in &now.statuses {
        let old = before.statuses.get(slug);
        // Its own move, a child's staffing, and a verdict it hears anyway are not said, nor the
        // owner's move or a person's: its inbox carries that line to whoever would hear the move
        // here (`Store::inbox`).
        let decided = |m: &MessageRow| {
            m.sender == handle
                || (m.by_person()
                    && m.kind == "system"
                    && (m.text.strip_prefix(m.sender.as_str()))
                        .is_some_and(|rest| rest.starts_with(MOVED)))
                || staffing(m, old, status)
                || (m.kind == "verdict"
                    && (now.verdict_tasks.contains(slug)
                        || (m.by_person()
                            && (now.roles.iter()).any(|(s, who, _)| s == slug && who == handle))))
        };
        // Work nobody orchestrates is said only as it blocks; its digest rows say the rest.
        if let Some(old) = old
            && old != status
            && now.hears_move(slug, status)
            && !announced.contains(slug.as_str())
            && !latest_status_action(&messages, slug).is_some_and(decided)
        {
            emit(&format!("STATUS {slug} {old} → {status}"));
        }
    }
    for (slug, title) in &now.ready {
        let old = before.statuses.get(slug);
        if !before.ready.contains_key(slug)
            && !latest_status_action(&messages, slug)
                .is_some_and(|m| m.sender == handle || staffing(m, old, "ready"))
            && !announced.contains(slug.as_str())
        {
            emit(&format!("NEW {slug} ready — {title}"));
        }
    }
    quiet_children(store, before, now, handle, emit)
}

/// A move of the orchestrator role: the session it left and the one it reached each hear it once,
/// from the line the move wrote on the thread, but for the one that wrote it. The tasks the board
/// gave this session ([`given`]) are said in one GIVEN line, however many there are, not one ROLE
/// line each. Returns each (slug, session) a move named, whose move says the task.
fn role_moves<'m>(
    store: &Store,
    now: &Seen,
    handle: &str,
    flipped: &BTreeSet<String>,
    messages: &'m [MessageRow],
    emit: &mut dyn FnMut(&str),
) -> Result<BTreeSet<(&'m str, &'m str)>, Error> {
    let mut parties: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut gave: Vec<&str> = Vec::new();
    let on_flipped = |m: &&MessageRow| m.kind == "system" && flipped.contains(&m.slug);
    let lines = messages.iter().filter(on_flipped);
    for message in lines {
        let Some((from, to)) = moved(&message.text) else {
            continue;
        };
        if !moved_for(&from, to, handle) {
            continue;
        }
        // The session that moved it, handing it on or taking it over, made the move: it is not
        // told its own write, nor told it again as a role change.
        if message.sender != handle && to == handle && given(&message.text).is_some() {
            if !gave.contains(&message.slug.as_str()) {
                gave.push(&message.slug);
            }
        } else if message.sender != handle {
            let status = match now.statuses.get(&message.slug) {
                Some(status) => status.clone(),
                None => store.get(&message.slug)?.status,
            };
            let line = format!("{to} orchestrator, {status} (from {})", from.join(", "));
            emit(&format!("ROLE {} {line}", message.slug));
        }
        parties.extend(
            from.iter()
                .chain([&to])
                .map(|who| (message.slug.as_str(), *who)),
        );
    }
    if let Some(line) = given_event(&gave) {
        emit(&line);
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
    let mut joined: BTreeSet<&str> = BTreeSet::new();
    joined.extend(
        (now.roles.iter())
            .filter(|(slug, who, _)| who == handle && !held_before(slug))
            .map(|(slug, _, _)| slug.as_str()),
    );
    let mut changed: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (slug, who, _) in now.roles.symmetric_difference(&before.roles) {
        if who != handle
            && now.hears_roles(slug)
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
    // The confirmed registrations are read only for a child that went quiet since the last look:
    // every look whose board moved diffs, and reading every registration for each recipient of
    // each write is what made many writes at once wait on the database.
    let mut confirmed = None;
    let mut board = None;
    for (child, &health) in &now.children {
        if before.children.get(child) == Some(&health)
            || matches!(health, Health::Live | Health::Left)
        {
            continue;
        }
        if confirmed.is_none() {
            confirmed = Some(store.confirmed_callbacks()?);
        }
        if (confirmed.as_ref().expect("read just above")).contains(child) {
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
