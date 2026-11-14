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
    /// `/api/queue`, which the web client's queue keeps (apps/symphony-web/src/cache.ts): named
    /// here so that no other part takes its name.
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

/// The parts of [`Item::task`]: what a task page keeps of a visit. Never its thread's earlier
/// pages, its landing offer, its subtasks or the rest of a long brief.
pub mod task {
    use symphony_api::TaskEntry;

    /// `/api/task-page`.
    pub const PAGE: &str = "page";
    /// The thread's newest page (`/api/task-history` without `before`).
    pub const THREAD: &str = "thread";

    /// The thread once the server answered its newest page with `page`: the entries this device
    /// drew (`drawn`, by id) give way to the page's, and every other one shown stays (one the feed
    /// brought meanwhile), each message once, in id order.
    pub fn newest(entries: &mut Vec<TaskEntry>, drawn: &[i64], page: Vec<TaskEntry>) {
        entries.retain(|entry| !drawn.contains(&entry.message.id));
        for entry in page {
            match entries
                .iter_mut()
                .find(|shown| shown.message.id == entry.message.id)
            {
                Some(shown) => *shown = entry,
                None => entries.push(entry),
            }
        }
        entries.sort_by_key(|entry| entry.message.id);
    }
}

/// The parts of [`Item::document`]: what a document page, or an entrypoint's folder page, keeps of
/// a visit. Never its versions, diffs, asks or history, nor a folder page but an entrypoint's own.
pub mod document {
    use std::collections::{BTreeMap, BTreeSet};

    use serde::Deserialize;
    #[cfg(test)]
    use symphony_api::Document;
    use symphony_api::{DocumentChange, DocumentOutline, Section};

    use crate::docs::{self, Body};

    /// `/api/document?view=outline`; or, for a document read ahead of its first visit, its first
    /// part (`view=first`), which carries the bodies of its leading sections ([`read_ahead`]).
    pub const OUTLINE: &str = "outline";
    /// The bodies of the sections the page drew, in one map by section key, each with its card and
    /// thread and the [`validator`] it was drawn under ([`KeptBody`]): a section's body is read
    /// again only when the outline names another validator for it.
    pub const SECTIONS: &str = "sections";
    /// An entrypoint's own folder page (`/api/folder?view=outline`).
    pub const FOLDER: &str = "folder";
    /// The names a kept body goes by ([`KeptBody`], [`Body`]): the page writes it from the values
    /// the server sent, as they came, not from its types, so no page kind carries the code that
    /// writes a block.
    pub const VALIDATOR: &str = "validator";
    pub const BODY: &str = "body";
    pub const SECTION: &str = "section";
    pub const CARD: &str = "card";
    pub const THREAD: &str = "thread";

    /// A section's body as this device keeps it, under the validator it was taken under.
    #[derive(Clone, Debug, PartialEq, Deserialize)]
    #[cfg_attr(test, derive(serde::Serialize))]
    pub struct KeptBody {
        pub validator: String,
        pub body: Body,
    }

    /// Whether a kept outline is a document's first part, read ahead of its first visit: one that
    /// carries bodies, which an outline never does.
    pub fn read_ahead(outline: &DocumentOutline) -> bool {
        !outline.document.threads.is_empty()
            || outline
                .document
                .sections
                .iter()
                .any(|section| !section.blocks.is_empty())
    }

    /// A document's first part read ahead ([`read_ahead`]), as a visit would have kept it: its
    /// outline, the bodies taken out, and each body under its validator.
    pub fn split(mut first: DocumentOutline) -> (DocumentOutline, BTreeMap<String, KeptBody>) {
        let keys = first
            .document
            .sections
            .iter()
            .map(|section| section.key.clone())
            .collect();
        let kept = docs::bodies(&first.document, &keys)
            .into_iter()
            .filter_map(|body| {
                let validator = validator(&body.section, &first.thread_hashes)?;
                Some((body.section.key.clone(), KeptBody { validator, body }))
            })
            .collect();
        for section in &mut first.document.sections {
            section.blocks.clear();
        }
        first.document.threads.clear();
        (first, kept)
    }

