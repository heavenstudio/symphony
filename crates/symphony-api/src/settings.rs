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

/// The entry and what it holds are the plugin kit's (`symphony-blocks`), as a plugin and a program
/// describe their own in that shape; re-exported here under the names they have always had.
pub use symphony_blocks::{
    EntryNeed, EntryState, Setting, SettingClass, SettingKind, SettingScope, SettingSource,
    SettingsEntry,
};

/// The Settings page's read.
pub const SETTINGS_PATH: &str = "/api/settings";

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
