//! A project's `docs/` as its docs build checks it, and the Markdown syntax every reader shares.
//!
//! [`check`] reads the documents once ([`Docs`]), runs the generic checks and the project's own
//! [`DocCheck`]s over that reading, and returns every reference that does not resolve, each naming
//! its `file:line`, or else the [`Index`] the build writes its typed tables from. [`syntax`] is how
//! the Symphony reads a document: lines, headings and anchors, fences and code, links, components,
//! and card lines with their entries.

#![forbid(unsafe_code)]

mod check;
pub mod syntax;

pub use check::{
    CardInfo, DocCheck, DocInfo, Docs, Index, Problem, check, is_history, prose_lines, runs,
};
