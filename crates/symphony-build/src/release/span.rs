//! How long a part of a run took, on both of the machine's clocks: the one that counts only while
//! the machine is awake ([`Instant`], which a bound of the run is set on) and the wall clock, which
//! goes on while it sleeps. What a record says a check or a run took is the wall clock's, as a log's
//! times and a step's own seconds are; what the wall clock counted past the awake one is how long
//! the machine slept under it, which the run's record, `--status` and each red it files say
//! (docs/workflow/RELEASES.md). No power log is read.
//!
//! While it runs, a run on macOS holds an assertion against idle sleep ([`awake`]); a closed lid
//! on battery sleeps all the same.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

/// The tests' sleep: the seconds the machine is taken to have slept under each span, in place of
/// none. No flag sets it.
pub(crate) const SLEPT: &str = "SYMPHONY_RELEASE_SLEPT";
/// The least the wall clock counts past the awake one that is a sleep: under it, the difference is
/// the wall clock's own correction (a step or a slew of the network's time).
const LEAST: f64 = 5.0;

/// A part of a run from its start, on both clocks.
#[derive(Clone, Copy)]
pub(crate) struct Span {
    /// Its start on the clock that stops while the machine sleeps: what a bound counts from.
    pub(crate) awake: Instant,
    wall: SystemTime,
}

/// How long a span took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Took {
    /// By the wall clock, a sleep counted.
    pub(crate) seconds: f64,
    /// How long of it the machine slept; 0 when it did not.
    pub(crate) slept: f64,
}

impl Span {
    /// A span that starts now.
    pub(crate) fn begin() -> Span {
        Span {
            awake: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// How long it took so far. A wall clock set back past its start counts as the awake one.
    pub(crate) fn took(&self) -> Took {
        let awake = self.awake.elapsed();
        let wall = self.wall.elapsed().unwrap_or(awake);
        let slept = std::env::var(SLEPT)
            .ok()
            .and_then(|seconds| seconds.parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map_or(Duration::ZERO, Duration::from_secs_f64);
        Took::of(awake, wall + slept)
    }
}

impl Took {
    /// What a span took that the awake clock counted `awake` of and the wall clock `wall`: the
    /// longer of the two, and as slept what the wall clock counted more, once that is [`LEAST`].
    pub(crate) fn of(awake: Duration, wall: Duration) -> Took {
        let (awake, wall) = (awake.as_secs_f64(), wall.as_secs_f64());
        let more = wall - awake;
        Took {
            seconds: wall.max(awake),
            slept: if more >= LEAST { more } else { 0.0 },
        }
    }
}

/// `slept` seconds in a few words (`45 s`, `26 min`, `1 h 4 min`); none for a run that did not
/// sleep.
pub(crate) fn said(slept: f64) -> Option<String> {
    if slept.is_nan() || slept <= 0.0 {
        return None;
    }
    let minutes = (slept / 60.0).round();
    Some(if slept < 60.0 {
        format!("{slept:.0} s")
    } else if minutes < 60.0 {
        format!("{minutes:.0} min")
    } else {
        format!(
            "{:.0} h {:.0} min",
            (minutes / 60.0).floor(),
            minutes % 60.0
        )
    })
}

/// Whether a record says no sleep: what leaves the field out of it.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde hands a field to skip_serializing_if by reference"
)]
pub(crate) fn none(slept: &f64) -> bool {
    *slept <= 0.0
}

/// What holds the machine awake while the process `pid` lives: macOS's assertion against idle
/// sleep, which ends with that process. A closed lid on battery sleeps all the same.
fn caffeinate(pid: u32) -> Command {
    let mut command = Command::new("caffeinate");
    command
        .args(["-i", "-w", &pid.to_string()])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// Hold the machine awake, on macOS, for as long as this process lives ([`caffeinate`]), which
/// ends what holds it. Nothing where the machine has none, and nothing of its power settings is
/// read or changed.
pub(crate) fn awake() {
    if cfg!(target_os = "macos") {
        let _ = caffeinate(std::process::id()).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn took(awake: f64, wall: f64) -> Took {
        Took::of(
            Duration::from_secs_f64(awake),
            Duration::from_secs_f64(wall),
        )
    }

    #[test]
    fn a_span_the_machine_slept_under_takes_the_wall_clocks_seconds_and_says_the_sleep() {
        // The run 2026-11-18T0931-12491: 1,425 s awake of 3,014 s.
        assert_eq!(
            took(1425.0, 3014.0),
            Took {
                seconds: 3014.0,
                slept: 1589.0
            }
        );
        // The least that is a sleep.
        assert_eq!(took(10.0, 15.0).slept, 5.0);
    }

    #[test]
    fn a_span_the_machine_stayed_awake_under_says_no_sleep() {
        let same = took(60.0, 60.0);
        assert_eq!((same.seconds, same.slept), (60.0, 0.0));
        // The wall clock corrected forward by less than a sleep: its seconds, and no sleep.
        let slewed = took(60.0, 64.9);
        assert_eq!((slewed.seconds, slewed.slept), (64.9, 0.0));
        // The wall clock set back: the awake clock's seconds, never fewer.
        let back = took(60.0, 2.0);
        assert_eq!((back.seconds, back.slept), (60.0, 0.0));
        assert_eq!(took(0.0, 0.0).seconds, 0.0);
    }

    #[test]
    fn a_span_begun_now_has_taken_no_time_to_speak_of_and_no_sleep() {
        let span = Span::begin();
        let took = span.took();
        if std::env::var_os(SLEPT).is_none() {
            assert_eq!(took.slept, 0.0, "{took:?}");
        }
        assert!(took.seconds >= 0.0 && span.awake <= Instant::now());
    }

    #[test]
    fn a_sleep_is_said_in_seconds_minutes_or_hours_and_none_is_not_said() {
        assert_eq!(said(0.0), None);
        assert_eq!(said(-3.0), None);
        assert_eq!(said(f64::NAN), None);
        assert_eq!(said(45.0).as_deref(), Some("45 s"));
        assert_eq!(said(60.0).as_deref(), Some("1 min"));
        assert_eq!(said(1589.0).as_deref(), Some("26 min"));
        assert_eq!(said(3599.0).as_deref(), Some("1 h 0 min"));
        assert_eq!(said(3840.0).as_deref(), Some("1 h 4 min"));
        assert!(none(&0.0) && !none(&5.0));
    }

    #[test]
    fn what_holds_the_machine_awake_ends_with_the_process_it_is_held_for() {
        let command = caffeinate(4242);
        assert_eq!(command.get_program(), "caffeinate");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-i", "-w", "4242"]);
        assert_eq!(command.get_current_dir(), Some(std::path::Path::new("/")));
    }
}
