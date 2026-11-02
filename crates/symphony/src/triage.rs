//! The review triage's ranking: what the owner should see first, and how they decide.
//!
//! `stats` reads every register under docs/ (the same cards `just symphony-status --json` reports) and
//! reports the owner's first-pass approval rate per category, how often an approval still carries a
//! note, the letters they pick and how often they answer with a question. `queue N` lists the N pending
//! cards the owner should see first: design cards by how many hulls and cards they gate, then their
//! questions an agent has answered, then hull model cards, then art nobody could decide, each with a
//! link that scrolls the Symphony to the card. `auto-approve` and its roster-variety check go with
//! tools/python-zero/art-cards.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::art::{self, ArtError};
use crate::board::Doc;
use crate::model::is_model;
use crate::register::{Decision, Group, REPLACES, Who};
use crate::text::{S, is_space, strip};

pub const DEFAULT_PORT: u16 = 8765;
pub const PORT_FILE: &str = "artifacts/docs-app.port";
/// Where the Symphony of the primary instance listens.
pub const DEFAULT_APP: &str = "http://127.0.0.1:8765";

const OWNER_VERDICTS: [&str; 4] = ["approved", "rejected", "question", "dismissed"];

static HULLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[ships?:(?P<ids>[^\]|]+)").unwrap());
static DEPENDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\*(?:Depends|After):\*{S}*(?P<slugs>[a-z0-9][a-z0-9, -]*)"
    ))
    .unwrap()
});
static BLOCKS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\*(?:Blocks|Gates):\*{S}*(?P<slugs>[a-z0-9][a-z0-9, -]*)"
    ))
    .unwrap()
});
static OFFERS_LETTERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bsay [A-Z]\b").unwrap());
static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]\b").unwrap());
static NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z]{2,}").unwrap());
static ONE_CAPITAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z])\b").unwrap());

/// The decided cards and the first-pass approvals among them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bucket {
    pub decided: u32,
    pub approved_first: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub categories: BTreeMap<String, Bucket>,
    pub first_post: Bucket,
    pub redraw: Bucket,
    pub approvals: u32,
    pub with_note: u32,
    pub art_letters: BTreeMap<String, u32>,
    pub design_letters: BTreeMap<String, u32>,
    pub decided: u32,
    pub asked: u32,
}

/// One card of the queue: where it is, why it is there, and the link that scrolls the app to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub slug: String,
    pub doc: String,
    pub reason: String,
    pub link: String,
}

/// A picture card: it fills a picture slot (`*Shows:*`). A picture on a design card does not make it art.
pub fn is_art(card: &Decision) -> bool {
    art::shows(card).is_some()
}

/// The folder of a `/`-separated path, and its last part without its extension.
fn parent_and_stem(doc: &str) -> (&str, &str) {
    let (parent, name) = doc.rsplit_once('/').unwrap_or(("", doc));
    let stem = match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    };
    (parent, stem)
}

pub fn category(doc: &str, card: &Decision) -> String {
    let doc = doc
        .strip_prefix('@')
        .and_then(|p| p.split_once('/'))
        .map_or(doc, |(_, path)| path);
    if is_model(card) {
        return format!("hull model · {}", card.topic.to_lowercase());
    }
    let (parent, stem) = parent_and_stem(doc);
    if is_art(card) && parent == art::SHIP_PAGES {
        return format!("roster art · {stem}");
    }
    if is_art(card) || (parent == art::PAGES && !art::candidates(card).is_empty()) {
        return "faction art".to_string(); // the second case: a picture card posted before cards named their slot
    }
    format!("design · {doc}")
}

pub fn is_redraw(card: &Decision) -> bool {
    REPLACES.is_match(&card.text)
}

fn marker_verdict(state: &str) -> Option<&'static str> {
    match state {
        "approved" | "resolved" => Some("approved"),
        "rejected" => Some("rejected"),
        "question" => Some("question"),
        _ => None,
    }
}

/// The owner's first verdict on the card; for a card from before dated lines, its marker.
pub fn first_verdict(card: &Decision) -> Option<String> {
    let entries = card.entries();
    for entry in &entries {
        if entry.who == Who::Owner
            && let Some(verdict) = entry
                .verdict
                .as_deref()
                .filter(|v| OWNER_VERDICTS.contains(v))
        {
            return Some(verdict.to_string());
        }
    }
    if entries
        .iter()
        .any(|e| e.who == Who::Owner && e.ts.is_some())
    {
        return None; // dated owner lines, but only chat or a reset
    }
    marker_verdict(card.state.as_str()).map(str::to_string)
}

