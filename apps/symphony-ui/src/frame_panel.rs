//! Interactive frame controls, compiled only into the persistent deferred module.
use super::{publish, snapshot, use_revision};
use crate::{
    block_view::BlocksView,
    docs,
    live::sleep,
    navigation::Route,
    platform,
    request::{field, get, post},
};
use dioxus::core::Task;
use dioxus::prelude::*;
use dioxus_primitives::{dioxus_attributes::attributes, merge_attributes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use symphony_api::{Activity, ActivityItem, Ask, Pending, PendingItem};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
const STYLE: Asset = asset!(
    "/assets/frame.css",
    AssetOptions::css().with_static_head(true)
);

fn restore<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    serde_wasm_bindgen::from_value(snapshot(key)).ok()
}
fn remember<T: Serialize>(key: &str, value: &T) {
    if let Some(window) = web_sys::window()
        && let Ok(value) = serde_wasm_bindgen::to_value(value)
    {
        let _ = js_sys::Reflect::set(&window, &key.into(), &value);
    }
}

#[component]
fn FrameExtras(route: Route, mut draft: Signal<Draft>) -> Element {
    let revision = use_revision(&["review:frame"]);
    let opened = use_revision(&["review:activity"]);
    let mut activity = use_signal(Vec::<ActivityItem>::new);
    let mut pending = use_signal(|| Pending {
        items: vec![],
        docs: vec![],
    });
    let mut seen = use_signal(|| {
        platform::current()
            .load("docs-review-seen")
            .and_then(|s| js_sys::JSON::parse(&s).ok())
            .and_then(|v| serde_wasm_bindgen::from_value::<BTreeSet<String>>(v).ok())
    });
    let mut known = use_signal(|| restore::<BTreeSet<String>>("reviewPreviousIds"));
    let mut toasts = use_signal(Vec::<ActivityItem>::new);
    let mut panel = use_signal(|| snapshot("reviewActivityOpen").as_bool() == Some(true));
    let mut trouble = use_signal(|| None::<String>);
    use_effect(move || {
        let _ = opened();
        let next = snapshot("reviewActivityOpen").as_bool() == Some(true);
        if next != *panel.peek() {
            panel.set(next);
        }
        if next && draft.peek().open {
            draft.write().open = false;
        }
    });
    use_effect(move || {
        if let Some(window) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &window,
                &"reviewActivityOpen".into(),
                &JsValue::from_bool(panel()),
            );
            publish("review:activity");
        }
    });
    use_effect(move || {
        let _ = revision();
        if let Some(why) = snapshot("reviewFrameError").as_string() {
            trouble.set(Some(why));
            return;
        }
        let got = restore::<Activity>("reviewActivitySnapshot");
        let cards = restore::<Pending>("reviewPendingSnapshot");
        if let (Some(got), Some(cards)) = (got, cards) {
            let ids: BTreeSet<_> = got.items.iter().map(|i| i.id.clone()).collect();
            if seen.peek().is_none() {
                seen.set(Some(ids.clone()));
                save_seen(&ids);
            }
            if let Some(previous) = known.peek().as_ref() {
                let added: Vec<_> = got
                    .items
                    .iter()
                    .filter(|i| !previous.contains(&i.id) && !is_read(i, &seen.read()))
                    .cloned()
                    .collect();
                if !added.is_empty() {
                    toasts.write().extend(added);
                    let keep = toasts.peek().len().saturating_sub(3);
                    toasts.write().drain(..keep);
                    spawn(async move {
                        sleep(7_000).await;
                        toasts.set(vec![]);
                    });
                }
            }
            known.set(Some(ids));
            activity.set(got.items);
            pending.set(cards);
            trouble.set(None);
        }
    });
    rsx! {
        if panel() { SuspenseBoundary { fallback: |_| rsx! { p { "Loading activity…" } },
            ActivityPanel { panel, activity, seen, trouble }
        } }
        div { id: "toasts", "aria-live": "polite",
            for item in toasts() {
                div { class: "pop", key: "{item.id}",
                    a { href: item.href, onclick: { let id = item.id.clone(); move |_| mark_seen(seen, &id) }, "{item.kind} · {item.slug}" }
                    crate::components::button::Button { r#type: "button", "aria-label": "Dismiss notification", onclick: { let id = item.id.clone(); move |_| toasts.write().retain(|i| i.id != id) }, "×" }
                }
            }
        }
        PendingControls { route, pending: pending() }
    }
}
#[component(lazy)]
fn ActivityPanel(
    mut panel: Signal<bool>,
    activity: Signal<Vec<ActivityItem>>,
    seen: Signal<Option<BTreeSet<String>>>,
    trouble: Signal<Option<String>>,
) -> Element {
    rsx! { aside { id: "activity-panel", "aria-label": "Recent activity",
        div { class: "panel-heading", h2 { "Recent activity" } crate::components::button::Button { r#type: "button", "aria-label": "Close activity", onclick: move |_| panel.set(false), "×" } }
        if let Some(why) = trouble() { p { role: "status", "Activity could not be refreshed: {why}" } }
        if activity.read().is_empty() { p { class: "muted", "No recent activity." } }
        ul { for item in activity() {
            li { key: "{item.id}", class: if is_read(&item, &seen.read()) { "" } else { "unread" },
                a { href: item.href.clone(), onclick: move |_| { mark_seen(seen, &item.id); panel.set(false); }, "{item.kind} · {item.slug}" }
                div { class: "meta", "{item.path} · {item.ts}" }
                div { class: "excerpt", "{item.text}" }
            }
        } }
    } }
}

fn is_read(item: &ActivityItem, seen: &Option<BTreeSet<String>>) -> bool {
    item.answered || seen.as_ref().is_some_and(|s| s.contains(&item.id))
}

fn save_seen(seen: &BTreeSet<String>) {
    if let Ok(value) = serde_wasm_bindgen::to_value(seen)
        && let Ok(json) = js_sys::JSON::stringify(&value)
    {
        platform::current().save("docs-review-seen", &String::from(json));
    }
}

fn mark_seen(mut seen: Signal<Option<BTreeSet<String>>>, id: &str) {
    seen.write().get_or_insert_default().insert(id.into());
    if let Some(ids) = seen.peek().as_ref() {
        save_seen(ids);
        publish("review:frame-seen");
    }
}

#[component]
fn PendingControls(route: Route, pending: Pending) -> Element {
    let mut targets = use_signal(|| (None::<PendingItem>, None::<PendingItem>));
    let mut running = use_signal(|| None::<Task>);
    use_effect(use_reactive!(|route, pending| {
        if let Some(task) = running.take() {
            task.cancel();
        }
        running.set(Some(spawn(async move {
            loop {
                let next = pending_targets(&route, &pending);
                if *targets.peek() != next {
                    targets.set(next);
                }
                sleep(200).await;
            }
        })));
    }));
    let keys = use_hook(move || {
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Some(key) = event.dyn_ref::<web_sys::KeyboardEvent>() else {
                return;
            };
            if key.alt_key() || key.ctrl_key() || key.meta_key() || key.shift_key() {
                return;
            }
            if event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|e| {
                    e.closest("input, textarea, select, [contenteditable], dialog[open],[role=dialog],[role=listbox],[role=menu]")
                        .ok()
                        .flatten()
                })
                .is_some()
            {
                return;
            }
            let target = match key.key().as_str() {
                "[" => targets.peek().0.clone(),
                "]" => targets.peek().1.clone(),
                _ => None,
            };
            if let Some(target) = target {
                event.prevent_default();
                go_pending(&target);
            }
        });
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        std::rc::Rc::new(handler)
    });
    use_drop(move || {
        if let Some(w) = web_sys::window() {
            let _ = w.remove_event_listener_with_callback(
                "keydown",
                keys.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    let (prev, next) = targets();
    rsx! { if !pending.items.is_empty() {
        nav { class: "pending-nav frame-pending", "aria-label": "Cards waiting on you",
            crate::components::button::Button { r#type: "button", "data-pending-action": "prev", disabled: prev.is_none(), title: "Previous pending decision ([)", onclick: move |_| if let Some(i) = &prev { go_pending(i); }, "← prev" }
            crate::components::button::Button { r#type: "button", "data-pending-action": "next", disabled: next.is_none(), title: "Next pending decision (])", onclick: move |_| if let Some(i) = &next { go_pending(i); }, "next →" }
            a { href: "#top", "↑ top" }
        }
    } }
}

fn pending_targets(route: &Route, pending: &Pending) -> (Option<PendingItem>, Option<PendingItem>) {
    let Some(window) = web_sys::window() else {
        return (None, None);
    };
    let Some(doc) = window.document() else {
        return (None, None);
    };
    let hash = window.location().hash().unwrap_or_default();
    let anchor = js_sys::decode_uri_component(hash.trim_start_matches('#'))
        .map(String::from)
        .unwrap_or_default();
    let rank = |path: &str| pending.docs.iter().position(|p| p == path);
    let path = if route.scope.is_empty() {
        route.path.clone()
    } else {
        format!("@{}/{}", route.scope, route.path)
    };
    let mine: Vec<_> = pending.items.iter().filter(|i| i.path == path).collect();
    let top = |i: &&PendingItem| {
        doc.get_element_by_id(&i.slug)
            .map(|e| e.get_bounding_client_rect().top())
    };
    // The last card may not reach the viewport top. Keep the hash as the cursor until the
    // reader scrolls away from the destination, then use the reading position again.
    let at = mine.iter().position(|i| {
        i.slug == anchor
            && top(i).is_some_and(|y| {
                y > 0.0
                    && y < window
                        .inner_height()
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(800.0)
            })
    });
    let prev = if let Some(at) = at {
        at.checked_sub(1).map(|i| mine[i])
    } else {
        mine.iter()
            .rev()
            .find(|i| top(i).is_some_and(|y| y < 40.0))
            .copied()
    }
    .or_else(|| {
        pending
            .items
            .iter()
            .rev()
            .find(|i| rank(&i.path) < rank(&path))
    });
    let next = if let Some(at) = at {
        mine.get(at + 1).copied()
    } else {
        mine.iter()
            .find(|i| top(i).is_some_and(|y| y > 100.0))
            .copied()
    }
    .or_else(|| pending.items.iter().find(|i| rank(&i.path) > rank(&path)));
    (prev.cloned(), next.cloned())
}

fn go_pending(item: &PendingItem) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        // Clicking a link lets the existing navigation adapter preserve the client shell.
        if let Ok(link) = doc.create_element("a") {
            let _ = link.set_attribute("href", &item.href);
            if let Some(body) = doc.body() {
                let _ = body.append_child(&link);
                if let Some(link) = link.dyn_ref::<web_sys::HtmlElement>() {
                    link.click();
                }
                link.remove();
            }
        }
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    open: bool,
    task: bool,
    title: String,
    text: String,
    slug: String,
    priority: String,
    ask: i64,
    ask_scope: String,
}

fn load_draft() -> Draft {
    if let Some(draft) = restore("reviewQuickDraft") {
        return draft;
    }
    platform::current()
        .load("review-quick-draft")
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .and_then(|v| serde_wasm_bindgen::from_value(v).ok())
        .unwrap_or_else(|| Draft {
            priority: "2".into(),
            ..Draft::default()
        })
}

fn save_draft(draft: &Draft) {
    remember("reviewQuickDraft", draft);
    if let Ok(value) = serde_wasm_bindgen::to_value(draft)
        && let Ok(json) = js_sys::JSON::stringify(&value)
    {
        platform::current().save("review-quick-draft", &String::from(json));
    }
}

/// The draft is typed data shared with the persistent panel module, never rendered HTML. Both
/// sides compare before setting so publishing a change cannot make an event feedback loop.
fn use_draft() -> Signal<Draft> {
    use futures_util::StreamExt;
    let mut draft = use_signal(load_draft);
    let adapter = use_hook(move || {
        let (send, mut recv) = futures_channel::mpsc::unbounded::<()>();
        spawn(async move {
            while recv.next().await.is_some() {
                let next = load_draft();
                if *draft.peek() != next {
                    draft.set(next);
                }
            }
        });
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = send.unbounded_send(());
        });
        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("review:quick", handler.as_ref().unchecked_ref());
        }
        std::rc::Rc::new(handler)
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "review:quick",
                adapter.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    use_effect(move || {
        save_draft(&draft());
        crate::live::flush_reload();
        if let Some(window) = web_sys::window()
            && let Ok(event) = web_sys::Event::new("review:quick")
        {
            let _ = window.dispatch_event(&event);
        }
    });
    draft
}

/// A separate mount owned by the shell. It remains mounted while page-kind runtimes change,
/// keeping the conversation, task form and scroll position intact without a second feed.
#[cfg(feature = "page-panel")]
#[component]
pub fn PanelApp() -> Element {
    std::hint::black_box(&STYLE);
    let route = crate::navigation::use_navigation();
    let draft = use_draft();
    rsx! { div { class: "app-frame",
        crate::components::deferred::OverlayHost {}
        FrameExtras { route: route(), draft }
        // The board itself has no media. When its conversation is mounted, that persistent
        // panel owns media there; every other page kind owns its own adapter instead.
        if route().path == "tasks" { crate::media_live::MediaRuntime {} }
        QuickPanel { route: route(), draft }
    } }
}

#[cfg(feature = "page-panel")]
#[derive(Clone, Deserialize)]
struct AskReply {
    ask: Ask,
    #[serde(default)]
    recipient: Option<String>,
}

#[cfg(feature = "page-panel")]
#[component]
fn QuickPanel(route: Route, mut draft: Signal<Draft>) -> Element {
    let mut thread = use_signal(|| None::<AskReply>);
    let mut sending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut read_error = use_signal(|| None::<String>);
    let mut created = use_signal(|| None::<String>);
    let mut revision = use_signal(|| 0_u64);
    let mut running = use_signal(|| None::<Task>);
    let mut attach = crate::attach::use_attach("quick".into());
    let current = draft();
    let (open, task, id, scope) = (
        current.open,
        current.task,
        current.ask,
        current.ask_scope.clone(),
    );
    use_effect(use_reactive!(|open, task, id, scope| {
        let _ = revision();
        if let Some(task) = running.take() {
            task.cancel();
        }
        if !open || task || id == 0 {
            return;
        }
        running.set(Some(spawn(async move {
            loop {
                match get::<AskReply>(&docs::scoped(
                    &scope,
                    &format!("/api/ask?format=blocks&id={id}"),
                ))
                .await
                {
                    Ok(got) => {
                        thread.set(Some(got));
                        read_error.set(None);
                    }
                    Err(why) => read_error.set(Some(why)),
                }
                sleep(3_000).await;
            }
        })));
    }));
    let send = move |event: FormEvent| {
        event.prevent_default();
        let d = draft();
        if sending()
            || attach.busy()
            || !d.task && d.text.trim().is_empty() && attach.is_empty()
            || d.task && d.title.trim().is_empty()
        {
            return;
        }
        if d.task && d.title.trim().chars().count() > symphony_api::QUICK_TASK_TITLE_MAX {
            error.set(Some(format!(
                "Use at most {} characters for the task title",
                symphony_api::QUICK_TASK_TITLE_MAX
            )));
            return;
        }
        sending.set(true);
        error.set(None);
        let route = route.clone();
        spawn(async move {
            let result = if d.task {
                let page = web_sys::window()
                    .and_then(|w| {
                        w.location().pathname().ok().map(|path| {
                            format!("{path}{}", w.location().hash().unwrap_or_default())
                        })
                    })
                    .unwrap_or_default();
                // The legacy quick-add API recognizes canonical document paths when linking a
                // task to its document; the client URL has an additional /app prefix.
                let page = if route.document && !route.folder {
                    let hash = web_sys::window()
                        .and_then(|w| w.location().hash().ok())
                        .unwrap_or_default();
                    docs::scoped(&route.scope, &format!("/{}{hash}", route.path))
                } else {
                    page
                };
                post(
                    &docs::scoped(&route.scope, "/api/quick-task"),
                    &[
                        ("title", d.title.into()),
                        ("body", attach.compose(&d.text).into()),
                        ("slug", d.slug.into()),
                        ("priority", d.priority.parse::<f64>().unwrap_or(2.0).into()),
                        ("page", page.into()),
                    ],
                )
                .await
            } else if d.ask > 0 {
                post(
                    &docs::scoped(&d.ask_scope, "/api/ask-reply"),
                    &[
                        ("id", (d.ask as f64).into()),
                        ("text", attach.compose(&d.text).into()),
                    ],
                )
                .await
            } else {
                let page = web_sys::window()
                    .and_then(|w| w.location().href().ok())
                    .unwrap_or_default();
                let (path, question) = if route.document && !route.folder {
                    (route.path.clone(), attach.compose(&d.text))
                } else {
                    (
                        crate::project::current().readme_route(),
                        attach.compose(&format!("{}\n\n(sent from {page})", d.text)),
                    )
                };
                let anchor = web_sys::window()
                    .and_then(|w| w.location().hash().ok())
                    .unwrap_or_default();
                let anchor = js_sys::decode_uri_component(anchor.trim_start_matches('#'))
                    .map(String::from)
                    .unwrap_or_default();
                post(
                    &docs::scoped(&route.scope, "/api/ask"),
                    &[
                        ("path", path.into()),
                        ("question", question.into()),
                        ("anchor", anchor.into()),
                    ],
                )
                .await
            };
            match result {
                Ok(value) => {
                    attach.clear();
                    let mut saved = draft.write();
                    saved.text.clear();
                    if saved.task {
                        saved.title.clear();
                        saved.slug.clear();
                        created.set(field(&value, "slug").as_string());
                    } else if saved.ask == 0 {
                        saved.ask = field(&value, "id").as_f64().unwrap_or_default() as i64;
                        saved.ask_scope = route.scope;
                    }
                    revision += 1;
                }
                Err(why) => error.set(Some(why)),
            }
            sending.set(false);
        });
    };
    let content = || {
        rsx! {
                div { class: "quick-thread", "aria-live": "polite",
                    if let Some(why) = read_error() { p { role: "status", "Conversation could not be refreshed: {why}" } }
                    if !current.task {
                        if current.ask > 0 {
                            if let Some(reply) = thread.read().as_ref().filter(|r| r.ask.id == current.ask) {
                                for message in &reply.ask.messages {
                                    article { class: "quick-message", key: "{message.seq}",
                                        p { class: "by", "{message.by}" }
                                        BlocksView { blocks: message.blocks.clone() }
                                    }
                                }
                                if reply.recipient.is_some() || reply.ask.status != "answered" {
                                    p { class: "muted",
                                        if let Some(handle) = &reply.recipient { "Chat goes to {handle}." }
                                        else { "Nobody is registered to receive chat; your message is queued." }
                                    }
                                }
                            } else { p { role: "status", "Loading conversation…" } }
                            crate::components::button::Button { r#type: "button", disabled: sending(), onclick: move |_| { draft.write().ask = 0; thread.set(None); }, "New conversation" }
                        } else { p { class: "muted", "Ask about this page." } }
                    } else if let Some(slug) = created() { p { role: "status", "✓ Task " a { href: "/task/{slug}", "{slug}" } " created" } }
                }
                form { onsubmit: send.clone(), "data-attach": attach.zone(),
                    if current.task {
                        label { r#for: "quick-title", "Title" }
                        crate::components::input::Input { id: "quick-title", value: "{current.title}", required: true, maxlength: symphony_api::QUICK_TASK_TITLE_MAX, aria_describedby: "quick-title-limit", disabled: sending(), oninput: move |e: FormEvent| draft.write().title = e.value() }
                        p { id: "quick-title-limit", role: "status",
                            "{current.title.trim().chars().count()} / {symphony_api::QUICK_TASK_TITLE_MAX} characters"
                        }
                    }
                    label { r#for: "quick-text", if current.task { "Details" } else { "Message" } }
                    crate::components::textarea::Textarea { id: "quick-text", rows: 3, value: "{current.text}", required: !current.task, maxlength: 4000, disabled: sending(), oninput: move |e: FormEvent| draft.write().text = e.value() }
                    if current.task {
                        div { class: "row",
                            label { "Slug (optional)" crate::components::input::Input { value: "{current.slug}", disabled: sending(), oninput: move |e: FormEvent| draft.write().slug = e.value() } }
                            label { "Priority" crate::components::select::SelectField {
                                label: "Priority", value: current.priority.clone(), disabled: sending(),
                                options: (0..=4).map(|priority| (priority.to_string(), format!("p{priority}"))).collect::<Vec<_>>(),
                                on_change: move |value| draft.write().priority = value,
                            } }
                        }
                    }
                    crate::attach::AttachControls { attach, disabled: sending() }
                    if let Some(why) = error() { p { role: "alert", "{why}" } }
                    crate::components::button::Button { r#type: "submit", disabled: sending() || attach.busy() || !current.task && current.text.trim().is_empty() && attach.is_empty() || current.task && current.title.trim().is_empty(), if sending() { "Sending…" } else { "Send" } }
                }
        }
    };
    rsx! {
        aside { id: "quick-sheet", hidden: !current.open, "aria-label": "Chat or add a task", "data-busy": sending() || !current.text.is_empty() || !attach.is_empty() || attach.busy(), "data-posting": sending(),
            crate::theme::ThemeMenu {}
            crate::components::tabs::Tabs {
                value: Some(if current.task { "task".to_string() } else { "chat".to_string() }),
                horizontal: true,
                on_value_change: move |value| draft.write().task = value == "task",
                div { class: "tabs",
                    crate::components::tabs::TabList {
                        crate::components::tabs::TabTrigger { id: "quick-chat-tab", value: "chat", index: 0usize, "Chat" }
                        crate::components::tabs::TabTrigger { id: "quick-task-tab", value: "task", index: 1usize, "Add task" }
                    }
                    crate::components::tooltip::Tooltip {
                        crate::components::tooltip::TooltipTrigger {
                            as: move |attributes: Vec<Attribute>| {
                                let base = attributes!(button {
                                    class: "collapse", r#type: "button", aria_label: "Collapse panel",
                                    onclick: move |_: MouseEvent| draft.write().open = false,
                                });
                                let merged = merge_attributes(vec![attributes, base]);
                                rsx! { button { ..merged, "⌄" } }
                            },
                        }
                        crate::components::tooltip::TooltipContent { "Collapse panel; keep the conversation" }
                    }
                }
                crate::components::tabs::TabContent { id: "quick-chat-panel", aria_labelledby: "quick-chat-tab", value: "chat", index: 0usize, {content()} }
                crate::components::tabs::TabContent { id: "quick-task-panel", aria_labelledby: "quick-task-tab", value: "task", index: 1usize, {content()} }
            }
        }
    }
}
