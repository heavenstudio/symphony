//! What a long-lived reader's store keeps of the machine between its views of the agents: every
//! live process, where it works and when it started, which a quiet in-process agent is judged by.

use super::Store;
use crate::cli::agents::{Answer, PROCESS_SCAN_SECONDS, ProcessScan};
use crate::lock::Recovered as _;
use std::sync::Mutex;

/// The machine's processes as a long-lived reader last listed them, and when that listing ended by
/// its store's clock ([`Store::kept_scan`]).
#[derive(Default)]
pub(super) struct Scan(Mutex<Option<(f64, Answer<ProcessScan>)>>);

impl Store {
    /// What `scan` answers, asked at most once in [`PROCESS_SCAN_SECONDS`] of this store's clock: a
    /// long-lived reader (the server's clock and dispatcher, the monitor) looks at its agents every
    /// second or so, and listing every process is a machine-wide `lsof`. A listing is dated when it
    /// ends, so one that took long still spares the views right after it another. An answer from a
    /// clock that went back is not kept.
    pub fn kept_scan(&self, scan: &dyn Fn() -> Answer<ProcessScan>) -> Answer<ProcessScan> {
        // A panic in `scan` poisons the lock with the kept listing untouched, since it is only ever
        // replaced whole, after `scan` returns: what it holds is a listing as it was taken, safe to
        // read and to replace.
        let mut kept = self.process_scan.0.lock().recover();
        if let Some((taken, answer)) = kept.as_ref()
            && (0.0..PROCESS_SCAN_SECONDS).contains(&(self.now() - taken))
        {
            return answer.clone();
        }
        let answer = scan();
        *kept = Some((self.now(), answer.clone()));
        answer
    }
}
