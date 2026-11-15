//! The agents page's task views and state. The task board and a task's page are the web client's
//! (apps/symphony-web/src/board.tsx, src/task-page.tsx); the agents page draws its cards here
//! ([`TaskRow`]).

use dioxus::prelude::*;
use symphony_api::{Role, Task, TaskFields};
// The agents page's alone (`detail`): a card and the board's order.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
use {
    crate::machine_work::{Scope, SharedWork, phases_view},
    crate::viewport::NearViewport as Near,
    std::cmp::Ordering,
    symphony_api::slug_order,
};

/// Open work: the most urgent first, then by slug, numbers by their value (`ready-3` before
/// `ready-30`), as the server pages a board's column.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
fn by_priority(a: &&Task, b: &&Task) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then_with(|| slug_order(&a.slug, &b.slug))
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

/// Each role `handle` holds on `tasks` says `health`: the feed sends a holder's health once, as its
/// session (none, when the session is gone, is `lost`, as the board says a role of no session),
/// never again in each task it holds.
pub fn holder_health<'a>(
    tasks: impl IntoIterator<Item = &'a mut Task>,
    handle: &str,
    health: Option<String>,
) {
    let health = health.unwrap_or_else(|| "lost".into());
    for role in tasks
        .into_iter()
        .flat_map(|task| task.roles.iter_mut())
        .filter(|role| role.handle == handle && role.health != health)
    {
        role.health.clone_from(&health);
    }
}

/// Whether the agents page, which lists no record of a task, shows it once `fields` changed it, by
/// the server's own rule (`agent_page::tasks`): an agent it shows (`shown`) came to hold it in a
/// role other than orchestrator, or it finished, for the Finished line of each agent that held it.
pub fn agents_page_takes(fields: &TaskFields, shown: impl Fn(&str) -> bool) -> bool {
    fields
        .status
        .as_deref()
        .is_some_and(|status| status == "done" || status == "cancelled")
        || fields
            .roles
            .iter()
            .flatten()
            .any(|role| role.role != "orchestrator" && shown(&role.handle))
}

#[component]
pub fn Roles(roles: Vec<Role>) -> Element {
    rsx! { span { class: "chips", for role in roles {
        span { key: "{role.role}:{role.handle}", class: "chip health-{role.health}", title: "{role.role} is {role.health}", "{role.role} {role.handle}" }
    } } }
}

