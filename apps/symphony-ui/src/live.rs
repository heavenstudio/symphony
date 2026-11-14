//! The board, live: fetched from the server's JSON API, then followed by its feed, one event at a
//! time, for the task pages, which apply each to what they show (`task_live::apply`). The browser
//! half of the app; home is the web client's (apps/symphony-web).
//!
//! A lost feed (it errs, falls silent, or sends what the client cannot read) is never trusted to
//! resume (symphony-api's contract): the client closes it, shows the board as stale, and starts
//! again from a fresh board.
//!
//! JSON is read by the browser (`Response.json`, `JSON.parse`) and turned into the wire types by
//! serde-wasm-bindgen: serde_json's parser in the wasm cost 11 KB gzipped of the first visit's
//! 250 KB budget.

use std::cell::Cell;
use std::cell::RefCell;
#[cfg(feature = "page-tasks")]
use std::pin::pin;
use std::rc::Rc;

use dioxus::prelude::*;
#[cfg(feature = "page-tasks")]
use symphony_api::{BOARD_PATH, FEED_PATH};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::Event;

mod feed;
pub(crate) use feed::{Feed, back};

pub(crate) const FEED_NOTICE: &str = "Reconnecting: the board may be out of date";
const REMOVED_DOCUMENT_NOTICE: &str =
    "This document was removed. Your draft is kept on this device.";
#[cfg(feature = "page-tasks")]
use {
    futures_channel::mpsc::unbounded,
    futures_util::{
        StreamExt,
        future::{Either, select},
    },
};

use crate::state::reload_for;
#[cfg(feature = "page-tasks")]
use crate::task_live::Data;
#[cfg(feature = "page-tasks")]
use {
    crate::cache::{Freshness, board},
    crate::state::{Backoff, Ended},
    crate::task_live,
};

/// The style sheet. dx links it from the page's head when it builds (`with_static_head`), so the
/// browser loads it beside the wasm rather than after it; the app only keeps it in the binary,
/// which is where dx finds it.
const STYLE: Asset = asset!(
    "/assets/board.css",
    AssetOptions::css().with_static_head(true)
);

/// The performance mark set once the board is first drawn (the phone test times it).
#[cfg(feature = "page-tasks")]
const READY_MARK: &str = "board-ready";
/// Where the tab keeps the build it last loaded the page again for ([`reload_for`]).
const RELOADED_FOR: &str = "symphony-ui-reloaded-for";

