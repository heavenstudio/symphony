//! Where an agent works, as the agents view compares it: a worktree's path resolved as Python's
//! `os.path.realpath` resolves it ([`resolve`]), and what a quiet in-process agent waits on there.
//!
//! An in-process agent (a subagent: no terminal, no monitor, its tool calls its only heartbeat) gone
//! quiet with no run of its own, in a linked worktree its binding recorded, is waiting on what it
//! started there when a live process of its own works in that worktree (its nearest checkout is
//! that one, never one beneath it): `running_test`, `{"recipe": "process", "phase": "running"}`,
//! and live. A process is its own when it began while the agent was there: not before its binding
//! recorded that worktree, nor after its last tool call, so a server or shell another started there
//! keeps nobody alive. Its start is compared on the clock the kernel dates it by: the board's on
//! macOS, whose stamp is fixed when it began, and on Linux the clock since boot, with the readings of
//! that clock the agent's calls gave, so setting the wall clock while it waits retimes nothing. When
//! the board cannot tell (the machine's processes cannot be listed, or one working there may be the
//! agent's but its start cannot be read, or the board's clock went back while the agent was there
//! and the process began in the time that leaves in doubt, or a call that bounds the stay read no
//! clock since boot): `running_test` with `"phase": "unknown"`, health `unknown`, so it is not called
//! lost either. With none working there that can be its it ages as before ([`waited_on`]).

use super::{Answer, Detail, Probes};
use crate::Store;
use crate::health::Health;
use crate::sys::Boottime;
use std::path::{Path, PathBuf};

/// How long a long-lived reader keeps the machine's process listing ([`Store::kept_scan`]): a
/// process that ended reads gone within this, far inside the ten minutes before lost.
pub const PROCESS_SCAN_SECONDS: f64 = 20.0;
/// The recipe of a wait the view sees as a process working in an in-process agent's worktree.
pub const PROCESS: &str = "process";
/// How far a process's start may fall outside the agent's stay and still be its: the kernel stamps
/// a start and a hook reads its clocks apart, each to its own grain (Linux's in clock ticks).
const START_SLACK_SECONDS: f64 = 1.0;

/// A live process the machine lists: where it works and when it started (none when the kernel could
/// not say: it may be anyone's, the agent's working there included).
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub cwd: PathBuf,
    pub started: Option<Began>,
}

/// When a listed process started, on the clock the kernel dates it by.
#[derive(Debug, Clone, PartialEq)]
pub enum Began {
    /// By the board's clock, a stamp fixed when it started (macOS): compared with the board's
    /// clock's times of the agent's calls.
    At(f64),
    /// On the clock since boot (Linux), which no setting of the wall clock moves: compared with the
    /// readings of that clock the agent's calls gave, never with a wall time.
    AfterBoot(Boottime),
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

/// An in-process agent's stay in the checkout its binding recorded, by the board's clock and by the
/// clock since boot.
#[derive(Debug, Clone, Copy)]
pub(super) struct Stay<'a> {
    /// When its binding recorded that checkout (`placed_at`).
    pub came: f64,
    /// The earliest time a call of its there read: `came`, unless the clock went back while it was
    /// there (`stay_from`).
    pub from: f64,
    /// Its last tool call.
    pub last: f64,
    /// The clock since boot when it came, and at its last call (`placed_boot`, `last_boot`): none
    /// when that call did not read it.
    pub came_boot: Option<&'a Boottime>,
    pub last_boot: Option<&'a Boottime>,
}

/// Whether a process working where an agent stayed is the agent's.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Whose {
    Its,
    Maybe,
    Not,
}

impl Stay<'_> {
    /// Whether a process that started `began` is its: when it started in the stay, by the clock it
    /// was dated on, it is; when that cannot be told (no start, the board's clock went back, a call
    /// that read no clock since boot), maybe; else not.
    fn whose(&self, began: Option<&Began>) -> Whose {
        let slack = START_SLACK_SECONDS;
        let (its, maybe) = match began {
            None => return Whose::Maybe,
            Some(Began::At(at)) => {
                let from = self.from.min(self.came);
                let within = (self.came - slack..=self.last + slack).contains(at);
                (within, (from - slack..self.came - slack).contains(at))
            }
            Some(Began::AfterBoot(began)) => {
                // Readings compare within one boot; a last call read in another boot came before
                // this boot, and before every process now running.
                let came = self.came_boot.filter(|s| s.boot == began.boot);
                let last = self.last_boot.filter(|s| s.boot == began.boot);
                let before = came.is_some_and(|came| began.at < came.at - slack);
                let after =
                    self.last_boot.is_some() && last.is_none_or(|l| began.at > l.at + slack);
                if before || after {
                    return Whose::Not;
                }
                (came.is_some() && last.is_some(), true)
            }
        };
        match (its, maybe) {
            (true, _) => Whose::Its,
            (false, true) => Whose::Maybe,
            (false, false) => Whose::Not,
        }
    }
}

/// What the machine's processes (`scan`, listed once a view) say of an in-process agent gone quiet
/// in `tree` (resolved) after `stay` there: its health and what it waits on. Live on a process
/// working there that began in its stay, by the clock its start is dated on; unknown when they
/// cannot be listed, or when one working there may be its but cannot be told to be: its start
/// cannot be read, or it began while the board's clock had gone back (after the earliest time a
/// call of its there read, before it came), or its start is dated since boot and the call that
/// would bound it read no clock since boot. None when `tree` is no linked worktree (the main
/// checkout, which every agent registered there shares, or a folder in no repository) or no process
/// working there can be its.
pub(super) fn waited_on(
    tree: &str,
    stay: &Stay,
    scan: &dyn Fn() -> Answer<Vec<Process>>,
) -> Option<(Health, Detail)> {
    let tree = Path::new(tree);
    if tree.as_os_str().is_empty() || !crate::git::linked(tree) {
        return None;
    }
    let (health, phase) = match scan() {
        Err(_) => (Health::Unknown, "unknown"),
        Ok(listed) => {
            let whose: Vec<Whose> = listed
                .iter()
                .filter(|p| works_in(&p.cwd, tree))
                .map(|p| stay.whose(p.started.as_ref()))
                .collect();
            if whose.contains(&Whose::Its) {
                (Health::Live, "running")
            } else if whose.contains(&Whose::Maybe) {
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
