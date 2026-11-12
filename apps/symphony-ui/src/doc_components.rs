//! The document reader's semantic components. Their markup comes with the client; the API supplies
//! content and state. Keeping forms outside these components preserves drafts as prose changes.

use dioxus::prelude::*;

/// Owns the task panel's placement for standalone documents and folder introductions alike.
/// The reader supplies its content; this component alone places related work beside it.
#[component]
pub fn DocumentWithTasks(tasks: Vec<symphony_api::DocumentTask>, children: Element) -> Element {
    rsx! {
        div { class: "document-content-layout",
            div { class: "document-content", {children} }
            if !tasks.is_empty() {
                aside { class: "doc-tasks", "aria-label": "Related tasks",
                    h2 { "Related tasks" }
                    ul {
                        for task in tasks {
                            crate::viewport::NearViewport { key: "{task.slug}", height: 40, list_item: true,
                                a { href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" }
                                span { class: "doc-task-status", "{task.status}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn TaskTag(task: Option<String>) -> Element {
    rsx! { if let Some(task) = task { a { class: "chip", href: crate::project::href(&format!("/task/{task}")), "{task}" } } }
}

#[component]
pub fn ComparisonNote(comparison: symphony_api::DocumentComparison) -> Element {
    rsx! { aside { class: "document-comparison", "Differences from main belong to "
        a { href: crate::project::href(&format!("/task/{}", comparison.task)), "{comparison.task}" }
    } }
}

#[cfg(any(not(target_arch = "wasm32"), feature = "page-docs"))]
#[component]
pub fn SectionDifference(
    comparison: Option<symphony_api::SectionComparison>,
    children: Element,
) -> Element {
    let Some(comparison) = comparison else {
        return children;
    };
    let (kind, label) = match comparison.kind {
        symphony_api::Difference::Added => ("added", "Added"),
        symphony_api::Difference::Changed => ("changed", "Changed"),
        symphony_api::Difference::Removed => ("removed", "Removed"),
    };
    rsx! { div { class: "section-difference difference-{kind}", "data-difference": kind,
        p { class: "difference-label", title: "Compared with main", "{label}" }
        if !comparison.before.is_empty() {
            details { class: "difference-before",
                summary { "Previously on main" }
                crate::viewport::NearViewport { crate::block_view::HistoricalBlocks { blocks: comparison.before } }
            }
        }
        {children}
    } }
}

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
    #[props(default = true)] anchor: bool,
) -> Element {
    rsx! {
        div { class: "decision", id: if anchor { Some(slug.clone()) } else { None }, "data-slug": slug.clone(),
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
