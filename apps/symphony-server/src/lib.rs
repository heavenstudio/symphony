//! The native Symphony server and Dioxus client host.
//!
//! `symphony-server` owns the app's port. There is one per machine: its lock and its address live in
//! `sf-symphony/` in the git common dir ([`home::Home::ensure`], the shape of the board's server in
//! `symphony/crates/tasks` on `tools/rust-port-s3`), so starters at once start one, and every checkout finds it. It serves the main
//! checkout, and every branch's worktree under `/@<tree id>/`. Axum serves the client at `/`,
//! the JSON read and save APIs, media and their shared SSE feed. No interpreter or proxy is used.

mod api;
mod backstop;
pub mod cli;
pub mod development;
mod file_body;
pub mod front;
pub mod gate;
pub mod home;
mod live;
pub mod log;
mod media_files;
pub mod orphans;
pub mod probe;
mod reload;
pub mod repo;
pub mod sys;
mod write;

#[cfg(test)]
mod testing;

#[cfg(test)]
#[path = "../../../../tools/gate/tests/verify/script.rs"]
mod fixture_script;
