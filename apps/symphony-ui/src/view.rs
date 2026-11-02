//! The board view's components: pure functions of a board and the time.
//!
//! Each region is a component of its own data, so a feed event that changes one card renders
//! that card's component again and nothing else: Dioxus skips a component whose props are equal.

use dioxus::prelude::*;
use symphony_api::{Board, Card, Counts, FactionArt, Picture, Progress};

use crate::media::PictureView;
use crate::words::{Moment, age, greeting, lead, plural};

/// How many pictures the hero shows: a picture card's candidates, or the hulls a card names.
const HERO_PICTURES: usize = 6;

/// The body of `/`.
#[component(lazy)]
pub fn BoardView(board: Board, now: Moment) -> Element {
    let counts = &board.counts;
    rsx! {
        div { class: "dash",
            div { class: "dash-head page-head",
                div {
                    h1 { "{greeting(now)}" }
                    p { class: "lead", "{lead(counts.waiting, counts.docs_waiting)}" }
                }
                p { class: "dash-links",
                    a { class: "pill", href: "/queue", "Full queue" }
                    a { class: "pill", href: "/docs/", "All docs" }
                    a { class: "pill", href: "/hulls", "Hull models" }
                    a { class: "pill", href: "/docs/DECISIONS.md", "Decision register" }
                    a { class: "pill", href: "/docs/workflow/REVIEW.md", "How review works" }
                }
            }
            div { class: "dash-top",
                Hero { card: board.next.first().cloned(), waiting: counts.waiting, spot: counts.spot_check, now }
                if board.next.len() > 1 {
                    NextList { cards: board.next[1..].to_vec(), waiting: counts.waiting }
                }
            }
            Tiles { counts: counts.clone() }
            div { class: "dash-grid",
                ProgressPanel { progress: board.progress.clone() }
            }
        }
    }
}

