//! Failed board operations wait 1, 2, 4, 8, 16, 32, then 60 seconds between attempts.
//! Each hub keeps separate state for its worker's open/read, page-reader open, chat journal,
//! callback dispatch and assignment. Success clears only that operation's streak. Deadlines use
//! a monotonic clock, injected by tests, and start when an attempt ends. Ticks, file/schema changes and owner
//! saves never clear a deadline: repaired work resumes at the next due tick or page request.
//! Nothing sleeps here; the existing interruptible board clock continues its healthy work.
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
}

impl Retry {
    /// Try `work` only once its deadline is due. `None` means no attempt, never success.
    pub(super) fn run<T, E>(
        &mut self,
        clock: &dyn Fn() -> Instant,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        if self.next.is_some_and(|next| clock() < next) {
            return None;
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
