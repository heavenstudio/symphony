//! Who is on a board ([`crate::people`]): its people and groups read whole in one statement, and
//! each change written with its line of the log, in the tables an additive step of this contract
//! defines ([`crate::schema::PEOPLE`]). A board without the step reads as one with nobody on it,
//! and a change there is refused before anything is sent.
//!
//! A change reads the board's people, decides in the process and writes the rows it changes whole
//! with its line, numbered after the newest it read and fenced on it ([`pg::change`]): every change
//! of a board's people meets every other on that one number, so none passes another, a group is
//! never removed under a person joining it, and the one that lost reads and decides again. A
//! change that would leave the board as it is writes nothing.

use super::postgres::people as pg;
use super::statement::Statement;
use super::{Error, Failure, Store, refused};
use crate::people::{Change, Did, Group, Holder, Member, People, Power, refused_name};
use crate::schema;
use crate::workflow::Host;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use symphony_plugin::execution::Principal;

/// The tables the step defines, each a change names.
pub const TABLES: [&str; 3] = ["people", "people_group", "people_log"];

/// How many times a change reads and decides again after another change passed it.
const TRIES: usize = 8;

/// What a change decided: its line's words, and the rows it writes.
#[derive(Default)]
struct Decided {
    did: Option<Did>,
    name: String,
    group: Option<String>,
    powers: Vec<Power>,
    members: Vec<Member>,
    members_gone: Vec<String>,
    groups: Vec<Group>,
    groups_gone: Vec<String>,
}

/// The people read: the statement's answer.
#[derive(Deserialize)]
struct Read {
    members: Vec<Member>,
    groups: Vec<Group>,
    revision: i64,
}

/// The board's people as a read of them answers (`postgres::people::people_object`).
pub(super) fn answered(answer: &Value) -> Result<People, String> {
    let read: Read = crate::objects::read(answer).map_err(|e| e.to_string())?;
    Ok(People {
        members: read.members,
        groups: read.groups,
        revision: read.revision,
    })
}

impl Store {
    /// Who is on the board: one read, which finds nobody on a board without the step.
    pub fn people(&self) -> Result<People, Error> {
        self.retrying(|| {
            let read = self
                .request(Statement::from(pg::PEOPLE), json!({}))
                .map(Some);
            let Some(found) = self.absent_board(read, None)? else {
                return Ok(People::default());
            };
            let answer = found.last().cloned().unwrap_or(Value::Null);
            answered(&answer)
                .map_err(|e| Failure::Stop(Error::Db(format!("the board's people: {e}"))))
        })
    }

    /// Every change made to the board's people, oldest first: none on a board without the step.
    pub fn people_log(&self) -> Result<Vec<Change>, Error> {
        self.retrying(|| {
            let read = self.request(Statement::from(pg::LOG), json!({})).map(Some);
            let Some(found) = self.absent_board(read, None)? else {
                return Ok(Vec::new());
            };
            let answer = found.last().cloned().unwrap_or(Value::Null);
            crate::objects::read(&answer)
                .map_err(|e| Failure::Stop(Error::Db(format!("the board's people log: {e}"))))
        })
    }

    /// Puts the person `name` on the board, in `groups` and with `powers`, as `by`'s change.
    /// Refused, with nothing written: a name [`refused_name`] refuses (no short name, a person's
    /// or a group's here already, a session's handle), a group the board does not keep, and a
    /// power no one is given yet ([`given`]).
    pub fn add_person(
        &self,
        name: &str,
        groups: &[String],
        powers: &[Power],
        by: &str,
    ) -> Result<Change, Error> {
        given(powers)?;
        let changed = self.change_people(by, |people| {
            let handles = self.session_handles()?;
            if let Some(why) = refused_name(name, people, &handles) {
                return Err(nothing(why));
            }
            for group in groups {
                kept_group(people, group)?;
            }
            let member = Member {
                name: name.into(),
                groups: in_order(groups.iter().cloned()),
                powers: said_order(powers.iter().copied()),
                revision: 0,
            };
            Ok(Some(Decided {
                did: Some(Did::Add),
                name: name.into(),
                group: None,
                powers: member.powers.clone(),
                members: vec![member],
                ..Decided::default()
            }))
        })?;
        Ok(changed.expect("an add always changes the board"))
    }

