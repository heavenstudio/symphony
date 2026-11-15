//! Task views and their local state. Drafts belong to the composer, never to feed snapshots. The
//! task board and the agents page are the web client's (apps/symphony-web/src/board.tsx,
//! agents.tsx).

use symphony_api::{Task, TaskChange, TaskFields, TaskPage};
// The task page's alone (`detail`).
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
use {
    crate::machine_work::{Scope, phases_view},
    crate::viewport::NearViewport as Near,
    dioxus::prelude::*,
    symphony_api::Role,
};

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

/// What a task page made of a task's changed fields ([`page_fields`]).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PageFields {
    /// The page is read again: its task's brief or status changed, or a subtask came or went, so
    /// its count of subtasks did.
    pub reload: bool,
    /// A task came under the page's task: its subtasks are read again with the page.
    pub child_came: bool,
}

/// The task page `page` takes a task's changed fields, which the feed sends alone, never the
/// task's record again: into its task and into a subtask it lists. A subtask that left its task
/// goes from the list, and one that came is read with the list again.
pub fn page_fields(page: &mut TaskPage, change: &TaskChange) -> PageFields {
    let mut done = PageFields::default();
    if page.task.slug == change.key {
        merge(&mut page.task, &change.fields);
        done.reload = change.fields.body.is_some() || change.fields.status.is_some();
    }
    let child = page.children.iter().position(|t| t.slug == change.key);
    if let Some(at) = child {
        merge(&mut page.children[at], &change.fields);
    }
    if let Some(parent) = &change.fields.parent
        && (*parent == page.task.slug) != child.is_some()
    {
        if let Some(at) = child {
            page.children.remove(at);
        } else {
            done.child_came = true;
        }
        done.reload = true;
    }
    done
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

#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_detail.rs"]
mod detail;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
use crate::timeline_slot as timeline;
#[cfg(all(target_arch = "wasm32", feature = "page-tasks"))]
pub(crate) use detail::LiveTaskPage;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
pub use detail::TaskDetail;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_gates.rs"]
pub mod gates;

/// Whether the card the feed's `pending` region names by `key` (`/docs/X.md#card`) is in a
/// document the task links (`links`, `docs/X.md#part`): one its owner gate may count, so a task
/// page's gates read again when it comes, changes or goes (`task_live::Data::gates_nudged`).
pub fn links_card(links: &[String], key: &str) -> bool {
    let path = key
        .trim_start_matches('/')
        .split('#')
        .next()
        .unwrap_or_default();
    !path.is_empty()
        && links
            .iter()
            .any(|link| link.split('#').next() == Some(path))
}

/// The branch whose new tip changes the gates of the task `slug`: the branch they were last
/// answered for (`answered`, with the task they were answered for) while that is this task, else
/// the slug, the branch the board names a task's work by.
pub fn gates_branch<'a>(slug: &'a str, answered: Option<&'a (String, String)>) -> &'a str {
    match answered {
        Some((task, branch)) if task == slug => branch,
        _ => slug,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_task_counts_a_card_in_a_document_it_links_whatever_part_it_links() {
        let links = ["docs/analysis/x.md#part".to_string(), "docs/B.md".into()];
        for (key, linked) in [
            ("/docs/analysis/x.md#card", true),
            ("docs/analysis/x.md#other", true),
            ("/docs/B.md#c", true),
            ("/docs/B.md", true),
            ("/docs/analysis/y.md#card", false),
            ("/docs/analysis/x.md.bak#card", false),
            ("/docs/analysis", false),
            ("#card", false),
            ("", false),
        ] {
            assert_eq!(super::links_card(&links, key), linked, "{key}");
        }
        assert!(!super::links_card(&[], "/docs/B.md#c"));
        assert!(
            !super::links_card(&[String::new()], "#card"),
            "an empty path is no document"
        );
    }

    #[test]
    fn a_tasks_gates_follow_the_branch_they_were_answered_for_else_its_slug() {
        let answered = ("tools/x".to_string(), "feature/x".to_string());
        assert_eq!(super::gates_branch("tools/x", Some(&answered)), "feature/x");
        assert_eq!(
            super::gates_branch("tools/y", Some(&answered)),
            "tools/y",
            "another task's answer says nothing of this one's"
        );
        assert_eq!(super::gates_branch("tools/x", None), "tools/x");
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

    /// A task page over `work/a` listing its subtask `work/a/1`, of two.
    fn page() -> symphony_api::TaskPage {
        let mut child = held("work/a/1", &[]);
        child.parent = Some("work/a".into());
        serde_json::from_value(json!({
            "task": held("work/a", &[]), "blocks": [], "children": [child], "children_count": 2,
        }))
        .unwrap()
    }

    fn change(key: &str, fields: serde_json::Value) -> symphony_api::TaskChange {
        serde_json::from_value(json!({"key": key, "fields": fields})).unwrap()
    }

    /// The feed sends a changed task as its changed fields alone: the page's task takes them, read
    /// again only for a new brief or status; a subtask it lists takes them in its place.
    #[test]
    fn a_task_page_takes_a_changed_tasks_fields_into_its_task_and_its_subtasks() {
        let mut page = page();
        let done = super::page_fields(&mut page, &change("work/a", json!({"title": "Renamed"})));
        assert_eq!(done, super::PageFields::default());
        assert_eq!(page.task.title, "Renamed");

        let done = super::page_fields(&mut page, &change("work/a", json!({"status": "review"})));
        assert_eq!(
            done,
            super::PageFields {
                reload: true,
                child_came: false
            }
        );
        assert_eq!(page.task.status, "review");

        let done = super::page_fields(
            &mut page,
            &change(
                "work/a/1",
                json!({"status": "in_progress", "title": "First"}),
            ),
        );
        assert_eq!(
            done,
            super::PageFields::default(),
            "a subtask's status moves no count"
        );
        assert_eq!(
            (
                page.children[0].title.as_str(),
                page.children[0].status.as_str()
            ),
            ("First", "in_progress")
        );

        let before = page.clone();
        let done = super::page_fields(&mut page, &change("work/b", json!({"title": "Other"})));
        assert_eq!(done, super::PageFields::default());
        assert_eq!(
            page, before,
            "a task the page does not show changes nothing"
        );
    }

    /// A subtask that leaves the page's task goes from its list; a task that comes under it is
    /// read with the list again, as the page holds no record of it to add.
    #[test]
    fn a_subtask_that_leaves_goes_and_one_that_comes_is_read_again() {
        let mut page = page();
        let done = super::page_fields(&mut page, &change("work/a/1", json!({"parent": "work/b"})));
        assert_eq!(
            done,
            super::PageFields {
                reload: true,
                child_came: false
            }
        );
        assert!(page.children.is_empty());

        let done = super::page_fields(&mut page, &change("work/c", json!({"parent": "work/a"})));
        assert_eq!(
            done,
            super::PageFields {
                reload: true,
                child_came: true
            }
        );
        assert!(page.children.is_empty(), "its record comes with the list");

        let done = super::page_fields(&mut page, &change("work/d", json!({"parent": ""})));
        assert_eq!(
            done,
            super::PageFields::default(),
            "a task that was no subtask and leaves its parent concerns no other page"
        );
    }

    /// A session gone from the board leaves its roles held by no session, which the board reads as
    /// lost.
    #[test]
    fn a_removed_sessions_roles_read_lost() {
        let mut tasks = vec![held("work/a", &[("amber", "executor", "stale")])];
        super::holder_health(tasks.iter_mut(), "amber", None);
        assert_eq!(healths(&tasks), [vec![("amber", "lost")]]);
    }
}
