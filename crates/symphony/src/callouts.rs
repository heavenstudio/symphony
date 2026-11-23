//! Review callouts: `> [!dislike] Too many lamps`, open until an agent nests `> > [!resolved] …`.

use crate::register::{CALLOUT, KINDS, NESTED_RESOLVED, QUOTE, is_fence};
use crate::text::{rstrip, strip};

/// One callout a person left in a document's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// 1-based.
    pub line: usize,
    pub kind: String,
    pub title: String,
    /// Who left it: a person's short name written before the title (`> [!dislike] ana · Too many
    /// lamps`, [`crate::edit::insert_comment_by`]), else `owner`, as every comment written before
    /// names was.
    pub by: String,
    pub body: Vec<String>,
    pub resolution: Option<String>,
    /// The callout as written, for the ledger (set by [`archive_callouts`]).
    pub raw: Vec<String>,
}

impl Comment {
    pub fn open(&self) -> bool {
        self.kind != "like" && self.resolution.is_none()
    }
}

/// The review kind a callout line opens, lowercased, if it opens one.
pub(crate) fn review_kind(line: &str) -> Option<(String, String)> {
    let caps = CALLOUT.captures(line)?;
    let kind = caps["kind"].to_ascii_lowercase();
    KINDS
        .contains(&kind.as_str())
        .then(|| (kind, strip(&caps["title"]).to_string()))
}

/// A callout's title read for who left it: `ana · Too many lamps` is `ana`'s, its name a short
/// name ([`symphony_docs::syntax::is_short_name`]) followed by ` · `; any other title is the
/// owner's, whole.
fn named(title: &str) -> (String, String) {
    match title.split_once(" · ") {
        Some((name, rest)) if symphony_docs::syntax::is_short_name(name) => {
            (name.to_string(), strip(rest).to_string())
        }
        _ => ("owner".to_string(), title.to_string()),
    }
}

/// Every review callout in `lines`, with its body and any nested resolution.
pub fn parse_comments<L: AsRef<str>>(lines: &[L]) -> Vec<Comment> {
    let mut found: Vec<Comment> = Vec::new();
    let mut current = false;
    let mut in_fence = false;
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if is_fence(line) {
            in_fence = !in_fence;
            current = false;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((kind, title)) = review_kind(line) {
            let (by, title) = named(&title);
            found.push(Comment {
                line: index + 1,
                kind,
                title,
                by,
                body: Vec::new(),
                resolution: None,
                raw: Vec::new(),
            });
            current = true;
            continue;
        }
        if !current {
            continue;
        }
        let Some(quote) = QUOTE.captures(line) else {
            current = false;
            continue;
        };
        let rest = &quote["rest"];
        let comment = found.last_mut().expect("the current callout");
        if let Some(resolved) = NESTED_RESOLVED.captures(rest) {
            comment.resolution = Some(strip(&resolved["text"]).to_string());
        } else if CALLOUT.is_match(line) {
            current = false; // a different callout type ends this comment
        } else {
            comment.body.push(rstrip(rest).to_string());
        }
    }
    found
}

/// Take every answered callout out of `text`: the text and what was taken, as written.
pub fn archive_callouts(text: &str) -> (String, Vec<Comment>) {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let comments = parse_comments(&lines);
    let mut taken: Vec<Comment> = Vec::new();
    for mut comment in comments.into_iter().rev() {
        if comment.resolution.is_none() {
            continue;
        }
        let mut start = comment.line - 1;
        let mut end = start + 1;
        while end < lines.len() && QUOTE.is_match(lines[end]) {
            end += 1;
        }
        comment.raw = lines[start..end]
            .iter()
            .map(|line| line.to_string())
            .collect();
        if end < lines.len() && strip(lines[end]).is_empty() && end + 1 < lines.len() {
            end += 1; // the blank line that separated it from what follows
        } else if start > 0 && strip(lines[start - 1]).is_empty() {
            start -= 1;
        }
        lines.drain(start..end);
        taken.push(comment);
    }
    taken.reverse();
    (lines.join("\n"), taken)
}

/// One ledger entry: where and when, then the callout word for word.
pub fn ledger_entry(doc_path: &str, comment: &Comment, stamp: &str) -> String {
    format!("## {stamp} · {doc_path}\n\n{}\n", comment.raw.join("\n"))
}
