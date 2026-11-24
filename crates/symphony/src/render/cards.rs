//! A comment callout and a decision card as HTML: the shapes the owner's verdicts are written in.

use std::sync::LazyLock;

use regex::Regex;

use crate::markup::escape;
use crate::register::{CALLOUT, Decision, Entry, Group, QUOTE, RESOLVED, Who, parse_decisions};
use crate::text::splitlines;

use super::{Block, Ctx};

static IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"!\[(?P<alt>[^\]]*)\]\((?P<src>[^)\s]+)\)").expect("the image pattern")
});
static LETTERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" [A-Z]$").expect("the letter pattern"));
static LINKED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|]*\|)?([^\]]*)\]\]").expect("the wikilink pattern"));

/// `html` with its one paragraph's tags dropped, when it is nothing else: a line of a thread, not a block.
pub(super) fn single_paragraph(html: String) -> String {
    if html.starts_with("<p>") && html.ends_with("</p>") && html.matches("<p>").count() == 1 {
        html[3..html.len() - 4].to_string()
    } else {
        html
    }
}

/// One callout block (already stripped of its leading `> `) to HTML.
pub(super) fn callout(lines: &[String], ctx: &Ctx<'_>) -> String {
    let opening = format!("> {}", lines[0]);
    let first = CALLOUT.captures(&opening).expect("a callout line");
    let kind = first["kind"].to_lowercase();
    let title = first["title"].trim().to_string();
    let (mut body, mut nested): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for line in &lines[1..] {
        match QUOTE.captures(line) {
            Some(quote) => {
                let rest = quote.name("rest").map_or("", |m| m.as_str());
                let rest = &line[line.len() - rest.len()..];
                if CALLOUT.is_match(line) || !nested.is_empty() {
                    nested.push(rest);
                } else {
                    body.push(rest);
                }
            }
            None => body.push(line),
        }
    }
    let opens_with_callout = |lines: &[&str]| {
        lines.first().and_then(|l| {
            CALLOUT
                .captures(&format!("> {l}"))
                .map(|c| c["kind"].to_lowercase())
        })
    };
    // An answered comment folds away: the title stays as a summary, the rest opens on click.
    let done = opens_with_callout(&nested).as_deref() == Some(RESOLVED);
    let (tag, head) = if done {
        ("details", "summary")
    } else {
        ("div", "p")
    };
    let mut out = vec![format!(
        "<{tag} class=\"callout callout-{kind}{}\">",
        if done { " callout-done" } else { "" }
    )];
    out.push(format!(
        "<{head} class=\"callout-title\"><span class=\"callout-kind\">{kind}</span> {}</{head}>",
        escape(&title)
    ));
    if !body.is_empty() {
        out.push(ctx.convert(&body.join("\n")));
    }
    if opens_with_callout(&nested).is_some() {
        let nested: Vec<String> = nested.iter().map(|l| (*l).to_string()).collect();
        out.push(callout(&nested, ctx));
    }
    out.push(format!("</{tag}>"));
    out.join("\n")
}

/// A note under a card, a person's or an agent's, drawn as a document's body is, so it can link,
/// show pictures and files, and use components; its line breaks stay and no line is a heading. A
/// person's line that carries a short name, not `owner`, names them after its time
/// (`<span class="entry-by">`), as the line reads in the file; its class stays `owner`, a person's.
fn entry_html(entry: &Entry, ctx: &Ctx<'_>) -> String {
    let body = single_paragraph(ctx.convert_note(&entry.text));
    let mut parts = Vec::new();
    if let Some(ts) = &entry.ts {
        parts.push(format!("<time>{}</time>", escape(ts)));
    }
    if entry.who == crate::register::Who::Owner && entry.name != entry.who.as_str() {
        parts.push(format!(
            "<span class=\"{BY_CLASS}\">{}</span>",
            escape(&entry.name)
        ));
    }
    if let Some(verdict) = entry.verdict.as_deref().filter(|v| !v.is_empty()) {
        parts.push(format!("<b>{}</b>", escape(verdict)));
    }
    if !entry.text.is_empty() {
        parts.push(format!("<span class=\"entry-text\">{body}</span>"));
    }
    format!(
        "<li class=\"entry {}\">{}</li>",
        entry.who.as_str(),
        parts.join(" ")
    )
}

/// The class of the name a person's line carries under a card ([`entry_html`]), which a thread
/// entry's first block keeps (`crate::blocks`) and the client draws before its verdict.
pub(crate) const BY_CLASS: &str = "entry-by";

/// Owner words as literal text, each line that names an upload drawn as the picture or link it is
/// ([`crate::uploads::owner_html`]), through the same Markdown path as an agent's picture.
pub(super) fn owner_html(text: &str, ctx: &Ctx<'_>) -> String {
    crate::uploads::owner_html(text, |line| single_paragraph(ctx.convert(line)))
}

pub(super) fn history_items(card: &Decision, ctx: &Ctx<'_>) -> String {
    card.entries()
        .iter()
        .map(|entry| entry_html(entry, ctx))
        .collect()
}

/// A picture of a card's compare row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picture {
    /// What the row calls it: `Base`, a candidate's letter, or nothing when its alt ends in none.
    pub label: String,
    /// Its alt text as the source wrote it, trimmed.
    pub name: String,
    pub src: String,
}

