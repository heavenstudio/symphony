//! Document and folder pages, forms and the document feed. Typed prose stays in keyed
//! components; drafts live outside that markup and are saved locally on every input event.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::pin::pin;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::web::WebEventExt;
use futures_channel::mpsc::unbounded;
use futures_util::{
    StreamExt,
    future::{Either, select},
};
use js_sys::Array;
use serde::Deserialize;
use symphony_api::{
    Block, Document, DocumentCard, DocumentChange, DocumentDiff, DocumentOutline, DocumentSections,
    Folder, Fragment, LinkTarget, Section, Version,
};
use wasm_bindgen::{JsCast, JsValue};

use crate::{
    block_view::{BlocksView, CardAnchor, Excerpts, SpansView},
    components::button::{Button, ButtonVariant},
    doc_components::DocumentWithTasks,
    docs::{self, DocumentState},
    live::{Feed, Heard, back, mark, pause, sleep},
    navigation::scroll_anchor,
    platform,
    request::{encode, field, get, get_found, post},
    state::Ended,
    viewport::NearViewport,
};

const STYLE: Asset = asset!(
    "/assets/docs.css",
    AssetOptions::css().with_static_head(true)
);

// The original reader reserves a contents column only for three or more headings.
const TOC_MIN_HEADINGS: usize = 3;

fn api(scope: &str, route: &str, path: &str) -> String {
    format!("{}?path={}", docs::scoped(scope, route), encode(path))
}

#[derive(Clone)]
struct DocumentRequests {
    frame: crate::frame::FrameRefresh,
    anchor: Signal<String>,
    order_refreshed: Signal<u64>,
    wanted: Signal<BTreeSet<String>>,
    loaded: Signal<BTreeSet<String>>,
    anchors: Signal<BTreeMap<String, Vec<String>>>,
    cursor: Rc<Cell<u64>>,
    epoch: Rc<Cell<u64>>,
    trouble: Signal<Option<String>>,
    /// The cards the owner just saved in ([`docs::settle`]).
    released: Rc<RefCell<BTreeMap<String, usize>>>,
    /// The page's document, whose latest copy a save reads its card's lines from ([`docs::release`]).
    state: Signal<Option<DocumentState>>,
}

fn keys_query(keys: &BTreeSet<String>) -> String {
    let values = Array::new();
    for key in keys {
        values.push(&key.clone().into());
    }
    js_sys::JSON::stringify(&values)
        .map(String::from)
        .map(|json| encode(&json))
        .unwrap_or_default()
}

/// What holds the shown page now, against `latest` ([`docs::held`]).
fn held(requests: &DocumentRequests, latest: &Document) -> BTreeSet<String> {
    let (engaged, drafts) = busy();
    let loading: Vec<String> = requests
        .wanted
        .peek()
        .difference(&requests.loaded.peek())
        .cloned()
        .collect();
    docs::held(
        &engaged,
        &drafts,
        &requests.released.borrow(),
        loading,
        latest,
    )
}

/// Show all but `held`; a card's own save that now shows ends its release.
fn show(current: &mut DocumentState, requests: &DocumentRequests, held: &BTreeSet<String>) {
    current.refresh(held);
    docs::settle(&mut requests.released.borrow_mut(), &current.shown);
}

#[derive(Clone, Copy)]
struct FolderNotice {
    stale: Signal<bool>,
    removed: Signal<bool>,
    error: Signal<String>,
}

