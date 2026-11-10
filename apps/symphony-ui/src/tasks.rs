//! Task views and their local state. Drafts belong to the composer, never to feed snapshots.

use crate::machine_work::{Scope, SharedWork, phases_view};
use crate::viewport::NearViewport as Near;
use dioxus::prelude::*;
use std::rc::Rc;
use symphony_api::{Role, Task, TaskFields};

pub fn merge(task: &mut Task, fields: &TaskFields) {
    macro_rules! field {
        ($($name:ident),*) => {$(if let Some(value) = &fields.$name { task.$name = value.clone(); })*};
    }
    field!(title, body, status, links, depends, roles);
    if let Some(value) = fields.priority {
        task.priority = value;
    }
    if let Some(value) = fields.updated {
        task.updated = value;
    }
    if let Some(value) = fields.instruction {
        task.instruction = value;
    }
    if let Some(parent) = &fields.parent {
        task.parent = (!parent.is_empty()).then(|| parent.clone());
    }
}

#[component]
pub fn TaskBoard(
    tasks: Vec<Task>,
    #[props(default)] work: SharedWork,
    #[props(default = true)] connected: bool,
    signoff: Vec<String>,
    counts: Vec<(String, usize)>,
    statuses: Vec<String>,
    refreshed: Signal<u64>,
    onfilter: EventHandler<(String, u8)>,
    onload: EventHandler<(String, bool, String, u8)>,
) -> Element {
    let mut search = use_signal(String::new);
    let mut status = use_signal(String::new);
    let mut days = use_signal(|| 1_u8);
    let mut subtree = use_signal(|| false);
    let search_input = crate::entry_events::use_value("oninput", move |query| {
        search.set(query.clone());
        onfilter.call((query, days()));
    });
    let query = search();
    let selected = status();
    let statuses = if statuses.is_empty() {
        counts.iter().map(|(s, _)| s.clone()).collect()
    } else {
        statuses
    };
    let mut filtered: Vec<_> = tasks
        .iter()
        .filter(|task| {
            (task.is_task() || !query.is_empty())
                && (selected.is_empty() || task.status == selected)
        })
        .collect();
    if query.is_empty() {
        filtered.sort_unstable_by(|a, b| (a.priority, &a.slug).cmp(&(b.priority, &b.slug)));
    }
    rsx! {
        div { class: "page-head", h1 { "Tasks" } a { href: "/agents", "Who is connected" } }
        {crate::machine_work::machines_view(&work, connected)}
        div { class: "row filters",
            label { "Search tasks and messages " crate::components::input::Input { r#type: "search", value: "{search}", actions: search_input } }
            label { "Status " crate::components::select::SelectField {
                label: "Status", value: status(),
                options: std::iter::once((String::new(), "All statuses".to_string())).chain(statuses.iter().map(|state| (state.clone(), state.replace('_', " ")))).collect::<Vec<_>>(),
                on_change: move |value| status.set(value),
            } }
            label { "Finished tasks " crate::components::select::SelectField {
                label: "Finished tasks", value: days().to_string(),
                options: vec![("1".into(), "Last 24 hours".into()), ("7".into(), "Last week".into())],
                on_change: move |value| {
                    let window = if value == "7" { 7 } else { 1 };
                    days.set(window);
                    onfilter.call((search(), window));
                },
            } }
        }
        div { class: "kanban",
            if !signoff.is_empty() {
                TaskColumn { work: work.clone(), connected, status: "signoff".to_string(), tasks: filtered.iter().filter(|t| signoff.contains(&t.slug)).map(|t| (*t).clone()).collect(), count: signoff.len(), query: search, days, onload, refreshed }
            }
            for (group, states) in [("pipeline", &["concept", "design", "ready", "in_progress", "review", "done"][..]), ("offline", &["backlog", "blocked", "cancelled"][..])] {
                div { class: group,
                    for state in statuses.iter().filter(|s| states.contains(&s.as_str())) {
                        if selected.is_empty() || selected == *state {
                            TaskColumn { work: work.clone(), connected, key: "{state}", status: state.clone(), tasks: filtered.iter().filter(|t| t.status == *state && !signoff.contains(&t.slug)).map(|t| (*t).clone()).collect(), count: counts.iter().find(|(s,_)| s == state).map_or(0, |(_,n)| *n), query: search, days, onload, refreshed }
                        }
                    }
                }
            }
        }
        crate::components::button::Button { aria_expanded: subtree(), actions: crate::entry_events::click(move || subtree.set(!subtree())), "Subtasks" }
        if subtree() {
            TreePanel { known: Rc::new(tasks) }
        }
    }
}

#[component(lazy)]
fn TreePanel(known: Rc<Vec<Task>>) -> Element {
    let roots = use_signal(Vec::<Task>::new);
    let total = use_signal(|| 0_usize);
    let cursor = use_signal(|| None::<String>);
    let trouble = use_signal(String::new);
    use_hook(|| {
        load_tree(None, false, roots, total, cursor, trouble);
    });
    rsx! {
        p { role: "status", "{trouble}" }
        ul { for task in roots() { TreeNode { key: "{task.slug}", task, known: known.clone() } } }
        if roots.read().len() < total() { crate::components::button::Button { actions: crate::entry_events::click(move || load_tree(None,true,roots,total,cursor,trouble)), "Load more task groups" } }
    }
}

fn load_tree(
    parent: Option<String>,
    more: bool,
    mut rows: Signal<Vec<Task>>,
    mut total: Signal<usize>,
    mut cursor: Signal<Option<String>>,
    mut trouble: Signal<String>,
) {
    #[cfg(target_arch = "wasm32")]
    spawn(async move {
        let after = if more { cursor() } else { None };
        let after = crate::task_live::cursor_query(after.as_deref());
        let selection = parent.as_ref().map_or_else(
            || "roots=1".into(),
            |p| {
                format!(
                    "parent={}",
                    js_sys::encode_uri_component(p)
                        .as_string()
                        .unwrap_or_default()
                )
            },
        );
        match crate::task_live::get::<symphony_api::TaskListing>(&format!(
            "/api/task-board?{selection}&older=1{after}"
        ))
        .await
        {
            Ok(list) => {
                cursor.set(list.next);
                total.set(list.counts.iter().map(|(_, n)| n).sum());
                if !more {
                    rows.set(Vec::new());
                }
                for task in list.tasks {
                    if !rows.read().iter().any(|t| t.slug == task.slug) {
                        rows.write().push(task);
                    }
                }
            }
            Err(err) => trouble.set(err),
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (
            parent,
            more,
            &mut rows,
            &mut total,
            &mut cursor,
            &mut trouble,
        );
    }
}

#[component]
fn TreeNode(task: Task, known: Rc<Vec<Task>>) -> Element {
    let shown = known
        .iter()
        .find(|t| t.slug == task.slug)
        .cloned()
        .unwrap_or(task);
    let mut open = use_signal(|| false);
    let children = use_signal(Vec::<Task>::new);
    let total = use_signal(|| 0_usize);
    let cursor = use_signal(|| None::<String>);
    let trouble = use_signal(String::new);
    let slug = shown.slug.clone();
    let more_slug = shown.slug.clone();
    rsx! { Near { id: "tree-{shown.slug}", height: 40, list_item: true,
        a { href: "/task/{shown.slug}", "{shown.title}" } " " span { class: "pill", "{shown.status.replace('_', \" \")}" }
        crate::components::button::Button { aria_expanded: open(), actions: crate::entry_events::click(move || { if !open() { load_tree(Some(slug.clone()),false,children,total,cursor,trouble); } open.set(!open()); }), "Subtasks" }
        if open() {
            p { role: "status", "{trouble}" }
            ul { for child in children() { TreeNode { key: "{child.slug}", task: child, known: known.clone() } } }
            if children.read().len() < total() { crate::components::button::Button { actions: crate::entry_events::click(move || load_tree(Some(more_slug.clone()),true,children,total,cursor,trouble)), "Load more subtasks" } }
        }
    } }
}

#[component]
fn TaskColumn(
    status: String,
    work: SharedWork,
    connected: bool,
    tasks: Vec<Task>,
    count: usize,
    query: Signal<String>,
    days: Signal<u8>,
    onload: EventHandler<(String, bool, String, u8)>,
    refreshed: Signal<u64>,
) -> Element {
    let mut visible = use_signal(|| false);
    let requested_status = status.clone();
    use_effect(move || {
        let query = query();
        let days = days();
        let _ = refreshed();
        if visible() {
            onload.call((requested_status.clone(), false, query, days));
        }
    });
    let mut rows: Vec<_> = tasks.iter().collect();
    if query().is_empty() && (status == "done" || status == "cancelled") {
        rows.sort_unstable_by(|a, b| b.updated.total_cmp(&a.updated).then(a.slug.cmp(&b.slug)));
    }
    let more_status = status.clone();
    // Keep unloaded columns in place while their first page is fetched. Otherwise revealing
    // an empty column briefly shrinks it, pulling every later column into the viewport too.
    let reserved = if count > 0 {
        u32::try_from(count.min(16)).unwrap_or(16) * 130 + 40
    } else {
        20
    };
    rsx! { section { class: if status == "signoff" { "column signoff" } else { "column" }, "data-status": "{status}",
        h2 { if status == "signoff" { "Waiting for your sign-off" } else { "{status.replace('_', \" \")}" } " " span { class: "count", "{count}" } }
        Near { id: "column-{status}", height: reserved, onenter: move |_| visible.set(true),
            if count > 0 && tasks.is_empty() { div { style: "min-height:{reserved}px", role: "status", "Loading tasks…" } }
            for task in rows { Near { key: "{task.slug}", id: "row-{task.slug}", height: 130, TaskRow { task: task.clone(), work: work.clone(), connected } } }
            if count > tasks.len() {
                crate::components::button::Button { actions: crate::entry_events::click(move || onload.call((more_status.clone(),true,query(),days()))), "Load more tasks" }
            }
        }
    } }
}

#[component]
pub fn Roles(roles: Vec<Role>) -> Element {
    rsx! { span { class: "chips", for role in roles {
        span { key: "{role.role}:{role.handle}", class: "chip health-{role.health}", title: "{role.role} is {role.health}", "{role.role} {role.handle}" }
    } } }
}

#[component]
fn TaskRow(
    task: Task,
    #[props(default)] work: SharedWork,
    #[props(default = true)] connected: bool,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_effect(crate::live::mark_tasks_ready);
    rsx! { div { class: "taskcard p{task.priority}", "data-slug": "{task.slug}",
        a { href: "/task/{task.slug}", code { "{task.slug}" } } " " span { class: "prio", "p{task.priority}" }
        span { class: "task-status st-{task.status}", "{task.status.replace('_', \" \")}" }
        div { "{task.title}" }
        {phases_view(&work, &Scope::Task(&task.slug), connected)}
        if let Some(parent) = &task.parent {
            div { class: "subtask-parent", "Subtask of " a { href: "/task/{parent}", "{parent}" } }
        }
        div { class: "chips", for link in &task.links { a { class: "docchip", href: "/{link}", "{link.rsplit('/').next().unwrap_or(link)}" } } }
        Roles { roles: task.roles }
    } }
}

#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_detail.rs"]
mod detail;
#[cfg(all(target_arch = "wasm32", feature = "page-tasks"))]
pub(crate) use detail::LiveTaskPage;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
pub use detail::{AgentsView, TaskDetail};

#[cfg(test)]
mod tests {
    use super::AgentsView;
    use dioxus::prelude::*;
    use serde_json::json;

    /// A server from before the panes probe's rename names it `herdr` alone: the agents page still
    /// shows its answer.
    #[test]
    fn the_agents_page_shows_the_panes_probe_a_server_named_herdr() {
        fn page() -> Element {
            let agents: symphony_api::Agents = serde_json::from_value(json!({
                "generated": 1.5,
                "probes": {"herdr": "unavailable", "slots": "ok"},
                "agents": [],
            }))
            .unwrap();
            rsx! { AgentsView { agents, tasks: Vec::new(), total: 0, more: false, lost_more: false, onmore: |_| {} } }
        }
        let mut dom = VirtualDom::new(page);
        dom.rebuild_in_place();
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">unavailable · ok</p>"), "{html}");
    }
}
