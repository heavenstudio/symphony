//! What the sweep asks of the machine's processes: whether a Claude Code agent's lock still has its
//! process (the kernel says, `kill(pid, 0)`; `ps -p PID` only dates it, against a reused pid).
//! The shared `process-cwd` crate scans working directories. Any doubt keeps a worktree.

use crate::sys;
use std::collections::BTreeMap;

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// A process's start as `ps -o lstart=` prints it in the C locale and Claude Code writes it into a
/// lock (`Wed Sep 30 13:10:22 2026`): Python's `time.strptime(..., "%a %b %d %H:%M:%S %Y")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    weekday: usize,
    month: usize,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    year: u32,
}

/// `name` as one of `names`, whole or by its first three letters, in any case.
fn named(name: &str, names: &[&str]) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    names
        .iter()
        .position(|full| lower == *full || (lower.len() == 3 && full.starts_with(&lower)))
}

/// One or two digits, at most `most`.
fn small(text: &str, least: u32, most: u32) -> Option<u32> {
    if text.is_empty() || text.len() > 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|n| (least..=most).contains(n))
}

/// `text` read as a start, its runs of spaces taken as one (`ps` pads a one-digit day with a space,
/// strftime with a zero); none when it is not one.
pub fn parse_start(text: &str) -> Option<Start> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let [weekday, month, day, clock, year] = words.as_slice() else {
        return None;
    };
    let clock: Vec<&str> = clock.split(':').collect();
    let [hour, minute, second] = clock.as_slice() else {
        return None;
    };
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Start {
        weekday: named(weekday, &WEEKDAYS)?,
        month: named(month, &MONTHS)?,
        day: small(day, 1, 31)?,
        hour: small(hour, 0, 23)?,
        minute: small(minute, 0, 59)?,
        second: small(second, 0, 61)?,
        year: year.parse().ok()?,
    })
}

/// Claude Code's lock, `claude agent agent-X (pid N start <date>)`, read: its pid's digits and the
/// start it wrote. None for any other reason.
pub fn claude_lock(reason: &str) -> Option<(String, String)> {
    let rest = reason.trim().strip_prefix("claude agent ")?;
    let (agent, rest) = rest.split_once(' ')?;
    if agent.is_empty() || agent.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = rest.strip_prefix("(pid ")?;
    let (pid, rest) = rest.split_once(' ')?;
    let start = rest.strip_prefix("start ")?.strip_suffix(')')?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if start.is_empty() || start.contains('\n') {
        return None;
    }
    Some((pid.to_string(), start.to_string()))
}

/// The agent Claude Code's lock `reason` names (`agent-X`, the name of the worktree it made for
/// it); none for any other reason.
pub fn claude_agent(reason: &str) -> Option<String> {
    claude_lock(reason)?;
    let rest = reason.trim().strip_prefix("claude agent ")?;
    rest.split_once(' ').map(|(agent, _)| agent.to_string())
}

/// What Claude Code's lock says of the process it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claude {
    /// Its process is gone, or another has its pid: why.
    Dead(String),
    /// Its process runs with the start it wrote. That process is the session that started the
    /// agent (its orchestrator), which outlives the agent: the lock alone does not say the agent
    /// still works.
    Live { pid: i64 },
    /// Anything about it in doubt: a start that cannot be read, a pid no process could have, a
    /// `ps` that cannot date it.
    Doubt,
}

/// Claude Code's lock `reason` read against the machine's processes; none when it is not Claude
/// Code's.
pub fn claude(
    reason: &str,
    alive: &dyn Fn(i64) -> bool,
    started: &dyn Fn(i64) -> Option<String>,
) -> Option<Claude> {
    let (digits, written_text) = claude_lock(reason)?;
    let Some(written) = parse_start(&written_text) else {
        return Some(Claude::Doubt);
    };
    // A pid outside what the kernel takes is no process of ours: a lock we do not understand. So is
    // 0, which `kill` reads as this process's group.
    let Some(pid) = digits
        .parse::<i64>()
        .ok()
        .filter(|pid| (1..=i64::from(i32::MAX)).contains(pid))
    else {
        return Some(Claude::Doubt);
    };
    if !alive(pid) {
        return Some(Claude::Dead(format!("lock dead: pid {pid} is not running")));
    }
    let Some(now) = started(pid) else {
        return Some(Claude::Doubt);
    };
    let Some(running) = parse_start(&now) else {
        return Some(Claude::Doubt);
    };
    if running == written {
        return Some(Claude::Live { pid });
    }
    let now = now.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(Claude::Dead(format!(
        "lock dead: pid {pid} was reused, it started {now}, not {written_text}"
    )))
}

