//! When the reload worker tries again a source version whose update failed: its build failed, or
//! the replacement it built did not take over. Not at its next check, since what failed it (the
//! machine's load, a full disk, a board slow to read) may still hold, but once a wait is over that
//! starts at [`FIRST`] and doubles with each failure of that version in a row, up to [`MOST`]
//! ([`backoff`]), so a version that cannot start costs at most one build per [`MOST`]. Any other
//! version, a source change, is tried at its first check, as before.
//!
//! A wait is over when either clock says so ([`crate::sys::outlasted`]): the monotonic one stops
//! while the machine sleeps, and the notice names wall time, so a retry never comes later than the
//! notice says but for the period of the check that finds it due. A wall clock set back delays
//! none; one set forward can make a retry due sooner.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The wait before a failed version is tried again the first time.
const FIRST: Duration = Duration::from_secs(5 * 60);
/// The longest wait between two tries of one failed version.
const MOST: Duration = Duration::from_secs(60 * 60);

/// A moment on both clocks ([`crate::sys::moment`]).
type Moment = (Instant, SystemTime);

/// The version whose update failed last, and when it is tried again.
pub(super) struct Retry {
    failed: Option<Failed>,
    /// The time now: the machine's, or a test's ([`Retry::timed_by`]).
    clock: Box<dyn Fn() -> Moment + Send>,
}

struct Failed {
    fingerprint: String,
    at: Moment,
    wait: Duration,
    /// Its try once the wait was over has begun: there is no other until it fails again.
    tried: bool,
}

impl Retry {
    pub fn new() -> Self {
        Self::timed_by(crate::sys::moment)
    }

    /// A retry whose waits run by `clock`.
    pub(super) fn timed_by(clock: impl Fn() -> Moment + Send + 'static) -> Self {
        Self {
            failed: None,
            clock: Box::new(clock),
        }
    }

    /// Whether to try `fingerprint` again now: the version whose update failed last, once its wait
    /// is over, and once for each failure, so a try that ends in no failure (a build kept as the
    /// running image) is its last.
    pub fn again(&mut self, fingerprint: &str) -> bool {
        let Some(failed) = &mut self.failed else {
            return false;
        };
        if failed.tried
            || failed.fingerprint != fingerprint
            || !crate::sys::outlasted(failed.at, (self.clock)(), failed.wait)
        {
            return false;
        }
        failed.tried = true;
        true
    }

    /// The update of `fingerprint` failed now: when it is tried again, to the nearest minute in
    /// this machine's time zone, as the notice says it.
    pub fn failed(&mut self, fingerprint: &str) -> String {
        let at = (self.clock)();
        let wait = backoff(
            self.failed
                .as_ref()
                .filter(|failed| failed.fingerprint == fingerprint)
                .map(|failed| failed.wait),
        );
        self.failed = Some(Failed {
            fingerprint: fingerprint.into(),
            at,
            wait,
            tried: false,
        });
        when(at.1, wait)
    }
}

/// The retry's exponential backoff, its one number: the wait after a version's failure, [`FIRST`]
/// after its first and twice the wait `before` after each next, up to [`MOST`]. It only says how
/// long: nothing sleeps on it, as the reload worker's own checks find a retry due.
fn backoff(before: Option<Duration>) -> Duration {
    before.map_or(FIRST, |wait| wait.saturating_mul(2).min(MOST))
}

/// When a wait of `wait` from `from` ends, to the nearest minute in this machine's time zone.
fn when(from: SystemTime, wait: Duration) -> String {
    let at = (from + wait + Duration::from_secs(30))
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    symphony::clock::stamp_at(libc::time_t::try_from(at).unwrap_or(libc::time_t::MAX))
}

#[cfg(test)]
mod tests {
    use super::{FIRST, MOST, Moment, Retry, backoff, when};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const MINUTE: Duration = Duration::from_secs(60);

