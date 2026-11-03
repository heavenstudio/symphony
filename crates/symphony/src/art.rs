//! Which picture a card shows, and for what.
//!
//! A faction page shows, per subfaction, one picture of each kind it names. Which picture is decided
//! in the Symphony, so the page holds none: a card says which slot it fills with `*Shows:*
//! <Subfaction> · <kind>` in its text (or, for a card posted before the tag existed, with an appended
//! agent line `shows: <Subfaction> · <kind>`), and its picture is the first image an agent posted
//! under it. Per slot: the most recently approved picture; failing that, the newest pending or
//! questioned one, shown as waiting on review; failing that, nothing. Rejected and dismissed pictures
//! are never shown.
//!
//! The HTML of a slot is shared by the live components; the `post` command goes with
//! tools/python-zero/art-cards.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use symphony_api::{Block, ContainerKind, FigureSize, Image, Presentation, Span};

use crate::register::{Decision, REGION_END, REGION_START, Who, parse_decisions};
use crate::text::{S, is_word, read_text, strip};

/// A state whose picture counts as the owner's choice (`auto-approved`: `[a]`, set by the triage tool).
pub const APPROVED: [&str; 3] = ["approved", "resolved", "auto-approved"];
pub const AUTO: &str = "auto-approved";
pub const REVERTED: &str = "reverted";
pub const PAGES: &str = "docs/factions";
pub const SHIP_PAGES: &str = "docs/ships";
/// Its hulls' factions name the ships pages.
pub const ROSTER: &str = "docs/data/ship-roster.v2.json";

/// A page or a picture cannot be read as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtError(pub String);

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ArtError {}

