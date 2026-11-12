//! Browser transport shared by task views. Only route changes and reconnects fetch snapshots;
//! ordinary feed events merge one record. Composers keep their own signals throughout.

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use symphony_api::{
    AgentPage, Agents, ItemChange, Message, Task, TaskChange, TaskEntry, TaskHistory, TaskListing,
    TaskPage,
};
use wasm_bindgen::JsValue;

#[derive(Clone, Default, PartialEq)]
pub struct Data {
    pub tasks: Option<Vec<Task>>,
    pub detail: Option<TaskPage>,
    pub agents: Option<Agents>,
    /// The slug the server has no task for, as its task route last answered.
    pub missing: Option<String>,
    pub error: String,
    pub entries: Vec<TaskEntry>,
    pub thread_loaded: bool,
    thread_requested: bool,
    child_requested: bool,
    pub before: Option<i64>,
    pub agent_total: usize,
    pub approval: Option<(symphony_api::TaskOffer, f64)>,
    /// The agents page was asked from the server's own machine (`AgentPage::local`).
    pub local: bool,
    page: String,
    child_cursor: Option<String>,
    agent_cursor: Option<String>,
    lost_cursor: Option<String>,
    lost_loaded: bool,
    generation: u64,
    events: Vec<(u64, JsValue)>,
}

impl Data {
    pub fn has_more_agents(&self, lost: bool) -> bool {
        if lost {
            !self.lost_loaded || self.lost_cursor.is_some()
        } else {
            self.agent_cursor.is_some()
        }
    }
}

pub fn route() -> String {
    format!("/{}", crate::navigation::Route::current().path)
}