    /// Takes the person `name` off the board as `by`'s change: their row goes, and the lines they
    /// wrote stay theirs. Refused where the board holds no person by that name.
    pub fn remove_person(&self, name: &str, by: &str) -> Result<Change, Error> {
        let changed = self.change_people(by, |people| {
            kept_member(people, name)?;
            Ok(Some(Decided {
                did: Some(Did::Remove),
                name: name.into(),
                members_gone: vec![name.into()],
                ..Decided::default()
            }))
        })?;
        Ok(changed.expect("a removal always changes the board"))
    }

    /// Puts the person `name` in `group` as `by`'s change: none, and nothing written, where they
    /// are in it already. Refused where the board keeps no such person or group.
    pub fn join_group(&self, name: &str, group: &str, by: &str) -> Result<Option<Change>, Error> {
        self.change_people(by, |people| {
            let mut member = kept_member(people, name)?.clone();
            kept_group(people, group)?;
            if member.groups.iter().any(|held| held == group) {
                return Ok(None);
            }
            member.groups = in_order(member.groups.into_iter().chain([group.to_string()]));
            Ok(Some(Decided {
                did: Some(Did::Join),
                name: name.into(),
                group: Some(group.into()),
                members: vec![member],
                ..Decided::default()
            }))
        })
    }

    /// Takes the person `name` out of `group` as `by`'s change. Refused where they are not in it.
    pub fn leave_group(&self, name: &str, group: &str, by: &str) -> Result<Change, Error> {
        let changed = self.change_people(by, |people| {
            let mut member = kept_member(people, name)?.clone();
            if !member.groups.iter().any(|held| held == group) {
                return Err(nothing(format!(
                    "{name} is in no group {group} on this board"
                )));
            }
            member.groups.retain(|held| held != group);
            Ok(Some(Decided {
                did: Some(Did::Leave),
                name: name.into(),
                group: Some(group.into()),
                members: vec![member],
                ..Decided::default()
            }))
        })?;
        Ok(changed.expect("a leave always changes the board"))
    }

    /// Gives `holder` `powers` as `by`'s change, the line naming those it did not hold: none, and
    /// nothing written, where it held every one. Refused where the board keeps no such holder, no
    /// power is named, or one no one is given yet is ([`given`]).
    pub fn grant(
        &self,
        holder: Holder<'_>,
        powers: &[Power],
        by: &str,
    ) -> Result<Option<Change>, Error> {
        named_powers(powers)?;
        self.change_people(by, |people| {
            let held = own_powers(people, holder)?;
            let added: Vec<Power> = said_order(powers.iter().copied())
                .into_iter()
                .filter(|power| !held.contains(power))
                .collect();
            if added.is_empty() {
                return Ok(None);
            }
            let after = said_order(held.iter().copied().chain(added.iter().copied()));
            Ok(Some(rewritten(people, holder, Did::Grant, added, after)))
        })
    }

    /// Takes `powers` from `holder` as `by`'s change, the line naming those it held. Refused, with
    /// nothing written, where it held none of them as its own (a power a person holds by a group
    /// is the group's to lose), where the board keeps no such holder, or no power is named.
    pub fn revoke(&self, holder: Holder<'_>, powers: &[Power], by: &str) -> Result<Change, Error> {
        named_powers(powers)?;
        let changed = self.change_people(by, |people| {
            let held = own_powers(people, holder)?;
            let taken: Vec<Power> = said_order(powers.iter().copied())
                .into_iter()
                .filter(|power| held.contains(power))
                .collect();
            if taken.is_empty() {
                let (kind, name) = said(holder);
                return Err(nothing(format!(
                    "the {kind} {name} holds none of those powers as its own"
                )));
            }
            let after: Vec<Power> = (held.iter().copied())
                .filter(|power| !taken.contains(power))
                .collect();
            Ok(Some(rewritten(people, holder, Did::Revoke, taken, after)))
        })?;
        Ok(changed.expect("a revoke always changes the board"))
    }