/// The next card, with a button straight to it; or the quiet of an empty queue.
#[component]
fn Hero(card: Option<Card>, waiting: u32, spot: u32, now: Moment) -> Element {
    let Some(card) = card else {
        return rsx! {
            section { class: "hero card empty",
                div { class: "hero-text",
                    p { class: "eyebrow", "Next up" }
                    h2 { class: "hero-title", "Nothing waits on you. ✦" }
                    p { class: "hero-summary",
                        "Every card has your verdict, and the agents have the next move. Enjoy the quiet."
                        if spot > 0 {
                            " While you are here: "
                            a { href: "/queue#spot", "{spot} auto-approved {plural(spot, \"card\")}" }
                            " could use a glance."
                        }
                    }
                }
            }
        };
    };
    let waited = age(card.posted.as_deref(), now);
    rsx! {
        section { class: "hero card",
            div { class: "hero-text",
                p { class: "eyebrow", "Next up · 1 of {waiting}" }
                h2 { class: "hero-title", "{card.title}" }
                if !card.summary.is_empty() {
                    p { class: "hero-summary", "{card.summary}" }
                }
                p { class: "hero-meta",
                    a { class: "docchip", href: "/{card.doc}", "{card.doc_title}" }
                    crate::doc_components::TaskTag { task: card.task.clone() }
                    " "
                    span { class: "why", "{card.why}" }
                    if !waited.is_empty() {
                        " "
                        span { class: "muted", "waiting {waited}" }
                    }
                }
                a { class: "go", href: "{card.href()}",
                    "Open this card "
                    span { "aria-hidden": "true", "→" }
                }
            }
            if !card.pictures.is_empty() {
                div { class: "pics",
                    for picture in card.pictures.iter().take(HERO_PICTURES) {
                        Figure { picture: picture.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn Figure(picture: Picture) -> Element {
    match &picture.src {
        None => rsx! {
            figure { class: "none",
                div { class: "nopic", "picture coming" }
                figcaption { "{picture.caption}" }
            }
        },
        Some(src) => rsx! {
            figure { class: if picture.waiting { "waiting" },
                PictureView { src: src.clone(), caption: picture.caption.clone() }
                figcaption { "{picture.caption}" }
            }
        },
    }
}

/// The cards after the next one, in the queue's order.
#[component]
fn NextList(cards: Vec<Card>, waiting: u32) -> Element {
    rsx! {
        section { class: "next card",
            h2 { class: "eyebrow", "Then" }
            div { class: "nextlist",
                for card in cards {
                    NextCard { key: "{card.doc}#{card.slug}", card }
                }
            }
            a { class: "more", href: "/queue", "show all {waiting} →" }
        }
    }
}

#[component]
fn NextCard(card: Card) -> Element {
    let thumb = card.pictures.iter().find_map(|picture| picture.src.clone());
    rsx! {
        a { class: "nextcard", href: "{card.href()}",
            if let Some(src) = thumb {
                PictureView { src, caption: String::new(), thumb: true }
            } else {
                span { class: "thumb glyph", "◆" }
            }
            span { class: "nexttext",
                strong { "{card.title}" }
                small { "{card.doc_title} · {card.why}" }
                if let Some(task) = &card.task { small { class: "chip", "{task}" } }
            }
        }
    }
}

/// The row of counts, each linking to where its detail lives.
#[component]
fn Tiles(counts: Counts) -> Element {
    let (tasks, live) = match &counts.tasks {
        Some(tasks) => (
            format!("{} · {}", tasks.in_progress, tasks.in_review),
            tasks.agents_live.to_string(),
        ),
        None => ("–".to_string(), "–".to_string()),
    };
    let waiting_hint = if counts.waiting > 0 {
        format!(
            "across {} {}",
            counts.docs_waiting,
            plural(counts.docs_waiting, "document")
        )
    } else {
        "all clear".to_string()
    };
    rsx! {
        div { class: "tiles",
            Tile { href: "/queue#you", value: counts.waiting.to_string(), label: "waiting on you", tone: "you", hint: waiting_hint }
            Tile { href: "/queue#spot", value: counts.spot_check.to_string(), label: "auto-approved to spot-check", tone: "auto", hint: "your verdict overrides" }
            Tile { href: "#activity", value: counts.answered_today.to_string(), label: "answered by agents today", tone: "answered", hint: "replies to you" }
            Tile { href: "/tasks", value: tasks, label: "tasks in progress · in review", tone: "tasks", hint: "on the task board" }
            Tile { href: "/agents", value: live, label: "agents live", tone: "agents", hint: "working now" }
        }
    }
}

#[component]
fn Tile(
    href: &'static str,
    value: String,
    label: &'static str,
    tone: &'static str,
    hint: String,
) -> Element {
    rsx! {
        a { class: "tile card tone-{tone}", href,
            b { "{value}" }
            span { "{label}" }
            small { "{hint}" }
        }
    }
}

/// Ship art per faction, and the design documents.
#[component]
fn ProgressPanel(progress: Progress) -> Element {
    let approved: u32 = progress
        .ship_art
        .iter()
        .map(|faction| faction.approved)
        .sum();
    let total: u32 = progress.ship_art.iter().map(|faction| faction.total).sum();
    let waiting = progress.decisions_waiting;
    rsx! {
        section { class: "progress panel card",
            if total > 0 {
                div { class: "art",
                    div { class: "headline",
                        h2 { "Ship art" }
                        p { class: "big",
                            "{percent(approved, total):.0}%"
                            small { "{approved} of {total} hulls approved" }
                        }
                    }
                    div { class: "bars",
                        for faction in progress.ship_art.iter() {
                            FactionBar { key: "{faction.faction}", faction: faction.clone() }
                        }
                    }
                    p { class: "legend",
                        span { class: "dot ok" }
                        "approved "
                        span { class: "dot wait" }
                        "waiting on you "
                        span { class: "dot empty" }
                        "not drawn"
                    }
                }
            }
            div { class: "docsummary",
                h3 {
                    "Design docs "
                    a { class: "subtle", href: "/docs/", "browse all {progress.docs} →" }
                }
                p { "{waiting} {plural(waiting, \"decision\")} waiting" }
            }
        }
    }
}

#[component]
fn FactionBar(faction: FactionArt) -> Element {
    let undrawn = faction
        .total
        .saturating_sub(faction.approved + faction.waiting);
    rsx! {
        a {
            class: "bar",
            href: "/{faction.page}",
            title: "{faction.approved} approved, {faction.waiting} waiting on review, {undrawn} not drawn yet",
            span { class: "bar-name", "{faction.name}" }
            span { class: "track",
                i { class: "ok", style: "width: {percent(faction.approved, faction.total):.1}%" }
                i { class: "wait", style: "width: {percent(faction.waiting, faction.total):.1}%" }
            }
            span { class: "bar-num",
                "{faction.approved}"
                small { "/{faction.total}" }
            }
        }
    }
}

fn percent(part: u32, whole: u32) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * f64::from(part) / f64::from(whole)
    }
}
