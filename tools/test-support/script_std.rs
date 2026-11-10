//! The fixture scripts (`script.rs`), for a crate whose children all start with plain std spawns:
//! it holds no spawn lock, so a machine copy's first run takes none either.

#[path = "script.rs"]
mod shared;
pub use shared::*;

fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    command.status()
}
