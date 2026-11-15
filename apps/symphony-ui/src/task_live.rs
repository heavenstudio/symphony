//! Browser transport of the task page. Only route changes and reconnects fetch snapshots;
//! ordinary feed events merge one record. Composers keep their own signals throughout. The agents
//! page is the web client's (apps/symphony-web/src/agents-live.ts).
//!
//! A task page keeps its page and its thread's newest page on this device
//! ([`crate::cache::Item::task`]): opened again, it draws them at once ([`opened`]) and, once the
//! board's version is read, asks whether each changed ([`load`]), a `304` confirming the copy and
//! a `200` replacing it on the page and on the device (docs/analysis/client-cache.md).

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use symphony_api::{
    ItemChange, Message, Task, TaskChange, TaskEntry, TaskHistory, TaskListing, TaskPage,
};
use wasm_bindgen::JsValue;

use crate::cache::{self, Freshness, Item, task as kept};
use crate::request::{self, Answer};

#[derive(Clone, Default, PartialEq)]
pub struct Data {
    pub detail: Option<TaskPage>,
    /// The slug the server has no task for, as its task route last answered.
    pub missing: Option<String>,
    pub error: String,
    pub entries: Vec<TaskEntry>,
    pub thread_loaded: bool,
    thread_requested: bool,
    child_requested: bool,
    /// A task came under the shown one since its subtasks were read: they are read again with the
    /// page ([`refresh`]), as the feed sent its changed fields alone.
    child_came: bool,
    pub before: Option<i64>,
    pub approval: Option<(symphony_api::TaskOffer, f64)>,
    page: String,
    child_cursor: Option<String>,
    generation: u64,
    events: Vec<(u64, JsValue)>,
    /// How many board events since the page started may have changed the shown task's gates other
    /// than through its own record: a verdict on its thread, a new tip of the branch its gates
    /// answer for ([`crate::tasks::gates_branch`]) and a card in a document it links
    /// ([`crate::tasks::links_card`]). Its gates read again when it grows.
    pub gates_nudged: u64,
    /// The task and the branch its gates were last answered for ([`gates_answered`]).
    gates_branch: Option<(String, String)>,
    /// What the page shows from this device, and whether the server has answered it.
    pub freshness: Freshness,
    /// The shown task's page as the server last answered it, with the answer's tag: the page's
    /// next read names the tag, and a `304` shows this copy again.
    page_copy: Option<(String, TaskPage)>,
    /// The tag of the thread's newest page this device drew, which its check names.
    thread_tag: String,
    /// The messages drawn from this device, by id, until the server answers the thread.
    drawn_entries: Vec<i64>,
    /// Earlier messages were asked for while the thread showed this device's copy: read once the
    /// thread is answered, from the cursor its answer gives.
    older_waiting: bool,
    /// Counts the checks of what the page drew ([`load`]): an older check's retries stop.
    round: u64,
}

impl Data {
    /// Whether the view `route` names has its content to draw: its task's page. Until it has, a
    /// failed read is said by the view's own trouble block, not by the notices.
    pub fn drawn(&self, route: &str) -> bool {
        route.strip_prefix("/task/").is_some_and(|slug| {
            self.detail
                .as_ref()
                .is_some_and(|page| page.task.slug == slug)
        })
    }

    /// Another task is shown: nothing of the thread, the offer or the subtasks of the one before.
    fn start(&mut self) {
        self.entries.clear();
        self.thread_loaded = false;
        self.approval = None;
        self.thread_requested = false;
        self.child_requested = false;
        self.child_came = false;
        self.child_cursor = None;
        self.before = None;
        self.page_copy = None;
        self.thread_tag.clear();
        self.drawn_entries.clear();
        self.older_waiting = false;
    }

    /// The route the page left showed what this device kept, unanswered: none of it is shown
    /// again before it is drawn or read anew.
    fn leave(&mut self) {
        if self.freshness.waits(kept::PAGE) {
            self.detail = None;
            self.page_copy = None;
        }
        if self.freshness.waits(kept::THREAD) {
            self.entries.clear();
            self.thread_loaded = false;
            self.thread_requested = false;
            self.before = None;
            self.drawn_entries.clear();
            self.older_waiting = false;
        }
        self.freshness.left(&[kept::PAGE, kept::THREAD]);
    }
}

/// The cache item of the task `slug`, as the page's checkout shows it.
fn kept_item(slug: &str) -> Item {
    Item::task(&crate::navigation::Route::current().scope, slug)
}

