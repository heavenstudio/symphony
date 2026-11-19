//! A launch that fails after registration releases its handle and the resources it made, and ends
//! the execution attempt it admitted: cancelled, the close of its terminal the answer to the
//! terminate it owes ([`Local::abandon`]).

use super::{Machine, attempts::Local, git, hc::Hc};
use crate::{Error, Store};
use std::path::Path;

pub(super) fn failed(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    handle: &str,
    terminal: &str,
    made: Option<&Path>,
    attempt: Option<(&Local<'_>, &str)>,
    mut problem: Error,
) -> Error {
    let mut incomplete = Vec::new();
    let closing = match attempt {
        Some((local, id)) => {
            let (closing, board) = local.abandon(id, hc, terminal, &problem.to_string());
            if let Err(why) = board {
                incomplete.push(format!("execution attempt {id}: {why}"));
            }
            closing
        }
        None if terminal.is_empty() => Ok(()),
        None => hc.close(terminal).map_err(|why| why.0),
    };
    let closed = match closing {
        Ok(()) => true,
        Err(why) => {
            incomplete.push(format!("terminal {terminal}: {why}"));
            false
        }
    };
    if let Err(why) = store.leave(handle) {
        incomplete.push(format!("release {handle}: {why}"));
    }
    // A terminal whose close failed may still be using the checkout. A caller's checkout is
    // never ours to remove, even when startup or its final board write fails.
    if closed
        && let Some(path) = made.filter(|path| path.exists())
        && let Err(why) = git(
            &machine.root,
            &["worktree", "remove", "--force", &path.to_string_lossy()],
        )
    {
        incomplete.push(format!("worktree {}: {why}", path.display()));
    }
    if !incomplete.is_empty() {
        let (Error::Refused(message) | Error::Db(message)) = &mut problem;
        message.push_str(&format!(
            "; launch cleanup incomplete: {}",
            incomplete.join("; ")
        ));
    }
    problem
}
