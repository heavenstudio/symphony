//! A dispatch pass's visit to one recipient, and the one machine observation it shares.

use super::Pass;
use crate::Store;
use crate::cli::agents::{Probe, Probes};

fn sampled<'a, T: Clone + 'a>(probe: Probe<'a, T>) -> Probe<'a, T> {
    let sample = std::cell::OnceCell::new();
    Box::new(move || sample.get_or_init(&probe).clone())
}

/// A pass's visit to one recipient: what the pass does there, and one machine observation shared by
/// planning and the leased read. Only machine inputs are retained: the agents view is rebuilt
/// against the refreshed board.
pub(in crate::store) struct Visit<'env> {
    pub(in crate::store) pass: Pass,
    env: &'env crate::cli::Env,
    /// The dispatching store, which keeps the machine's process listing across visits and passes.
    store: &'env Store,
    sample: std::cell::OnceCell<Probes<'env>>,
}

impl<'env> Visit<'env> {
    pub(in crate::store) fn new(
        pass: Pass,
        env: &'env crate::cli::Env,
        store: &'env Store,
    ) -> Self {
        Visit {
            pass,
            env,
            store,
            sample: std::cell::OnceCell::new(),
        }
    }

    pub(in crate::store) fn probes(&self) -> &Probes<'env> {
        self.sample.get_or_init(|| {
            let probes = crate::cli::probes::machine(self.env).kept_by(self.store);
            Probes {
                panes: sampled(probes.panes),
                runs: sampled(probes.runs),
                ..probes
            }
        })
    }
}
