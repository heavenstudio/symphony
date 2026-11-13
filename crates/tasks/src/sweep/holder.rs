//! What a worktree's lock says of whoever holds it. Git keeps a lock until someone lifts it, so the
//! lock alone never says whether its holder still works there:
//!
//! * Claude Code's (`claude agent agent-X (pid N start <date>)`) names the process of the session
//!   that started agent X, its orchestrator, which outlives the agent. Its process gone, or another
//!   with its pid, the lock is dead. Its process running, the agent is known finished only by the
//!   board ([`super::Index::agent_finished`]);
//! * a land's (`symphony land: SLUG (pid N, started S)`) is dead once N is not the process that
//!   started at S (`crate::land::started`), or, written by an earlier land with no start
//!   (`(pid N)`), once N is not running; dead, it marks the worktree of a land that ended without
//!   removing it;
//! * the release run's ([`RELEASE_LOCK`], or [`LEGACY_LOCK`] where a build older than the rename
//!   locked it and no run has moved it yet) marks the checkout it reuses on each run, which keeps
//!   it;
//! * any other lock, or one in doubt, keeps its worktree.

use super::process::{self, Claude};

/// The reason the release run's checkout is locked with (`symphony::release::place::LOCK_REASON`).
pub const RELEASE_LOCK: &str = "the Symphony release run's checkout (docs/RELEASES.md)";
/// The reason a build older than the rename locked that checkout with, at `nightly` beside where it
/// is now (`symphony::release::place::LEGACY_LOCK_REASON`): the same checkout until a run moves it.
pub const LEGACY_LOCK: &str = "the Symphony nightly's checkout (docs/RELEASES.md)";
/// How long the release run builds incrementally in its checkout before it clears the build itself
/// (`symphony::release::place::BUILD_KEPT_DAYS`): the sweep clears it once nothing touched the
/// checkout for that long, a release run no longer running.
pub const RELEASE_BUILD_DAYS: f64 = 7.0;

/// A worktree's holder, as its lock says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// Not locked.
    Nobody,
    /// Locked by a process that is gone: why.
    Gone(String),
    /// Claude Code's lock of a live process: the agent it names and that process, its orchestrator.
    Agent { agent: String, pid: i64 },
    /// A land's lock whose process runs.
    Land(i64),
    /// A land's lock whose process is gone: the worktree of a land that ended without removing it.
    LandEnded(i64),
    /// The release run's checkout.
    Release,
    /// Any other lock, or one in doubt: its reason.
    Foreign(String),
}

impl Holder {
    /// The holder `reason` names (none: not locked), read against the machine's processes.
    pub fn of(
        reason: Option<&str>,
        alive: &dyn Fn(i64) -> bool,
        started: &dyn Fn(i64) -> Option<String>,
    ) -> Holder {
        let Some(reason) = reason else {
            return Holder::Nobody;
        };
        if let Some(claude) = process::claude(reason, alive, started) {
            return match (claude, process::claude_agent(reason)) {
                (Claude::Dead(why), _) => Holder::Gone(why),
                (Claude::Live { pid }, Some(agent)) => Holder::Agent { agent, pid },
                _ => Holder::Foreign(reason.to_string()),
            };
        }
        if [RELEASE_LOCK, LEGACY_LOCK].contains(&reason.trim()) {
            return Holder::Release;
        }
        if reason.starts_with(crate::land::REASON) {
            // A pid no process could have is not a land's: a lock we do not understand.
            return match crate::land::locked_by(reason)
                .filter(|(pid, _)| (1..=i64::from(i32::MAX)).contains(pid))
            {
                Some((pid, start)) => {
                    // As a land reads another's: the process it started as, when the lock says
                    // when; a live pid, when it does not.
                    let runs = match start {
                        Some(start) => crate::land::started(pid).as_deref() == Some(&*start),
                        None => alive(pid),
                    };
                    if runs {
                        Holder::Land(pid)
                    } else {
                        Holder::LandEnded(pid)
                    }
                }
                None => Holder::Foreign(reason.to_string()),
            };
        }
        Holder::Foreign(reason.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTED: &str = "Wed Sep 30 13:10:22 2026";

    fn of(reason: Option<&str>, alive: bool) -> Holder {
        Holder::of(reason, &|_| alive, &|_| Some(STARTED.to_string()))
    }

    #[test]
    fn each_lock_names_its_holder() {
        let claude = format!("claude agent agent-a5a6 (pid 41 start {STARTED})");
        assert_eq!(of(None, true), Holder::Nobody);
        assert_eq!(
            of(Some(&claude), true),
            Holder::Agent {
                agent: "agent-a5a6".into(),
                pid: 41
            }
        );
        assert_eq!(
            of(Some(&claude), false),
            Holder::Gone("lock dead: pid 41 is not running".into())
        );
        let land = "symphony land: sim/jump (pid 77)";
        assert_eq!(of(Some(land), true), Holder::Land(77));
        assert_eq!(of(Some(land), false), Holder::LandEnded(77));
        assert_eq!(of(Some(RELEASE_LOCK), false), Holder::Release);
        assert_eq!(of(Some(LEGACY_LOCK), false), Holder::Release);
        for foreign in [
            "",
            "on a USB disk",
            "symphony land: sim/jump (pid 0)",
            "symphony land: sim/jump",
            "claude agent agent-a5a6 (pid 41 start sometime)",
        ] {
            assert_eq!(
                of(Some(foreign), false),
                Holder::Foreign(foreign.into()),
                "{foreign}"
            );
        }
    }

    #[test]
    fn a_lands_lock_with_its_start_runs_only_as_the_process_that_started_then() {
        let ours = i64::from(std::process::id());
        let start = crate::land::started(ours).expect("this process's start");
        let lock = |start: &str| format!("symphony land: sim/jump (pid {ours}, started {start})");
        // `alive` says nothing here: the start does.
        assert_eq!(of(Some(&lock(&start)), false), Holder::Land(ours));
        assert_eq!(
            of(Some(&lock("1.000000")), true),
            Holder::LandEnded(ours),
            "a process that took its number after it"
        );
        // A start the land could not read is written `-`: a live pid then.
        assert_eq!(of(Some(&lock("-")), true), Holder::Land(ours));
        assert_eq!(of(Some(&lock("-")), false), Holder::LandEnded(ours));
    }

    #[test]
    fn a_claude_lock_in_doubt_is_foreign() {
        let claude = format!("claude agent agent-a5a6 (pid 41 start {STARTED})");
        let undated = Holder::of(Some(&claude), &|_| true, &|_| None);
        assert_eq!(undated, Holder::Foreign(claude));
    }
}
