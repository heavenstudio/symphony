//! A session's health, derived, never stored: from when it was last seen, a long tool call's busy
//! mark, whether its monitor's process is gone, and whether it holds work another session could take
//! over.

use crate::rows::SessionRow;

/// A heartbeat is every 20 s, so three missed beats still read live.
pub const LIVE_SECONDS: f64 = 90.0;
/// Then stale for this long (ten minutes in all) before lost.
pub const STALE_SECONDS: f64 = 510.0;
/// How long a reservation holds.
pub const RESERVE_SECONDS: f64 = 3600.0;

/// Where a task's executor's work can still be claimed.
pub const EXECUTING: [&str; 4] = ["concept", "design", "ready", "in_progress"];
/// Where a task is over: nobody works on it, and the sessions launched for it are cleaned up.
pub const FINISHED: [&str; 2] = ["done", "cancelled"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Health {
    Live,
    Stale,
    /// The native presence sweep is overdue; absence has not been established.
    Unknown,
    Lost,
    Left,
}

impl Health {
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Live => "live",
            Health::Stale => "stale",
            Health::Unknown => "unknown",
            Health::Lost => "lost",
            Health::Left => "left",
        }
    }

    /// Live, stale, or awaiting a native sweep: another session must not take its work.
    pub fn holds(self) -> bool {
        matches!(self, Health::Live | Health::Stale | Health::Unknown)
    }

    /// Stale, lost or awaiting a native sweep: said to whoever is told of the session as STALE,
    /// LOST or LIVENESS UNKNOWN.
    pub fn quiet(self) -> bool {
        matches!(self, Health::Stale | Health::Lost | Health::Unknown)
    }
}

/// Live, stale, lost or left. Gone past the lost line holding nothing anyone could claim is left: a
/// session that finished its part ends quietly. A dead monitor on this host makes a session stale at
/// once (it is re-armed after it expires), but only time makes it lost.
pub fn health(
    session: &SessionRow,
    now: f64,
    host: &str,
    pid_alive: &dyn Fn(i64) -> bool,
    holds_claimable: bool,
) -> Health {
    if session.left_at.is_some() {
        return Health::Left;
    }
    let age = now - session.last_seen;
    let busy = session.busy_until.is_some_and(|until| now <= until);
    let detached = matches!(session.pid, Some(pid) if pid != 0)
        && session.host == host
        && !session.pid.is_some_and(pid_alive);
    if (age <= LIVE_SECONDS || busy) && !detached {
        Health::Live
    } else if age <= LIVE_SECONDS + STALE_SECONDS || busy {
        Health::Stale
    } else if holds_claimable {
        Health::Lost
    } else {
        Health::Left
    }
}

/// Whether `role` on a task in `status` is work another session could take over: an executor's
/// unfinished work, or a review still waiting for its verdict.
pub fn claimable(role: &str, status: &str) -> bool {
    (role == "executor" && EXECUTING.contains(&status))
        || (role == "reviewer" && status == "review")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000.0;
    const GONE: f64 = LIVE_SECONDS + STALE_SECONDS + 1.0;

    fn seen(ago: f64) -> SessionRow {
        SessionRow {
            handle: "amber-heron".into(),
            kind: "worker".into(),
            connected: NOW - ago,
            last_seen: NOW - ago,
            ..Default::default()
        }
    }

    fn of(session: &SessionRow, alive: &[i64], holding: bool) -> Health {
        health(
            session,
            NOW,
            "this-host",
            &|pid| alive.contains(&pid),
            holding,
        )
    }

    #[test]
    fn health_ages_from_live_to_stale_to_lost() {
        assert_eq!(of(&seen(0.0), &[], true), Health::Live);
        assert_eq!(of(&seen(LIVE_SECONDS), &[], true), Health::Live);
        assert_eq!(of(&seen(LIVE_SECONDS + 1.0), &[], true), Health::Stale);
        assert_eq!(
            of(&seen(LIVE_SECONDS + STALE_SECONDS), &[], true),
            Health::Stale
        );
        assert_eq!(of(&seen(GONE), &[], true), Health::Lost);
    }

    #[test]
    fn gone_holding_nothing_reads_left() {
        assert_eq!(of(&seen(GONE), &[], false), Health::Left);
        assert_eq!(of(&seen(LIVE_SECONDS + 1.0), &[], false), Health::Stale);
    }

    #[test]
    fn leaving_is_its_own_state() {
        let left = SessionRow {
            left_at: Some(NOW),
            ..seen(0.0)
        };
        assert_eq!(of(&left, &[], true), Health::Left);
    }

    #[test]
    fn a_dead_local_pid_is_stale_at_once_but_only_time_makes_it_lost() {
        let monitored = SessionRow {
            pid: Some(41),
            host: "this-host".into(),
            ..seen(0.0)
        };
        assert_eq!(of(&monitored, &[41], true), Health::Live);
        assert_eq!(of(&monitored, &[], true), Health::Stale);
        let gone = SessionRow {
            pid: Some(41),
            host: "this-host".into(),
            ..seen(GONE)
        };
        assert_eq!(of(&gone, &[], true), Health::Lost);
    }

    #[test]
    fn a_pid_on_another_host_is_not_probed() {
        let elsewhere = SessionRow {
            pid: Some(41),
            host: "some-other-machine".into(),
            ..seen(0.0)
        };
        assert_eq!(
            health(
                &elsewhere,
                NOW,
                "this-host",
                &|_| panic!("probed another host's pid"),
                true
            ),
            Health::Live
        );
    }

    #[test]
    fn a_pid_of_zero_is_no_monitor() {
        let none = SessionRow {
            pid: Some(0),
            host: "this-host".into(),
            ..seen(0.0)
        };
        assert_eq!(
            health(&none, NOW, "this-host", &|_| panic!("probed pid 0"), true),
            Health::Live
        );
    }

    #[test]
    fn one_long_call_reads_live_until_it_could_have_ended_then_lapses_to_lost() {
        let busy = SessionRow {
            busy_until: Some(NOW + 600.0),
            ..seen(GONE)
        };
        assert_eq!(of(&busy, &[], true), Health::Live);
        let lapsed = SessionRow {
            busy_until: Some(NOW - 1.0),
            ..seen(GONE)
        };
        assert_eq!(of(&lapsed, &[], true), Health::Lost);
    }

    #[test]
    fn a_busy_session_whose_monitor_died_is_stale_not_live() {
        let busy = SessionRow {
            busy_until: Some(NOW + 600.0),
            pid: Some(41),
            host: "this-host".into(),
            ..seen(GONE)
        };
        assert_eq!(of(&busy, &[], true), Health::Stale);
    }

    #[test]
    fn claimable_is_only_work_another_session_could_take() {
        for status in EXECUTING {
            assert!(claimable("executor", status), "{status}");
        }
        for status in ["review", "done", "blocked", "cancelled", "backlog"] {
            assert!(!claimable("executor", status), "{status}");
        }
        assert!(claimable("reviewer", "review"));
        for status in ["in_progress", "done", "ready"] {
            assert!(!claimable("reviewer", status), "{status}");
        }
        for role in ["orchestrator", "reserved"] {
            assert!(!claimable(role, "in_progress"), "{role}");
        }
    }

    #[test]
    fn each_health_has_its_public_name() {
        let named: Vec<&str> = [
            Health::Live,
            Health::Stale,
            Health::Unknown,
            Health::Lost,
            Health::Left,
        ]
        .map(Health::as_str)
        .to_vec();
        assert_eq!(named, ["live", "stale", "unknown", "lost", "left"]);
    }
}