/// Open the route's page: for a task, one more visit to it, and what this device kept of it drawn
/// at once unless the page shows the task already: its page and, kept with it, its thread's newest
/// page. [`load`] checks each once the board's version is read. What this device drew for the
/// route before goes, unless answered.
pub async fn opened(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route.peek().clone();
    let mut left = data.peek().clone();
    left.leave();
    if left != *data.peek() {
        data.set(left);
    }
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    let item = kept_item(slug);
    cache::visit_later(&item);
    let Some(page) = cache::read::<TaskPage>(&item, kept::PAGE).await else {
        return;
    };
    let thread = cache::read::<TaskHistory>(&item, kept::THREAD).await;
    let shown = |data: Signal<Data>| {
        data.peek()
            .detail
            .as_ref()
            .is_some_and(|shown| shown.task.slug == slug)
    };
    if *route.peek() != selected || shown(data) || page.value.task.slug != slug {
        return;
    }
    let mut d = data.write();
    d.start();
    d.page_copy = Some((page.etag, page.value.clone()));
    d.detail = Some(page.value);
    d.missing = None;
    d.error.clear();
    d.freshness.drawn(kept::PAGE, page.saved);
    if let Some(thread) = thread {
        d.drawn_entries = thread.value.entries.iter().map(|e| e.message.id).collect();
        d.entries = thread.value.entries;
        d.before = thread.value.before;
        d.thread_loaded = true;
        d.thread_requested = true;
        d.thread_tag = thread.etag;
        d.freshness.drawn(kept::THREAD, thread.saved);
    }
}

/// Read `part` with `read`, and while the page still shows this device's copy of it unanswered,
/// again, waiting longer after each failure, until it is answered, the route changes or a newer
/// check starts (`round`). Returns once the first read is done.
async fn answer<F: Future<Output = ()> + 'static>(
    part: &'static str,
    round: u64,
    route: Memo<String>,
    data: Signal<Data>,
    read: impl Fn(Memo<String>, Signal<Data>) -> F + 'static,
) {
    let selected = route.peek().clone();
    let current = move || {
        *route.peek() == selected && data.peek().round == round && data.peek().freshness.waits(part)
    };
    read(route, data).await;
    if !current() {
        return;
    }
    spawn(async move {
        let mut backoff = crate::state::Backoff::default();
        while current() {
            crate::live::sleep(backoff.after(0.0)).await;
            if !current() {
                break;
            }
            read(route, data).await;
        }
    });
}

/// Keep the branch the gates of a task were answered for, `(slug, branch)`: the moved branches
/// the feed names are matched against it.
pub fn gates_answered(mut data: Signal<Data>, answered: (String, String)) {
    if data.peek().gates_branch.as_ref() != Some(&answered) {
        data.write().gates_branch = Some(answered);
    }
}

pub fn route() -> String {
    format!("/{}", crate::navigation::Route::current().path)
}

pub fn url(path: &str) -> String {
    crate::project::scoped(&crate::navigation::Route::current().scope, path)
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    crate::request::get(&url(path)).await
}

pub async fn post(path: &str, fields: &[(&str, &str)]) -> Result<JsValue, String> {
    let fields = fields
        .iter()
        .map(|(key, value)| (*key, JsValue::from_str(value)))
        .collect::<Vec<_>>();
    crate::request::post(&url(path), &fields).await
}

pub async fn load(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let since = data.read().generation;
    if selected.starts_with("/task/") {
        let catch_up = data.read().thread_loaded
            && data
                .read()
                .detail
                .as_ref()
                .is_some_and(|p| selected.strip_prefix("/task/") == Some(p.task.slug.as_str()));
        let same_task = data
            .read()
            .detail
            .as_ref()
            .is_some_and(|p| selected.strip_prefix("/task/") == Some(p.task.slug.as_str()));
        // What a page drawn from this device was asked to read waits for its page's answer
        // ([`refresh`]).
        let waiting = data.peek().freshness.waits(kept::PAGE);
        let requested = data.read().thread_requested && same_task && !waiting;
        let children_requested = data.read().child_requested && same_task && !waiting;
        let round = {
            let mut d = data.write();
            d.round += 1;
            d.round
        };
        // The page and the thread this device drew are checked at once, each asked again until it
        // answers; a thread read before is caught up from its newest message once the page is.
        if same_task && data.peek().freshness.waits(kept::THREAD) {
            futures_util::future::join(
                answer(kept::PAGE, round, route, data, refresh),
                answer(kept::THREAD, round, route, data, check_thread),
            )
            .await;
        } else {
            answer(kept::PAGE, round, route, data, refresh).await;
            if catch_up {
                catch_up_thread(route, data).await;
            } else if requested {
                history(false, route, data).await;
            }
        }
        if children_requested
            && data
                .read()
                .detail
                .as_ref()
                .is_some_and(|p| p.children.is_empty() && p.children_count > 0)
        {
            children(false, route, data).await;
        }
        return;
    }
    if route() == selected {
        data.write().page = selected;
        data.write().error.clear();
        replay(since, route, data);
    }
}