#[component(lazy)]
pub fn DocumentPage(path: String, scope: String, #[props(default)] embedded: bool) -> Element {
    std::hint::black_box(&STYLE);
    let state = use_signal(|| None::<DocumentState>);
    let stale = use_signal(|| true);
    let trouble = use_signal(|| None::<String>);
    // The server has no document at this path (it answered 404), and the reader's Retry presses.
    let missing = use_signal(|| false);
    let mut retry = use_signal(|| 0_u64);
    let folder_notice = try_consume_context::<FolderNotice>();
    use_effect(move || {
        if embedded && let Some(mut notice) = folder_notice {
            // An intro the server no longer has follows no feed: the folder's next read drops it.
            notice.stale.set(stale() && !missing());
            notice.error.set(trouble().unwrap_or_default());
            notice
                .removed
                .set(state.read().as_ref().is_some_and(|state| state.removed));
        }
    });
    let wanted = use_signal(BTreeSet::new);
    let loaded = use_signal(BTreeSet::new);
    let anchors = use_signal(BTreeMap::new);
    let anchor = crate::navigation::use_anchor();
    let mut postponed_cards = use_signal(Vec::<String>::new);
    use_context_provider(|| postponed_cards);
    let order_refreshed = use_signal(|| 0_u64);
    let mut order_generation = use_signal(|| 0_u64);
    let frame = use_context::<crate::frame::FrameRefresh>();
    use_effect(use_reactive!(|scope| {
        let _ = (frame.0)();
        let _ = order_refreshed();
        order_generation += 1;
        let ticket = *order_generation.peek();
        let url = docs::scoped(&scope, "/api/postponed");
        spawn(async move {
            if let Ok(cards) = get::<Vec<String>>(&url).await
                && ticket == *order_generation.peek()
            {
                postponed_cards.set(cards);
            }
        });
    }));
    let requests = use_hook(move || DocumentRequests {
        frame: consume_context(),
        anchor,
        order_refreshed,
        wanted,
        loaded,
        anchors,
        cursor: Rc::new(Cell::new(0)),
        epoch: Rc::new(Cell::new(0)),
        trouble,
        released: Rc::default(),
        state,
    });
    use_context_provider(|| requests.clone());
    let p = path.clone();
    let s = scope.clone();
    let live_requests = requests.clone();
    use_hook(move || {
        spawn(document_live(
            p.clone(),
            s.clone(),
            DocumentRead {
                state,
                stale,
                trouble,
                missing,
                retry,
            },
            live_requests.clone(),
        ));
        spawn(load_sections(p, s, state, live_requests));
    });
    let mut marked = use_signal(|| false);
    let render_blocks = use_callback(|blocks| rsx! { BlocksView { blocks } });
    use_effect(move || {
        let requested = wanted.read();
        let ready = !requested.is_empty() && requested.is_subset(&loaded.read());
        if state.read().is_some() && ready && !marked() {
            marked.set(true);
            scroll_anchor();
            mark("document-ready");
        }
    });
    let ready = if state.read().is_some() {
        if marked() { "document" } else { "outline" }
    } else {
        crate::trouble::ready(missing(), trouble().is_some())
    };
    rsx! {
        div { class: "document-page", "data-ready": ready, "data-feed": if stale() { "stale" } else { "live" },
            if let Some(state) = state() {
                DocumentBody { anchor_ids: anchors(), present: state.latest.sections.iter().map(|s| s.key.clone()).collect(), document: state.shown, scope: scope.clone(), embedded }
                if !embedded {
                    crate::asks::Asks { key: "{scope}/{path}", path: path.clone(), scope: docs::scoped(&scope, ""), render_blocks }
                }
            } else if missing() {
                // A folder's intro shows nothing in its place: the folder is still there.
                if !embedded {
                    crate::trouble::Trouble { title: "Document not found", text: "There is no document at {path}: it may have been moved, renamed or removed.", back: Some((docs::scoped(&scope, &crate::project::current().documents_href()), "All documents".to_string())), retry: move |_| retry += 1 }
                }
            } else if let Some(why) = trouble().filter(|_| !embedded) {
                crate::trouble::Trouble { title: "The document could not be loaded", text: crate::trouble::failed(&why), retry: move |_| retry += 1 }
            } else if trouble().is_none() || (embedded && folder_notice.is_none()) {
                p { role: "status", {trouble().unwrap_or_else(|| "Loading document…".into())} }
            }
            // Until the document is shown, what kept it away is the block above's to say, once.
            if !embedded {
                crate::live::PageNotices { reconnecting: stale() && state.read().is_some(), reconnect: "Reconnecting: this document may be out of date", error: trouble().filter(|_| state.read().is_some()).unwrap_or_default(), document_removed: state.read().as_ref().is_some_and(|state| state.removed) }
            }
        }
    }
}

#[component]
fn DocumentBody(
    document: symphony_api::Document,
    scope: String,
    present: Vec<String>,
    anchor_ids: BTreeMap<String, Vec<String>>,
    embedded: bool,
) -> Element {
    let path = document.path.clone();
    let wide = use_hook(|| {
        web_sys::window()
            .and_then(|window| window.inner_width().ok())
            .and_then(|width| width.as_f64())
            .is_none_or(|width| width > 1100.0)
    });
    let content = rsx! {
        div { class: "doc-layout",
            if !embedded {
                div { class: "doc-head",
                    p { class: "eyebrow", {document.path.trim_start_matches(&format!("@{scope}/"))} }
                    h1 { "{document.title}" }
                    nav { class: "doc-tools", "aria-label": "Document tools",
                        a { href: crate::project::href("/"), "Board" }
                        a { href: crate::project::href(&docs::scoped(&scope, &crate::project::current().documents_href())), "All docs" }
                        Versions { path: path.clone(), scope: scope.clone() }
                    }
                }
            }
            if !embedded && document.headings.len() >= TOC_MIN_HEADINGS {
                nav { class: "doc-toc", "aria-label": "On this page",
                    details { open: wide,
                        summary { "On this page" }
                        div { class: "toc-links",
                            for heading in &document.headings {
                                NearViewport { key: "{heading.anchor}", height: 28,
                                    a { href: "#{heading.anchor}", class: "level-{heading.level}", "{heading.text}" }
                                }
                            }
                        }
                    }
                }
            }
            article { class: "doc-body",
                if let Some(comparison) = document.comparison.clone() {
                    crate::doc_components::ComparisonNote { comparison }
                }
                for (index, section) in document.sections.iter().enumerate() {
                    NearViewport {
                        key: "{section.key}",
                        class: {
                            let (first, last) = docs::part_edges(&document.sections, index);
                            format!("doc-part{}{}", if first { " doc-part-start" } else { "" }, if last { " doc-part-end" } else { "" })
                        },
                        anchors: {
                            let mut ids = anchor_ids.get(&section.key).cloned().unwrap_or_default(); if section.start > 0 { ids.push(format!("L{}", section.start)); } ids
                        },
                        height: if section.key.starts_with("card:") { 100 } else { 160 },
                        HydratedSection {
                        ids: anchor_ids.get(&section.key).cloned().unwrap_or_default(),
                        gone: !present.contains(&section.key),
                        section: section.clone(), path: path.clone(), scope: scope.clone(),
                        card: document.cards.iter().find(|c| section.key == format!("card:{}", c.slug)).cloned(),
                        thread: section.key.strip_prefix("card:").and_then(|slug| document.threads.get(slug)).cloned()
                        }
                    }
                }
            }
        }
    };
    rsx! {
        DocumentWithTasks { tasks: document.tasks, {content} }
    }
}

