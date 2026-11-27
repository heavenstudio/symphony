//! Which group a thing belongs to ([`crate::people::ownership`]): the board's ownership log read
//! whole, the map an admin keeps written a line at a time, and a hand-over of a task or a card
//! recorded with the person who made it, in the table an additive step of this contract defines
//! ([`crate::schema::OWNERSHIP`]). A board without the step reads as one with nothing mapped, so
//! every thing goes to its default, and a write there is refused before anything is sent.
//!
//! A write reads the log (and, for a hand-over, the board's people and the name written on the
//! thing), decides in the process and writes its line numbered after the newest it read, fenced on
//! it ([`pg::line`]) and on the people revision a hand-over decided on: a change of the map, of
//! the people or of the thing between the read and the write makes it read and decide again.

use super::postgres::ownership as pg;
use super::postgres::{Pg, work::said};
use super::statement::Statement;
use super::work::no_task;
use super::{Error, Failure, REFUSED, Store, Task, refused};
use crate::people::People;
use crate::people::ownership::{
    By, Did, Line, Map, Owning, Place, Thing, deciders, owning, refused_decide, refused_hand,
};
use crate::schema;
use serde_json::{Value, json};

/// The tables a write of the log names: the people revision a hand-over is fenced on, and the log.
const TABLES: [&str; 2] = ["people_log", "ownership"];

/// How many times a write reads and decides again after another passed it.
const TRIES: usize = 8;

/// The refusal a task's hand-over meets where what it decided on moved, which reads again.
const MOVED: &str = "the task, the map or the people moved under this hand-over";

/// The board field holding the name written on a task ([`crate::red::FIELDS`]`.`[`GROUP`]).
pub const GROUP: &str = "group";

impl Task {
    /// The name written on it, a group's or a person's ([`crate::people::ownership`]): what it was
    /// filed for (`tasks create --group`) or handed to. None where nothing is.
    pub fn group(&self) -> Option<&str> {
        (self.fields.get(crate::red::FIELDS))
            .and_then(|board| board.get(GROUP))
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
    }
}

impl Store {
    /// Every line of the ownership log, oldest first: none on a board without the step.
    pub fn ownership_log(&self) -> Result<Vec<Line>, Error> {
        self.retrying(|| {
            let read = self.request(Statement::from(pg::LOG), json!({})).map(Some);
            let Some(found) = self.absent_board(read, None)? else {
                return Ok(Vec::new());
            };
            let answer = found.last().cloned().unwrap_or(Value::Null);
            crate::objects::read(&answer)
                .map_err(|e| Failure::Stop(Error::Db(format!("the board's ownership log: {e}"))))
        })
    }

    /// The board's map of places to groups.
    pub fn ownership_map(&self) -> Result<Map, Error> {
        Ok(Map::of(&self.ownership_log()?))
    }

    /// Whose `thing` is, `written` the name written on it (a card's `*For:*`; a task's is read
    /// from its record by [`Store::task_owning`]): one read of the people and one of the map.
    pub fn owning(&self, thing: &Thing, written: Option<&str>) -> Result<Owning, Error> {
        let people = self.people()?;
        let map = self.ownership_map()?;
        Ok(owning(&people, &map, written, &thing.places()))
    }

    /// Whose the task `task` is.
    pub fn task_owning(&self, task: &Task) -> Result<Owning, Error> {
        self.owning(&Thing::Task(task.slug.clone()), task.group())
    }

    /// The people whose approve meets the `owner` stamp of the task `task` beside the person at the
    /// machine's: those of its owner who hold decide ([`deciders`]), read now, the map only on a
    /// board that names someone. None where the board's people or its map cannot be read, so an
    /// approve they would count is not counted, as [`Store::deciders`] reads none then.
    pub fn task_deciders(&self, task: &Task) -> Vec<String> {
        let read = || -> Result<Vec<String>, Error> {
            let people = self.people()?;
            if people.members.is_empty() {
                return Ok(Vec::new());
            }
            let map = self.ownership_map()?;
            let places = Thing::Task(task.slug.clone()).places();
            Ok(deciders(
                &people,
                &owning(&people, &map, task.group(), &places),
            ))
        };
        read().unwrap_or_default()
    }

