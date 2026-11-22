//! What a store holds of its project's limits ([`crate::limits`]), which the CLI and the Symphony
//! read from the main checkout's `symphony.toml` when they open it, and a store they keep reads
//! again whenever the file changes: the cap on an orchestrator's open tasks ([`crate::slots`]), the
//! low-volume threshold and the agents a machine runs ([`crate::cli::pull`],
//! [`crate::launch::running`]), and when the pipeline alarm raises and clears ([`crate::alarm`]).

use super::Store;
use crate::alarm::Thresholds;
use crate::limits::{Held, Limits, Read, Watched};
use std::sync::Arc;

impl Store {
    /// This store, held to the limits of the project file `watched` watches, as that file is at
    /// each read: a change to it reaches a store kept open with no restart.
    pub fn watching(mut self, watched: Arc<Watched>) -> Store {
        self.limits = Held::Watched(watched);
        self
    }

    /// This store, held to `limits` from now on, whatever a project file says.
    pub fn limiting(mut self, limits: Limits) -> Store {
        self.limits = Held::Set(limits);
        self
    }

    /// The limits this store holds to now.
    pub fn limits(&self) -> Limits {
        self.limits.now(|| self.stored())
    }

    /// The project file this store's limits are read from, as it reads it now, with where each
    /// was written: none for a store held to limits set here ([`Store::limiting`]).
    pub fn limits_read(&self) -> Option<Read> {
        match &self.limits {
            Held::Watched(watched) => Some(watched.read_with(&self.stored())),
            Held::Set(_) => None,
        }
    }

    /// This store, its starts held to `slots` open tasks an orchestrator: a claim, a move into
    /// in_progress and taking the orchestrator role on open work are refused past it
    /// ([`crate::slots`]). None caps nothing, as a store opens. Its other limits stay as they are
    /// now, set from here.
    pub fn capping(self, slots: Option<usize>) -> Store {
        let limits = Limits {
            orchestrator_slots: slots,
            ..self.limits()
        };
        self.limiting(limits)
    }

    /// The cap this store holds starts to, if any ([`Store::capping`]).
    pub fn slots(&self) -> Option<usize> {
        self.limits().orchestrator_slots
    }

    /// This store, its PULL digests raising the pipeline alarm at `thresholds` ([`crate::alarm`]);
    /// a store opens with the defaults. Its other limits stay as they are now, set from here.
    pub fn alarming(self, thresholds: Thresholds) -> Store {
        let limits = Limits {
            alarm: thresholds,
            ..self.limits()
        };
        self.limiting(limits)
    }

    /// When this store's PULL digests raise the pipeline alarm ([`Store::alarming`]).
    pub fn alarm(&self) -> Thresholds {
        self.limits().alarm
    }
}
