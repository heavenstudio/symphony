//! The complete review queue. The existing page feed invalidates this typed snapshot; rows mount
//! their contents near the viewport, while section anchors remain available for the home links.

use dioxus::prelude::*;
use symphony_api::Card;
#[cfg(target_arch = "wasm32")]
use symphony_api::{Queue, QueueItem};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
use crate::{docs, navigation, request};
use crate::{queue_filter, viewport::NearViewport};

const STYLE: Asset = asset!(
    "/assets/queue.css",
    AssetOptions::css().with_static_head(true)
);

#[cfg(target_arch = "wasm32")]
pub(crate) fn changed(value: &JsValue) -> bool {
    let changes = request::field(value, "changes");
    js_sys::Array::is_array(&changes)
        && js_sys::Array::from(&changes).iter().any(|change| {
            request::field(&change, "region")
                .as_string()
                .is_some_and(|region| {
                    matches!(
                        region.as_str(),
                        "queue-item"
                            | "queue-item-order"
                            | "spot-check"
                            | "spot-check-order"
                            | "agent-queue"
                            | "agent-queue-order"
                    )
                })
        })
}

#[component]
#[cfg(target_arch = "wasm32")]
pub fn QueuePage(scope: String, refreshed: Signal<u64>) -> Element {
    std::hint::black_box(&STYLE);
    let mut queue = use_signal(|| None::<Queue>);
    let mut error = use_signal(|| None::<String>);
    let mut generation = use_signal(|| 0_u64);
    let mut selected = use_signal(|| queue_filter::load(crate::platform::current().as_ref()));
    use_effect(use_reactive!(|scope| {
        let _ = refreshed();
        generation += 1;
        let ticket = *generation.peek();
        let url = docs::scoped(&scope, "/api/queue");
        spawn(async move {
            let fetched = request::get::<Queue>(&url).await;
            if ticket != *generation.peek() {
                return;
            }
            match fetched {
                Ok(fetched) => {
                    if queue.peek().as_ref() != Some(&fetched) {
                        queue.set(Some(fetched));
                    }
                    error.set(None);
                }
                Err(why) => error.set(Some(why)),
            }
        });
    }));
    let mut located = use_signal(|| false);
    use_effect(move || {
        if let Some(drawn) = queue.read().as_ref()
            && !*located.peek()
        {
            located.set(true);
            navigation::scroll_anchor();
            // What each group showed, and its height once its rows near the screen drew, for the
            // ghost of the next visit.
            let rows = [
                visible(&drawn.waiting, &selected.peek()).len(),
                drawn.spot_check.len(),
                drawn.agent.len(),
            ];
            crate::platform::current().save(ROWS, &keep_rows(rows));
            let parts = GROUPS.map(|(id, _)| (format!("queue:{id}"), format!("#{id}")));
            crate::viewport::remember_drawn(parts.to_vec(), 500);
        }
    });
    rsx! {
        div { "data-ready": if located() { "queue" } else { "loading" }, "aria-busy": "{queue.read().is_none() && error.read().is_none()}",
            div { class: "dash queue-page",
                div { class: "page-head",
                    div {
                        p { class: "eyebrow", a { href: crate::project::href(&docs::scoped(&scope, "/")), "← Home" } }
                        h1 { "Review queue" }
                        p { class: "lead", "Every card, with a mix of document topics." }
                    }
                }
                if let Some(why) = error() { p { role: "alert", "{why}" } }
                if let Some(queue) = queue() {
                    WaitingQueue { items: queue.waiting, selected: selected(),
                        onselect: move |category: String| {
                            queue_filter::remember(crate::platform::current().as_ref(), &category);
                            selected.set(category);
                        }
                    }
                    QueueGroup { id: "spot", title: "Auto-approved: spot-check", items: queue.spot_check, empty: "none" }
                    QueueGroup { id: "agent", title: "Waiting on the agent", items: queue.agent, empty: "every comment is answered" }
                } else { p { role: "status", class: "status-text", "Loading the queue…" } GhostQueue {} }
            }
        }
    }
}

