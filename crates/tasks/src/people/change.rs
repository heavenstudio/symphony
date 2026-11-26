//! The changes of a person that write the machine's profile as well as a board's row, as the
//! command line and the People page make them: [`add`] and [`profile`], one function each, so the
//! two refuse and say the same.
//!
//! A profile is every board's on the machine ([`super::Profiles`]), so who writes one is said by
//! [`Writer`]: the person at the machine writes any, and alone gives a sign-in, as a sign-in decides
//! who acts under the name on every board holding it, now or later; a board's admin who is not
//! that person makes one only for someone who has none, as they add them, with no sign-in; a
//! person writes their own display name, git handles, addresses and the ways their notices go by.
//! An address and a way are where a person's notices go, theirs to say, so an admin who is not the
//! person at the machine writes neither. A sign-in is one person's, checked under the profiles'
//! lock where the profiles are written, so two writes at once cannot give one sign-in to two
//! people. An address is held to the rule its sender enforces ([`super::ways`]), and a refusal of
//! one says the rule, never the address.
//!
//! A refusal to anyone but the person at the machine names no other person and no profile: what
//! another board's people hold is not theirs to learn. A failure keeps its cause apart
//! ([`Failure`]), as a cause can name the machine folder's path, which only the person at the
//! machine is shown.

use super::{Change, Power, Profile, Profiles, Signin, ways};
use crate::Store;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// What a profile is given: a display name, sign-ins and git handles, each kept where the profile
/// does not hold it already; addresses and the ways notices go by, each set as given.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Given {
    pub display: Option<String>,
    pub signins: Vec<Signin>,
    pub git: Vec<String>,
    /// Addresses by the way's word ([`ways::WAYS`]): the address for that way, the email's in
    /// place of the profile's first, those after it kept; an empty one takes the way's address
    /// away, the first email for mail.
    pub addresses: BTreeMap<String, String>,
    /// Ways notices go by, by the kind's word ([`ways::KINDS`]): the way's word; an empty one sends
    /// that kind by none, as the page and the Mac app show every notice.
    pub notices: BTreeMap<String, String>,
}

impl Given {
    /// Whether it gives nothing: no display name, sign-in, git handle, address or way.
    pub fn is_empty(&self) -> bool {
        self.display.as_deref().is_none_or(|d| d.trim().is_empty())
            && self.signins.is_empty()
            && self.git.iter().all(|git| git.trim().is_empty())
            && self.addresses.is_empty()
            && self.notices.is_empty()
    }

    /// Whether it gives an address or a way, which only the person and the person at the machine
    /// write.
    fn reaches(&self) -> bool {
        !self.addresses.is_empty() || !self.notices.is_empty()
    }

    /// Itself with each address and way trimmed and held to its rule, an address as the profile
    /// holds it ([`ways::address`]: a WhatsApp number as `+` and its digits).
    ///
    /// # Errors
    ///
    /// An address its way's rule does not take, a way that is none, or a kind that is none: the
    /// rule said, never the address.
    fn checked(&self) -> Result<Given, String> {
        let mut checked = self.clone();
        for (way, text) in &mut checked.addresses {
            let trimmed = text.trim();
            *text = if trimmed.is_empty() && ways::WAYS.contains(&way.as_str()) {
                String::new()
            } else {
                ways::address(way, trimmed)?
            };
        }
        for (kind, way) in &mut checked.notices {
            if !ways::KINDS.contains(&kind.as_str()) {
                return Err(format!(
                    "a notice is of one of {}, and that kind is none of them",
                    ways::KINDS.join(", ")
                ));
            }
            *way = way.trim().to_string();
            if !way.is_empty() && !ways::WAYS.contains(&way.as_str()) {
                return Err(format!(
                    "a notice goes by one of {}, or by none, and that way is none of them",
                    ways::WAYS.join(", ")
                ));
            }
        }
        Ok(checked)
    }
}

/// What is given is printed without its addresses: a log of a change names none.
impl fmt::Debug for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("display", &self.display)
            .field("signins", &self.signins.len())
            .field("git", &self.git)
            .field("addresses", &self.addresses.keys().collect::<Vec<_>>())
            .field("notices", &self.notices)
            .finish()
    }
}

/// Why `given` cannot be written: its addresses and ways where `writer` is no person who writes
/// them, else one its rules refuse; `given` as it is written where it can be.
fn writable(given: &Given, writer: Writer) -> Result<Given, Stopped> {
    if writer == Writer::Other && given.reaches() {
        return Err(Stopped::Refused(
            "a person's addresses and the ways their notices go by are theirs and the person at \
             the machine's to write: nothing was written"
                .into(),
        ));
    }
    given
        .checked()
        .map_err(|why| Stopped::Refused(format!("{why}: nothing was written")))
}

/// Who writes a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The person at the machine, at its terminal or on its pages: any profile.
    Machine,
    /// Someone a board named, writing a profile not their own: as that board's admin adding a
    /// person who has none, they make it with no sign-in, and they write no other.
    Other,
    /// The person themself: their own display name, git handles, addresses and the ways their
    /// notices go by.
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
    let given = &writable(given, writer)?;
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
/// ([`Writer::Other`]), a sign-in from anyone but the person at the machine, an address or a way
/// its rule refuses ([`ways`]), and a sign-in another profile holds, read under the lock.
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
    let given = &writable(given, writer)?;
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

/// Why `writer` may not add `name` with `given` to a board over `profiles`: none where they may.
/// Anyone but the person at the machine adds as the board's admin: they give no sign-in, which
/// would decide who acts as `name` on every board holding it, now or later, and make a profile
/// only for a person who has none. Their refusals name no profile and no other person.
fn addable(profiles: &Profiles, name: &str, given: &Given, writer: Writer) -> Result<(), String> {
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
/// does not hold it, and its addresses and ways as `given` sets them.
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
    for (way, address) in &given.addresses {
        if way != ways::EMAIL {
            if address.is_empty() {
                profile.chat.shift_remove(way.as_str());
            } else {
                profile
                    .chat
                    .insert(way.clone(), Value::from(address.as_str()));
            }
        } else if address.is_empty() {
            // The email a notice goes to is the first: taking it away takes that one.
            if !profile.emails.is_empty() {
                profile.emails.remove(0);
            }
        } else {
            // It replaces the first; those beyond it stay, but for a copy of it.
            let rest = profile
                .emails
                .iter()
                .skip(1)
                .filter(|held| *held != address);
            profile.emails = std::iter::once(address).chain(rest).cloned().collect();
        }
    }
    for (kind, way) in &given.notices {
        if way.is_empty() {
            profile.preferences.shift_remove(kind.as_str());
        } else {
            profile
                .preferences
                .insert(kind.clone(), Value::from(way.as_str()));
        }
    }
}
