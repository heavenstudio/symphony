//! The client cache: what a page drew, kept on this device so a return visit draws at once and
//! then asks the server only whether it changed (docs/analysis/client-cache.md). The store is the
//! shell's (`assets/cache.js`, `symphonyCache`), so no page kind's wasm carries IndexedDB
//! bindings: this module names what is kept, under the wire crate's
//! [`symphony_api::CACHE_FORMAT`], and reads it back as the page's types. Without the store (the
//! development runtime, a browser with no IndexedDB) every read misses and nothing is kept.

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

#[cfg(target_arch = "wasm32")]
pub use browser::{forget, keep, read, read_raw, visit};

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
}

#[cfg(test)]
mod tests;
