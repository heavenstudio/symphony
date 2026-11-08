//! Where an agent works, as the agents view compares it: a worktree's path resolved as Python's
//! `os.path.realpath` resolves it ([`resolve`]), and what a quiet in-process agent waits on there.
//!
//! An in-process agent (a subagent: no terminal, no monitor, its tool calls its only heartbeat) gone
//! quiet with no run of its own, in a linked worktree its binding recorded, is waiting on what it
//! started there when a live process of its own works in that worktree (its nearest checkout is
//! that one, never one beneath it): `running_test`, `{"recipe": "process", "phase": "running"}`,
//! and live. A process is its own when it began while the agent was there: not before its binding
//! recorded that worktree, nor after its last tool call, so a server or shell another started there
//! keeps nobody alive. When the machine's processes cannot be listed the board cannot tell:
//! `running_test` with `"phase": "unknown"`, health `unknown`, so it is not called lost either. With
//! none of its own working there it ages as before ([`waited_on`]).

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
/// when the kernel could not say, which ties it to no agent).
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

/// What the machine's processes (`scan`, listed once a view) say of an in-process agent gone quiet
/// in `tree` (resolved), where it has been from `stay`'s first time (when its binding recorded
/// `tree`) to its second (its last tool call): its health and what it waits on, live on a process
/// that began there in that stay, unknown when they cannot be listed. None when `tree` is no linked
/// worktree (the main checkout, which every agent registered there shares, or a folder in no
/// repository) or no process of its own works there.
pub(super) fn waited_on(
    tree: &str,
    stay: (f64, f64),
    scan: &dyn Fn() -> Answer<Vec<Process>>,
) -> Option<(Health, Detail)> {
    let tree = Path::new(tree);
    if tree.as_os_str().is_empty() || !crate::git::linked(tree) {
        return None;
    }
    let within = (stay.0 - START_SLACK_SECONDS)..=(stay.1 + START_SLACK_SECONDS);
    let its_own =
        |p: &Process| p.started.is_some_and(|at| within.contains(&at)) && works_in(&p.cwd, tree);
    let (health, phase) = match scan() {
        Err(_) => (Health::Unknown, "unknown"),
        Ok(listed) if listed.iter().any(its_own) => (Health::Live, "running"),
        Ok(_) => return None,
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
