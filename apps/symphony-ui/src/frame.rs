//! The shared header and first-paint metadata. Closed controls, conversation state and typed
//! activity/pending views live in the persistent panel module, loaded on interaction or a toast.

use dioxus::prelude::*;
use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

use crate::{docs, navigation::Route, platform, request::field};

#[cfg(feature = "page-panel")]
#[path = "frame_panel.rs"]
mod panel;
#[cfg(feature = "page-panel")]
pub use panel::PanelApp;

const STYLE: Asset = asset!(
    "/assets/frame.css",
    AssetOptions::css().with_static_head(true)
);
const COMPONENT_THEME: Asset = asset!(
    "/assets/dx-components-theme.css",
    AssetOptions::css().with_static_head(true)
);

#[derive(Clone, Copy)]
pub struct FrameRefresh(pub Signal<u64>, Signal<crate::state::MetadataReads>);

impl FrameRefresh {
    pub fn refresh(mut self) {
        self.0 += 1;
    }
    pub fn connected(mut self) {
        // A stream that opens again may have missed an answer: every open chat reads once.
        asks_moved(&JsValue::TRUE);
        let refresh = self.1.write().connected();
        if refresh {
            self.refresh();
        }
    }
    pub fn heard(self, value: &JsValue) {
        let changes = field(value, "changes");
        if !js_sys::Array::is_array(&changes) {
            return;
        }
        let mut refresh = false;
        for change in js_sys::Array::from(&changes).iter() {
            match field(&change, "region").as_string().as_deref() {
                Some("activity" | "pending" | "docs") => refresh = true,
                Some("asks") => asks_moved(&field(&change, "data")),
                _ => {}
            }
        }
        if refresh {
            self.refresh();
        }
    }
}

/// Tell every module's ask readers (the panel's chat, a document's questions) what the page's
/// feed heard of the asks: the region's data, or `true` when anything may have moved.
fn asks_moved(moved: &JsValue) {
    if let Some(window) = web_sys::window()
        && let Ok(event) = web_sys::Event::new("review:asks")
    {
        let _ = js_sys::Reflect::set(&event, &"moved".into(), moved);
        let _ = window.dispatch_event(&event);
    }
}

/// `review:asks`'s news read: `true`, or the region's data.
#[cfg(any(feature = "page-panel", feature = "page-docs"))]
fn asks_news(moved: &JsValue) -> crate::state::AsksNews {
    crate::state::AsksNews {
        everything: moved.as_bool() == Some(true),
        recipient: moved.is_object()
            && js_sys::Reflect::has(moved, &"recipient".into()) == Ok(true),
        threads: array(field(moved, "threads"))
            .iter()
            .filter_map(|thread| {
                Some((
                    field(&thread, "id").as_f64()? as i64,
                    field(&thread, "path").as_string()?,
                ))
            })
            .collect(),
    }
}

/// A count of the `review:asks` news that `concerns` accepts: what a thread's reader reads
/// again on, instead of a timer. The browser listener only forwards; the count moves in a task.
#[cfg(any(feature = "page-panel", feature = "page-docs"))]
pub(crate) fn use_asks_heard(
    concerns: impl Fn(&crate::state::AsksNews) -> bool + 'static,
) -> Signal<u64> {
    let mut heard = use_signal(|| 0_u64);
    let listener = use_hook(move || {
        let (send, mut recv) = unbounded::<crate::state::AsksNews>();
        spawn(async move {
            while let Some(news) = recv.next().await {
                if concerns(&news) {
                    heard += 1;
                }
            }
        });
        let listener = std::rc::Rc::new(Closure::<dyn FnMut(web_sys::Event)>::new(
            move |event: web_sys::Event| {
                let _ = send.unbounded_send(asks_news(&field(event.as_ref(), "moved")));
            },
        ));
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "review:asks",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
        listener
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "review:asks",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    heard
}

// The interactive frame belongs to the persistent module; page teardown has nothing to save.
pub(crate) fn save_session() {}

pub(super) fn snapshot(key: &str) -> JsValue {
    web_sys::window().map_or(JsValue::UNDEFINED, |w| field(w.as_ref(), key))
}

fn remember(key: &str, value: &JsValue) {
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(&window, &key.into(), value);
    }
}

pub(super) fn publish(event: &str) {
    if let Some(window) = web_sys::window()
        && let Ok(event) = web_sys::Event::new(event)
    {
        let _ = window.dispatch_event(&event);
    }
}

pub(super) fn use_revision(events: &'static [&'static str]) -> Signal<u64> {
    let mut revision = use_signal(|| 0_u64);
    let listener = use_hook(move || {
        let (send, mut recv) = unbounded();
        spawn(async move {
            while recv.next().await.is_some() {
                revision += 1;
            }
        });
        let listener = std::rc::Rc::new(Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = send.unbounded_send(());
        }));
        if let Some(window) = web_sys::window() {
            for event in events {
                let _ = window.add_event_listener_with_callback(
                    event,
                    listener.as_ref().as_ref().unchecked_ref(),
                );
            }
        }
        listener
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            for event in events {
                let _ = window.remove_event_listener_with_callback(
                    event,
                    listener.as_ref().as_ref().unchecked_ref(),
                );
            }
        }
    });
    revision
}

