//! A project's `docs/` as its docs build checks it, and the Markdown syntax every reader shares.
//!
//! [`check`] reads the documents once ([`Docs`]), runs the generic checks and the project's own
//! [`DocCheck`]s over that reading, and returns every reference that does not resolve, each naming
//! its `file:line`, or else the [`Index`] the build writes its typed tables from. [`syntax`] is how
//! the Symphony reads a document: lines, headings and anchors, fences and code, links, components,
//! and card lines with their entries. [`text`] is Python's text rules a reader of the files keeps
//! (whitespace, lines, a file's text), and [`markup`] the escaping and quoting a page's links follow.
//!
//! One of the plugin kit's three crates, with `symphony-plugin` and `symphony-blocks`
//! (`symphony_plugin`'s doc says what each holds); it depends on no other workspace crate.

#![forbid(unsafe_code)]

mod check;
pub mod markup;
pub mod syntax;
pub mod text;

pub use check::{
    CardInfo, DocCheck, DocInfo, Docs, Index, Problem, check, is_history, prose_lines, runs,
};
