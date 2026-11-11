//! The persistent chat and quick-task composer owns its draft through save confirmation.

use super::{Draft, save_draft};
use crate::{
    block_view::BlocksView,
    docs,
    live::sleep,
    navigation::Route,
    request::{field, get, post},
};
use dioxus::core::Task;
use dioxus::prelude::*;
use dioxus_primitives::{dioxus_attributes::attributes, merge_attributes};
use serde::Deserialize;
use symphony_api::Ask;
use wasm_bindgen::JsValue;

#[derive(Clone, Deserialize)]
struct AskReply {
    ask: Ask,
    #[serde(default)]
    recipient: Option<String>,
}

/// Keep the owner's composer mounted while an uncertain task save is checked.
#[component]
pub(super) fn QuickPanel(route: Route, mut draft: Signal<Draft>) -> Element {
    let mut thread = use_signal(|| None::<AskReply>);
    let mut sending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut read_error = use_signal(|| None::<String>);
    let mut created = use_signal(|| None::<String>);
    let mut checking = use_signal(|| None::<String>);
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
    // The page's feed says when this thread moves, the chat recipient changes or a stream opens
    // again; the open chat reads its thread then, never on a timer. A failed read tries again
    // until one answers.
    let heard = crate::frame::use_asks_heard(move |news| {
        let current = draft.peek();
        current.open && !current.task && current.ask > 0 && news.concerns(Some(current.ask), None)
    });
    use_effect(use_reactive!(|open, task, id, scope| {
        let _ = (revision(), heard());
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
                        return;
                    }
                    Err(why) => read_error.set(Some(why)),
                }
                sleep(3_000).await;
            }
        })));
    }));
    let send = move |event: FormEvent| {
        event.prevent_default();
        let mut d = draft();
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
        checking.set(None);
        if d.task {
            created.set(None);
            if d.request.is_empty() {
                let identity = (|| {
                    let mut bytes = [0u8; 16];
                    web_sys::window()
                        .ok_or_else(|| JsValue::from_str("No window"))?
                        .crypto()?
                        .get_random_values_with_u8_array(&mut bytes)?;
                    Ok::<_, JsValue>(
                        bytes
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>(),
                    )
                })();
                match identity {
                    Ok(identity) => {
                        d.request = identity.clone();
                        d.request_scope.clone_from(&route.scope);
                        let mut saved = draft.write();
                        saved.request = identity;
                        saved.request_scope.clone_from(&route.scope);
                    }
                    Err(why) => {
                        error.set(Some(crate::live::describe(why)));
                        sending.set(false);
                        return;
                    }
                }
            }
        }
        let route = route.clone();
        spawn(async move {
            let result = if d.task {
                let page = crate::project::page().unwrap_or_default();
                // Document tasks keep the canonical page URL and its anchor.
                let page = if !d.request_page.is_empty() {
                    d.request_page.clone()
                } else if route.document && !route.folder {
                    let hash = web_sys::window()
                        .and_then(|w| w.location().hash().ok())
                        .unwrap_or_default();
                    docs::scoped(&route.scope, &format!("/{}{hash}", route.path))
                } else {
                    page
                };
                draft.write().request_page.clone_from(&page);
                // A reload must retain the same identity and page before this request can run.
                save_draft(&draft());
                let url = docs::scoped(&d.request_scope, "/api/quick-task");
                let fields = [
                    ("request", d.request.into()),
                    ("title", d.title.into()),
                    ("body", attach.compose(&d.text).into()),
                    ("slug", d.slug.into()),
                    ("priority", d.priority.parse::<f64>().unwrap_or(2.0).into()),
                    ("page", page.into()),
                ];
                loop {
                    match crate::request::task_post(&url, &fields).await {
                        Ok(crate::task_save::Outcome::Saved(slug)) => {
                            break Ok(crate::request::object(&[("slug", slug.into())]));
                        }
                        Ok(crate::task_save::Outcome::Checking(why)) => checking.set(Some(why)),
                        Err(crate::task_save::Failure::Uncertain(why)) => checking.set(Some(why)),
                        Err(why) => break Err(why.into_text()),
                    }
                    sleep(1_000).await;
                }
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
                        saved.request.clear();
                        saved.request_page.clear();
                        saved.request_scope.clear();
                        created.set(field(&value, "slug").as_string());
                    } else if saved.ask == 0 {
                        saved.ask = field(&value, "id").as_f64().unwrap_or_default() as i64;
                        saved.ask_scope = route.scope;
                    }
                    revision += 1;
                }
                Err(why) => {
                    draft.write().request.clear();
                    draft.write().request_page.clear();
                    draft.write().request_scope.clear();
                    error.set(Some(why));
                }
            }
            checking.set(None);
            sending.set(false);
        });
    };
    let busy = sending() || !current.request.is_empty();
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
                    } else if let Some(why) = checking() { p { role: "status", "Checking save… {why}" } }
                    else if !current.request.is_empty() { p { role: "status", "Check the previous save to continue; your text is kept." } }
                    else if let Some(slug) = created() { p { role: "status", "✓ Task " a { href: "/task/{slug}", "{slug}" } " created" } }
                }
                form { onsubmit: send.clone(), "data-attach": attach.zone(),
                    if current.task {
                        label { r#for: "quick-title", "Title" }
                        crate::components::input::Input { id: "quick-title", value: "{current.title}", required: true, maxlength: symphony_api::QUICK_TASK_TITLE_MAX, aria_describedby: "quick-title-limit", disabled: busy, oninput: move |e: FormEvent| draft.write().title = e.value() }
                        p { id: "quick-title-limit", role: "status",
                            "{current.title.trim().chars().count()} / {symphony_api::QUICK_TASK_TITLE_MAX} characters"
                        }
                    }
                    label { r#for: "quick-text", if current.task { "Details" } else { "Message" } }
                    crate::components::textarea::Textarea { id: "quick-text", rows: 3, value: "{current.text}", required: !current.task, maxlength: 4000, disabled: busy, oninput: move |e: FormEvent| draft.write().text = e.value() }
                    if current.task {
                        div { class: "row",
                            label { "Slug (optional)" crate::components::input::Input { value: "{current.slug}", disabled: busy, oninput: move |e: FormEvent| draft.write().slug = e.value() } }
                            label { "Priority" crate::components::select::SelectField {
                                label: "Priority", value: current.priority.clone(), disabled: busy,
                                options: (0..=4).map(|priority| (priority.to_string(), format!("p{priority}"))).collect::<Vec<_>>(),
                                on_change: move |value| draft.write().priority = value,
                            } }
                        }
                    }
                    crate::attach::AttachControls { attach, disabled: busy }
                    if let Some(why) = error() { p { role: "alert", "{why}" } }
                    crate::components::button::Button { r#type: "submit", disabled: sending() || attach.busy() || !current.task && current.text.trim().is_empty() && attach.is_empty() || current.task && current.title.trim().is_empty(), if sending() { "Sending…" } else if !current.request.is_empty() { "Check save" } else { "Send" } }
                }
        }
    };
    rsx! {
        aside { id: "quick-sheet", hidden: !current.open, "aria-label": "Chat or add a task", "data-busy": busy || !current.title.is_empty() || !current.text.is_empty() || !attach.is_empty() || attach.busy(), "data-posting": sending(),
            crate::theme::ThemeMenu {}
            crate::components::tabs::Tabs {
                disabled: busy,
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