static SHOWS_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\*Shows:\*{S}*(?P<sub>[^·*]+?){S}*·{S}*(?P<kind>[\w-]+)"
    ))
    .unwrap()
});
static SHOWS_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^shows:{S}*(?P<sub>[^·]+?){S}*·{S}*(?P<kind>[\w-]+){S}*$"
    ))
    .unwrap()
});
static IMAGE_ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\(([^)\s]+)\)").unwrap());
static LETTERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" [A-Z]$").unwrap());
static AUTO_LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Z])\b").unwrap());
/// An artifacts path an approval names; Python's `(?<![\w/])` before it is checked by hand.
static EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)/?artifacts/[^\s`<>"'()]+\.(?:png|jpe?g|webp|svg)\b"#).unwrap()
});
static CAPITAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z])\b").unwrap());
static ANY_LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Za-z])\b").unwrap());

/// The (subfaction, kind) slot a picture card fills, or `None`.
pub fn shows(card: &Decision) -> Option<(String, String)> {
    let entries = card.entries();
    shows_in(
        &card.text,
        entries
            .iter()
            .filter(|e| e.who == Who::Agent)
            .map(|e| e.text.as_str()),
    )
}

/// [`shows`] of a card whose text is `text` and whose agents wrote `agent_lines`.
pub fn shows_in<'a>(
    text: &str,
    agent_lines: impl IntoIterator<Item = &'a str>,
) -> Option<(String, String)> {
    if let Some(found) = SHOWS_TEXT.captures(text) {
        return Some((
            strip(&found["sub"]).to_string(),
            found["kind"].to_lowercase(),
        ));
    }
    for line in agent_lines {
        if let Some(tagged) = SHOWS_LINE.captures(line) {
            return Some((
                strip(&tagged["sub"]).to_string(),
                tagged["kind"].to_lowercase(),
            ));
        }
    }
    None
}

/// (alt, src) of every image in the first agent line that has one.
pub fn candidates(card: &Decision) -> Vec<(String, String)> {
    let entries = card.entries();
    candidates_in(
        entries
            .iter()
            .filter(|e| e.who == Who::Agent)
            .map(|e| e.text.as_str()),
    )
}

/// [`candidates`] of a card whose agents wrote `agent_lines`.
pub fn candidates_in<'a>(agent_lines: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    agent_lines
        .into_iter()
        .map(images)
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

fn images(text: &str) -> Vec<(String, String)> {
    IMAGE_ALT
        .captures_iter(text)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

/// The first place in `text` that holds an artifacts image path not glued to a word or a longer path.
fn explicit_path(text: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = EXPLICIT.find_at(text, from) {
        let before = text[..found.start()].chars().next_back();
        if !before.is_some_and(|c| is_word(c) || c == '/') {
            return Some(found.as_str().to_string());
        }
        from = found.start()
            + text[found.start()..]
                .chars()
                .next()
                .map_or(1, char::len_utf8);
    }
    None
}

/// The card's picture: the candidate the owner's latest approval or question to name a letter picks,
/// else the first one posted.
///
/// An explicit artifacts image path in an owner approval overrides candidate letters, but only one an
/// agent posted on this card; an [`ArtError`] otherwise. A plain approval keeps a letter an earlier
/// question named ("B, but where are the guns?" then "approved"). A question is prose, so only a
/// capital letter counts there: "is a C better?" names C, not A.
pub fn picture(card: &Decision) -> Result<Option<String>, ArtError> {
    let found = candidates(card);
    let Some(first) = found.first() else {
        return Ok(None);
    };
    let mut letters: HashMap<&str, &str> = HashMap::new();
    if found.len() > 1 {
        for (alt, src) in &found {
            if LETTERED.is_match(alt) {
                letters.insert(alt.rsplit(' ').next().unwrap_or(alt), src);
            }
        }
    }
    let entries = card.entries();
    let posted: HashSet<String> = entries
        .iter()
        .filter(|e| e.who == Who::Agent)
        .flat_map(|e| images(&e.text))
        .map(|(_, src)| src)
        .collect();
    let mut withdrawn = false; // a later `agent · reverted` line takes the auto pick back
    for entry in entries.iter().rev() {
        let verdict = entry.verdict.as_deref();
        withdrawn = withdrawn || (entry.who == Who::Agent && verdict == Some(REVERTED));
        // the triage tool's pick, until the owner names another
        if verdict == Some(AUTO)
            && entry.who == Who::Agent
            && !withdrawn
            && let Some(src) = AUTO_LETTER
                .captures(&entry.text)
                .and_then(|auto| letters.get(&auto[1]))
        {
            return Ok(Some((*src).to_string()));
        }
        if entry.who == Who::Owner
            && verdict == Some("approved")
            && let Some(path) = explicit_path(&entry.text)
                .map(|explicit| format!("/{}", explicit.trim_start_matches('/')))
        {
            if !posted.contains(&path) {
                return Err(ArtError(format!(
                    "card {}: the approval names {path}, which was never posted on that card",
                    card.slug
                )));
            }
            return Ok(Some(path));
        }
        if entry.who == Who::Owner
            && matches!(verdict, Some("approved" | "question"))
            && !letters.is_empty()
        {
            let pattern = if verdict == Some("question") {
                &CAPITAL
            } else {
                &ANY_LETTER
            };
            let named = pattern
                .captures_iter(&entry.text)
                .map(|c| c[1].to_uppercase())
                .find(|w| letters.contains_key(w.as_str()));
            if let Some(name) = named {
                return Ok(Some(letters[name.as_str()].to_string()));
            }
        }
    }
    Ok(Some(first.1.clone()))
}

/// When the card was last approved: the owner's latest verdict, or the triage tool's auto line.
fn approved_at(card: &Decision) -> Option<String> {
    card.entries().into_iter().rev().find_map(|entry| {
        let counts = (entry.who == Who::Owner && entry.verdict.is_some())
            || entry.verdict.as_deref() == Some(AUTO);
        entry.ts.filter(|_| counts)
    })
}

/// (picture, still waiting on review) for one slot, or `None` when nothing can be shown.
pub fn choose(
    cards: &[Decision],
    sub: &str,
    kind: &str,
) -> Result<Option<(String, bool)>, ArtError> {
    let mut candidates: Vec<&Decision> = Vec::new();
    for card in cards {
        if shows(card).is_some_and(|(s, k)| s == sub && k == kind)
            && picture(card)?.is_some_and(|p| !p.is_empty())
        {
            candidates.push(card);
        }
    }
    let newest = |of: &[&Decision], when: &dyn Fn(&Decision) -> String| -> Option<String> {
        let mut best: Option<(&Decision, String)> = None;
        for card in of {
            let at = when(card);
            if best.as_ref().is_none_or(|(_, held)| at > *held) {
                best = Some((card, at));
            }
        }
        best.and_then(|(card, _)| picture(card).ok().flatten())
    };
    let approved: Vec<&Decision> = candidates
        .iter()
        .copied()
        .filter(|c| APPROVED.contains(&c.state.as_str()))
        .collect();
    if let Some(best) = newest(&approved, &|c| {
        approved_at(c).or_else(|| c.posted_at()).unwrap_or_default()
    }) {
        return Ok(Some((best, false)));
    }
    let pending: Vec<&Decision> = candidates
        .iter()
        .copied()
        .filter(|c| matches!(c.state.as_str(), "pending" | "question"))
        .collect();
    Ok(newest(&pending, &|c| c.posted_at().unwrap_or_default()).map(|best| (best, true)))
}

/// The cards between the register markers of a page's lines; none without a region.
pub fn register_cards<L: AsRef<str>>(lines: &[L]) -> Vec<Decision> {
    let Some(start) = lines.iter().position(|l| l.as_ref() == REGION_START) else {
        return Vec::new();
    };
    let Some(end) = lines[start..].iter().position(|l| l.as_ref() == REGION_END) else {
        return Vec::new();
    };
    parse_decisions(&lines[start + 1..start + end])
}

/// `Sub · kind, kind` as written in an art component: (subfaction, kinds); an [`ArtError`] without both.
pub fn parse_slot(arg: &str) -> Result<(String, Vec<String>), ArtError> {
    let (sub, kinds) = arg.split_once('·').unwrap_or((arg, ""));
    let names: Vec<String> = kinds
        .split(',')
        .map(|k| strip(k).to_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    if strip(sub).is_empty() || names.is_empty() {
        return Err(ArtError(format!(
            "an art slot is written [[art:<Subfaction> · <kind>, <kind>]], not [[art:{}]]",
            strip(arg)
        )));
    }
    Ok((strip(sub).to_string(), names))
}

/// A figure per kind, from this page's own register, with a visible pending or missing caption,
/// side by side.
pub fn slot(cards: &[Decision], sub: &str, kinds: &[String]) -> Result<Block, ArtError> {
    let mut figures = Vec::new();
    for kind in kinds {
        let label = crate::ship_art::capitalize(kind);
        let (picture, caption, href) = match choose(cards, sub, kind)? {
            None => (None, format!("{label}: picture coming"), None),
            Some((src, waiting)) => {
                let caption = if waiting {
                    format!("{label} · waiting on your review")
                } else {
                    label.clone()
                };
                let picture = Image {
                    src: src.clone(),
                    alt: label,
                    presentation: Presentation::default(),
                };
                (Some(picture), caption, Some(src))
            }
        };
        figures.push(Block::Figure {
            picture,
            caption: vec![Span::Text { text: caption }],
            href,
            badge: None,
            size: FigureSize::Slot,
        });
    }
    Ok(Block::Container {
        tag: ContainerKind::Div,
        presentation: Presentation::class("pair"),
        blocks: figures,
    })
}

/// One page per faction: the lowercase pages in the factions folder, not its README or a topic's
/// UPPERCASE hub.
pub fn faction_pages(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(PAGES)) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            name.ends_with(".md") && name == name.to_lowercase() && name != "README.md"
        })
        .collect();
    found.sort();
    found
}

/// One ships page per faction in the roster (`just ship-art` writes them), for the ones written so far.
pub fn ship_pages(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = read_text(&root.join(ROSTER)) else {
        return Vec::new();
    };
    let Ok(roster) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let factions: std::collections::BTreeSet<String> = roster["hulls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| h["faction"].as_str().map(str::to_string))
        .collect();
    let mut found: Vec<PathBuf> = factions
        .into_iter()
        .map(|f| root.join(SHIP_PAGES).join(format!("{f}.md")))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found
}

/// Every page with picture slots: the faction pages and the per-faction ships pages.
pub fn pages(root: &Path) -> Vec<PathBuf> {
    let mut found = faction_pages(root);
    found.extend(ship_pages(root));
    found
}
