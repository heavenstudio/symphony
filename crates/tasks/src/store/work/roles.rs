//! Roles on a task beside claim and reserve: joining as orchestrator or reviewer, handing the
//! orchestrator role over (one orchestrator a task, [`crate::orchestration`]), and letting a
//! role go.

use crate::orchestration;
use crate::rows::RoleRow;
use crate::stages::{EVERY_ROLE, JOINED, RELEASED};
use crate::store::{Change, Error, Held, Opens, Say, Store, Task, refused, repr};

impl Store {
    /// The orchestrator role on `slug`, a task `handle` just filed, when `handle` is an
    /// orchestrator's session and nobody orchestrates the task, on it or above it
    /// ([`orchestration::Owners::of`]): whether it took it ([`Store::take_role`]). A task an
    /// orchestrator files is its own, so the owner's word on it reaches that one, as on any task it
    /// orchestrates (`Store::inbox`); one filed under a task an orchestrator holds is that one's
    /// already. A filer with no session takes none. `tasks create` takes it but under `--no-role`;
    /// a refusal of the role (a cap, [`Store::take_role`]) leaves the task filed.
    pub fn orchestrate_filed(&self, slug: &str, handle: &str) -> Result<bool, Error> {
        if handle == super::OWNER {
            return Ok(false);
        }
        match self.session(handle) {
            Ok(session) if session.row.kind == "orchestrator" => {}
            Ok(_) | Err(Error::Refused(_)) => return Ok(false),
            Err(error) => return Err(error),
        }
        let mut chain = vec![slug.to_owned()];
        chain.extend(self.ancestors(slug)?);
        let tasks = self.tasks_by_slug(&chain)?;
        if !orchestration::Owners::new(&tasks).of(slug).is_empty() {
            return Ok(false);
        }
        self.take_role(slug, handle, "orchestrator")?;
        Ok(true)
    }

