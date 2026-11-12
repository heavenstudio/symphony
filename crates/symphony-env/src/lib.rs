//! Symphony's environment variables, each by its one `SYMPHONY_` name, and the one rule every
//! reader reads them by: an empty value is unset. [`ALL`] lists them, for a test that clears what
//! its child would inherit.

#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};

/// A variable, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Var {
    pub name: &'static str,
}

impl Var {
    pub const fn new(name: &'static str) -> Var {
        Var { name }
    }

    /// Its value as `lookup` answers names (a child's environment, a parsed map), an empty one
    /// being none.
    pub fn get<T: AsRef<OsStr>>(self, lookup: impl FnOnce(&'static str) -> Option<T>) -> Option<T> {
        lookup(self.name).filter(|value| !value.as_ref().is_empty())
    }

    /// This process's value, an empty one being none.
    pub fn os(self) -> Option<OsString> {
        self.get(std::env::var_os)
    }

    /// [`Var::os`] as text: none when the value is not Unicode, as `std::env::var` has it.
    pub fn text(self) -> Option<String> {
        self.os()?.into_string().ok()
    }
}

/// The SurrealDB the board's server runs, in place of the one pinned in the git common dir.
pub const SURREAL: Var = Var::new("SYMPHONY_SURREAL");
/// The git common dir `tools/surreal.sh` and the client's build scripts use; the shell reads it.
pub const GIT_COMMON_DIR: Var = Var::new("SYMPHONY_GIT_COMMON_DIR");
/// The Chrome `symphony shot` and the browser tests drive.
pub const CHROME: Var = Var::new("SYMPHONY_CHROME");
/// The browser and profile a Chrome leash owns, set by the program that runs itself as one.
pub const CHROME_LEASH: Var = Var::new("SYMPHONY_CHROME_LEASH");
/// The client a foreground server serves, in place of the embedded one.
pub const UI_DIST: Var = Var::new("SYMPHONY_UI_DIST");
/// The one server's port, a test's own.
pub const PORT: Var = Var::new("SYMPHONY_PORT");
/// Times the server's stages.
pub const PROFILE: Var = Var::new("SYMPHONY_PROFILE");
/// Says each stage of a server's start, when `1`.
pub const STARTUP_TRACE: Var = Var::new("SYMPHONY_STARTUP_TRACE");
/// Says each stage of a verdict's publication.
pub const PUBLICATION_TRACE: Var = Var::new("SYMPHONY_PUBLICATION_TRACE");
/// The listeners a serving server hands its successor.
pub const HANDOVER: Var = Var::new("SYMPHONY_HANDOVER");
/// The test whose end a server it started ends with.
pub const TEST_PID: Var = Var::new("SYMPHONY_TEST_PID");
/// A project's machine-work reader, in place of its prepared one.
pub const MACHINE_WORK_READER: Var = Var::new("SYMPHONY_MACHINE_WORK_READER");
/// Another command run in place of the board's `tasks` binary.
pub const TASKS_CLI: Var = Var::new("SYMPHONY_TASKS_CLI");
/// The worktree sweep's window of recent activity, in hours.
pub const SWEEP_RECENT_HOURS: Var = Var::new("SYMPHONY_SWEEP_RECENT_HOURS");
/// Where a launch makes new worktrees.
pub const WT_DIR: Var = Var::new("SYMPHONY_WT_DIR");
/// The checkout a hook command runs for.
pub const HOOK_ROOT: Var = Var::new("SYMPHONY_HOOK_ROOT");
/// The heartbeat hook's clock, a test's file.
pub const HOOK_BEAT_CLOCK: Var = Var::new("SYMPHONY_HOOK_BEAT_CLOCK");
/// The source root a read audit reports under.
pub const READ_AUDIT_ROOT: Var = Var::new("SYMPHONY_READ_AUDIT_ROOT");
/// The report a read audit appends to.
pub const READ_AUDIT_REPORT: Var = Var::new("SYMPHONY_READ_AUDIT_REPORT");
/// The free gigabytes under which the board and the pages say the disk runs low.
pub const LOW_DISK_GB: Var = Var::new("SYMPHONY_LOW_DISK_GB");

/// Every variable above.
pub const ALL: [Var; 20] = [
    SURREAL,
    GIT_COMMON_DIR,
    CHROME,
    CHROME_LEASH,
    UI_DIST,
    PORT,
    PROFILE,
    STARTUP_TRACE,
    PUBLICATION_TRACE,
    HANDOVER,
    TEST_PID,
    MACHINE_WORK_READER,
    TASKS_CLI,
    SWEEP_RECENT_HOURS,
    WT_DIR,
    HOOK_ROOT,
    HOOK_BEAT_CLOCK,
    READ_AUDIT_ROOT,
    READ_AUDIT_REPORT,
    LOW_DISK_GB,
];

/// Whether `name` is one of Symphony's variables: for a test that clears what its child would
/// inherit.
pub fn named(name: &str) -> bool {
    ALL.iter().any(|var| var.name == name)
}

#[cfg(test)]
mod tests;
