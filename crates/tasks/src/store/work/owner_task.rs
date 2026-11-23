//! Task creation and its optional immutable owner-save receipt share one transaction.

use super::red::{filing, named};
use super::{
    BEFORE_WORK, Error, NewTask, OWNER, REFUSED, Store, Task, no_task, plain_slug, refused, repr,
    sql,
};
use crate::store::postgres::{Pg, work as pg};
use crate::store::statement::both;
use serde_json::{Value, json};
use std::fmt::Write as _;

impl Store {
    /// A new task, on the record. `assignee`: a session reserved without a lapse.
    pub fn create(&self, new: &NewTask) -> Result<Task, Error> {
        self.create_with_receipt(new, None, None, true)
    }

    /// [`Store::create`], the task filed on `arm` ([`crate::arm`]): the label is written with the
    /// task, in its one transaction, and no write changes it after.
    pub fn create_in_arm(&self, new: &NewTask, arm: &str) -> Result<Task, Error> {
        let arm = crate::arm::named(arm).map_err(refused)?;
        self.create_with_receipt(new, None, Some(arm), true)
    }

    /// The slugs of the owner's tasks saved under `receipt` ([`Store::create_owner_task`]), and of
    /// the person's this store writes for ([`Store::for_person`]), two at most: none when the save
    /// never reached the board, one when it did, and two when the receipt does not say which task
    /// it made.
    pub fn owner_saved(&self, receipt: &str) -> Result<Vec<String>, Error> {
        let read = || both(sql::OWNER_SAVED, pg::OWNER_SAVED);
        let person = self.person.as_deref().filter(|name| self.is_person(name));
        let vars = json!({"receipt": receipt, "by_person": person});
        self.retrying(|| self.read(read(), vars.clone()))
    }

    /// Create an owner's task with an opaque save receipt in the task's private fields: one the
    /// owner files, or a person this store writes for ([`Store::is_person`]).
    /// The receipt, task and creation message commit together; edits retain the receipt. Then the
    /// board gives it an orchestrator ([`Store::give_filed`]): a give that fails is said on stderr
    /// and fails nothing, the task saved, and the board's next read of the whole board gives it
    /// ([`Store::give_if_due`]).
    pub fn create_owner_task(&self, new: &NewTask, receipt: &str) -> Result<Task, Error> {
        let made = self.owner_task(new, receipt, true)?;
        match self.give_filed(&made.slug, OWNER, None) {
            Ok(_) => Ok(self.get(&made.slug).unwrap_or(made)),
            Err(error) => {
                eprintln!("{}: no orchestrator was given it yet: {error}", made.slug);
                Ok(made)
            }
        }
    }

    /// Commit the owner's receipt, task and creation message without waiting for orchestration
    /// or callback delivery. A server clock must dispatch this board's callbacks.
    /// The durable unowned task is picked up by [`Store::give_if_due`] on the next whole-board
    /// read, including after a restart. Call [`Store::create_owner_task`] to give it immediately.
    pub fn persist_owner_task(&self, new: &NewTask, receipt: &str) -> Result<Task, Error> {
        self.owner_task(new, receipt, false)
    }

    fn owner_task(&self, new: &NewTask, receipt: &str, deliver: bool) -> Result<Task, Error> {
        if !self.is_person(&new.by) || receipt.is_empty() {
            return Err(refused(format!(
                "owner task receipt {} requires actor owner and a nonempty receipt; actor is {}",
                repr(receipt),
                repr(&new.by)
            )));
        }
        self.create_with_receipt(new, Some(receipt), None, deliver)
    }

