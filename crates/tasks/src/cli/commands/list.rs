//! `list`: the board as a tree, or as JSON; `--mine` keeps the tasks the session holds a role on,
//! `--doubled` those still holding two orchestrators ([`crate::orchestration::doubled`]).

use super::{Context, Stop};
use crate::cli::shape;
use crate::cli::text::{roles_text, tree_order};
use crate::pyjson::{self, Py};
use crate::{Store, orchestration};
use std::collections::HashSet;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let mut tasks = store.tasks(p.get("status"))?;
    if p.flag("mine") {
        let me = c.actor()?;
        tasks.retain(|t| t.roles.iter().any(|r| r.handle == me));
    }
    if p.flag("doubled") {
        let doubled: HashSet<String> = (orchestration::doubled(&tasks))
            .map(|t| t.slug.clone())
            .collect();
        tasks.retain(|t| doubled.contains(&t.slug));
    }
    if p.flag("json") {
        let json = Py::obj(vec![("tasks", shape::tasks(&tasks))]);
        c.say(&pyjson::dumps(&json));
        return Ok(());
    }
    for (depth, t) in tree_order(&tasks) {
        let roles = roles_text(t);
        let roles = if roles.is_empty() {
            String::new()
        } else {
            format!("  [{roles}]")
        };
        c.say(&format!(
            "{}p{} {:<11} {:<28} {}{roles}",
            "  ".repeat(depth),
            t.priority,
            t.status,
            t.slug,
            t.title
        ));
    }
    Ok(())
}
