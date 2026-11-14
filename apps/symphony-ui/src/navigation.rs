//! Typed routes for the active page module. shell.js owns browser links and history once for the
//! whole SPA; modules subscribe only while mounted and drop the subscription with their runtime.

use dioxus::prelude::*;
use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::Event;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Route {
    pub path: String,
    pub scope: String,
    /// Whether it is a document or folder page of an entrypoint
    /// ([`symphony_api::ProjectView::is_document_route`]).
    pub document: bool,
    /// Whether that page is a folder's rather than a document's
    /// ([`symphony_api::ProjectView::is_folder_route`]).
    pub folder: bool,
}

impl Route {
    pub fn parse(path: &str) -> Option<Self> {
        let decoded = js_sys::decode_uri_component(path).ok().map(String::from)?;
        let mut normalized = String::with_capacity(decoded.len());
        for ch in decoded.chars() {
            if ch != '/' || !normalized.ends_with('/') {
                normalized.push(ch);
            }
        }
        // The one server serves the project under its base, `/<id>`: the route is below it.
        let project = crate::project::current();
        let path = project.strip(normalized.as_str());
        let mut path = path.trim_start_matches('/');
        path = path.strip_prefix("app/").unwrap_or(path);
        let (scope, path) = if let Some(rest) = path.strip_prefix('@') {
            let (scope, path) = rest.split_once('/')?;
            (scope.to_string(), path.strip_prefix("app/").unwrap_or(path))
        } else {
            (String::new(), path)
        };
        let slashed = path.strip_prefix("doc/").unwrap_or(path);
        let path = slashed.strip_suffix('/').unwrap_or(slashed);
        // the route as the server answers it: a trailing slash names a folder page, whatever its
        // name holds
        let named = match crate::project::nav_page(&project.nav, path) {
            Some(item) => item.document.as_str(),
            None if path == "task" => "tasks",
            None => slashed,
        };
        let (document, folder) = (
            project.is_document_route(named),
            project.is_folder_route(named),
        );
        let path = named.strip_suffix('/').unwrap_or(named);
        (path.is_empty()
            || path == "tasks"
            || path == "queue"
            || path == "reports"
            || path == "agents"
            || path.starts_with("task/")
            || document)
            .then(|| Self {
                path: path.into(),
                scope,
                document,
                folder,
            })
    }
    pub fn current() -> Self {
        web_sys::window()
            .and_then(|w| w.location().pathname().ok())
            .and_then(|p| Self::parse(&p))
            .unwrap_or_default()
    }
}

pub fn use_navigation() -> Signal<Route> {
    let mut route = use_signal(Route::current);
    let listener = use_hook(move || {
        let (send, mut recv) = unbounded();
        spawn(async move {
            while let Some(next) = recv.next().await {
                route.set(next);
            }
        });
        let handler = Closure::<dyn FnMut(Event)>::new(move |_| {
            let _ = send.unbounded_send(Route::current());
        });
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "review:navigate",
                handler.as_ref().unchecked_ref(),
            );
        }
        std::rc::Rc::new(handler)
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "review:navigate",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    route
}

/// Card-to-card navigation can change only the fragment, without remounting the document.
pub fn use_anchor() -> Signal<String> {
    fn anchor() -> String {
        web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .and_then(|hash| js_sys::decode_uri_component(hash.trim_start_matches('#')).ok())
            .map(String::from)
            .unwrap_or_default()
    }
    let mut current = use_signal(anchor);
    let listener = use_hook(move || {
        let (send, mut recv) = unbounded();
        spawn(async move {
            while let Some(next) = recv.next().await {
                current.set(next);
            }
        });
        let handler = Closure::<dyn FnMut(Event)>::new(move |_| {
            let _ = send.unbounded_send(anchor());
        });
        if let Some(window) = web_sys::window() {
            for name in ["hashchange", "review:navigate"] {
                let _ =
                    window.add_event_listener_with_callback(name, handler.as_ref().unchecked_ref());
            }
        }
        std::rc::Rc::new(handler)
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            for name in ["hashchange", "review:navigate"] {
                let _ = window.remove_event_listener_with_callback(
                    name,
                    listener.as_ref().as_ref().unchecked_ref(),
                );
            }
        }
    });
    current
}

pub fn scroll_anchor() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else {
        return;
    };
    let hash = window.location().hash().unwrap_or_default();
    let anchor = js_sys::decode_uri_component(hash.trim_start_matches('#'))
        .ok()
        .map(String::from)
        .unwrap_or_default();
    if anchor.is_empty() {
        // The shell owns the guarded top reset. A late content load must not undo a scroll
        // made since navigation; this callback only locates a requested fragment.
        return;
    }
    if let Some(element) = doc.get_element_by_id(&anchor) {
        if let Some(details) = element.dyn_ref::<web_sys::HtmlDetailsElement>() {
            details.set_open(true);
        }
        let mut parent = element.parent_element();
        while let Some(one) = parent {
            if let Some(details) = one.dyn_ref::<web_sys::HtmlDetailsElement>() {
                details.set_open(true);
            }
            parent = one.parent_element();
        }
        element.scroll_into_view();
    }
}

/// Whether the reader is busy on the page (a field, an open form or list): a new build waits.
#[cfg(any(feature = "page-tasks", feature = "page-docs", feature = "page-panel"))]
pub(crate) fn engaged() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|doc| {
            doc.query_selector(
                "[data-busy=true], .section-prose details[open], .history details[open], [role=listbox][data-state=open]",
            )
            .ok()
            .flatten()
            .is_some()
                || doc.active_element().is_some_and(|active| {
                    active.matches("input, textarea, select, button[aria-haspopup=listbox], [role=listbox], [role=option]").unwrap_or(false)
                })
        })
}
