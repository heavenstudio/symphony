//! The fixture scripts (`script.rs`), for a crate whose children start under process-cwd's spawn
//! lock (`crates/process-cwd/src/spawn.rs`, `spawning`).

#[path = "script.rs"]
mod shared;
pub use shared::*;

#[cfg(test)]
#[path = "script_tests.rs"]
#[expect(
    clippy::duplicate_mod,
    reason = "adapter tests bind the shared source to both locked and standard spawn policies"
)]
mod tests;

/// A machine copy's first run starts under the lock every std spawn here takes, so the child never
/// copies a descriptor a sibling thread has not yet made close-on-exec.
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    process_cwd::spawn::Spawning::status_locked(command)
}
