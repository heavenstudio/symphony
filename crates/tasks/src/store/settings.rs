//! What a store holds of its project's `[board]` settings, which the CLI and the Symphony read from
//! the main checkout's `symphony.toml` when they open it: the cap on an orchestrator's open tasks
//! ([`crate::slots`]) and when the pipeline alarm raises and clears ([`crate::alarm`]).

use super::Store;
use crate::alarm::Thresholds;

impl Store {
    /// This store, its starts held to `slots` open tasks an orchestrator: a claim, a move into
    /// in_progress and taking the orchestrator role on open work are refused past it
    /// ([`crate::slots`]). None caps nothing, as a store opens.
    pub fn capping(mut self, slots: Option<usize>) -> Store {
        self.slots = slots;
        self
    }

    /// The cap this store holds starts to, if any ([`Store::capping`]).
    pub fn slots(&self) -> Option<usize> {
        self.slots
    }

    /// This store, its PULL digests raising the pipeline alarm at `thresholds` ([`crate::alarm`]);
    /// a store opens with the defaults.
    pub fn alarming(mut self, thresholds: Thresholds) -> Store {
        self.alarm = thresholds;
        self
    }

    /// When this store's PULL digests raise the pipeline alarm ([`Store::alarming`]).
    pub fn alarm(&self) -> Thresholds {
        self.alarm
    }
}