fn property(value: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(value, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

/// Apply the additional task regions without teaching the front page's wire enum about them.
pub fn apply(value: &JsValue, mut data: Signal<Data>) -> Result<bool, String> {
    crate::viewport::hold_place("[id^=message-], [id^=child-], [data-slug]");
    let generation = data.read().generation + 1;
    {
        let mut shown = data.write();
        shown.generation = generation;
        shown.events.push((generation, value.clone()));
        if shown.events.len() > 128 {
            shown.events.remove(0);
        }
    }
    apply_event(value, data)
}

fn replay(since: u64, route: Memo<String>, data: Signal<Data>) {
    let events: Vec<_> = data
        .read()
        .events
        .iter()
        .filter(|(generation, _)| *generation > since)
        .map(|(_, value)| value.clone())
        .collect();
    let mut needed = false;
    let mut failed = data
        .read()
        .events
        .first()
        .is_some_and(|(first, _)| since.saturating_add(1) < *first);
    for event in events {
        match apply_event(&event, data) {
            Ok(refresh) => needed |= refresh,
            Err(_) => failed = true,
        }
    }
    if needed || failed {
        spawn(async move {
            if failed {
                load(route, data).await;
            } else {
                refresh(route, data).await;
            }
        });
    }
}

fn apply_event(value: &JsValue, mut data: Signal<Data>) -> Result<bool, String> {
    let before = data();
    let mut next = before.clone();
    let mut reload_detail = false;
    let mut reload_page = false;
    let selected = self::route();
    for change in js_sys::Array::from(&property(value, "changes")) {
        let region = property(&change, "region").as_string().unwrap_or_default();
        let value = property(&change, "data");
        match region.as_str() {
            "task" => {
                let change: ItemChange<Task> = decode(value)?;
                if let Some(page) = &mut next.detail
                    && (page.children.iter().any(|t| t.slug == change.key)
                        || change
                            .value
                            .as_ref()
                            .is_some_and(|t| t.parent.as_deref() == Some(&page.task.slug)))
                {
                    let existed = page.children.iter().any(|t| t.slug == change.key);
                    let belongs = change
                        .value
                        .as_ref()
                        .is_some_and(|t| t.parent.as_deref() == Some(&page.task.slug));
                    if change.removed || !belongs {
                        page.children.retain(|t| t.slug != change.key);
                    } else {
                        item(&mut page.children, change.clone(), |t| &t.slug);
                    }
                    reload_detail |= !existed || change.removed || !belongs;
                }
                if next
                    .detail
                    .as_ref()
                    .is_some_and(|p| p.task.slug == change.key)
                {
                    if change.removed {
                        next.detail = None;
                        if selected.strip_prefix("/task/") == Some(change.key.as_str()) {
                            next.missing = Some(change.key.clone());
                            next.error = "This task was removed".into();
                        }
                    } else {
                        reload_detail = true;
                    }
                }
                // A task filed under the slug the page could not find: read it now.
                reload_page |= !change.removed
                    && next.missing.as_deref() == Some(change.key.as_str())
                    && selected.strip_prefix("/task/") == Some(change.key.as_str());
            }
            // The feed sends a task it changed as these fields alone: a view that does not hold
            // the task reads it again when the change brings it in.
            "task-fields" => {
                let change: TaskChange = decode(value)?;
                if let Some(page) = next.detail.as_mut() {
                    let done = crate::tasks::page_fields(page, &change);
                    reload_detail |= done.reload;
                    next.child_came |= done.child_came;
                }
            }
            // A holder's health: its roles on the page's task and its subtasks say it.
            "session" => crate::tasks::holder_health(
                next.detail
                    .iter_mut()
                    .flat_map(|page| std::iter::once(&mut page.task).chain(&mut page.children)),
                &property(&value, "key").as_string().unwrap_or_default(),
                property(&property(&value, "value"), "health").as_string(),
            ),
            "message" => {
                let message: Message = decode(value.clone())?;
                if next
                    .detail
                    .as_ref()
                    .is_some_and(|p| p.task.slug == message.slug)
                    && !next
                        .entries
                        .iter()
                        .any(|entry| entry.message.id == message.id)
                {
                    if message.kind == "verdict" {
                        next.gates_nudged += 1;
                    }
                    next.entries.push(TaskEntry {
                        message,
                        stamp: property(&value, "stamp").as_string().unwrap_or_default(),
                        blocks: if property(&value, "blocks").is_undefined() {
                            Vec::new()
                        } else {
                            decode(property(&value, "blocks"))?
                        },
                    });
                }
            }
            // A card that came, changed or went: the owner gate of a task that links its document.
            "pending" => {
                if let (Some(key), Some(page)) = (property(&value, "key").as_string(), &next.detail)
                    && crate::tasks::links_card(&page.task.links, &key)
                {
                    next.gates_nudged += 1;
                }
            }
            "branches" => {
                if let Some(page) = &next.detail {
                    let branch =
                        crate::tasks::gates_branch(&page.task.slug, next.gates_branch.as_ref());
                    if js_sys::Array::from(&value)
                        .iter()
                        .any(|moved| moved.as_string().as_deref() == Some(branch))
                    {
                        next.gates_nudged += 1;
                    }
                }
            }
            "machine-work" => {
                let work: symphony_api::MachineWork = decode(value)?;
                if let Some(page) = &mut next.detail {
                    page.machine_work = work;
                }
            }
            "tasks-status" => {
                reload_page |= property(&value, "available").as_bool() == Some(true);
                next.error = crate::words::BoardStatus {
                    available: property(&value, "available").as_bool(),
                    slow: property(&value, "slow").as_bool() == Some(true),
                }
                .notice()
                .into();
            }
            _ => {}
        }
    }
    if before != next {
        data.set(next);
    }
    Ok(reload_page
        || (reload_detail
            && before
                .detail
                .as_ref()
                .is_some_and(|p| selected.strip_prefix("/task/") == Some(p.task.slug.as_str()))))
}

/// Read the route's page again: for a task, naming the copy the page holds, so that a `304` shows
/// it again and a `200` replaces it, on the page and on this device.
pub async fn refresh(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let since = data.read().generation;
    if let Some(slug) = selected.strip_prefix("/task/") {
        let copy = data
            .peek()
            .page_copy
            .clone()
            .filter(|(_, page)| page.task.slug == slug);
        let read = read_page(slug, copy).await;
        if route() != selected {
            return;
        }
        match read {
            Ok(None) => {
                let mut d = data.write();
                d.missing = Some(slug.to_string());
                d.page = selected.clone();
                d.error.clear();
                // The task is gone, and so is what this device drew and kept of it.
                if d.detail.as_ref().is_some_and(|p| p.task.slug == slug) {
                    d.detail = None;
                    d.start();
                }
                d.freshness.gone(kept::PAGE);
                d.freshness.gone(kept::THREAD);
                drop(d);
                let item = kept_item(slug);
                spawn(async move { cache::forget(&item).await });
                replay(since, route, data);
            }
            Ok(Some((mut page, fresh))) => {
                let mut d = data.write();
                let drawn = d.freshness.waits(kept::PAGE);
                if d.detail
                    .as_ref()
                    .is_none_or(|p| p.task.slug != page.task.slug)
                {
                    d.start();
                } else if let Some(old) = &d.detail {
                    page.children = old.children.clone();
                }
                if let Some((tag, value)) = fresh {
                    cache::keep_later(&kept_item(slug), kept::PAGE, tag.clone(), value);
                    d.page_copy = Some((tag, page.clone()));
                }
                d.detail = Some(page);
                d.missing = None;
                d.page = selected.clone();
                d.error.clear();
                d.freshness.answered(kept::PAGE);
                // What a page drawn from this device was asked to read meanwhile, it reads now.
                let thread = drawn && d.thread_requested && !d.thread_loaded;
                let came = std::mem::take(&mut d.child_came);
                let subtasks = d.child_requested
                    && (came
                        || (drawn
                            && d.detail
                                .as_ref()
                                .is_some_and(|p| p.children.is_empty() && p.children_count > 0)));
                drop(d);
                replay(since, route, data);
                if thread {
                    spawn(history(false, route, data));
                }
                if subtasks {
                    spawn(children(false, route, data));
                }
            }
            Err(err) => {
                let mut d = data.write();
                d.freshness.failed(kept::PAGE);
                // A page drawn from this device says the server is not reachable instead.
                if d.freshness.shown().is_none() {
                    d.error = err;
                }
            }
        }
    } else {
        Box::pin(load(route, data)).await;
    }
}

/// The task page of `slug`, naming the `copy` the page holds (with its tag) when it holds one:
/// `None` when the server has no such task; else the page and, when the server sent one rather
/// than a `304`, the tag and body to keep.
async fn read_page(
    slug: &str,
    copy: Option<(String, TaskPage)>,
) -> Result<Option<(TaskPage, Option<(String, JsValue)>)>, String> {
    let tag = copy.as_ref().map_or("", |(tag, _)| tag.as_str());
    let path = format!("/api/task-page?slug={}", request::encode(slug));
    match request::get_if_found(&url(&path), tag).await? {
        None => Ok(None),
        Some(Answer::Same) => copy
            .map(|(_, page)| Some((page, None)))
            .ok_or_else(|| "The server answered 304 to a plain read".to_string()),
        Some(Answer::Fresh(value, tag)) => serde_wasm_bindgen::from_value(value.clone())
            .map(|page| Some((page, Some((tag, value)))))
            .map_err(|e| e.to_string()),
    }
}

/// The thread's newest page, `path`, read fresh: the page and the tag and body to keep.
async fn read_newest(path: &str) -> Result<(TaskHistory, Option<(String, JsValue)>), String> {
    match request::get_if(&url(path), "").await? {
        Answer::Fresh(value, tag) => serde_wasm_bindgen::from_value(value.clone())
            .map(|page| (page, Some((tag, value))))
            .map_err(|e| e.to_string()),
        Answer::Same => Err("The server answered 304 to a plain read".into()),
    }
}

/// Ask whether the thread's newest page this device drew changed, naming its tag: a `304`
/// confirms it, and a `200` replaces what the device drew ([`cache::task::newest`]), on the page
/// and on the device. Earlier messages asked for meanwhile are read then.
async fn check_thread(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    let tag = data.peek().thread_tag.clone();
    let path = format!("/api/task-history?slug={}", request::encode(slug));
    let read = request::get_if(&url(&path), &tag)
        .await
        .and_then(|answer| match answer {
            Answer::Same => Ok(None),
            Answer::Fresh(value, tag) => {
                serde_wasm_bindgen::from_value::<TaskHistory>(value.clone())
                    .map(|page| Some((page, tag, value)))
                    .map_err(|e| e.to_string())
            }
        });
    if route() != selected || !data.peek().freshness.waits(kept::THREAD) {
        return;
    }
    let mut d = data.write();
    match read {
        Ok(fresh) => {
            let drawn = std::mem::take(&mut d.drawn_entries);
            if let Some((page, tag, value)) = fresh {
                cache::task::newest(&mut d.entries, &drawn, page.entries);
                d.before = page.before;
                cache::keep_later(&kept_item(slug), kept::THREAD, tag.clone(), value);
                d.thread_tag = tag;
            }
            d.freshness.answered(kept::THREAD);
            let older = std::mem::take(&mut d.older_waiting);
            drop(d);
            if older {
                spawn(history(true, route, data));
            }
        }
        Err(err) => {
            d.freshness.failed(kept::THREAD);
            if d.freshness.shown().is_none() {
                d.error = err;
            }
        }
    }
}

pub async fn history(older: bool, route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    if !older && data.read().thread_loaded {
        return;
    }
    // Earlier than this device's copy of the newest page waits for the server's: its cursor may
    // be another.
    if older && data.peek().freshness.waits(kept::THREAD) {
        data.write().older_waiting = true;
        return;
    }
    data.write().thread_requested = true;
    // A page drawn from this device reads nothing before the board's version is read and its own
    // check answered: the thread is read then ([`refresh`]), and the feed replays what follows.
    if !older && data.peek().freshness.waits(kept::PAGE) {
        return;
    }
    let before = if older { data.read().before } else { None };
    if older && before.is_none() {
        return;
    }
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    let cursor = before.map_or(String::new(), |id| format!("&before={id}"));
    let path = format!("/api/task-history?slug={encoded}{cursor}");
    // The newest page is kept on this device; an earlier one never is.
    let read = if older {
        get::<TaskHistory>(&path).await.map(|page| (page, None))
    } else {
        read_newest(&path).await
    };
    match read {
        Ok((page, fresh)) if route() == selected => {
            if let Some((tag, value)) = fresh {
                cache::keep_later(&kept_item(slug), kept::THREAD, tag.clone(), value);
                data.write().thread_tag = tag;
            }
            let mut shown = data.write();
            for entry in page.entries {
                if !shown
                    .entries
                    .iter()
                    .any(|e| e.message.id == entry.message.id)
                {
                    shown.entries.push(entry);
                }
            }
            shown.entries.sort_by_key(|e| e.message.id);
            shown.before = page.before;
            shown.thread_loaded = true;
        }
        Ok(_) => {}
        Err(err) => data.write().error = err,
    }
}

async fn catch_up_thread(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    let mut after = data
        .read()
        .entries
        .iter()
        .map(|e| e.message.id)
        .max()
        .unwrap_or(0);
    loop {
        match get::<TaskHistory>(&format!("/api/task-history?slug={encoded}&after={after}")).await {
            Ok(page) if route() == selected => {
                let next = page.after;
                let mut shown = data.write();
                for entry in page.entries {
                    if !shown
                        .entries
                        .iter()
                        .any(|e| e.message.id == entry.message.id)
                    {
                        shown.entries.push(entry);
                    }
                }
                shown.entries.sort_by_key(|e| e.message.id);
                drop(shown);
                if let Some(cursor) = next.filter(|id| *id > after) {
                    after = cursor;
                } else {
                    break;
                }
            }
            Ok(_) => break,
            Err(err) => {
                data.write().error = err;
                break;
            }
        }
    }
}

pub async fn children(more: bool, route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    data.write().child_requested = true;
    // As the thread: read once the page drawn from this device is answered ([`refresh`]).
    if data.peek().freshness.waits(kept::PAGE) {
        return;
    }
    let cursor = if more {
        data.read().child_cursor.clone()
    } else {
        None
    };
    let cursor = cursor_query(cursor.as_deref());
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    match get::<TaskListing>(&format!("/api/task-board?parent={encoded}&older=1{cursor}")).await {
        Ok(list) if route() == selected => {
            data.write().child_cursor = list.next;
            if let Some(page) = data.write().detail.as_mut() {
                if !more {
                    page.children.clear();
                }
                for task in list.tasks {
                    item(
                        &mut page.children,
                        ItemChange {
                            key: task.slug.clone(),
                            value: Some(task),
                            removed: false,
                        },
                        |t| &t.slug,
                    );
                }
                page.children_count = list.counts.iter().map(|(_, n)| n).sum();
            }
        }
        Ok(_) => {}
        Err(err) => data.write().error = err,
    }
}

pub async fn offer(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let Some(slug) = selected.strip_prefix("/task/") else {
        return;
    };
    let Some(revision) = data
        .read()
        .detail
        .as_ref()
        .filter(|p| p.task.slug == slug)
        .map(|p| p.task.updated)
    else {
        return;
    };
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    match get::<symphony_api::TaskOffer>(&format!("/api/task-offer?slug={encoded}")).await {
        Ok(offer) if route() == selected => {
            if data
                .read()
                .detail
                .as_ref()
                .is_some_and(|p| p.task.updated == revision)
            {
                data.write().approval = offer.payload.is_some().then_some((offer, revision));
            }
        }
        Ok(_) => {}
        Err(err) => {
            if route() == selected {
                data.write().error = err;
            }
        }
    }
}

pub fn cursor_query(cursor: Option<&str>) -> String {
    cursor.map_or(String::new(), |cursor| {
        format!(
            "&cursor={}",
            js_sys::encode_uri_component(cursor)
                .as_string()
                .unwrap_or_default()
        )
    })
}

fn decode<T: DeserializeOwned>(value: JsValue) -> Result<T, String> {
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

fn item<T>(items: &mut Vec<T>, change: ItemChange<T>, key: impl Fn(&T) -> &str) {
    if change.removed {
        items.retain(|old| key(old) != change.key);
    } else if let Some(value) = change.value {
        if let Some(index) = items.iter().position(|old| key(old) == change.key) {
            items[index] = value;
        } else {
            items.push(value);
        }
    }
}
