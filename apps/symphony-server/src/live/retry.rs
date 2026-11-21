//! Failed board operations wait 1, 2, 4, 8, 16, 32, then 60 seconds between attempts.
//! Each hub keeps separate state for its worker's open/read, page-reader open, chat journal,
//! callback dispatch and assignment. Success clears only that operation's streak. Deadlines use
//! a monotonic clock, injected by tests, and start when an attempt ends. Ticks, file/schema changes and owner
//! saves never clear a deadline: repaired work resumes at the next due tick or page request.
//! Nothing sleeps here; the existing interruptible board clock continues its healthy work.
//!
//! A hold is the one exception ([`Retry::run_unless_released`]): it ends by its holder's release,
//! not by load, so an opening that failed while the board's database was held
//! ([`symphony_board::server::Home::held`]) is due at the first wake that finds the hold gone,
//! whatever its deadline, and a failure after it begins a new streak at one second. Each wake before
//! that reads the hold's file and attempts nothing. A board that is down and not held keeps its
//! deadlines.
//!
//! Skipped dispatch/open attempts produce no log or health pass. The existing health counters
//! count actual dispatch attempts and actual worker connection failures, retaining their wire
//! shape. Journal records and callback cursors stay with their existing durable writers until
//! delivery succeeds; deferring an attempt acknowledges nothing.

use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Retry {
    next: Option<Instant>,
    delay: Duration,
    /// Whether the board was held when the last attempt failed.
    held: bool,
}

impl Retry {
    /// Try `work` only once its deadline is due. `None` means no attempt, never success.
    pub(super) fn run<T, E>(
        &mut self,
        clock: &dyn Fn() -> Instant,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        self.run_unless_released(clock, &|| false, work)
    }

