//! The board view's components: pure functions of a board and the time.
//!
//! Each region is a component of its own data, so a feed event that changes one card renders
//! that card's component again and nothing else: Dioxus skips a component whose props are equal.

use dioxus::prelude::*;
use symphony_api::{Board, Card, Counts, Picture, Progress, ProgressBar, ProjectView, Region};

use crate::media::PictureView;
use crate::words::{Moment, age, greeting, lead, plural};

/// How many pictures the hero shows: a picture card's candidates, or the items a card names.
const HERO_PICTURES: usize = 6;

/// The body of `/`.
#[component(lazy)]
pub fn BoardView(board: Board, now: Moment, #[props(default)] project: ProjectView) -> Element {
    let counts = &board.counts;
    rsx! {
        div { class: "dash",
            div { class: "dash-head page-head",
                div {
                    h1 { "{greeting(now)}" }
                    p { class: "lead", "{lead(counts.waiting, counts.docs_waiting)}" }
                }
                p { class: "dash-links",
                    a { class: "pill", href: crate::project::href("/queue"), "Full queue" }
                    a { class: "pill", href: crate::project::href(&project.documents_href()), "All docs" }
                    for item in project.nav.iter() {
                        a { key: "{item.href}", class: "pill", href: crate::project::href(&item.href), "{item.title}" }
                    }
                    // The register and the guide are the project's: a project without either file
                    // (a new one) has no link to it, nor a read ahead of it.
                    if !project.register.is_empty() {
                        a { class: "pill", href: crate::project::href(&format!("/{}", project.register)), "Decision register" }
                    }
                    if !project.guide.is_empty() {
                        a { class: "pill", href: crate::project::href(&format!("/{}", project.guide)), "How review works" }
                    }
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
                ProgressPanel { progress: board.progress.clone(), docs: project.documents_href() }
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
                            a { href: crate::project::href("/queue#spot"), "{spot} auto-approved {plural(spot, \"card\")}" }
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
                    a { class: "docchip", href: crate::project::href(&format!("/{}", card.doc)), "{card.doc_title}" }
                    crate::doc_components::TaskTag { task: card.task.clone() }
                    " "
                    span { class: "why", "{card.why}" }
                    if !waited.is_empty() {
                        " "
                        span { class: "muted", "waiting {waited}" }
                    }
                }
                a { class: "go", href: crate::project::href(&card.href()),
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

/// The cards after the next one, in the queue's order, each by its place in it: the hero is 1.
#[component]
fn NextList(cards: Vec<Card>, waiting: u32) -> Element {
    rsx! {
        section { class: "next card",
            h2 { class: "eyebrow", "Then" }
            div { class: "nextlist",
                for (index, card) in cards.into_iter().enumerate() {
                    NextCard { key: "{card.doc}#{card.slug}", card, rank: index + 2 }
                }
            }
            a { class: "more", href: crate::project::href("/queue"), "show all {waiting} →" }
        }
    }
}

/// A card of the list, its first picture as its thumbnail, else its place in the queue.
#[component]
fn NextCard(card: Card, rank: usize) -> Element {
    let thumb = card.pictures.iter().find_map(|picture| picture.src.clone());
    rsx! {
        a { class: "nextcard", href: crate::project::href(&card.href()),
            if let Some(src) = thumb {
                PictureView { src, caption: String::new(), thumb: true }
            } else {
                span { class: "thumb glyph", "{rank}" }
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
            ActivityTile { value: counts.answered_today.to_string() }
            Tile { href: "/tasks", value: tasks, label: "tasks in progress · in review", tone: "tasks", hint: "on the board" }
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
        a { class: "tile card tone-{tone}", href: crate::project::href(href),
            b { "{value}" }
            span { "{label}" }
            small { "{hint}" }
        }
    }
}

/// The agents' replies of today are listed in the activity panel, no page of their own: the tile
/// opens that panel, as the header's activity button does.
#[component]
fn ActivityTile(value: String) -> Element {
    #[cfg(target_arch = "wasm32")]
    let open = crate::entry_events::click(crate::frame::open_activity);
    #[cfg(not(target_arch = "wasm32"))]
    let open: Vec<Attribute> = Vec::new();
    rsx! {
        button { r#type: "button", class: "tile card tone-answered", "aria-controls": "activity-panel", ..open,
            b { "{value}" }
            span { "answered by agents today" }
            small { "replies to you" }
        }
    }
}

/// The plugins' progress regions that have something to count, and the design documents, set
/// apart by a rule only below a region.
#[component]
fn ProgressPanel(progress: Progress, docs: String) -> Element {
    let waiting = progress.decisions_waiting;
    rsx! {
        section { class: "progress panel card",
            for region in progress.regions.iter().filter(|region| counted(region) > 0) {
                RegionView { key: "{region.id}", region: region.clone() }
            }
            div { class: "docsummary",
                h3 {
                    "Design docs "
                    a { class: "subtle", href: crate::project::href(&docs), "browse all {progress.docs} →" }
                }
                p { "{waiting} {plural(waiting, \"decision\")} waiting" }
            }
        }
    }
}

/// How many things a region's bars count in all.
fn counted(region: &Region) -> u32 {
    region.bars.iter().map(|bar| bar.total).sum()
}

/// A region of bars, done and waiting out of a total each, with its headline percentage and its
/// legend's three words.
#[component]
fn RegionView(region: Region) -> Element {
    let done: u32 = region.bars.iter().map(|bar| bar.done).sum();
    let total = counted(&region);
    let [ok, wait, empty] = region.legend.clone();
    rsx! {
        div { class: "region", "data-region": "{region.id}",
            div { class: "headline",
                h2 { "{region.title}" }
                p { class: "big",
                    "{percent(done, total):.0}%"
                    small { "{region.note}" }
                }
            }
            div { class: "bars",
                for bar in region.bars.iter() {
                    BarView { key: "{bar.key}", bar: bar.clone() }
                }
            }
            p { class: "legend",
                span { class: "dot ok" }
                "{ok} "
                span { class: "dot wait" }
                "{wait} "
                span { class: "dot empty" }
                "{empty}"
            }
        }
    }
}

#[component]
fn BarView(bar: ProgressBar) -> Element {
    rsx! {
        a {
            class: "bar",
            href: crate::project::href(&format!("/{}", bar.page)),
            title: "{bar.title}",
            span { class: "bar-name", "{bar.name}" }
            span { class: "track",
                i { class: "ok", style: "width: {percent(bar.done, bar.total):.1}%" }
                i { class: "wait", style: "width: {percent(bar.waiting, bar.total):.1}%" }
            }
            span { class: "bar-num",
                "{bar.done}"
                small { "/{bar.total}" }
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