pub fn url(path: &str) -> String {
    crate::docs::scoped(&crate::navigation::Route::current().scope, path)
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
    let result = if selected.starts_with("/task/") {
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
        let requested = data.read().thread_requested && same_task;
        let children_requested = data.read().child_requested && same_task;
        refresh(route, data).await;
        if catch_up {
            catch_up_thread(route, data).await;
        } else if requested {
            history(false, route, data).await;
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
    } else if selected == "/agents" {
        match get::<AgentPage>("/api/agent-page").await {
            Ok(page) => {
                if route() == selected {
                    data.write().agents = Some(page.view);
                    data.write().local = page.local;
                    data.write().tasks = Some(page.tasks);
                    data.write().agent_total = page.total;
                    data.write().agent_cursor = page.next;
                    data.write().lost_cursor = None;
                    data.write().lost_loaded = false;
                }
                Ok(())
            }
            Err(err) => Err(err),
        }
    } else {
        Ok(())
    };
    if route() == selected {
        data.write().page = selected;
        data.write().error = result.err().unwrap_or_default();
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
            "task" | "task-summary" => {
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
                    } else if region == "task" {
                        reload_detail = true;
                    }
                }
                // A task filed under the slug the page could not find: read it now.
                reload_page |= !change.removed
                    && next.missing.as_deref() == Some(change.key.as_str())
                    && selected.strip_prefix("/task/") == Some(change.key.as_str());
                if let Some(tasks) = &mut next.tasks {
                    item(tasks, change, |t| &t.slug);
                }
            }
            "task-fields" => {
                let change: TaskChange = decode(value)?;
                if let Some(task) = next
                    .tasks
                    .as_mut()
                    .and_then(|tasks| tasks.iter_mut().find(|t| t.slug == change.key))
                {
                    crate::tasks::merge(task, &change.fields);
                }
                if let Some(page) = next.detail.as_mut().filter(|p| p.task.slug == change.key) {
                    crate::tasks::merge(&mut page.task, &change.fields);
                    if change.fields.body.is_some() || change.fields.status.is_some() {
                        reload_detail = true;
                    }
                }
            }
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
            "agent" => {
                let change = decode(value)?;
                if let Some(agents) = &mut next.agents {
                    item(&mut agents.agents, change, |a| &a.handle);
                }
            }
            "agent-probes" => {
                if let Some(agents) = &mut next.agents {
                    agents.probes = decode(value)?;
                }
            }
            "machine-work" => {
                let work: symphony_api::MachineWork = decode(value)?;
                if let Some(page) = &mut next.detail {
                    page.machine_work = work.clone();
                }
                if let Some(agents) = &mut next.agents {
                    agents.machine_work = work;
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

pub async fn refresh(route: Memo<String>, mut data: Signal<Data>) {
    let selected = route();
    let since = data.read().generation;
    if let Some(slug) = selected.strip_prefix("/task/") {
        let encoded = js_sys::encode_uri_component(slug)
            .as_string()
            .unwrap_or_default();
        let read =
            crate::request::get_found::<TaskPage>(&url(&format!("/api/task-page?slug={encoded}")))
                .await;
        match read {
            Ok(None) => {
                if route() == selected {
                    data.write().missing = Some(slug.to_string());
                    data.write().page = selected.clone();
                    data.write().error.clear();
                    replay(since, route, data);
                }
            }
            Ok(Some(mut page)) => {
                if route() == selected {
                    if data
                        .read()
                        .detail
                        .as_ref()
                        .is_none_or(|p| p.task.slug != page.task.slug)
                    {
                        data.write().entries.clear();
                        data.write().thread_loaded = false;
                        data.write().approval = None;
                        data.write().thread_requested = false;
                        data.write().child_requested = false;
                        data.write().child_cursor = None;
                        data.write().before = None;
                    } else if let Some(old) = &data.read().detail {
                        page.children = old.children.clone();
                    }
                    data.write().detail = Some(page);
                    data.write().missing = None;
                    data.write().page = selected.clone();
                    data.write().error.clear();
                    replay(since, route, data);
                }
            }
            Err(err) => {
                if route() == selected {
                    data.write().error = err;
                }
            }
        }
    } else {
        Box::pin(load(route, data)).await;
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
    data.write().thread_requested = true;
    let before = if older { data.read().before } else { None };
    if older && before.is_none() {
        return;
    }
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    let cursor = before.map_or(String::new(), |id| format!("&before={id}"));
    match get::<TaskHistory>(&format!("/api/task-history?slug={encoded}{cursor}")).await {
        Ok(page) if route() == selected => {
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

pub async fn more_agents(lost: bool, route: Memo<String>, mut data: Signal<Data>) {
    if route() != "/agents" {
        return;
    }
    let cursor = if lost {
        if data.read().lost_loaded && data.read().lost_cursor.is_none() {
            return;
        }
        data.read().lost_cursor.clone()
    } else {
        if data.read().agent_cursor.is_none() {
            return;
        }
        data.read().agent_cursor.clone()
    };
    let cursor = cursor_query(cursor.as_deref());
    let health = if lost { "&health=lost" } else { "" };
    let query = format!("{health}{cursor}");
    match get::<AgentPage>(&format!(
        "/api/agent-page?{}",
        query.trim_start_matches('&')
    ))
    .await
    {
        Ok(page) if route() == "/agents" => {
            let mut shown = data.write();
            shown.local = page.local;
            if lost {
                shown.lost_cursor = page.next;
                shown.lost_loaded = true;
            } else {
                shown.agent_cursor = page.next;
                shown.agent_total = page.total;
            }
            if let Some(view) = &mut shown.agents {
                for agent in page.view.agents {
                    item(
                        &mut view.agents,
                        ItemChange {
                            key: agent.handle.clone(),
                            value: Some(agent),
                            removed: false,
                        },
                        |a| &a.handle,
                    );
                }
            }
            if let Some(tasks) = &mut shown.tasks {
                for task in page.tasks {
                    item(
                        tasks,
                        ItemChange {
                            key: task.slug.clone(),
                            value: Some(task),
                            removed: false,
                        },
                        |t| &t.slug,
                    );
                }
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
