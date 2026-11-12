//! Small DOM event attributes for controls that need no rich event data. Dioxus still owns
//! listener updates, dispatch and teardown. Inputs read their own node during dispatch.

use dioxus::core::AttributeValue;
use dioxus::prelude::*;

pub(crate) fn click(handler: impl FnMut() + 'static) -> Vec<Attribute> {
    action("onclick", handler)
}

pub(crate) fn action(event: &'static str, mut handler: impl FnMut() + 'static) -> Vec<Attribute> {
    vec![Attribute::new(
        event,
        AttributeValue::listener(move |_: Event<dioxus::html::PlatformEventData>| handler()),
        None,
        false,
    )]
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn marker() -> String {
    thread_local! { static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
    NEXT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        // The persistent panel and page are separate WASM memories in the same document.
        // Their counters must not select each other's controls or visibility sentinels.
        format!("{}-{id}", crate::module::kind())
    })
}

/// The callback reads the input synchronously, so fast edits cannot collapse into the last
/// value while a task waits. A stable, per-control marker avoids active-element assumptions.
pub(crate) fn use_value(
    event: &'static str,
    mut handler: impl FnMut(String) + 'static,
) -> Vec<Attribute> {
    #[cfg(target_arch = "wasm32")]
    {
        let id = use_hook(marker);
        let selector = format!("[data-entry-input='{id}']");
        vec![
            Attribute::new("data-entry-input", id, None, false),
            Attribute::new(
                event,
                AttributeValue::listener(move |_: Event<dioxus::html::PlatformEventData>| {
                    if let Some(element) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.query_selector(&selector).ok().flatten())
                        && let Ok(value) = js_sys::Reflect::get(&element, &"value".into())
                        && let Some(value) = value.as_string()
                    {
                        handler(value);
                    }
                }),
                None,
                false,
            ),
        ]
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        vec![Attribute::new(
            event,
            AttributeValue::listener(move |event: Event<dioxus::html::PlatformEventData>| {
                handler(FormData::from(event.data().as_ref()).value());
            }),
            None,
            false,
        )]
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn use_visibility(initial: bool) -> (Signal<bool>, Vec<Attribute>) {
    use futures_channel::mpsc::unbounded;
    use futures_util::StreamExt;
    use wasm_bindgen::{JsCast, closure::Closure};

    let mut shown = use_signal(|| initial);
    let id = use_hook(marker);
    let observer = use_hook(|| {
        let (send, mut recv) = unbounded();
        spawn(async move {
            while recv.next().await.is_some() {
                shown.set(true);
            }
        });
        let listener = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
            if entries.iter().any(|entry| {
                entry
                    .unchecked_into::<web_sys::IntersectionObserverEntry>()
                    .is_intersecting()
            }) {
                let _ = send.unbounded_send(());
            }
        });
        let observer = web_sys::IntersectionObserver::new(listener.as_ref().unchecked_ref()).ok();
        std::rc::Rc::new((observer, listener))
    });
    let attached = observer.clone();
    let selector = format!("[data-entry-visible='{id}']");
    let mut effect = use_effect(move || {
        if let Some(observer) = &attached.0 {
            observer.disconnect();
            let _ = observer.take_records();
            if !shown()
                && let Some(element) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.query_selector(&selector).ok().flatten())
            {
                observer.observe(&element);
            }
        } else if !*shown.peek() {
            // A browser without IntersectionObserver must still expose its content.
            shown.set(true);
        }
    });
    // Props may replace a media node without changing the latch (for example a mockup
    // becoming a picture). Reattach after that render's DOM edits as well.
    effect.mark_dirty();
    use_drop(move || {
        if let Some(observer) = &observer.0 {
            observer.disconnect();
            // Disconnect stops observation; drain queued records before dropping the callback.
            let _ = observer.take_records();
        }
    });
    (
        shown,
        vec![Attribute::new("data-entry-visible", id, None, false)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::core::{ElementId, NoOpMutations};
    use std::{
        any::Any,
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Clone)]
    struct Control {
        value: Rc<Cell<usize>>,
        calls: Rc<RefCell<Vec<usize>>>,
    }

    fn control(props: Control) -> Element {
        let value = props.value.get();
        rsx! { crate::components::button::Button {
            id: "run", disabled: false,
            actions: click(move || props.calls.borrow_mut().push(value)), "Run"
        } }
    }

    #[test]
    fn a_reused_listener_calls_the_current_render_once() {
        let value = Rc::new(Cell::new(3));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut dom = VirtualDom::new_with_props(
            control,
            Control {
                value: value.clone(),
                calls: calls.clone(),
            },
        );
        dom.rebuild(&mut NoOpMutations);
        let dispatch = |dom: &VirtualDom| {
            let data = Rc::new(dioxus::html::PlatformEventData::new(Box::new(()))) as Rc<dyn Any>;
            dom.runtime()
                .handle_event("click", Event::new(data, true), ElementId(1));
        };
        dispatch(&dom);
        value.set(8);
        dom.mark_dirty(ScopeId::APP);
        dom.render_immediate(&mut NoOpMutations);
        dispatch(&dom);
        assert_eq!(*calls.borrow(), [3, 8]);
    }
}
