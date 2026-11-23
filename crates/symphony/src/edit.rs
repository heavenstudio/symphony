//! The Symphony's writes into a document: a callout for a comment, a mark and a dated line for a
//! verdict, a dated line for a message or an agent's reply. Pure functions over text, each returning
//! the whole new text; nothing lives only in the app.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::clock::stamp_minutes;
use crate::register::{self, KINDS, State, Who, card_end, card_line, remark, sort_register};
use crate::text::{NS, S, is_space, lstrip, repr, rstrip, splitlines, strip};

/// The edit could not be applied as asked; the file is left untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError(pub String);

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

/// A tap sent again within this many minutes of the same verdict is a repeat, not a new decision.
pub const REPEAT_MINUTES: i64 = 5;

/// The verdicts the owner gives, in the order a refusal lists them. `resolved` (a tidy) and
/// `auto-approved` (the triage tool) are set by agents, never by the owner.
const OWNER_STATES: [State; 5] = [
    State::Pending,
    State::Approved,
    State::Rejected,
    State::Question,
    State::Dismissed,
];

/// A verdict line as the app writes it: `  [YYYY-MM-DD HH:MM] WHO · STATE[ — NOTE]`, WHO the
/// short name of who gave it (`owner` for the person at the machine).
static VERDICT_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^  \[(\d{{4}}-\d{{2}}-\d{{2}} \d{{2}}:\d{{2}})\] ({NS}+) · ({NS}+)(?: — (.*))?$"
    ))
    .expect("the entry")
});
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?P<hashes>#{{1,6}}){S}+(?P<title>.+?){S}*#*{S}*$"
    ))
    .expect("the heading")
});

/// The note's lines, stripped, and between them one empty line where the owner left blank ones: a
/// single Enter is a line break and a blank line a paragraph gap. No blank line opens or ends it.
fn written(note: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = Vec::new();
    for line in splitlines(note).into_iter().map(strip) {
        if !line.is_empty() || lines.last().is_some_and(|last| !last.is_empty()) {
            lines.push(line);
        }
    }
    if lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    lines
}

/// The note's lines ([`written`]), refused when one outside its code opens a review callout: the
/// page's comment reader would take it for a comment on the document, and archiving that comment
/// would cut it out of the card. Refused too when its code is not closed, or opens on its first
/// line, which the file holds on the entry's own line: the file's readers would take the rest of
/// the file, later comments among it, for code.
fn note_lines(note: &str) -> Result<Vec<&str>, EditError> {
    let lines = written(note);
    if lines.first().is_some_and(|first| register::is_fence(first)) {
        return Err(EditError(
            "a note cannot open with a code fence: its first line is the entry's own, so write a line of words before the code".into(),
        ));
    }
    let mut fenced = false;
    for line in &lines {
        if register::is_fence(line) {
            fenced = !fenced;
        } else if !fenced && crate::callouts::review_kind(line).is_some() {
            return Err(EditError(format!(
                "a note cannot hold a review callout ({}): the page would read it as a comment of its own",
                repr(line)
            )));
        }
    }
    if fenced {
        return Err(EditError(
            "a note's code fence is not closed: end the code with its own ``` (or ~~~) line, or the page would read the rest of the file as code".into(),
        ));
    }
    Ok(lines)
}

/// A note's later line as the file holds it, under its entry: indented to continue it, a paragraph
/// gap left empty.
fn continued(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!("    {line}")
    }
}

/// The index of the line that opens the card `slug`.
fn find_card<L: AsRef<str>>(lines: &[L], slug: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| card_line(line.as_ref()).is_some_and(|card| card.slug == slug))
}

fn no_card(slug: &str) -> EditError {
    EditError(format!("no decision with slug {}", repr(slug)))
}

/// One typed message as a callout: its first non-blank line is the title, the rest the body.
pub fn split_message(message: &str) -> Result<(String, String), EditError> {
    let lines = splitlines(strip(message));
    let Some(first) = lines.first() else {
        return Err(EditError("the comment is empty".to_string()));
    };
    Ok((
        strip(first).to_string(),
        lines[1..].join("\n").trim_matches('\n').to_string(),
    ))
}

