//! Where a project's build of a Symphony binary is, found as the recipes `symphony init` installs
//! find it (`shipped/symphony.just`): this checkout's `.target/bin/NAME` link, else its main
//! checkout's (from a linked worktree), else the release the machine runs (`current/bin/NAME` in
//! the machine folder, which a release's install points: [`crate::machine::release`]), else `NAME`
//! on `PATH`. The project's build setup makes the links; nothing here builds. A project with no
//! build of its own so runs the release, and one that builds its own keeps running it. The heartbeat and the memory guard find `symphony` the same way, and
//! the beat the board's binary ([`crate::hook`]). The parts the board shares, for what runs from the
//! main checkout alone, are [`symphony_board::builds`].

use std::path::{Path, PathBuf};

pub use symphony_board::builds::{Beyond, CURRENT, LINKS, link, on_path, released, runnable};

/// `root`'s link to its build of `name`, else, for a linked worktree, its main checkout's.
pub fn linked(root: &Path, name: &str) -> Option<PathBuf> {
    link(root, name).or_else(|| link(&main_checkout(root)?, name))
}

/// The main checkout of the linked worktree at `root`: its common dir less `/.git`, read from the
/// worktree's own marker with no child process. None for a main checkout or no checkout.
pub(crate) fn main_checkout(root: &Path) -> Option<PathBuf> {
    let main = symphony_board::builds::main_of(root);
    (root.canonicalize().ok()? != main.canonicalize().ok()?).then_some(main)
}

/// The project's build of `name`: its link ([`linked`]), else the machine's release's, else `name`
/// on the search path ([`Beyond::find`]).
pub fn find(root: &Path, name: &str, beyond: &Beyond) -> Option<PathBuf> {
    linked(root, name).or_else(|| beyond.find(name))
}

#[cfg(test)]
#[path = "builds_tests.rs"]
mod tests;