#[component]
fn HydratedSection(
    section: Section,
    path: String,
    scope: String,
    card: Option<DocumentCard>,
    thread: Option<Fragment>,
    gone: bool,
    ids: Vec<String>,
    #[props(default)] unfolded: bool,
) -> Element {
    let mut requests = use_context::<DocumentRequests>();
    let key = section.key.clone();
    let fold = !unfolded
        && card
            .as_ref()
            .is_some_and(|c| c.group == "resolved" || c.group == "rejected");
    use_effect(use_reactive!(|fold, key| {
        if !fold {
            requests.wanted.write().insert(key);
        }
    }));
    let ready = !section.blocks.is_empty();
    use_effect(use_reactive!(|ready, ids| {
        let target = web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .and_then(|hash| js_sys::decode_uri_component(hash.trim_start_matches('#')).ok())
            .map(String::from)
            .unwrap_or_default();
        if ready && !target.is_empty() && ids.contains(&target) {
            scroll_anchor();
        }
    }));
    // The outer slot owns the comparison even while a settled card is closed. Its unfolded
    // child owns only the live content, so opening it never repeats the change label/history.
    let comparison = if unfolded {
        None
    } else {
        section.comparison.clone()
    };
    let removed = section
        .comparison
        .as_ref()
        .is_some_and(|c| c.kind == symphony_api::Difference::Removed);
    let content = if fold && let Some(card) = &card {
        let summary = section.blocks.iter().find_map(|block| match block {
            Block::SettledCard { summary, .. } => Some(summary.clone()),
            _ => None,
        });
        rsx! { div { class: "doc-section folded-slot", id: "L{section.start}",
            details { class: "settled", id: "{card.slug}",
                summary {
                    if let Some(summary) = summary { SpansView { spans: summary } }
                    else { code { "{card.slug}" } " · {card.state} · {card.text}" }
                    for id in ids.iter().filter(|id| *id != &card.slug) { span { id: "{id}" } }
                }
                NearViewport { HydratedSection { section: section.clone(), path, scope, card: Some(card.clone()), thread, gone, ids: ids.clone(), unfolded: true } }
            }
        } }
    } else if ready && removed {
        rsx! { crate::block_view::HistoricalBlocks { blocks: section.blocks } }
    } else if ready {
        rsx! { DocSection { section, path, scope, card, thread, gone, unfolded } }
    } else {
        rsx! { div { class: "section-loading", id: (section.start > 0).then(|| format!("L{}", section.start)),
            for id in ids { span { id } }
            p { role: "status", "Loading section…" }
        } }
    };
    rsx! { crate::doc_components::SectionDifference { comparison, {content} } }
}

