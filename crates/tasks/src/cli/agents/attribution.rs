//! Match observed pane ownership without treating paths, terminal ids or host labels as identity.

use super::{MESSAGE_CHARS, Pane, Row};
use crate::rows::TaskSessionRow;
use std::collections::HashMap;

/// The same positive, unique checkout ownership gates legacy runs and normalized reservations.
/// Unobserved local registrations still count as possible sharers; a host label never proves an owner.
pub(super) fn fallback_owners<'board>(
    rows: &[Row<'board>],
    local_host: &str,
    real: &dyn Fn(&str) -> String,
) -> HashMap<String, &'board str> {
    let mut sharing: HashMap<String, usize> = HashMap::new();
    for row in rows {
        if row.record.is_none_or(|record| record.closed.is_none())
            && (row.pane.is_some() || !local_host.is_empty() && row.session.row.host == local_host)
        {
            *sharing.entry(real(&row.worktree)).or_default() += 1;
        }
    }
    let mut owners = HashMap::new();
    for row in rows {
        if row.pane.is_none() || row.record.is_some_and(|record| record.closed.is_some()) {
            continue;
        }
        let checkout = real(&row.worktree);
        if !checkout.is_empty() && sharing.get(&checkout) == Some(&1) {
            owners.insert(checkout, row.session.row.handle.as_str());
        }
    }
    owners
}

/// The pane that is this agent's: its recorded one, else one running its conversation, else one
/// named for its handle.
pub(crate) fn find_pane<'p>(
    panes: &'p [Pane],
    record: Option<&TaskSessionRow>,
    bound: Option<&str>,
    handle: &str,
) -> Option<&'p Pane> {
    if let Some(record) = record
        && let Some(pane) = panes.iter().rev().find(|p| p.id == record.pane)
    {
        return Some(pane);
    }
    if let Some(pane) = conversation_pane(panes, record, bound) {
        return Some(pane);
    }
    panes
        .iter()
        .find(|p| p.title.as_deref() == Some(handle) || p.name.as_deref() == Some(handle))
}

/// A bound conversation observed in a local pane proves locality even when an older
/// registration omitted its host. A pane id or checkout alone is insufficient.
pub(super) fn conversation_pane<'p>(
    panes: &'p [Pane],
    record: Option<&TaskSessionRow>,
    bound: Option<&str>,
) -> Option<&'p Pane> {
    let wanted: Vec<&str> = [
        record.map(|r| {
            r.claude_session
                .strip_prefix(crate::launch::CODEX)
                .unwrap_or(&r.claude_session)
        }),
        bound,
    ]
    .into_iter()
    .flatten()
    .filter(|w| !w.is_empty())
    .collect();
    panes
        .iter()
        .find(|p| p.session.as_deref().is_some_and(|s| wanted.contains(&s)))
}

/// Compact hc listings can omit argv. An explicit board-handle label is ownership evidence,
/// while a bare terminal id/path is not. A contradictory observed conversation refuses the tag.
pub(super) fn owned_pane<'p>(
    panes: &'p [Pane],
    record: Option<&TaskSessionRow>,
    bound: Option<&str>,
    handle: &str,
) -> Option<&'p Pane> {
    conversation_pane(panes, record, bound).or_else(|| {
        let expected = bound.is_some_and(|id| !id.is_empty())
            || record.is_some_and(|r| {
                !r.claude_session
                    .trim_start_matches(crate::launch::CODEX)
                    .is_empty()
            });
        panes.iter().find(|p| {
            (!expected || p.session.is_none())
                && (p.title.as_deref() == Some(handle) || p.name.as_deref() == Some(handle))
        })
    })
}

/// Its words on one line, its whitespace as Python's `str.split` finds it, cut to [`MESSAGE_CHARS`].
pub(super) fn cut(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.chars().count() <= MESSAGE_CHARS {
        return line;
    }
    let mut short: String = line.chars().take(MESSAGE_CHARS).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::Pane;
    use super::owned_pane;

    #[test]
    fn unknown_host_panes_need_an_explicit_owner_without_a_conflicting_conversation() {
        for (bound, conversation, name, matched) in [
            (None, None, "worker", true),
            (Some("bound"), None, "worker", true),
            (Some("bound"), Some("other"), "worker", false),
            (Some("bound"), Some("bound"), "other", true),
            (Some("bound"), None, "other", false),
        ] {
            let terminal = serde_json::json!({
                "address": "same-pane",
                "cwd": "/same/path",
                "name": name,
                "argv": conversation.map(|id| vec!["claude", "--session-id", id]).unwrap_or_default(),
            });
            let panes = [Pane::from_hc(terminal.as_object().unwrap()).unwrap()];
            assert_eq!(
                owned_pane(&panes, None, bound, "worker").is_some(),
                matched,
                "bound={bound:?}, conversation={conversation:?}, name={name}"
            );
        }
    }
}
