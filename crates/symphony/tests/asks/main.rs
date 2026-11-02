//! Ephemeral document asks expire after a day and never reach tracked docs.

#[path = "../common/mod.rs"]
mod common;

mod cli;
mod line;
mod path;
mod store;

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use symphony::asks::AskStore;

pub const DAY: f64 = 24.0 * 3600.0;

/// A clock the test moves by hand.
#[derive(Clone)]
pub struct Clock(Rc<Cell<f64>>);

impl Default for Clock {
    fn default() -> Clock {
        Clock(Rc::new(Cell::new(1_800_000_000.0)))
    }
}

impl Clock {
    pub fn new() -> Clock {
        Clock::default()
    }

    pub fn now(&self) -> f64 {
        self.0.get()
    }

    pub fn add(&self, seconds: f64) {
        self.0.set(self.0.get() + seconds);
    }
}

/// The store at `path`, on `clock`.
pub fn store_on(path: &Path, clock: &Clock) -> AskStore {
    let clock = clock.clone();
    AskStore::with_clock(path, Box::new(move || clock.now())).unwrap()
}
