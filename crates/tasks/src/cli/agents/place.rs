//! Where an agent works, as the agents view compares it: a worktree's path resolved as Python's
//! `os.path.realpath` resolves it ([`resolve`]), and what a quiet in-process agent waits on there.
//!
//! An in-process agent (a subagent: no terminal, no monitor, its tool calls its only heartbeat) gone
//! quiet with no run of its own, in a linked worktree its binding recorded, is waiting on what it
//! started there when a live process of its own works in that worktree (its nearest checkout is
//! that one, never one beneath it): `running_test`, `{"recipe": "process", "phase": "running"}`,
//! and live. A process is its own when it began while the agent was there: not before its binding
//! recorded that worktree, nor after its last tool call, so a server or shell another started there
//! keeps nobody alive. When the board cannot tell (the machine's processes cannot be listed, or one
//! working there may be the agent's but its start cannot be read, or the clock went back while the
//! agent was there and the process began in the time that leaves in doubt): `running_test` with
//! `"phase": "unknown"`, health `unknown`, so it is not called lost either. With none working there
//! that can be its it ages as before ([`waited_on`]).

use super::{Answer, Detail, Probes};
use crate::Store;
use crate::health::Health;
use std::path::{Path, PathBuf};

/// How long a long-lived reader keeps the machine's process listing ([`Store::kept_scan`]): a
/// process that ended reads gone within this, far inside the ten minutes before lost.
pub const PROCESS_SCAN_SECONDS: f64 = 20.0;
/// The recipe of a wait the view sees as a process working in an in-process agent's worktree.
pub const PROCESS: &str = "process";
/// How far a process's start may fall outside the agent's stay and still be its: the kernel stamps
/// a start and the board reads its own clock apart, and Linux names its boot in whole seconds.
const START_SLACK_SECONDS: f64 = 1.0;

/// A live process the machine lists: where it works and when it started, by the board's clock (none
/// when the kernel could not say: it may be anyone's, the agent's working there included).
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub cwd: PathBuf,
    pub started: Option<f64>,
}

impl<'a> Probes<'a> {
    /// These probes, the process listing kept by `store` ([`Store::kept_scan`]): a long-lived reader's,
    /// which views its agents every second or so.
    pub fn kept_by(self, store: &'a Store) -> Probes<'a> {
        let processes = self.processes;
        Probes {
            processes: Box::new(move || store.kept_scan(&*processes)),
            ..self
        }
    }
}

/// `path` with its symbolic links resolved, as Python's `os.path.realpath`: what does not exist is kept
/// as written beneath the part that does. "" stays "".
pub(super) fn resolve(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let given = Path::new(path);
    let whole = if given.is_absolute() {
        given.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(given)
    };
    let mut rest = Vec::new();
    let mut at: &Path = &whole;
    loop {
        if let Ok(found) = at.canonicalize() {
            let mut out: PathBuf = found;
            out.extend(rest.iter().rev());
            return out.to_string_lossy().into_owned();
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                at = parent;
            }
            _ => return whole.to_string_lossy().into_owned(),
        }
    }
}

/// Whether a process whose working directory is `cwd` works in the worktree whose top folder is
/// `tree`: `cwd` is that folder or below it, and no checkout lies between (a `.git` beneath `tree`
/// is another repository's, a scratch one or a nested worktree).
fn works_in(cwd: &Path, tree: &Path) -> bool {
    cwd.starts_with(tree)
        && cwd
            .ancestors()
            .take_while(|folder| *folder != tree)
            .all(|folder| !folder.join(".git").exists())
}

/// An in-process agent's stay in the checkout its binding recorded, by the board's clock.
#[derive(Debug, Clone, Copy)]
pub(super) struct Stay {
    /// When its binding recorded that checkout (`placed_at`).
    pub came: f64,
    /// The earliest time a call of its there read: `came`, unless the clock went back while it was
    /// there (`stay_from`).
    pub from: f64,
    /// Its last tool call.
    pub last: f64,
}

/// What the machine's processes (`scan`, listed once a view) say of an in-process agent gone quiet
/// in `tree` (resolved) after `stay` there: its health and what it waits on. Live on a process
/// working there that began in its stay; unknown when they cannot be listed, or when one working
/// there may be its but cannot be told to be: its start cannot be read, or it began while the clock
/// had gone back (after the earliest time a call of its there read, before it came). None when
/// `tree` is no linked worktree (the main checkout, which every agent registered there shares, or
/// a folder in no repository) or no process working there can be its.
pub(super) fn waited_on(
    tree: &str,
    stay: Stay,
    scan: &dyn Fn() -> Answer<Vec<Process>>,
) -> Option<(Health, Detail)> {
    let tree = Path::new(tree);
    if tree.as_os_str().is_empty() || !crate::git::linked(tree) {
        return None;
    }
    let (from, came) = (stay.from.min(stay.came), stay.came);
    let within = (came - START_SLACK_SECONDS)..=(stay.last + START_SLACK_SECONDS);
    let went_back = (from - START_SLACK_SECONDS)..(came - START_SLACK_SECONDS);
    let its_own = |p: &&Process| p.started.is_some_and(|at| within.contains(&at));
    let maybe = |p: &&Process| p.started.is_none_or(|at| went_back.contains(&at));
    let (health, phase) = match scan() {
        Err(_) => (Health::Unknown, "unknown"),
        Ok(listed) => {
            let there: Vec<&Process> = listed.iter().filter(|p| works_in(&p.cwd, tree)).collect();
            if there.iter().any(its_own) {
                (Health::Live, "running")
            } else if there.iter().any(maybe) {
                (Health::Unknown, "unknown")
            } else {
                return None;
            }
        }
    };
    let detail = Detail {
        recipe: PROCESS.into(),
        phase: phase.into(),
        since: None,
    };
    Some((health, detail))
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn a_path_resolves_as_python_s_realpath_resolves_it() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(resolve(""), "");
        assert_eq!(
            resolve("not/there"),
            here.join("not/there").to_string_lossy()
        );
        assert_eq!(resolve("/no-such-top/x"), "/no-such-top/x");
        assert_eq!(resolve("/"), "/");
    }
}