    /// Which of the section bodies a page holds are current, and so kept on this device and not
    /// read again: each body is taken under the [`validator`] the server's outline names for its
    /// section, and stays current while the outline names the same one. Before the server has
    /// answered, the bodies drawn from this device are current by the outline kept with them, and
    /// unconfirmed until the server's outline names the same validator or the body is read again.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Bodies {
        /// The validators of the outline the server last answered, by key, less the sections the
        /// feed changed since.
        validators: BTreeMap<String, String>,
        /// That outline's thread hashes, which a body read since is taken under.
        threads: BTreeMap<String, String>,
        /// The validator each body the page holds was taken under, by key.
        taken: BTreeMap<String, String>,
        /// The bodies drawn from this device that the server has not confirmed yet.
        unconfirmed: BTreeSet<String>,
        /// A body was taken or let go since the device was last given what is taken
        /// ([`Bodies::unkept`]). A body the feed changed is not: the device's copy of it is
        /// recognised as out of date by the next outline.
        dirty: bool,
    }

    impl Bodies {
        /// The bodies this device `kept` drawn with `outline`, the outline kept with them: of
        /// them, those current by it, each taken under its validator and unconfirmed.
        pub fn drawn(
            &mut self,
            outline: &DocumentOutline,
            kept: BTreeMap<String, KeptBody>,
        ) -> Vec<Body> {
            let validators = validators(outline);
            let mut drawn = Vec::new();
            for (key, kept) in kept {
                if kept.body.section.key == key && validators.get(&key) == Some(&kept.validator) {
                    self.taken.insert(key.clone(), kept.validator);
                    self.unconfirmed.insert(key);
                    drawn.push(kept.body);
                }
            }
            drawn
        }

        /// The bodies [`Bodies::drawn`] came with a document's first part read ahead of the
        /// visit, kept in its outline ([`read_ahead`]), which the outline the visit reads replaces
        /// on the device: the device is to keep those still current as the visit's own.
        pub fn drawn_ahead(&mut self) {
            self.dirty = true;
        }

        /// The server answered the outline: bodies are taken under its validators from now on.
        /// Returns the keys of the bodies held that are still current by it, which the page keeps
        /// and reads no more. A body drawn from this device that is not current stays unconfirmed
        /// while the page still wants it (`wanted`), until it is read again.
        pub fn answered(
            &mut self,
            outline: &DocumentOutline,
            wanted: &BTreeSet<String>,
        ) -> BTreeSet<String> {
            self.validators = validators(outline);
            self.threads = outline.thread_hashes.clone();
            let validators = &self.validators;
            let held = self.taken.len();
            self.taken
                .retain(|key, valid| validators.get(key) == Some(valid));
            self.dirty |= self.taken.len() != held;
            let current: BTreeSet<String> = self.taken.keys().cloned().collect();
            self.unconfirmed
                .retain(|key| !current.contains(key) && wanted.contains(key));
            current
        }

        /// The page read the bodies of the sections `asked` and holds the answer's `sections`: each
        /// whose section the server's outline names as it is now is taken under its validator,
        /// and no other asked one is. Whether what is taken changed.
        pub fn read(&mut self, asked: &BTreeSet<String>, sections: &[Section]) -> bool {
            let before = self.taken.clone();
            self.taken.retain(|key, _| !asked.contains(key));
            for section in sections {
                let valid = validator(section, &self.threads)
                    .filter(|valid| self.validators.get(&section.key) == Some(valid));
                match valid {
                    Some(valid) => self.taken.insert(section.key.clone(), valid),
                    None => self.taken.remove(&section.key),
                };
            }
            self.unconfirmed.retain(|key| !asked.contains(key));
            let changed = self.taken != before;
            self.dirty |= changed;
            changed
        }

        /// The feed changed `delta`'s sections (their text, card or thread): no validator the
        /// outline named tells them current any more, until the server's next outline. Whether
        /// what is taken changed.
        pub fn changed(&mut self, delta: &DocumentChange) -> bool {
            let cards = delta
                .threads
                .keys()
                .chain(delta.cards.iter().map(|card| &card.key))
                .map(|slug| format!("card:{slug}"));
            let keys: Vec<String> = delta
                .sections
                .iter()
                .map(|section| section.key.clone())
                .chain(delta.deleted.iter().cloned())
                .chain(cards)
                .collect();
            let mut changed = false;
            for key in keys {
                self.validators.remove(&key);
                changed |= self.taken.remove(&key).is_some();
            }
            changed
        }

        /// Whether every body drawn from this device is confirmed or read again.
        pub fn confirmed(&self) -> bool {
            self.unconfirmed.is_empty()
        }

        /// Which bodies this device is to keep, each under its validator, when one was taken or
        /// let go since it was last given them; `None` when it holds them already.
        pub fn unkept(&mut self) -> Option<BTreeMap<String, String>> {
            std::mem::take(&mut self.dirty).then(|| self.taken.clone())
        }

        /// What this device keeps of `document`, as the page writes it from what the server sent:
        /// each body it holds that is taken, under its validator.
        #[cfg(test)]
        pub fn kept(&self, document: &Document) -> BTreeMap<String, KeptBody> {
            let keys = self.taken.keys().cloned().collect();
            docs::bodies(document, &keys)
                .into_iter()
                .filter_map(|body| {
                    let validator = self.taken.get(&body.section.key)?.clone();
                    Some((body.section.key.clone(), KeptBody { validator, body }))
                })
                .collect()
        }
    }

    /// What tells a section's kept body current: the outline's `hash` of it, and for a card the
    /// hash of its thread (`DocumentOutline::thread_hashes`), which its own leaves out. `None` for a
    /// section never kept: one showing a difference from main, whose `before` the outline leaves
    /// out.
    pub fn validator(section: &Section, threads: &BTreeMap<String, String>) -> Option<String> {
        if section.comparison.is_some() {
            return None;
        }
        Some(match section.key.strip_prefix("card:") {
            Some(slug) => format!(
                "{}/{}",
                section.hash,
                threads.get(slug).map_or("", String::as_str)
            ),
            None => section.hash.clone(),
        })
    }

    /// The validator of each section of `outline` that may be kept, by key.
    pub fn validators(outline: &DocumentOutline) -> BTreeMap<String, String> {
        outline
            .document
            .sections
            .iter()
            .filter_map(|section| {
                validator(section, &outline.thread_hashes).map(|valid| (section.key.clone(), valid))
            })
            .collect()
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

    /// The page opened another of its routes (a task page, another task): of `parts`, which it
    /// drew for the route it left, none is shown or asked for any more. With nothing else drawn
    /// left unanswered, nothing it shows came from the device.
    pub fn left(&mut self, parts: &[&str]) {
        self.unanswered
            .retain(|drawn| !parts.contains(&drawn.as_str()));
        self.failing
            .retain(|failed| !parts.contains(&failed.as_str()));
        if self.unanswered.is_empty() {
            self.saved = None;
        }
    }

    /// Whether `part` was drawn from the cache and the server has not answered it yet.
    pub fn waits(&self, part: &str) -> bool {
        self.unanswered.iter().any(|drawn| drawn == part)
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
pub use browser::{forget, keep, keep_later, read, read_raw, read_sent, visit, visit_later};

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
        read_sent(item, part).await.map(|(kept, _)| kept)
    }

    /// [`read`], with the value as the browser parsed it beside it: what a page keeps again as it
    /// came, rather than writing it from its type.
    pub async fn read_sent<T: DeserializeOwned>(
        item: &Item,
        part: &str,
    ) -> Option<(Kept<T>, JsValue)> {
        let raw = read_raw(item, part).await?;
        let value = raw.value.clone();
        let kept = raw.readable(serde_wasm_bindgen::from_value);
        if kept.is_none() {
            forget(item).await;
        }
        Some((kept?, value))
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
