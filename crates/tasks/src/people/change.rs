//! The changes of a person that write the machine's profile as well as a board's row, as the
//! command line and the People page make them: [`add`] and [`profile`], one function each, so the
//! two refuse and say the same.
//!
//! A profile is every board's on the machine ([`super::Profiles`]), so who writes one is said by
//! [`Writer`]: the person at the machine writes any; a board's admin who is not that person makes
//! one only for someone who has none, as they add them, since a sign-in decides who acts under the
//! name on every board and a git handle whose commits their saves are; a person writes their own
//! display name and git handles. A sign-in is one person's, checked under the profiles' lock where the profiles are
//! written, so two writes at once cannot give one sign-in to two people.

use super::{Change, Power, Profile, Profiles, Signin};
use crate::Store;
use std::path::Path;

/// What a profile is given: a display name, sign-ins and git handles, each kept where the profile
/// does not hold it already.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Given {
    pub display: Option<String>,
    pub signins: Vec<Signin>,
    pub git: Vec<String>,
}

impl Given {
    /// Whether it gives nothing: no display name, sign-in or git handle.
    pub fn is_empty(&self) -> bool {
        self.display.as_deref().is_none_or(|d| d.trim().is_empty())
            && self.signins.is_empty()
            && self.git.iter().all(|git| git.trim().is_empty())
    }
}

/// Who writes a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The person at the machine, at its terminal or on its pages: any profile.
    Machine,
    /// Someone a board named, writing a profile not their own: as that board's admin adding a
    /// person who has none, they make it, and they write no other.
    Other,
    /// The person themself: their own display name and git handles.
    Own,
}

/// Why a change stopped: refused, with nothing written, or failed, with what stands said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    Refused(String),
    Failed(String),
}

impl From<crate::Error> for Stopped {
    fn from(error: crate::Error) -> Stopped {
        match error {
            crate::Error::Refused(why) => Stopped::Refused(why),
            crate::Error::Db(_) => Stopped::Failed(error.to_string()),
        }
    }
}

/// What an add did: the board's change, and how many sign-ins the person's profile holds now.
#[derive(Clone, Debug, PartialEq)]
pub struct Added {
    pub line: Change,
    pub signins: usize,
}

/// Puts `name` on the board of `store` in `groups` with `powers`, as `by`'s change, and their
/// profile in the machine folder `dir` made or given `given` as `writer` may (a person a board
/// named adds as its admin, [`Writer::Other`]). The profiles' lock is held from their read to
/// their write, the board's change between, so a sign-in another profile holds, a profile an
/// admin may not write and whatever the board
/// refuses ([`Store::add_person`]) are refused with nothing written, and no two adds give one
/// sign-in to two people. A profile that cannot be written takes the board's change back, saying
/// so.
#[allow(clippy::too_many_arguments)]
pub fn add(
    store: &Store,
    dir: &Path,
    name: &str,
    groups: &[String],
    powers: &[Power],
    given: &Given,
    writer: Writer,
    by: &str,
) -> Result<Added, Stopped> {
    let mut taken: Option<Change> = None;
    let written = Profiles::change(dir, |profiles| {
        let line = match addable(profiles, name, given, writer)
            .map_err(Stopped::Refused)
            .and_then(|()| Ok(store.add_person(name, groups, powers, by)?))
        {
            Ok(line) => line,
            Err(stopped) => return Ok(Err(stopped)),
        };
        taken = Some(line.clone());
        fill(profiles, name, given);
        let signins = profiles.get(name).map_or(0, |held| held.signins.len());
        Ok(Ok(Added { line, signins }))
    });
    let why = match written {
        Ok(done) => return done,
        Err(why) => why,
    };
    let Some(line) = taken else {
        return Err(Stopped::Failed(format!(
            "the machine's profiles could not be locked or read ({why}): nothing was written"
        )));
    };
    let undone = store.remove_person(name, by).map(|_| "taken back");
    Err(Stopped::Failed(format!(
        "{name}'s profile could not be written ({why}): the board's change {} is {}",
        line.n,
        undone.unwrap_or("still on the board, which a removal of them takes back")
    )))
}

/// The machine's profile of `name` in `dir` made or given `given`, as `writer` may, on no board:
/// whether it changed. Refused, with nothing written: a name no board could give a person
/// ([`super::refused_person_name`]), a profile not the writer's
/// ([`Writer::Other`]), a sign-in from anyone but the person at the machine, and a sign-in another
/// profile holds, read under the lock.
pub fn profile(dir: &Path, name: &str, given: &Given, writer: Writer) -> Result<bool, Stopped> {
    if let Some(why) = super::refused_person_name(name) {
        return Err(Stopped::Refused(format!("{why}: nothing was written")));
    }
    match writer {
        Writer::Other => {
            return Err(Stopped::Refused(format!(
                "only the person at the machine, or {name}, changes {name}'s profile: nothing \
                 was written"
            )));
        }
        Writer::Own if !given.signins.is_empty() => {
            return Err(Stopped::Refused(
                "a sign-in is the person at the machine's to give: nothing was written".into(),
            ));
        }
        Writer::Own | Writer::Machine => {}
    }
    Profiles::change(dir, |profiles| {
        if let Err(why) = taken_signins(profiles, name, &given.signins) {
            return Ok(Err(why));
        }
        let before = profiles.clone();
        fill(profiles, name, given);
        Ok(Ok(*profiles != before))
    })
    .map_err(|why| Stopped::Failed(format!("{name}'s profile could not be written ({why})")))?
    .map_err(Stopped::Refused)
}

/// Why `writer` may not add `name` with `given` to a board over `profiles`: none where they may.
/// Anyone but the person at the machine adds as the board's admin, making a profile only for a
/// person who has none.
fn addable(profiles: &Profiles, name: &str, given: &Given, writer: Writer) -> Result<(), String> {
    if writer != Writer::Machine && profiles.get(name).is_some() && !given.is_empty() {
        return Err(format!(
            "{name} has a profile on this machine, which only the person at the machine \
             changes: add them with no display name, sign-in or git handle and they keep \
             theirs, so nothing was written"
        ));
    }
    taken_signins(profiles, name, &given.signins)
}

/// Refused where one of `signins` is another profile's than `name`'s: a sign-in is one person's.
fn taken_signins(profiles: &Profiles, name: &str, signins: &[Signin]) -> Result<(), String> {
    for signin in signins {
        if let Some(holder) = profiles.signed_in(&signin.service, &signin.subject)
            && holder.name != name
        {
            return Err(format!(
                "that {} sign-in is {}'s: a sign-in is one person's, so nothing was written",
                signin.service, holder.name
            ));
        }
    }
    Ok(())
}

/// `name`'s profile among `profiles`, made where there is none, given what `given` names where it
/// does not hold it.
fn fill(profiles: &mut Profiles, name: &str, given: &Given) {
    let at = match (profiles.people.iter()).position(|profile| profile.name == name) {
        Some(at) => at,
        None => {
            profiles.people.push(Profile {
                name: name.into(),
                ..Profile::default()
            });
            profiles.people.len() - 1
        }
    };
    let profile = &mut profiles.people[at];
    if let Some(display) = (given.display.as_deref())
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        profile.display = display.into();
    }
    for signin in &given.signins {
        if !(profile.signins.iter()).any(|held| held.is(&signin.service, &signin.subject)) {
            profile.signins.push(signin.clone());
        }
    }
    for git in given.git.iter().map(|git| git.trim()) {
        if !git.is_empty() && !profile.git.iter().any(|held| held == git) {
            profile.git.push(git.into());
        }
    }
}