    /// Join a task as `orchestrator` or `reviewer` (executor is `claim`, reserved is `reserve`).
    /// Taking the orchestrator role makes the open work it takes on the taker's: refused when that
    /// raises the taker's open tasks past the store's cap ([`crate::slots`]). The reviewer's role
    /// is refused to its executor, and to a session that requested the work, one that orchestrates
    /// the task or did or that filed it, but on a fast fix ([`Held::requested`]): its verdict is
    /// none the board takes ([`Store::verdict`]), so it is told at once who gives it instead.
    pub fn take_role(&self, slug: &str, handle: &str, role: &str) -> Result<Task, Error> {
        if role != "orchestrator" && role != "reviewer" {
            return Err(refused(format!(
                "take_role is for orchestrator or reviewer; use claim or reserve for {}",
                repr(role)
            )));
        }
        let starts = role == "orchestrator";
        // The board the commands a refusal names reach: this one.
        let board = self.board_command();
        // Who orchestrated the task, read from its thread as the verdict reads it.
        let held = if role == "reviewer" {
            self.ever_held(slug)?
        } else {
            Held::default()
        };
        self.ruling(slug, handle, starts, |view| {
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
            if role == "reviewer" {
                let fast = crate::fast_fix::marked(&task.fields).is_some();
                let orchestrates = view.holds(handle, "orchestrator");
                if let Some(why) = held.requested(handle, &task.created_by, orchestrates, fast) {
                    return Err(refused(super::unfresh_refusal(
                        &board, slug, handle, why, true,
                    )));
                }
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
                    // A hand-over is capped as this take is: it is no way round the cap.
                    let capped = if self.slots().is_some() {
                        format!(", which {handle}'s cap decides as it does a take")
                    } else {
                        String::new()
                    };
                    return Err(refused(format!(
                        "{slug} is orchestrated by {} ({}); {} hands it over with: {board} handover {slug} {handle}{capped}",
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
                change.say = Say::system(format!("{handle}{JOINED}{role}"));
                // The open work it takes on, the task's and the work below it nobody else
                // orchestrates: the new orchestrator's slots.
                if single {
                    change.opens = Some(Opens {
                        orchestrator: Some(handle.into()),
                        what: format!(
                            "{handle} cannot take orchestrator on {slug}, which is {}",
                            task.status
                        ),
                        ..Opens::default()
                    });
                }
            }
            if !took.is_empty() {
                change.say = Say::system(orchestration::took_line(handle, &took));
            }
            Ok(change)
        })
    }

    /// Move the orchestrator role on `slug` from `from`, who holds it, to `to`, in one write: the
    /// thread names the new orchestrator, and `to` hears one ROLE event (`from` made the move, and is
    /// not told its own write). The open work it hands on,
    /// the task's and the work below it nobody else orchestrates, takes `to`'s slots: refused, as
    /// [`Store::take_role`] is, when that raises `to`'s open tasks past the cap. The owner's is not.
    pub fn hand_over(&self, slug: &str, from: &str, to: &str) -> Result<Task, Error> {
        let capped = from != super::OWNER;
        let board = self.board_command();
        self.ruling(slug, from, capped, |view| {
            view.actor(from)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!("{slug} is {}", task.status)));
            }
            // Refused with who hands it over instead, or how to take the role first.
            if !view.holds(from, "orchestrator") {
                let not = format!("{from} does not orchestrate {slug}");
                let holder = view.holders("orchestrator").next();
                return Err(refused(match holder.map(|r| (&r.handle, view.health(&r.handle))) {
                    Some((other, _)) if other == to => format!("{not}; {to} already does"),
                    Some((other, health)) if health.holds() => format!(
                        "{not}; {other} does, and hands it over with: {board} handover {slug} {to}"
                    ),
                    Some((other, health)) => format!(
                        "{not}; {other}, who did, is {}: take the role first: {board} role {slug} \
                         orchestrator",
                        health.as_str()
                    ),
                    None => format!(
                        "{not}, and nobody does: take the role first: {board} role {slug} \
                         orchestrator"
                    ),
                }));
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
            change.opens = capped.then(|| Opens {
                orchestrator: Some(to.into()),
                what: format!(
                    "{from} cannot hand {slug}, which is {}, to {to}",
                    task.status
                ),
                ..Opens::default()
            });
            Ok(change)
        })
    }

    /// Let go of `role` on a task, or of every role it holds there. Work in progress with no executor
    /// left is ready again. Letting go of the orchestrator role is refused while it would leave open
    /// work with no orchestrator, the task's or the work below it the role keeps ([`orchestration::orphaned`]),
    /// unless an orchestrator holds a task above it: the refusal names the hand-over to the live
    /// orchestrator rule 4 picks ([`orchestration::Pool`]), or says none is live to take it.
    pub fn release(&self, slug: &str, handle: &str, role: Option<&str>) -> Result<Task, Error> {
        // Only a holder of the role reads the board for it; a task not found is the rule's to refuse.
        if role.is_none_or(|role| role == "orchestrator") {
            let orchestrating = match self.get(slug) {
                Ok(task) => {
                    (task.roles.iter()).any(|r| r.handle == handle && r.role == "orchestrator")
                }
                Err(Error::Refused(_)) => false,
                Err(error) => return Err(error),
            };
            if orchestrating {
                self.keeps_orchestrated(slug, handle, role.is_none())?;
            }
        }
        let board = self.board_command();
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            let mine = |r: &&RoleRow| r.handle == handle && role.is_none_or(|role| r.role == role);
            let mut change = Change::default();
            let letting_go = view.snap.roles.iter().filter(mine);
            for held in letting_go {
                change.drop(&held.handle, &held.role);
            }
            // Refused with the roles it holds there instead, and how to let them go.
            if change.drop.is_empty() {
                let none = format!("{handle} holds no {} on {slug}", role.unwrap_or("role"));
                let held: Vec<&str> = view
                    .snap
                    .roles
                    .iter()
                    .filter(|r| r.handle == handle)
                    .map(|r| r.role.as_str())
                    .collect();
                return Err(refused(match held.as_slice() {
                    [] => none,
                    [one] => format!("{none}, but {one}: {board} release {slug} --role {one}"),
                    [rest @ .., last] => format!(
                        "{none}, but {} and {last}: {board} release {slug} --role ROLE lets one \
                         go, {board} release {slug} every one",
                        rest.join(", ")
                    ),
                }));
            }
            let executing = view.holders("executor").any(|r| !mine(&r));
            if task.status == "in_progress" && !executing {
                change.status = Some("ready".into());
            }
            change.say = Say::system(format!(
                "{handle}{RELEASED}{} (released)",
                role.unwrap_or(EVERY_ROLE)
            ));
            Ok(change)
        })
    }

    /// Refused when `handle` letting go of its orchestrator role on `slug` would leave open work
    /// with no orchestrator ([`Store::release`]). `every`: the release lets every role go, so the
    /// refusal says how to let another go alone.
    fn keeps_orchestrated(&self, slug: &str, handle: &str, every: bool) -> Result<(), Error> {
        let board = self.board()?;
        let holds = |t: &Task, r: &crate::Role| {
            t.slug == slug && r.handle == handle && r.role == "orchestrator"
        };
        let orphans = orchestration::orphaned(&board.tasks, holds);
        if orphans.is_empty() {
            return Ok(());
        }
        let named: Vec<&str> = orphans.iter().map(|t| t.slug.as_str()).collect();
        let command = self.board_command();
        let cannot = format!(
            "{handle} cannot let go of orchestrator on {slug}: {} would have no orchestrator",
            named.join(", ")
        );
        let mut refusal = match orchestration::Pool::of(&board, &[handle]).pick() {
            Some(to) => format!("{cannot}; hand it over instead: {command} handover {slug} {to}"),
            None => format!(
                "{cannot}, and no other orchestrator is live to take it: it stays {handle}'s \
                 (`{command} leave` leaves it waiting for the next orchestrator)"
            ),
        };
        let task = board.tasks.iter().find(|t| t.slug == slug);
        let roles = task.into_iter().flat_map(|t| t.roles.iter());
        let other = roles.filter(|r| r.handle == handle && r.role != "orchestrator");
        if let Some(other) = other.map(|r| r.role.as_str()).next().filter(|_| every) {
            refusal.push_str(&format!(
                "; {other} goes alone with: {command} release {slug} --role {other}"
            ));
        }
        Err(refused(refusal))
    }
}
