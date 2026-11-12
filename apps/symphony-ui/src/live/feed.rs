//! The board's feed in the browser: an `EventSource` on the page's project's feed, each event it
//! hears sent on as [`Heard`].
//!
//! A page that goes away closes its stream: left (`pagehide`, which every browser fires as a
//! page is left, whether for good or into its back/forward cache) or frozen (Chrome's `freeze`).
//! A page kept for back and forward with its stream open would hold one of the browser's six
//! connections to the server (HTTP/1.1) for as long as it is kept, and the page after five such
//! full loads in one tab would wait for its reads behind streams that never end: its data, and
//! the chat's module once asked for. The stream ends as [`Heard::Away`], and the page reads again
//! from a fresh snapshot once it is back (`pageshow`, `resume`: [`back`]), as after any lost
//! stream. Hidden alone (another tab in front) the page keeps its stream: the home page notifies
//! the owner from the background.

use std::cell::Cell;

use futures_channel::mpsc::UnboundedSender;
use symphony_api::{FEED_EVENT, PING_EVENT, SERVER_EVENT};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Event, EventSource, EventTarget, MessageEvent};

use super::Heard;

/// The events that say the page went away, each on its target: the window's, then the document's.
const LEAVING: [&str; 2] = ["pagehide", "freeze"];
/// The events that say the page is back, as [`LEAVING`].
const RETURNING: [&str; 2] = ["pageshow", "resume"];

thread_local! {
    /// Whether the page is away: left or frozen, and not shown or resumed since.
    static AWAY: Cell<bool> = const { Cell::new(false) };
    /// Whether this module listens for the page going away and coming back.
    static WATCHED: Cell<bool> = const { Cell::new(false) };
}

/// The window, then the document: where [`LEAVING`] and [`RETURNING`] are heard.
fn targets() -> Option<[EventTarget; 2]> {
    let window = web_sys::window()?;
    let document = window.document()?;
    Some([window.into(), document.into()])
}

/// Listen, once and for the module's life, for the page going away and coming back.
fn watch() {
    if WATCHED.replace(true) {
        return;
    }
    let Some(targets) = targets() else {
        return;
    };
    for (names, away) in [(LEAVING, true), (RETURNING, false)] {
        for (target, name) in targets.iter().zip(names) {
            let heard = Closure::<dyn FnMut(Event)>::new(move |_: Event| AWAY.set(away));
            let _ = target.add_event_listener_with_callback(name, heard.as_ref().unchecked_ref());
            heard.forget();
        }
    }
}

/// Whether the page is away now: left (kept for back and forward, or unloading) or frozen.
pub(crate) fn away() -> bool {
    watch();
    AWAY.get()
}

/// Wait until the page is back, if it is away. A page reads only once it is: a stream opened
/// while it is away would be held by a page nobody sees.
pub(crate) async fn back() {
    while away() {
        super::sleep(100).await;
    }
}

/// The feed's stream and its listeners, which send on what the stream says, each event's data
/// parsed: closed when dropped, so a board no longer shown, or a stream lost, stops listening,
/// and when the page goes away.
pub(crate) struct Feed {
    source: EventSource,
    listener: Closure<dyn FnMut(Event)>,
    /// Closes the stream as the page goes away, and says so.
    leaving: Closure<dyn FnMut(Event)>,
}

impl Feed {
    /// The stream at `url`, or none when it cannot be opened or the page is away.
    pub(crate) fn open(url: &str, sender: UnboundedSender<Heard>) -> Option<Feed> {
        if away() {
            return None;
        }
        // Resolve the origin before handing the URL to browser/desktop domain guards.
        // The feed is the page's project's, under its base on the one server.
        let url = crate::project::current().here(url);
        let base = web_sys::window()?.location().href().ok()?;
        let url = web_sys::Url::new_with_base(&url, &base).ok()?.href();
        let source = EventSource::new(&url).ok()?;
        let leaving = {
            let (source, sender) = (source.clone(), sender.clone());
            Closure::new(move |_: Event| {
                // Before the page is put away, whichever of its listeners runs first.
                AWAY.set(true);
                source.close();
                let _ = sender.unbounded_send(Heard::Away);
            })
        };
        let feed = Feed {
            source,
            listener: Closure::new(move |event: Event| {
                let said = match event.type_().as_str() {
                    "open" => Heard::Open,
                    PING_EVENT => Heard::Ping,
                    FEED_EVENT | SERVER_EVENT => match event
                        .dyn_ref::<MessageEvent>()
                        .and_then(|message| message.data().as_string())
                        .and_then(|text| js_sys::JSON::parse(&text).ok())
                    {
                        Some(value) if event.type_() == SERVER_EVENT => match value.as_string() {
                            Some(message) => Heard::Server(message),
                            None => Heard::Lost,
                        },
                        Some(value) => Heard::Delta(value),
                        None => Heard::Lost,
                    },
                    _ => Heard::Lost,
                };
                let _ = sender.unbounded_send(said);
            }),
            leaving,
        };
        for name in ["open", "error", PING_EVENT, FEED_EVENT, SERVER_EVENT] {
            feed.source
                .add_event_listener_with_callback(name, feed.listener.as_ref().unchecked_ref())
                .ok()?;
        }
        for (target, name) in targets()?.iter().zip(LEAVING) {
            target
                .add_event_listener_with_callback(name, feed.leaving.as_ref().unchecked_ref())
                .ok()?;
        }
        Some(feed)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.source.close();
        if let Some(targets) = targets() {
            for (target, name) in targets.iter().zip(LEAVING) {
                let _ = target.remove_event_listener_with_callback(
                    name,
                    self.leaving.as_ref().unchecked_ref(),
                );
            }
        }
    }
}
