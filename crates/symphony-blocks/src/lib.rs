//! The block vocabulary of the Symphony's plugin kit: what a plugin renders to and what its seams
//! return, as the server sends it and the client draws it. Document content as data ([`Block`],
//! [`Span`] and what they hold, never rendered HTML), what a page draws that markup cannot
//! ([`Media`] and its [`Viewer`]), a board field's value ([`FieldValue`]), a task's gate ([`Gate`]),
//! a home region ([`Region`]), a card's pictures ([`Picture`]), a navigation entry ([`NavItem`]), a
//! board action's offer ([`TaskOffer`]) and a Settings page's entry ([`SettingsEntry`]). Serde
//! only, so one definition builds for the server and a plugin's process, and is written out as the
//! client's TypeScript.
//!
//! The kit is `symphony-plugin`, this crate and `symphony-docs`, one version for the three
//! (`symphony_plugin`'s doc says what each holds); this crate depends on no other workspace crate.
//! `symphony_plugin::vocabulary` re-exports it whole, so everything public here is the kit's
//! public vocabulary, and a type made public here is a published addition.
//! The app's own wire types, which move with the server and the client, are `symphony-api`'s, which
//! re-exports these under the same names.
//!
//! The lists grow in a minor of the kit: a new kind of [`Block`], [`Span`] or [`Media`], a new
//! word one carries ([`CalloutKind`], [`FigureSize`], [`ContainerKind`], [`InlineKind`],
//! [`VectorKind`], [`CardState`], [`CardGroup`], [`ThreadAuthor`]), a new [`GateState`] or a new
//! word a setting carries ([`SettingScope`], [`SettingClass`], [`SettingKind`]) is no new major.
//! So each is `#[non_exhaustive]`: a plugin makes a variant as before, and a match on one outside
//! this crate ends with `_`. [`FieldValue`] alone is matched whole: a number or text,
//! as JSON has them. A struct here is built by its fields, so a field added to one is a new major.
//!
//! ```
//! // A plugin's match names the kinds it knows and leaves the rest, a later minor's too, to `_`.
//! use symphony_blocks::Media;
//! fn label(media: &Media) -> &str {
//!     match media {
//!         Media::Mockup { label, .. } | Media::Viewer { label, .. } => label,
//!         _ => "",
//!     }
//! }
//! let mockup = Media::Mockup { src: "a.html".into(), label: "A".into() };
//! assert_eq!(label(&mockup), "A");
//! ```
//!
//! ```compile_fail,E0004
//! // Without the `_`, the match is refused outside this crate, though it names every kind.
//! use symphony_blocks::Media;
//! fn kind(media: &Media) -> &str {
//!     match media {
//!         Media::Mockup { .. } => "mockup",
//!         Media::Viewer { .. } => "viewer",
//!         Media::Gallery { .. } => "gallery",
//!     }
//! }
//! ```

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod blocks;
mod field_value;
mod gates;
mod media;
mod settings;

pub use blocks::*;
pub use field_value::FieldValue;
pub use gates::{Gate, GatePast, GateState};
pub use media::{GalleryItem, Media, View, Viewer};
pub use settings::{
    EntryNeed, EntryState, Setting, SettingClass, SettingKind, SettingScope, SettingSource,
    SettingsEntry,
};

/// One picture a card shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Picture {
    /// Where the picture is served; `None` for an item not drawn yet ("picture coming").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// The candidate's letter ("A"), or the item's name.
    pub caption: String,
    /// The picture still waits on the owner's review.
    #[serde(default, skip_serializing_if = "is_false")]
    pub waiting: bool,
}

/// A region of the home a plugin adds: a headline over bars of done, waiting and remaining work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Region {
    /// The region's id among the project's regions, stable across snapshots.
    pub id: String,
    pub title: String,
    /// The line under the headline's percentage (`21 of 31 items approved`).
    pub note: String,
    pub bars: Vec<ProgressBar>,
    /// The legend's words for done, waiting and remaining, in that order.
    pub legend: [String; 3],
}

/// One bar of a home region: how much of `total` is done, waiting, and (the rest) remaining.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProgressBar {
    pub key: String,
    pub name: String,
    /// Where the bar links: a project-relative path.
    pub page: String,
    /// What the bar says on hover.
    pub title: String,
    pub done: u32,
    pub waiting: u32,
    pub total: u32,
}

/// What a project's board action offers the owner on a task: a button named `label` beside the
/// task's `branch`, shown while there is a `payload`, which the button sends back to the action
/// `action` as it came (approve to land: the exact patch to land).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskOffer {
    /// The board action's name.
    pub action: String,
    pub label: String,
    pub branch: String,
    pub payload: Option<String>,
}

/// A plugin's entry in the header and in the home's links.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct NavItem {
    /// The header's word (`▤ shelf`), drawn as the plugin gives it, after the Symphony's own
    /// (`symphony_api::page_names`).
    pub label: String,
    /// The home's link text (`Shelf items`).
    pub title: String,
    /// The path it opens (`/shelf`), a client page.
    pub href: String,
    /// The document that page shows, a project-relative path.
    pub document: String,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}