/// A row of the queue while it loads: a bar for its title and one for the line under it; a card
/// waiting on the owner also has its rank and its Later button.
#[component]
fn GhostRow(#[props(default)] ranked: bool) -> Element {
    rsx! { div { class: "ghost-row",
        if ranked { span { class: "ghost-rank", "\u{a0}" } }
        div {
            span { class: "bone", style: "--w:66%" }
            div { span { class: "bone", style: "--w:44%" } }
            if ranked { span { class: "ghost-field ghost-later" } }
        }
    } }
}

/// The queue's groups in their order: each one's anchor, and the rows it is guessed to show when
/// this browser kept none.
#[cfg(target_arch = "wasm32")]
const GROUPS: [(&str, usize); 3] = [("you", 4), ("spot", 2), ("agent", 2)];
/// Where this browser keeps how many rows each group showed when the queue last drew, as
/// `WAITING SPOT AGENT`; the groups' heights are kept as `queue:ANCHOR`
/// (`viewport::drawn_height`).
#[cfg(target_arch = "wasm32")]
const ROWS: &str = "queue-rows";

/// The rows `kept` (as `ROWS` holds them) gives each group, none for one it does not say.
#[cfg(any(target_arch = "wasm32", test))]
fn recall_rows(kept: &str) -> [Option<usize>; 3] {
    let mut rows = kept.split_whitespace().map(|n| n.parse().ok());
    [(); 3].map(|()| rows.next().flatten())
}

/// The rows each group showed, as `ROWS` keeps them.
#[cfg(any(target_arch = "wasm32", test))]
fn keep_rows([waiting, spot, agent]: [usize; 3]) -> String {
    format!("{waiting} {spot} {agent}")
}

/// The queue before its read: each group's card with its heading and as many rows as it showed
/// last, at the height it took in a window as wide; else the rows it is likely to hold.
#[component]
#[cfg(target_arch = "wasm32")]
fn GhostQueue() -> Element {
    let rows = recall_rows(&crate::platform::current().load(ROWS).unwrap_or_default());
    rsx! {
        for (group, (id, guess)) in GROUPS.into_iter().enumerate() {
            div { key: "{id}", class: "card ghost-group", "data-for": "{id}", style: crate::viewport::drawn_style(crate::viewport::drawn_height(&format!("queue:{id}"))),
                div { span { class: "bone", style: "--w:12ch" } }
                // What waits on the owner opens on its topic chips.
                if group == 0 { div { class: "ghost-chips", span { class: "ghost-pill", span { class: "bone", style: "--w:4ch" } } span { class: "ghost-pill", span { class: "bone", style: "--w:6ch" } } } }
                // A group that showed none says so in a line.
                if rows[group] == Some(0) { div { class: "ghost-none", span { class: "bone", style: "--w:14ch" } } }
                for n in 0..rows[group].unwrap_or(guess).min(12) { GhostRow { key: "{n}", ranked: group == 0 } }
            }
        }
    }
}

/// The cards of what waits on the owner that its group shows with the topic `selected`: the
/// topic's while it has any, else all.
fn visible<'a>(items: &'a [Card], selected: &str) -> Vec<&'a Card> {
    let counts = queue_filter::counts(items.iter().map(|card| card.doc.as_str()));
    let selected = queue_filter::active(selected, &counts);
    items
        .iter()
        .filter(|card| selected.is_empty() || symphony_api::queue_category(&card.doc) == selected)
        .collect()
}

