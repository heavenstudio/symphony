//! A wait for a condition: it looks until the condition holds, and only a backstop on the wall ends
//! it otherwise, long enough that a program which is right but starved of the CPU on a loaded
//! machine still gets there. The backstop is the one rule the Chrome driver's waits keep too
//! (`crates/chrome-devtools`): six times the wait's allowance, never under [`FLOOR`]
//! ([`backstop`]). Past it the wait fails with a [`Stall`], which says how often it looked, how long
//! and the machine's load average then, so a red reads as what it is.

use std::fmt;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// The shortest backstop: past the longest stall measured on this machine under load, a page's
/// renderer given no task for 88 s (tools/browser-wait-counts-frozen-page).
pub const FLOOR: Duration = Duration::from_secs(120);

/// How many times a wait's allowance its backstop is.
const TIMES: u32 = 6;

/// The pause between two looks: from a millisecond, doubling up to [`LONGEST_PAUSE`], so a
/// condition that holds soon is seen soon and a long wait looks a few times a second.
const FIRST_PAUSE: Duration = Duration::from_millis(1);
const LONGEST_PAUSE: Duration = Duration::from_millis(100);

/// The wall-clock backstop of a wait whose allowance is `allowance`: six times it, never under
/// [`FLOOR`]. The allowance is what the wait would take on a quiet machine with room to spare, the
/// budget it had when it was a bound on the wall; a wait with none of its own passes none and waits
/// [`FLOOR`].
pub const fn backstop(allowance: Duration) -> Duration {
    let times = allowance.saturating_mul(TIMES);
    if times.as_nanos() < FLOOR.as_nanos() {
        FLOOR
    } else {
        times
    }
}

/// A wait that ran past its backstop.
pub struct Stall {
    what: String,
    looks: u64,
    backstop: Duration,
    waited: Duration,
    load: Load,
}

/// The machine's load average over 1, 5 and 15 minutes when it was read, or none where it could
/// not be, as a failure on a loaded machine says it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load(pub Option<[f64; 3]>);

impl Load {
    /// The load average now ([`load`]).
    pub fn now() -> Load {
        Load(load())
    }
}

impl fmt::Display for Load {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some([one, five, fifteen]) => write!(
                f,
                "load average {one:.2} {five:.2} {fifteen:.2} over 1, 5 and 15 minutes"
            ),
            None => f.write_str("no load average to read"),
        }
    }
}

impl Stall {
    /// How many times the wait looked at its condition.
    pub fn looks(&self) -> u64 {
        self.looks
    }

    /// How long the wait looked.
    pub fn waited(&self) -> Duration {
        self.waited
    }
}

impl fmt::Display for Stall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: stalled: still not so after {} looks, and the wait's wall-clock backstop of {:.1?} ran out after {:.1?}; {}",
            self.what, self.looks, self.backstop, self.waited, self.load
        )
    }
}

impl fmt::Debug for Stall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Stall {}

/// The machine's load average over 1, 5 and 15 minutes, as `getloadavg` reads it.
pub fn load() -> Option<[f64; 3]> {
    let mut load = [0.0f64; 3];
    // SAFETY: writes at most three doubles into `load`, which holds three.
    let read = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
    (read == 3).then_some(load)
}

/// Looks at `condition` until it gives a value, which is returned; `what` names the condition in
/// the [`Stall`] it fails with once `backstop` ([`backstop`]) has passed on the wall with none. It
/// looks first at once, and once more when the backstop has passed, so a condition that holds is
/// never failed for the time it took to be looked at.
pub fn until<T>(
    what: impl Into<String>,
    backstop: Duration,
    mut condition: impl FnMut() -> Option<T>,
) -> Result<T, Stall> {
    let began = Instant::now();
    let (mut looks, mut pause) = (0u64, FIRST_PAUSE);
    loop {
        looks += 1;
        if let Some(value) = condition() {
            return Ok(value);
        }
        let waited = began.elapsed();
        if waited >= backstop {
            return Err(Stall {
                what: what.into(),
                looks,
                backstop,
                waited,
                load: Load::now(),
            });
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "the named helper of RULES.md code/no-timed-waits for a test's wait: test_time::wait::until"
        )]
        std::thread::sleep(pause.min(backstop.saturating_sub(waited)));
        pause = pause.saturating_mul(2).min(LONGEST_PAUSE);
    }
}

/// [`until`] for a condition that holds or does not.
pub fn until_true(
    what: impl Into<String>,
    backstop: Duration,
    mut condition: impl FnMut() -> bool,
) -> Result<(), Stall> {
    until(what, backstop, || condition().then_some(()))
}
