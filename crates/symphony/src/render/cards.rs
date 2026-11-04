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

/// Owner words stay literal text, but for the files they attached; an agent entry is markdown, so it
/// can link, show images and use components.
fn entry_html(entry: &Entry, ctx: &Ctx<'_>) -> String {
    let body = if entry.who == Who::Agent {
        single_paragraph(ctx.convert(&entry.text))
    } else {
        owner_html(&entry.text, ctx)
    };
    let mut parts = Vec::new();
    if let Some(ts) = &entry.ts {
        parts.push(format!("<time>{}</time>", escape(ts)));
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

type Pictures = Vec<(String, String)>;

/// The pictures a card compares, as (label, src): the base a card was drawn from (an agent line
/// starting "Base"), and the candidates of the first agent line holding pictures, each lettered by its
/// alt text (empty when the alt does not end in a letter).
pub(crate) fn compare_pictures(card: &Decision) -> (Pictures, Pictures) {
    let (mut base, mut candidates): (Pictures, Pictures) = (Vec::new(), Vec::new());
    for entry in card.entries().iter().filter(|e| e.who == Who::Agent) {
        let found: Vec<_> = IMAGE.captures_iter(&entry.text).collect();
        let Some(first) = found.first() else {
            continue;
        };
        if entry.text.starts_with("Base") {
            if base.is_empty() {
                base = vec![("Base".to_string(), first["src"].to_string())];
            }
        } else if candidates.is_empty() {
            candidates = found
                .iter()
                .map(|f| {
                    let alt = &f["alt"];
                    let label = if LETTERED.is_match(alt) {
                        alt.rsplit(' ').next().unwrap_or("")
                    } else {
                        ""
                    };
                    (label.to_string(), f["src"].to_string())
                })
                .collect();
        }
    }
    (base, candidates)
}

/// One labelled row of the pictures to choose between, an `.html` mockup as media. Empty when there is
/// nothing to compare.
fn compare_html(card: &Decision, ctx: &Ctx<'_>) -> String {
    let (base, candidates) = compare_pictures(card);
    let row: Vec<_> = base.into_iter().chain(candidates).collect();
    if row.len() < 2 {
        return String::new();
    }
    let figures: String = row
        .iter()
        .map(|(label, src)| {
            format!(
                "<figure><img src=\"{}\" alt=\"{}\" loading=\"lazy\"><figcaption>{}</figcaption></figure>",
                escape(src),
                escape(label),
                escape(label)
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
