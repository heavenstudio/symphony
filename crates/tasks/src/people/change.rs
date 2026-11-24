//! The changes of a person that write the machine's profile as well as a board's row, as the
//! command line and the People page make them: [`add`] and [`profile`], one function each, so the
//! two refuse and say the same.
//!
//! A profile is every board's on the machine ([`super::Profiles`]), so who writes one is said by
//! [`Writer`]: the person at the machine writes any, and alone gives a sign-in, as a sign-in decides
//! who acts under the name on every board holding it, now or later; a board's admin who is not
//! that person makes one only for someone who has none, as they add them, with no sign-in; a
//! person writes their own display name and git handles. A sign-in is one person's, checked under
//! the profiles' lock where the profiles are written, so two writes at once cannot give one
//! sign-in to two people.
//!
//! A refusal to anyone but the person at the machine names no other person and no profile: what
//! another board's people hold is not theirs to learn. A failure keeps its cause apart
//! ([`Failure`]), as a cause can name the machine folder's path, which only the person at the
//! machine is shown.

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
    /// person who has none, they make it with no sign-in, and they write no other.
    Other,
    /// The person themself: their own display name and git handles.
    Own,
}

/// Why a change stopped: refused, with nothing written, or failed, with what stands said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    Refused(String),
    Failed(Failure),
}

/// A change that failed: what stands, said to anyone, and its cause apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// What failed and what stands, with no cause.
    pub said: String,
    /// Why, as the machine folder or the board said it: it can name the folder's path, so only
    /// the person at the machine is shown it.
    pub why: String,
    /// The person the change left on the board where taking its change back failed too, whom a
    /// removal of them takes off: each caller names its own way to it.
    pub left: Option<String>,
}

impl Failure {
    fn new(said: impl Into<String>, why: impl ToString) -> Failure {
        Failure {
            said: said.into(),
            why: why.to_string(),
            left: None,
        }
    }

    /// What it says: its cause in brackets where `cause`, and, for the person left on the board,
    /// `remedy` of their name after.
    pub fn told(&self, cause: bool, remedy: impl FnOnce(&str) -> String) -> String {
        let mut told = self.said.clone();
        if cause {
            told = format!("{told} ({})", self.why);
        }
        if let Some(left) = &self.left {
            told = format!("{told}: {}", remedy(left));
        }
        told
    }
}

impl From<crate::Error> for Stopped {
    fn from(error: crate::Error) -> Stopped {
        match error {
            crate::Error::Refused(why) => Stopped::Refused(why),
            crate::Error::Db(_) => {
                Stopped::Failed(Failure::new("the board could not be changed", error))
            }
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
/// their write, the board's change between, so a sign-in another profile holds, a profile or a
/// sign-in an admin may not write and whatever the board refuses ([`Store::add_person`]) are
/// refused with nothing written, and no two adds give one sign-in to two people. A profile that
/// cannot be written takes the board's change back, saying so.
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
        return Err(Stopped::Failed(Failure::new(
            "the machine's profiles could not be locked or read, so nothing was written",
            why,
        )));
    };
    Err(Stopped::Failed(match store.remove_person(name, by) {
        Ok(_) => Failure::new(
            format!(
                "{name}'s profile could not be written, so the board's change {} is taken back",
                line.n
            ),
            why,
        ),
        Err(_) => Failure {
            left: Some(name.into()),
            ..Failure::new(
                format!(
                    "{name}'s profile could not be written, and the board's change {} still \
                     stands",
                    line.n
                ),
                why,
            )
        },
    }))
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
    .map_err(|why| {
        Stopped::Failed(Failure::new(
            format!("{name}'s profile could not be written, so nothing changed"),
            why,
        ))
    })?
    .map_err(Stopped::Refused)
}

/// What a board the machine serves holds under a name ([`name_machine`]'s `held`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Held {
    /// A person of that name: theirs where the machine's profile of the name is.
    Person,
    /// A group of that name: never a person's.
    Group,
}

