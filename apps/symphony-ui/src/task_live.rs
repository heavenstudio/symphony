//! Browser transport of the agents page (`/agents`). Only route changes and reconnects fetch
//! snapshots; ordinary feed events merge one record. A task's page is the web client's
//! (apps/symphony-web/src/task-live.ts).

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use symphony_api::{AgentPage, Agents, ItemChange, Task, TaskChange};
use wasm_bindgen::JsValue;

use crate::cache::Freshness;

#[derive(Clone, Default, PartialEq)]
pub struct Data {
    pub tasks: Option<Vec<Task>>,
    pub agents: Option<Agents>,
    pub error: String,
    pub agent_total: usize,
    /// The agents page was asked from the server's own machine (`AgentPage::local`).
    pub local: bool,
    page: String,
    agent_cursor: Option<String>,
    lost_cursor: Option<String>,
    lost_loaded: bool,
    generation: u64,
    events: Vec<(u64, JsValue)>,
    /// What the page shows from this device, and whether the server has answered it.
    pub freshness: Freshness,
}

impl Data {
    /// Whether the agents page has its content to draw. Until it has, a failed read is said by the
    /// page's own trouble block, not by the notices.
    pub fn drawn(&self) -> bool {
        self.agents.is_some()
    }

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
    let result = if selected == "/agents" {
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
    crate::viewport::hold_place("[data-slug]");
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
    let mut reload_page = false;
    let selected = self::route();
    for change in js_sys::Array::from(&property(value, "changes")) {
        let region = property(&change, "region").as_string().unwrap_or_default();
        let value = property(&change, "data");
        match region.as_str() {
            "task" => {
                let change: ItemChange<Task> = decode(value)?;
                if let Some(tasks) = &mut next.tasks {
                    item(tasks, change, |t| &t.slug);
                }
            }
            // The feed sends a task it changed as these fields alone: a view that does not hold
            // the task reads it again when the change brings it in.
            "task-fields" => {
                let change: TaskChange = decode(value)?;
                let listed = next
                    .tasks
                    .as_mut()
                    .and_then(|tasks| tasks.iter_mut().find(|t| t.slug == change.key));
                let listed = listed
                    .map(|task| crate::tasks::merge(task, &change.fields))
                    .is_some();
                // A task the agents page lists no record of, which it now shows.
                reload_page |= selected == "/agents"
                    && !listed
                    && next.agents.as_ref().is_some_and(|agents| {
                        crate::tasks::agents_page_takes(&change.fields, |handle| {
                            agents.agents.iter().any(|agent| agent.handle == handle)
                        })
                    });
            }
            // A holder's health: its roles on the agents' tasks say it.
            "session" => crate::tasks::holder_health(
                next.tasks.iter_mut().flatten(),
                &property(&value, "key").as_string().unwrap_or_default(),
                property(&property(&value, "value"), "health").as_string(),
            ),
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
    Ok(reload_page)
}

/// Read the route's page again: an agents page drawn merges its first page, any other reads it.
pub async fn refresh(route: Memo<String>, data: Signal<Data>) {
    let selected = route();
    if selected == "/agents" && data.peek().drawn() {
        // What the feed changed on an agents page drawn: its first page merged, not read afresh.
        merge_agents("", None, route, data).await;
    } else {
        Box::pin(load(route, data)).await;
    }
}

pub async fn more_agents(lost: bool, route: Memo<String>, data: Signal<Data>) {
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
    let query = format!("?{}", query.trim_start_matches('&'));
    merge_agents(&query, Some(lost), route, data).await;
}

/// Read the agents page at `query` and merge its answer into the page drawn, the agents and the
/// tasks it lists put in or replaced: the next page of the lost agents or of the others (`paged`),
/// whose cursor moves on, or else its first page again for what the feed changed, which keeps both
/// cursors and the lost agents opened.
async fn merge_agents(
    query: &str,
    paged: Option<bool>,
    route: Memo<String>,
    mut data: Signal<Data>,
) {
    match get::<AgentPage>(&format!("/api/agent-page{query}")).await {
        Ok(page) if route() == "/agents" => {
            let mut shown = data.write();
            shown.local = page.local;
            match paged {
                Some(true) => {
                    shown.lost_cursor = page.next;
                    shown.lost_loaded = true;
                }
                Some(false) => {
                    shown.agent_cursor = page.next;
                    shown.agent_total = page.total;
                }
                None => {
                    shown.agent_total = page.total;
                    shown.error.clear();
                }
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