/// A callout of `kind` after line `after_line` (1-based), blank lines around it. Refused inside a
/// decision register (after its start marker, or after any line before its end marker): a register
/// holds cards only, and a callout there would leave it text that is not a card, so every later
/// verdict on its cards would be refused ([`register::sort_register`]). Refused too between the
/// lines of a card that sits inline in its document, which it would cut from the card; after the
/// card's last line it is a comment on the document like any other.
pub fn insert_comment(
    text: &str,
    after_line: i64,
    kind: &str,
    title: &str,
    body: &str,
) -> Result<String, EditError> {
    if !KINDS.contains(&kind) {
        return Err(EditError(format!(
            "unknown comment kind {}; use one of {}",
            repr(kind),
            KINDS.join(", ")
        )));
    }
    let lines = splitlines(text);
    let Some(at) = usize::try_from(after_line)
        .ok()
        .filter(|at| (1..=lines.len()).contains(at))
    else {
        return Err(EditError(format!(
            "line {after_line} is outside the document ({} lines)",
            lines.len()
        )));
    };
    if let Some((start, end)) = register::regions(&lines)
        .into_iter()
        .find(|(start, end)| (*start..*end).contains(&(at - 1)))
    {
        return Err(EditError(format!(
            "line {at} is inside the decision register (lines {} to {}), which holds cards only: a comment there would stop every later verdict on its cards. Say it on the card itself (its Verdict / message), or comment on a block outside the register",
            start + 1,
            end + 1
        )));
    }
    // The line commented on and the one after it are one card's: the callout would part them.
    let carded = register::carded(&lines);
    let within = carded[at - 1]
        && lines
            .get(at)
            .is_some_and(|next| carded[at] && card_line(next).is_none());
    if within {
        let first = (0..at)
            .rev()
            .find(|index| card_line(lines[*index]).is_some())
            .unwrap_or(at - 1);
        let slug = card_line(lines[first]).map_or("", |card| card.slug);
        return Err(EditError(format!(
            "line {at} is inside the card {} (lines {} to {}): a comment there would cut the lines under it from the card. Say it on the card itself (its Verdict / message), or comment on a block outside the card",
            repr(slug),
            first + 1,
            card_end(&lines, first)
        )));
    }
    let title = format!("> [!{kind}] {}", strip(title));
    let mut callout = vec![title];
    callout.extend(
        splitlines(body)
            .into_iter()
            .filter(|line| !strip(line).is_empty())
            .map(|line| format!("> {}", rstrip(line))),
    );
    let (head, tail) = lines.split_at(at);
    let mut out: Vec<String> = head.iter().map(|line| line.to_string()).collect();
    out.push(String::new());
    out.extend(callout);
    if tail.first().is_some_and(|line| !strip(line).is_empty()) {
        out.push(String::new());
    }
    out.extend(tail.iter().map(|line| line.to_string()));
    Ok(out.join("\n") + "\n")
}

/// Set a card's verdict and append a dated owner entry; regroup the register if it has one, as the
/// project's card kinds group its cards.
pub fn set_decision(
    text: &str,
    slug: &str,
    state: &str,
    note: &str,
    now: &str,
    plugins: &crate::plugins::Plugins,
) -> Result<String, EditError> {
    set_decision_by(text, slug, state, note, now, plugins, Who::Owner.as_str())
}

/// [`set_decision`] given by `by`, the short name of who asked (`symphony_plugin::Actor`), which
/// the entry carries in the owner's place: `owner` for the person at the machine, whose entry reads
/// as [`set_decision`]'s. The record's readers know `owner` and `agent` alone until a board names
/// people (docs/product/analysis/users-and-identity.md, "How a person's name is written").
pub fn set_decision_by(
    text: &str,
    slug: &str,
    state: &str,
    note: &str,
    now: &str,
    plugins: &crate::plugins::Plugins,
    by: &str,
) -> Result<String, EditError> {
    let Some(state) = State::parse(state).filter(|s| OWNER_STATES.contains(s)) else {
        let owner: Vec<&str> = OWNER_STATES.iter().map(|s| s.as_str()).collect();
        return Err(EditError(format!(
            "unknown decision state {}; use one of {}",
            repr(state),
            owner.join(", ")
        )));
    };
    let lines = splitlines(text);
    let index = find_card(&lines, slug).ok_or_else(|| no_card(slug))?;
    let end = card_end(&lines, index);
    let note = note_lines(note)?;
    let mut entry = format!("  [{now}] {by} · {}", state.as_str());
    if let Some(first) = note.first() {
        entry.push_str(&format!(" — {first}"));
    }
    let mut out: Vec<String> = lines[..index].iter().map(|line| line.to_string()).collect();
    out.push(remark(lines[index], state.mark()));
    out.extend(lines[index + 1..end].iter().map(|line| line.to_string()));
    out.push(entry);
    out.extend(note.iter().skip(1).map(|line| continued(line)));
    out.extend(lines[end..].iter().map(|line| line.to_string()));
    sort_register(&(out.join("\n") + "\n"), plugins).map_err(|problem| EditError(problem.0))
}