#[component]
fn DocSection(
    section: Section,
    path: String,
    scope: String,
    card: Option<DocumentCard>,
    thread: Option<Fragment>,
    gone: bool,
    unfolded: bool,
) -> Element {
    let mut editing = use_signal(|| false);
    let mut fragment = docs::fragment(&section);
    let settled = card
        .as_ref()
        .is_some_and(|c| c.group == "resolved" || c.group == "rejected");
    let summary = fragment
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::SettledCard { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if settled && let [Block::SettledCard { blocks, .. }] = fragment.blocks.as_slice() {
        fragment.blocks = blocks.clone();
    }
    let slug = card.as_ref().map(|c| c.slug.clone());
    let state = card.as_ref().map_or("", |c| c.state.as_str()).to_string();
    let is_card = card.is_some();
    let postponed_cards = use_context::<Signal<Vec<String>>>();
    let postponed = slug
        .as_ref()
        .is_some_and(|slug| postponed_cards.read().contains(slug));
    let target = (use_context::<DocumentRequests>().anchor)();
    let later = card.as_ref().filter(|card| matches!(card.group.as_str(), "decide" | "confirm")).map(|card| {
        rsx! { crate::postpone_live::Controls { path: path.clone(), scope: scope.clone(), slug: card.slug.clone(), postponed, seen: (editing() || target == card.slug).then(|| format!("{target}:{}", editing())) } }
    });
    let excerpt = use_callback(|target: LinkTarget| rsx! { Excerpt { target } });
    use_context_provider(|| Excerpts(is_card.then_some(excerpt)));
    use_context_provider(|| CardAnchor(if unfolded { slug.clone() } else { None }));
    // The button that opens the editor, where Escape returns focus.
    let opener = format!("opener-{}", encode(&format!("{path}:{}", section.key)));
    let content = rsx! {
            div { class: "section-prose",
                onclick: move |event: MouseEvent| {
                    let interactive = event.data().try_as_web_event().and_then(|e| e.target()).and_then(|e| e.dyn_into::<web_sys::Element>().ok()).and_then(|e| e.closest("a, button, input, summary, textarea, img, iframe, .viewer-stage, .excerpt-box").ok().flatten()).is_some();
                    if is_card && !interactive { editing.set(true); }
                },
                FragmentView { fragment }
            }
            if let Some(thread) = thread {
                NearViewport { height: 100,
                    FragmentView { fragment: thread }
                }
            }
            if is_card {
                div { class: "doc-actions",
                    if postponed { small { class: "postponed", "postponed" } }
                    crate::components::button::Button { id: opener.clone(), r#type: "button", aria_expanded: editing(), onclick: move |_| editing.toggle(), if editing() { "Close" } else { "Verdict / message" } }
                }
            } else {
                crate::components::button::Button { id: opener.clone(), class: "add", r#type: "button", aria_expanded: editing(), onclick: move |_| editing.toggle(), if editing() { "Close comment" } else { "+ comment" } }
            }
            if gone { p { role: "alert", "This block was removed elsewhere. Copy your draft or discard it; saving to its old position is disabled." } }
            Editor { path, scope, section_key: section.key.clone(), slug, state, after_line: section.end, editing, opener: opener.clone(), gone, later }
    };
    rsx! {
        section { class: if is_card { "doc-section doc-card" } else { "doc-section" }, id: if unfolded { String::new() } else { format!("L{}", section.start) },
            "data-state": card.as_ref().map(|c| c.state.clone()),
            "data-doc-key": "{section.key}", "data-start": "{section.start}", "data-end": "{section.end}",
            "data-busy": if editing() { "true" } else { "false" },
            if settled && !unfolded {
                details { class: "settled", id: card.as_ref().map(|c| c.slug.clone()).unwrap_or_default(),
                    summary { SpansView { spans: summary } }
                    NearViewport { {content} }
                }
            } else { {content} }
        }
    }
}

#[component]
fn FragmentView(fragment: Fragment) -> Element {
    rsx! { BlocksView { blocks: fragment.blocks } }
}

/// A link's excerpt, drawn beside the button that opened it: the section its anchor names. Its
/// cards own no anchor and its links open no excerpt, so a card's link to its own section shows
/// that section once and nothing nests. It holds its section's markup while it is open.
#[component]
fn Excerpt(target: LinkTarget) -> Element {
    let mut read = use_signal(|| None::<Result<Fragment, String>>);
    let anchor = target.anchor.clone().unwrap_or_default();
    let title = format!("{}#{anchor}", target.path);
    use_hook(|| {
        let url = format!(
            "{}&format=blocks&anchor={}",
            api("", "/api/section", target.path.trim_start_matches('/')),
            encode(&anchor)
        );
        spawn(async move { read.set(Some(get::<Fragment>(&url).await)) })
    });
    rsx! {
        div { class: "excerpt-box", role: "region", "aria-label": "Excerpt of {title}", "data-busy": "true",
            p { class: "excerpt-title", "{title}" }
            {match read() {
                None => rsx! { p { role: "status", "Loading the excerpt…" } },
                Some(Err(why)) => rsx! { p { role: "alert", "{why}" } },
                Some(Ok(fragment)) if fragment.blocks.is_empty() => rsx! { p { "This section is empty." } },
                Some(Ok(fragment)) => rsx! { crate::block_view::HistoricalBlocks { blocks: fragment.blocks } },
            }}
        }
    }
}

#[component]
fn Editor(
    path: String,
    scope: String,
    section_key: String,
    slug: Option<String>,
    state: String,
    after_line: usize,
    mut editing: Signal<bool>,
    // The id of the button that opens this editor, where Escape returns focus.
    opener: String,
    gone: bool,
    later: Option<Element>,
) -> Element {
    let DocumentRequests {
        released,
        state: page,
        ..
    } = use_context::<DocumentRequests>();
    let key = format!("review-draft:{path}:{section_key}");
    let tap_key = format!("review-tap:{path}:{section_key}");
    let platform = platform::current();
    let mut text = use_signal(|| platform.load(&key).unwrap_or_default());
    let mut kind = use_signal(|| "comment".to_string());
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut saved = use_signal(|| None::<String>);
    let mut attach = crate::attach::use_attach(key.clone());
    let field_id = format!("draft-{}", encode(&key));
    let focus_id = field_id.clone();
    use_effect(move || {
        if editing() {
            focus(&focus_id);
        }
    });
    let draft_key = key.clone();
    let p = path.clone();
    let s = scope.clone();
    let card = slug.clone();
    let save_key = key.clone();
    let message = slug.is_some();
    let save = use_callback(move |action: String| {
        if gone {
            error.set(Some("This block was removed; your draft is kept.".into()));
            return;
        }
        if pending() {
            return;
        }
        if attach.busy() {
            error.set(Some("A file is still uploading".into()));
            return;
        }
        // A comment's first line is its title, so a comment needs words; a message may be
        // files alone.
        if text().trim().is_empty()
            && (action == "comment" || action == "message" && attach.is_empty())
        {
            error.set(Some("Type a message first".into()));
            return;
        }
        let note = attach.compose(&text());
        pending.set(true);
        error.set(None);
        saved.set(None);
        let path = p.clone();
        let scope = s.clone();
        let slug = card.clone();
        let key = save_key.clone();
        let tap_key = tap_key.clone();
        // The owner's own answer shows on its card at once, whatever else holds the card open: the
        // card's lines of the owner's before it are read as the save is sent, since the feed may
        // bring its change before the answer to the post.
        let release = slug.is_some().then(|| {
            let before = page
                .peek()
                .as_ref()
                .map_or(0, |page| docs::owner_lines(&page.latest, &section_key));
            (section_key.clone(), before)
        });
        let released = released.clone();
        spawn(async move {
            let platform = platform::current();
            let mut fields = vec![("path", path.into()), ("text", note.clone().into())];
            let route = if let Some(slug) = slug {
                fields.push(("slug", slug.into()));
                if action == "message" {
                    "/api/message"
                } else {
                    // Persist the id before sending. A retry of an unanswered identical tap,
                    // including after a reload, cannot append the verdict twice.
                    let signature = format!("{action}\n{note}");
                    let previous = platform.load(&tap_key).and_then(|v| {
                        v.split_once('\n')
                            .map(|(id, sig)| (id.to_string(), sig.to_string()))
                    });
                    let tap = previous
                        .filter(|(_, sig)| sig == &signature)
                        .map(|(id, _)| id)
                        .unwrap_or_else(|| {
                            format!("ui-{}-{}", js_sys::Date::now(), js_sys::Math::random())
                        });
                    platform.save(&tap_key, &format!("{tap}\n{signature}"));
                    fields.extend([
                        ("state", action.clone().into()),
                        ("note", note.into()),
                        ("tap", tap.into()),
                    ]);
                    "/api/decision"
                }
            } else {
                fields.extend([
                    ("after_line", JsValue::from_f64(after_line as f64)),
                    ("kind", kind().into()),
                ]);
                "/api/comment"
            };
            match post(&docs::scoped(&scope, route), &fields).await {
                Ok(reply) => {
                    if let Some((section, before)) = release {
                        // A retried tap the server had written already: no new line comes.
                        let repeat = field(&reply, "repeat").as_bool() == Some(true);
                        docs::release(&mut released.borrow_mut(), section, before, repeat);
                    }
                    text.set(String::new());
                    attach.clear();
                    platform.save(&key, "");
                    platform.save(&tap_key, "");
                    saved.set(Some(if action == "message" {
                        "Message sent".into()
                    } else if action == "comment" {
                        "Comment saved".into()
                    } else {
                        format!("Verdict saved: {action}")
                    }));
                    editing.set(false);
                }
                Err(why) => error.set(Some(why)),
            }
            pending.set(false);
        });
    });
    rsx! {
        div { class: "editor", "data-busy": if editing() || pending() || !text().is_empty() || !attach.is_empty() || attach.busy() { "true" } else { "false" },
            // A closed draft holds its card's words, not the page's markup or order.
            "data-draft": if !editing() && !pending() && !attach.busy() && (!text().is_empty() || !attach.is_empty()) { "true" } else { "false" },
            if (!text().is_empty() || !attach.is_empty()) && !editing() {
                crate::components::button::Button { r#type: "button", onclick: move |_| editing.set(true), "Resume draft" }
            }
            form { class: if message { "verdict" } else { "comment" }, hidden: !editing(), "data-attach": attach.zone(),
                onsubmit: move |event| { event.prevent_default(); save.call(if message { "message".into() } else { "comment".into() }); },
                // Escape closes the editor, its draft kept, and returns focus to the button that
                // opened it. An open list in the form takes its own Escape and stops it there.
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Escape {
                        event.prevent_default();
                        editing.set(false);
                        focus(&opener);
                    }
                },
                crate::components::textarea::Textarea { id: field_id, value: "{text}", disabled: pending(), "aria-label": "Message or verdict note",
                    placeholder: "Write a message, or add a note to your verdict…",
                    oninput: move |event: FormEvent| { let value = event.value(); platform::current().save(&draft_key, &value); text.set(value); },
                    onkeydown: move |event: KeyboardEvent| {
                        if event.key() == Key::Enter && event.modifiers().intersects(Modifiers::CONTROL | Modifiers::META) {
                            event.prevent_default(); save.call(if message { "message".into() } else { "comment".into() });
                        }
                    },
                }
                crate::attach::AttachControls { attach, disabled: pending() }
                if message {
                    // Approve is what the owner most often does, so it alone is filled; the card's
                    // own verdict is marked as current, never promoted.
                    div { class: "verdict-choices",
                        for (value, label) in [("approved", "✓ Approve"), ("rejected", "✗ Reject"), ("question", "? Question")] {
                            Button { r#type: "button", class: if value == "approved" { "v-approved primary".to_string() } else { format!("v-{value}") }, "aria-current": (value == state).then_some("true"), disabled: pending(), onclick: move |_| save.call(value.into()), "{label}" }
                        }
                        {later}
                    }
                }
                div { class: "row",
                    if !message {
                        crate::components::select::SelectField { label: "Comment kind", value: kind(), on_change: move |value| kind.set(value),
                            options: ["comment", "question", "decision"].map(|value| (value.to_string(), value.to_string())).to_vec(),
                        }
                    }
                    Button { r#type: "submit", class: if message { "" } else { "primary" }, disabled: pending(), if pending() { "Saving…" } else if message { "Send message" } else { "Save comment" } }
                    if message {
                        Button { r#type: "button", class: "v-dismissed", "aria-current": (state == "dismissed").then_some("true"), disabled: pending(), onclick: move |_| save.call("dismissed".into()), "⊘ Dismiss" }
                        // A pending card has no verdict to take back.
                        if state != "pending" {
                            Button { r#type: "button", variant: ButtonVariant::Ghost, class: "reset", title: "Take back the verdict: the card waits for yours again", disabled: pending(), onclick: move |_| save.call("pending".into()), "↺ Reset to pending" }
                        }
                    }
                    Button { r#type: "button", variant: ButtonVariant::Ghost, class: "discard", title: "Clear what you wrote and close", disabled: pending(), onclick: move |_| { text.set(String::new()); attach.clear(); platform::current().save(&key, ""); editing.set(false); }, "Discard draft" }
                }
            }
            if let Some(why) = error() { p { role: "alert", "{why}" } }
            if let Some(message) = saved() { p { role: "status", "{message}" } }
        }
    }
}

/// Moves focus to the element the id `id` names, if the page has it.
fn focus(id: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = element.focus();
    }
}

#[derive(Deserialize)]
struct VersionsResponse {
    versions: Vec<Version>,
}

#[component]
fn Versions(path: String, scope: String) -> Element {
    let mut versions = use_signal(|| None::<Vec<Version>>);
    let mut diff = use_signal(|| None::<DocumentDiff>);
    let mut error = use_signal(|| None::<String>);
    let p = path.clone();
    let s = scope.clone();
    rsx! {
        details { class: "versions",
            summary { onclick: move |_| {
                let url = api(&s, "/api/versions", &p);
                spawn(async move { match get::<VersionsResponse>(&url).await { Ok(value) => versions.set(Some(value.versions)), Err(why) => error.set(Some(why)) } });
            }, "Versions and diffs" }
            if let Some(versions) = versions() {
                ul {
                    for version in versions {
                        NearViewport { key: "{version.sha}", height: 36,
                        li {
                            crate::components::button::Button { r#type: "button", onclick: {
                                let url = format!("{}&sha={}", api(&scope, "/api/diff", &path), encode(&version.sha));
                                move |_| { let url = url.clone(); spawn(async move { match get::<DocumentDiff>(&url).await { Ok(value) => diff.set(Some(value)), Err(why) => error.set(Some(why)) } }); }
                            }, "{version.date} · {version.subject}" }
                        }
                        }
                    }
                }
            }
            if let Some(diff) = diff() { h3 { "{diff.subject}" } pre { class: "diff", "{diff.diff}" } }
            if let Some(why) = error() { p { role: "alert", "{why}" } }
        }
    }
}

#[component(lazy)]
pub fn FolderPage(path: String, scope: String) -> Element {
    std::hint::black_box(&STYLE);
    let mut folder = use_signal(|| None::<Folder>);
    let mut error = use_signal(|| None::<String>);
    let mut missing = use_signal(|| false);
    let mut retry = use_signal(|| 0_u64);
    let mut stale = use_signal(|| false);
    let mut notice_error = use_signal(String::new);
    let mut removed = use_signal(|| false);
    use_context_provider(|| FolderNotice {
        stale,
        error: notice_error,
        removed,
    });
    use_effect(move || {
        if folder
            .read()
            .as_ref()
            .is_some_and(|folder| folder.intro.is_none())
        {
            stale.set(false);
            notice_error.set(String::new());
            removed.set(false);
        }
    });
    let p = path.clone();
    let s = scope.clone();
    use_hook(move || {
        spawn(async move {
            loop {
                match get_found::<Folder>(&format!("{}&view=outline", api(&s, "/api/folder", &p)))
                    .await
                {
                    Ok(Some(value)) => {
                        folder.set(Some(value));
                        missing.set(false);
                        error.set(None);
                    }
                    Ok(None) => {
                        missing.set(true);
                        error.set(None);
                    }
                    Err(why) => error.set(Some(why)),
                }
                pause(5_000, retry).await;
            }
        });
    });
    rsx! {
        div { class: "document-page", "data-ready": if folder.read().is_some() { "folder" } else { crate::trouble::ready(missing(), error().is_some()) }, "data-feed": if stale() { "stale" } else { "live" },
            if let Some(folder) = folder() {
                div { class: "doc-head",
                    p { class: "eyebrow", {folder.path.trim_start_matches(&format!("@{scope}/"))} "/" }
                    h1 { "{folder.title}" }
                    p { class: "doc-meta", span { class: "pill", if folder.docs == 1 { "1 doc" } else { "{folder.docs} docs" } } }
                }
                if let Some(intro) = folder.intro { DocumentPage { key: "{intro.path}", path: intro.path, scope: scope.clone(), embedded: true } }
                if !folder.entries.is_empty() {
                    h2 { class: "section-title", "In this folder" span { class: "count", "{folder.entries.len()}" } }
                }
                div { class: "folder-entries",
                    for entry in folder.entries {
                        NearViewport { key: "{entry.path}", height: 120,
                        a { class: if entry.kind == "folder" { "card folder-entry tree-folder" } else { "card folder-entry" }, href: crate::project::href(&if entry.kind == "folder" { format!("/{}/", entry.path) } else { format!("/{}", entry.path) }),
                            strong { class: "tree-name", if entry.kind == "folder" { span { class: "tree-icon", "aria-hidden": "true", "▸" } } "{entry.title}" }
                            div { class: "tree-pills",
                                if entry.kind == "folder" { span { class: "pill", if entry.docs == 1 { "1 doc" } else { "{entry.docs} docs" } } }
                                if entry.waiting > 0 { span { class: "pill wait", "{entry.waiting} waiting" } }
                            }
                            if !entry.summary.is_empty() { p { "{entry.summary}" } }
                        }
                        }
                    }
                }
            } else if missing() {
                crate::trouble::Trouble { title: "Folder not found", text: "There is no folder at {path}: it may have been moved, renamed or removed.", back: Some((docs::scoped(&scope, &crate::project::current().documents_href()), "All documents".to_string())), retry: move |_| retry += 1 }
            } else if let Some(why) = error() {
                crate::trouble::Trouble { title: "The folder could not be loaded", text: crate::trouble::failed(&why), retry: move |_| retry += 1 }
            } else { p { role: "status", "Loading folder…" } }
            crate::live::PageNotices { reconnecting: stale(), reconnect: "Reconnecting: this document may be out of date", error: if folder.read().is_some() { error().unwrap_or(notice_error()) } else { String::new() }, document_removed: removed() }
        }
    }
}

/// The sections the owner is engaged in (a focused field; an open form, excerpt, gallery or
/// details), and those holding only a closed draft.
fn busy() -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut engaged, mut drafts) = (BTreeSet::new(), BTreeSet::new());
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return (engaged, drafts);
    };
    if let Some(active) = document.active_element()
        && active
            .matches("input, textarea, select, button[aria-haspopup=listbox], [role=option]")
            .unwrap_or(false)
        && let Some(section) = active.closest("[data-doc-key]").ok().flatten()
        && let Some(key) = section.get_attribute("data-doc-key")
    {
        engaged.insert(key);
    }
    for (selector, keys) in [
        (
            "[data-busy=true]:not([data-draft=true]), .section-prose details[open], .history details[open]",
            &mut engaged,
        ),
        ("[data-draft=true]", &mut drafts),
    ] {
        let Ok(found) = document.query_selector_all(selector) else {
            continue;
        };
        for i in 0..found.length() {
            if let Some(key) = found
                .item(i)
                .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                .and_then(|node| node.closest("[data-doc-key]").ok().flatten())
                .and_then(|section| section.get_attribute("data-doc-key"))
            {
                keys.insert(key);
            }
        }
    }
    (engaged, drafts)
}

/// What a document page's read keeps: the document, whether its feed is behind, why the last read
/// failed, whether the server has no such document, and the reader's Retry presses.
#[derive(Clone, Copy)]
struct DocumentRead {
    state: Signal<Option<DocumentState>>,
    stale: Signal<bool>,
    trouble: Signal<Option<String>>,
    missing: Signal<bool>,
    retry: Signal<u64>,
}

async fn document_live(
    path: String,
    scope: String,
    read: DocumentRead,
    mut requests: DocumentRequests,
) {
    let DocumentRead {
        mut state,
        mut stale,
        mut trouble,
        mut missing,
        retry,
    } = read;
    let mut backoff = crate::state::Backoff::default();
    // Flush deferred fragments when a form, excerpt or focused field becomes free, including
    // when the feed has nothing further to say. No page fetch is needed to do it.
    let flush_requests = requests.clone();
    spawn(async move {
        let mut previous_busy = BTreeSet::new();
        loop {
            sleep(400).await;
            crate::live::flush_reload();
            if state.peek().as_ref().is_none_or(|s| s.shown == s.latest) {
                continue;
            }
            let Some(mut next) = state() else {
                continue;
            };
            let keys = held(&flush_requests, &next.latest);
            if !keys.is_empty() && keys == previous_busy {
                continue;
            }
            previous_busy = keys.clone();
            show(&mut next, &flush_requests, &keys);
            if state.peek().as_ref() != Some(&next) {
                state.set(Some(next));
            }
        }
    });
    loop {
        back().await;
        let mut ended = Ended::Lost(0.0);
        match get_found::<DocumentOutline>(&format!(
            "{}&view=outline",
            api(&scope, "/api/document", &path)
        ))
        .await
        {
            Ok(None) => {
                missing.set(true);
                trouble.set(None);
            }
            Ok(Some(outline)) => {
                missing.set(false);
                requests.epoch.set(requests.epoch.get().wrapping_add(1));
                requests.cursor.set(outline.document.version);
                requests.anchors.set(outline.anchors);
                requests.loaded.write().clear();
                let mut next =
                    state().unwrap_or_else(|| DocumentState::new(outline.document.clone()));
                next.latest = outline.document;
                next.removed = false;
                let keys = held(&requests, &next.latest);
                show(&mut next, &requests, &keys);
                state.set(Some(next));
                ended =
                    follow_document(&path, &scope, state, stale, trouble, requests.clone()).await;
            }
            Err(why) => trouble.set(Some(why)),
        }
        stale.set(true);
        pause(backoff.wait(ended), retry).await;
    }
}

async fn load_sections(
    path: String,
    scope: String,
    mut state: Signal<Option<DocumentState>>,
    mut requests: DocumentRequests,
) {
    loop {
        sleep(50).await;
        if state.peek().is_none() {
            continue;
        }
        let missing = requests
            .wanted
            .peek()
            .difference(&requests.loaded.peek())
            .cloned()
            .collect::<BTreeSet<_>>();
        if missing.is_empty() {
            continue;
        }
        let url = format!(
            "{}&format=blocks&keys={}",
            api(&scope, "/api/section", &path),
            keys_query(&missing)
        );
        let epoch = requests.epoch.get();
        match get::<DocumentSections>(&url).await {
            Ok(parts) => {
                if requests.epoch.get() != epoch {
                    continue;
                }
                // Let the ordered feed catch up to the response's cursor before installing it.
                // An older body must never overwrite a later edit that the feed already applied.
                let deadline = js_sys::Date::now() + 2_000.0;
                while requests.cursor.get() < parts.version
                    && js_sys::Date::now() < deadline
                    && requests.epoch.get() == epoch
                {
                    sleep(50).await;
                }
                if requests.cursor.get() < parts.version || requests.epoch.get() != epoch {
                    continue;
                }
                if state
                    .peek()
                    .as_ref()
                    .is_some_and(|s| s.latest.version > parts.version)
                {
                    continue;
                }
                requests.loaded.write().extend(missing);
                if let Some(current) = state.write().as_mut() {
                    current.change(parts.change);
                    let keys = held(&requests, &current.latest);
                    show(current, &requests, &keys);
                }
                requests.trouble.set(None);
            }
            Err(why) => {
                requests.trouble.set(Some(why));
                sleep(1_000).await;
            }
        }
    }
}

async fn follow_document(
    path: &str,
    scope: &str,
    mut state: Signal<Option<DocumentState>>,
    mut stale: Signal<bool>,
    mut trouble: Signal<Option<String>>,
    mut requests: DocumentRequests,
) -> Ended {
    let mut opened = None;
    'subscribe: loop {
        let subscribed = (requests.wanted)();
        let document = if scope.is_empty() {
            path.to_string()
        } else {
            format!("@{scope}/{path}")
        };
        let url = format!(
            "{}?since={}&document={}&sections={}",
            docs::scoped(scope, "/api/feed"),
            requests.cursor.get(),
            encode(&document),
            keys_query(&subscribed)
        );
        let (sender, mut heard) = unbounded();
        let Some(_feed) = Feed::open(&url, sender) else {
            break;
        };
        let mut last_word = js_sys::Date::now();
        loop {
            let event = match select(heard.next(), pin!(sleep(200))).await {
                Either::Left((Some(event), _)) => event,
                Either::Right(_) => {
                    if (requests.wanted)() != subscribed {
                        continue 'subscribe;
                    }
                    if js_sys::Date::now() - last_word < 12_500.0 {
                        continue;
                    }
                    Heard::Lost
                }
                _ => Heard::Lost,
            };
            last_word = js_sys::Date::now();
            match event {
                Heard::Open => {
                    requests.frame.connected();
                    opened.get_or_insert(last_word);
                    stale.set(false);
                    trouble.set(None);
                }
                Heard::Ping => {}
                Heard::Server(message) => {
                    *crate::live::SERVER_NOTICE.write() = message;
                }
                Heard::Lost => break 'subscribe,
                Heard::Away => return Ended::Away,
                Heard::Delta(value) => {
                    requests.frame.heard(&value);
                    let Some(version) = field(&value, "version").as_f64().filter(|v| {
                        v.is_finite()
                            && *v >= 0.0
                            && v.fract() == 0.0
                            && *v <= 9_007_199_254_740_991.0
                    }) else {
                        break 'subscribe;
                    };
                    requests.cursor.set(version as u64);
                    let changes = field(&value, "changes");
                    if !Array::is_array(&changes) {
                        break 'subscribe;
                    }
                    for change in Array::from(&changes) {
                        if matches!(
                            field(&change, "region").as_string().as_deref(),
                            Some("queue-item" | "queue-item-order")
                        ) {
                            requests.order_refreshed += 1;
                        }
                        if field(&change, "region").as_string().as_deref() != Some("document") {
                            continue;
                        }
                        let data = field(&change, "data");
                        let Ok(mut delta) =
                            serde_wasm_bindgen::from_value::<DocumentChange>(data.clone())
                        else {
                            break 'subscribe;
                        };
                        if delta.path != document {
                            continue;
                        }
                        if let Ok(anchors) = serde_wasm_bindgen::from_value::<
                            BTreeMap<String, Vec<String>>,
                        >(field(&data, "anchors"))
                        {
                            requests.anchors.write().extend(anchors);
                        }
                        for section in &mut delta.sections {
                            if section.blocks.is_empty()
                                && requests.wanted.peek().contains(&section.key)
                            {
                                let before = state
                                    .peek()
                                    .as_ref()
                                    .and_then(|s| {
                                        s.latest.sections.iter().find(|old| old.key == section.key)
                                    })
                                    .cloned();
                                if let Some(before) = before.filter(|old| {
                                    old.hash == section.hash && !old.blocks.is_empty()
                                }) {
                                    section.blocks = before.blocks;
                                    section.comparison = before.comparison;
                                } else {
                                    requests.loaded.write().remove(&section.key);
                                }
                            }
                        }
                        if let Some(current) = state.write().as_mut() {
                            current.change(delta);
                            current.latest.version = version as u64;
                            let keys = held(&requests, &current.latest);
                            show(current, &requests, &keys);
                        }
                    }
                }
            }
        }
    }
    Ended::Lost(opened.map_or(0.0, |at| js_sys::Date::now() - at))
}
