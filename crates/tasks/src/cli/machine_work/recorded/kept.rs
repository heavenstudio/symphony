//! [`runs`](super::runs) kept between reads ([`Kept`]), for a reader that asks often (the
//! Symphony's task page): each file is read again only once it is another file than the one read,
//! by its length, when it changed and its inode ([`Mark`]). The history is appended to or renamed,
//! and an operation's record is a new file renamed over the one before, so each write gives the
//! path another mark; an unfinished operation's lease is asked on every read, as a recorder that
//! ended wrote nothing. What a read gives is what [`runs`](super::runs) gives of the same folder.

use super::super::native::MAX_ROWS;
use super::{HISTORY, OPERATIONS, ROTATED, Reading, Record, Step, held, history, names, record};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// What tells the file at a path from the one read there before: its length, when it changed and
/// its inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    len: u64,
    changed: Option<SystemTime>,
    inode: u64,
}

impl Mark {
    /// The file at `path` now; none while there is none.
    fn of(path: &Path) -> Option<Mark> {
        let found = fs::metadata(path).ok()?;
        Some(Mark {
            len: found.len(),
            changed: found.modified().ok(),
            inode: found.ino(),
        })
    }
}

/// How many times a [`Kept`] read the history's files, and an operation's record: what a read
/// that found nothing new adds nothing to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// Reads of the history, both its files.
    pub histories: usize,
    /// Reads of one operation's record.
    pub records: usize,
}

/// The history as one read found it: its two files' marks, taken before they were read, its
/// records, and where each operation is among them.
struct History {
    marks: [Option<Mark>; 2],
    records: Vec<Record>,
    at: HashMap<String, usize>,
}

/// One work folder's runs, kept between reads.
#[derive(Default)]
pub struct Kept {
    history: Option<History>,
    /// Each operation's record by its path: the file's mark, taken before it was read, and what it
    /// read as; none for a file that is no record, which no later read of the same file makes one.
    operations: HashMap<PathBuf, (Mark, Option<Record>)>,
    /// The operations still in the folder as the last read found them.
    live: Vec<Record>,
    /// Where the last read put a live operation over the history's record of it.
    over: Vec<usize>,
    /// The runs as the last read gave them.
    runs: Arc<Vec<Record>>,
    reads: Reads,
}

impl Kept {
    /// Every run the work folder `folder` holds, each once, as [`runs`](super::runs) gives them:
    /// the same list as the read before while no file changed and no recorder ended.
    pub fn runs(&mut self, folder: &Path) -> Arc<Vec<Record>> {
        let again = self.history(folder);
        let live = self.live(folder);
        if again || live != self.live {
            self.put_together(live, again);
        }
        Arc::clone(&self.runs)
    }

    /// What it has read so far.
    pub fn reads(&self) -> Reads {
        self.reads
    }

    /// Read the history again if either of its files is another than the one read; whether it did.
    /// A line appended, or a rotation, while it is read leaves the marks taken before it, so the
    /// next read reads it again. A history that could not be read is none, and is not kept.
    fn history(&mut self, folder: &Path) -> bool {
        let marks = [ROTATED, HISTORY].map(|name| Mark::of(&folder.join(name)));
        if self
            .history
            .as_ref()
            .is_some_and(|kept| kept.marks == marks)
        {
            return false;
        }
        self.reads.histories += 1;
        self.history = history(folder).ok().map(|records| History {
            marks,
            at: (records.iter().enumerate())
                .map(|(index, run)| (run.operation_id.clone(), index))
                .collect(),
            records,
        });
        true
    }

    /// The operations in the folder, as [`read`](super::read) finds them, each file read only when
    /// it is another than the one read before.
    fn live(&mut self, folder: &Path) -> Vec<Record> {
        let paths = names(&folder.join(OPERATIONS)).unwrap_or_default();
        let mut found = Vec::new();
        for path in &paths {
            if found.len() >= MAX_ROWS {
                break;
            }
            let Some(first) = self.operation(path) else {
                continue;
            };
            if first.state == Step::Finished {
                found.push(first);
                continue;
            }
            match held(&path.with_extension("lease")) {
                Some(true) => found.push(first),
                // A recorder that finished between the two reads left its lease with its record
                // done, another file now; one that ended without saying so left the file it was.
                Some(false) => {
                    if let Some(again) = self.operation(path)
                        && again.state == Step::Finished
                    {
                        found.push(again);
                    }
                }
                None => {}
            }
        }
        self.operations.retain(|path, _| paths.contains(path));
        found
    }

    /// The record at `path`: as kept while the file is the one read, else read now. A file that
    /// could not be read is not kept.
    fn operation(&mut self, path: &Path) -> Option<Record> {
        let mark = Mark::of(path)?;
        if let Some((kept, read)) = self.operations.get(path)
            && *kept == mark
        {
            return read.clone();
        }
        self.reads.records += 1;
        let read = match record(path) {
            Reading::Record(read, _) => Some(*read),
            Reading::NotOne => None,
            Reading::Failed => {
                self.operations.remove(path);
                return None;
            }
        };
        self.operations
            .insert(path.to_path_buf(), (mark, read.clone()));
        read
    }

    /// The runs from the history kept and `live`: each live operation over the history's record of
    /// it, else after the history, in the folder's order. A history read `again` is copied whole;
    /// else only the records that differ are, unless a reader still holds the list given before.
    fn put_together(&mut self, live: Vec<Record>, again: bool) {
        let (records, at): (&[Record], _) = match &self.history {
            Some(history) => (&history.records, Some(&history.at)),
            None => (&[], None),
        };
        let runs = Arc::make_mut(&mut self.runs);
        if again {
            self.over.clear();
            runs.clear();
            runs.extend(records.iter().cloned());
        } else {
            runs.truncate(records.len());
            for index in self.over.drain(..) {
                runs[index].clone_from(&records[index]);
            }
        }
        for run in &live {
            match at.and_then(|at| at.get(&run.operation_id)) {
                Some(&index) => {
                    runs[index].clone_from(run);
                    self.over.push(index);
                }
                None => runs.push(run.clone()),
            }
        }
        self.live = live;
    }
}
