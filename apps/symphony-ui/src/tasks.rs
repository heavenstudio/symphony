//! Task views and their local state. Drafts belong to the composer, never to feed snapshots.

use crate::machine_work::{Scope, SharedWork, phases_view};
use crate::viewport::NearViewport as Near;
use dioxus::prelude::*;
use std::cmp::Ordering;
use std::rc::Rc;
use symphony_api::{Role, Task, TaskFields, TaskStage};

/// Both of the board's task orders have this one type, so the board compiles one sort for them.
type TaskOrder = fn(&&Task, &&Task) -> Ordering;

/// Open work: the most urgent first, then by slug.
fn by_priority(a: &&Task, b: &&Task) -> Ordering {
    (a.priority, &a.slug).cmp(&(b.priority, &b.slug))
}

/// Finished work: the most recently updated first, then by slug.
fn by_recent(a: &&Task, b: &&Task) -> Ordering {
    b.updated.total_cmp(&a.updated).then(a.slug.cmp(&b.slug))
}

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
    let toggle = if subtree() { "Hide" } else { "Show" };
    let search_input = crate::entry_events::use_value("oninput", move |query| {
        search.set(query.clone());
        onfilter.call((query, days()));
    });
    let query = search();
    let selected = status();
    let mut statuses: Vec<String> = if statuses.is_empty() {
        counts.iter().map(|(s, _)| s.clone()).collect()
    } else {
        statuses
    };
    // With no machine reporting this project's work, nothing can place a task in Build or Test:
    // one narrow rail says so once, where two empty columns would each repeat it.
    let unreported = work.machines.is_empty();
    let stages = [TaskStage::Build, TaskStage::Test].map(TaskStage::as_str);
    if unreported {
        statuses.retain(|s| !stages.contains(&s.as_str()));
    } else {
        for stage in stages {
            if !statuses.iter().any(|s| s == stage) {
                statuses.push(stage.to_string());
            }
        }
    }
    let mut filtered: Vec<_> = tasks
        .iter()
        .filter(|task| {
            (task.is_task() || !query.is_empty())
                && (selected.is_empty() || work.task_stage(task, connected).as_str() == selected)
        })
        .collect();
    if query.is_empty() {
        filtered.sort_unstable_by(by_priority as TaskOrder);
    }
    let column = |state: &str| {
        rsx! {
            TaskColumn { work: work.clone(), connected, key: "{state}", status: state.to_string(), tasks: filtered.iter().filter(|t| work.task_stage(t, connected).as_str() == state && !signoff.contains(&t.slug)).map(|t| (*t).clone()).collect(), count: counts.iter().find(|(s,_)| s == state).map_or(0, |(_,n)| *n), query: search, days, onload, refreshed }
        }
    };
    rsx! {
        div { class: "page-head", h1 { "Tasks" } a { href: crate::project::href("/agents"), "Who is connected" } }
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
            if selected.is_empty() {
                p { id: "board-scroll-hint", class: "board-scroll-hint muted", "Columns continue right → · Swipe, scroll, or focus the board and use arrow keys." }
            }
            if selected.is_empty() || TaskStage::PIPELINE.iter().any(|stage| stage.as_str() == selected) {
            div { class: "board-scroll", tabindex: "0", role: "region", aria_label: "Task pipeline columns", aria_describedby: if selected.is_empty() { "board-scroll-hint" } else { "" },
                div { class: "pipeline",
                    for stage in TaskStage::PIPELINE {
                        if unreported && stage == TaskStage::Build && (selected.is_empty() || stages.contains(&selected.as_str())) {
                            {unreported_rail(connected)}
                        }
                        if statuses.iter().any(|s| s == stage.as_str()) && (selected.is_empty() || selected == stage.as_str()) {
                            {column(stage.as_str())}
                        }
                    }
                }
            }
            }
                div { class: "offline",
                    for state in statuses.iter().filter(|s| ["backlog", "blocked", "cancelled"].contains(&s.as_str())) {
                        if selected.is_empty() || selected == *state {
                            {column(state)}
                        }
                    }
                }
        }
        section { class: "task-groups", aria_labelledby: "task-groups-title",
            h2 { id: "task-groups-title", "Task groups" }
            crate::components::button::Button { aria_expanded: subtree(), "aria-controls": "task-groups", actions: crate::entry_events::click(move || subtree.set(!subtree())),
                "{toggle} task groups"
            }
            if subtree() {
                TreePanel { known: Rc::new(tasks) }
            }
        }
    }
}