    /// Why the person `name`, holding decide, may not decide the task `slug`: no member of its
    /// group, not the person it is written for, or a task this board maps no group to
    /// ([`refused_decide`]); none where they may, and none on a board that names nobody. Refused
    /// for a slug no task has.
    pub fn refused_task_decide(&self, slug: &str, name: &str) -> Result<Option<String>, Error> {
        let task = self.get(slug)?;
        let people = self.people()?;
        if people.members.is_empty() {
            return Ok(None);
        }
        let places = Thing::Task(task.slug.clone()).places();
        let owning = owning(&people, &self.ownership_map()?, task.group(), &places);
        Ok(refused_decide(&people, &owning, &places, name, false))
    }

    /// [`Store::task_deciders`] of the task `slug`: none for a slug no task has.
    pub fn task_deciders_of(&self, slug: &str) -> Vec<String> {
        (self.get(slug).ok())
            .map(|task| self.task_deciders(&task))
            .unwrap_or_default()
    }

    /// Maps `place` to the group `group` as `by`'s change: none, and nothing written, where it maps
    /// there already. Refused, with nothing written, where the board keeps no such group.
    pub fn map_place(&self, place: &Place, group: &str, by: &str) -> Result<Option<Line>, Error> {
        self.write_line(by, false, |people, map| {
            if people.group(group).is_none() {
                return Err(nothing(format!(
                    "this board keeps no group {group} to map {} to",
                    place.said()
                )));
            }
            if map.group(place) == Some(group) {
                return Ok(None);
            }
            Ok(Some(Line {
                did: Did::Map,
                place: Some(place.text()),
                name: Some(group.into()),
                ..blank()
            }))
        })
    }

    /// Takes `place` off the map as `by`'s change: its things go to the default. Refused, with
    /// nothing written, where the map does not name it.
    pub fn unmap_place(&self, place: &Place, by: &str) -> Result<Line, Error> {
        let line = self.write_line(by, false, |_, map| {
            if map.group(place).is_none() {
                return Err(nothing(format!(
                    "the map names no group for {}",
                    place.said()
                )));
            }
            Ok(Some(Line {
                did: Did::Unmap,
                place: Some(place.text()),
                ..blank()
            }))
        })?;
        Ok(line.expect("an unmap always changes the map"))
    }

    /// Records that `actor`, a person ([`Store::is_person`]; `owner`, or the name this store writes
    /// them under, the person at the machine: [`Store::is_machine`]), hands the card `thing`, written to `written` now, to `to`: the line, or none, and nothing
    /// written, where it is written to `to` already. The card's file is its caller's to write: the
    /// board keeps who did. Refused, with nothing written, as [`refused_hand`] refuses, and for an
    /// actor who is no person.
    pub fn hand_card(
        &self,
        thing: &Thing,
        written: Option<&str>,
        to: &str,
        actor: &str,
    ) -> Result<Option<Line>, Error> {
        self.handing(actor)?;
        let machine = self.is_machine(actor);
        self.write_line(actor, true, |people, map| {
            let owning = owning(people, map, written, &thing.places());
            decided_hand(people, &owning, thing, to, actor, machine)
        })
    }

    /// Hands the open task `slug` to `to` as `actor`'s, a person ([`Store::is_person`]; `owner`,
    /// or the name this store writes them under, the person at the machine: [`Store::is_machine`]):
    /// the name written on it, its line in the log and the board's line
    /// on its thread naming who handed it, whose it was and whose it is, in one write. None, and
    /// nothing written, where it is written to `to` already. Refused, with nothing written, as
    /// [`refused_hand`] refuses, for a task missing or closed, and for an actor who is no person.
    pub fn hand_task(&self, slug: &str, to: &str, actor: &str) -> Result<Option<Task>, Error> {
        self.handing(actor)?;
        let thing = Thing::Task(slug.to_string());
        let said_line = said(
            "(v ->> 'actor') || ' handed it to ' || (v ->> 'name') || ' (it was ' || \
             (v ->> 'from') || ')'",
        );
        let body = || {
            Statement::from(Pg::script(pg::hand_task(&said_line)))
                .naming(&TABLES)
                .posting()
        };
        for _ in 0..TRIES {
            let task = self.get(slug)?;
            let people = self.people()?;
            let map = self.ownership_map()?;
            let was = task.group().map(str::to_string);
            let owning = owning(&people, &map, was.as_deref(), &thing.places());
            let machine = self.is_machine(actor);
            let Some(mut line) = decided_hand(&people, &owning, &thing, to, actor, machine)? else {
                return Ok(None);
            };
            line.n = map.revision + 1;
            line.at = self.now();
            line.by = actor.into();
            let vars = self.acting(
                actor,
                line.at,
                json!({
                    "slug": slug, "name": to, "was": was, "line": line,
                    "from": format!("{}, {}", owning.name, owning.said()),
                    "expected": map.revision, "roster": people.revision,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                    "closed": format!("{REFUSED}{slug} is "),
                    "moved": format!("{REFUSED}{MOVED}"),
                }),
            );
            match self.retrying(|| self.written(body(), vars.clone(), slug)) {
                Err(Error::Refused(why)) if why == MOVED => continue,
                done => return done.map(Some),
            }
        }
        Err(refused(format!(
            "the task, the map or the people moved under this hand-over {TRIES} times: nothing \
             was written by it"
        )))
    }