/// Letter to picture for a card's candidates, in the order posted; a single picture is A.
fn letters(card: &Decision) -> Vec<(String, String)> {
    let found = art::candidates(card);
    if found.len() == 1 {
        return vec![("A".to_string(), found[0].1.clone())];
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for (alt, src) in found {
        if alt.len() >= 2
            && alt.ends_with(|c: char| c.is_ascii_uppercase())
            && alt[..alt.len() - 1].ends_with(' ')
        {
            let letter = alt[alt.len() - 1..].to_string();
            match out.iter_mut().find(|(l, _)| *l == letter) {
                Some(held) => held.1 = src,
                None => out.push((letter, src)),
            }
        }
    }
    out
}

/// An approval note is still work when it says more than which letter.
fn is_note(text: &str) -> bool {
    NOTE.is_match(&LETTER.replace_all(text, ""))
}

/// ("art" or "design", letter) the owner picked on an approved card, or `None`.
fn picked_letter(card: &Decision) -> Result<Option<(&'static str, String)>, ArtError> {
    let entries = card.entries();
    let approved = |e: &&crate::register::Entry| {
        e.who == Who::Owner && e.verdict.as_deref() == Some("approved")
    };
    if !matches!(card.state.as_str(), "approved" | "resolved")
        || !entries.iter().any(|e| approved(&e))
    {
        return Ok(None);
    }
    if is_art(card) {
        let letters = letters(card);
        if letters.len() < 2 {
            return Ok(None);
        }
        let chosen = art::picture(card)?;
        return Ok(letters
            .into_iter()
            .find(|(_, src)| Some(src) == chosen.as_ref())
            .map(|(l, _)| ("art", l)));
    }
    if !OFFERS_LETTERS.is_match(&card.text) {
        return Ok(None);
    }
    for entry in entries.iter().rev().filter(approved) {
        if let Some(named) = ONE_CAPITAL.captures(&entry.text) {
            return Ok(Some(("design", named[1].to_string())));
        }
    }
    Ok(None)
}

fn cards_of(docs: &[Doc]) -> Vec<(String, Decision)> {
    docs.iter()
        .flat_map(|doc| {
            doc.decisions
                .iter()
                .map(|card| (doc.path.clone(), card.clone()))
        })
        .collect()
}

pub fn stats(docs: &[Doc]) -> Result<Stats, ArtError> {
    let mut found = Stats {
        categories: BTreeMap::new(),
        first_post: Bucket::default(),
        redraw: Bucket::default(),
        approvals: 0,
        with_note: 0,
        art_letters: BTreeMap::new(),
        design_letters: BTreeMap::new(),
        decided: 0,
        asked: 0,
    };
    for (doc, card) in cards_of(docs) {
        let verdict = first_verdict(&card);
        let entries = card.entries();
        if let Some(verdict) = &verdict {
            found.decided += 1;
            let approved = u32::from(verdict == "approved");
            let post = if is_redraw(&card) {
                &mut found.redraw
            } else {
                &mut found.first_post
            };
            post.decided += 1;
            post.approved_first += approved;
            let bucket = found.categories.entry(category(&doc, &card)).or_default();
            bucket.decided += 1;
            bucket.approved_first += approved;
            let asked = entries
                .iter()
                .any(|e| e.who == Who::Owner && e.verdict.as_deref() == Some("question"));
            found.asked += u32::from(asked || verdict == "question");
        }
        for entry in &entries {
            if entry.who == Who::Owner && entry.verdict.as_deref() == Some("approved") {
                found.approvals += 1;
                found.with_note += u32::from(is_note(&entry.text));
            }
        }
        if let Some((kind, letter)) = picked_letter(&card)? {
            let counts = if kind == "art" {
                &mut found.art_letters
            } else {
                &mut found.design_letters
            };
            *counts.entry(letter).or_default() += 1;
        }
    }
    Ok(found)
}

/// Python's `"%-7s %4s" % ("part/whole", "NN%" or "-")`, with `round` going to the even.
fn rate(part: u32, whole: u32) -> String {
    let share = if whole == 0 {
        "-".to_string()
    } else {
        format!(
            "{}%",
            (100.0 * f64::from(part) / f64::from(whole)).round_ties_even() as i64
        )
    };
    format!("{:<7} {:>4}", format!("{part}/{whole}"), share)
}

fn collapse(text: &str) -> String {
    text.split(is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn render_stats(found: &Stats) -> String {
    let mut out =
        vec!["FIRST-PASS APPROVAL (the owner's first verdict was an approval)".to_string()];
    let width = found
        .categories
        .keys()
        .map(|n| n.chars().count())
        .chain([10])
        .max()
        .unwrap_or(10);
    for (name, bucket) in &found.categories {
        out.push(format!(
            "  {name:<width$}  {}",
            rate(bucket.approved_first, bucket.decided)
        ));
    }
    out.push(String::new());
    for (name, bucket) in [("first post", &found.first_post), ("redraw", &found.redraw)] {
        out.push(format!(
            "  {name:<width$}  {}",
            rate(bucket.approved_first, bucket.decided)
        ));
    }
    out.push(String::new());
    out.push(format!(
        "approvals with a note: {} (a note is still work)",
        collapse(&rate(found.with_note, found.approvals))
    ));
    for (kind, picks) in [
        ("art", &found.art_letters),
        ("design", &found.design_letters),
    ] {
        let listed = picks
            .iter()
            .map(|(l, n)| format!("{l} {n}"))
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(format!(
            "{kind} letters: {}",
            if listed.is_empty() { "none" } else { &listed }
        ));
    }
    out.push(format!(
        "answered with a question: {}",
        collapse(&rate(found.asked, found.decided))
    ));
    out.join("\n")
}

fn slugs_in(pattern: &Regex, text: &str) -> BTreeSet<String> {
    pattern
        .captures_iter(text)
        .flat_map(|m| {
            m["slugs"]
                .split(',')
                .map(|s| strip(s).to_string())
                .collect::<Vec<_>>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// How many (hulls, cards) a design card gates: hulls it names, cards naming it in `*Depends:*`,
/// cards in its `*Blocks:*`.
pub fn gates(card: &Decision, cards: &[(String, Decision)]) -> (usize, usize) {
    let hulls: BTreeSet<String> = HULLS
        .captures_iter(&card.text)
        .flat_map(|m| {
            m["ids"]
                .split(',')
                .map(|h| strip(h).to_string())
                .collect::<Vec<_>>()
        })
        .filter(|h| !h.is_empty())
        .collect();
    let mut waiting: BTreeSet<String> = cards
        .iter()
        .filter(|(_, other)| {
            other.slug != card.slug && slugs_in(&DEPENDS, &other.text).contains(&card.slug)
        })
        .map(|(_, other)| other.slug.clone())
        .collect();
    waiting.extend(
        slugs_in(&BLOCKS, &card.text)
            .into_iter()
            .filter(|s| *s != card.slug),
    );
    (hulls.len(), waiting.len())
}

fn link(doc: &str, slug: &str, app: &str) -> String {
    format!("{app}/{doc}#{slug}")
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The `n` pending cards the owner should see first, highest leverage first.
pub fn queue(docs: &[Doc], n: usize, app: &str) -> Vec<Item> {
    let cards = cards_of(docs);
    let mut design: Vec<(usize, String, Item)> = Vec::new();
    let mut answered: Vec<(String, Item)> = Vec::new();
    let mut art_cards: Vec<(String, Item)> = Vec::new();
    let mut models: Vec<(String, Item)> = Vec::new();
    for (doc, card) in &cards {
        let posted = card.posted_at().unwrap_or_default();
        let thread = card.thread();
        let item = |reason: String| Item {
            slug: card.slug.clone(),
            doc: doc.clone(),
            reason,
            link: link(doc, &card.slug, app),
        };
        if thread.group == Group::Decide && is_model(card) {
            // the owner's sign-off alone: no triage decides one
            let why = if thread.asks_owner {
                "an agent asks you"
            } else {
                "only you sign it off"
            };
            models.push((posted, item(format!("{} · {why}", category(doc, card)))));
        } else if thread.group == Group::Decide && !is_art(card) {
            let (hulls, waiting) = gates(card, &cards);
            let mut reason = if hulls > 0 || waiting > 0 {
                format!(
                    "design · gates {} and {}",
                    plural(hulls, "hull"),
                    plural(waiting, "card")
                )
            } else {
                "design · gates nothing known".to_string()
            };
            if thread.asks_owner {
                reason = "design · an agent asks you".to_string();
            }
            design.push((usize::MAX - (hulls + waiting), posted, item(reason)));
        } else if thread.group == Group::Confirm {
            let what = if card.state.as_str() == "question" {
                "your question"
            } else {
                "your approval note"
            };
            answered.push((posted, item(format!("{what}, answered by an agent"))));
        } else if thread.group == Group::Decide {
            let why = if thread.asks_owner {
                "an agent asks you"
            } else {
                "the triage left it for you"
            };
            art_cards.push((posted, item(format!("{} · {why}", category(doc, card)))));
        }
    }
    design.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    answered.sort_by(|a, b| a.0.cmp(&b.0));
    models.sort_by(|a, b| a.0.cmp(&b.0));
    art_cards.sort_by(|a, b| a.0.cmp(&b.0));
    let mut ranked: Vec<Item> = design.into_iter().map(|(_, _, i)| i).collect();
    ranked.extend(answered.into_iter().map(|(_, i)| i));
    ranked.extend(models.into_iter().map(|(_, i)| i));
    ranked.extend(art_cards.into_iter().map(|(_, i)| i));
    ranked.truncate(n);
    ranked
}

pub fn render_queue(items: &[Item]) -> String {
    if items.is_empty() {
        return "nothing waiting on the owner".to_string();
    }
    items
        .iter()
        .enumerate()
        .flat_map(|(at, item)| {
            [
                format!("{:>2}. {}  {}", at + 1, item.slug, item.doc),
                format!("    {}", item.reason),
                format!("    {}", item.link),
            ]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn listening(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
}

/// The Symphony of the tree at `root`: its recorded port while something listens there (a killed app
/// leaves a stale record), else the primary instance's. Liveness is checked on loopback, so an app run
/// with `--host <tailnet ip>` alone reads as stale.
pub fn app_url(root: &Path) -> String {
    let recorded = std::fs::read_to_string(root.join(PORT_FILE))
        .ok()
        .and_then(|text| strip(&text).parse::<u16>().ok());
    let port = recorded
        .filter(|port| listening(*port))
        .unwrap_or(DEFAULT_PORT);
    format!("http://127.0.0.1:{port}")
}
