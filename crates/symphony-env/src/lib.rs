//! Symphony's environment variables, each read by its own `SYMPHONY_` name and then, for the
//! bridge the names card approved, by the `SF_` name it had in Star Foundry
//! (docs/analysis/sf-names.md#Variables). A process that finds a value under an old name says so
//! once per variable, on its stderr; a command that prints nothing (a hook) reads [`Var::quiet`].
//! The release after both registered projects have moved stops reading [`Var::old`]; every reader
//! keeps reading through here.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::sync::{Mutex, PoisonError};

/// A variable's name, and the old name read after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Var {
    pub name: &'static str,
    pub old: &'static str,
}

/// Which of a variable's two names gave its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    New,
    Old,
}

impl Var {
    pub const fn new(name: &'static str, old: &'static str) -> Var {
        Var { name, old }
    }

    /// Both names, its own first: for a list that keeps or passes the variable on, or a test that
    /// clears it.
    pub const fn names(self) -> [&'static str; 2] {
        [self.name, self.old]
    }

    /// What `lookup` finds under the variable's name, else under its old one, and which name gave
    /// it. What counts as set is the lookup's: an empty value it answers wins, and one it reads as
    /// unset lets the old name answer.
    pub fn pick<T>(self, mut lookup: impl FnMut(&'static str) -> Option<T>) -> Option<(T, Named)> {
        if let Some(value) = lookup(self.name) {
            return Some((value, Named::New));
        }
        lookup(self.old).map(|value| (value, Named::Old))
    }

    /// [`Var::pick`]'s value, said once on this process's stderr when the old name gave it.
    pub fn read<T>(self, lookup: impl FnMut(&'static str) -> Option<T>) -> Option<T> {
        self.read_noting(lookup, &SAID, &mut std::io::stderr())
    }

    /// [`Var::read`] with the notices and the writer named: said to `out` the first time
    /// `notices` meet the old name.
    pub fn read_noting<T>(
        self,
        lookup: impl FnMut(&'static str) -> Option<T>,
        notices: &Notices,
        out: &mut dyn Write,
    ) -> Option<T> {
        let (value, named) = self.pick(lookup)?;
        if named == Named::Old {
            notices.say(self, out);
        }
        Some(value)
    }

    /// [`Var::pick`]'s value without a word, for a command that prints nothing.
    pub fn quiet<T>(self, lookup: impl FnMut(&'static str) -> Option<T>) -> Option<T> {
        self.pick(lookup).map(|(value, _)| value)
    }

    /// This process's value, either name read as `std::env::var_os` reads one: set to anything,
    /// an empty value too.
    pub fn os(self) -> Option<OsString> {
        self.read(std::env::var_os)
    }

    /// [`Var::os`] as text: none when the value is not Unicode, as `std::env::var` has it.
    pub fn text(self) -> Option<String> {
        self.os()?.into_string().ok()
    }

    /// What a process that read the old name says.
    pub fn notice(self) -> String {
        let Var { name, old } = self;
        format!("{old} is the old name of {name} and will stop being read: set {name} instead")
    }
}

/// The variables a process has said it read by their old names, so that each is said once.
#[derive(Debug, Default)]
pub struct Notices(Mutex<BTreeSet<&'static str>>);

impl Notices {
    pub const fn new() -> Notices {
        Notices(Mutex::new(BTreeSet::new()))
    }

    /// Writes `var`'s notice to `out` the first time these notices meet it; whether it did. A
    /// notice that cannot be written is not tried again.
    pub fn say(&self, var: Var, out: &mut dyn Write) -> bool {
        let first = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(var.old);
        if first {
            let _ = writeln!(out, "{}", var.notice());
        }
        first
    }
}

/// This process's notices.
static SAID: Notices = Notices::new();

/// The SurrealDB the board's server runs, in place of the one pinned in the git common dir.
pub const SURREAL: Var = Var::new("SYMPHONY_SURREAL", "SF_SURREAL");
/// The git common dir `tools/surreal.sh` and the client's build scripts use; the shell reads it.
pub const GIT_COMMON_DIR: Var = Var::new("SYMPHONY_GIT_COMMON_DIR", "SF_GIT_COMMON_DIR");
/// The Chrome `symphony shot` and the browser tests drive.
pub const CHROME: Var = Var::new("SYMPHONY_CHROME", "SF_CHROME");
/// The browser and profile a Chrome leash owns, set by the program that runs itself as one.
pub const CHROME_LEASH: Var = Var::new("SYMPHONY_CHROME_LEASH", "SF_SYMPHONY_CHROME_LEASH");
/// The client a foreground server serves, in place of the embedded one.
pub const UI_DIST: Var = Var::new("SYMPHONY_UI_DIST", "SF_SYMPHONY_UI_DIST");
/// The one server's port, a test's own.
pub const PORT: Var = Var::new("SYMPHONY_PORT", "SF_SYMPHONY_PORT");
/// Times the server's stages.
pub const PROFILE: Var = Var::new("SYMPHONY_PROFILE", "SF_SYMPHONY_PROFILE");
/// Says each stage of a server's start, when `1`.
pub const STARTUP_TRACE: Var = Var::new("SYMPHONY_STARTUP_TRACE", "SF_SYMPHONY_STARTUP_TRACE");
/// Says each stage of a verdict's publication.
pub const PUBLICATION_TRACE: Var = Var::new(
    "SYMPHONY_PUBLICATION_TRACE",
    "SF_SYMPHONY_PUBLICATION_TRACE",
);
/// The listeners a serving server hands its successor, which it sets under both names.
pub const HANDOVER: Var = Var::new("SYMPHONY_HANDOVER", "SF_SYMPHONY_HANDOVER");
/// The test whose end a server it started ends with.
pub const TEST_PID: Var = Var::new("SYMPHONY_TEST_PID", "SF_SYMPHONY_TEST_PID");
/// A project's machine-work reader, in place of its prepared one.
pub const MACHINE_WORK_READER: Var =
    Var::new("SYMPHONY_MACHINE_WORK_READER", "SF_MACHINE_WORK_READER");
/// Another command run in place of the board's `tasks` binary.
pub const TASKS_CLI: Var = Var::new("SYMPHONY_TASKS_CLI", "SF_TASKS_CLI");
/// The worktree sweep's window of recent activity, in hours.
pub const SWEEP_RECENT_HOURS: Var =
    Var::new("SYMPHONY_SWEEP_RECENT_HOURS", "SF_SWEEP_RECENT_HOURS");
/// Where a launch makes new worktrees.
pub const WT_DIR: Var = Var::new("SYMPHONY_WT_DIR", "SF_WT_DIR");
/// The checkout a hook command runs for.
pub const HOOK_ROOT: Var = Var::new("SYMPHONY_HOOK_ROOT", "SF_HOOKS_ROOT");
/// The heartbeat hook's clock, a test's file.
pub const HOOK_BEAT_CLOCK: Var = Var::new("SYMPHONY_HOOK_BEAT_CLOCK", "SF_HOOKS_BEAT_CLOCK");
/// The source root a read audit reports under.
pub const READ_AUDIT_ROOT: Var = Var::new("SYMPHONY_READ_AUDIT_ROOT", "SF_READ_AUDIT_ROOT");
/// The report a read audit appends to.
pub const READ_AUDIT_REPORT: Var = Var::new("SYMPHONY_READ_AUDIT_REPORT", "SF_READ_AUDIT_REPORT");

/// Every variable above.
pub const ALL: [Var; 19] = [
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
];

/// Whether `name` is one of Symphony's variables by either name: for a test that clears what its
/// child would inherit.
pub fn named(name: &str) -> bool {
    ALL.iter().any(|var| var.names().contains(&name))
}

#[cfg(test)]
mod tests;