    /// Refused where `actor` is no person, or the board lacks the step.
    fn handing(&self, actor: &str) -> Result<(), Error> {
        if !self.is_person(actor) {
            return Err(nothing(format!(
                "{actor} is no person: a thing is handed on by a member of its group, never by a \
                 session"
            )));
        }
        self.keeping_ownership()
    }

    /// Refused on a board that lacks the step its table comes with.
    fn keeping_ownership(&self) -> Result<(), Error> {
        if self.additions()? < schema::ownership_step() {
            return Err(refused(
                "the map of places is not on this board yet: its schema lacks the step that adds \
                 it, which the project's own build of the board adds at its next command: nothing \
                 was written"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Takes back the card's hand-over line `line`, which [`Store::hand_card`] wrote, where its
    /// caller could not write the card's `*For:*`: so the board never names a hand-over the card
    /// does not hold. Whether it was taken back: not where it is no longer the line written, or
    /// never was one. A line written after it keeps its number.
    pub fn take_back(&self, line: &Line) -> Result<bool, Error> {
        if line.did != Did::Hand {
            return Ok(false);
        }
        self.retrying(|| {
            self.transact_dispatch(
                Statement::from(pg::TAKEN_BACK).naming(&["ownership"]),
                json!({ "line": line }),
                false,
            )
        })
    }

    /// The line `decide` makes from the board's people and map, written as `by`'s at this store's
    /// clock, fenced on the newest line read and, where `fence_people`, on the people revision:
    /// none, and nothing written, where it decides on none.
    fn write_line(
        &self,
        by: &str,
        fence_people: bool,
        decide: impl Fn(&People, &Map) -> Result<Option<Line>, Error>,
    ) -> Result<Option<Line>, Error> {
        self.keeping_ownership()?;
        let by = by.trim();
        if by.is_empty() {
            return Err(nothing("a change of the map says who made it".into()));
        }
        for _ in 0..TRIES {
            let people = self.people()?;
            let map = self.ownership_map()?;
            let Some(mut line) = decide(&people, &map)? else {
                return Ok(None);
            };
            line.n = map.revision + 1;
            line.at = self.now();
            line.by = by.into();
            let vars = json!({
                "expected": map.revision,
                "roster": fence_people.then_some(people.revision),
                "line": line,
            });
            let wrote: bool = self.retrying(|| {
                self.transact_dispatch(
                    Statement::from(pg::line()).naming(&TABLES),
                    vars.clone(),
                    false,
                )
            })?;
            if wrote {
                return Ok(Some(line));
            }
        }
        Err(refused(format!(
            "the map changed under this change {TRIES} times: nothing was written by it"
        )))
    }
}

/// The hand-over of `thing`, whose `owning` says it is, to `to` by `actor` (`machine`: the person
/// at the machine, under `owner` or the name they gave themselves), decided: refused as
/// [`refused_hand`] refuses; none where it is written to `to` already; else its line, numbered and
/// dated by its writer.
fn decided_hand(
    people: &People,
    owning: &Owning,
    thing: &Thing,
    to: &str,
    actor: &str,
    machine: bool,
) -> Result<Option<Line>, Error> {
    if let Some(why) = refused_hand(people, owning, to, actor, machine) {
        return Err(nothing(why));
    }
    if owning.by == By::Named && owning.name == to {
        return Ok(None);
    }
    Ok(Some(Line {
        did: Did::Hand,
        thing: Some(thing.text()),
        name: Some(to.into()),
        from: Some(owning.name.clone()),
        ..blank()
    }))
}

/// A line with nothing said yet: its writer numbers, dates and signs it.
fn blank() -> Line {
    Line {
        n: 0,
        at: 0.0,
        by: String::new(),
        did: Did::Map,
        place: None,
        thing: None,
        name: None,
        from: None,
    }
}

/// A refusal that wrote nothing.
fn nothing(why: String) -> Error {
    refused(format!("{why}: nothing was written"))
}
