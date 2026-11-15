//! A tap that chooses an option keeps its click. The popup chooses on the press's `pointerup` and
//! closes; the `click` the browser sends after a finger's or a pen's tap is aimed where the option
//! was, and with the popup gone it would land on whatever lay under the option: the Add task form's
//! Send, which sent the form when a tap chose its priority on a phone, or a card on the board under
//! a filter's popup. (A mouse's click goes to no element once the popup that held its press is
//! gone.) The first popup a page draws puts four listeners on the document for good, ahead of
//! every other: a finger's or a pen's `pointerup` on an option of a select's popup arms them, the
//! browser's next `click` is then swallowed, and the next press or key disarms them, so a click of
//! the reader's own goes through, as does one a script makes. An option's own click, where the
//! press chose nothing and the popup stays, is swallowed too: the option does nothing with it.

use std::cell::Cell;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Element, Event};

/// An option of a select's popup (`SelectPopup`'s root carries the marker).
const OPTION: &str = "[data-symphony-control=select] [role=option]";

thread_local! {
    static KEPT: Cell<bool> = const { Cell::new(false) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Whether `event`, a `pointerup`, was a finger's or a pen's on an option of a select's popup.
fn tapped_option(event: &Event) -> bool {
    let pointer = js_sys::Reflect::get(event, &"pointerType".into())
        .ok()
        .and_then(|kind| kind.as_string());
    pointer.as_deref() != Some("mouse")
        && event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|element| element.closest(OPTION).ok().flatten())
            .is_some()
}

/// Keep each press that chooses an option from clicking what lies under it, once per page.
pub(super) fn keep_taps() {
    if KEPT.with(|kept| kept.replace(true)) {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let listen = |name: &str, heard: fn(&Event)| {
        let listener = Closure::<dyn FnMut(Event)>::new(move |event: Event| heard(&event));
        let _ = document.add_event_listener_with_callback_and_bool(
            name,
            listener.as_ref().unchecked_ref(),
            true,
        );
        // The page's for as long as it lives, as the guard is.
        listener.forget();
    };
    listen("pointerup", |event| {
        if tapped_option(event) {
            ARMED.set(true);
        }
    });
    listen("pointerdown", |_| ARMED.set(false));
    listen("keydown", |_| ARMED.set(false));
    listen("click", |event| {
        if event.is_trusted() && ARMED.replace(false) {
            event.prevent_default();
            event.stop_propagation();
        }
    });
}
