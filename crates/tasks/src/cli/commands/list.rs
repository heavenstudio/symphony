//! `list`: the board's open tasks as a tree, or as JSON, each task with when it took the status it
//! holds ([`Store::in_status_since`]); `--all` adds the done and cancelled ones, as `--status`
//! naming either does; `--mine` keeps the tasks the session holds a role on, `--doubled` those still
//! holding two orchestrators ([`crate::orchestration::doubled`]).

use super::{Context, Stop};
use crate::cli::shape;
use crate::cli::text::{roles_text, tree_order};
use crate::health::FINISHED;
use crate::pyjson::{self, Py};
use crate::{Store, Task, orchestration};
use std::collections::HashSet;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let mut tasks = store.tasks(p.get("status"))?;
    if p.flag("mine") {
        let me = c.actor()?;
        tasks.retain(|t| t.roles.iter().any(|r| r.handle == me));
    }
    // A status asked for is shown whatever it is; otherwise finished work only with --all.
    let listed = tasks.len();
    if p.get("status").is_none() && !p.flag("all") {
        tasks.retain(|t| !FINISHED.contains(&t.status.as_str()));
    }
    let hidden = listed - tasks.len();
    if p.flag("doubled") {
        let doubled: HashSet<String> = (orchestration::doubled(&tasks))
            .map(|t| t.slug.clone())
            .collect();
        tasks.retain(|t| doubled.contains(&t.slug));
    }
    if p.flag("json") {
        let slugs: Vec<String> = tasks.iter().map(|t| t.slug.clone()).collect();
        let since = store.in_status_since(&slugs)?;
        let listed = tasks
            .iter()
            .map(|t| shape::listed(t, since.get(&t.slug).copied()))
            .collect();
        let json = Py::obj(vec![("tasks", Py::List(listed))]);
        c.say(&pyjson::dumps(&json));
        return Ok(());
    }
    if tasks.is_empty() {
        c.say(if p.flag("doubled") {
            "no task holds two orchestrators"
        } else if hidden > 0 {
            "no open tasks (--all lists done and cancelled ones too)"
        } else {
            "no tasks"
        });
        return Ok(());
    }
    for line in tree_lines(&tasks) {
        c.say(&line);
    }
    Ok(())
}

/// `tasks` as the tree prints them, one line each, indented under its parent: its priority, status
/// and slug, and its title, the titles lined up after the longest slug.
fn tree_lines(tasks: &[Task]) -> Vec<String> {
    let ordered = tree_order(tasks);
    let width = ordered
        .iter()
        .map(|(depth, t)| 2 * depth + t.slug.chars().count())
        .max()
        .unwrap_or(0);
    ordered
        .into_iter()
        .map(|(depth, t)| {
            let roles = roles_text(t);
            let roles = if roles.is_empty() {
                String::new()
            } else {
                format!("  [{roles}]")
            };
            let pad = width - 2 * depth;
            format!(
                "{}p{} {:<11} {:<pad$} {}{roles}",
                "  ".repeat(depth),
                t.priority,
                t.status,
                t.slug,
                t.title
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::tree_lines;
    use crate::Task;

    fn task(slug: &str, parent: Option<&str>, status: &str) -> Task {
        Task {
            branch: None,
            id: 0,
            slug: slug.into(),
            title: format!("T {slug}"),
            body: String::new(),
            status: status.into(),
            priority: 2,
            parent: parent.map(str::to_string),
            created_by: String::new(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles: Vec::new(),
            instruction: false,
            fields: Default::default(),
        }
    }

    #[test]
    fn titles_line_up_after_the_longest_slug_at_any_depth() {
        let tasks = [
            task("a/top", None, "ready"),
            task(
                "a/top/a-much-longer-subtask-slug-than-28",
                Some("a/top"),
                "in_progress",
            ),
            task("b/x", None, "review"),
        ];
        assert_eq!(
            tree_lines(&tasks),
            [
                "p2 ready       a/top                                      T a/top",
                "  p2 in_progress a/top/a-much-longer-subtask-slug-than-28 T a/top/a-much-longer-subtask-slug-than-28",
                "p2 review      b/x                                        T b/x",
            ]
        );
        let titles: Vec<usize> = tree_lines(&tasks)
            .iter()
            .map(|line| line.find("T ").unwrap())
            .collect();
        assert!(titles.windows(2).all(|w| w[0] == w[1]), "{titles:?}");
    }
}
