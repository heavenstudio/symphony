//! The board, live: fetched from the server's JSON API, then kept current by its feed, one event
//! at a time ([`crate::state::apply`]); [`crate::view`] draws it. The browser half of the app.
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
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
use std::pin::pin;
use std::rc::Rc;

use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedSender;
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
use symphony_api::{BOARD_PATH, FEED_PATH};
#[cfg(feature = "page-review")]
use symphony_api::{Board, Delta};
use symphony_api::{FEED_EVENT, PING_EVENT, SERVER_EVENT};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, EventSource, MessageEvent};

pub(crate) const FEED_NOTICE: &str = "Reconnecting: the board may be out of date";
const REMOVED_DOCUMENT_NOTICE: &str =
    "This document was removed. Your draft is kept on this device.";
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
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
#[cfg(feature = "page-review")]
use {crate::platform, crate::state::apply, crate::view::BoardView, crate::words::Moment};
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
use {crate::state::Backoff, crate::task_live};

/// The style sheet. dx links it from the page's head when it builds (`with_static_head`), so the
/// browser loads it beside the wasm rather than after it; the app only keeps it in the binary,
/// which is where dx finds it.
const STYLE: Asset = asset!(
    "/assets/board.css",
    AssetOptions::css().with_static_head(true)
);

/// The performance mark set once the board is first drawn (the phone test times it).
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
const READY_MARK: &str = "board-ready";
/// Where the tab keeps the build it last loaded the page again for ([`reload_for`]).
const RELOADED_FOR: &str = "symphony-ui-reloaded-for";

thread_local! { static DEFERRED_BUILD: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub(crate) static SERVER_NOTICE: GlobalSignal<String> = Signal::global(String::new);

#[component]
pub fn App() -> Element {
    std::hint::black_box(&STYLE);
    let route = crate::navigation::use_navigation();
    #[cfg(any(
        feature = "page-tasks",
        feature = "page-review",
        feature = "page-queue"
    ))]
    let task_route = use_memo(move || format!("/{}", route().path));
    let current = route();
    let page = match current.path.as_str() {
        #[cfg(feature = "page-board")]
        "tasks" => rsx! { crate::board_live::TasksPage { key: "{current.scope}" } },
        #[cfg(feature = "page-docs")]
        _ if current.document => {
            rsx! { main {
                if !current.folder {
                    crate::docs_live::DocumentPage { key: "{current.scope}/{current.path}", path: current.path.clone(), scope: current.scope.clone() }
                } else {
                    crate::docs_live::FolderPage { key: "{current.scope}/{current.path}", path: current.path.clone(), scope: current.scope.clone() }
                }
            } }
        }
        #[cfg(any(
            feature = "page-tasks",
            feature = "page-review",
            feature = "page-queue"
        ))]
        _ => {
            rsx! { BoardPage { key: "{current.scope}", route: task_route, scope: current.scope.clone() } }
        }
        #[cfg(not(any(
            feature = "page-tasks",
            feature = "page-review",
            feature = "page-queue"
        )))]
        _ => rsx! { p { "Loading page…" } },
    };
    rsx! {
        crate::frame::Frame { route: current.clone(),
        SuspenseBoundary { fallback: |_| rsx! { main { p { "Loading page…" } PageNotices { reconnecting: false } } },
        {page}
        }
        }
        MediaLayer {}
    }
}

#[component]
fn MediaLayer() -> Element {
    #[cfg(any(feature = "page-tasks", feature = "page-docs", feature = "page-review"))]
    {
        // The activation signal belongs to this component so it lives as long as its runtime.
        let needed = crate::media_live::use_activation();
        rsx! { if needed() { crate::media_live::MediaRuntime {} } }
    }
    #[cfg(not(any(feature = "page-tasks", feature = "page-docs", feature = "page-review")))]
    rsx! {}
}

