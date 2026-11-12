//! The client cache: what a page drew, kept on this device so a return visit draws at once and
//! then asks the server only whether it changed (docs/analysis/client-cache.md). The store is the
//! shell's (`assets/cache.js`, `symphonyCache`), so no page kind's wasm carries IndexedDB
//! bindings: this module names what is kept, under the wire crate's
//! [`symphony_api::CACHE_FORMAT`], and reads it back as the page's types. Without the store (the
//! development runtime, a browser with no IndexedDB) every read misses and nothing is kept.

use dioxus::prelude::{GlobalSignal, Signal};

/// Whether the page may show what is out of date: drawn from this device's cache and not yet
/// checked ([`Freshness::shown`]), or reconnecting; the page's notices set it
/// (`live::PageNotices`). The owner's actions wait meanwhile, disabled and saying
/// [`crate::words::CHECKING`]: one given on a card or task that changed since would act on what
/// the owner never saw. Drafts stay editable.
pub static STALE: GlobalSignal<bool> = Signal::global(|| false);

/// Which caps an item counts against: each project keeps the 30 tasks opened last and the 20
/// documents visited most (`CAPS` in cache.js); the board's reads are outside both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Board,
    Task,
    Document,
}

impl Kind {
    /// The name cache.js caps it by.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::Task => "task",
            Kind::Document => "document",
        }
    }
}

/// What one visit opens, and eviction drops whole: its parts are the reads its page drew.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    /// Unique in its project: the kind's name, then what it names.
    pub id: String,
    /// A project's entrypoint page, kept while the project's cache has room.
    pub pinned: bool,
}

impl Item {
    /// The board, the queue and home of the checkout `scope` names (`""`: the main checkout).
    pub fn board(scope: &str) -> Item {
        Item::new(Kind::Board, scoped(scope, ""), false)
    }

    /// The task `slug`'s page, as the checkout `scope` names shows it.
    pub fn task(scope: &str, slug: &str) -> Item {
        Item::new(Kind::Task, scoped(scope, slug), false)
    }

    /// A document or folder page by its path as the API names it (`docs/X.md`, or
    /// `@tree/docs/X.md` for a branch's); `entrypoint` for an entrypoint's own page.
    pub fn document(path: &str, entrypoint: bool) -> Item {
        Item::new(Kind::Document, path.to_string(), entrypoint)
    }

    fn new(kind: Kind, name: String, pinned: bool) -> Item {
        Item {
            id: format!("{}:{name}", kind.name()),
            kind,
            pinned,
        }
    }
}

fn scoped(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("@{scope}/{name}")
    }
}

/// The parts of [`Item::board`]: what home, the queue and the task board keep of a visit.
pub mod board {
    /// `/api/board`, as home and the queue drew it.
    pub const BOARD: &str = "board";
    /// `/api/queue`.
    pub const QUEUE: &str = "queue";
    /// The task board's counts (`/api/task-board?counts=1`).
    pub const COUNTS: &str = "counts";
    /// The machine's work (`/api/machine-work`), which places the task board's tasks.
    pub const WORK: &str = "work";

    /// A task board column's first page.
    pub fn column(status: &str) -> String {
        format!("column:{status}")
    }

    /// Whether a read of the task board's listing is the default view's, the one kept: no
    /// search, finished tasks of the last 24 hours, stages by the machine's activity.
    pub fn kept_view(query: &str, days: u8, activity: u8) -> bool {
        query.is_empty() && days == 1 && activity == 1
    }

    /// The columns a task board with these counts may have kept: each status its listing names
    /// (its counts' when it names none), the machine's two stages and the owner's sign-off, once
    /// each, in that order.
    pub fn columns(statuses: &[String], counts: &[(String, usize)]) -> Vec<String> {
        let named: Vec<&str> = if statuses.is_empty() {
            counts.iter().map(|(status, _)| status.as_str()).collect()
        } else {
            statuses.iter().map(String::as_str).collect()
        };
        let mut columns: Vec<String> = Vec::new();
        for status in named.into_iter().chain(["build", "test", "signoff"]) {
            if !columns.iter().any(|column| column == status) {
                columns.push(status.to_string());
            }
        }
        columns
    }
}

