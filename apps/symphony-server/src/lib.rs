//! The native Symphony server, which serves the web client it embeds.
//!
//! `symphony-server` owns the app's port. There is one per machine: its record, lock, log and the
//! registry of the projects it serves live in the machine folder ([`home::Home::ensure`],
//! `symphony::machine`), so starters at once start one, and every project's checkouts find it. It
//! serves each registered project under `/<id>/` and each project's worktrees under
//! `/<id>/@<tree id>/` ([`projects`]); which build serves them is the host rule ([`host`]). Axum
//! serves the client, the JSON read and save APIs, media and their shared SSE feed. No
//! interpreter or proxy is used.

mod api;
mod attempts;
mod backstop;
pub mod binary;
pub mod cli;
mod client;
pub mod execution;
mod file_body;
pub mod front;
pub mod gate;
mod held;
pub mod home;
pub mod host;
pub mod hosted;
mod live;
pub mod log;
mod mark;
mod media_files;
mod pages;
pub mod probe;
mod projects;
mod reload;
pub mod repo;
mod session_go;
pub mod shelf;
mod shell;
mod spawn;
mod starter;
pub mod sys;
mod write;

#[cfg(test)]
mod testing;

#[cfg(test)]
#[path = "../../../tools/test-support/script_board.rs"]
mod fixture_script;
