//! A dispatch pass's visit to one recipient, and the one machine observation it shares.

use super::{Callback, DeliveryError, Pass, deliver};
use crate::Store;
use crate::cli::agents::{Probe, Probes};

fn sampled<'a, T: Clone + 'a>(probe: Probe<'a, T>) -> Probe<'a, T> {
    let sample = std::cell::OnceCell::new();
    Box::new(move || sample.get_or_init(&probe).clone())
}

/// A pass's visit to one recipient: what the pass does there, and one machine observation shared by
/// planning and the leased read. The canonical holdings are shared across recipients too;
/// the agents view is rebuilt against the refreshed board.
pub(in crate::store) struct Visit<'env> {
    pub(in crate::store) pass: Pass,
    pub(in crate::store) holdings: Option<&'env crate::cli::pull::Holdings>,
    env: &'env crate::cli::Env,
    /// The dispatching store, which keeps the machine's process listing across visits and passes.
    store: &'env Store,
    sample: std::cell::OnceCell<Probes<'env>>,
    /// What a collecting pass would have sent, in order ([`Pass::Collect`]).
    collected: std::cell::RefCell<Vec<String>>,
}

impl<'env> Visit<'env> {
    pub(in crate::store) fn new(
        pass: Pass,
        env: &'env crate::cli::Env,
        store: &'env Store,
        holdings: Option<&'env crate::cli::pull::Holdings>,
    ) -> Self {
        Visit {
            pass,
            env,
            store,
            holdings,
            sample: std::cell::OnceCell::new(),
            collected: std::cell::RefCell::default(),
        }
    }

    /// Deliver `text` to `callback`, or, in a collecting pass, keep it for the reader.
    pub(super) fn send(&self, callback: &Callback, text: &str) -> Result<(), DeliveryError> {
        if self.pass == Pass::Collect {
            self.collected.borrow_mut().push(text.to_string());
            return Ok(());
        }
        deliver(callback, text)
    }

    /// What a collecting pass kept.
    pub(super) fn collected(self) -> Vec<String> {
        self.collected.into_inner()
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