/// Asks the panel module to move focus into the panel `which` (`quick` or `activity`) once it
/// shows, and remembers the control that opened it, where Escape returns focus.
fn focus_on_open(which: &str) {
    let opener = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element());
    remember(
        "reviewPanelOpener",
        &opener.map_or(JsValue::NULL, Into::into),
    );
    remember("reviewPanelFocus", &which.into());
}

fn quick_open() {
    focus_on_open("quick");
    let mut draft = snapshot("reviewQuickDraft");
    if !draft.is_object() {
        draft = platform::current()
            .load("review-quick-draft")
            .and_then(|s| js_sys::JSON::parse(&s).ok())
            .unwrap_or_else(|| js_sys::Object::new().into());
    }
    let _ = js_sys::Reflect::set(&draft, &"open".into(), &JsValue::TRUE);
    if field(&draft, "priority").as_string().is_none() {
        let _ = js_sys::Reflect::set(&draft, &"priority".into(), &"2".into());
    }
    remember("reviewQuickDraft", &draft);
    remember("reviewActivityOpen", &JsValue::FALSE);
    publish("review:quick");
    publish("review:activity");
}

/// Opens the activity panel, as the header's activity button does while it is closed: the home's
/// count of today's replies leads there.
pub fn open_activity() {
    focus_on_open("activity");
    // Activity and Appearance share the header's right edge: one open closes the other.
    remember("reviewAppearanceOpen", &JsValue::FALSE);
    publish("review:appearance");
    remember("reviewActivityOpen", &JsValue::TRUE);
    publish("review:activity");
}

fn seen_ids() -> JsValue {
    platform::current()
        .load("docs-review-seen")
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .unwrap_or(JsValue::UNDEFINED)
}

fn array(value: JsValue) -> js_sys::Array {
    if js_sys::Array::is_array(&value) {
        value.unchecked_into()
    } else {
        js_sys::Array::new()
    }
}

fn unread() -> bool {
    let seen = array(seen_ids());
    array(field(&snapshot("reviewActivitySnapshot"), "items"))
        .iter()
        .any(|item| {
            field(&item, "answered").as_bool() != Some(true)
                && !seen.includes(&field(&item, "id"), 0)
        })
}