/// A kept read: its value, the tag the server answered it with, and when it was kept
/// (milliseconds since the epoch, for the page's "saved 14:32").
#[derive(Clone, Debug, PartialEq)]
pub struct Kept<T> {
    pub value: T,
    pub etag: String,
    pub saved: f64,
}

impl<V> Kept<V> {
    /// The kept read with its value read by `parse` (as the page's type), its tag and time kept;
    /// `None` when this client cannot read it, which the browser's `read` takes as a miss and
    /// forgets.
    pub fn readable<T, E>(self, parse: impl FnOnce(V) -> Result<T, E>) -> Option<Kept<T>> {
        Some(Kept {
            value: parse(self.value).ok()?,
            etag: self.etag,
            saved: self.saved,
        })
    }
}

/// Whether a page shows what this device kept, and so whether its actions wait (the stale rule,
/// docs/analysis/client-cache.md, "A stale draw"). A page drawn from the cache says so until its
/// feed is open and the server has answered every kept read it drew; the page asks again each
/// read that fails until it answers. It says the server is not reachable while a read it made
/// fails and its feed is not open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Freshness {
    /// When the oldest copy drawn was kept, ms since the epoch, while one is shown.
    saved: Option<f64>,
    /// The parts drawn from the cache the server has not answered yet.
    unanswered: Vec<String>,
    /// The reads whose last try failed, by part (the board's read among them, drawn or not).
    failing: Vec<String>,
    open: bool,
}

impl Freshness {
    /// `part`, kept at `saved` (ms since the epoch), was drawn from the cache.
    pub fn drawn(&mut self, part: &str, saved: f64) {
        self.saved = Some(self.saved.map_or(saved, |oldest| oldest.min(saved)));
        if !self.unanswered.iter().any(|drawn| drawn == part) {
            self.unanswered.push(part.to_string());
        }
    }

    /// The server answered `part`, and the page shows its answer.
    pub fn answered(&mut self, part: &str) {
        self.unanswered.retain(|drawn| drawn != part);
        self.failing.retain(|failed| failed != part);
        self.settle();
    }

    /// The page no longer shows `part` (a search replaced it), answered or not, and asks for it
    /// no more.
    pub fn gone(&mut self, part: &str) {
        self.unanswered.retain(|drawn| drawn != part);
        self.failing.retain(|failed| failed != part);
        self.settle();
    }

    /// The read of `part` went unanswered: the server is not reachable, as far as the page can
    /// tell, until that read is answered or the feed opens; a copy drawn meanwhile says so too
    /// (the page's first read may fail before this device's copy is read).
    pub fn failed(&mut self, part: &str) {
        if !self.failing.iter().any(|failed| failed == part) {
            self.failing.push(part.to_string());
        }
    }

    /// The feed is open: the server answers, and the reads that failed are being asked again.
    pub fn opened(&mut self) {
        self.open = true;
        self.settle();
    }

    /// The feed was lost.
    pub fn lost(&mut self) {
        self.open = false;
    }

    fn settle(&mut self) {
        if self.open && self.unanswered.is_empty() {
            self.saved = None;
        }
    }

