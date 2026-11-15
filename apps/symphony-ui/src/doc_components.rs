//! The blocks' semantic components, which `block_view` draws the panel's blocks with (the
//! documents' and a task's page are the web client's, apps/symphony-web). Their markup comes with
//! the client; the API supplies content and state.

use dioxus::prelude::*;

#[component]
pub fn Heading(level: u8, anchor: String, children: Element) -> Element {
    match level {
        1 => rsx! { h1 { id: anchor, {children} } },
        2 => rsx! { h2 { id: anchor, {children} } },
        3 => rsx! { h3 { id: anchor, {children} } },
        4 => rsx! { h4 { id: anchor, {children} } },
        5 => rsx! { h5 { id: anchor, {children} } },
        _ => rsx! { h6 { id: anchor, {children} } },
    }
}

#[component]
pub fn Code(text: String, language: String, block: bool) -> Element {
    if block {
        rsx! { pre { code { class: "language-{language}", "{text}" } } }
    } else {
        rsx! { code { "{text}" } }
    }
}

#[component]
pub fn Callout(tone: String, title: Element, done: bool, children: Element) -> Element {
    let heading = rsx! {
        span { class: "callout-kind", "{tone}" }
        " "
        {title}
    };
    if done {
        rsx! { details { class: "callout callout-{tone} callout-done",
            summary { class: "callout-title", {heading} }
            {children}
        } }
    } else {
        rsx! { div { class: "callout callout-{tone}",
            p { class: "callout-title", {heading} }
            {children}
        } }
    }
}

#[component]
pub fn Decision(
    slug: String,
    state: String,
    group: String,
    open: bool,
    decided: String,
    posted: String,
    body: Element,
    comparison: Element,
) -> Element {
    rsx! {
        div { class: "decision", id: slug.clone(), "data-slug": slug.clone(),
            "data-state": state, "data-group": group, "data-open": "{open}", "data-decided": decided,
            div { class: "decision-text",
                code { "{slug}" }
                " "
                if !posted.is_empty() { time { class: "posted", "{posted}" } " " }
                {body}
                span { class: "state", "{state}" if !decided.is_empty() { " · {decided}" } }
            }
            {comparison}
        }
    }
}

#[component]
pub fn DocumentList(ordered: bool, start: u64, children: Element) -> Element {
    if ordered {
        rsx! { ol { start: "{start}", {children} } }
    } else {
        rsx! { ul { {children} } }
    }
}

#[component]
pub fn Unknown(description: String, attributes: Vec<String>, children: Element) -> Element {
    rsx! {
        aside { class: "unknown-block", role: "alert",
            strong { "Unsupported document content: {description}" }
            if !attributes.is_empty() {
                p { "Attributes: " code { {attributes.join(", ")} } }
            }
            {children}
        }
    }
}
