//! Each cached page-kind module keeps a tiny Dioxus root. Deactivation unmounts its app and
//! closes its feed before resolving; activation mounts it again from the shell's frame snapshot.

use dioxus::prelude::*;
use futures_channel::mpsc::{UnboundedSender, unbounded};
use futures_util::StreamExt;
use wasm_bindgen::{JsValue, closure::Closure};

struct Switch {
    shown: bool,
    done: js_sys::Function,
}

thread_local! {
    static CHANNEL: std::cell::RefCell<Option<UnboundedSender<Switch>>> = const { std::cell::RefCell::new(None) };
    static QUEUED: std::cell::RefCell<Vec<Switch>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn change(shown: bool) -> js_sys::Promise {
    js_sys::Promise::new(&mut |done, _| {
        let next = Switch { shown, done };
        CHANNEL.with(|channel| {
            if let Some(channel) = channel.borrow().as_ref() {
                let _ = channel.unbounded_send(next);
            } else {
                QUEUED.with(|queued| queued.borrow_mut().push(next));
            }
        });
    })
}

#[component]
fn ModuleRoot() -> Element {
    let mut shown = use_signal(|| false);
    let mut done = use_signal(|| None::<js_sys::Function>);
    use_hook(move || {
        let (send, mut recv) = unbounded::<Switch>();
        QUEUED.with(|queued| {
            for next in queued.borrow_mut().drain(..) {
                let _ = send.unbounded_send(next);
            }
        });
        CHANNEL.with(|channel| *channel.borrow_mut() = Some(send));
        spawn(async move {
            while let Some(next) = recv.next().await {
                if !next.shown {
                    crate::frame::save_session();
                }
                done.set(Some(next.done));
                shown.set(next.shown);
            }
        });
    });
    // Effects run after the frame's DOM edits and scope teardown. The shell waits for this
    // acknowledgement before mounting another kind, so page feeds never overlap.
    use_effect(move || {
        let _ = shown();
        if let Some(done) = done.take() {
            let _ = done.call0(&JsValue::UNDEFINED);
        }
    });
    #[cfg(feature = "page-panel")]
    return rsx! { if shown() { crate::frame::PanelApp {} } };
    #[cfg(all(feature = "page-overlay", not(feature = "page-panel")))]
    return rsx! { if shown() { crate::components::deferred::OverlayApp {} } };
    #[cfg(all(
        feature = "page-timeline",
        not(any(feature = "page-panel", feature = "page-overlay"))
    ))]
    return rsx! { if shown() { crate::task_timeline::TimelineApp {} } };
    #[cfg(not(any(
        feature = "page-panel",
        feature = "page-overlay",
        feature = "page-timeline"
    )))]
    rsx! { if shown() { crate::live::App {} } }
}

/// The shell caches imports for the document's lifetime. Each module's callbacks and empty
/// root live as long; only the active page's components, requests and feed remain mounted.
pub(crate) fn kind() -> &'static str {
    if cfg!(feature = "page-panel") {
        "panel"
    } else if cfg!(feature = "page-overlay") {
        "overlay"
    } else if cfg!(feature = "page-timeline") {
        "timeline"
    } else if cfg!(feature = "page-board") {
        "board"
    } else if cfg!(feature = "page-tasks") {
        "tasks"
    } else if cfg!(feature = "page-docs") {
        "docs"
    } else if cfg!(feature = "page-queue") {
        "queue"
    } else {
        "review"
    }
}

pub fn install() {
    let kind = kind();
    let start = Closure::<dyn FnMut() -> js_sys::Promise>::new(|| change(true));
    let stop = Closure::<dyn FnMut() -> js_sys::Promise>::new(|| change(false));
    let callbacks = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&callbacks, &"run".into(), start.as_ref());
    let _ = js_sys::Reflect::set(&callbacks, &"stop".into(), stop.as_ref());
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(&window, &format!("reviewModule_{kind}").into(), &callbacks);
    }
    start.forget();
    stop.forget();
    // The shell's PERSISTENT roots: the chat panel's, the overlay host's and the task timeline's
    // (which the shell moves into the task page's box).
    let root = match kind {
        "panel" => "quick-root".into(),
        "overlay" => "overlay-root".into(),
        "timeline" => "timeline-root".into(),
        _ => format!("review-root-{kind}"),
    };
    dioxus::web::launch::launch_cfg(ModuleRoot, dioxus::web::Config::default().rootname(root));
}
