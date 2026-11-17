//! How a test starts its fixtures and stand-ins: as it likes, in its own whole environment, session
//! and marks included, past the board's rule that a child is `sys::command`'s (the crate's
//! clippy.toml). A test names a child `Command::of(PROGRAM)`, std's or the board's, and clears its
//! environment with `.cleared()`. The constructors the rule refuses are called here alone, each
//! under an expectation of its own, so clippy reads every test against the rest of what the file
//! lists, the timers of RULES.md `code/no-timed-waits` among them.
//!
//! Each test target and the library's tests take it as the module `children` at their root.

use std::ffi::OsStr;

/// A test's own child: a command made, and an environment cleared, as the test likes.
pub trait Child {
    /// A command for `program`, in this process's whole environment.
    fn of(program: impl AsRef<OsStr>) -> Self;

    /// This command with no environment but what is set on it after.
    #[allow(
        dead_code,
        reason = "a target whose tests clear no child's environment never calls it"
    )]
    fn cleared(&mut self) -> &mut Self;
}

impl Child for std::process::Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
    )]
    fn of(program: impl AsRef<OsStr>) -> Self {
        Self::new(program)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
    )]
    fn cleared(&mut self) -> &mut Self {
        self.env_clear()
    }
}

impl Child for symphony_board::sys::Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
    )]
    fn of(program: impl AsRef<OsStr>) -> Self {
        Self::new(program)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
    )]
    fn cleared(&mut self) -> &mut Self {
        self.env_clear()
    }
}
