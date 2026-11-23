//! The Settings page's read and its one write. `GET` [`SETTINGS_PATH`] answers a [`SettingsPage`],
//! everything in force for a project and where each value comes from. `POST`
//! [`SETTING_WRITE_PATH`] takes a [`SettingWrite`]: a preference kept in the board's store, its
//! entry removed, or its stored value made the project's, committed to the project file
//! ([`SettingAction`]); it answers a [`SettingWritten`], or a refusal that says why with nothing
//! changed.
//!
//! The page is a list of entries in one shape ([`SettingsEntry`]): its name, what it is for, its
//! state and why, what it needs and whether this machine has it, and its settings
//! ([`Setting`]), each with its key, its kind, the value in force, where that value was written
//! ([`SettingSource`]), the scopes it may be written in and its class. The client draws every
//! entry with one component and knows none by name, so an entry a later server describes draws
//! with no change to the page. What the page writes is beside the entries, in
//! [`SettingsPage::stored`]: one item for each setting it draws a control for, by its key.

use serde::{Deserialize, Serialize};

/// The entry and what it holds are the plugin kit's (`symphony-blocks`), as a plugin and a program
/// describe their own in that shape; re-exported here under the names they have always had.
pub use symphony_blocks::{
    EntryNeed, EntryState, Setting, SettingClass, SettingKind, SettingScope, SettingSource,
    SettingsEntry,
};

/// The Settings page's read.
pub const SETTINGS_PATH: &str = "/api/settings";

/// The Settings page's write ([`SettingWrite`]).
pub const SETTING_WRITE_PATH: &str = "/api/setting";

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
    /// that does not load, a local file that is refused, a limit that cannot be read, a board's
    /// store that cannot be read or an entry of it that is not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    pub sections: Vec<SettingsSection>,
    /// The settings the page writes, each a preference the board's store keeps: one for each it
    /// draws a control for, whether the store has an entry for it or not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stored: Vec<StoredSetting>,
}

/// A setting the page writes, and what the board's store holds for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct StoredSetting {
    /// The setting, by its key on the page ([`Setting::key`]: `[limits] low_volume`).
    pub setting: String,
    /// Its key in the store, which a write names (`limits.low_volume`).
    pub key: String,
    /// What the store's entry holds, as shown; none with no entry, or once it was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether that value is the one in force: not where the local file writes the setting, nor
    /// where the store does not read its entry.
    pub held: bool,
    /// Why the store does not read its entry, when it refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// Every change of the entry, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<StoredChange>,
    /// Where making it the project's stands while its commit waits for its delivery by pull
    /// request: the branch that delivers it, and how far it went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

/// One change of a stored entry: what it held before and after (none: no entry), when and by
/// whom.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct StoredChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// When, as the board says a minute (`2026-11-22 20:15`).
    pub at: String,
    pub by: String,
}

/// What a write does to a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingAction {
    /// The value kept in the board's store, in force at once.
    Store,
    /// The store's entry removed: the files' value is in force again.
    Remove,
    /// The stored value made the project's: committed to the project file, then the entry removed.
    Commit,
}

/// A value a write gives, of the setting's kind: on or off, a whole number, or text (a choice's
/// among them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum SettingValue {
    Switch(bool),
    Number(i64),
    Text(String),
}

/// The body of a write ([`SETTING_WRITE_PATH`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingWrite {
    /// The setting, by its key in the store ([`StoredSetting::key`]).
    pub key: String,
    pub action: SettingAction,
    /// The value [`SettingAction::Store`] keeps; none for the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<SettingValue>,
}

/// What a write did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SettingWritten {
    /// What it did, in a sentence.
    pub said: String,
    /// The commit made on the default branch, for a commit made there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Whether the commit waits for its delivery by pull request, its entry kept meanwhile.
    #[serde(default)]
    pub waiting: bool,
}