/// One out-of-flow stack for feed, fetch and server messages. Its matching end
/// reserve cannot shrink during this mount, so clearing an error at page end
/// cannot clamp the reader's scroll position.
#[component]
pub(crate) fn PageNotices(
    reconnecting: bool,
    #[props(default = FEED_NOTICE.to_owned())] reconnect: String,
    #[props(default)] error: String,
    #[props(default)] document_removed: bool,
) -> Element {
    let server = SERVER_NOTICE.read().clone();
    let controller = use_hook(|| Rc::new(RefCell::new(None::<NoticeGeometry>)));
    use_effect(move || {
        if controller.borrow().is_none() {
            *controller.borrow_mut() = NoticeGeometry::new();
        }
    });
    let visible = reconnecting || !error.is_empty() || !server.is_empty() || document_removed;
    rsx! {
        div { class: "stale page-notices notice-stack", hidden: !visible,
            if reconnecting { p { class: "pill wait", role: "status", "{reconnect}" } }
            if !error.is_empty() { p { class: "pill wait", role: "alert", "{error}" } }
            if document_removed { p { class: "pill wait document-removed", role: "alert", "{REMOVED_DOCUMENT_NOTICE}" } }
            if !server.is_empty() { p { class: "pill wait server-notice", role: "status", "{server}" } }
        }
        div { class: "feed-notice-space page-notice-reserve notice-stack", "aria-hidden": "true",
            p { class: "pill", "{reconnect}" }
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

#[cfg(feature = "page-review")]
type PageBoard = Board;
#[cfg(all(
    any(feature = "page-tasks", feature = "page-queue"),
    not(feature = "page-review")
))]
#[derive(Clone, serde::Deserialize)]
struct PageBoard {
    version: u64,
    ping_ms: u64,
    build: String,
}

#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
#[component]
fn BoardPage(route: Memo<String>, scope: String) -> Element {
    let frame = use_context::<crate::frame::FrameRefresh>();
    let board = use_signal(|| None::<PageBoard>);
    let stale = use_signal(|| false);
    let trouble = use_signal(|| None::<String>);
    #[cfg(feature = "page-queue")]
    let queue_refreshed = use_signal(|| 0_u64);
    #[cfg(not(feature = "page-queue"))]
    let _ = scope;
    #[cfg(feature = "page-review")]
    let mut now = use_signal(local_now);
    #[cfg(feature = "page-tasks")]
    let data = use_signal(Data::default);
    use_hook(|| {
        spawn(keep_live(
            board,
            stale,
            trouble,
            PageViews {
                #[cfg(feature = "page-queue")]
                queue_refreshed,
                #[cfg(feature = "page-tasks")]
                route,
                #[cfg(feature = "page-tasks")]
                data,
                frame,
            },
        ));
        spawn(async move {
            loop {
                sleep(60_000).await;
                #[cfg(feature = "page-review")]
                now.set(local_now());
                flush_reload();
            }
        });
    });
    #[cfg(feature = "page-tasks")]
    {
        let first_route = use_hook(|| Rc::new(Cell::new(true)));
        use_effect(move || {
            let _ = route();
            if !first_route.replace(false) {
                spawn(task_live::load(route, data));
            }
        });
    }
    #[cfg(not(feature = "page-tasks"))]
    let _ = route;
    let marked = use_hook(|| Rc::new(Cell::new(false)));
    use_effect(move || {
        if board.read().is_some() && !marked.replace(true) {
            mark(READY_MARK);
        }
    });
    let content = if board.read().is_some() {
        #[cfg(feature = "page-review")]
        let front = match (board(), now()) {
            (Some(board), Some(now)) => {
                rsx! { BoardView { board, now, project: crate::project::current() } }
            }
            _ => rsx! { p { "Loading the board…" } },
        };
        #[cfg(not(feature = "page-review"))]
        let front = rsx! { p { "Page unavailable" } };
        #[cfg(feature = "page-queue")]
        let front = if route() == "/queue" {
            rsx! { crate::queue::QueuePage { scope, refreshed: queue_refreshed } }
        } else {
            front
        };
        #[cfg(feature = "page-tasks")]
        {
            if route().starts_with("/task/") || route() == "/agents" {
                rsx! { crate::tasks::LiveTaskPage { route, data } }
            } else {
                front
            }
        }
        #[cfg(not(feature = "page-tasks"))]
        {
            front
        }
    } else {
        rsx! { p { class: "lead", {trouble().map_or_else(|| "Loading the board…".into(), |why| format!("The board could not be loaded: {why}. Trying again."))} } }
    };
    rsx! { main { "data-ready": "board", "data-feed": if stale() { "stale" } else { "live" },
        {content}
        PageNotices { reconnecting: stale() }
    } }
}

#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
async fn fetch_board() -> Result<PageBoard, String> {
    crate::request::get(&task_live::url(BOARD_PATH)).await
}

#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
#[derive(Clone, Copy)]
struct PageViews {
    #[cfg(feature = "page-queue")]
    queue_refreshed: Signal<u64>,
    #[cfg(feature = "page-tasks")]
    route: Memo<String>,
    #[cfg(feature = "page-tasks")]
    data: Signal<Data>,
    frame: crate::frame::FrameRefresh,
}