    /// While the page shows a copy from this device: when the oldest was kept, and whether the
    /// server is not reachable.
    pub fn shown(&self) -> Option<(f64, bool)> {
        self.saved
            .map(|saved| (saved, !self.open && !self.failing.is_empty()))
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::{forget, keep, keep_later, read, read_raw, visit, visit_later};

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::{Item, Kept};
    use crate::request::field;
    use serde::de::DeserializeOwned;
    use symphony_api::CACHE_FORMAT;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = symphonyCache, js_name = read, catch)]
        fn store_read(
            format: u32,
            project: &str,
            id: &str,
            part: &str,
        ) -> Result<js_sys::Promise, JsValue>;
        #[allow(clippy::too_many_arguments)]
        #[wasm_bindgen(js_namespace = symphonyCache, js_name = keep, catch)]
        fn store_keep(
            format: u32,
            project: &str,
            id: &str,
            kind: &str,
            part: &str,
            etag: &str,
            value: &JsValue,
            size: Option<f64>,
            ahead: bool,
        ) -> Result<js_sys::Promise, JsValue>;
        #[wasm_bindgen(js_namespace = symphonyCache, js_name = visit, catch)]
        fn store_visit(
            format: u32,
            project: &str,
            id: &str,
            kind: &str,
            pinned: bool,
        ) -> Result<js_sys::Promise, JsValue>;
        #[wasm_bindgen(js_namespace = symphonyCache, js_name = forget, catch)]
        fn store_forget(format: u32, project: &str, id: &str) -> Result<js_sys::Promise, JsValue>;
    }

    /// The page's project, as its other kept values name it ([`crate::project::stored`]).
    fn project() -> String {
        crate::project::current()
            .base
            .trim_start_matches('/')
            .to_string()
    }

    /// What the store's promise gave, or `None`: no store, a failure, or nothing kept.
    async fn answer(promise: Result<js_sys::Promise, JsValue>) -> Option<JsValue> {
        let value = JsFuture::from(promise.ok()?).await.ok()?;
        (!value.is_null() && !value.is_undefined()).then_some(value)
    }

    /// `part` of `item` as kept, its value as the browser parsed it.
    pub async fn read_raw(item: &Item, part: &str) -> Option<Kept<JsValue>> {
        let kept = answer(store_read(CACHE_FORMAT, &project(), &item.id, part)).await?;
        Some(Kept {
            value: field(&kept, "value"),
            etag: field(&kept, "etag").as_string()?,
            saved: field(&kept, "saved").as_f64()?,
        })
    }

    /// `part` of `item` as kept, read as `T`. A value this client cannot read is a miss, and its
    /// item is forgotten, so the page fetches it whole.
    pub async fn read<T: DeserializeOwned>(item: &Item, part: &str) -> Option<Kept<T>> {
        let kept = read_raw(item, part)
            .await?
            .readable(serde_wasm_bindgen::from_value);
        if kept.is_none() {
            forget(item).await;
        }
        kept
    }

    /// Keep `value`, answered with `etag`, as `part` of `item`; `ahead` for a page read before
    /// it was opened. Whether the item is still kept once the caps hold.
    pub async fn keep(item: &Item, part: &str, etag: &str, value: &JsValue, ahead: bool) -> bool {
        let kept = store_keep(
            CACHE_FORMAT,
            &project(),
            &item.id,
            item.kind.name(),
            part,
            etag,
            value,
            None,
            ahead,
        );
        answer(kept).await.and_then(|kept| kept.as_bool()) == Some(true)
    }

    /// The page of `item` was opened: one more visit.
    pub async fn visit(item: &Item) {
        let visited = store_visit(
            CACHE_FORMAT,
            &project(),
            &item.id,
            item.kind.name(),
            item.pinned,
        );
        answer(visited).await;
    }

    /// Drop `item` and every part of it.
    pub async fn forget(item: &Item) {
        answer(store_forget(CACHE_FORMAT, &project(), &item.id)).await;
    }

    /// [`keep`] a visited page's read, the page not waiting for the store.
    pub fn keep_later(item: &Item, part: &str, etag: String, value: JsValue) {
        let (item, part) = (item.clone(), part.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            keep(&item, &part, &etag, &value, false).await;
        });
    }

    /// [`visit`], the page not waiting for the store.
    pub fn visit_later(item: &Item) {
        let item = item.clone();
        wasm_bindgen_futures::spawn_local(async move { visit(&item).await });
    }
}

#[cfg(test)]
mod tests;
