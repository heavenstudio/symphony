//! What the chat's panel and the menus share of the page: the styles every page takes from the
//! program's head, and the events the page's header (the web client's) and the panel tell each
//! other by. The conversation, its state and the activity and pending views live in the panel
//! module (`frame_panel.rs`), loaded on interaction or a toast.

use dioxus::prelude::*;
#[cfg(any(feature = "page-panel", feature = "page-overlay"))]
use {
    crate::request::field,
    futures_channel::mpsc::unbounded,
    futures_util::StreamExt,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

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
const BOARD: Asset = asset!(
    "/assets/board.css",
    AssetOptions::css().with_static_head(true)
);

/// Keep the styles every page takes as they are (package.sh's shared sheet: the frame's, the
/// controls' theme and the board's rules, whose class names the web client's markup uses) in the
/// program's binary, so dx links each from the page's head when it builds (`with_static_head`),
/// though no part of the program draws them all.
pub fn keep_styles() {
    std::hint::black_box((&STYLE, &COMPONENT_THEME, &BOARD));
}

/// `review:asks`'s news read: `true`, or the region's data.
#[cfg(feature = "page-panel")]
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
#[cfg(feature = "page-panel")]
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

#[cfg(any(feature = "page-panel", feature = "page-overlay"))]
pub(super) fn snapshot(key: &str) -> JsValue {
    web_sys::window().map_or(JsValue::UNDEFINED, |w| field(w.as_ref(), key))
}

#[cfg(feature = "page-panel")]
pub(super) fn publish(event: &str) {
    if let Some(window) = web_sys::window()
        && let Ok(event) = web_sys::Event::new(event)
    {
        let _ = window.dispatch_event(&event);
    }
}

#[cfg(any(feature = "page-panel", feature = "page-overlay"))]
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

#[cfg(feature = "page-panel")]
fn array(value: JsValue) -> js_sys::Array {
    if js_sys::Array::is_array(&value) {
        value.unchecked_into()
    } else {
        js_sys::Array::new()
    }
}
