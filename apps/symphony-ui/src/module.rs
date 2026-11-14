//! Each cached page-kind module keeps a tiny Dioxus root. Deactivation unmounts its app and
//! closes its feed before resolving; activation mounts it again from the shell's frame snapshot.
//! The program (`page-program`, src/split.rs) keeps one such root for each of its kinds, in its one
//! instance.

#[cfg(not(feature = "page-program"))]
use dioxus::prelude::*;
#[cfg(not(feature = "page-program"))]
use futures_channel::mpsc::{UnboundedSender, unbounded};
#[cfg(not(feature = "page-program"))]
use futures_util::StreamExt;
#[cfg(not(feature = "page-program"))]
use wasm_bindgen::{JsValue, closure::Closure};

struct Switch {
    shown: bool,
    done: js_sys::Function,
}

#[cfg(not(feature = "page-program"))]
thread_local! {
    static CHANNEL: std::cell::RefCell<Option<UnboundedSender<Switch>>> = const { std::cell::RefCell::new(None) };
    static QUEUED: std::cell::RefCell<Vec<Switch>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(not(feature = "page-program"))]
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

#[cfg(not(feature = "page-program"))]
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
    if cfg!(feature = "page-program") {
        // One instance for all its kinds: one counter for their markers and ids.
        "program"
    } else if cfg!(feature = "page-panel") {
        "panel"
    } else if cfg!(feature = "page-overlay") {
        "overlay"
    } else if cfg!(feature = "page-timeline") {
        "timeline"
    } else if cfg!(feature = "page-tasks") {
        "tasks"
    } else if cfg!(feature = "page-docs") {
        "docs"
    } else {
        "review"
    }
}

#[cfg(not(feature = "page-program"))]
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

#[cfg(feature = "page-program")]
pub use program::install;

/// The program's kinds, each a Dioxus root of its own in the one instance, launched on the shell's
/// first `run` of it (its root exists only once the shell has made it), with a channel of its own.
/// A kind's `run` loads its chunk before its root shows anything: a chunk that fails to load
/// rejects `run`, which the shell's catches take to its `gone` state.
#[cfg(feature = "page-program")]
mod program {
    use std::cell::RefCell;

    use dioxus::prelude::*;
    use futures_channel::mpsc::{UnboundedSender, unbounded};
    use futures_util::StreamExt;
    use wasm_bindgen::{JsValue, closure::Closure};
    use wasm_bindgen_futures::JsFuture;

    use super::Switch;
    use crate::split::Chunk;

    /// A kind's channel once its root has mounted, and the switches asked of it before.
    type Slot = (Chunk, Option<UnboundedSender<Switch>>, Vec<Switch>);

    thread_local! {
        static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
    }

    fn change(chunk: Chunk, shown: bool) -> js_sys::Promise {
        js_sys::Promise::new(&mut |done, _| {
            let next = Switch { shown, done };
            let launch = SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                match slots.iter_mut().find(|(of, _, _)| *of == chunk) {
                    Some((_, Some(channel), _)) => {
                        let _ = channel.unbounded_send(next);
                        false
                    }
                    Some((_, None, queued)) => {
                        queued.push(next);
                        false
                    }
                    // A kind never shown has nothing to stop.
                    None if !shown => {
                        let _ = next.done.call0(&JsValue::UNDEFINED);
                        false
                    }
                    None => {
                        slots.push((chunk, None, vec![next]));
                        true
                    }
                }
            });
            if launch {
                dioxus::web::launch::launch_virtual_dom(
                    VirtualDom::new_with_props(KindRoot, KindRootProps { chunk }),
                    dioxus::web::Config::default().rootname(chunk.root()),
                );
            }
        })
    }

    /// The kind's chunk in, then its root shown; rejected when the chunk fails to load.
    fn run(chunk: Chunk) -> js_sys::Promise {
        wasm_bindgen_futures::future_to_promise(async move {
            chunk.load().await?;
            JsFuture::from(change(chunk, true)).await
        })
    }

    #[component]
    fn KindRoot(chunk: Chunk) -> Element {
        let mut shown = use_signal(|| false);
        let mut done = use_signal(|| None::<js_sys::Function>);
        use_hook(move || {
            let (send, mut recv) = unbounded::<Switch>();
            SLOTS.with(|slots| {
                if let Some((_, channel, queued)) = slots
                    .borrow_mut()
                    .iter_mut()
                    .find(|(of, _, _)| *of == chunk)
                {
                    for next in queued.drain(..) {
                        let _ = send.unbounded_send(next);
                    }
                    *channel = Some(send);
                }
            });
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
        // As a module's root does: the shell waits for this before it mounts another kind.
        use_effect(move || {
            let _ = shown();
            if let Some(done) = done.take() {
                let _ = done.call0(&JsValue::UNDEFINED);
            }
        });
        if !shown() {
            return rsx! {};
        }
        match chunk {
            Chunk::Tasks | Chunk::Reports => rsx! { crate::live::App {} },
            Chunk::Panel => rsx! { crate::split::PanelPart {} },
            Chunk::Overlay => rsx! { crate::split::OverlayPart {} },
            Chunk::Timeline => rsx! { crate::split::TimelinePart {} },
        }
    }

    pub fn install() {
        for chunk in Chunk::ALL {
            let start = Closure::<dyn FnMut() -> js_sys::Promise>::new(move || run(chunk));
            let stop = Closure::<dyn FnMut() -> js_sys::Promise>::new(move || change(chunk, false));
            let callbacks = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&callbacks, &"run".into(), start.as_ref());
            let _ = js_sys::Reflect::set(&callbacks, &"stop".into(), stop.as_ref());
            if let Some(window) = web_sys::window() {
                let _ = js_sys::Reflect::set(
                    &window,
                    &format!("reviewModule_{}", chunk.kind()).into(),
                    &callbacks,
                );
            }
            start.forget();
            stop.forget();
        }
    }
}