thread_local! { static DEFERRED_BUILD: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub(crate) static SERVER_NOTICE: GlobalSignal<String> = Signal::global(String::new);

/// `ms` since the epoch as the wall clock where the owner is.
#[cfg(any(feature = "page-board", feature = "page-tasks"))]
pub(crate) fn local_moment(ms: f64) -> Option<crate::words::Moment> {
    let date = js_sys::Date::new(&JsValue::from_f64(ms));
    crate::words::Moment::from_parts(
        i64::from(date.get_full_year()),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes(),
    )
}

#[component]
pub fn App() -> Element {
    std::hint::black_box(&STYLE);
    let route = crate::navigation::use_navigation();
    #[cfg(feature = "page-tasks")]
    let task_route = use_memo(move || format!("/{}", route().path));
    let current = route();
    let page = match current.path.as_str() {
        #[cfg(feature = "page-board")]
        "tasks" => rsx! { crate::board_live::TasksPage { key: "{current.scope}" } },
        // The program's pages are in their chunks (src/split.rs).
        #[cfg(feature = "page-program")]
        _ => {
            rsx! { crate::split::TasksPart { key: "{current.scope}", route: task_route, scope: current.scope.clone() } }
        }
        #[cfg(all(feature = "page-tasks", not(feature = "page-program")))]
        _ => {
            rsx! { BoardPage { key: "{current.scope}", route: task_route, scope: current.scope.clone() } }
        }
        #[cfg(not(feature = "page-tasks"))]
        _ => rsx! { p { "Loading page…" } },
    };
    rsx! {
        crate::frame::Frame { route: current.clone(),
        SuspenseBoundary { fallback: |_| rsx! { main { crate::viewport::Skeleton { lines: 8, label: "Loading page…" } PageNotices { reconnecting: false } } },
        {page}
        }
        }
        MediaLayer {}
    }
}

#[component]
fn MediaLayer() -> Element {
    #[cfg(feature = "page-tasks")]
    {
        // The activation signal belongs to this component so it lives as long as its runtime.
        let needed = crate::media_live::use_activation();
        // The program's runtime is a chunk of its own, loaded once a page shows media.
        #[cfg(feature = "page-program")]
        return rsx! { if needed() { crate::split::MediaPart { needed } } };
        #[cfg(not(feature = "page-program"))]
        rsx! { if needed() { crate::media_live::MediaRuntime {} } }
    }
    #[cfg(not(feature = "page-tasks"))]
    rsx! {}
}

/// One out-of-flow stack for feed, fetch and server messages. Its matching end
/// reserve cannot shrink during this mount, so clearing an error at page end
/// cannot clamp the reader's scroll position. `device` is a page drawn from this device's cache
/// ([`crate::cache::Freshness::shown`]): its notice stands in for the reconnecting one. Either sets
/// [`crate::cache::STALE`] while it is shown, and only the notices that set it clear it, so the
/// page's notices leave it as they found it whatever mounts or unmounts beside them (the loading
/// page's, say).
#[component]
pub(crate) fn PageNotices(
    reconnecting: bool,
    #[props(default = FEED_NOTICE.to_owned())] reconnect: String,
    #[props(default)] error: String,
    #[props(default)] document_removed: bool,
    #[props(default)] device: Option<(f64, bool)>,
) -> Element {
    let server = SERVER_NOTICE.read().clone();
    let controller = use_hook(|| Rc::new(RefCell::new(None::<NoticeGeometry>)));
    use_effect(move || {
        if controller.borrow().is_none() {
            *controller.borrow_mut() = NoticeGeometry::new();
        }
    });
    // Only the kinds that draw from this device say so.
    #[cfg(any(feature = "page-board", feature = "page-tasks"))]
    let device = device.map(|(saved, unreachable)| {
        let saved = local_moment(saved)
            .zip(local_moment(js_sys::Date::now()))
            .map(|(saved, now)| crate::words::saved_at(saved, now))
            .unwrap_or_default();
        crate::words::device_notice(&saved, unreachable)
    });
    #[cfg(not(any(feature = "page-board", feature = "page-tasks")))]
    let device = device.and(None::<String>);
    let checking = reconnecting || device.is_some();
    let held = use_hook(|| Rc::new(Cell::new(false)));
    let holding = held.clone();
    use_effect(use_reactive!(|checking| {
        if holding.replace(checking) != checking {
            *crate::cache::STALE.write() = checking;
        }
    }));
    use_drop(move || {
        if held.get() {
            *crate::cache::STALE.write() = false;
        }
    });
    let visible = checking || !error.is_empty() || !server.is_empty() || document_removed;
    let waiting = device.clone().unwrap_or_else(|| reconnect.clone());
    rsx! {
        div { class: "stale page-notices notice-stack", hidden: !visible,
            if let Some(device) = &device { p { class: "pill wait device-notice", role: "status", "{device}" } }
            else if reconnecting { p { class: "pill wait", role: "status", "{reconnect}" } }
            if !error.is_empty() { p { class: "pill wait", role: "alert", "{error}" } }
            if document_removed { p { class: "pill wait document-removed", role: "alert", "{REMOVED_DOCUMENT_NOTICE}" } }
            if !server.is_empty() { p { class: "pill wait server-notice", role: "status", "{server}" } }
        }
        div { class: "feed-notice-space page-notice-reserve notice-stack", "aria-hidden": "true",
            p { class: "pill", "{waiting}" }
            if !error.is_empty() { p { class: "pill", "{error}" } }
            if document_removed { p { class: "pill", "{REMOVED_DOCUMENT_NOTICE}" } }
            if !server.is_empty() { p { class: "pill", "{server}" } }
        }
    }
}

/// Own the measurement and window listener for this page, not the persistent
/// panel. Wrapped messages and viewport changes move toasts above the actual
/// notice stack, rather than guessing how many text lines it occupies.
struct NoticeGeometry {
    observer: web_sys::ResizeObserver,
    _resized: Closure<dyn FnMut(js_sys::Array)>,
    window_resized: Closure<dyn FnMut(Event)>,
    root: web_sys::HtmlElement,
}

impl NoticeGeometry {
    fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let document = window.document()?;
        let notice = document.query_selector(".page-notices").ok()??;
        let reserve = document
            .query_selector(".page-notice-reserve")
            .ok()??
            .dyn_into::<web_sys::HtmlElement>()
            .ok()?;
        let root = document
            .document_element()?
            .dyn_into::<web_sys::HtmlElement>()
            .ok()?;
        let maximum = Cell::new(0.0_f64);
        let (r, n, w, css) = (
            reserve.clone(),
            notice.clone(),
            window.clone(),
            root.clone(),
        );
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            let height = r.get_bounding_client_rect().height();
            if height > maximum.get() {
                maximum.set(height);
                let _ = r.style().set_property("min-height", &format!("{height}px"));
            }
            let box_ = n.get_bounding_client_rect();
            let bottom = if box_.height() > 0.0 {
                w.inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map_or(66.0, |height| height - box_.top() + 8.0)
            } else {
                66.0
            };
            let _ = css
                .style()
                .set_property("--notice-toast-bottom", &format!("{bottom}px"));
        });
        let resized = {
            let sync = sync.clone();
            Closure::new(move |_: js_sys::Array| sync())
        };
        let observer = web_sys::ResizeObserver::new(resized.as_ref().unchecked_ref()).ok()?;
        observer.observe(&notice);
        observer.observe(&reserve);
        let window_resized = {
            let sync = sync.clone();
            Closure::new(move |_: Event| sync())
        };
        window
            .add_event_listener_with_callback("resize", window_resized.as_ref().unchecked_ref())
            .ok()?;
        sync();
        Some(Self {
            observer,
            _resized: resized,
            window_resized,
            root,
        })
    }
}

