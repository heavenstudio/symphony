//! When a server generation's recorders record: the one gate the Symphony server's generation
//! holds (`front::Shared` in `apps/symphony-server`), which every project it serves and both of its
//! recorders, the board's health hours ([`crate::board_health::hours`]) and the agents' day files
//! (`live/agent_activity.rs`), read at each call; so does the hourly roll-up at each day file it
//! writes ([`crate::reports::write`], `live/reports.rs`), so a reload never has two generations
//! writing one.
//!
//! A reload's hand-off has two generations alive at once, both counting their own ticks, so the
//! replacement's gate is held until the hand-off commits, and the generation it replaces closes its
//! own just before it writes the commit byte: no two generations record the same moment. A
//! hand-off that fails after that close opens the old generation's gate again ([`Gate::reopen`]).
//! The calls are the reload's (`front.rs` holds and opens, `reload.rs` closes and reopens); one
//! call moves both recorders, so they agree on every minute.

use std::sync::{Mutex, MutexGuard};

use crate::lock::Recovered as _;

/// One server generation's gate: open until held or closed.
#[derive(Debug)]
pub struct Gate(Mutex<State>);

/// How a [`Gate`] stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// Recording; since this time, by the server's clock, when it was opened after a hold or a
    /// close, so a line begins after it.
    Open { since: Option<f64> },
    /// A replacement's, until its hand-off commits: nothing is recorded.
    Held,
    /// Closed at this time, by the server's clock, as the hand-off commits: nothing is recorded
    /// after it.
    Closed(f64),
}

impl Default for Gate {
    fn default() -> Gate {
        Gate(Mutex::new(State::Open { since: None }))
    }
}

impl Gate {
    fn state(&self) -> MutexGuard<'_, State> {
        // Each holder reads or replaces a whole value.
        self.0.lock().recover()
    }

    /// A replacement's, before its hubs start: nothing is recorded until [`Gate::open`].
    pub fn hold(&self) {
        *self.state() = State::Held;
    }

    /// A held gate's hand-off committed at `at`: recording from then. Any other gate stays as it
    /// is, so a close by its generation's own reload is never undone here.
    pub fn open(&self, at: f64) {
        self.opened(|state| state == State::Held, at);
    }

    /// Nothing recorded after `at`, by the server's clock, until it opens again: its generation's
    /// hand-off commits.
    pub fn close(&self, at: f64) {
        *self.state() = State::Closed(at);
    }

    /// A closed gate's hand-off failed at `at`: its generation goes on, recording again from then.
    /// Any other gate stays as it is.
    pub fn reopen(&self, at: f64) {
        self.opened(|state| matches!(state, State::Closed(_)), at);
    }

    /// Open since `at`, when it stands as `from` says.
    fn opened(&self, from: impl Fn(State) -> bool, at: f64) {
        let mut state = self.state();
        if from(*state) {
            *state = State::Open { since: Some(at) };
        }
    }

    /// How it stands now.
    pub fn stands(&self) -> State {
        *self.state()
    }

    /// `record` run with how it stands, held meanwhile, so that no hold, open or close falls during
    /// it: a close falls between two of a recorder's calls.
    pub fn with<T>(&self, record: impl FnOnce(State) -> T) -> T {
        let state = self.state();
        record(*state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gate_opens_only_from_a_hold_and_reopens_only_from_a_close() {
        let gate = Gate::default();
        assert_eq!(gate.stands(), State::Open { since: None });
        gate.open(1.0);
        gate.reopen(1.0);
        assert_eq!(
            gate.stands(),
            State::Open { since: None },
            "an open gate stays"
        );
        gate.hold();
        gate.reopen(2.0);
        assert_eq!(gate.stands(), State::Held, "a reopen leaves a hold");
        gate.open(3.0);
        assert_eq!(gate.stands(), State::Open { since: Some(3.0) });
        gate.close(4.0);
        gate.open(5.0);
        assert_eq!(gate.stands(), State::Closed(4.0), "an open leaves a close");
        gate.reopen(6.0);
        assert_eq!(gate.stands(), State::Open { since: Some(6.0) });
        assert_eq!(gate.with(|state| state), State::Open { since: Some(6.0) });
    }
}
