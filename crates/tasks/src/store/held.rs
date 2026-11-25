//! Words held for people ([`crate::held`]): a person's message held with the board's line that says
//! so, a held message passed on, and the reads of a task's held messages, in the table of
//! [`crate::schema::HELD`]; and beside them a person's comment on a document held, marked passed
//! once its words are in the file, and the reads of a document's. A board without the step holds
//! nothing: its reads find none, and a hold or a pass there is refused by the schema's guard
//! before anything is written.

use super::messages::{BOARD, Poster};
use super::postgres::held as pg;
use super::statement::Statement;
use super::work::{OWNER, ROLES};
use super::{Error, Failure, REFUSED, Store, refused, repr};
use crate::held::{Held, MARKER, MESSAGE, OnDocument, Place, Said, passed_text};
use crate::people::Power;
use serde_json::{Value, json};

impl Store {
    /// `sender`'s message on `slug`'s thread, which waits on an agent's answer: said as
    /// [`Store::say_asking_as`] says it where they may direct agents (the person at the machine
    /// always does, as `owner` or the name they gave themselves: [`Store::is_machine`]), and
    /// held for people ([`Store::hold_words`]) where they are a person a board named who may not.
    /// A line that reads as an approve ([`crate::stamps::read::reads_as_approve`]) is said, with
    /// the note in its words, by one who may decide the task, whether or not they may direct
    /// agents: it is their verdict, and the note beside a verdict is written with it.
    /// A person who may direct agents and may not decide the task is refused a line that would
    /// read as an approve ([`crate::stamps::read::reads_as_approve`]), with nothing written: one
    /// without decide, and one holding it who is not of the task's owner
    /// ([`crate::people::ownership::refused_decide`]). The `owner` stamp counts the approve of the
    /// task's deciders as they are when it is read ([`Store::task_deciders`]), so it would count
    /// once they were given decide, joined its group or a remap made it theirs. Their held words,
    /// passed on, read as the passer's first ([`crate::held::passed_text`]), never as an approve.
    pub fn say_or_hold_as(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Said, Error> {
        if self.is_machine(sender) || !self.is_person(sender) {
            return self.say_asking_as(slug, sender, text, to).map(Said::Line);
        }
        let powers = self.people()?.powers_of(sender);
        let directs = powers.holds(Power::DirectAgents);
        if crate::stamps::read::reads_as_approve(text) {
            let why = if powers.holds(Power::Decide) {
                self.refused_task_decide(slug, sender)?
            } else {
                Some(format!("{sender} may not decide on this board"))
            };
            match why {
                // Their approve, the note beside it among its words, is written with it: deciding
                // carries the right to say why (users-and-identity.md, "The note beside a verdict").
                None => return self.say_asking_as(slug, sender, text, to).map(Said::Line),
                Some(_) if !directs => {
                    return self.hold_words(slug, sender, text, to).map(Said::Held);
                }
                Some(why) => {
                    return Err(refused(format!(
                        "{why}, and a line of theirs that starts with approve reads as one: \
                         nothing was written"
                    )));
                }
            }
        }
        if !directs {
            return self.hold_words(slug, sender, text, to).map(Said::Held);
        }
        self.say_asking_as(slug, sender, text, to).map(Said::Line)
    }

    /// Whether `name` may direct the board's agents: the person at the machine
    /// ([`Store::is_machine`]), a session (an agent's own words are no person's), or a person the
    /// board gives the power to.
    pub fn may_direct_agents(&self, name: &str) -> Result<bool, Error> {
        if self.is_machine(name) || !self.is_person(name) {
            return Ok(true);
        }
        Ok(self.people()?.powers_of(name).holds(Power::DirectAgents))
    }

    /// `author`'s message `text` on `slug`'s thread, to `to` (a role, a session or nobody), held for
    /// people: the row, and the board's line to `owner` saying a message is held, whose number is
    /// the row's ([`crate::held`]). No agent is handed the words. Refused, with nothing written: an
    /// author who is not a person this store writes for ([`Store::for_person`]; the person at the
    /// machine's words are never held, [`Store::is_machine`]), an empty text, a task, role or session that is not there, and a board
    /// without the step.
    pub fn hold_words(
        &self,
        slug: &str,
        author: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Held, Error> {
        if self.is_machine(author) || !self.is_person(author) {
            return Err(refused(format!(
                "only a person's words are held, and {} writes as no person here: nothing was \
                 written",
                repr(author)
            )));
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(refused("an empty message says nothing".into()));
        }
        let mut vars = self.acting(
            author,
            self.now(),
            json!({
                "slug": slug, "sender": BOARD, "author": author, "text": text, "to": to,
                "marker": MARKER,
            }),
        );
        addressed(&mut vars, slug, to);
        let body = || Statement::from(pg::hold()).posting().naming(&pg::TABLES);
        self.retrying(|| self.transact(body(), vars.clone()))
    }

    /// The held message `id` on `slug`'s thread, passed on by `by`: the owner or the person this
    /// store writes for, or a session, which is the person at the machine's own agent. In one write
    /// its words go on the thread under its author's name, marked a person's, to whom they were
    /// written, with the passer first ([`passed_text`]), opening or joining the task's ask as a
    /// person's message does, and the row is marked passed with that line's number. The line then
    /// tells as a post's does: written to the task's executor where none reads it, the task's
    /// orchestrators are told so once. Refused, with nothing written: no such message held on the
    /// task, one passed already (naming who passed it), a session that is not on the board, and a
    /// board without the step. Whether `by` may direct agents is its caller's to have checked, as
    /// every power is (`apps/symphony-server/src/powers.rs`).
    pub fn pass(&self, slug: &str, id: i64, by: &str) -> Result<Held, Error> {
        let held = (self.held_on(slug)?.into_iter())
            .find(|held| held.id == id)
            .ok_or_else(|| none_held(slug, id))?;
        let text = passed_text(by, &held.text);
        let to = held.to.as_deref();
        let (task_about, task_words) = super::asks::on_thread_vars(slug, &text);
        let mut vars = self.acting(
            by,
            self.now(),
            json!({
                "slug": slug, "id": id, "by": by, "sender": held.author, "text": text, "to": to,
                "task_about": task_about, "task_words": task_words, "asking": true,
                "none_held": format!("{REFUSED}{}", none_held_text(slug, id)),
                "passed_already": format!(
                    "{REFUSED}message #{id} on {slug} was passed on already, by %s: nothing was \
                     written"
                ),
            }),
        );
        // The line is its author's, a person's whoever passes it.
        vars["by_person"] = held.author.clone().into();
        addressed(&mut vars, slug, to);
        let poster = if self.is_person(by) {
            Poster::Owner
        } else {
            Poster::Session
        };
        let body = || {
            Statement::from(pg::pass(poster))
                .posting()
                .naming(&pg::TABLES)
        };
        let passed: Held = self.retrying(|| self.transact(body(), vars.clone()))?;
        // Its line tells as a post's does: once, where no executor reads it.
        if let Some(line) = &passed.passed {
            self.tell_line_if_unread(slug, line.line, &passed.author, passed.to.as_deref());
        }
        Ok(passed)
    }

    /// The messages held on `slug`, passed on or not, oldest first: none on a board without the
    /// step. A person's read alone: no agent is handed what it answers.
    pub fn held_on(&self, slug: &str) -> Result<Vec<Held>, Error> {
        self.retrying(|| {
            let answer = self.request(Statement::from(pg::on_task()), json!({"slug": slug}));
            let answer = answer.map_err(Failure::from)?;
            let answer = answer.last().cloned().unwrap_or(Value::Null);
            crate::objects::read(&answer)
                .map_err(|e| Failure::Stop(Error::Db(format!("the held messages: {e}"))))
        })
    }

    /// How many messages on `slug` are held and not passed on: what an agent is told of them.
    pub fn held_count(&self, slug: &str) -> Result<u64, Error> {
        self.retrying(|| {
            let answer = self.request(Statement::from(pg::count()), json!({"slug": slug}));
            let answer = answer.map_err(Failure::from)?;
            let count = answer.last().and_then(Value::as_u64).unwrap_or(0);
            Ok(count)
        })
    }
}

/// The variables a line to `to` on `slug`'s thread is checked by, as a post's: a role it names
/// that is none, and a session's handle it names, refused if no session has it.
fn addressed(vars: &mut Value, slug: &str, to: Option<&str>) {
    let role = to.and_then(|to| to.strip_prefix('@'));
    if let Some(role) = role.filter(|role| !ROLES.contains(role)) {
        let roles: Vec<String> = ROLES.iter().map(|r| format!("@{r}")).collect();
        vars["no_role"] =
            format!("{REFUSED}no role @{role}; use one of {}", roles.join(", ")).into();
    }
    if let Some(handle) = to.filter(|to| *to != OWNER && role.is_none()) {
        vars["handle"] = handle.into();
        vars["nobody"] = format!("{REFUSED}no session {} to send to", repr(handle)).into();
    }
    vars["no_task"] = format!("{REFUSED}{}", super::work::no_task(slug)).into();
}

fn none_held_text(slug: &str, id: i64) -> String {
    format!("no message #{id} is held on {slug}: nothing was written")
}

/// The refusal of a pass of `id`, which no message held on `slug` has.
fn none_held(slug: &str, id: i64) -> Error {
    refused(none_held_text(slug, id))
}

impl Store {
    /// `author`'s comment `text` on a document, left at `place`, held for people
    /// ([`crate::held::OnDocument`]): its row alone, with no line on any thread and no ask, so no
    /// agent is handed it. Refused, with nothing written: an author who is not a person this store
    /// writes for (the person at the machine's words, as `owner` or under the name they gave
    /// themselves, are never held: [`Store::is_machine`]), an empty text, a place that names no
    /// document, a card's message with no card, a callout with no block, and a board without the
    /// step.
    pub fn hold_on_document(
        &self,
        author: &str,
        text: &str,
        place: &Place,
    ) -> Result<OnDocument, Error> {
        if self.is_machine(author) || !self.is_person(author) {
            return Err(refused(format!(
                "only a person's words are held, and {} writes as no person here: nothing was \
                 written",
                repr(author)
            )));
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(refused("an empty comment says nothing".into()));
        }
        if let Some(why) = misplaced(place) {
            return Err(refused(format!("{why}: nothing was written")));
        }
        let vars = self.acting(
            author,
            self.now(),
            json!({"author": author, "text": text, "document": place}),
        );
        let body = || Statement::from(pg::hold_on_document()).naming(&pg::TABLES);
        self.retrying(|| self.transact(body(), vars.clone()))
    }

    /// The held comment `id` marked passed on by `by`, the owner or the person this store writes for,
    /// once the Symphony's save wrote its words into the document's file: its row as it now stands.
    /// Refused, with nothing written: no comment held by that number, one passed already (naming
    /// who passed it), and a board without the step. Whether `by` may direct agents is its caller's
    /// to have checked, as every power is (`apps/symphony-server/src/powers.rs`).
    pub fn pass_on_document(&self, id: i64, by: &str) -> Result<OnDocument, Error> {
        let vars = self.acting(
            by,
            self.now(),
            json!({
                "id": id, "by": by,
                "none_held": format!("{REFUSED}{}", none_on_document_text(id)),
                "passed_already": format!(
                    "{REFUSED}comment #{id} was passed on already, by %s: nothing was written"
                ),
            }),
        );
        let body = || Statement::from(pg::pass_on_document()).naming(&pg::TABLES);
        self.retrying(|| self.transact(body(), vars.clone()))
    }

    /// The comments held on the document `path` of the checkout `scope` (none: the project's own),
    /// passed on or not, oldest first: none on a board without the step. A person's read alone.
    pub fn held_on_document(
        &self,
        path: &str,
        scope: Option<&str>,
    ) -> Result<Vec<OnDocument>, Error> {
        self.held_documents(pg::on_document(), json!({"path": path, "scope": scope}))
    }

    /// The held comment `id`, passed on or not: none where no comment is held by that number.
    pub fn held_comment(&self, id: i64) -> Result<Option<OnDocument>, Error> {
        Ok(self.held_documents(pg::comment(), json!({"id": id}))?.pop())
    }

    /// The comments held on documents and not passed on, oldest first: what is told of them.
    pub fn held_on_documents(&self) -> Result<Vec<OnDocument>, Error> {
        self.held_documents(pg::unpassed(), json!({}))
    }

    /// The held comments the read `statement` answers with `vars`.
    fn held_documents(
        &self,
        statement: super::postgres::Pg<'static>,
        vars: Value,
    ) -> Result<Vec<OnDocument>, Error> {
        self.retrying(|| {
            let answer = self.request(Statement::from(statement.clone()), vars.clone());
            let answer = answer.map_err(Failure::from)?;
            let answer = answer.last().cloned().unwrap_or(Value::Null);
            crate::objects::read(&answer)
                .map_err(|e| Failure::Stop(Error::Db(format!("the held comments: {e}"))))
        })
    }
}

/// Why `place` is no place a held comment can be left: none where it is one.
fn misplaced(place: &Place) -> Option<&'static str> {
    if place.path.trim().is_empty() {
        return Some("a held comment names its document");
    }
    match (place.kind.as_str(), &place.card, &place.block) {
        (MESSAGE, Some(card), None) if !card.trim().is_empty() => None,
        (MESSAGE, ..) => Some("a held message names its card and no block"),
        (_, None, Some(_)) => None,
        _ => Some("a held callout names the block it was left after and no card"),
    }
}

fn none_on_document_text(id: i64) -> String {
    format!("no comment #{id} is held on a document: nothing was written")
}
