//! How the Symphony reaches a project's board to read it, and how a failed read is told.

use std::path::Path;

/// How long the server waits on each board request: its board tick shares the data worker's thread
/// with every document refresh, and a page gives up on an answer after 15 s (`symphony-ui`'s
/// `request.rs`). A read with no whole answer in that time goes once more
/// ([`symphony_board::rpc::Kind`]), and so waits at most twice this.
const BOARD_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// A page read's board failure: a board that took the request and answered too late is slow (504),
/// any other failure 503, said after `unreachable` (the words a route puts before it).
pub fn read_failure(error: &str, unreachable: &str) -> (u16, String) {
    if symphony_board::rpc::slow(error) {
        (504, format!("the task board is slow: {error}"))
    } else {
        (503, format!("{unreachable}{error}"))
    }
}

/// The board of the project at `root`, as this process's environment reaches it.
pub fn connect(root: &Path) -> Result<symphony_board::Store, String> {
    connect_in(root, crate::children::board_env())
}

/// [`connect`] to a board whose server is running, never starting one: for a read that can go
/// without the board, as the branch scan's [`crate::branches::branch_cards`] does.
pub fn connect_running(root: &Path) -> Result<symphony_board::Store, String> {
    open(
        root,
        crate::children::board_env(),
        symphony_board::cli::running_board,
    )
}

/// The board of the project at `root` as `env` reaches it, opened to read by the schema rule: a
/// board one version ahead is read and never written, and a write on it is refused naming the
/// project and `just symphony`; a board behind this build, or not made yet, is refused before it is
/// read, naming `just tasks`, and never moved forward or made. So a branch's build, its command
/// line or a preview server, cannot move the live board past the build its board command runs.
pub fn connect_in(
    root: &Path,
    env: symphony_board::cli::Env,
) -> Result<symphony_board::Store, String> {
    open(root, env, symphony_board::cli::board)
}

/// The board `reach` finds for the project at `root` under `env`, opened to read as [`connect_in`]
/// says.
fn open(
    root: &Path,
    mut env: symphony_board::cli::Env,
    reach: fn(&symphony_board::cli::Env) -> Result<symphony_board::cli::Reached, String>,
) -> Result<symphony_board::Store, String> {
    env.cwd = root.to_path_buf();
    // A read-only web request is never a board session or a launcher.
    env.vars.remove("TASKS_SESSION");
    let reached = reach(&env)?;
    let client = reached.client.waiting(BOARD_WAIT);
    let how = symphony_board::Opening::read()
        .of(reached.project.as_deref())
        .live(reached.live);
    let store = match env.vars.get("TASKS_NOW") {
        Some(text) => {
            let now = text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("TASKS_NOW={text} is not a time in seconds"))?;
            symphony_board::Store::opening(
                client,
                Box::new(move || now),
                Box::new(symphony_board::sys::pid_alive),
                &symphony_board::sys::hostname(),
                &how,
            )
        }
        None => symphony_board::Store::open_as(client, &how),
    }
    .map_err(|e| e.to_string())?;
    // The cap the command line holds starts to, which the agents view and the PULL digest show.
    Ok(symphony_board::cli::capped(store, &env))
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;