#[component]
pub fn Frame(route: Route, children: Element) -> Element {
    std::hint::black_box(&STYLE);
    std::hint::black_box(&COMPONENT_THEME);
    let mut refresh = use_context_provider(|| {
        FrameRefresh(
            Signal::new(0),
            Signal::new(crate::state::MetadataReads::default()),
        )
    });
    let revision = use_revision(&[
        "review:frame",
        "review:quick",
        "review:activity",
        "review:panel-ready",
        "review:frame-seen",
        "review:appearance",
    ]);
    let mut generation = use_signal(|| 0_u64);
    use_effect(move || {
        let _ = (refresh.0)();
        generation += 1;
        let ticket = *generation.peek();
        let started = refresh.1.write().start();
        spawn(async move {
            let (activity, pending) = futures_util::future::join(
                crate::request::get_json("/api/activity"),
                crate::request::get_json("/api/pending"),
            )
            .await;
            if ticket != *generation.peek() {
                return;
            }
            let retry = refresh
                .1
                .write()
                .finished(started, activity.is_ok() && pending.is_ok());
            match (activity, pending) {
                (Ok(activity), Ok(pending)) => {
                    let previous = snapshot("reviewKnownIds");
                    let ids = js_sys::Array::new();
                    for item in array(field(&activity, "items")) {
                        ids.push(&field(&item, "id"));
                    }
                    let added = js_sys::Array::is_array(&previous)
                        && ids
                            .iter()
                            .any(|id| !js_sys::Array::from(&previous).includes(&id, 0));
                    if !js_sys::Array::is_array(&seen_ids())
                        && let Ok(json) = js_sys::JSON::stringify(&ids)
                    {
                        platform::current().save("docs-review-seen", &String::from(json));
                    }
                    let listed = |value: &JsValue| {
                        js_sys::Array::is_array(value).then(|| js_sys::Array::from(value).to_vec())
                    };
                    if !crate::state::keeps_panel_baseline(
                        listed(&snapshot("reviewPreviousIds")).as_deref(),
                        listed(&previous).as_deref(),
                        snapshot("reviewPanelReady").as_bool() == Some(true),
                    ) {
                        remember("reviewPreviousIds", &previous);
                    }
                    remember("reviewKnownIds", ids.as_ref());
                    remember("reviewActivitySnapshot", &activity);
                    remember("reviewPendingSnapshot", &pending);
                    remember("reviewFrameError", &JsValue::NULL);
                    publish("review:frame");
                    if added {
                        publish("review:activity-arrived");
                    }
                }
                (Err(why), _) | (_, Err(why)) => {
                    remember("reviewFrameError", &why.into());
                    publish("review:frame");
                }
            }
            if retry {
                refresh.refresh();
            }
        });
    });
    let _ = revision();
    let open = field(&snapshot("reviewQuickDraft"), "open").as_bool() == Some(true);
    let panel = snapshot("reviewActivityOpen").as_bool() == Some(true);
    let appearance = snapshot("reviewAppearanceOpen").as_bool() == Some(true);
    let has_pending = array(field(&snapshot("reviewPendingSnapshot"), "items")).length() > 0;
    let project = crate::project::current();
    let links = crate::project::header_links(&project, &route.path, route.document);
    let crumbs = crate::project::crumbs(&project, &route.path);
    rsx! {
        div { class: if open { "app-frame quick-expanded" } else { "app-frame" },
            header { class: "app-header", id: "top",
                if !project.base.is_empty() {
                    a { href: "/", "data-native": "true", class: "project-switch", {symphony_api::page_names::PROJECTS} }
                }
                // The Symphony's own pages stay the global view's on a branch's document; the rest
                // are that branch's.
                nav { class: "app-pages", "aria-label": "Pages",
                    for link in links {
                        a { href: if matches!(link.href.as_str(), "/" | "/queue" | "/tasks" | "/agents") { link.href.clone() } else { docs::scoped(&route.scope, &link.href) },
                            class: if link.active { "active" } else { "" }, "aria-current": if link.active { "page" }, "{link.label}" }
                    }
                }
                if !crumbs.is_empty() {
                    nav { class: "crumbs", "aria-label": "Breadcrumbs",
                        for (index, crumb) in crumbs.iter().enumerate() {
                            if index > 0 { " / " }
                            if index + 1 < crumbs.len() {
                                a { href: docs::scoped(&route.scope, &format!("/{}/", crumbs[..=index].join("/"))), "{crumb}" }
                            } else { span { "{crumb}" } }
                        }
                    }
                }
                // Appearance is a setting of this project on this device: its panel lives in the
                // persistent module, which this button loads, as the bell loads activity's.
                crate::components::button::Button { id: "appearance", r#type: "button", "aria-label": "Appearance", title: "Appearance", "aria-expanded": appearance, "aria-controls": "appearance-panel",
                    actions: crate::entry_events::click(move || {
                        remember("reviewAppearanceOpen", &JsValue::from_bool(!appearance));
                        remember("reviewActivityOpen", &JsValue::FALSE);
                        publish("review:activity");
                        publish("review:appearance");
                    }),
                    svg { width: "16", height: "16", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", "aria-hidden": "true",
                        circle { cx: "12", cy: "12", r: "9" }
                        path { d: "M12 3a9 9 0 0 1 0 18z", fill: "currentColor" }
                    }
                }
                crate::components::button::Button { id: "bell", r#type: "button", "aria-expanded": panel, "aria-controls": "activity-panel",
                    actions: crate::entry_events::click(move || { if panel { remember("reviewActivityOpen", &JsValue::FALSE); publish("review:activity"); } else { open_activity(); } }),
                    svg { width: "14", height: "14", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round", "aria-hidden": "true",
                        path { d: "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" }
                        path { d: "M10.3 21a1.94 1.94 0 0 0 3.4 0" }
                    }
                    " Activity"
                    if unread() { span { class: "dot ok", "aria-label": "Unread activity" } }
                }
            }
            div { class: "frame-page", {children} }
            if has_pending && snapshot("reviewPanelReady").as_bool() != Some(true) {
                nav { class: "pending-nav frame-pending", "aria-label": "Cards waiting on you",
                    crate::components::button::Button { r#type: "button", "data-pending-step": "prev", title: "Previous pending decision ([)", "← prev" }
                    crate::components::button::Button { r#type: "button", "data-pending-step": "next", title: "Next pending decision (])", "next →" }
                    a { href: "#top", "↑ top" }
                }
            }
            crate::components::button::Button { id: "quick-open", r#type: "button", "aria-label": "Chat or add a task", "aria-controls": "quick-sheet", "aria-expanded": open,
                actions: crate::entry_events::click(quick_open), "+" }
        }
    }
}
