//! A clock only a test moves, for a maintenance worker ([`super::Clock`]). Each read and each wait
//! is counted, so the test knows when the worker looked at a time it set and acted on it; a wait
//! ends at the clock's next move or at its wakes, never after a length of real time.

use super::{Clock, Stamp};
use crate::backstop::{Waker, Wakes};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// The wall-clock backstop of the waits for a worker's reads and waits: only a stall reaches it
/// (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

#[derive(Default)]
struct State {
    /// How far past the start the clock reads.
    elapsed: Duration,
    /// How often it was read.
    reads: u64,
    /// How often it was moved.
    moves: u64,
    /// Each wait begun, by the time it was given to run on this clock, in order.
    waits: Vec<Duration>,
    /// The waits under way: each is woken at the next move.
    waiting: Vec<Waker>,
}

#[derive(Clone)]
pub(crate) struct TestClock {
    start: Stamp,
    state: Arc<(Mutex<State>, Condvar)>,
}

impl TestClock {
    pub(crate) fn new() -> Self {
        Self {
            start: Stamp::now(),
            state: Arc::default(),
        }
    }

    /// The clock as one worker reads it.
    pub(crate) fn reader(&self) -> Reader {
        Reader {
            clock: self.clone(),
            seen: AtomicU64::new(0),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.0.lock().unwrap()
    }

    /// Until `done` holds of the state, which each read and wait tells; a stall fails naming `what`.
    fn until(&self, what: &str, mut done: impl FnMut(&State) -> bool) -> MutexGuard<'_, State> {
        let (state, changed) = &*self.state;
        let (state, waited) = changed
            .wait_timeout_while(state.lock().unwrap(), BACKSTOP, |state| !done(state))
            .unwrap();
        assert!(
            !waited.timed_out(),
            "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
            test_time::Load::now()
        );
        state
    }

    /// Move the clock to `elapsed` past its start, and wake every wait under way.
    pub(crate) fn move_to(&self, elapsed: Duration) {
        moved(&mut self.state(), elapsed);
    }

    /// Move the clock on by `by` ([`TestClock::move_to`]).
    pub(crate) fn advance(&self, by: Duration) {
        let mut state = self.state();
        let elapsed = state.elapsed + by;
        moved(&mut state, elapsed);
    }

    /// Move the clock to `elapsed` past its start, when given, then wait for `more` reads: each
    /// counted is of the time moved to.
    pub(crate) fn read_at(&self, elapsed: Option<Duration>, more: u64) {
        let seen = {
            let mut state = self.state();
            if let Some(elapsed) = elapsed {
                moved(&mut state, elapsed);
            }
            state.reads
        };
        drop(self.until("the worker to read its clock", |state| {
            state.reads >= seen + more
        }));
    }

    /// Move the clock to `elapsed` past its start. The first read after decides on that time;
    /// the second comes only once the worker has acted on the decision.
    pub(crate) fn set(&self, elapsed: Duration) {
        self.read_at(Some(elapsed), 2);
    }

    /// Until `count` waits have begun in all: the time each was given, in order.
    pub(crate) fn waited(&self, count: usize) -> Vec<Duration> {
        self.until(&format!("the worker's wait {count}"), |state| {
            state.waits.len() >= count
        })
        .waits
        .clone()
    }

    /// How many waits have begun.
    pub(crate) fn waits(&self) -> usize {
        self.state().waits.len()
    }
}

/// The clock `state` moved to `elapsed`, every wait under way woken.
fn moved(state: &mut State, elapsed: Duration) {
    state.elapsed = elapsed;
    state.moves += 1;
    for waker in state.waiting.drain(..) {
        waker.wake();
    }
}

/// A [`TestClock`] as one worker reads it: its wait ends at once when the clock moved since the
/// worker last read it, so no move between a look and its wait is missed.
pub(crate) struct Reader {
    clock: TestClock,
    /// The moves the worker's last read saw.
    seen: AtomicU64,
}

impl Clock for Reader {
    fn now(&self) -> Stamp {
        let mut state = self.clock.state();
        state.reads += 1;
        self.seen.store(state.moves, Ordering::SeqCst);
        self.clock.state.1.notify_all();
        Stamp {
            mono: self.clock.start.mono + state.elapsed,
            wall: self.clock.start.wall + state.elapsed,
        }
    }

    /// Until the clock moves or `wakes` is woken: `left` is said, and runs on no real clock.
    fn wait(&self, wakes: &Wakes, left: Duration) {
        {
            let mut state = self.clock.state();
            state.waits.push(left);
            self.clock.state.1.notify_all();
            if state.moves != self.seen.load(Ordering::SeqCst) {
                return;
            }
            state.waiting.push(wakes.waker());
        }
        wakes.wait(None);
    }
}