/// Why the lock `reason` is dead, or none when it protects its worktree: its process runs with the
/// start it wrote, or it is not Claude Code's, or anything about it is in doubt (a start that cannot
/// be read, a pid no process could have, a `ps` that cannot date it).
pub fn dead_lock(
    reason: &str,
    alive: &dyn Fn(i64) -> bool,
    started: &dyn Fn(i64) -> Option<String>,
) -> Option<String> {
    match claude(reason, alive, started)? {
        Claude::Dead(why) => Some(why),
        Claude::Live { .. } | Claude::Doubt => None,
    }
}

/// Whether `pid` runs, asked of the kernel (`kill(pid, 0)`): one this user may not signal runs.
pub fn pid_alive(pid: i64) -> bool {
    sys::pid_alive(pid)
}

/// When `pid` started, as `ps -o lstart=` prints it in the C locale and in UTC, the way Claude Code
/// writes it into a lock; none when `ps` cannot say. `vars` is the caller's environment, its zone and
/// locale overridden: a local `ps` at -03 reads 10:10:22 where the lock says 13:10:22.
pub fn process_start(pid: i64, vars: &BTreeMap<String, String>) -> Option<String> {
    let done = sys::output(
        sys::command_with("ps", vars)
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", "UTC"),
    )
    .ok()?;
    let said = String::from_utf8_lossy(&done.stdout).trim().to_string();
    (done.status.success() && !said.is_empty()).then_some(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::children::Child as _;
    use crate::sys::Command;

    #[test]
    fn a_start_reads_with_padded_days_and_full_names_and_nothing_else() {
        let start = parse_start("Wed Sep 30 13:10:22 2026").unwrap();
        assert_eq!(parse_start("wed  sep 30  13:10:22 2026"), Some(start));
        assert_eq!(
            parse_start("Wednesday September 30 13:10:22 2026"),
            Some(start)
        );
        assert_eq!(
            parse_start("Thu Jan  1 00:00:00 1970"),
            parse_start("Thu Jan 01 00:00:00 1970")
        );
        for text in [
            "mer. 30 sept. 10:50:28 2026",
            "sometime last week",
            "Wed Sep 32 13:10:22 2026",
            "Wed Sep 30 24:10:22 2026",
            "Wed Sep 30 13:10:22 26",
            "Wed Sep 30 13:10 2026",
            "",
        ] {
            assert_eq!(parse_start(text), None, "{text}");
        }
    }

    #[test]
    fn only_claude_codes_lock_is_read() {
        assert_eq!(
            claude_lock("claude agent agent-ab12 (pid 4242 start Wed Sep 30 13:10:22 2026)"),
            Some(("4242".into(), "Wed Sep 30 13:10:22 2026".into()))
        );
        for reason in [
            "",
            "on a USB disk that is not mounted",
            "claude agent  (pid 1 start x)",
            "claude agent a (pid x start Wed Sep 30 13:10:22 2026)",
            "claude agent a (pid 1 start Wed Sep 30 13:10:22 2026",
            "claude agent a (pid -1 start Wed Sep 30 13:10:22 2026)",
        ] {
            assert_eq!(claude_lock(reason), None, "{reason}");
        }
    }

    #[test]
    fn a_lock_is_dead_only_when_its_process_is_gone_or_another_has_its_pid() {
        let lock = "claude agent a (pid 41 start Wed Sep 30 13:10:22 2026)";
        let never = |_: i64| -> Option<String> { panic!("ps asked of a dead pid") };
        assert_eq!(
            dead_lock(lock, &|_| false, &never).as_deref(),
            Some("lock dead: pid 41 is not running")
        );
        let same = |_: i64| Some("Wed Sep 30 13:10:22 2026".to_string());
        assert_eq!(dead_lock(lock, &|_| true, &same), None);
        let other = |_: i64| Some("Thu Oct  1 09:00:00 2026".to_string());
        assert_eq!(
            dead_lock(lock, &|_| true, &other).as_deref(),
            Some(
                "lock dead: pid 41 was reused, it started Thu Oct 1 09:00:00 2026, not Wed Sep 30 13:10:22 2026"
            )
        );
        assert_eq!(dead_lock(lock, &|_| true, &|_| None), None);
        let localized = |_: i64| Some("mer. 30 sept. 10:50:28 2026".to_string());
        assert_eq!(dead_lock(lock, &|_| true, &localized), None);
    }

    #[test]
    fn a_claude_lock_says_its_agent_and_whether_its_process_runs() {
        let lock = "claude agent agent-a5a6 (pid 41 start Wed Sep 30 13:10:22 2026)";
        assert_eq!(claude_agent(lock).as_deref(), Some("agent-a5a6"));
        assert_eq!(claude_agent("symphony land: sim/jump (pid 41)"), None);
        assert_eq!(claude_agent("claude agent a (pid x start y)"), None);
        let same = |_: i64| Some("Wed Sep 30 13:10:22 2026".to_string());
        assert_eq!(
            claude(lock, &|_| true, &same),
            Some(Claude::Live { pid: 41 })
        );
        assert!(matches!(
            claude(lock, &|_| false, &same),
            Some(Claude::Dead(_))
        ));
        assert_eq!(claude(lock, &|_| true, &|_| None), Some(Claude::Doubt));
        let unreadable = "claude agent a (pid 41 start sometime last week)";
        assert_eq!(claude(unreadable, &|_| false, &same), Some(Claude::Doubt));
        let zero = "claude agent a (pid 0 start Wed Sep 30 13:10:22 2026)";
        assert_eq!(claude(zero, &|_| false, &same), Some(Claude::Doubt));
        let foreign = |_: i64| Some("mer. 30 sept. 10:50:28 2026".to_string());
        assert_eq!(claude(lock, &|_| true, &foreign), Some(Claude::Doubt));
        assert_eq!(claude("on a USB disk", &|_| true, &same), None);
    }

    #[test]
    fn a_pid_no_process_could_have_is_a_foreign_lock() {
        let gone = |_: i64| false;
        for pid in ["0", "2147483648", "99999999999999999999"] {
            let lock = format!("claude agent a (pid {pid} start Wed Sep 30 13:10:22 2026)");
            assert_eq!(dead_lock(&lock, &gone, &|_| None), None, "{pid}");
        }
    }

    #[test]
    fn a_process_we_may_not_signal_is_alive_and_a_reaped_one_is_not() {
        assert!(pid_alive(1));
        let mut child = Command::of("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        assert!(!pid_alive(pid));
    }

    #[test]
    fn ps_is_read_in_the_c_locale_and_in_utc_whatever_the_caller_uses() {
        let me = i64::from(std::process::id());
        let vars = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            let mut vars: BTreeMap<String, String> = std::env::vars().collect();
            vars.extend(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            vars
        };
        let french = vars(&[
            ("LC_ALL", "fr_FR.UTF-8"),
            ("LANG", "fr_FR.UTF-8"),
            ("LC_TIME", "fr_FR.UTF-8"),
        ]);
        let start = process_start(me, &french).unwrap();
        assert!(parse_start(&start).is_some(), "{start}");
        let elsewhere = process_start(me, &vars(&[("TZ", "America/Sao_Paulo")])).unwrap();
        assert_eq!(parse_start(&elsewhere), parse_start(&start));
        let utc = sys::output(
            Command::of("ps")
                .args(["-o", "lstart=", "-p", &me.to_string()])
                .env("LC_ALL", "C")
                .env("TZ", "UTC"),
        )
        .unwrap();
        assert_eq!(
            parse_start(&String::from_utf8_lossy(&utc.stdout)),
            parse_start(&start)
        );
    }
}