/// Build and Test of a project no machine reports for: one narrow rail where their columns go.
fn unreported_rail(connected: bool) -> Element {
    rsx! { section { class: "column rail", "data-status": "build-test", aria_label: "Build and test",
        h2 { "build · test" }
        p { class: "muted work-empty",
            if connected { "No machine reports this project's builds or tests." } else { "Disconnected · build and test activity unknown" }
        }
        a { href: "/agents#machine-context", "Machine details" }
    } }
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
        div { id: "task-groups",
            // `trouble::Trouble`'s markup for a part of a page, written out: the component would add
            // to the board module's first load, which only this block of it would use.
            if !trouble.read().is_empty() {
                section { class: "trouble card", role: "status",
                    h3 { "The task groups could not be loaded" }
                    p { "{crate::trouble::failed(&trouble())}" }
                    p { class: "trouble-actions",
                        crate::components::button::Button { r#type: "button", actions: crate::entry_events::click(move || load_tree(None, false, roots, total, cursor, trouble)), "Retry" }
                    }
                }
            }
            ul { for task in roots() { TreeNode { key: "{task.slug}", task, known: known.clone() } } }
            if roots.read().len() < total() { crate::components::button::Button { actions: crate::entry_events::click(move || load_tree(None,true,roots,total,cursor,trouble)), "Load more task groups" } }
        }
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
                trouble.set(String::new());
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
        a { href: crate::project::href(&format!("/task/{}", shown.slug)), "{shown.title}" } " " span { class: "pill", "{shown.status.replace('_', \" \")}" }
        crate::components::button::Button { aria_expanded: open(), actions: crate::entry_events::click(move || { if !open() { load_tree(Some(slug.clone()),false,children,total,cursor,trouble); } open.set(!open()); }), "Subtasks" }
        if open() {
            if !trouble.read().is_empty() { p { role: "status", "Its subtasks could not be loaded: {trouble}" } }
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
    // Complete totals make empty-column reads unnecessary. Retained rows still refresh so a
    // reconnect can remove stale cards, and a newly populated column must request its page.
    let needs_page = count > 0 || !tasks.is_empty();
    use_effect(use_reactive!(|needs_page| {
        let query = query();
        let days = days();
        let _ = refreshed();
        if visible() && needs_page {
            onload.call((requested_status.clone(), false, query, days));
        }
    }));
    let mut rows: Vec<_> = tasks.iter().collect();
    if query().is_empty() && (status == "done" || status == "cancelled") {
        rows.sort_unstable_by(by_recent as TaskOrder);
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
        if status == "build" { {crate::machine_work::board_context(&work, connected)} }
        if count == 0 && (status == "build" || status == "test") {
            p { class: "muted work-empty", "No tasks with fresh {status} activity." }
        }
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
        a { href: crate::project::href(&format!("/task/{}", task.slug)), code { "{task.slug}" } } " " span { class: "prio", "p{task.priority}" }
        span { class: "task-status st-{task.status}", "{task.status.replace('_', \" \")}" }
        div { "{task.title}" }
        {phases_view(&work, &Scope::Task(&task.slug), connected)}
        if let Some(parent) = &task.parent {
            div { class: "subtask-parent", "Subtask of " a { href: crate::project::href(&format!("/task/{parent}")), "{parent}" } }
        }
        div { class: "chips", for link in &task.links { a { class: "docchip", href: crate::project::href(&format!("/{link}")), "{link.rsplit('/').next().unwrap_or(link)}" } } }
        Roles { roles: task.roles }
    } }
}

#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_detail.rs"]
mod detail;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
use crate::timeline_slot as timeline;
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
    /// shows its answer, under the probe's name.
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
        assert!(
            html.contains(">Terminal panes: unavailable · Test slots: ok</p>"),
            "{html}"
        );
    }

    #[derive(Clone, PartialEq, Props)]
    struct Board {
        work: crate::machine_work::SharedWork,
        connected: bool,
        statuses: Vec<String>,
    }

    #[expect(non_snake_case, reason = "a Dioxus component")]
    fn BoardPage(board: Board) -> Element {
        let refreshed = use_signal(|| 0_u64);
        rsx! { super::TaskBoard {
            tasks: Vec::new(), work: board.work, connected: board.connected, signoff: Vec::new(),
            counts: Vec::new(), statuses: board.statuses, refreshed, onfilter: |_| {}, onload: |_| {},
        } }
    }

    /// The pipeline's columns, in order, as `data-status` names them.
    fn pipeline(work: symphony_api::MachineWork, connected: bool, statuses: &[&str]) -> String {
        let board = Board {
            work: work.into(),
            connected,
            statuses: statuses.iter().map(|s| (*s).to_string()).collect(),
        };
        let mut dom = VirtualDom::new_with_props(BoardPage, board);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn columns(html: &str) -> Vec<&str> {
        html.split("data-status=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    fn reported() -> symphony_api::MachineWork {
        serde_json::from_value(
            json!({"freshness":"incomplete","unassigned":[],"machines":[{
            "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
            "age_ms":0,"freshness":"incomplete","complete":false,"reason":"","counts":null,
            "operations":[]}]}),
        )
        .unwrap()
    }

    const LIFECYCLE: [&str; 9] = [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
    ];

    /// No machine reports the project's work: Build and Test are one narrow rail, said once, and
    /// the status filter offers neither. A server's listing that names them changes nothing.
    #[test]
    fn a_project_no_machine_reports_for_draws_one_rail_for_build_and_test() {
        let mut named = LIFECYCLE.to_vec();
        named.extend(["build", "test"]);
        for statuses in [&LIFECYCLE[..], &named[..]] {
            let html = pipeline(symphony_api::MachineWork::default(), true, statuses);
            assert_eq!(
                columns(&html),
                [
                    "concept",
                    "design",
                    "ready",
                    "in_progress",
                    "build-test",
                    "review",
                    "done",
                    "backlog",
                    "blocked",
                    "cancelled"
                ],
                "{html}"
            );
            assert_eq!(
                html.matches("No machine reports this project").count(),
                1,
                "{html}"
            );
            assert!(!html.contains("No tasks with fresh"), "{html}");
            assert!(!html.contains("Machine context"), "{html}");
            assert!(html.contains("/agents#machine-context"), "{html}");
        }
        let html = pipeline(symphony_api::MachineWork::default(), false, &LIFECYCLE);
        assert!(
            html.contains("Disconnected · build and test activity unknown"),
            "{html}"
        );
    }

    /// Once a machine reports, even with nothing going, Build and Test are full columns again.
    #[test]
    fn a_reporting_machine_brings_back_the_build_and_test_columns() {
        let html = pipeline(reported(), true, &LIFECYCLE);
        let columns = columns(&html);
        assert_eq!(
            columns[..8],
            [
                "concept",
                "design",
                "ready",
                "in_progress",
                "build",
                "test",
                "review",
                "done"
            ],
            "{html}"
        );
        assert!(!columns.contains(&"build-test"), "{html}");
        assert_eq!(html.matches("No tasks with fresh").count(), 2, "{html}");
        assert_eq!(html.matches("Machine context").count(), 1, "{html}");
    }

    #[test]
    fn open_work_sorts_by_priority_then_slug_and_finished_work_newest_first_then_slug() {
        let task = |slug: &str, priority: i64, updated: f64| -> symphony_api::Task {
            serde_json::from_value(json!({"slug": slug, "title": slug, "body": "",
                "status": "ready", "priority": priority, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": updated, "links": [], "depends": [], "roles": [],
                "instruction": false}))
            .unwrap()
        };
        let tasks = [
            task("b", 2, 10.0),
            task("c", 1, 30.0),
            task("a", 2, 30.0),
            task("d", 1, 10.0),
        ];
        let slugs = |order: super::TaskOrder| {
            let mut rows: Vec<_> = tasks.iter().collect();
            rows.sort_unstable_by(order);
            rows.iter().map(|t| t.slug.as_str()).collect::<Vec<_>>()
        };
        assert_eq!(slugs(super::by_priority), ["c", "d", "a", "b"]);
        assert_eq!(slugs(super::by_recent), ["a", "c", "b", "d"]);
    }
}