/// Gives the person at the machine their own profile, `name` its short name, `given` its display
/// name and git handles, in the machine folder `dir` (docs/product/analysis/users-and-identity.md,
/// "How a person's name is written"): from then on every line they write, on every board the
/// machine serves, carries `name` where it carried `owner`. Only the person at the machine asks
/// it (the People page's You, behind the gate's own judgement of who they are), and it is said
/// once: a name once used is never given to another, so it is never changed.
///
/// A profile of `name` that holds no sign-in is taken as theirs, with what it holds: only they, or
/// a board's admin adding someone the machine gives no sign-in yet, made it, and no visitor ever
/// wrote under it, as nobody reaches a board as a profile with no sign-in. So the person at the
/// machine who added themselves to a board before naming themselves takes that name, and a row a
/// board holds of it is theirs. One with a sign-in is someone's who reaches the boards, and is
/// refused.
///
/// `held` says what a board the machine serves holds under `name` (a person, a group, nothing),
/// asked under the profiles' lock, which a group's add takes too ([`add_group`]): a group is
/// refused, and a person where the machine holds no profile of the name, as they are someone a
/// board named on another machine; a board that cannot be read fails it with nothing written.
///
/// Refused, with nothing written: a name no board could give a person
/// ([`super::refused_person_name`]), a profile someone signs in as, a second name, and a sign-in,
/// as the person at the machine is known by where they are, never by a service.
pub fn name_machine(
    dir: &Path,
    name: &str,
    given: &Given,
    held: impl FnOnce(&str) -> Result<Option<Held>, Stopped>,
) -> Result<(), Stopped> {
    let name = name.trim();
    if let Some(why) = super::refused_person_name(name) {
        return Err(Stopped::Refused(format!("{why}: nothing was written")));
    }
    if !given.signins.is_empty() {
        return Err(Stopped::Refused(
            "the person at the machine is known by where they are, never by a sign-in: give \
             yourself a name with none, so nothing was written"
                .into(),
        ));
    }
    let written = Profiles::change(dir, |profiles| {
        if let Some(already) = profiles.machine() {
            return Ok(Err(Stopped::Refused(format!(
                "you write as {} already, and a short name once given is never changed: \
                 nothing was written",
                already.name
            ))));
        }
        let profile = profiles.get(name);
        if profile.is_some_and(|profile| !profile.signins.is_empty()) {
            return Ok(Err(Stopped::Refused(format!(
                "{name} is the profile of someone who signs in, and a name once used is never \
                 given to another: choose another name, so nothing was written"
            ))));
        }
        match held(name) {
            Err(stopped) => return Ok(Err(stopped)),
            Ok(Some(Held::Group)) => {
                return Ok(Err(Stopped::Refused(format!(
                    "{name} is a group's name on a board of this machine, and a person's name and \
                     a group's never meet: choose another name, so nothing was written"
                ))));
            }
            Ok(Some(Held::Person)) if profile.is_none() => {
                return Ok(Err(Stopped::Refused(format!(
                    "{name} is a person on a board of this machine whom it holds no profile of, \
                     someone named elsewhere: choose another name, so nothing was written"
                ))));
            }
            Ok(Some(Held::Person) | None) => {}
        }
        fill(profiles, name, given);
        profiles.machine = Some(name.into());
        Ok(Ok(()))
    });
    written.map_err(|why| {
        Stopped::Failed(Failure::new(
            "the machine's profiles could not be locked, read or written, so nothing was written",
            why,
        ))
    })?
}

/// Makes the group `name` on the board of `store` with `powers`, as `by`'s change
/// ([`Store::add_group`]), refused where a profile of the machine is named `name`: a person's name
/// and a group's never meet, and a profile's person can be on any board, the person at the
/// machine on every one, so a line under the name never reads as the group's. The profiles' lock
/// is held from the read through the board's change, as [`name_machine`] holds it, so the two
/// never both take one name.
pub fn add_group(
    store: &Store,
    dir: &Path,
    name: &str,
    powers: &[Power],
    by: &str,
) -> Result<Change, Stopped> {
    Profiles::change(dir, |profiles| {
        if profiles.get(name).is_some() {
            return Ok(Err(Stopped::Refused(format!(
                "{name} is a person's name on this machine, and a person's name and a group's \
                 never meet: name the group otherwise, so nothing was written"
            ))));
        }
        Ok(store.add_group(name, powers, by).map_err(Stopped::from))
    })
    .map_err(|why| {
        Stopped::Failed(Failure::new(
            "the machine's profiles could not be locked or read, so nothing was written",
            why,
        ))
    })?
}

/// Why `writer` may not add `name` with `given` to a board over `profiles`: none where they may.
/// Nobody adds the person at the machine, who is on every board with every power and no row. Anyone
/// but the person at the machine adds as the board's admin: they give no sign-in, which would
/// decide who acts as `name` on every board holding it, now or later, and make a profile only for
/// a person who has none. Their refusals name no profile and no other person.
fn addable(profiles: &Profiles, name: &str, given: &Given, writer: Writer) -> Result<(), String> {
    if profiles.machine.as_deref() == Some(name) {
        return Err(format!(
            "{name} is the person at the machine, on every board with every power, and no row \
             adds them: nothing was written"
        ));
    }
    if writer == Writer::Machine {
        return taken_signins(profiles, name, &given.signins);
    }
    if !given.signins.is_empty() {
        return Err(
            "a sign-in is the person at the machine's to give, as it says who acts under a name \
             on every board of the machine: add them with none, so nothing was written"
                .into(),
        );
    }
    if profiles.get(name).is_some() && !given.is_empty() {
        return Err(format!(
            "an admin's add gives a display name or git handles only to a name new to the \
             machine: add {name} with none, and they or the person at the machine write theirs, \
             so nothing was written"
        ));
    }
    Ok(())
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
