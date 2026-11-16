//! Task creation and its optional immutable owner-save receipt share one transaction.

use super::red::{filing, named};
use super::{
    BEFORE_WORK, Error, NewTask, OWNER, REFUSED, Store, Task, no_task, plain_slug, refused, repr,
    sql,
};
use serde_json::{Value, json};
use std::fmt::Write as _;

impl Store {
    /// A new task, on the record. `assignee`: a session reserved without a lapse.
    pub fn create(&self, new: &NewTask) -> Result<Task, Error> {
        self.create_with_receipt(new, None, None)
    }

    /// [`Store::create`], the task filed on `arm` ([`crate::arm`]): the label is written with the
    /// task, in its one transaction, and no write changes it after.
    pub fn create_in_arm(&self, new: &NewTask, arm: &str) -> Result<Task, Error> {
        let arm = crate::arm::named(arm).map_err(refused)?;
        self.create_with_receipt(new, None, Some(arm))
    }

    /// The slugs of the owner's tasks saved under `receipt` ([`Store::create_owner_task`]), two
    /// at most: none when the save never reached the board, one when it did, and two when the
    /// receipt does not say which task it made.
    pub fn owner_saved(&self, receipt: &str) -> Result<Vec<String>, Error> {
        self.retrying(|| self.read(sql::OWNER_SAVED, json!({"receipt": receipt})))
    }

    /// Create an owner's task with an opaque save receipt in the task's private fields.
    /// The receipt, task and creation message commit together; edits retain the receipt. Then the
    /// board gives it an orchestrator ([`Store::give_filed`]): a give that fails is said on stderr
    /// and fails nothing, the task saved, and the board's next read of the whole board gives it
    /// ([`Store::give_if_due`]).
    pub fn create_owner_task(&self, new: &NewTask, receipt: &str) -> Result<Task, Error> {
        if new.by != OWNER || receipt.is_empty() {
            return Err(refused(format!(
                "owner task receipt {} requires actor owner and a nonempty receipt; actor is {}",
                repr(receipt),
                repr(&new.by)
            )));
        }
        let made = self.create_with_receipt(new, Some(receipt), None)?;
        match self.give_filed(&made.slug, OWNER, None) {
            Ok(_) => Ok(self.get(&made.slug).unwrap_or(made)),
            Err(error) => {
                eprintln!("{}: no orchestrator was given it yet: {error}", made.slug);
                Ok(made)
            }
        }
    }

    fn create_with_receipt(
        &self,
        new: &NewTask,
        receipt: Option<&str>,
        arm: Option<&str>,
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
        let fast_fix = match new.fast_fix.as_deref().map(str::trim) {
            Some("") => {
                return Err(refused(
                    "a fast fix records why it needs no reviewer: its risk, in words".into(),
                ));
            }
            fast_fix => fast_fix,
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
        let body = sql::create(&sql::Creating {
            acts: new.by != OWNER,
            receipt: receipt.is_some(),
            red: red.is_some(),
            fast_fix: fast_fix.is_some(),
            arm: arm.is_some(),
        });
        let mut vars = self.acting(
            &new.by,
            (self.clock)(),
            json!({
                "slug": new.slug, "title": title, "body": new.body, "status": new.status,
                "priority": new.priority, "parent": parent, "instruction": new.instruction,
                "needed": needed, "links": links, "assignee": new.assignee, "said": said,
                "owner_save": receipt, "red": red, "fast_fix": fast_fix, "arm": arm,
                "taken": format!("{REFUSED}task {} already exists", new.slug),
                "unassignable": new.assignee.as_ref()
                    .map(|a| format!("{REFUSED}no session {} to assign it to", repr(a))),
            }),
        );
        if red.is_some()
            && let (Some(vars), Value::Object(filer)) = (vars.as_object_mut(), filing(&new.by))
        {
            vars.extend(filer);
        }
        self.retrying(|| self.written(&body, vars.clone(), &new.slug))
    }
}
