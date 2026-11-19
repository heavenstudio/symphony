//! Whether the clock's tick may judge the sessions now, or must wait for the idle sweep first.
//!
//! The idle sweep ([`crate::front::maintenance_worker`]) is what looks at each native channel and
//! confirms the sessions whose listener answers ([`symphony_board::launch::reap::sweep`]). While the
//! machine is awake it does so every round, and no session whose listener answers goes quiet past
//! the lost line. A machine asleep runs neither the sweep nor the tick, and its wall clock goes on
//! while its monotonic one stops. At the wake, a tick that judged before the sweep's next round read
//! the gap of the sleep as each such session's silence: it said LOST of every one holding work, and
//! the round after it then confirmed their listeners again, so the next wake said each LOST again,
//! of the same sessions, quiet since the same call.
//!
//! So after a sleep the tick's dispatch and its give wait until a round that began after the wake
//! has ended, answered or failed ([`Presence::judges`]). The tick waits on that event and on no
//! timer, for at most the awake time a round may take: past it the tick judges anyway, as it did
//! before, since a sweep that never ends must not hold every line.

use std::time::{Duration, Instant};

/// More wall time than awake time since the last round began says the machine slept since: the
/// slack a round may start late before the sweep's log calls it late.
const SLEPT: Duration = crate::front::LATE_SWEEP;

/// The longest awake time the tick waits for a round after a sleep: a round's own timeout, which
/// ends a hung one, and the slack it may start late within.
pub(crate) const AT_MOST: Duration = crate::front::SWEEP_TIMEOUT.saturating_add(SLEPT);

/// A moment on both of the hub's clocks: its wall clock, in seconds since the epoch, and its
/// monotonic one.
#[derive(Clone, Copy, Debug)]
struct At {
    wall: f64,
    mono: Instant,
}

/// The idle sweep's rounds as the tick needs them, and the tick's own wait.
#[derive(Debug, Default)]
pub(crate) struct Presence {
    /// When the round under way began.
    running: Option<At>,
    /// When the last round that ended had begun: what it saw is what the board holds of presence.
    swept: Option<At>,
    /// When the tick began to wait for a round, on the monotonic clock.
    waiting: Option<Instant>,
    /// Whether the tick has said it judged without one, once for each wait.
    lapsed: bool,
}

/// What a tick does with its judgement ([`Presence::judges`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Judging {
    /// It judges: no sleep since the last round began, or no round ended yet.
    Now,
    /// It waits for a round that begins after the wake; `began` on the first tick that does.
    Waits { began: bool },
    /// It judges without one, past [`AT_MOST`] of awake time waiting; `first` on the first tick.
    Anyway { first: bool },
}

impl Presence {
    /// A round began at `wall` and `mono`.
    pub(crate) fn began(&mut self, wall: f64, mono: Instant) {
        self.running = Some(At { wall, mono });
    }

    /// The round under way ended, answered or failed.
    pub(crate) fn ended(&mut self) {
        if let Some(round) = self.running.take() {
            self.swept = Some(round);
        }
    }

    /// Whether a tick at `wall` and `mono` judges. Before any round has ended it does, as a server
    /// that runs no sweep always does; after one, it waits once the wall clock has gone more than
    /// [`SLEPT`] past the awake time since that round began, which only a sleep makes, until a later
    /// round has ended or [`AT_MOST`] of awake time has passed. A wall clock set back never waits.
    pub(crate) fn judges(&mut self, wall: f64, mono: Instant) -> Judging {
        let Some(swept) = self.swept else {
            return Judging::Now;
        };
        let awake = mono.saturating_duration_since(swept.mono).as_secs_f64();
        if wall - swept.wall - awake <= SLEPT.as_secs_f64() {
            self.waiting = None;
            self.lapsed = false;
            return Judging::Now;
        }
        match self.waiting {
            None => {
                self.waiting = Some(mono);
                Judging::Waits { began: true }
            }
            Some(since) if mono.saturating_duration_since(since) < AT_MOST => {
                Judging::Waits { began: false }
            }
            Some(_) => {
                let first = !self.lapsed;
                self.lapsed = true;
                Judging::Anyway { first }
            }
        }
    }
}