    /// Makes the group `name`, with `powers`, as `by`'s change. Refused, with nothing written,
    /// where [`refused_name`] refuses the name (people and groups share one name space), or a
    /// power no one is given yet is named ([`given`]).
    pub fn add_group(&self, name: &str, powers: &[Power], by: &str) -> Result<Change, Error> {
        given(powers)?;
        let changed = self.change_people(by, |people| {
            let handles = self.session_handles()?;
            if let Some(why) = refused_name(name, people, &handles) {
                return Err(nothing(why));
            }
            let group = Group {
                name: name.into(),
                powers: said_order(powers.iter().copied()),
                revision: 0,
            };
            Ok(Some(Decided {
                did: Some(Did::GroupAdd),
                name: name.into(),
                powers: group.powers.clone(),
                groups: vec![group],
                ..Decided::default()
            }))
        })?;
        Ok(changed.expect("a group's add always changes the board"))
    }

    /// Removes the group `name` as `by`'s change, taking each of its members out of it in the same
    /// write. Refused where the board keeps no such group.
    pub fn remove_group(&self, name: &str, by: &str) -> Result<Change, Error> {
        let changed = self.change_people(by, |people| {
            kept_group(people, name)?;
            let members = (people.members.iter())
                .filter(|member| member.groups.iter().any(|group| group == name))
                .map(|member| {
                    let mut member = member.clone();
                    member.groups.retain(|group| group != name);
                    member
                })
                .collect();
            Ok(Some(Decided {
                did: Some(Did::GroupRemove),
                name: name.into(),
                members,
                groups_gone: vec![name.into()],
                ..Decided::default()
            }))
        })?;
        Ok(changed.expect("a group's removal always changes the board"))
    }

    /// The people who hold decide on the board, whose approve meets the `owner` stamp as the
    /// owner's does ([`crate::stamps::read::owner_at_by`]): none where the board's people cannot
    /// be read, so the stamp is the owner's alone then.
    pub fn deciders(&self) -> Vec<String> {
        (self.people())
            .map(|people| people.holding(Power::Decide))
            .unwrap_or_default()
    }

    /// The host a workflow operation `principal` asks is judged on: [`Host::local`], and for a
    /// person the board named ([`crate::workflow::person`]) [`Host::local_with`] its people, read
    /// now, so the owner's operation reads nothing more than it did.
    pub fn host_for(&self, principal: &Principal) -> Result<Host, Error> {
        if principal.issuer == crate::workflow::PEOPLE {
            return Ok(Host::local_with(&self.people()?));
        }
        Ok(Host::local())
    }

    /// Every handle a session of the board was given.
    fn session_handles(&self) -> Result<Vec<String>, Error> {
        self.retrying(|| {
            let handles = Statement::from(super::postgres::sessions::HANDLES);
            self.read_together(handles, json!({}))
        })
    }

    /// Refused on a board that lacks the step its tables come with.
    fn keeping_people(&self) -> Result<(), Error> {
        if self.additions()? < schema::people_step() {
            return Err(refused(
                "people are not on this board yet: its schema lacks the step that adds them, \
                 which the project's own build of the board adds at its next command: nothing \
                 was written"
                    .into(),
            ));
        }
        Ok(())
    }

