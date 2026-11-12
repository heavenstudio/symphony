//! The seam tests' plugin (`tests/project/seams/every.rs`) as a process serves it, its seams a
//! process answers alone (`every::PIPED`), started as `[plugins.process]` starts one: the subject of
//! the tests of a plugin's own process (`tests/project/seams/process.rs`), which find it as
//! `CARGO_BIN_EXE_symphony-test-plugin`. Never a project's plugin.

use std::process::ExitCode;

#[path = "../../tests/project/seams/every.rs"]
mod every;

fn main() -> ExitCode {
    symphony_plugin::serve(&every::PIPED)
}