fn unreadable_time(stamp: &str) -> EditError {
    EditError(format!(
        "time data {} does not match format '%Y-%m-%d %H:%M'",
        repr(stamp)
    ))
}

/// Whether the card's newest lines are already this owner verdict, same state and note, written
/// within [`REPEAT_MINUTES`] of `now`: a tap sent again because its answer was lost. An error when a
/// time does not read as `%Y-%m-%d %H:%M`.
pub fn is_repeat_decision(
    text: &str,
    slug: &str,
    state: &str,
    note: &str,
    now: &str,
) -> Result<bool, EditError> {
    is_repeat_decision_by(text, slug, state, note, now, Who::Owner.as_str())
}

/// [`is_repeat_decision`] of a verdict given by `by` ([`set_decision_by`]): one that another gave
/// with the same words is no repeat of it.
pub fn is_repeat_decision_by(
    text: &str,
    slug: &str,
    state: &str,
    note: &str,
    now: &str,
    by: &str,
) -> Result<bool, EditError> {
    let lines = splitlines(text);
    let note = written(note);
    let tail: Vec<String> = note.iter().skip(1).map(|line| continued(line)).collect();
    let Some(index) = find_card(&lines, slug) else {
        return Ok(false);
    };
    let end = card_end(&lines, index);
    let Some(start) = end
        .checked_sub(1 + tail.len())
        .filter(|start| *start > index)
    else {
        return Ok(false);
    };
    let Some(entry) = VERDICT_ENTRY.captures(lines[start]) else {
        return Ok(false);
    };
    let said = entry.get(4).map_or("", |m| m.as_str());
    if &entry[2] != by
        || &entry[3] != state
        || said != note.first().copied().unwrap_or("")
        || lines[start + 1..end] != tail[..]
    {
        return Ok(false);
    }
    let now_at = stamp_minutes(now).ok_or_else(|| unreadable_time(now))?;
    let then = stamp_minutes(&entry[1]).ok_or_else(|| unreadable_time(&entry[1]))?;
    Ok((0..=REPEAT_MINUTES).contains(&(now_at - then)))
}

/// Append a chat line to a card, the owner's or an agent's (no verdict, or an agent's `question`);
/// its mark stays as it is.
pub fn add_message(
    text: &str,
    slug: &str,
    message: &str,
    now: &str,
    who: Who,
    verdict: Option<&str>,
) -> Result<String, EditError> {
    add_message_by(text, slug, message, now, who.as_str(), verdict)
}

/// [`add_message`] said by `by`: `agent`, or the short name of the person who asked, which the line
/// carries in the owner's place, as [`set_decision_by`]'s entry does.
pub fn add_message_by(
    text: &str,
    slug: &str,
    message: &str,
    now: &str,
    by: &str,
    verdict: Option<&str>,
) -> Result<String, EditError> {
    let message = note_lines(message)?;
    let Some(first) = message.first() else {
        return Err(EditError("a message needs some text".to_string()));
    };
    let lines = splitlines(text);
    let index = find_card(&lines, slug).ok_or_else(|| no_card(slug))?;
    let end = card_end(&lines, index);
    let verdict = verdict
        .filter(|v| !v.is_empty())
        .map_or(String::new(), |v| format!(" · {v}"));
    let mut out: Vec<String> = lines[..end].iter().map(|line| line.to_string()).collect();
    out.push(format!("  [{now}] {by}{verdict} — {first}"));
    out.extend(message.iter().skip(1).map(|line| continued(line)));
    out.extend(lines[end..].iter().map(|line| line.to_string()));
    Ok(out.join("\n") + "\n")
}