impl Drop for NoticeGeometry {
    fn drop(&mut self) {
        self.observer.disconnect();
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "resize",
                self.window_resized.as_ref().unchecked_ref(),
            );
        }
        let _ = self.root.style().remove_property("--notice-toast-bottom");
    }
}

#[cfg(feature = "page-tasks")]
#[derive(Clone, serde::Deserialize)]
struct PageBoard {
    version: u64,
    ping_ms: u64,
    build: String,
}

#[cfg(feature = "page-tasks")]
#[component]
pub(crate) fn BoardPage(route: Memo<String>, scope: String) -> Element {
    let frame = use_context::<crate::frame::FrameRefresh>();
    let board = use_signal(|| None::<PageBoard>);
    let stale = use_signal(|| false);
    let trouble = use_signal(|| None::<String>);
    let mut retry = use_signal(|| 0_u64);
    // Each scope's board is a page of its own (its key); a task page's slice is its own.
    let _ = scope;
    let data = use_signal(Data::default);
    use_hook(|| {
        spawn(keep_live(
            board,
            stale,
            trouble,
            PageViews {
                route,
                data,
                frame,
                retry,
            },
        ));
        spawn(async move {
            loop {
                sleep(60_000).await;
                flush_reload();
            }
        });
    });
    // Another route of the kind: drawn from this device, when it kept it, then read.
    {
        let first_route = use_hook(|| Rc::new(Cell::new(true)));
        use_effect(move || {
            let _ = route();
            if !first_route.replace(false) {
                spawn(async move {
                    task_live::opened(route, data).await;
                    task_live::load(route, data).await;
                });
            }
        });
    }
    // A task or agents view's failed fetch is a page notice, as the board's is: an alert in the
    // page's flow would shrink the page when a reconnect clears it, clamping a reader at its end.
    // A view with nothing yet to show says it in its own trouble block instead, once.
    let error = use_memo(move || {
        let data = data.read();
        if data.drawn(&route()) {
            data.error.clone()
        } else {
            String::new()
        }
    })();
    let marked = use_hook(|| Rc::new(Cell::new(false)));
    use_effect(move || {
        if board.read().is_some() && !marked.replace(true) {
            mark(READY_MARK);
        }
    });
    // A task page this device kept is drawn before the board is read.
    let (device, drawn) = (data.read().freshness.shown(), data.read().drawn(&route()));
    let content = if board.read().is_some() || drawn {
        // What the device drew is not the machine's work now: shown as out of touch until read.
        if route().starts_with("/task/") || route() == "/agents" {
            rsx! { crate::tasks::LiveTaskPage { route, data, connected: !stale() && device.is_none() } }
        } else {
            rsx! { p { "Page unavailable" } }
        }
    } else if let Some(why) = trouble() {
        rsx! { crate::trouble::Trouble { title: "The board could not be loaded", text: format!("{} It is also tried again on its own.", crate::trouble::failed(&why)), retry: move |_| retry += 1 } }
    } else {
        rsx! { crate::viewport::Skeleton { lines: 8, label: "Loading the board…" } }
    };
    // Ready once a board is drawn, or a task page this device kept.
    let ready = if board.read().is_some() || drawn {
        "board"
    } else {
        crate::trouble::ready(false, trouble.read().is_some())
    };
    rsx! { main { "data-ready": ready, "data-feed": if stale() || device.is_some() { "stale" } else { "live" },
        "data-cache": device.map(|_| "device"),
        {content}
        PageNotices { reconnecting: stale() && board.read().is_some(), error, device }
    } }
}