impl Picture {
    /// What its caption says, at place `n` (from 1) of its row: its label, else its source's
    /// name, else a brief `Picture n` for one the source named in no way.
    fn caption(&self, n: usize) -> String {
        [&self.label, &self.name]
            .into_iter()
            .find(|text| !text.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Picture {n}"))
    }

    /// Its accessible name, which the viewer captions it with too: the source's name, so a
    /// lettered candidate says what it pictures; the base keeps `Base`, which its style reads.
    fn alt(&self, n: usize) -> String {
        if self.label == BASE || self.name.is_empty() {
            self.caption(n)
        } else {
            self.name.clone()
        }
    }
}

const BASE: &str = "Base";

type Pictures = Vec<Picture>;

/// The pictures a card compares: the base a card was drawn from (an agent line starting "Base"),
/// and the candidates of the first agent line holding pictures, each lettered by its alt text
/// (no label when the alt does not end in a letter) and named by the whole of it.
pub(crate) fn compare_pictures(card: &Decision) -> (Pictures, Pictures) {
    let (mut base, mut candidates): (Pictures, Pictures) = (Vec::new(), Vec::new());
    for entry in card.entries().iter().filter(|e| e.who == Who::Agent) {
        let found: Vec<_> = IMAGE.captures_iter(&entry.text).collect();
        let Some(first) = found.first() else {
            continue;
        };
        if entry.text.starts_with(BASE) {
            if base.is_empty() {
                base = vec![Picture {
                    label: BASE.to_string(),
                    name: first["alt"].trim().to_string(),
                    src: first["src"].to_string(),
                }];
            }
        } else if candidates.is_empty() {
            candidates = found
                .iter()
                .map(|f| {
                    let alt = f["alt"].trim();
                    let label = if LETTERED.is_match(alt) {
                        alt.rsplit(' ').next().unwrap_or("")
                    } else {
                        ""
                    };
                    Picture {
                        label: label.to_string(),
                        name: alt.to_string(),
                        src: f["src"].to_string(),
                    }
                })
                .collect();
        }
    }
    (base, candidates)
}

/// One labelled row of the pictures to choose between, an `.html` mockup as media. Empty when there is
/// nothing to compare. Every picture is named and captioned ([`Picture::alt`], [`Picture::caption`]),
/// so a reader, by eye or by a screen reader, tells one from the next.
fn compare_html(card: &Decision, ctx: &Ctx<'_>) -> String {
    let (base, candidates) = compare_pictures(card);
    let row: Vec<_> = base.into_iter().chain(candidates).collect();
    if row.len() < 2 {
        return String::new();
    }
    let figures: String = row
        .iter()
        .zip(1..)
        .map(|(picture, n)| {
            format!(
                "<figure><img src=\"{}\" alt=\"{}\" loading=\"lazy\"><figcaption>{}</figcaption></figure>",
                escape(&picture.src),
                escape(&picture.alt(n)),
                escape(&picture.caption(n))
            )
        })
        .collect();
    format!(
        "<div class=\"compare\">{}</div>",
        super::media::embed_mockups(&figures, ctx)
    )
}

/// A card's text as one plain line: markdown emphasis and link brackets dropped, long text cut.
fn summary(text: &str) -> String {
    const LIMIT: usize = 140;
    let plain = LINKED
        .replace_all(text, "$2")
        .replace("**", "")
        .replace('*', "");
    if plain.chars().count() <= LIMIT {
        plain
    } else {
        let cut: String = plain.chars().take(LIMIT - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Every card of the block, each with its text, its compare row and its thread.
pub(super) fn decisions(block: &Block, ctx: &Ctx<'_>) -> String {
    let mut out: Vec<String> = Vec::new();
    for card in parse_decisions(&splitlines(&block.text)) {
        let thread = card.thread(ctx.plugins);
        let inline = ctx.convert(&card.text);
        let history = if ctx.detached_threads {
            String::new()
        } else {
            history_items(&card, ctx)
        };
        let posted = thread.posted_at.as_deref().map_or_else(String::new, |at| {
            format!("<time class=\"posted\">{}</time> ", escape(at))
        });
        let line_attr = if ctx.detached_threads {
            format!("data-line-offset=\"{}\"", card.line - 1)
        } else {
            format!("data-line=\"{}\"", block.start + card.line - 1)
        };
        let one = format!(
            "<div class=\"decision\" id=\"{slug}\" data-slug=\"{slug}\" data-state=\"{state}\" {line_attr} data-open=\"{open}\" data-group=\"{group}\" data-decided=\"{decided}\"><div class=\"decision-text\"><code>{slug}</code> {posted}{inline}</div>{compare}<ul class=\"history\">{history}</ul></div>",
            slug = escape(&card.slug),
            state = card.state.as_str(),
            open = if thread.open { "true" } else { "false" },
            group = thread.group.key(),
            decided = escape(thread.decided_at.as_deref().unwrap_or("")),
            compare = compare_html(&card, ctx),
        );
        out.push(if matches!(thread.group, Group::Resolved | Group::Rejected) {
            // done: one line, the whole card a click away
            format!(
                "<details class=\"settled\"><summary><code>{}</code> <span class=\"settled-state\">{}</span> {}</summary>{one}</details>",
                escape(&card.slug),
                card.state.as_str(),
                escape(&summary(&card.text))
            )
        } else {
            one
        });
    }
    out.join("\n")
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
