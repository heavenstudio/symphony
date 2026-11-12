//! Ephemeral questions tied to a place in a document. Drafts stay on the device; only submitting
//! sends a question or follow-up. A read replaces thread data, never a half-typed answer.

use dioxus::prelude::*;
use symphony_api::{Ask, Block};
use wasm_bindgen::JsCast;

use crate::media_live::{fetch, field, post, sleep};
use crate::platform;

/// Key this component by scope and path when switching documents, just like the document feed.
#[component]
pub fn Asks(
    path: String,
    render_blocks: Callback<Vec<Block>, Element>,
    #[props(default)] scope: String,
) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    rsx! {
        section { class: "asks-shell", ..reveal,
            if shown() { AsksBody { path, scope, render_blocks } } else { h2 { "Questions about this document" } }
        }
    }
}

#[component]
fn AsksBody(path: String, scope: String, render_blocks: Callback<Vec<Block>, Element>) -> Element {
    let mut asks = use_signal(Vec::<Ask>::new);
    let mut recipient = use_signal(|| None::<String>);
    let mut trouble = use_signal(|| None::<String>);
    let mut refresh = use_signal(|| 0_u64);
    let mut running = use_signal(|| None::<dioxus::core::Task>);
    let endpoint = format!("{scope}/api/asks?format=blocks&path={}", encode(&path));
    let endpoint_for_poll = endpoint.clone();
    // The page's feed says when a thread of this document moves, the chat recipient changes or
    // the stream opens again; the questions are read then, never on a timer. A failed read tries
    // again until one answers.
    let watched = path.clone();
    let heard = crate::frame::use_asks_heard(move |news| news.concerns(None, Some(&watched)));
    use_effect(move || {
        let _ = heard();
        if let Some(task) = running.take() {
            task.cancel();
        }
        let endpoint = endpoint_for_poll.clone();
        running.set(Some(spawn(async move {
            loop {
                match fetch(&endpoint).await {
                    Ok(value) => match field::<Vec<Ask>>(&value, "asks") {
                        Ok(fetched) => {
                            if *asks.peek() != fetched {
                                asks.set(fetched);
                            }
                            recipient.set(field(&value, "recipient").unwrap_or(None));
                            trouble.set(None);
                            return;
                        }
                        Err(why) => trouble.set(Some(why)),
                    },
                    Err(why) => trouble.set(Some(why)),
                }
                sleep(3_000).await;
            }
        })));
    });
    // A submitted message refreshes immediately, without waiting for the next poll.
    use_effect(move || {
        if refresh() == 0 {
            return;
        }
        let endpoint = endpoint.clone();
        spawn(async move {
            if let Ok(value) = fetch(&endpoint).await
                && let Ok(fetched) = field(&value, "asks")
            {
                asks.set(fetched);
            }
        });
    });
    rsx! {
        section { class: "asks", id: "asks", "aria-label": "Questions about this document",
            h2 { "Ask about this document" }
            p { class: "muted",
                if let Some(handle) = recipient() { "Chat goes to {handle}." }
                else { "Nobody is registered to receive chat. Your question is queued." }
                " Questions go after a day without activity."
            }
            if let Some(why) = trouble() { p { role: "status", "Questions could not be refreshed: {why}" } }
            NewAsk { path: path.clone(), scope: scope.clone(), on_sent: move |_| refresh += 1 }
            for ask in asks() {
                AskThread { key: "{ask.id}", ask, scope: scope.clone(), render_blocks, on_sent: move |_| refresh += 1 }
            }
        }
    }
}

