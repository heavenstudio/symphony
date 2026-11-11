//! A dispatch pass's visit to one recipient, and the one machine observation it shares.

use super::{Callback, DeliveryError, Pass, deliver};
use crate::Store;
use crate::cli::agents::{Probe, Probes};
use std::cell::{Cell, RefCell};

fn sampled<'a, T: Clone + 'a>(probe: Probe<'a, T>) -> Probe<'a, T> {
    let sample = std::cell::OnceCell::new();
    Box::new(move || sample.get_or_init(&probe).clone())
}

/// Where a collecting pass prints what a delivery would send ([`Pass::Collect`]): each text whole,
/// flushed, before the pass records it delivered.
pub type Print<'p> = &'p mut dyn FnMut(&str) -> std::io::Result<()>;

/// A pass's visit to one recipient: what the pass does there, and one machine observation shared by
/// planning and the leased read. The canonical holdings are shared across recipients too;
/// the agents view is rebuilt against the refreshed board.
pub(in crate::store) struct Visit<'env> {
    pub(in crate::store) pass: Pass,
    pub(in crate::store) holdings: Option<&'env crate::cli::pull::Holdings>,
    pub(in crate::store) env: &'env crate::cli::Env,
    /// The dispatching store, which keeps the machine's process listing across visits and passes.
    store: &'env Store,
    sample: std::cell::OnceCell<Probes<'env>>,
    /// Where a collecting pass prints ([`Pass::Collect`]), and how many texts it printed.
    print: RefCell<Option<Print<'env>>>,
    printed: Cell<usize>,
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
            print: RefCell::new(None),
            printed: Cell::new(0),
        }
    }

    /// A collecting pass's visit ([`Pass::Collect`]), printing by `print`.
    pub(in crate::store) fn collecting(
        env: &'env crate::cli::Env,
        store: &'env Store,
        holdings: Option<&'env crate::cli::pull::Holdings>,
        print: Print<'env>,
    ) -> Self {
        let visit = Visit::new(Pass::Collect, env, store, holdings);
        *visit.print.borrow_mut() = Some(print);
        visit
    }

    /// Deliver `text` to `callback`, or, in a collecting pass, print it. A text that does not print
    /// whole is not delivered: it waits to be printed again, so a failure repeats a line and never
    /// loses one.
    pub(super) fn send(&self, callback: &Callback, text: &str) -> Result<(), DeliveryError> {
        if self.pass != Pass::Collect {
            return deliver(callback, text);
        }
        let mut print = self.print.borrow_mut();
        let print = print.as_mut().expect("a collecting visit prints");
        print(text).map_err(|why| DeliveryError::retry(format!("printing: {why}")))?;
        self.printed.set(self.printed.get() + 1);
        Ok(())
    }

    /// How many texts a collecting pass printed.
    pub(super) fn printed(&self) -> usize {
        self.printed.get()
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
