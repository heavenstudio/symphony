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
        let refresh = self.1.write().connected();
        if refresh {
            self.refresh();
        }
    }
    pub fn heard(self, value: &JsValue) {
        let changes = field(value, "changes");
        if js_sys::Array::is_array(&changes)
            && js_sys::Array::from(&changes).iter().any(|c| {
                matches!(
                    field(&c, "region").as_string().as_deref(),
                    Some("activity" | "pending" | "docs")
                )
            })
        {
            self.refresh();
        }
    }
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

fn quick_open() {
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
                    remember("reviewPreviousIds", &previous);
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
    let has_pending = array(field(&snapshot("reviewPendingSnapshot"), "items")).length() > 0;
    let project = crate::project::current();
    let plugin_page = crate::project::nav_for_document(&project.nav, &route.path);
    let crumbs: Vec<_> = if let Some(item) = plugin_page {
        vec![item.href.trim_matches('/')]
    } else if route.path.is_empty() {
        vec!["board"]
    } else {
        route.path.trim_end_matches('/').split('/').collect()
    };
    let mut links = vec![
        (
            "/".to_string(),
            "⌂ board".to_string(),
            route.path.is_empty() && !project.is_document_route(""),
        ),
        (
            project.documents_href(),
            "☰ docs".into(),
            project.is_document_route(&route.path) && plugin_page.is_none(),
        ),
        (
            "/tasks".into(),
            "▦ tasks".into(),
            route.path == "tasks" || route.path.starts_with("task/"),
        ),
        ("/agents".into(), "◉ agents".into(), route.path == "agents"),
    ];
    links.extend(project.nav.iter().map(|item| {
        (
            item.href.clone(),
            item.label.clone(),
            item.document == route.path,
        )
    }));
    rsx! {
        div { class: if open { "app-frame quick-expanded" } else { "app-frame" },
            header { class: "app-header", id: "top",
                for (href, label, active) in links {
                    a { href: if matches!(href.as_str(), "/" | "/tasks" | "/agents") { href.clone() } else { docs::scoped(&route.scope, &href) }, class: if active { "active" } else { "" }, "{label}" }
                }
                nav { class: "crumbs", "aria-label": "Breadcrumbs",
                    for (index, crumb) in crumbs.iter().enumerate() {
                        if index > 0 { " / " }
                        if index + 1 < crumbs.len() {
                            a { href: docs::scoped(&route.scope, &format!("/{}/", crumbs[..=index].join("/"))), "{crumb}" }
                        } else { span { "{crumb}" } }
                    }
                }
                crate::components::button::Button { id: "bell", r#type: "button", "aria-expanded": panel, "aria-controls": "activity-panel",
                    actions: crate::entry_events::click(move || { remember("reviewActivityOpen", &JsValue::from_bool(!panel)); publish("review:activity"); }),
                    svg { width: "14", height: "14", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round", "aria-hidden": "true",
                        path { d: "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" }
                        path { d: "M10.3 21a1.94 1.94 0 0 0 3.4 0" }
                    }
                    " activity"
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