#[component]
fn NewAsk(path: String, scope: String, on_sent: EventHandler<()>) -> Element {
    let draft_key = format!("review-ask:{scope}/{path}");
    let saved = draft_key.clone();
    let mut text = use_signal(move || platform::current().load(&saved).unwrap_or_default());
    let mut anchor = use_signal(String::new);
    let mut quote = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut trouble = use_signal(|| None::<String>);
    let mut attach = crate::attach::use_attach(draft_key.clone());
    let input_key = draft_key.clone();
    // The question waits while the page may show what is out of date; the draft stays editable.
    let stale = crate::cache::STALE();
    let checking = crate::words::CHECKING;
    rsx! {
        crate::components::button::Button { r#type: "button", onmousedown: move |event: MouseEvent| event.prevent_default(),
            onclick: move |_| {
                let (picked_anchor, picked_quote) = selection();
                anchor.set(picked_anchor);
                quote.set(picked_quote);
            },
            "Use selected passage"
        }
        if !quote().is_empty() || !anchor().is_empty() {
            blockquote {
                if !anchor().is_empty() { small { "#{anchor()}" } }
                "{quote()}"
            }
            crate::components::button::Button { r#type: "button", onclick: move |_| { anchor.set(String::new()); quote.set(String::new()); }, "Clear passage" }
        }
        form {
            onsubmit: move |event| {
                event.prevent_default();
                if sending() || *crate::cache::STALE.peek() || attach.busy() || text().trim().is_empty() && attach.is_empty() { return; }
                sending.set(true);
                trouble.set(None);
                let (path, scope, key) = (path.clone(), scope.clone(), draft_key.clone());
                let (question, anchor, quote) = (attach.compose(&text()), anchor(), quote());
                spawn(async move {
                    match post(&format!("{scope}/api/ask"), &[("path", &path), ("question", &question), ("anchor", &anchor), ("quote", &quote)]).await {
                        Ok(_) => { text.set(String::new()); attach.clear(); platform::current().save(&key, ""); on_sent.call(()); }
                        Err(why) => trouble.set(Some(why)),
                    }
                    sending.set(false);
                });
            },
            "data-attach": attach.zone(),
            label { r#for: "new-ask", "Question" }
            crate::components::textarea::Textarea { id: "new-ask", value: "{text}", maxlength: 4000, required: attach.is_empty(), disabled: sending(),
                oninput: move |event: FormEvent| {
                    text.set(event.value());
                    platform::current().save(&input_key, &text());
                },
            }
            crate::attach::AttachControls { attach, disabled: sending() }
            if let Some(why) = trouble() { p { role: "alert", "{why}" } }
            crate::components::button::Button { r#type: "submit", disabled: sending() || stale || attach.busy() || text().trim().is_empty() && attach.is_empty(),
                if sending() { "Sending…" } else if stale { "{checking}" } else { "Ask" }
            }
        }
    }
}

#[component]
fn AskThread(
    ask: Ask,
    scope: String,
    render_blocks: Callback<Vec<Block>, Element>,
    on_sent: EventHandler<()>,
) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    let key = format!("review-ask-reply:{}", ask.id);
    let saved = key.clone();
    let input_key = key.clone();
    let mut text = use_signal(move || platform::current().load(&saved).unwrap_or_default());
    let mut sending = use_signal(|| false);
    let mut trouble = use_signal(|| None::<String>);
    let mut attach = crate::attach::use_attach(key.clone());
    // The follow-up waits while the page may show what is out of date; the draft stays editable.
    let stale = crate::cache::STALE();
    let checking = crate::words::CHECKING;
    rsx! {
        article { class: "ask-thread card", id: "ask-{ask.id}",
            "data-deferred": !shown(),
            ..reveal,
            if shown() {
            p { class: "muted", "Question {ask.id} · {ask.status}" }
            if !ask.anchor.is_empty() { a { href: "#{ask.anchor}", "#{ask.anchor}" } }
            if !ask.quote.is_empty() { blockquote { "{ask.quote}" } }
            for message in ask.messages {
                div { key: "{message.seq}", class: "ask-message",
                    p { class: "by", "{message.by}" }
                    {render_blocks.call(message.blocks)}
                }
            }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if sending() || *crate::cache::STALE.peek() || attach.busy() || text().trim().is_empty() && attach.is_empty() { return; }
                    sending.set(true);
                    trouble.set(None);
                    let (scope, key, message) = (scope.clone(), key.clone(), attach.compose(&text()));
                    spawn(async move {
                        match post(&format!("{scope}/api/ask-reply"), &[("id", &ask.id.to_string()), ("text", &message)]).await {
                            Ok(_) => { text.set(String::new()); attach.clear(); platform::current().save(&key, ""); on_sent.call(()); }
                            Err(why) => trouble.set(Some(why)),
                        }
                        sending.set(false);
                    });
                },
                "data-attach": attach.zone(),
                label { r#for: "reply-{ask.id}", "Follow up" }
                crate::components::textarea::Textarea { id: "reply-{ask.id}", value: "{text}", maxlength: 4000, required: attach.is_empty(), disabled: sending(),
                    oninput: move |event: FormEvent| {
                        text.set(event.value());
                        platform::current().save(&input_key, &text());
                    },
                }
                crate::attach::AttachControls { attach, disabled: sending() }
                if let Some(why) = trouble() { p { role: "alert", "{why}" } }
                crate::components::button::Button { r#type: "submit", disabled: sending() || stale || attach.busy() || text().trim().is_empty() && attach.is_empty(),
                    if sending() { "Sending…" } else if stale { "{checking}" } else { "Send follow-up" }
                }
            }
            }
        }
    }
}

/// A selected quote and the last document heading before it, or the current hash for a question
/// about a whole section. Selection outside the document (including a form) is never attached.
fn selection() -> (String, String) {
    let Some(window) = web_sys::window() else {
        return Default::default();
    };
    let mut anchor = window
        .location()
        .hash()
        .unwrap_or_default()
        .trim_start_matches('#')
        .to_string();
    let Some(selection) = window.get_selection().ok().flatten() else {
        return (anchor, String::new());
    };
    let quote = selection.to_string().as_string().unwrap_or_default();
    let Some(node) = selection.anchor_node() else {
        return (anchor, String::new());
    };
    let element = node
        .dyn_ref::<web_sys::Element>()
        .cloned()
        .or_else(|| node.parent_element());
    let Some(element) = element else {
        return (anchor, String::new());
    };
    if element
        .closest(".asks, input, textarea")
        .ok()
        .flatten()
        .is_some()
    {
        return Default::default();
    }
    let Some(main) = element.closest("main").ok().flatten() else {
        return Default::default();
    };
    if let Ok(headings) = main.query_selector_all("h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]")
    {
        for index in 0..headings.length() {
            if let Some(heading) = headings
                .item(index)
                .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                && (heading.contains(Some(&node))
                    || heading.compare_document_position(&node)
                        & web_sys::Node::DOCUMENT_POSITION_FOLLOWING
                        != 0)
            {
                anchor = heading.id();
            }
        }
    }
    (anchor, quote.chars().take(4000).collect())
}

fn encode(value: &str) -> String {
    js_sys::encode_uri_component(value)
        .as_string()
        .unwrap_or_default()
}
