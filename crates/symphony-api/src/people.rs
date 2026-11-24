//! The People page's read and its two writes (docs/workflow/TASKS.md, "People"). `GET`
//! [`PEOPLE_PATH`] answers a [`PeoplePage`]: who is on the board besides the person at the machine,
//! the groups and the log of every change, and what the one reading may change there. `POST`
//! [`PEOPLE_CHANGE_PATH`] takes a [`PeopleChange`], one change of the board's people, which needs
//! the administer power; `POST` [`PROFILE_PATH`] takes a [`ProfileWrite`], a person's own display
//! name and git handles, or any profile for the person at the machine. Both answer the page as the
//! change left it, or a refusal that says why with nothing written.
//!
//! A sign-in is named by its service alone: no answer carries a sign-in's subject, an email or a
//! chat address. A power is the word the board writes (`decide`, `direct_agents`).

use serde::{Deserialize, Serialize};

/// The People page's read.
pub const PEOPLE_PATH: &str = "/api/people";

/// A change of the board's people ([`PeopleChange`]).
pub const PEOPLE_CHANGE_PATH: &str = "/api/people-change";

/// A write of a person's profile ([`ProfileWrite`]).
pub const PROFILE_PATH: &str = "/api/people-profile";

/// Who is on a board, and what the one reading may change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PeoplePage {
    /// The people the board named, by short name.
    pub people: Vec<PersonRow>,
    /// Its groups, by name.
    pub groups: Vec<GroupRow>,
    /// Every change made to them, oldest first.
    pub log: Vec<PeopleLogLine>,
    /// The number of the newest change: 0 on a board none was made on.
    pub revision: i64,
    /// Whether the one reading may change who is on the board: the person at the machine, or a
    /// person holding administer.
    pub administers: bool,
    /// Whether the one reading is the person at the machine, who writes any profile.
    pub machine: bool,
    /// The one reading's short name on the board; none for the person at the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you: Option<String>,
    /// Why the board's people could not be read, said whole; the page then lists nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// A person on the board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PersonRow {
    pub name: String,
    /// How pages show them: their profile's display name, else their short name.
    pub display: String,
    /// Every power they hold, their own and their groups', read among them.
    pub powers: Vec<String>,
    /// The powers given to them, not their groups'.
    pub own: Vec<String>,
    pub groups: Vec<String>,
    /// The services they sign in by, each once: never a subject.
    pub signins: Vec<String>,
    /// Their git handles.
    pub git: Vec<String>,
    /// Whether the machine holds a profile of theirs.
    pub profile: bool,
}

/// A group of the board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct GroupRow {
    pub name: String,
    pub powers: Vec<String>,
}

/// One line of the board's people log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PeopleLogLine {
    /// Its number, from 1.
    pub n: i64,
    /// When, in seconds since the epoch.
    pub at: f64,
    /// Who made it: a session's handle, a person's short name, or `owner`.
    pub by: String,
    /// What it did, as the log writes it (`add`, `group-remove`).
    pub did: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub powers: Vec<String>,
}

/// What a change of the board's people does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum PeopleAction {
    /// A person put on the board, with a profile made where they have none.
    Add,
    /// A person taken off the board; their lines stay theirs, their profile the machine's.
    Remove,
    /// A person put in a group.
    Join,
    /// A person taken out of a group.
    Leave,
    /// Powers given to a person, or to a group where the change names one.
    Grant,
    /// Powers taken from a person, or from a group where the change names one.
    Revoke,
    /// A group made.
    GroupAdd,
    /// A group removed, its members out of it.
    GroupRemove,
}

/// The body of a change ([`PEOPLE_CHANGE_PATH`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct PeopleChange {
    pub action: PeopleAction,
    /// The person's short name; the group's for a group's add or removal.
    #[serde(default)]
    pub name: String,
    /// The group a join or a leave names, or whose powers a grant or a revoke changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The powers an add, a group's add, a grant or a revoke names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub powers: Vec<String>,
    /// The groups an add puts the person in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    /// An add's profile, made where the person has none: their display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// An add's profile: the sign-ins it is given, each `SERVICE:SUBJECT`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signins: Vec<String>,
    /// An add's profile: its git handles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
}

/// The body of a profile's write ([`PROFILE_PATH`]): each field given is kept, the rest stay.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProfileWrite {
    /// The person whose profile it is.
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// Git handles added to the profile.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
    /// Sign-ins added to it, each `SERVICE:SUBJECT`: the person at the machine's alone to give.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signins: Vec<String>,
}
