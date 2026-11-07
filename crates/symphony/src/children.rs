//! Every process the Symphony starts, from the server or this library, runs without the session
//! that started the Symphony: an agent that starts it from its terminal hands it the board handle
//! it acts as and the hc terminal and core it runs in. A child that kept hc's and ran `hc run`
//! would start a terminal under that one, which ends with it (`hc --skill`).

use std::ffi::OsStr;
use symphony_board::sys::Command;

/// What ties a process to its starter's session: the board handle (`TASKS_SESSION`), and hc's
/// terminal, its socket, the core and the scope.
pub const SESSION_VARS: [&str; 5] = [
    "TASKS_SESSION",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
    "HC_SESSION",
    "HC_SCOPE",
];

/// `command` keeps none of [`SESSION_VARS`] from this process's environment, nor any its caller
/// set on it before. A shared launcher calls it on every command it is handed.
pub fn sessionless(command: &mut Command) -> &mut Command {
    for name in SESSION_VARS {
        command.env_remove(name);
    }
    command
}

/// A command for `program` that runs without its starter's session. What the caller sets on it
/// after, such as the `TASKS_SESSION` of the actor a board command runs as, it keeps.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    sessionless(&mut command);
    command
}

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;
