//! The People page's read and its two writes (docs/workflow/TASKS.md, "People"). `GET`
//! [`PEOPLE_PATH`] answers a [`PeoplePage`]: who is on the board besides the person at the machine,
//! the groups and the log of every change, and what the one reading may change there. `POST`
//! [`PEOPLE_CHANGE_PATH`] takes a [`PeopleChange`], one change of the board's people, which needs
//! the administer power; `POST` [`PROFILE_PATH`] takes a [`ProfileWrite`], a person's own display
//! name, git handles, addresses and the ways their notices go by, or any profile for the person at
//! the machine. Both answer the page as the change left it, or a refusal that says why with nothing
//! written.
//!
//! A sign-in is named by its service alone: no answer carries a sign-in's subject. A person's
//! addresses ([`PersonRow::addresses`]) are in the answer of the person themself, of the person at
//! the machine and of the board's admins, and the ways their notices go by
//! ([`PersonRow::notices`]) in the person's and the person at the machine's alone: anyone else's
//! answer carries neither field. A power is the word the board writes (`decide`,
//! `direct_agents`).

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
    /// The powers the board gives no one yet, each with why: the page draws them as not yet
    /// givable, and the board refuses a grant of one in the same words.
    pub withheld: Vec<WithheldPower>,
    /// The one reading's short name on the board; none for the person at the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you: Option<String>,
    /// Why the board's people could not be read, said whole; the page then lists nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// The person at the machine's own profile, which their You section shows and writes: for
    /// them alone, none for anyone else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mine: Option<MachineProfile>,
    /// The sign-in services this server knows a visitor by, which an add and a profile take a
    /// sign-in of: `tailscale` while this machine is on a tailnet, none where it is on none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signins: Vec<String>,
}

/// The person at the machine's own profile.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct MachineProfile {
    /// The short name they gave themselves, fixed once given: none until then, and their lines
    /// read `owner`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Their display name: empty where they gave none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display: String,
    /// Their git handles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
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
    /// Where their notices reach them: in the answer of the person themself, of the person at the
    /// machine and of the board's admins; in no one else's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Addresses>,
    /// The way each kind of notice goes by, every kind the board knows in its order: in the answer
    /// of the person themself and of the person at the machine; in no one else's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<NoticeWay>>,
}

/// A person's addresses, each the one a notice by its way goes to, and none where they hold none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Addresses {
    /// The email a notice by mail goes to: the first of their emails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Their Slack member id (`U024BE7LH`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<String>,
    /// Their WhatsApp number, `+` and its digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<String>,
}

/// The way one kind of notice goes by.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct NoticeWay {
    /// The kind, as a notice writes it: `card`, `question`, `verdict`.
    pub kind: String,
    /// The way it goes by, `email`, `slack` or `whatsapp`; none where it goes by none, as the page
    /// and the Mac app show every notice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub way: Option<String>,
}

/// A power the board gives no one yet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WithheldPower {
    /// Its word, as `direct_agents`.
    pub power: String,
    /// Why no one is given it yet, as the board's refusal of a grant says it after the power.
    pub why: String,
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
    /// The email a notice by mail goes to, put first among their emails; an empty one takes the
    /// first away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Their Slack member id; an empty one takes it away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<String>,
    /// Their WhatsApp number, `+` and its country code; an empty one takes it away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<String>,
    /// The ways kinds of notice go by, each kind named set: a way of none sends it by none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<NoticeWay>,
    /// Whether it gives the person at the machine `name` as their own short name, once and for
    /// good: theirs alone to ask, and refused once they have one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub machine: bool,
}
