//! The task board needs summaries and counts, not task briefs, threads, agents or documents.
//! Keeping its state and feed decoding here lets those page kinds stay in their own wasm chunks.

use std::{
    cell::{Cell, RefCell},
    pin::pin,
    rc::Rc,
};

use dioxus::prelude::*;
use futures_channel::mpsc::{UnboundedSender, unbounded};
use futures_util::{
    StreamExt,
    future::{Either, join, select},
};
use serde::Deserialize;
use symphony_api::{ItemChange, MachineWork, Task, TaskChange, TaskListing};
use wasm_bindgen::JsValue;

use crate::{
    cache::{self, Freshness, Item, board},
    frame::FrameRefresh,
    live::{Feed, Heard, PageNotices, back, sleep},
    request::{self, Answer},
    state::{Ended, FewMap},
    task_live::url,
};

#[derive(Clone, Default, PartialEq)]
struct BoardData {
    tasks: Vec<Task>,
    work: crate::machine_work::SharedWork,
    counts: Vec<(String, usize)>,
    lifecycle_counts: Vec<(String, usize)>,
    connected: bool,
    statuses: Vec<String>,
    signoff: Vec<String>,
    query: String,
    days: u8,
    ready: bool,
    error: String,
    cursors: FewMap<String, String>,
    pending_more: Vec<String>,
    requests: FewMap<String, u64>,
    count_request: u64,
    generation: u64,
    journal: Rc<RefCell<Journal>>,
    /// The copy of each of the default view's reads the page holds, by its part of `Item::board`
    /// (`counts`, `column:ready`): the next read of it names the copy's tag, and a `304` is that
    /// copy.
    copies: Copies,
    /// The columns drawn from this device's cache the server has not answered yet.
    drawn: Vec<String>,
    /// Each column checked once the board's version was read, by the generation it was checked
    /// at: its own read at that generation would ask again.
    checked: FewMap<String, u64>,
    /// The board's version is not read yet: no column is read before it, as the feed replays only
    /// what changed after it.
    waiting: bool,
    /// The tasks drawn from this device no fresh answer has listed yet, by slug: once every column
    /// drawn from the device has answered, those left are gone from the server's board.
    from_device: Vec<String>,
    /// Which check of the page's reads is current ([`check`]): a check's reads stop asking again
    /// once another has started.
    round: u64,
    /// How many times the counts were answered, and the machine's work (by its read or by the
    /// feed's `machine-work` event): a read that failed is asked again until one is.
    counted: u64,
    worked: u64,
}

/// A read's copy: the tag the server answered it with, and the listing.
type Copy = Rc<(String, TaskListing)>;

/// The copies a page holds, shared by each of its states: one state's are another's only when
/// they are the same, so comparing states never compares listings.
#[derive(Clone, Default)]
struct Copies(Rc<RefCell<FewMap<String, Copy>>>);

impl PartialEq for Copies {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Copies {
    fn get(&self, part: &str) -> Option<Copy> {
        self.0.borrow().get(part).cloned()
    }

