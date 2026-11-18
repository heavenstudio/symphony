//! The pictures a text shows, in every form the renderer draws one: `![alt](path)`, with a title
//! (`"…"`, `'…'` or `(…)`) or the path in angle brackets; a reference `![alt][label]`, `![alt][]`
//! or `![alt]` to a definition `[label]: path`; and raw `<img src="path">`. Code spans and fenced
//! blocks show none. A picture this server shows by its own URL (`http://127.0.0.1:8765/<id>/…`)
//! names the route after it ([`local_route`]).

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::register::is_fence;

/// A picture a text shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Picture {
    /// The line it is on, from 0.
    pub line: usize,
    /// Its path as written (for a reference, its definition's).
    pub target: String,
    /// For a reference, the line of the definition it takes its path from, from 0, in the text the
    /// definitions were read from.
    pub defined: Option<usize>,
}

/// The reference definitions `[label]: path` of a text, by their label as CommonMark matches it
/// (case and runs of space folded): each one's path and line, the first of a label winning.
pub(super) type Definitions = HashMap<String, (String, usize)>;

/// A title after a path, in any of its three quotings.
const TITLE: &str = r#"(?:\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?"#;

static INLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"!\[[^\]]*\]\(\s*(?:<(?P<angled>[^>]*)>|(?P<bare>[^\s)]+)){TITLE}\s*\)"
    ))
    .expect("an inline picture pattern")
});
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"!\[(?P<alt>[^\]]*)\](?:\[(?P<label>[^\]]*)\])?").expect("a reference pattern")
});
static DEFINITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^\s*(?:>\s*)*\[(?P<label>[^\]]+)\]:\s*(?:<(?P<angled>[^>]*)>|(?P<bare>\S+)){TITLE}\s*$"
    ))
    .expect("a definition pattern")
});
static IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)<img\b[^>]*?\ssrc\s*=\s*(?:"(?P<dq>[^"]*)"|'(?P<sq>[^']*)'|(?P<uq>[^\s>"']+))"#,
    )
    .expect("an img pattern")
});
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]*`").expect("a code span"));
static LOCAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://(?:127\.0\.0\.1|localhost|\[::1\])(?::\d+)?(?P<path>/[^?#]*)?")
        .expect("a local URL pattern")
});

/// `label` as CommonMark matches it: case and runs of space folded.
fn folded(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Each line of `text` outside fenced blocks (numbered as [`crate::text::splitlines`] splits them,
/// as the cards are), with its code spans blanked to the same length.
fn prose(text: &str) -> Vec<(usize, String)> {
    let mut fenced = false;
    let mut found = Vec::new();
    for (index, line) in crate::text::splitlines(text).into_iter().enumerate() {
        if is_fence(line) {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            found.push((
                index,
                CODE.replace_all(line, |c: &regex::Captures<'_>| " ".repeat(c[0].len()))
                    .into_owned(),
            ));
        }
    }
    found
}

/// The reference definitions of `text`.
pub(super) fn definitions(text: &str) -> Definitions {
    let mut found = Definitions::new();
    for (index, line) in prose(text) {
        if let Some(caps) = DEFINITION.captures(&line) {
            let target = caps
                .name("angled")
                .or(caps.name("bare"))
                .map_or("", |m| m.as_str());
            found
                .entry(folded(&caps["label"]))
                .or_insert_with(|| (target.to_string(), index));
        }
    }
    found
}

/// The pictures `text` shows, in the order they stand, a reference read against `definitions`
/// (one naming no definition is plain text, and shows nothing).
pub(super) fn pictures(text: &str, definitions: &Definitions) -> Vec<Picture> {
    let mut found = Vec::new();
    for (index, line) in prose(text) {
        if DEFINITION.is_match(&line) {
            continue;
        }
        let mut here: Vec<(usize, Picture)> = Vec::new();
        let mut rest = line.clone();
        for caps in INLINE.captures_iter(&line) {
            let whole = caps.get(0).expect("a match");
            let target = caps
                .name("angled")
                .or(caps.name("bare"))
                .map_or("", |m| m.as_str());
            here.push((
                whole.start(),
                Picture {
                    line: index,
                    target: target.to_string(),
                    defined: None,
                },
            ));
            rest.replace_range(whole.range(), &" ".repeat(whole.len()));
        }
        for caps in IMG.captures_iter(&line) {
            let whole = caps.get(0).expect("a match");
            let target = ["dq", "sq", "uq"]
                .iter()
                .find_map(|name| caps.name(name))
                .map_or("", |m| m.as_str());
            here.push((
                whole.start(),
                Picture {
                    line: index,
                    target: target.to_string(),
                    defined: None,
                },
            ));
            rest.replace_range(whole.range(), &" ".repeat(whole.len()));
        }
        for caps in REFERENCE.captures_iter(&rest) {
            let whole = caps.get(0).expect("a match");
            let label = caps
                .name("label")
                .map(|m| m.as_str())
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(&caps["alt"]);
            if let Some((target, at)) = definitions.get(&folded(label)) {
                here.push((
                    whole.start(),
                    Picture {
                        line: index,
                        target: target.clone(),
                        defined: Some(*at),
                    },
                ));
            }
        }
        symphony_board::order::sort_by_key(&mut here, &|(start, _)| *start);
        found.extend(here.into_iter().map(|(_, picture)| picture));
    }
    found
}

/// What `target` names when it is this server's own URL (`http://127.0.0.1`, `localhost` or `[::1]`,
/// any port) for the project `id`: none when it is not such a URL; `Some(None)` when it is, but
/// names no file of this project (another project's, or a page of the Symphony's own); else the
/// route from the root, `/…`. The shared server serves a project under `/<id>/`; a server of the
/// project's own serves it at `/`, where the route counts only where `serves` says it names a
/// file of the project.
pub(super) fn local_route(
    target: &str,
    id: &str,
    serves: impl Fn(&str) -> bool,
) -> Option<Option<String>> {
    let caps = LOCAL.captures(target)?;
    let path = caps.name("path").map_or("/", |m| m.as_str());
    let own = format!("/{id}");
    if let Some(rest) = path
        .strip_prefix(&own)
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
    {
        return Some(Some(if rest.is_empty() {
            "/".into()
        } else {
            rest.to_string()
        }));
    }
    Some(serves(path.trim_start_matches('/')).then(|| path.to_string()))
}

#[cfg(test)]
#[path = "pictures_tests.rs"]
mod tests;
