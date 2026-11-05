//! Roles on a task beside claim and reserve: joining as orchestrator or reviewer, handing the
//! orchestrator role over (one orchestrator a task, [`crate::orchestration`]), and letting a
//! role go.

use crate::orchestration;
use crate::rows::RoleRow;
use crate::store::{Change, Error, Say, Store, Task, refused, repr};

impl Store {
    /// Join a task as `orchestrator` or `reviewer` (executor is `claim`, reserved is `reserve`).
    pub fn take_role(&self, slug: &str, handle: &str, role: &str) -> Result<Task, Error> {
        if role != "orchestrator" && role != "reviewer" {
            return Err(refused(format!(
                "take_role is for orchestrator or reviewer; use claim or reserve for {}",
                repr(role)
            )));
        }
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!("{slug} is {}", task.status)));
            }
            if role == "reviewer" && view.holds(handle, "executor") {
                return Err(refused(format!(
                    "{handle} executes {slug} and cannot review its own work"
                )));
            }
            let mut change = Change::default();
            let holding = view.holds(handle, role);
            // One orchestrator a task: a holder still there refuses, one lost or left is dropped.
            let mut took = Vec::new();
            let single = role == "orchestrator";
            let others = view.holders(role).filter(|r| single && r.handle != handle);
            for other in others {
                let health = view.health(&other.handle);
                if health.holds() {
                    if holding {
                        continue; // a double both still hold: settled by handover or release
                    }
                    return Err(refused(format!(
                        "{slug} is orchestrated by {} ({}); {} hands it over with: just tasks handover {slug} {handle}",
                        other.handle,
                        health.as_str(),
                        other.handle
                    )));
                }
                change.saw(view, &other.handle);
                change.drop(&other.handle, role);
                took.push((other.handle.clone(), health.as_str()));
            }
            if !holding {
                change.add = Some(RoleRow {
                    slug: slug.into(),
                    handle: handle.into(),
                    role: role.into(),
                    since: view.now,
                    expires: None,
                });
                change.say = Say::system(format!("{handle} joined as {role}"));
            }
            if !took.is_empty() {
                change.say = Say::system(orchestration::took_line(handle, &took));
            }
            Ok(change)
        })
    }

    /// Move the orchestrator role on `slug` from `from`, who holds it, to `to`, in one write: the
    /// thread names the new orchestrator, and both hear one ROLE event.
    pub fn hand_over(&self, slug: &str, from: &str, to: &str) -> Result<Task, Error> {
        self.rule(slug, from, |view| {
            view.actor(from)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!("{slug} is {}", task.status)));
            }
            if !view.holds(from, "orchestrator") {
                return Err(refused(format!("{from} does not orchestrate {slug}")));
            }
            if to == from {
                return Err(refused(format!("{from} already orchestrates {slug}")));
            }
            if view.session(to).is_none() {
                return Err(refused(format!(
                    "no session {} to hand {slug} to",
                    repr(to)
                )));
            }
            // Only to a session still there, or the task would be nobody's. Leaving between this
            // read and the write is refused by the write itself (`WRITE`), then decided again.
            let health = view.health(to);
            if !health.holds() {
                return Err(refused(format!(
                    "{to} is {}; hand {slug} to a session still here",
                    health.as_str()
                )));
            }
            let mut change = Change::default();
            change.drop(from, "orchestrator");
            if !view.holds(to, "orchestrator") {
                change.add = Some(RoleRow {
                    slug: slug.into(),
                    handle: to.into(),
                    role: "orchestrator".into(),
                    since: view.now,
                    expires: None,
                });
            }
            change.say = Say::system(orchestration::handed_line(from, to));
            Ok(change)
        })
    }

    /// Let go of `role` on a task, or of every role it holds there. Work in progress with no executor
    /// left is ready again.
    pub fn release(&self, slug: &str, handle: &str, role: Option<&str>) -> Result<Task, Error> {
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            let mine = |r: &&RoleRow| r.handle == handle && role.is_none_or(|role| r.role == role);
            let mut change = Change::default();
            for held in view.snap.roles.iter().filter(mine) {
                change.drop(&held.handle, &held.role);
            }
            if change.drop.is_empty() {
                return Err(refused(format!(
                    "{handle} holds no {} on {slug}",
                    role.unwrap_or("role")
                )));
            }
            let executing = view.holders("executor").any(|r| !mine(&r));
            if task.status == "in_progress" && !executing {
                change.status = Some("ready".into());
            }
            change.say = Say::system(format!(
                "{handle} released {} (released)",
                role.unwrap_or("every role")
            ));
            Ok(change)
        })
    }
}
