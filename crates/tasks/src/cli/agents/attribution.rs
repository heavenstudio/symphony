use super::{MESSAGE_CHARS, Pane};
use crate::rows::TaskSessionRow;
use std::path::{Path, PathBuf};

/// `path` with its symbolic links resolved, as Python's `os.path.realpath`: what does not exist is kept
/// as written beneath the part that does. "" stays "".
pub(super) fn resolve(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let given = Path::new(path);
    let whole = if given.is_absolute() {
        given.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(given)
    };
    let mut rest = Vec::new();
    let mut at: &Path = &whole;
    loop {
        if let Ok(found) = at.canonicalize() {
            let mut out: PathBuf = found;
            out.extend(rest.iter().rev());
            return out.to_string_lossy().into_owned();
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                at = parent;
            }
            _ => return whole.to_string_lossy().into_owned(),
        }
    }
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
                .strip_prefix("codex:")
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
            || record.is_some_and(|r| !r.claude_session.trim_start_matches("codex:").is_empty());
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
    use super::{owned_pane, resolve};

    #[test]
    fn unknown_host_panes_need_an_explicit_owner_without_a_conflicting_conversation() {
        for (bound, conversation, name, matched) in [
            (None, None, "worker", true),
            (Some("bound"), None, "worker", true),
            (Some("bound"), Some("other"), "worker", false),
            (Some("bound"), Some("bound"), "other", true),
            (Some("bound"), None, "other", false),
        ] {
            let panes = [Pane {
                id: "same-pane".into(),
                agent: None,
                status: None,
                session: conversation.map(str::to_string),
                cwd: "/same/path".into(),
                title: None,
                name: Some(name.into()),
            }];
            assert_eq!(
                owned_pane(&panes, None, bound, "worker").is_some(),
                matched,
                "bound={bound:?}, conversation={conversation:?}, name={name}"
            );
        }
    }

    #[test]
    fn a_path_resolves_as_python_s_realpath_resolves_it() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(resolve(""), "");
        assert_eq!(
            resolve("not/there"),
            here.join("not/there").to_string_lossy()
        );
        assert_eq!(resolve("/no-such-top/x"), "/no-such-top/x");
        assert_eq!(resolve("/"), "/");
    }
}
