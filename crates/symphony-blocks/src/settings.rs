//! A Settings page's entry, in the one shape every entry takes: its name, what it is for, its
//! state and why, what it needs and whether this machine has it, and its settings ([`Setting`]),
//! each with its key, its kind, the value in force, where that value was written
//! ([`SettingSource`]), the scopes it may be written in and its class. The Symphony describes its
//! own entries in it; a plugin describes its own through the describers seam
//! (`symphony_plugin::Describer`), and a program such as the GitHub exchange prints one as the
//! answer of a command. The page, `symphony_api::settings::SettingsPage`, draws every entry with
//! one component and knows none by name.
//!
//! A description declares and runs nothing, and its class is not the describer's to lower:
//! whatever the Symphony would run, or that decides what lands, is shown reviewed whatever its
//! describer says, and a describer may mark a setting of its own reviewed.
//!
//! The structs are built by their fields, so a field added to one is a new major of the kit; the
//! words ([`SettingScope`], [`SettingClass`], [`SettingKind`]) are `#[non_exhaustive]`, as a new
//! word is a minor.

use serde::{Deserialize, Serialize};

/// Whom a value holds for, and so where it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SettingScope {
    /// The committed project file: the same on every machine.
    Project,
    /// The board's store of preferences, read after the project file and before the local file:
    /// in force at once, and committed nowhere.
    Board,
    /// The local settings file beside it, which git does not version: one machine's.
    Machine,
    /// The browser: one device's.
    Device,
}

/// Whether a setting is one a person sets, or one a reviewed task changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// What the value stands over: for a local file's, where the committed one is written, said as
    /// [`SettingSource::said`] says one; for the board's store's, the committed value and where
    /// (`4 (symphony.toml [limits] low_volume, line 5)`); for a local file's over the store's, the
    /// stored value and the store.
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
