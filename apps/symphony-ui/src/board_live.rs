//! The task board needs summaries and counts, not task briefs, threads, agents or documents.
//! Keeping its state and feed decoding here lets those page kinds stay in their own wasm chunks.

use std::{
    cell::{Cell, RefCell},
    pin::pin,
    rc::Rc,
};

use dioxus::prelude::*;
use futures_channel::mpsc::unbounded;
use futures_util::{
    StreamExt,
    future::{Either, select},
};
use serde::Deserialize;
use symphony_api::{ItemChange, Task, TaskChange, TaskListing};
use wasm_bindgen::JsValue;

use crate::{
    frame::FrameRefresh,
    live::{Feed, Heard, PageNotices, sleep},
    request,
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
    cursors: std::collections::BTreeMap<String, String>,
    pending_more: Vec<String>,
    requests: std::collections::BTreeMap<String, u64>,
    count_request: u64,
    generation: u64,
    journal: Rc<RefCell<Journal>>,
}

#[derive(Default, PartialEq)]
struct Journal {
    version: u64,
    events: Vec<(u64, JsValue)>,
    readers: std::collections::BTreeMap<u64, usize>,
}

/// Keep exactly the events outstanding snapshots need, including when a request is cancelled.
struct ReadCursor {
    journal: Rc<RefCell<Journal>>,
    since: u64,
}

impl ReadCursor {
    fn new(journal: Rc<RefCell<Journal>>) -> Self {
        let since = journal.borrow().version;
        *journal.borrow_mut().readers.entry(since).or_default() += 1;
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
        if let Some(since) = journal.readers.keys().next().copied() {
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
        ..BoardData::default()
    });
    let refreshed = use_signal(|| 0_u64);
    let stale = use_signal(|| false);
    use_hook(move || {
        spawn(keep_live(data, refreshed, stale, frame));
    });
    rsx! { main { "data-ready": "board", "data-feed": if stale() { "stale" } else { "live" },
        if data.read().ready {
            crate::tasks::TaskBoard {
                tasks: data.read().tasks.clone(), counts: data.read().counts.clone(),
                work: data.read().work.clone(), connected: !stale(),
                statuses: data.read().statuses.clone(), signoff: data.read().signoff.clone(),
                refreshed,
                onfilter: move |(query, days)| { spawn(filter(query, days, data)); },
                onload: move |(status, more, query, days)| { spawn(column(status, more, query, days, data)); },
            }
        } else { p { "Loading tasks…" } }
        PageNotices { reconnecting: stale(), error: data.read().error.clone() }
    } }
}

async fn counts(mut data: Signal<BoardData>) {
    let (query, days, generation, request, since, activity) = {
        let mut d = data.write();
        d.count_request += 1;
        (
            d.query.clone(),
            d.days,
            d.generation,
            d.count_request,
            ReadCursor::new(d.journal.clone()),
            u8::from(d.connected),
        )
    };
    let encoded = request::encode(&query);
    let result = request::get::<TaskListing>(&url(&format!(
        "/api/task-board?counts=1&q={encoded}&days={days}&activity={activity}"
    )))
    .await;
    if data.read().generation != generation || data.read().count_request != request {
        return;
    }
    match result {
        Ok(list) => {
            let mut d = data.write();
            d.counts = list.counts;
            d.lifecycle_counts = list.lifecycle_counts;
            d.work = list.machine_work.into();
            d.statuses = list.statuses;
            d.signoff = list.signoff;
            d.ready = true;
            d.error.clear();
            replay(&since, &mut d);
        }
        Err(err) => data.write().error = err,
    }
}

async fn filter(query: String, days: u8, mut data: Signal<BoardData>) {
    // A -> B -> A is three searches, even though the first and last have the same text.
    let generation = {
        let mut d = data.write();
        d.query = query;
        d.days = days;
        d.generation += 1;
        d.tasks.clear();
        d.cursors.clear();
        d.pending_more.clear();
        d.generation
    };
    sleep(250).await;
    if data.read().generation == generation {
        counts(data).await;
    }
}

async fn column(status: String, more: bool, query: String, days: u8, mut data: Signal<BoardData>) {
    let (generation, version) = {
        let mut d = data.write();
        // A live refresh resets the first page's cursor. Keep a Load more click behind that
        // read instead of cancelling it and accidentally asking for the first page again.
        if more && !d.cursors.contains_key(&status) {
            if !d.pending_more.contains(&status) {
                d.pending_more.push(status);
            }
            return;
        }
        let generation = d.generation;
        let version = d.requests.entry(status.clone()).or_default();
        *version += 1;
        (generation, *version)
    };
    sleep(250).await;
    if data.read().generation != generation || data.read().requests.get(&status) != Some(&version) {
        return;
    }
    let (since, cursor, activity) = {
        let d = data.read();
        (
            ReadCursor::new(d.journal.clone()),
            if more {
                d.cursors.get(&status).cloned()
            } else {
                None
            },
            u8::from(d.connected),
        )
    };
    let encoded = request::encode(&query);
    let cursor = crate::task_live::cursor_query(cursor.as_deref());
    let result = request::get::<TaskListing>(&url(&format!(
        "/api/task-board?q={encoded}&status={status}&days={days}{cursor}&activity={activity}"
    )))
    .await;
    if data.read().generation != generation || data.read().requests.get(&status) != Some(&version) {
        return;
    }
    match result {
        Ok(list) => {
            let mut d = data.write();
            let has_next = list.next.is_some();
            if let Some(cursor) = list.next {
                d.cursors.insert(status.clone(), cursor);
            } else {
                d.cursors.remove(&status);
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
            d.work = list.machine_work.into();
            replay(&since, &mut d);
            crate::live::mark_tasks_ready();
            let queued = !more && has_next && d.pending_more.contains(&status);
            d.pending_more.retain(|waiting| waiting != &status);
            drop(d);
            if queued {
                spawn(column(status, true, query, days, data));
            }
        }
        Err(err) => data.write().error = err,
    }
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
                refresh = true;
            }
            "task" | "task-summary" => {
                let mut change: ItemChange<Task> =
                    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?;
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
    frame: FrameRefresh,
) {
    let mut backoff = crate::state::Backoff::default();
    let pending = Rc::new(Cell::new(false));
    loop {
        let mut held = 0.0;
        match request::get::<Handshake>(&url("/api/board")).await {
            Ok(board) => {
                crate::live::reload_if_redeployed(&board.build);
                data.write().generation += 1;
                counts(data).await;
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
                                if !data.read().connected {
                                    {
                                        let mut d = data.write();
                                        d.connected = true;
                                        d.generation += 1;
                                        d.cursors.clear();
                                        d.pending_more.clear();
                                    }
                                    counts(data).await;
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
                                let Ok(refresh) = apply(&value, data) else {
                                    break;
                                };
                                if refresh && !pending.replace(true) {
                                    let pending = pending.clone();
                                    spawn(async move {
                                        loop {
                                            sleep(750).await;
                                            let generation = data.read().generation;
                                            counts(data).await;
                                            if generation == data.read().generation {
                                                refreshed += 1;
                                                pending.set(false);
                                                break;
                                            }
                                        }
                                    });
                                }
                            }
                            _ => break,
                        }
                    }
                    held = opened.map_or(0.0, |at| js_sys::Date::now() - at);
                }
            }
            Err(err) => data.write().error = err,
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
        sleep(backoff.after(held)).await;
    }
}