    fn put(&self, part: &str, tag: String, list: TaskListing) {
        self.0
            .borrow_mut()
            .insert(part.to_string(), Rc::new((tag, list)));
    }
}

/// What the task board keeps of a visit: the default view's counts, the machine's work and each
/// column's first page, as parts of the board's item ([`board`]).
fn kept_item() -> Item {
    Item::board(&crate::navigation::Route::current().scope)
}

#[derive(Default, PartialEq)]
struct Journal {
    version: u64,
    events: Vec<(u64, JsValue)>,
    readers: FewMap<u64, usize>,
}

/// Keep exactly the events outstanding snapshots need, including when a request is cancelled.
struct ReadCursor {
    journal: Rc<RefCell<Journal>>,
    since: u64,
}

impl ReadCursor {
    fn new(journal: Rc<RefCell<Journal>>) -> Self {
        let since = journal.borrow().version;
        *journal.borrow_mut().readers.or_default(since) += 1;
        Self { journal, since }
    }
}

impl Drop for ReadCursor {
    fn drop(&mut self) {
        let mut journal = self.journal.borrow_mut();
        if let Some(readers) = journal.readers.get_mut(&self.since) {
            *readers -= 1;
            if *readers == 0 {
                journal.readers.remove(&self.since);
            }
        }
        if let Some(since) = journal.readers.first_key().copied() {
            journal.events.retain(|(version, _)| *version > since);
        } else {
            journal.events.clear();
        }
    }
}

/// Only the feed handshake fields; the front page's cards are decoded by its own page.
#[derive(Deserialize)]
struct Handshake {
    version: u64,
    ping_ms: u64,
    build: String,
}

#[component]
pub fn TasksPage() -> Element {
    let frame = use_context::<FrameRefresh>();
    let data = use_signal(|| BoardData {
        days: 1,
        connected: true,
        waiting: true,
        ..BoardData::default()
    });
    let refreshed = use_signal(|| 0_u64);
    let stale = use_signal(|| false);
    let freshness = use_signal(Freshness::default);
    use_hook(move || {
        spawn(keep_live(data, refreshed, stale, freshness, frame));
    });
    let device = freshness.read().shown();
    rsx! { main { "data-ready": "board", "data-feed": if stale() || device.is_some() { "stale" } else { "live" },
        "data-cache": device.map(|_| "device"),
        if data.read().ready {
            crate::tasks::TaskBoard {
                tasks: data.read().tasks.clone(), counts: data.read().counts.clone(),
                work: data.read().work.clone(), connected: !stale(),
                statuses: data.read().statuses.clone(), signoff: data.read().signoff.clone(),
                refreshed,
                onfilter: move |(query, days)| { spawn(filter(query, days, data, freshness)); },
                onload: move |(status, more, query, days)| { spawn(column(status, more, query, days, data, freshness)); },
            }
        } else { p { "Loading tasks…" } }
        PageNotices { reconnecting: stale(), error: data.read().error.clone(), device }
    } }
}

/// Draw what this device kept of the default view before the server answers: the counts, the
/// machine's work that placed them and each column's first page. A column reads nothing before
/// the board's version is read ([`BoardData::waiting`]), and its copy is checked then.
async fn draw_kept(mut data: Signal<BoardData>, mut freshness: Signal<Freshness>) {
    let item = kept_item();
    let Some(counts) = cache::read::<TaskListing>(&item, board::COUNTS).await else {
        return;
    };
    let work = cache::read::<MachineWork>(&item, board::WORK).await;
    let mut columns = Vec::new();
    for status in board::columns(&counts.value.statuses, &counts.value.counts) {
        if let Some(column) = cache::read::<TaskListing>(&item, &board::column(&status)).await {
            columns.push((status, column));
        }
    }
    let mut d = data.write();
    let mut shown = freshness.write();
    if let Some(work) = work {
        d.work = work.value.into();
        shown.drawn(board::WORK, work.saved);
    }
    d.copies
        .put(board::COUNTS, counts.etag, counts.value.clone());
    show_counts(&mut d, counts.value);
    shown.drawn(board::COUNTS, counts.saved);
    for (status, column) in columns {
        let part = board::column(&status);
        d.from_device
            .extend(column.value.tasks.iter().map(|task| task.slug.clone()));
        d.copies.put(&part, column.etag, column.value.clone());
        show_column(&mut d, &status, false, column.value);
        shown.drawn(&part, column.saved);
        d.drawn.push(status);
    }
    if !d.drawn.is_empty() {
        crate::live::mark_tasks_ready();
    }
}

/// A listing's complete totals: its stages' counts, the statuses and the owner's sign-off column.
fn show_counts(d: &mut BoardData, list: TaskListing) {
    d.counts = list.counts;
    d.lifecycle_counts = list.lifecycle_counts;
    d.statuses = list.statuses;
    d.signoff = list.signoff;
    d.ready = true;
}

/// A page of `status`'s column, which replaces the column's tasks when it is the first (`more`
/// false). Whether the column has another.
fn show_column(d: &mut BoardData, status: &str, more: bool, list: TaskListing) -> bool {
    let has_next = list.next.is_some();
    if let Some(cursor) = list.next {
        d.cursors.insert(status.to_string(), cursor);
    } else {
        d.cursors.remove(status);
    }
    if !more {
        let work = d.work.clone();
        let connected = d.connected;
        d.tasks
            .retain(|t| work.task_stage(t, connected).as_str() != status);
    }
    for task in list.tasks {
        upsert(
            &mut d.tasks,
            ItemChange {
                key: task.slug.clone(),
                value: Some(task),
                removed: false,
            },
        );
    }
    d.counts = list.counts;
    d.lifecycle_counts = list.lifecycle_counts;
    d.signoff = list.signoff;
    has_next
}

/// Read a listing, `path` under the page's project. With the `copy` the page holds, the read
/// names its tag and a `304` is that copy; otherwise the server's body, with the tag and value to
/// keep.
async fn read_listing(
    path: &str,
    copy: Option<Copy>,
) -> Result<(TaskListing, Option<(String, JsValue)>), String> {
    let tag = copy.as_ref().map_or("", |copy| copy.0.as_str());
    match request::get_if(&url(path), tag).await? {
        Answer::Same => copy
            .map(|copy| (copy.1.clone(), None))
            .ok_or_else(|| "The server answered 304 to a plain read".to_string()),
        Answer::Fresh(value, tag) => serde_wasm_bindgen::from_value(value.clone())
            .map(|list| (list, Some((tag, value))))
            .map_err(|e| e.to_string()),
    }
}

/// The read of `part` failed: a page drawn from this device says whether the server is
/// reachable, any other why.
fn unanswered(
    mut data: Signal<BoardData>,
    mut freshness: Signal<Freshness>,
    part: &str,
    err: String,
) {
    freshness.write().failed(part);
    if freshness.peek().shown().is_none() {
        data.write().error = err;
    }
}

/// Keep a fresh answer of the default view as `part`'s copy, on the page and on this device.
fn keep(d: &BoardData, part: &str, list: &TaskListing, fresh: Option<(String, JsValue)>) {
    if let Some((tag, value)) = fresh {
        d.copies.put(part, tag.clone(), list.clone());
        cache::keep_later(&kept_item(), part, tag, value);
    }
}

/// The board's listings leave the machine's work out (`work=0`): every recorder's heartbeat
/// moves it, so a listing with it would never be answered `304`. The page reads it here once the
/// board's version is read, and follows the feed's `machine-work` events after; an event applied
/// while the read was out is newer than its answer.
async fn machine_work(mut data: Signal<BoardData>, mut freshness: Signal<Freshness>) {
    let worked = data.peek().worked;
    let read = request::get_json(&url("/api/machine-work"))
        .await
        .and_then(|value| {
            serde_wasm_bindgen::from_value::<MachineWork>(value.clone())
                .map(|work| (work, value))
                .map_err(|e| e.to_string())
        });
    match read {
        Ok((work, value)) => {
            if data.peek().worked == worked {
                let mut d = data.write();
                d.work = work.into();
                d.worked += 1;
                cache::keep_later(&kept_item(), board::WORK, String::new(), value);
            }
            freshness.write().answered(board::WORK);
        }
        Err(err) => {
            unanswered(data, freshness, board::WORK, err);
        }
    }
}

async fn counts(mut data: Signal<BoardData>, mut freshness: Signal<Freshness>) {
    let (query, days, generation, request, since, activity, copy) = {
        let mut d = data.write();
        d.count_request += 1;
        let activity = u8::from(d.connected);
        let copy = if board::kept_view(&d.query, d.days, activity) {
            d.copies.get(board::COUNTS)
        } else {
            None
        };
        (
            d.query.clone(),
            d.days,
            d.generation,
            d.count_request,
            ReadCursor::new(d.journal.clone()),
            activity,
            copy,
        )
    };
    let encoded = request::encode(&query);
    let result = read_listing(
        &format!("/api/task-board?counts=1&q={encoded}&days={days}&activity={activity}&work=0"),
        copy,
    )
    .await;
    if data.read().generation != generation || data.read().count_request != request {
        return;
    }
    match result {
        Ok((list, fresh)) => {
            let mut d = data.write();
            if board::kept_view(&query, days, activity) {
                keep(&d, board::COUNTS, &list, fresh);
            }
            show_counts(&mut d, list);
            d.error.clear();
            d.counted += 1;
            replay(&since, &mut d);
            freshness.write().answered(board::COUNTS);
        }
        Err(err) => {
            unanswered(data, freshness, board::COUNTS, err);
        }
    }
}

async fn filter(
    query: String,
    days: u8,
    mut data: Signal<BoardData>,
    mut freshness: Signal<Freshness>,
) {
    // A -> B -> A is three searches, even though the first and last have the same text.
    let generation = {
        let mut d = data.write();
        d.query = query;
        d.days = days;
        d.generation += 1;
        d.tasks.clear();
        d.from_device.clear();
        d.cursors.clear();
        d.pending_more.clear();
        // The columns this device drew went with their tasks.
        let mut shown = freshness.write();
        for status in std::mem::take(&mut d.drawn) {
            shown.gone(&board::column(&status));
        }
        d.generation
    };
    sleep(250).await;
    if data.read().generation == generation {
        counts(data, freshness).await;
    }
}

async fn column(
    status: String,
    more: bool,
    query: String,
    days: u8,
    mut data: Signal<BoardData>,
    freshness: Signal<Freshness>,
) {
    let (generation, version) = {
        let mut d = data.write();
        // A live refresh resets the first page's cursor, and a column drawn from this device has
        // only the device's until it is checked. Keep a Load more click behind that read instead
        // of cancelling it and accidentally asking for the first page again.
        if more && (!d.cursors.contains_key(&status) || d.drawn.contains(&status)) {
            if !d.pending_more.contains(&status) {
                d.pending_more.push(status);
            }
            return;
        }
        // Before the board's version, nothing is read; once checked at this generation, the
        // column is read already.
        if d.waiting || !more && d.checked.get(&status) == Some(&d.generation) {
            return;
        }
        let generation = d.generation;
        let version = d.requests.or_default(status.clone());
        *version += 1;
        (generation, *version)
    };
    sleep(250).await;
    read_column(
        status,
        more,
        query,
        days,
        (generation, version),
        data,
        freshness,
    )
    .await;
}

/// Check a column drawn from this device once the board's version is read: at once, not after
/// the 250 ms a column waits, and marked so that its own read at this generation does not ask
/// again. Unanswered, the column reads itself when it is next asked to, and [`check`] asks again.
async fn check_column(status: String, mut data: Signal<BoardData>, freshness: Signal<Freshness>) {
    let (ticket, query, days) = {
        let mut d = data.write();
        let generation = d.generation;
        d.checked.insert(status.clone(), generation);
        let version = d.requests.or_default(status.clone());
        *version += 1;
        ((generation, *version), d.query.clone(), d.days)
    };
    if !read_column(status.clone(), false, query, days, ticket, data, freshness).await {
        data.write().checked.remove(&status);
    }
}

/// Read a page of `status`'s column, unless a later read of it or another generation supersedes
/// it (`generation`, `version`: the read's). Whether the page shows the server's answer.
async fn read_column(
    status: String,
    more: bool,
    query: String,
    days: u8,
    (generation, version): (u64, u64),
    mut data: Signal<BoardData>,
    mut freshness: Signal<Freshness>,
) -> bool {
    let current = |data: Signal<BoardData>| {
        let d = data.read();
        d.generation == generation && d.requests.get(&status) == Some(&version)
    };
    if !current(data) {
        return false;
    }
    let part = board::column(&status);
    let (since, cursor, activity, copy) = {
        let d = data.read();
        let activity = u8::from(d.connected);
        (
            ReadCursor::new(d.journal.clone()),
            if more {
                d.cursors.get(&status).cloned()
            } else {
                None
            },
            activity,
            if !more && board::kept_view(&query, days, activity) {
                d.copies.get(&part)
            } else {
                None
            },
        )
    };
    let encoded = request::encode(&query);
    let cursor = crate::task_live::cursor_query(cursor.as_deref());
    let result = read_listing(
        &format!(
            "/api/task-board?q={encoded}&status={status}&days={days}{cursor}&activity={activity}&work=0"
        ),
        copy,
    )
    .await;
    if !current(data) {
        return false;
    }
    let (list, fresh) = match result {
        Ok(read) => read,
        Err(err) => {
            unanswered(data, freshness, &part, err);
            return false;
        }
    };
    let mut d = data.write();
    if !more && board::kept_view(&query, days, activity) {
        keep(&d, &part, &list, fresh);
    }
    d.from_device
        .retain(|slug| !list.tasks.iter().any(|task| &task.slug == slug));
    let has_next = show_column(&mut d, &status, more, list);
    replay(&since, &mut d);
    crate::live::mark_tasks_ready();
    if !more {
        d.drawn.retain(|drawn| drawn != &status);
        // Every column drawn from the device has answered: a task it drew that none listed is
        // gone from the board, whichever stage the machine's work places it in now.
        if d.drawn.is_empty() {
            let gone = std::mem::take(&mut d.from_device);
            d.tasks.retain(|task| !gone.contains(&task.slug));
        }
        freshness.write().answered(&part);
    }
    let queued = !more && has_next && d.pending_more.contains(&status);
    d.pending_more.retain(|waiting| waiting != &status);
    drop(d);
    if queued {
        spawn(column(status, true, query, days, data, freshness));
    }
    true
}

/// Once the board's version is read: the counts, the machine's work and each column still drawn
/// from this device, all at once. Returns when each has answered or failed once; one that failed
/// is asked again until it answers, by its read or (the machine's work) by the feed, or the next
/// check starts.
async fn check(mut data: Signal<BoardData>, freshness: Signal<Freshness>) {
    let round = {
        let mut d = data.write();
        d.round += 1;
        d.round
    };
    let current = move || data.peek().round == round;
    let (done, mut all) = unbounded::<()>();
    // Answered since this check started, by any read of it or by the feed.
    let (drawn, worked, counted) = {
        let d = data.peek();
        (d.drawn.clone(), d.worked, d.counted)
    };
    for status in drawn {
        until_answered(done.clone(), current, async move || {
            let answered = |data: Signal<BoardData>| !data.peek().drawn.contains(&status);
            answered(data) || {
                check_column(status.clone(), data, freshness).await;
                answered(data)
            }
        });
    }
    until_answered(done.clone(), current, async move || {
        data.peek().worked != worked || {
            machine_work(data, freshness).await;
            data.peek().worked != worked
        }
    });
    until_answered(done.clone(), current, async move || {
        data.peek().counted != counted || {
            counts(data, freshness).await;
            data.peek().counted != counted
        }
    });
    drop(done);
    // Every sender is dropped once its first try is done: the stream then ends.
    let _ = all.next().await;
}

/// Try `read` until it says it was answered, waiting longer after each failure, while `current`;
/// `done` goes once the first try is over.
fn until_answered(
    done: UnboundedSender<()>,
    current: impl Fn() -> bool + 'static,
    mut read: impl AsyncFnMut() -> bool + 'static,
) {
    spawn(async move {
        let mut answered = read().await;
        drop(done);
        let mut backoff = crate::state::Backoff::default();
        while !answered {
            sleep(backoff.after(0.0)).await;
            if !current() {
                break;
            }
            answered = read().await;
        }
    });
}

fn field(value: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(value, &name.into()).unwrap_or(JsValue::UNDEFINED)
}

fn upsert(tasks: &mut Vec<Task>, change: ItemChange<Task>) {
    if change.removed {
        tasks.retain(|t| t.slug != change.key);
    } else if let Some(task) = change.value {
        if let Some(old) = tasks.iter_mut().find(|t| t.slug == change.key) {
            *old = task;
        } else {
            tasks.push(task);
        }
    }
}

fn apply(value: &JsValue, mut data: Signal<BoardData>) -> Result<bool, String> {
    let before = data();
    {
        let mut journal = before.journal.borrow_mut();
        journal.version += 1;
        let version = journal.version;
        if !journal.readers.is_empty() {
            journal.events.push((version, value.clone()));
        }
    }
    let mut next = before.clone();
    let refresh = merge(value, &mut next)?;
    if before != next {
        data.set(next);
    }
    Ok(refresh)
}

fn replay(cursor: &ReadCursor, data: &mut BoardData) {
    let events: Vec<_> = cursor
        .journal
        .borrow()
        .events
        .iter()
        .filter(|(version, _)| *version > cursor.since)
        .map(|(_, value)| value.clone())
        .collect();
    for value in events {
        let _ = merge(&value, data);
    }
}

fn merge(value: &JsValue, d: &mut BoardData) -> Result<bool, String> {
    let mut refresh = false;
    for change in js_sys::Array::from(&field(value, "changes")) {
        let region = field(&change, "region").as_string().unwrap_or_default();
        let value = field(&change, "data");
        let searching = !d.query.is_empty();
        match region.as_str() {
            "machine-work" => {
                let work: symphony_api::MachineWork =
                    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?;
                d.work = work.into();
                d.worked += 1;
                refresh = true;
            }
            "task" | "task-summary" => {
                let mut change: ItemChange<Task> =
                    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?;
                d.from_device.retain(|slug| slug != &change.key);
                if let Some(task) = &change.value
                    && !searching
                    && matches!(task.status.as_str(), "done" | "cancelled")
                    && task.updated < js_sys::Date::now() / 1000.0 - f64::from(d.days) * 86_400.0
                {
                    change.removed = true;
                }
                if searching {
                    refresh = true;
                } else {
                    upsert(&mut d.tasks, change);
                }
            }
            "task-fields" => {
                let change: TaskChange =
                    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?;
                if let Some(task) = d.tasks.iter_mut().find(|t| t.slug == change.key) {
                    crate::tasks::merge(task, &change.fields);
                }
                refresh |=
                    searching || change.fields.status.is_some() || change.fields.parent.is_some();
            }
            "message" => refresh |= searching,
            "task-signoff" if searching => refresh = true,
            "task-signoff" => {
                d.signoff = serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?
            }
            // Legacy feed totals describe lifecycle. Ask for the complete stage projection
            // under this client's search/window, never infer totals from loaded pages.
            "task-counts" | "task-recent-counts" => refresh = true,
            "tasks-status" => {
                refresh |= field(&value, "available").as_bool() == Some(true);
                d.error = crate::words::BoardStatus {
                    available: field(&value, "available").as_bool(),
                    slow: field(&value, "slow").as_bool() == Some(true),
                }
                .notice()
                .into();
            }
            _ => {}
        }
    }
    if refresh {
        // Every change affecting a query's complete totals invalidates older pages/counts.
        // The pending refresh retries if another such event arrives while its read is held.
        d.generation += 1;
        d.cursors.clear();
        d.pending_more.clear();
    }
    Ok(refresh)
}

async fn keep_live(
    mut data: Signal<BoardData>,
    mut refreshed: Signal<u64>,
    mut stale: Signal<bool>,
    mut freshness: Signal<Freshness>,
    frame: FrameRefresh,
) {
    cache::visit_later(&kept_item());
    let mut backoff = crate::state::Backoff::default();
    let pending = Rc::new(Cell::new(false));
    // The first read of the board's version goes with the draw of what this device kept; nothing
    // else is read before both.
    let mut first = Some(
        join(
            draw_kept(data, freshness),
            request::get::<Handshake>(&url("/api/board")),
        )
        .await
        .1,
    );
    loop {
        back().await;
        let mut ended = Ended::Lost(0.0);
        let read = match first.take() {
            Some(read) => read,
            None => request::get::<Handshake>(&url("/api/board")).await,
        };
        match read {
            Ok(board) => {
                crate::live::reload_if_redeployed(&board.build);
                {
                    let mut d = data.write();
                    d.generation += 1;
                    d.waiting = false;
                }
                freshness.write().answered(board::BOARD);
                check(data, freshness).await;
                refreshed += 1;
                let (send, mut heard) = unbounded();
                if let Some(_feed) = Feed::open(
                    &url(&format!("/api/feed?since={}&document=", board.version)),
                    send,
                ) {
                    let quiet =
                        i32::try_from(board.ping_ms.saturating_mul(5) / 2).unwrap_or(i32::MAX);
                    let mut opened = None;
                    let mut version = board.version;
                    loop {
                        match select(heard.next(), pin!(sleep(quiet))).await {
                            Either::Left((Some(Heard::Open), _)) => {
                                stale.set(false);
                                freshness.write().opened();
                                if !data.read().connected {
                                    {
                                        let mut d = data.write();
                                        d.connected = true;
                                        d.generation += 1;
                                        d.cursors.clear();
                                        d.pending_more.clear();
                                    }
                                    counts(data, freshness).await;
                                    refreshed += 1;
                                }
                                frame.connected();
                                opened = Some(js_sys::Date::now());
                            }
                            Either::Left((Some(Heard::Ping), _)) => crate::live::flush_reload(),
                            Either::Left((Some(Heard::Server(message)), _)) => {
                                *crate::live::SERVER_NOTICE.write() = message;
                            }
                            Either::Left((Some(Heard::Delta(value)), _)) => {
                                let Some(next) =
                                    field(&value, "version").as_f64().map(|v| v as u64)
                                else {
                                    break;
                                };
                                if next <= version {
                                    continue;
                                }
                                version = next;
                                frame.heard(&value);
                                let changes = js_sys::Array::from(&field(&value, "changes"));
                                for change in changes {
                                    if field(&change, "region").as_string().as_deref()
                                        == Some("client")
                                        && let Some(build) =
                                            field(&field(&change, "data"), "build").as_string()
                                    {
                                        crate::live::reload_if_redeployed(&build);
                                    }
                                }
                                let worked = data.peek().worked;
                                let Ok(refresh) = apply(&value, data) else {
                                    break;
                                };
                                // The event carries the machine's whole work: it answers for
                                // the work's read.
                                if data.peek().worked != worked {
                                    freshness.write().answered(board::WORK);
                                }
                                if refresh && !pending.replace(true) {
                                    let pending = pending.clone();
                                    spawn(async move {
                                        loop {
                                            sleep(750).await;
                                            let generation = data.read().generation;
                                            counts(data, freshness).await;
                                            if generation == data.read().generation {
                                                refreshed += 1;
                                                pending.set(false);
                                                break;
                                            }
                                        }
                                    });
                                }
                            }
                            Either::Left((Some(Heard::Away), _)) => {
                                ended = Ended::Away;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if ended != Ended::Away {
                        ended = Ended::Lost(opened.map_or(0.0, |at| js_sys::Date::now() - at));
                    }
                }
                freshness.write().lost();
            }
            Err(err) => {
                unanswered(data, freshness, board::BOARD, err);
            }
        }
        stale.set(true);
        {
            let mut d = data.write();
            d.connected = false;
            d.generation += 1;
            d.cursors.clear();
            d.pending_more.clear();
            if !d.lifecycle_counts.is_empty() {
                d.counts = d.lifecycle_counts.clone();
            }
        }
        refreshed += 1;
        sleep(backoff.wait(ended)).await;
    }
}