    /// The change `decide` makes from the board's people, written with its line as `by`'s at
    /// this store's clock: none, and nothing written, where it decides on none.
    fn change_people(
        &self,
        by: &str,
        decide: impl Fn(&People) -> Result<Option<Decided>, Error>,
    ) -> Result<Option<Change>, Error> {
        self.keeping_people()?;
        let by = writer(by)?;
        for _ in 0..TRIES {
            let people = self.people()?;
            let Some(mut decided) = decide(&people)? else {
                return Ok(None);
            };
            let n = people.revision + 1;
            let line = Change {
                n,
                at: self.now(),
                by: by.into(),
                did: decided.did.expect("a change says what it did"),
                name: decided.name,
                group: decided.group,
                powers: decided.powers,
            };
            (decided.members.iter_mut()).for_each(|member| member.revision = n);
            (decided.groups.iter_mut()).for_each(|group| group.revision = n);
            let vars = json!({
                "expected": people.revision,
                "line": line,
                "members": decided.members,
                "members_gone": decided.members_gone,
                "groups": decided.groups,
                "groups_gone": decided.groups_gone,
            });
            let wrote: bool = self.retrying(|| {
                self.transact_dispatch(
                    Statement::from(pg::change()).naming(&TABLES),
                    vars.clone(),
                    false,
                )
            })?;
            if wrote {
                return Ok(Some(line));
            }
        }
        Err(refused(format!(
            "the board's people changed under this change {TRIES} times: nothing was written by it"
        )))
    }
}

/// The person `name`, refused where the board holds none by it.
fn kept_member<'a>(people: &'a People, name: &str) -> Result<&'a Member, Error> {
    (people.member(name)).ok_or_else(|| nothing(format!("no person {name} is on this board")))
}

/// The group `name`, refused where the board keeps none by it.
fn kept_group<'a>(people: &'a People, name: &str) -> Result<&'a Group, Error> {
    (people.group(name)).ok_or_else(|| nothing(format!("this board keeps no group {name}")))
}

/// The powers `holder` holds as its own: a person's given them, not their groups'.
fn own_powers(people: &People, holder: Holder<'_>) -> Result<Vec<Power>, Error> {
    Ok(match holder {
        Holder::Person(name) => kept_member(people, name)?.powers.clone(),
        Holder::Group(name) => kept_group(people, name)?.powers.clone(),
    })
}

/// The change of `holder`'s own powers to `after`, its line naming `named`.
fn rewritten(
    people: &People,
    holder: Holder<'_>,
    did: Did,
    named: Vec<Power>,
    after: Vec<Power>,
) -> Decided {
    let mut decided = Decided {
        did: Some(did),
        powers: named,
        ..Decided::default()
    };
    match holder {
        Holder::Person(name) => {
            let mut member = people.member(name).cloned().unwrap_or_default();
            member.powers = after;
            decided.name = name.into();
            decided.members = vec![member];
        }
        Holder::Group(name) => {
            let mut group = people.group(name).cloned().unwrap_or_default();
            group.powers = after;
            decided.name = name.into();
            decided.group = Some(name.into());
            decided.groups = vec![group];
        }
    }
    decided
}

/// How a refusal names `holder`.
fn said(holder: Holder<'_>) -> (&'static str, &str) {
    match holder {
        Holder::Person(name) => ("person", name),
        Holder::Group(name) => ("group", name),
    }
}

/// Refused where no power is named, or one no one is given yet is.
fn named_powers(powers: &[Power]) -> Result<(), Error> {
    if powers.is_empty() {
        return Err(nothing("name at least one power".into()));
    }
    given(powers)
}

/// Refused where `powers` hold one no one is given yet ([`Power::withheld`]), in its words.
fn given(powers: &[Power]) -> Result<(), Error> {
    match (powers.iter()).find_map(|power| power.withheld().map(|why| (power, why))) {
        Some((power, why)) => Err(nothing(format!(
            "{} is given to no one yet: {why}",
            power.said()
        ))),
        None => Ok(()),
    }
}

/// `powers` once each, in the order they are said.
fn said_order(powers: impl IntoIterator<Item = Power>) -> Vec<Power> {
    powers
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// `names` once each, in order.
fn in_order(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// A refusal that wrote nothing.
fn nothing(why: String) -> Error {
    refused(format!("{why}: nothing was written"))
}

/// `by`, who a change is recorded as made by: refused empty.
fn writer(by: &str) -> Result<&str, Error> {
    let by = by.trim();
    if by.is_empty() {
        return Err(nothing(
            "a change of the board's people says who made it".into(),
        ));
    }
    Ok(by)
}