#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
async fn keep_live(
    mut board: Signal<Option<PageBoard>>,
    mut stale: Signal<bool>,
    mut trouble: Signal<Option<String>>,
    views: PageViews,
) {
    let mut backoff = Backoff::default();
    loop {
        let mut held = 0.0;
        match fetch_board().await {
            Ok(fetched) => {
                let since = fetched.version;
                let quiet =
                    i32::try_from(fetched.ping_ms.saturating_mul(5) / 2).unwrap_or(i32::MAX);
                reload_if_redeployed(&fetched.build);
                board.set(Some(fetched));
                #[cfg(feature = "page-tasks")]
                task_live::load(views.route, views.data).await;
                #[cfg(feature = "page-queue")]
                {
                    let mut revision = views.queue_refreshed;
                    revision += 1;
                }
                held = follow(board, stale, since, quiet, views).await;
            }
            Err(why) => trouble.set(Some(why)),
        }
        stale.set(true);
        sleep(backoff.after(held)).await;
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
}

/// Follow the feed from `since` until its stream is lost: when it errs, when it says nothing for
/// `quiet_ms`, or when it sends an event this client cannot read (the board fetched next has its
/// change). Each event is applied where it lands, the owner told when more cards wait on them,
/// the page loaded again for another client build. How long the stream held, in ms.
#[cfg(any(
    feature = "page-tasks",
    feature = "page-review",
    feature = "page-queue"
))]
async fn follow(
    mut board: Signal<Option<PageBoard>>,
    mut stale: Signal<bool>,
    since: u64,
    quiet_ms: i32,
    views: PageViews,
) -> f64 {
    let PageViews {
        #[cfg(feature = "page-queue")]
        mut queue_refreshed,
        #[cfg(feature = "page-tasks")]
        route,
        #[cfg(feature = "page-tasks")]
        data,
        frame,
    } = views;
    let (sender, mut heard) = unbounded::<Heard>();
    let Some(_feed) = Feed::open(
        &task_live::url(&format!("{FEED_PATH}?since={since}&document=")),
        sender,
    ) else {
        return 0.0;
    };
    #[cfg(feature = "page-review")]
    let platform = platform::current();
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
                continue;
            }
            Heard::Ping => continue,
            Heard::Server(message) => {
                *SERVER_NOTICE.write() = message;
                continue;
            }
            Heard::Lost => break,
            Heard::Delta(value) => value,
        };
        #[cfg(feature = "page-review")]
        let Ok(delta) = serde_wasm_bindgen::from_value::<Delta>(value.clone()) else {
            break;
        };
        #[cfg(feature = "page-review")]
        let version = delta.version;
        #[cfg(not(feature = "page-review"))]
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
        #[cfg(feature = "page-queue")]
        if crate::queue::changed(&value) {
            queue_refreshed += 1;
        }
        #[cfg(feature = "page-tasks")]
        let Ok(refresh) = task_live::apply(&value, data) else {
            break;
        };
        #[cfg(feature = "page-review")]
        {
            let (outcome, build) = match board.write().as_mut() {
                Some(shown) => (apply(shown, &delta), shown.build.clone()),
                None => break,
            };
            if let Some(notice) = outcome.notice {
                platform.notify(&notice.title, &notice.body);
            }
            reload_if_redeployed(&build);
        }
        #[cfg(not(feature = "page-review"))]
        {
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
        }
        #[cfg(feature = "page-tasks")]
        if refresh {
            task_live::refresh(route, data).await;
        }
    }
    opened.map_or(0.0, |at| js_sys::Date::now() - at)
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
    let tried = storage.get_item(RELOADED_FOR).ok().flatten();
    if reload_for(own.as_deref(), served, tried.as_deref()) {
        let _ = storage.set_item(RELOADED_FOR, served);
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

/// The feed's stream and its listener, which sends on what the stream says, each event's data
/// parsed: closed when dropped, so a board no longer shown, or a stream lost, stops listening.
pub(crate) struct Feed {
    source: EventSource,
    listener: Closure<dyn FnMut(Event)>,
}

impl Feed {
    pub(crate) fn open(url: &str, sender: UnboundedSender<Heard>) -> Option<Feed> {
        // Resolve the origin before handing the URL to browser/desktop domain guards.
        let base = web_sys::window()?.location().href().ok()?;
        let url = web_sys::Url::new_with_base(url, &base).ok()?.href();
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

/// The wall clock where the owner is.
#[cfg(feature = "page-review")]
fn local_now() -> Option<Moment> {
    let date = js_sys::Date::new_0();
    Moment::from_parts(
        i64::from(date.get_full_year()),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes(),
    )
}

pub(crate) async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
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