/// The board, read fresh: its version starts the feed.
#[cfg(feature = "page-tasks")]
async fn fetch_board() -> Result<PageBoard, String> {
    crate::request::get(&task_live::url(BOARD_PATH)).await
}

#[cfg(feature = "page-tasks")]
#[derive(Clone, Copy)]
struct PageViews {
    route: Memo<String>,
    data: Signal<Data>,
    frame: crate::frame::FrameRefresh,
    /// The reader's presses of Retry, which cut the wait before the next read.
    retry: Signal<u64>,
}

#[cfg(feature = "page-tasks")]
async fn keep_live(
    mut board: Signal<Option<PageBoard>>,
    mut stale: Signal<bool>,
    mut trouble: Signal<Option<String>>,
    views: PageViews,
) {
    let mut backoff = Backoff::default();
    // A task page draws what this device kept of its task while the board is read, and checks it
    // once the board's version is: the feed then replays every change made after the check.
    let mut opening = Some(task_live::opened(views.route, views.data));
    loop {
        back().await;
        let mut ended = Ended::Lost(0.0);
        let fetched = fetch_board();
        let fetched = async {
            match opening.take() {
                Some(opening) => futures_util::future::join(fetched, opening).await.0,
                None => fetched.await,
            }
        };
        match fetched.await {
            Ok(fetched) => {
                let since = fetched.version;
                let quiet =
                    i32::try_from(fetched.ping_ms.saturating_mul(5) / 2).unwrap_or(i32::MAX);
                reload_if_redeployed(&fetched.build);
                board.set(Some(fetched));
                freshen(views, |shown| shown.answered(board::BOARD));
                task_live::load(views.route, views.data).await;
                ended = follow(board, stale, since, quiet, views).await;
                freshen(views, Freshness::lost);
            }
            Err(why) => {
                freshen(views, |shown| shown.failed(board::BOARD));
                trouble.set(Some(why));
            }
        }
        stale.set(true);
        pause(backoff.wait(ended), views.retry).await;
    }
}

/// Tell what the page shows from this device ([`Freshness`]) what its board's read and feed did,
/// in the task page's data, which draws the whole page: written only when this changes it.
#[cfg(feature = "page-tasks")]
fn freshen(views: PageViews, change: impl Fn(&mut Freshness)) {
    let mut data = views.data;
    let mut next = data.peek().freshness.clone();
    change(&mut next);
    if next != data.peek().freshness {
        data.write().freshness = next;
    }
}

/// What the feed's stream said.
pub(crate) enum Heard {
    /// The stream is open: the board is fresh.
    Open,
    /// The server is there.
    Ping,
    /// One event's data, parsed.
    Delta(JsValue),
    Server(String),
    /// The stream failed, the server ended it, or it sent what this client cannot read.
    Lost,
    /// The page went away (left or frozen) and closed the stream ([`feed`]).
    Away,
}

