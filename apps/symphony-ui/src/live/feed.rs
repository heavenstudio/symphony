//! The board's feed in the browser: an `EventSource` on the page's project's feed, each event it
//! hears sent on as [`Heard`].

use futures_channel::mpsc::UnboundedSender;
use symphony_api::{FEED_EVENT, PING_EVENT, SERVER_EVENT};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Event, EventSource, MessageEvent};

use super::Heard;

/// The feed's stream and its listener, which sends on what the stream says, each event's data
/// parsed: closed when dropped, so a board no longer shown, or a stream lost, stops listening.
pub(crate) struct Feed {
    source: EventSource,
    listener: Closure<dyn FnMut(Event)>,
}

impl Feed {
    pub(crate) fn open(url: &str, sender: UnboundedSender<Heard>) -> Option<Feed> {
        // Resolve the origin before handing the URL to browser/desktop domain guards.
        // The feed is the page's project's, under its base on the one server.
        let url = crate::project::current().here(url);
        let base = web_sys::window()?.location().href().ok()?;
        let url = web_sys::Url::new_with_base(&url, &base).ok()?.href();
        let feed = Feed {
            source: EventSource::new(&url).ok()?,
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
        };
        for name in ["open", "error", PING_EVENT, FEED_EVENT, SERVER_EVENT] {
            feed.source
                .add_event_listener_with_callback(name, feed.listener.as_ref().unchecked_ref())
                .ok()?;
        }
        Some(feed)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.source.close();
    }
}
