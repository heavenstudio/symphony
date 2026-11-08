//! What a long-lived reader's store keeps of the machine between its views of the agents: the
//! working directory of every process, which a quiet in-process agent is judged by.

use super::Store;
use crate::cli::agents::{Answer, PROCESS_SCAN_SECONDS};
use std::path::PathBuf;
use std::sync::Mutex;

/// The machine's process working directories as a long-lived reader last asked for them, and when
/// by its store's clock ([`Store::kept_scan`]).
#[derive(Default)]
pub(super) struct Scan(Mutex<Option<(f64, Answer<Vec<PathBuf>>)>>);

impl Store {
    /// What `scan` answers, asked at most once in [`PROCESS_SCAN_SECONDS`] of this store's clock: a
    /// long-lived reader (the server's clock and dispatcher, the monitor) looks at its agents every
    /// second or so, and listing every process's working directory is a machine-wide `lsof`. An
    /// answer from a clock that went back is not kept.
    pub fn kept_scan(&self, scan: &dyn Fn() -> Answer<Vec<PathBuf>>) -> Answer<Vec<PathBuf>> {
        let now = self.now();
        let mut kept = self
            .process_scan
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((taken, answer)) = kept.as_ref()
            && (0.0..PROCESS_SCAN_SECONDS).contains(&(now - taken))
        {
            return answer.clone();
        }
        let answer = scan();
        *kept = Some((now, answer.clone()));
        answer
    }
}
