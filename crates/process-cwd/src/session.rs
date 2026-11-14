//! What ties a process to the one that started it, which no child it starts keeps: the session an
//! agent's terminal hands every command it runs ([`SESSION_VARS`]) and the marks of how that one
//! command was asked for ([`MARKS`]). The board builds every child it starts without them
//! (`symphony_board::sys::command`, which re-exports these names), and a plugin's process starts
//! its own the same way, `unmarked(sessionless(&mut Command::new(program)))`, without linking the
//! board.
//!
//! A child that kept hc's terminal and ran `hc run` would start a terminal under that one, which
//! ends with it (`hc --skill`), and an hc call would answer for that core and scope alone. A child
//! is the project's command or the board's own work, never the one command an agent handed a path
//! to, or the land the `land` recipe asked for.

use crate::spawn::Command;

/// What ties a process to its starter's session: the board handle (`TASKS_SESSION`), and hc's
/// terminal, its socket, the core and the scope.
pub const SESSION_VARS: [&str; 5] = [
    "TASKS_SESSION",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
    "HC_SESSION",
    "HC_SCOPE",
];

/// `command` keeps none of [`SESSION_VARS`], from this process's environment, from one it was
/// given, or set on it before. A launcher handed a command calls it on every one.
pub fn sessionless(command: &mut Command) -> &mut Command {
    for name in SESSION_VARS {
        command.env_remove(name);
    }
    command
}

/// The marker the recipes an agent runs unasked set (the board's `confine::MARKER`), under which
/// that one command writes only into its checkout. A child that kept it would refuse its own writes
/// outside that checkout, a test's in its temporary folder among them.
pub const UNASKED: &str = "SYMPHONY_UNASKED";

/// The `land` recipe's mark (the board's `land::RECIPE`), without which `tasks land` refuses. A
/// child that kept it could land.
pub const LAND: &str = "SYMPHONY_LAND";

/// What marks how a process was asked for, which no child of it is: [`UNASKED`] and [`LAND`].
pub const MARKS: [&str; 2] = [UNASKED, LAND];

/// `command` keeps none of [`MARKS`], from this process's environment, from one it was given, or
/// set on it before.
pub fn unmarked(command: &mut Command) -> &mut Command {
    for name in MARKS {
        command.env_remove(name);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::output;

    /// What a child of `command` holds of the session, the marks and one variable of neither, `|`
    /// between, run by `sh`.
    fn saw(command: &mut Command) -> String {
        let said = "printf '%s|%s|%s|%s|%s|%s|%s|%s' \"${TASKS_SESSION-unset}\" \
                    \"${HC_TERMINAL_ID-unset}\" \"${HC_TERMINAL_SOCKET-unset}\" \
                    \"${HC_SESSION-unset}\" \"${HC_SCOPE-unset}\" \"${SYMPHONY_UNASKED-unset}\" \
                    \"${SYMPHONY_LAND-unset}\" \"${PROCESS_CWD_KEPT-unset}\"";
        let done = output(command.args(["-c", said])).unwrap();
        assert!(done.status.success(), "{done:?}");
        String::from_utf8(done.stdout).unwrap()
    }

    /// A command for `sh` with a session, both marks and one variable of neither set on it, in an
    /// environment of only those when `cleared`, else in this process's: what an agent's terminal
    /// hands a command the unasked recipe runs.
    fn started_in(cleared: bool) -> Command {
        let mut command = Command::new("/bin/sh");
        if cleared {
            command.env_clear();
        }
        command
            .envs(SESSION_VARS.map(|name| (name, "s")))
            .envs(MARKS.map(|name| (name, "1")))
            .env("PROCESS_CWD_KEPT", "kept");
        command
    }

    fn started() -> Command {
        started_in(true)
    }

    #[test]
    fn a_child_keeps_neither_the_session_nor_the_marks_once_each_is_removed() {
        assert_eq!(saw(&mut started()), "s|s|s|s|s|1|1|kept");
        assert_eq!(
            saw(sessionless(&mut started())),
            "unset|unset|unset|unset|unset|1|1|kept"
        );
        assert_eq!(saw(unmarked(&mut started())), "s|s|s|s|s|unset|unset|kept");
        assert_eq!(
            saw(unmarked(sessionless(&mut started()))),
            "unset|unset|unset|unset|unset|unset|unset|kept"
        );
        // Set on a command of this process's environment, removed as well.
        assert_eq!(
            saw(unmarked(sessionless(&mut started_in(false)))),
            "unset|unset|unset|unset|unset|unset|unset|kept"
        );
    }

    #[test]
    fn what_a_caller_sets_after_the_removal_is_kept() {
        let mut command = started();
        unmarked(sessionless(&mut command)).env("TASKS_SESSION", "an-actor");
        assert_eq!(
            saw(&mut command),
            "an-actor|unset|unset|unset|unset|unset|unset|kept"
        );
    }
}