/// A task's card, as the board draws it (the web client's `taskcard.tsx`): the agents page's.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[component]
fn TaskRow(
    task: Task,
    #[props(default)] work: SharedWork,
    #[props(default = true)] connected: bool,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_effect(crate::live::mark_tasks_ready);
    // The title is the card's one link to its task; the slug above it is text to read and copy.
    rsx! { div { class: "taskcard p{task.priority}", "data-slug": "{task.slug}",
        code { class: "taskcard-slug", "{task.slug}" } " " span { class: "prio", "p{task.priority}" }
        span { class: "task-status st-{task.status}", "{task.status.replace('_', \" \")}" }
        a { class: "taskcard-title", href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" }
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
pub use detail::AgentsView;
#[cfg(all(target_arch = "wasm32", feature = "page-tasks"))]
pub(crate) use detail::LiveTaskPage;

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

    /// A card opens its task from its title: the title is the card's one link to the task, and
    /// the slug above it is plain text. The subtask's parent and the linked documents keep theirs.
    #[test]
    fn a_cards_title_is_its_one_link_to_the_task() {
        let task: symphony_api::Task = serde_json::from_value(json!({"slug": "tools/fix-it",
            "title": "Fix the thing", "body": "", "status": "ready", "priority": 2,
            "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
            "links": [], "depends": [], "roles": [], "instruction": false}))
        .unwrap();
        let html = dioxus_ssr::render_element(rsx! { super::TaskRow { task: task.clone() } });
        assert_eq!(html.matches("<a ").count(), 1, "{html}");
        assert!(
            html.contains(
                r#"<a class="taskcard-title" href="/task/tools/fix-it">Fix the thing</a>"#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"<code class="taskcard-slug">tools/fix-it</code>"#),
            "{html}"
        );
        let mut subtask = task;
        subtask.parent = Some("tools/parent".into());
        subtask.links = vec!["docs/TASKS.md".into()];
        let html = dioxus_ssr::render_element(rsx! { super::TaskRow { task: subtask } });
        assert_eq!(
            html.matches(r#"href="/task/tools/fix-it""#).count(),
            1,
            "{html}"
        );
        assert!(html.contains(r#"href="/task/tools/parent""#), "{html}");
        assert!(html.contains(r#"href="/docs/TASKS.md""#), "{html}");
    }

    #[test]
    fn open_work_sorts_by_priority_then_slug() {
        let task = |slug: &str, priority: i64, updated: f64| -> symphony_api::Task {
            serde_json::from_value(json!({"slug": slug, "title": slug, "body": "",
                "status": "ready", "priority": priority, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": updated, "links": [], "depends": [], "roles": [],
                "instruction": false}))
            .unwrap()
        };
        let slugs = |tasks: &[symphony_api::Task]| {
            let mut rows: Vec<_> = tasks.iter().collect();
            rows.sort_unstable_by(super::by_priority);
            rows.iter().map(|t| t.slug.to_string()).collect::<Vec<_>>()
        };
        assert_eq!(
            slugs(&[
                task("b", 2, 10.0),
                task("c", 1, 30.0),
                task("a", 2, 30.0),
                task("d", 1, 10.0),
            ]),
            ["c", "d", "a", "b"]
        );
        // A slug's numbers order by their value, as the server pages a column.
        assert_eq!(
            slugs(&[
                task("ready-30", 3, 10.0),
                task("ready-3", 3, 10.0),
                task("ready-10", 3, 10.0),
                task("ready-9", 3, 10.0),
                task("urgent-40", 1, 10.0),
            ]),
            ["urgent-40", "ready-3", "ready-9", "ready-10", "ready-30"]
        );
    }

    fn held(slug: &str, roles: &[(&str, &str, &str)]) -> symphony_api::Task {
        serde_json::from_value(json!({
            "slug": slug, "title": slug, "body": "", "status": "done", "priority": 2,
            "parent": null, "created_by": "owner", "created": 1.0, "updated": 1.0,
            "links": [], "depends": [], "instruction": false,
            "roles": roles.iter().map(|(handle, role, health)| json!({
                "handle": handle, "role": role, "since": 1.0, "expires": null, "health": health,
            })).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    fn healths(tasks: &[symphony_api::Task]) -> Vec<Vec<(&str, &str)>> {
        tasks
            .iter()
            .map(|task| {
                task.roles
                    .iter()
                    .map(|role| (role.handle.as_str(), role.health.as_str()))
                    .collect()
            })
            .collect()
    }

    /// The feed says a holder's health once, as its session: every role of that handle, on every
    /// task, says it, and no other role changes.
    #[test]
    fn a_sessions_health_reaches_every_role_its_holder_holds_and_no_other() {
        let mut tasks = vec![
            held(
                "work/a",
                &[
                    ("amber", "executor", "live"),
                    ("birch", "orchestrator", "live"),
                ],
            ),
            held("work/b", &[("amber", "reviewer", "live")]),
            held("work/c", &[("birch", "executor", "live")]),
            held("work/d", &[]),
        ];
        super::holder_health(&mut tasks, "amber", Some("stale".into()));
        assert_eq!(
            healths(&tasks),
            [
                vec![("amber", "stale"), ("birch", "live")],
                vec![("amber", "stale")],
                vec![("birch", "live")],
                vec![],
            ]
        );
        let before = tasks.clone();
        super::holder_health(&mut tasks, "cedar", Some("left".into()));
        assert_eq!(
            tasks, before,
            "a session that holds nothing changes nothing"
        );
    }

    fn change(key: &str, fields: serde_json::Value) -> symphony_api::TaskChange {
        serde_json::from_value(json!({"key": key, "fields": fields})).unwrap()
    }

    /// A session gone from the board leaves its roles held by no session, which the board reads as
    /// lost.
    #[test]
    fn a_removed_sessions_roles_read_lost() {
        let mut tasks = vec![held("work/a", &[("amber", "executor", "stale")])];
        super::holder_health(tasks.iter_mut(), "amber", None);
        assert_eq!(healths(&tasks), [vec![("amber", "lost")]]);
    }

    /// The agents page takes a task it lists no record of as the server lists one: held by an agent
    /// it shows in a role other than orchestrator, or finished; never for an orchestrator's role
    /// alone, an agent it does not show or a change of nothing it reads.
    #[test]
    fn the_agents_page_takes_a_task_a_shown_agent_works_on_or_that_finished() {
        let fields = |value| change("work/x", value).fields;
        let shown = |handle: &str| ["amber", "birch"].contains(&handle);
        let roles = |held: &[(&str, &str)]| {
            json!(
                held.iter()
                    .map(|(handle, role)| json!({"handle": handle, "role": role, "since": 1.0, "expires": null, "health": "live"}))
                    .collect::<Vec<_>>()
            )
        };
        let takes = |value| super::agents_page_takes(&fields(value), shown);
        assert!(takes(
            json!({"roles": roles(&[("amber", "orchestrator"), ("birch", "executor")])})
        ));
        assert!(takes(json!({"roles": roles(&[("amber", "reviewer")])})));
        assert!(
            !takes(json!({"roles": roles(&[("amber", "orchestrator")])})),
            "an orchestrator's open task is in its Following list, read when opened"
        );
        assert!(
            !takes(json!({"roles": roles(&[("cedar", "executor")])})),
            "an agent the page does not show"
        );
        assert!(!takes(json!({"roles": []})));
        for status in ["done", "cancelled"] {
            assert!(
                takes(json!({"status": status})),
                "{status} reaches its holders' Finished lines"
            );
        }
        for status in ["in_progress", "review", "ready"] {
            assert!(!takes(json!({"status": status})), "{status}");
        }
        assert!(!takes(json!({"title": "Renamed", "priority": 1})));
    }
}
