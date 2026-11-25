//! The Symphony library: what the owner says in `docs/`, read and written in the shapes
//! docs/workflow/REVIEW.md sets down.
//!
//! The owner reviews the design documents in the Symphony (`just symphony`), which writes callouts into
//! the text and verdicts into the decision cards. A document has no status: it has versions and
//! decisions waiting. This crate reads those back out ([`register`] for the cards, [`callouts`] for
//! the comments, [`board`] for the whole tree and `just symphony-status`, [`lint`] for its prose rules) and
//! writes the owner's and the agents' words into them ([`edit`]), checking a write never drops a line
//! a card had. An agent hears the owner through save-time native [`callbacks`], which also deliver
//! the owner's [`asks`], the app's ephemeral questions that are never written into a document.
//!
//! On top of the cards: the project's [`plugins`] say what kind of card each is, which picture it
//! shows and for what slot, [`triage`] ranks what the owner should see first
//! (`just symphony-cli triage`), [`verdicts`] reads the cards waiting on an agent, the cards unmerged
//! branches hold among them ([`branches`]), and [`owner_asks`] closes the owner's asks on the board
//! as their answers are read. The app's own board session comes through [`taskcli`], which runs the
//! project's build of the board's binary ([`builds`]).
//!
//! Text rules retain Python's exact meanings ([`text`]): line breaks, whitespace and quoted values
//! preserve byte-for-byte file compatibility.

pub mod artifacts;
pub mod asks;
pub mod assets;
pub mod board;
pub mod branches;
pub mod builds;
pub mod callbacks;
pub mod callouts;
mod channel;
pub mod children;
pub mod cli;
pub mod clock;
pub mod commit;
pub mod dashboard;
pub mod decided;
pub mod docs_audit;
pub mod docview;
pub mod edit;
pub mod entrypoint;
pub mod gates;
pub mod guards;
pub mod images;
pub mod init;
pub mod landing;
pub mod lint;
pub mod machine;
pub mod markup;
pub mod merging;
pub mod notices;
pub mod owner_asks;
pub mod owners;
pub mod page;
pub mod plugins;
pub mod postpone;
pub mod project;
pub mod register;
pub mod reload;
pub mod render;
pub mod renderers;
pub mod reports;
pub mod save;
pub mod settings;
pub use symphony_schema as schema;
pub mod taskcli;
pub mod taskview;
pub mod text;
pub mod theme;
pub mod timeline;
pub mod tree;
pub mod triage;
pub mod uploads;
pub mod verdicts;
pub mod versions;

pub use board::Doc;
pub use callouts::Comment;
pub use edit::EditError;
pub use register::{Decision, Entry, Group, ReviewError, State, Who};
pub mod blocks;
pub mod hook;

/// The temporary project tree the unit tests share with the integration tests.
#[cfg(test)]
#[path = "../tests/common/tree.rs"]
mod test_tree;

/// The git layouts a checkout can have, which the unit tests share with the server's.
#[cfg(test)]
#[path = "../tests/common/layouts.rs"]
mod test_layouts;

#[cfg(test)]
#[path = "../../../tools/test-support/script_board.rs"]
mod fixture_script;
