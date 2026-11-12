//! The complete review queue. The existing page feed invalidates this typed snapshot; rows mount
//! their contents near the viewport, while section anchors remain available for the home links.

use dioxus::prelude::*;
use symphony_api::Card;
#[cfg(target_arch = "wasm32")]
use symphony_api::{Queue, QueueItem};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
use crate::{
    cache::{self, Freshness, Item, board},
    docs, navigation,
    request::{self, Answer},
};
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

/// The queue: the copy this device kept is drawn at once, and checked once the board's read has
/// started the feed (`refreshed` counts the board's reads; the feed's queue events add to it).
#[component]
#[cfg(target_arch = "wasm32")]
pub fn QueuePage(scope: String, refreshed: Signal<u64>) -> Element {
    std::hint::black_box(&STYLE);
    let mut queue = use_signal(|| None::<Queue>);
    let mut error = use_signal(|| None::<String>);
    let mut generation = use_signal(|| 0_u64);
    let mut selected = use_signal(|| queue_filter::load(crate::platform::current().as_ref()));
    let mut etag = use_signal(String::new);
    let mut freshness = use_context::<Signal<Freshness>>();
    let item = use_hook(|| Item::board(&scope));
    use_hook(|| {
        let item = item.clone();
        spawn(async move {
            if let Some(kept) = cache::read::<Queue>(&item, board::QUEUE).await
                && queue.peek().is_none()
            {
                etag.set(kept.etag);
                queue.set(Some(kept.value));
                freshness.write().drawn(board::QUEUE, kept.saved);
            }
        });
    });
    use_effect(use_reactive!(|scope| {
        if refreshed() == 0 {
            return;
        }
        generation += 1;
        let ticket = *generation.peek();
        let url = docs::scoped(&scope, "/api/queue");
        let item = item.clone();
        // A read that fails is asked again, waiting longer each time, until it answers or a newer
        // read replaces it: the copy drawn from this device, and its held Later buttons, wait for
        // an answer.
        spawn(async move {
            let mut backoff = crate::state::Backoff::default();
            loop {
                let kept = etag.peek().clone();
                let fetched = request::get_if(&url, &kept).await;
                if ticket != *generation.peek() {
                    return;
                }
                let fetched = match fetched {
                    Ok(Answer::Same) => Ok(None),
                    Ok(Answer::Fresh(value, tag)) => {
                        serde_wasm_bindgen::from_value::<Queue>(value.clone())
                            .map(|fetched| {
                                etag.set(tag.clone());
                                cache::keep_later(&item, board::QUEUE, tag, value);
                                Some(fetched)
                            })
                            .map_err(|e| e.to_string())
                    }
                    Err(why) => Err(why),
                };
                match fetched {
                    Ok(fetched) => {
                        if let Some(fetched) = fetched
                            && queue.peek().as_ref() != Some(&fetched)
                        {
                            queue.set(Some(fetched));
                        }
                        error.set(None);
                        freshness.write().answered(board::QUEUE);
                        return;
                    }
                    Err(why) => {
                        freshness.write().failed(board::QUEUE);
                        error.set(Some(why));
                    }
                }
                crate::live::sleep(backoff.after(0.0)).await;
                if ticket != *generation.peek() {
                    return;
                }
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
        div { "data-ready": if located() { "queue" } else { "loading" },
            div { class: "dash queue-page",
                div { class: "page-head",
                    div {
                        p { class: "eyebrow", a { href: crate::project::href(&docs::scoped(&scope, "/")), "← board" } }
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
                } else { p { role: "status", "Loading the queue…" } }
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
                        NearViewport { height: 64,
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
                        NearViewport { height: 72,
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
