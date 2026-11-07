//! How every server the board starts keeps its data: [`Storage`], SurrealKV's settings given as
//! the server's environment, and the store a server's command line names.

use std::path::Path;

/// How the board's SurrealKV keeps what it is given, as the environment of every server this module
/// starts: SurrealDB 3.3 reads `SURREAL_SURREALKV_*` for its storage engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Storage {
    /// A value of more bytes than this goes apart from its key, to the value log. SurrealKV deletes a
    /// value-log file only once no sstable points into it, so every version of a row rewritten in
    /// place stays there for as long as one older value in that file is live: 102 GiB of log held
    /// a board of 0.5 GiB (tools/tasks-db-space). Kept beside their keys, in the sstables, the
    /// versions a rewrite leaves are dropped by compaction. A store with values already in the log
    /// still reads them.
    pub value_log_threshold: u64,
    /// How many bytes are written before they go to an sstable: the most the write-ahead log holds.
    /// One transaction must fit in it.
    pub memtable: u64,
}

impl Storage {
    /// The board's. Its largest value, a task's brief, is 1.7 MB, far below 64 MiB, and its export's
    /// largest statement 1.8 MB; SurrealKV's own memtable on a 64 GiB machine is 4 GiB.
    pub const BOARD: Storage = Storage {
        value_log_threshold: 64 << 20,
        memtable: 256 << 20,
    };

    pub(super) fn environment(self) -> [(&'static str, String); 2] {
        [
            (
                "SURREAL_SURREALKV_VLOG_THRESHOLD",
                self.value_log_threshold.to_string(),
            ),
            (
                "SURREAL_SURREALKV_MAX_MEMTABLE_SIZE",
                self.memtable.to_string(),
            ),
        ]
    }
}

/// A store at `path`, as a server's command line names it.
pub(super) fn store(path: &Path) -> String {
    format!("surrealkv://{}", path.display())
}
