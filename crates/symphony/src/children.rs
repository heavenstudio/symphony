//! Every process the Symphony starts, from the server or this library, runs without the session
//! that started the Symphony: an agent that starts it from its terminal hands it the board handle
//! it acts as and the hc terminal and core it runs in. A child that kept hc's and ran `hc run`
//! would start a terminal under that one, which ends with it (`hc --skill`).
//!
//! The names, [`sessionless`] and [`command`] are the board's ([`symphony_board::sys`]), which
//! starts its own children the same way: this crate depends on the board, so the one list is there,
//! from the leaf a plugin's process starts its children with (`process_cwd::session`). So is
//! [`git`], a git on the repository of the folder it runs in alone, whatever repository or index
//! the environment names (a git hook's `GIT_DIR` and `GIT_INDEX_FILE`).

use std::io;
use std::process::Output;
use symphony_board::cli::Env;
use symphony_board::sys::Command;

pub use symphony_board::sys::{SESSION_VARS, command, git, sessionless};

/// What `command` printed, run to its end ([`symphony_board::sys::output`]) without its starter's
/// session: the runner for a command the board builds and hands back to run (its `lsof`).
pub fn output(command: &mut Command) -> io::Result<Output> {
    symphony_board::sys::output(sessionless(command))
}

/// This process's environment as the board reads it, without [`SESSION_VARS`]: what a board call
/// made in this process hands to the children the board starts with that environment alone (the
/// hc probe of the agents view).
pub fn board_env() -> Env {
    let mut env = Env::of_process();
    for name in SESSION_VARS {
        env.vars.remove(name);
    }
    env
}

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;