/// Resolve the open comment whose callout opens line `line` (1-based) of `text` with `answer`, on
/// one line nested under its quoted lines (`> > [!resolved] answer`): the text, and the comment's
/// title. Refused for a line that opens no comment, a comment already resolved, a like, which
/// waits on no one, and an empty answer.
pub fn resolve_comment(
    text: &str,
    line: usize,
    answer: &str,
) -> Result<(String, String), EditError> {
    let answer = answer.split_whitespace().collect::<Vec<_>>().join(" ");
    if answer.is_empty() {
        return Err(EditError("an answer needs some text".to_string()));
    }
    let lines = splitlines(text);
    let comments = crate::callouts::parse_comments(&lines);
    let Some(comment) = comments.iter().find(|c| c.line == line) else {
        let open: Vec<String> = comments
            .iter()
            .filter(|c| c.open())
            .map(|c| format!("L{}", c.line))
            .collect();
        let named = if open.is_empty() {
            "it has no open comment".to_string()
        } else {
            format!("its open comments open on {}", open.join(", "))
        };
        return Err(EditError(format!(
            "line {line} opens no comment: name one by the line its `> [!kind]` callout is on ({named})"
        )));
    };
    if comment.kind == "like" {
        return Err(EditError(format!(
            "the comment on line {line} is a like: it waits on no answer"
        )));
    }
    if comment.resolution.is_some() {
        return Err(EditError(format!(
            "the comment on line {line} is already resolved"
        )));
    }
    let mut end = line;
    while end < lines.len()
        && register::QUOTE.is_match(lines[end])
        && !register::CALLOUT.is_match(lines[end])
    {
        end += 1;
    }
    let open = lines[line - 1];
    let indent = &open[..open.len() - lstrip(open).len()];
    let mut out: Vec<String> = lines[..end].iter().map(|line| line.to_string()).collect();
    out.push(format!("{indent}> > [!resolved] {answer}"));
    out.extend(lines[end..].iter().map(|line| line.to_string()));
    Ok((out.join("\n") + "\n", comment.title.clone()))
}

/// A post only adds: refuse, naming the card, when `after` drops a line `before` had under any card.
pub fn check_nothing_lost(before: &str, after: &str, slug: &str) -> Result<(), EditError> {
    register::check_nothing_lost(before, after, &format!("posting on {}", repr(slug)))
        .map_err(|problem| EditError(problem.0))
}

/// Index of the closing `---` when the lines open with frontmatter.
pub fn frontmatter_end<L: AsRef<str>>(lines: &[L]) -> Option<usize> {
    if lines
        .first()
        .is_none_or(|first| strip(first.as_ref()) != "---")
    {
        return None;
    }
    lines
        .iter()
        .skip(1)
        .position(|line| strip(line.as_ref()) == "---")
        .map(|at| at + 1)
}

/// The id Python-Markdown's toc gives a heading (its `slugify` with `-`), so a check needs no
/// renderer: accents folded (NFKD, then ASCII only), punctuation dropped, spaces and hyphens one `-`.
pub fn heading_anchor(heading: &str) -> String {
    let kept: String = heading
        .nfkd()
        .filter(|c| {
            c.is_ascii() && (c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || is_space(*c))
        })
        .collect();
    let mut anchor = String::with_capacity(kept.len());
    let mut joining = false;
    for c in strip(&kept).to_ascii_lowercase().chars() {
        if c == '-' || is_space(c) {
            if !joining {
                anchor.push('-');
            }
            joining = true;
        } else {
            anchor.push(c);
            joining = false;
        }
    }
    anchor
}

/// The markdown under the heading whose anchor is `anchor`, to the next heading of the same or a
/// higher level; the whole body when `anchor` is empty; `None` when no heading has it.
pub fn section_text(text: &str, anchor: &str) -> Option<String> {
    let lines = splitlines(text);
    let body = &lines[frontmatter_end(&lines).map_or(0, |end| end + 1)..];
    let joined = |part: &[&str]| part.join("\n").trim_matches('\n').to_string() + "\n";
    if anchor.is_empty() {
        return Some(joined(body));
    }
    let (mut start, mut level, mut fenced) = (None, 0, false);
    for (index, line) in body.iter().enumerate() {
        if lstrip(line).starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let Some(heading) = HEADING.captures(line).filter(|_| !fenced) else {
            continue;
        };
        let depth = heading["hashes"].len();
        match start {
            None if heading_anchor(&heading["title"]) == anchor => {
                start = Some(index);
                level = depth;
            }
            Some(from) if depth <= level => return Some(joined(&body[from..index])),
            _ => {}
        }
    }
    start.map(|from| joined(&body[from..]))
}
