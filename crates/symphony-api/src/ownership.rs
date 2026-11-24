//! Which group a card or a task belongs to, as a page draws it (docs/workflow/TASKS.md, "Which
//! group a thing belongs to"): its group or person, what gave it, and whom the one reading may
//! hand it to. A task's page carries its [`Owner`] ([`crate::TaskPage::owner`]); `GET`
//! [`CARD_OWNERS_PATH`]`?path=DOC` answers a document's [`CardOwners`]. Neither is said on a board
//! that names nobody, where every thing is the person at the machine's and the page draws nothing.
//!
//! `POST` [`TASK_HAND_ROUTE`] (`{slug, to}`) and [`CARD_HAND_ROUTE`] (`{path, slug, to}`) hand a
//! thing on, by a member of its group: the task's name written on it with the board's line on its
//! thread, the card's `*For:*` in its file, each kept in the board's log with who did.

use serde::{Deserialize, Serialize};

/// A document's cards' owners ([`CardOwners`]), `?path=DOC`.
pub const CARD_OWNERS_PATH: &str = "/api/card-owners";

/// A task handed to another group or person: `{slug, to}`.
pub const TASK_HAND_ROUTE: &str = "/api/task-hand";

/// A card handed to another group or person: `{path, slug, to}`.
pub const CARD_HAND_ROUTE: &str = "/api/card-hand";

/// Whose a thing is, as the one reading sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Owner {
    /// The group's or the person's short name, or `owner` for the person at the machine.
    pub name: String,
    /// `group`, `person` or `machine`.
    pub kind: String,
    /// What gave it: `named` (a name written on it), `map` or `default`.
    pub by: String,
    /// What gave it, as words a page shows beside the name: `named on it`, `from the area app`.
    pub said: String,
    /// Whom the one reading may hand it to, groups then people: empty where they may not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hand_to: Vec<String>,
}

/// The owners of a document's cards, by slug: empty on a board that names nobody.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct CardOwners {
    pub cards: std::collections::BTreeMap<String, Owner>,
}
