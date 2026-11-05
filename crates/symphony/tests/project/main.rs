//! A project: its `symphony.toml` over the Symphony's defaults, and the plugins it names, asked at
//! each seam.

#[path = "../common/mod.rs"]
mod common;

/// The task store's test server, as its own tests run on it: a database of its own per test.
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;

mod entrypoints;
mod fields;
mod folders;
mod manifest;
mod offers;
mod seams;

use common::Tree;
