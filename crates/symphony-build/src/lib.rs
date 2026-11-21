//! This repository's own build and release commands, `symphony-build`'s (docs/RELEASES.md,
//! docs/CHECKS.md): publishing a built client, the client's release inputs, the size budgets, the
//! crates check, the splitter's repair, the pages' time to synced and the release run
//! ([`release`]). A package of its own, never published and never shipped, so the library every
//! project links (`symphony-app`) compiles none of it; what it reads of the library is the
//! library's public API.

use std::io::Write;

mod commands;
mod release;
mod replace;

/// The temporary project tree the unit tests share with the library's tests.
#[cfg(test)]
#[path = "../../symphony/tests/common/tree.rs"]
mod test_tree;

/// `symphony-build COMMAND ARGS…`, the whole command line without the program name. Its exit code.
pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    commands::main(args, out, err)
}
