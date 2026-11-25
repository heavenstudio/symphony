//! The review triage's ranking: what the owner should see first, and how they decide.
//!
//! `stats` reads every register under the documents (the same cards `just symphony-status --json`
//! reports) and reports the owner's first-pass approval rate per category, how often an approval
//! still carries a note, the letters they pick and how often they answer with a question. `queue N`
//! lists the N pending cards the owner should see first: design cards by how much they gate, then
//! their questions an agent has answered, then the cards only the owner signs off, then pictures
//! nobody could decide, each with a link that scrolls the Symphony to the card. The project's card
//! kinds say which card is which and what a card gates besides the cards waiting on it. `auto-approve`
//! is the project's own.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::board::Doc;
use crate::plugins::Plugins;
use crate::register::{Decision, Group, REPLACES, Who};
use crate::text::{S, is_space, strip};

pub const DEFAULT_PORT: u16 = 8765;
/// The Symphony's port record, in the project's artifacts folder.
pub const PORT_FILE: &str = "docs-app.port";
/// Where the Symphony of the primary instance listens.
pub const DEFAULT_APP: &str = "http://127.0.0.1:8765";

const OWNER_VERDICTS: [&str; 4] = ["approved", "rejected", "question", "dismissed"];

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
    pub picture_letters: BTreeMap<String, u32>,
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

/// A picture card: one of the project's card kinds names the slot it fills. A picture on a design
/// card does not make it a picture card.
pub fn is_picture(plugins: &Plugins, card: &Decision) -> bool {
    plugins.slot(card).is_some()
}

