//! The seam tests' plugin with one object at every seam (`tests/project/seams/every.rs`), served
//! from a process of its own as `[plugins.process]` starts one: the subject of the tests of a
//! plugin's own process (`tests/project/seams/process.rs`), which find it as
//! `CARGO_BIN_EXE_symphony-test-plugin`. Never a project's plugin.

use std::process::ExitCode;

#[path = "../../tests/project/seams/every.rs"]
mod every;

fn main() -> ExitCode {
    symphony_plugin::serve(&every::EVERY)
}
