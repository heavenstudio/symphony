//! The entry's shape, as the Settings page reads it (`symphony_blocks::SettingsEntry` and what it
//! holds): written out here, with the same wire words, so that the adapter links no member of the
//! workspace (`tests/boundary.rs`). Only what the exchange says is here: every setting it shows is
//! the project's and reviewed. Its tests read each answer back as the kit's types and write it
//! again, so a word or a field this shape spells otherwise fails there
//! (`tests/exchange/describe.rs`).

use serde::Serialize;

/// Where a value may be written: the project's committed file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingScope {
    Project,
}

/// Each of its settings is changed by a reviewed task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingClass {
    Reviewed,
}

/// How a value is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Text,
    Number,
    Switch,
    Choice,
}

/// Where a value in force stands: its file and line, or its default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SettingSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<SettingScope>,
    pub said: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// One setting, with its value in force.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Setting {
    pub key: String,
    pub name: String,
    pub kind: SettingKind,
    pub value: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    pub source: SettingSource,
    pub scopes: Vec<SettingScope>,
    pub class: SettingClass,
}

/// Its state and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryState {
    pub word: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    pub why: String,
}

/// A thing it needs, and whether this machine has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryNeed {
    pub what: String,
    pub met: bool,
    pub said: String,
}

/// The exchange's entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SettingsEntry {
    pub id: String,
    pub name: String,
    pub about: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EntryState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<EntryNeed>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
}
