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
        if queue.read().is_some() && !*located.peek() {
            located.set(true);
            navigation::scroll_anchor();
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

/// The queue before its read: each group's card with its heading and the rows it is likely to hold.
#[component]
#[cfg(target_arch = "wasm32")]
fn GhostQueue() -> Element {
    rsx! {
        for (group, rows) in [(0_u8, 4_u8), (1, 2), (2, 2)] {
            div { key: "{group}", class: "card ghost-group",
                div { span { class: "bone", style: "--w:12ch" } }
                // What waits on the owner opens on its topic chips.
                if group == 0 { div { class: "ghost-chips", span { class: "ghost-pill", span { class: "bone", style: "--w:4ch" } } span { class: "ghost-pill", span { class: "bone", style: "--w:6ch" } } } }
                for n in 0..rows { GhostRow { key: "{n}", ranked: group == 0 } }
            }
        }
    }
}

#[component]
pub fn WaitingQueue(items: Vec<Card>, selected: String, onselect: EventHandler<String>) -> Element {
    std::hint::black_box(&STYLE);
    let counts = queue_filter::counts(items.iter().map(|card| card.doc.as_str()));
    let selected = queue_filter::active(&selected, &counts).to_string();
    let visible: Vec<_> = items
        .iter()
        .filter(|card| selected.is_empty() || symphony_api::queue_category(&card.doc) == selected)
        .collect();
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