/// Follow the feed from `since` until its stream is lost: when it errs, when it says nothing for
/// `quiet_ms`, or when it sends an event this client cannot read (the board fetched next has its
/// change); or until the page goes away. Each event is applied where it lands, the page loaded
/// again for another client build. How the stream ended.
#[cfg(feature = "page-tasks")]
async fn follow(
    mut board: Signal<Option<PageBoard>>,
    mut stale: Signal<bool>,
    since: u64,
    quiet_ms: i32,
    views: PageViews,
) -> Ended {
    let PageViews {
        route, data, frame, ..
    } = views;
    let (sender, mut heard) = unbounded::<Heard>();
    let Some(_feed) = Feed::open(
        &task_live::url(&format!("{FEED_PATH}?since={since}&document=")),
        sender,
    ) else {
        return Ended::Lost(0.0);
    };
    let mut opened = None;
    loop {
        let said = match select(heard.next(), pin!(sleep(quiet_ms))).await {
            Either::Left((Some(said), _)) => said,
            _ => Heard::Lost,
        };
        let value = match said {
            Heard::Open => {
                frame.connected();
                opened = Some(js_sys::Date::now());
                stale.set(false);
                freshen(views, Freshness::opened);
                continue;
            }
            Heard::Ping => continue,
            Heard::Server(message) => {
                *SERVER_NOTICE.write() = message;
                continue;
            }
            Heard::Lost => break,
            Heard::Away => return Ended::Away,
            Heard::Delta(value) => value,
        };
        let Some(version) = crate::request::field(&value, "version")
            .as_f64()
            .map(|v| v as u64)
        else {
            break;
        };
        if board
            .read()
            .as_ref()
            .is_some_and(|shown| version <= shown.version)
        {
            continue;
        }
        frame.heard(&value);
        let Ok(refresh) = task_live::apply(&value, data) else {
            break;
        };
        if let Some(shown) = board.write().as_mut() {
            shown.version = version;
        }
        for change in js_sys::Array::from(&crate::request::field(&value, "changes")) {
            if crate::request::field(&change, "region")
                .as_string()
                .as_deref()
                == Some("client")
                && let Some(build) =
                    crate::request::field(&crate::request::field(&change, "data"), "build")
                        .as_string()
            {
                reload_if_redeployed(&build);
            }
        }
        if refresh {
            task_live::refresh(route, data).await;
        }
    }
    Ended::Lost(opened.map_or(0.0, |at| js_sys::Date::now() - at))
}

/// Load the page again when the server serves another client build than this page's own (the
/// shell's module script), once per build ([`reload_for`]); without the tab's storage to remember
/// it in, never.
pub(crate) fn reload_if_redeployed(served: &str) {
    // DX owns dev build identity. The API's release hash describes a different build entirely.
    if cfg!(feature = "devtools") {
        return;
    }
    if crate::navigation::engaged() {
        DEFERRED_BUILD.with(|pending| *pending.borrow_mut() = Some(served.into()));
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let own = window
        .document()
        .and_then(|document| {
            document
                .query_selector("script[src^='/assets/symphony-shell-']")
                .ok()
                .flatten()
        })
        .and_then(|script| script.get_attribute("src"));
    let Ok(Some(storage)) = window.session_storage() else {
        return;
    };
    let key = crate::project::stored(&crate::project::current(), RELOADED_FOR);
    let tried = storage.get_item(&key).ok().flatten();
    if reload_for(own.as_deref(), served, tried.as_deref()) {
        let _ = storage.set_item(&key, served);
        let _ = window.location().reload();
    }
}

pub(crate) fn flush_reload() {
    if !crate::navigation::engaged()
        && let Some(build) = DEFERRED_BUILD.with(|pending| pending.borrow_mut().take())
    {
        reload_if_redeployed(&build);
    }
}

pub(crate) async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Waits `ms` before a read is tried again, or less once the reader presses Retry (`retry`
/// counts the presses).
#[cfg(feature = "page-tasks")]
pub(crate) async fn pause(ms: i32, retry: Signal<u64>) {
    let asked = *retry.peek();
    let mut left = ms;
    while left > 0 && *retry.peek() == asked {
        let step = left.min(200);
        sleep(step).await;
        left -= step;
    }
}

pub(crate) fn mark(name: &str) {
    if let Some(performance) = web_sys::window().and_then(|window| window.performance()) {
        let _ = performance.mark(name);
    }
}

pub(crate) fn mark_tasks_ready() {
    thread_local! { static MARKED: Cell<bool> = const { Cell::new(false) }; }
    MARKED.with(|marked| {
        if !marked.replace(true) {
            mark("tasks-ready");
        }
    });
}

pub(crate) fn describe(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
