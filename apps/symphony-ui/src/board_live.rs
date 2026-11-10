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
    statuses: Vec<String>,
    signoff: Vec<String>,
    query: String,
    days: u8,
    ready: bool,
    error: String,
    cursors: std::collections::BTreeMap<String, String>,
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
    let (query, days, generation, request, since) = {
        let mut d = data.write();
        d.count_request += 1;
        (
            d.query.clone(),
            d.days,
            d.generation,
            d.count_request,
            ReadCursor::new(d.journal.clone()),
        )
    };
    let encoded = request::encode(&query);
    let result = request::get::<TaskListing>(&url(&format!(
        "/api/task-board?counts=1&q={encoded}&days={days}"
    )))
    .await;
    if data.read().generation != generation || data.read().count_request != request {
        return;
    }
    match result {
        Ok(list) => {
            let mut d = data.write();
            d.counts = list.counts;
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
        let generation = d.generation;
        let version = d.requests.entry(status.clone()).or_default();
        *version += 1;
        (generation, *version)
    };
    sleep(250).await;
    if data.read().generation != generation || data.read().requests.get(&status) != Some(&version) {
        return;
    }
    let (since, cursor) = {
        let d = data.read();
        (
            ReadCursor::new(d.journal.clone()),
            if more {
                d.cursors.get(&status).cloned()
            } else {
                None
            },
        )
    };
    let encoded = request::encode(&query);
    let cursor = crate::task_live::cursor_query(cursor.as_deref());
    let result = request::get::<TaskListing>(&url(&format!(
        "/api/task-board?q={encoded}&status={status}&days={days}{cursor}"
    )))
    .await;
    if data.read().generation != generation || data.read().requests.get(&status) != Some(&version) {
        return;
    }
    match result {
        Ok(list) => {
            let mut d = data.write();
            if let Some(cursor) = list.next {
                d.cursors.insert(status.clone(), cursor);
            } else {
                d.cursors.remove(&status);
            }
            if !more {
                d.tasks.retain(|t| t.status != status);
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
            for (s, n) in list.counts {
                if s == status {
                    set_count(&mut d.counts, s, n);
                }
            }
            d.signoff = list.signoff;
            d.work = list.machine_work.into();
            replay(&since, &mut d);
            crate::live::mark_tasks_ready();
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

fn set_count(counts: &mut Vec<(String, usize)>, status: String, count: usize) {
    if let Some((_, old)) = counts.iter_mut().find(|(s, _)| *s == status) {
        *old = count;
    } else {
        counts.push((status, count));
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
                refresh |= searching;
            }
            "message" => refresh |= searching,
            "task-signoff" if searching => refresh = true,
            "task-signoff" => {
                d.signoff = serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?
            }
            "task-counts" if !searching => {
                let counts: Vec<(String, usize)> =
                    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())?;
                for (status, count) in counts {
                    if status != "done" && status != "cancelled" {
                        set_count(&mut d.counts, status, count);
                    }
                }
            }
            "task-recent-counts" if !searching => {
                let counts: Vec<(String, usize)> = serde_wasm_bindgen::from_value(field(
                    &value,
                    if d.days == 7 { "week" } else { "day" },
                ))
                .map_err(|e| e.to_string())?;
                for (status, count) in counts {
                    set_count(&mut d.counts, status, count);
                }
            }
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
                                        sleep(750).await;
                                        counts(data).await;
                                        refreshed += 1;
                                        pending.set(false);
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
        sleep(backoff.after(held)).await;
    }
}
