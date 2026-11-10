//! The fixture scripts (`script.rs`), for a crate that starts its children through the board's
//! `sys`, which takes process-cwd's spawn lock (`crates/process-cwd/src/spawn.rs`).

#[path = "script.rs"]
mod shared;
pub use shared::*;

/// A machine copy's first run starts under the lock every std spawn here takes, so the child never
/// copies a descriptor a sibling thread has not yet made close-on-exec.
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    symphony_board::sys::Spawning::status_locked(command)
}