#[component]
pub fn WaitingQueue(items: Vec<Card>, selected: String, onselect: EventHandler<String>) -> Element {
    std::hint::black_box(&STYLE);
    let counts = queue_filter::counts(items.iter().map(|card| card.doc.as_str()));
    let visible = visible(&items, &selected);
    let selected = queue_filter::active(&selected, &counts).to_string();
    rsx! {
        section { id: "you", class: "card queue-group",
            h2 { "Waiting on you ({items.len()})" }
            queue_filter::CategoryChips { counts, selected: selected.clone(), onselect }
            if !selected.is_empty() { p { class: "queue-filter-count muted", role: "status", "Showing {visible.len()} of {items.len()}" } }
            ol { class: "queue-list ranked",
                if items.is_empty() { li { class: "muted", "nothing to decide" } }
                for (index, card) in visible.into_iter().enumerate() {
                    li { key: "{card.doc}#{card.slug}", "data-card": "{card.doc}#{card.slug}",
                        NearViewport { height: 64, ghost: rsx! { GhostRow { ranked: true } },
                            div { class: "queue-row",
                                span { class: "queue-rank", "{index + 1}" }
                                div {
                                    a { href: crate::project::href(&card.href()), "{card.title}" }
                                    p { class: "muted", "{card.doc_title} · {card.why}" }
                                    crate::doc_components::TaskTag { task: card.task.clone() }
                                    QueueControls { card: card.clone() }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn QueueControls(card: Card) -> Element {
    #[cfg(target_arch = "wasm32")]
    return rsx! { crate::postpone_live::Controls { path: card.doc, slug: card.slug, scope: String::new(), postponed: card.postponed } };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = card;
        rsx! {}
    }
}

#[component]
#[cfg(target_arch = "wasm32")]
fn QueueGroup(id: String, title: String, items: Vec<QueueItem>, empty: String) -> Element {
    rsx! {
        section { id, class: "card queue-group",
            h2 { "{title} ({items.len()})" }
            ul { class: "queue-list",
                if items.is_empty() { li { class: "muted", "{empty}" } }
                for item in items {
                    li { key: "{item.href}", "data-card": "{item.path}#{item.slug}",
                        NearViewport { height: 72, ghost: rsx! { GhostRow {} },
                            div { class: "queue-row",
                                div {
                                    a { href: crate::project::href(&item.href), "{item.title}" }
                                    if let Some(branch) = item.branch {
                                        p { class: "muted", span { class: "chip", "{branch}" } " {item.path}" }
                                    } else {
                                        if !item.kind.is_empty() { span { class: "chip", "{item.kind}" } }
                                        if !item.text.is_empty() { p { class: "muted", "{item.text}" } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Each group's rows are kept in the queue's order and read back as kept; a group the kept
    /// line does not say, or says unreadably, has none kept, and its ghost guesses.
    #[test]
    fn the_rows_each_group_showed_read_back_as_kept() {
        let kept = super::keep_rows([1, 0, 3]);
        assert_eq!(kept, "1 0 3");
        assert_eq!(super::recall_rows(&kept), [Some(1), Some(0), Some(3)]);
        assert_eq!(super::recall_rows(""), [None, None, None]);
        assert_eq!(super::recall_rows("2 x"), [Some(2), None, None]);
    }

    fn card(doc: &str) -> symphony_api::Card {
        serde_json::from_value(serde_json::json!({
            "doc": doc, "slug": "s", "title": "T", "doc_title": "D", "why": "w", "href": "/h"
        }))
        .unwrap()
    }

    /// What waits on the owner shows the topic chosen while it has cards, else all of them.
    #[test]
    fn the_waiting_group_shows_the_chosen_topic_while_it_has_cards() {
        let items = [
            card("docs/lamps/a.md"),
            card("docs/DECISIONS.md"),
            card("docs/lamps/b.md"),
        ];
        assert_eq!(super::visible(&items, "").len(), 3);
        let lamps = super::visible(&items, "lamps");
        assert_eq!(lamps.len(), 2);
        assert!(lamps.iter().all(|card| card.doc.starts_with("docs/lamps/")));
        assert_eq!(super::visible(&items, "gone").len(), 3);
    }
}