    fn create_with_receipt(
        &self,
        new: &NewTask,
        receipt: Option<&str>,
        arm: Option<&str>,
        deliver: bool,
    ) -> Result<Task, Error> {
        if !plain_slug(&new.slug) {
            return Err(refused(format!(
                "slug {} must look like area/short-name (lowercase, digits, dashes)",
                repr(&new.slug)
            )));
        }
        let title = new.title.trim();
        if title.is_empty() {
            return Err(refused("a task needs a title".into()));
        }
        if !BEFORE_WORK.contains(&new.status.as_str()) {
            return Err(refused(format!(
                "a new task starts as {}, not {}",
                BEFORE_WORK.join(", "),
                repr(&new.status)
            )));
        }
        if !(0..=4).contains(&new.priority) {
            return Err(refused("priority runs 0 (most urgent) to 4".into()));
        }
        let red = (new.red.as_deref()).map(|what| named(&new.slug, what));
        let red = red.transpose()?;
        // A red's filing names `red` unless it names a kind of its own, so a filer that names none,
        // or runs a build that takes none, files its reds of that kind ([`crate::kind`]).
        let kind = match new.kind.as_deref() {
            Some(kind) => Some(crate::kind::named(kind).map_err(refused)?),
            None => red.map(|_| crate::kind::RED),
        };
        let parent = new.parent.as_deref().filter(|p| !p.is_empty());
        if new.instruction && parent.is_none() {
            return Err(refused(
                "an instruction task is a subtask: it needs a parent".into(),
            ));
        }
        let needed: Vec<Value> = new
            .depends
            .iter()
            .map(String::as_str)
            .chain(parent)
            .map(|other| json!({"slug": other, "missing": format!("{REFUSED}{}", no_task(other))}))
            .collect();
        let links: Vec<Value> = new
            .links
            .iter()
            .map(|l| ("doc", l))
            .chain(new.depends.iter().map(|d| ("dep", d)))
            .map(|(kind, target)| json!({"kind": kind, "target": target}))
            .collect();
        let mut said = format!("{}{}", crate::stages::CREATED, new.by);
        if let Some(assignee) = &new.assignee {
            let _ = write!(said, " for {assignee}");
        }
        let file_as = |placing: Option<crate::workflow::Transition>| {
            let creating = sql::Creating {
                acts: !self.is_person(&new.by),
                receipt: receipt.is_some(),
                red: red.is_some(),
                arm: arm.is_some(),
                kind: kind.is_some(),
                placed: placing.is_some(),
            };
            let body = || both(sql::create(&creating), Pg::script(pg::create(&creating))).posting();
            let mut vars = self.acting(
                &new.by,
                (self.clock)(),
                json!({
                    "slug": new.slug, "title": title, "body": new.body, "status": new.status,
                    "priority": new.priority, "parent": parent, "instruction": new.instruction,
                    "needed": needed, "links": links, "assignee": new.assignee, "said": said,
                    "owner_save": receipt, "red": red, "arm": arm, "kind": kind,
                    "taken": format!("{REFUSED}task {} already exists", new.slug),
                    "unassignable": new.assignee.as_ref()
                        .map(|a| format!("{REFUSED}no session {} to assign it to", repr(a))),
                    "unplaced": format!("{REFUSED}{UNPLACED}"),
                    "placed": placing.as_ref().map(crate::workflow::Transition::position),
                    "placing": placing,
                }),
            );
            if red.is_some()
                && let (Some(vars), Value::Object(filer)) = (vars.as_object_mut(), filing(&new.by))
            {
                vars.extend(filer);
            }
            self.retrying(|| self.written_statement(body(), vars.clone(), &new.slug, deliver))
        };
        // Filed as the board's own in its one write; a board with a custom workflow active refuses
        // that in the same write, and the task is filed again at the workflow's initial pair.
        match file_as(None) {
            Err(Error::Refused(why)) if why == UNPLACED => file_as(self.placing(new)?),
            filed => filed,
        }
    }
}

/// The refusal of a filing written as the board's own while a custom workflow is active, which
/// files it again at the workflow's initial pair (`sql::UNPLACED`).
const UNPLACED: &str = "a custom workflow is active, which starts the task at its initial pair";
