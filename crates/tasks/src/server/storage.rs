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

    /// The storage a private replay of a log of `logged` bytes runs under ([`super::Home::replay`]),
    /// none when this storage's memtable holds the log with room to spare: a memtable twice the
    /// log, as SurrealKV lays each write out with more than the log holds of it, at least this
    /// one's and at most the largest SurrealKV makes (`u32::MAX` bytes, the arena's offsets). Twice
    /// the log or more is never asked of this storage's memtable, so an empty log asks for none.
    pub fn replaying(self, logged: u64) -> Option<Storage> {
        let needed = logged.saturating_mul(2);
        (needed > self.memtable).then(|| Storage {
            memtable: needed.min(u64::from(u32::MAX)),
            ..self
        })
    }

    /// The environment a server keeps its data this way under, each variable and its value.
    pub fn environment(self) -> [(&'static str, String); 2] {
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

/// The bytes the write-ahead log of the store at `data` holds: every segment in its `wal/`, which
/// SurrealKV 0.21 empties when it closes the store cleanly and replays at the next start
/// otherwise; 0 for a store that has none.
pub(super) fn logged(data: &Path) -> u64 {
    let Ok(segments) = std::fs::read_dir(data.join("wal")) else {
        return 0;
    };
    segments
        .filter_map(Result::ok)
        .filter(|segment| segment.path().extension().is_some_and(|ext| ext == "wal"))
        .filter_map(|segment| segment.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|meta| meta.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{Storage, logged};
    use std::fs;

    /// A replay's memtable: none while the board's holds twice the log, else twice the log, never
    /// past SurrealKV's largest.
    #[test]
    fn a_replay_asks_for_a_memtable_of_twice_the_log_only_past_the_boards_own() {
        let board = Storage::BOARD;
        assert_eq!(board.replaying(0), None);
        assert_eq!(
            board.replaying(128 << 20),
            None,
            "twice it is the board's own"
        );
        let replaying = board.replaying((128 << 20) + 1).unwrap();
        assert_eq!(replaying.memtable, (256 << 20) + 2);
        assert_eq!(replaying.value_log_threshold, board.value_log_threshold);
        // A log of 3.3 GB, as a server under SurrealKV's own 4 GiB memtable writes: the largest
        // memtable SurrealKV makes.
        assert_eq!(
            board.replaying(3_300_000_000).unwrap().memtable,
            u64::from(u32::MAX)
        );
        assert_eq!(
            board.replaying(u64::MAX).unwrap().memtable,
            u64::from(u32::MAX)
        );
    }

    /// The log is the segments in `wal/`, only files named `*.wal`; none at all for a store that has
    /// no `wal/`, or no store.
    #[test]
    fn the_log_is_the_bytes_of_the_segments_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("storage-logged-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(logged(&dir), 0, "no store");
        fs::create_dir_all(dir.join("wal")).unwrap();
        assert_eq!(logged(&dir), 0, "an empty log, as a clean close leaves it");
        fs::write(dir.join("wal/00000000000000000017.wal"), vec![0u8; 3000]).unwrap();
        fs::write(dir.join("wal/00000000000000000018.wal"), vec![0u8; 45]).unwrap();
        fs::write(dir.join("wal/notes.txt"), vec![0u8; 999]).unwrap();
        fs::create_dir_all(dir.join("wal/inner.wal")).unwrap();
        assert_eq!(logged(&dir), 3045);
        let _ = fs::remove_dir_all(&dir);
    }
}