pub fn category(plugins: &Plugins, doc: &str, card: &Decision) -> String {
    let doc = doc
        .strip_prefix('@')
        .and_then(|p| p.split_once('/'))
        .map_or(doc, |(_, path)| path);
    plugins
        .triage_category(doc, card)
        .unwrap_or_else(|| format!("design · {doc}"))
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

/// An approval note is still work when it says more than which letter.
fn is_note(text: &str) -> bool {
    NOTE.is_match(&LETTER.replace_all(text, ""))
}

/// ("picture" or "design", letter) the owner picked on an approved card, or `None`.
fn picked_letter(
    plugins: &Plugins,
    card: &Decision,
) -> Result<Option<(&'static str, String)>, String> {
    let entries = card.entries();
    let approved = |e: &&crate::register::Entry| {
        e.who == Who::Owner && e.verdict.as_deref() == Some("approved")
    };
    if !matches!(card.state.as_str(), "approved" | "resolved")
        || !entries.iter().any(|e| approved(&e))
    {
        return Ok(None);
    }
    if is_picture(plugins, card) {
        return Ok(plugins.chosen(card)?.map(|letter| ("picture", letter)));
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

pub fn stats(plugins: &Plugins, docs: &[Doc]) -> Result<Stats, String> {
    let mut found = Stats {
        categories: BTreeMap::new(),
        first_post: Bucket::default(),
        redraw: Bucket::default(),
        approvals: 0,
        with_note: 0,
        picture_letters: BTreeMap::new(),
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
            let bucket = found
                .categories
                .entry(category(plugins, &doc, &card))
                .or_default();
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
        if let Some((kind, letter)) = picked_letter(plugins, &card)? {
            let counts = if kind == "picture" {
                &mut found.picture_letters
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
        ("picture", &found.picture_letters),
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
    let mut slugs = BTreeSet::new();
    slugs.extend(
        (pattern.captures_iter(text))
            .flat_map(|m| {
                m["slugs"]
                    .split(',')
                    .map(|s| strip(s).to_string())
                    .collect::<Vec<_>>()
            })
            .filter(|s| !s.is_empty()),
    );
    slugs
}

/// What a design card of `doc` gates: what the project's card kinds count in it, each as (how
/// many, the word for one), and how many cards wait on it (those naming it in `*Depends:*`, those
/// in its `*Blocks:*`).
pub fn gates(
    plugins: &Plugins,
    doc: &str,
    card: &Decision,
    cards: &[(String, Decision)],
) -> (Vec<(usize, String)>, usize) {
    let mut waiting: BTreeSet<String> = BTreeSet::new();
    waiting.extend(
        (cards.iter())
            .filter(|(_, other)| {
                other.slug != card.slug && slugs_in(&DEPENDS, &other.text).contains(&card.slug)
            })
            .map(|(_, other)| other.slug.clone()),
    );
    waiting.extend(
        slugs_in(&BLOCKS, &card.text)
            .into_iter()
            .filter(|s| *s != card.slug),
    );
    (plugins.gates(doc, card), waiting.len())
}

fn link(doc: &str, slug: &str, app: &str) -> String {
    format!("{app}/{doc}#{slug}")
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The `n` pending cards the owner should see first, highest leverage first, as the project's card
/// kinds sort them.
pub fn queue(plugins: &Plugins, docs: &[Doc], n: usize, app: &str) -> Vec<Item> {
    let cards = cards_of(docs);
    let mut design: Vec<(usize, String, Item)> = Vec::new();
    let mut answered: Vec<(String, Item)> = Vec::new();
    let mut picture_cards: Vec<(String, Item)> = Vec::new();
    let mut sign_offs: Vec<(String, Item)> = Vec::new();
    for (doc, card) in &cards {
        let posted = card.posted_at().unwrap_or_default();
        let thread = card.thread(plugins);
        let item = |reason: String| Item {
            slug: card.slug.clone(),
            doc: doc.clone(),
            reason,
            link: link(doc, &card.slug, app),
        };
        if thread.group == Group::Decide && !plugins.auto_approvable(card) {
            // the owner's sign-off alone: no triage decides one
            let why = if thread.asks_owner {
                "an agent asks you"
            } else {
                "only you sign it off"
            };
            sign_offs.push((
                posted,
                item(format!("{} · {why}", category(plugins, doc, card))),
            ));
        } else if thread.group == Group::Decide && !is_picture(plugins, card) {
            let (counted, waiting) = gates(plugins, doc, card, &cards);
            let total = waiting + counted.iter().map(|(n, _)| n).sum::<usize>();
            let mut reason = if total > 0 {
                let named: Vec<String> = counted
                    .iter()
                    .map(|(n, word)| (*n, word.as_str()))
                    .chain([(waiting, "card")])
                    .map(|(n, word)| plural(n, word))
                    .collect();
                format!("design · gates {}", named.join(" and "))
            } else {
                "design · gates nothing known".to_string()
            };
            if thread.asks_owner {
                reason = "design · an agent asks you".to_string();
            }
            design.push((usize::MAX - total, posted, item(reason)));
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
            picture_cards.push((
                posted,
                item(format!("{} · {why}", category(plugins, doc, card))),
            ));
        }
    }
    symphony_board::order::sort_by(&mut design, &|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    symphony_board::order::sort_by(&mut answered, &|a, b| a.0.cmp(&b.0));
    symphony_board::order::sort_by(&mut sign_offs, &|a, b| a.0.cmp(&b.0));
    symphony_board::order::sort_by(&mut picture_cards, &|a, b| a.0.cmp(&b.0));
    let mut ranked: Vec<Item> = design.into_iter().map(|(_, _, i)| i).collect();
    ranked.extend(answered.into_iter().map(|(_, i)| i));
    ranked.extend(sign_offs.into_iter().map(|(_, i)| i));
    ranked.extend(picture_cards.into_iter().map(|(_, i)| i));
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

/// Where the Symphony serving the checkout at `root` records its port: [`PORT_FILE`] in the
/// project's artifacts folder.
pub fn port_file(root: &Path) -> PathBuf {
    root.join(crate::project::Project::of(root).artifact(PORT_FILE))
}

/// The Symphony of the tree at `root`: its recorded port while something listens there (a killed app
/// leaves a stale record), else the primary instance's. Liveness is checked on loopback, so an app run
/// with `--host <tailnet ip>` alone reads as stale.
pub fn app_url(root: &Path) -> String {
    let recorded = std::fs::read_to_string(port_file(root))
        .ok()
        .and_then(|text| strip(&text).parse::<u16>().ok());
    let port = recorded
        .filter(|port| listening(*port))
        .unwrap_or(DEFAULT_PORT);
    format!("http://127.0.0.1:{port}")
}
