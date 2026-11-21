//! The Settings page's one read: `GET` [`SETTINGS_PATH`] answers a [`SettingsPage`], everything in
//! force for a project and where each value comes from. It is a read alone: nothing the page
//! draws writes a setting.
//!
//! The page is a list of entries in one shape ([`SettingsEntry`]): its name, what it is for, its
//! state and why, what it needs and whether this machine has it, and its settings
//! ([`Setting`]), each with its key, its kind, the value in force, where that value was written
//! ([`SettingSource`]), the scopes it may be written in and its class. The client draws every
//! entry with one component and knows none by name, so an entry a later server describes draws
//! with no change to the page.

use serde::{Deserialize, Serialize};

/// The Settings page's read.
pub const SETTINGS_PATH: &str = "/api/settings";

/// Whom a value holds for, and so where it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingScope {
    /// The committed project file: the same on every machine.
    Project,
    /// The local settings file beside it, which git does not version: one machine's.
    Machine,
    /// The browser: one device's.
    Device,
}

/// Whether a setting is one a person sets, or one a reviewed task changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingClass {
    Preference,
    /// It holds a command or a program the Symphony runs, or decides what lands: the keys the
    /// local settings file refuses by name.
    Reviewed,
}

/// What a setting's value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Text,
    Number,
    /// On or off.
    Switch,
    /// One of [`Setting::choices`].
    Choice,
    /// Several texts, said as one.
    List,
    /// A table or more said as a sentence: what a reviewed setting is now.
    Sentence,
}

/// Where a value in force was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingSource {
    /// The scope of the file that wrote it; none for a default, which no file writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SettingScope>,
    /// Where it stands, whole, as `just tasks limits` names one
    /// (`symphony.local.toml [limits] machine_agents, line 6`), or the default said as one.
    pub said: String,
    /// The file's name, when a file wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Its line there, when it can be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The committed value a local file's holds over, said as [`SettingSource::said`] says one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<String>,
}

/// One setting of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Setting {
    /// Its table and key as the file spells them (`[limits] low_volume`), or the table alone for
    /// one said as a sentence (`[land]`).
    pub key: String,
    /// What it is, in words.
    pub name: String,
    pub kind: SettingKind,
    /// The value in force, as shown.
    pub value: String,
    /// What a [`SettingKind::Choice`] may be.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    pub source: SettingSource,
    /// The scopes it may be written in.
    pub scopes: Vec<SettingScope>,
    pub class: SettingClass,
}

/// Whether an entry is on, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct EntryState {
    /// Its state in a word or two (`On`, `Off`, `None selected`).
    pub word: String,
    /// On or off, for an entry that is one of the two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// Why it is so.
    pub why: String,
}

/// Something an entry needs, and whether this machine has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct EntryNeed {
    /// What is needed (`the claude program`).
    pub what: String,
    pub met: bool,
    /// What was found, or what is missing and what that means.
    pub said: String,
}

/// One thing the page shows: an integration, a tool, or a group of settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingsEntry {
    /// Stable among the page's entries: its anchor.
    pub id: String,
    pub name: String,
    /// What it is for.
    pub about: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<EntryState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<EntryNeed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
    /// Where it is said as a sentence in a file: for an entry that carries no setting of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SettingSource>,
}

/// A part of the page: a heading over entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingsSection {
    pub id: String,
    pub title: String,
    /// One line under the heading.
    pub about: String,
    pub entries: Vec<SettingsEntry>,
}

/// A file the settings are read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingsFile {
    /// Its name (`symphony.toml`).
    pub name: String,
    pub scope: SettingScope,
    /// Where it is, or would be.
    pub path: String,
    /// Whether it is there.
    pub found: bool,
}

/// Everything in force for a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingsPage {
    /// The files read, the project file first.
    pub files: Vec<SettingsFile>,
    /// What is wrong with a file, each said whole with what holds in its place: a project file
    /// that does not load, a local file that is refused, a limit that cannot be read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    pub sections: Vec<SettingsSection>,
}