    /// A whole minute of wall time, so a stamp of it plus whole minutes is that minute's.
    fn whole_minute() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// The minute `at` falls in, as the notice names it.
    fn stamp(at: SystemTime) -> String {
        let seconds = at.duration_since(UNIX_EPOCH).unwrap().as_secs();
        symphony::clock::stamp_at(libc::time_t::try_from(seconds).unwrap())
    }

    /// A retry on a test's clock, and that clock, which the test sets.
    fn retry() -> (Retry, Arc<Mutex<Moment>>) {
        let now = Arc::new(Mutex::new((Instant::now(), whole_minute())));
        let clock = now.clone();
        (Retry::timed_by(move || *clock.lock().unwrap()), now)
    }

    /// `now` moved on by `by` on both clocks.
    fn pass(now: &Mutex<Moment>, by: Duration) {
        let mut now = now.lock().unwrap();
        *now = (now.0 + by, now.1 + by);
    }

    #[test]
    fn a_failed_version_is_tried_once_after_a_wait_that_doubles_up_to_an_hour() {
        let (mut retry, now) = retry();
        assert!(!retry.again("one"), "nothing failed: nothing to try again");
        let start = now.lock().unwrap().1;
        let mut waited = Duration::ZERO;
        // Each failure of one version doubles its wait, up to the bound; it is not tried before
        // that wait is over, then once.
        for minutes in [5_u32, 10, 20, 40, 60, 60] {
            let wait = minutes * MINUTE;
            assert_eq!(retry.failed("one"), stamp(start + waited + wait));
            pass(&now, wait - Duration::from_secs(1));
            assert!(!retry.again("one"), "{minutes} min less a second");
            assert!(!retry.again("two"), "another version is no retry");
            pass(&now, Duration::from_secs(1));
            assert!(retry.again("one"), "{minutes} min");
            assert!(!retry.again("one"), "once a failure");
            waited += wait;
        }
        assert_eq!((FIRST, MOST), (5 * MINUTE, 60 * MINUTE));
        assert_eq!(
            backoff(Some(Duration::MAX)),
            MOST,
            "doubling saturates at the bound"
        );
        // Another version's failure starts its own wait from the first, and the one before is
        // tried no more.
        assert_eq!(retry.failed("two"), stamp(start + waited + 5 * MINUTE));
        pass(&now, MOST);
        assert!(!retry.again("one"));
        assert!(retry.again("two"));
        assert_eq!(
            retry.failed("one"),
            stamp(start + waited + MOST + 5 * MINUTE)
        );
    }

    #[test]
    fn a_failed_version_is_due_when_either_clock_says_its_wait_is_over() {
        // Whether a version that failed is due once the clocks read `later` of the failure's.
        let due = |later: fn(Moment) -> Moment| {
            let (mut retry, now) = retry();
            retry.failed("one");
            let moment = later(*now.lock().unwrap());
            *now.lock().unwrap() = moment;
            retry.again("one")
        };
        assert!(
            !due(|(mono, wall)| (mono + 4 * MINUTE, wall + 4 * MINUTE)),
            "four minutes on both"
        );
        assert!(
            due(|(mono, wall)| (mono + Duration::from_secs(1), wall + 5 * MINUTE)),
            "the machine slept: one second awake, the notice's five minutes of wall time"
        );
        assert!(
            due(|(mono, wall)| (mono + 5 * MINUTE, wall - MINUTE)),
            "the wall clock set back: five awake minutes still end the wait"
        );
    }

    #[test]
    fn the_notice_names_the_minute_the_wait_ends_from_the_failure() {
        // 29 s and 31 s past a whole minute round to either side of it.
        let failed = whole_minute();
        let five = 5 * MINUTE;
        assert_eq!(
            when(failed + Duration::from_secs(29), five),
            stamp(failed + five)
        );
        assert_eq!(
            when(failed + Duration::from_secs(31), five),
            stamp(failed + 6 * MINUTE)
        );
        assert_eq!(when(failed, MOST), stamp(failed + MOST));
    }
}