    /// [`Retry::run`] for work a hold refuses: `held` says whether the board is held now, and is
    /// asked after a failed attempt and at each wake that waits on a failure it was held at.
    pub(super) fn run_unless_released<T, E>(
        &mut self,
        clock: &dyn Fn() -> Instant,
        held: &dyn Fn() -> bool,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        if self.next.is_some_and(|next| clock() < next) {
            if !self.held || held() {
                return None;
            }
            // Released: what follows is a new streak, not the hold's.
            *self = Self::default();
        }
        let result = work();
        if result.is_ok() {
            *self = Self::default();
        } else {
            self.delay = self
                .delay
                .saturating_mul(2)
                .clamp(Duration::from_secs(1), Duration::from_secs(60));
            self.next = Some(clock() + self.delay);
            self.held = held();
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn failures_double_to_sixty_and_success_resets_the_next_failure() {
        let now = Cell::new(Instant::now());
        let clock = || now.get();
        let mut retry = Retry::default();
        for seconds in [1, 2, 4, 8, 16, 32, 60, 60, 60] {
            // A slow attempt's whole wait starts after it ends.
            assert_eq!(
                retry.run(&clock, || {
                    now.set(now.get() + Duration::from_secs(3));
                    Err::<(), _>("unavailable")
                }),
                Some(Err("unavailable"))
            );
            let due = now.get() + Duration::from_secs(seconds);
            now.set(due - Duration::from_nanos(1));
            assert_eq!(
                retry.run(&clock, || -> Result<(), ()> { panic!("before deadline") }),
                None
            );
            now.set(due);
        }
        assert_eq!(retry.run(&clock, || Ok::<_, ()>(42)), Some(Ok(42)));
        // Healthy work keeps its cadence, even in the same clock instant.
        assert_eq!(retry.run(&clock, || Ok::<_, ()>(43)), Some(Ok(43)));
        assert_eq!(
            retry.run(&clock, || Err::<(), _>("again")),
            Some(Err("again"))
        );
        now.set(now.get() + Duration::from_secs(1));
        assert_eq!(retry.run(&clock, || Ok::<_, ()>(())), Some(Ok(())));
    }

    /// One failed attempt at `now`, with the board held or not.
    fn fail(retry: &mut Retry, now: &Cell<Instant>, held: &Cell<bool>) -> Option<Result<(), ()>> {
        retry.run_unless_released(&|| now.get(), &|| held.get(), || Err(()))
    }

    #[test]
    fn a_failure_under_a_hold_is_due_at_the_first_wake_after_its_release() {
        let now = Cell::new(Instant::now());
        let held = Cell::new(true);
        let mut retry = Retry::default();
        // Held for an hour: the deadlines stand, and reach sixty seconds.
        for seconds in [1, 2, 4, 8, 16, 32, 60, 60] {
            assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
            now.set(now.get() + Duration::from_secs(seconds) - Duration::from_nanos(1));
            assert_eq!(
                fail(&mut retry, &now, &held),
                None,
                "held, before {seconds} s"
            );
            now.set(now.get() + Duration::from_nanos(1));
        }
        assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
        // Released in the same instant: the next wake attempts, sixty seconds early.
        held.set(false);
        assert_eq!(
            retry.run_unless_released(&|| now.get(), &|| held.get(), || Ok::<_, ()>(7)),
            Some(Ok(7))
        );
    }

    #[test]
    fn a_failure_after_a_release_begins_a_new_streak_and_is_not_early_again() {
        let now = Cell::new(Instant::now());
        let held = Cell::new(true);
        let mut retry = Retry::default();
        for seconds in [1, 2, 4] {
            assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
            now.set(now.get() + Duration::from_secs(seconds));
        }
        assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
        held.set(false);
        // The released board's server is not up yet: one second, then two, as any failure.
        for seconds in [1, 2] {
            assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
            now.set(now.get() + Duration::from_secs(seconds) - Duration::from_nanos(1));
            assert_eq!(
                fail(&mut retry, &now, &held),
                None,
                "down, before {seconds} s"
            );
            now.set(now.get() + Duration::from_nanos(1));
        }
    }

    #[test]
    fn a_board_held_only_after_it_failed_keeps_its_deadline_at_the_release() {
        let now = Cell::new(Instant::now());
        let held = Cell::new(false);
        let mut retry = Retry::default();
        assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
        // Down, then held and released within the wait: the failure was not the hold's.
        held.set(true);
        assert_eq!(fail(&mut retry, &now, &held), None);
        held.set(false);
        assert_eq!(fail(&mut retry, &now, &held), None);
        now.set(now.get() + Duration::from_secs(1));
        assert_eq!(fail(&mut retry, &now, &held), Some(Err(())));
    }

    #[test]
    fn a_wake_that_waits_on_no_hold_never_asks_whether_the_board_is_held() {
        let now = Cell::new(Instant::now());
        let asked = Cell::new(0);
        let held = || {
            asked.set(asked.get() + 1);
            false
        };
        let mut retry = Retry::default();
        let clock = || now.get();
        assert_eq!(
            retry.run_unless_released(&clock, &held, || Err::<(), _>(())),
            Some(Err(()))
        );
        assert_eq!(asked.get(), 1, "asked once, as the attempt failed");
        for _ in 0..100 {
            assert_eq!(
                retry.run_unless_released(&clock, &held, || Err::<(), _>(())),
                None
            );
        }
        assert_eq!(asked.get(), 1);
        now.set(now.get() + Duration::from_secs(1));
        assert_eq!(
            retry.run_unless_released(&clock, &held, || Ok::<_, ()>(())),
            Some(Ok(()))
        );
        assert_eq!(asked.get(), 1, "a success asks nothing");
    }

    #[test]
    fn thousands_of_wakes_do_not_skip_deadlines_or_count_as_attempts() {
        let start = Instant::now();
        let now = Cell::new(start);
        let mut retry = Retry::default();
        let mut attempts = Vec::new();
        for tick in 0..36_000 {
            now.set(start + Duration::from_millis(tick * 100));
            retry.run(&|| now.get(), || {
                attempts.push(tick / 10);
                Err::<(), _>(())
            });
        }
        assert_eq!(&attempts[..8], [0, 1, 3, 7, 15, 31, 63, 123]);
        assert_eq!(attempts.len(), 65);
        assert_eq!(attempts.last(), Some(&3543));
    }
}
