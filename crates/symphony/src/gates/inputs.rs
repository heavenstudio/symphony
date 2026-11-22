//! What a task's gates are read from, kept between reads ([`Inputs`]), each part by what tells it
//! has not moved, so a read with nothing new reads none of it again and still puts the gates
//! together from what stands now:
//!
//! - the task, its thread, its branch and the tasks under it ([`OfBoard`]), by the task's stamp
//!   (`Store::task_stamp`): one small read of the board a gates read, in place of the task, its
//!   thread and its sessions;
//! - the recorder's runs, by its files (`recorded::Kept`): a file is read again once it is another;
//! - the stamps catalogue, by the commit the main checkout's head is at: `Catalogue::of` asks two
//!   `git` processes, once a commit;
//! - whether a tip holds work of its own ([`super::own_commits`]), by that tip and the default
//!   branch's;
//! - what the cards a task's branch posted say of its `owner` stamp at a tip
//!   (`symphony_board::stamps::posted::read`), by that tip and the default branch's: a few `git`
//!   processes, read only where the owner's approve on the thread does not name the tip.
//!
//! The refs themselves (the branch's tip, the default branch's, the main checkout's head) are
//! resolved on every read, as nothing cheaper says a ref has not moved.
//!
//! One thing is read later than it changes. The tasks under a task, whose runs the `tests` stamp
//! counts, are read with the task ([`OfBoard::ours`]), and its stamp holds only the tasks right
//! under it: a task moved under one of its subtasks joins at the task's next post, role, status
//! or subtask. This is the task page's reading only: a land reads its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use symphony_board::cli::machine_work::recorded::{self, Record};
use symphony_board::lock::Recovered as _;
use symphony_board::stamps::Catalogue;
use symphony_board::stamps::posted::{self, Posted};
use symphony_board::{Error, Message, Store, Task, TaskStamp};

/// How many times each part a task's gates are read from was read anew ([`super::Kept::reads`]):
/// a read that found nothing new adds to none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// A task and its thread, read from the board.
    pub tasks: usize,
    /// The recorder's history, both its files.
    pub histories: usize,
    /// One operation's record in the recorder's folder.
    pub records: usize,
    /// The stamps catalogue, by `git`.
    pub catalogues: usize,
    /// Whether a tip holds work of its own, a merge base.
    pub merge_bases: usize,
    /// What the cards a branch posted say of its `owner` stamp at a tip.
    pub posted: usize,
}

/// A task as the board held it at one stamp: what its gates read of the board.
pub struct OfBoard {
    stamp: Option<TaskStamp>,
    pub record: Task,
    pub thread: Vec<Message>,
    /// The branch its gates are answered for (`taskview::approval_branch`).
    pub branch: String,
    ours: OnceLock<Vec<String>>,
}

impl OfBoard {
    /// The task and every task under it ([`super::stamps::family`]), read once a stamp, when a
    /// gate first reads the runs of the work under it.
    pub fn ours(&self, store: &Store) -> &[String] {
        self.ours
            .get_or_init(|| super::stamps::family(store, &self.record.slug))
    }
}

/// One checkout's work folder, found once, and its recorder's runs.
struct Checkout {
    work: Option<PathBuf>,
    runs: recorded::Kept,
}

/// Whether a task's tip holds work of its own, as one read found it.
struct Own {
    /// The task's tip.
    tip: String,
    /// The default branch's.
    default_tip: Option<String>,
    holds: bool,
}

/// What the cards a task's branch posted say of its `owner` stamp, as one read found it.
struct Cards {
    /// The task's tip.
    tip: String,
    /// The default branch's.
    default_tip: Option<String>,
    posted: Posted,
}

/// What tasks' gates are read from, kept between reads (the module's doc).
#[derive(Default)]
pub struct Inputs {
    /// By checkout and slug.
    tasks: Mutex<HashMap<(PathBuf, String), Arc<OfBoard>>>,
    /// By checkout.
    checkouts: Mutex<HashMap<PathBuf, Checkout>>,
    /// By main checkout: the commit its head was at, and the catalogue read there.
    catalogues: Mutex<HashMap<PathBuf, (String, Catalogue)>>,
    /// By checkout and slug.
    own: Mutex<HashMap<(PathBuf, String), Own>>,
    /// By checkout and slug.
    cards: Mutex<HashMap<(PathBuf, String), Cards>>,
    read_tasks: AtomicUsize,
    read_catalogues: AtomicUsize,
    read_merge_bases: AtomicUsize,
    read_posted: AtomicUsize,
}

impl Inputs {
    /// What was read anew so far.
    pub fn reads(&self) -> Reads {
        let (mut histories, mut records) = (0, 0);
        for checkout in self.checkouts.lock().recover().values() {
            let read = checkout.runs.reads();
            histories += read.histories;
            records += read.records;
        }
        Reads {
            tasks: self.read_tasks.load(Ordering::SeqCst),
            histories,
            records,
            catalogues: self.read_catalogues.load(Ordering::SeqCst),
            merge_bases: self.read_merge_bases.load(Ordering::SeqCst),
            posted: self.read_posted.load(Ordering::SeqCst),
        }
    }

    /// The task `slug` of the project at `root` as the board holds it: as kept while its stamp is
    /// the one it was read at, else read now, with its thread and its branch. The stamp is read
    /// first, so a change that lands while the task is read is found by the next read. A slug no
    /// task has is refused as `Store::get` refuses it, and nothing is kept of it.
    pub fn task(&self, store: &Store, root: &Path, slug: &str) -> Result<Arc<OfBoard>, Error> {
        let stamp = store.task_stamp(slug)?;
        let key = (root.to_path_buf(), slug.to_string());
        if stamp.is_some()
            && let Some(kept) = self.tasks.lock().recover().get(&key)
            && kept.stamp == stamp
        {
            return Ok(Arc::clone(kept));
        }
        self.read_tasks.fetch_add(1, Ordering::SeqCst);
        let record = store.get(slug)?;
        let thread = store.thread(slug)?;
        let branch = crate::taskview::approval_branch(store, &record)?;
        let read = Arc::new(OfBoard {
            stamp,
            record,
            thread,
            branch,
            ours: OnceLock::new(),
        });
        if read.stamp.is_some() {
            self.tasks.lock().recover().insert(key, Arc::clone(&read));
        }
        Ok(read)
    }

    /// Every run the recorder holds for the repository at `root` ([`super::runs`]), each file read
    /// again only once it is another (`recorded::Kept`); none outside a repository. Reads of one
    /// checkout's runs take turns, so two at once read each file once.
    pub fn runs(&self, root: &Path) -> Arc<Vec<Record>> {
        let mut checkouts = self.checkouts.lock().recover();
        let checkout = checkouts
            .entry(root.to_path_buf())
            .or_insert_with(|| Checkout {
                work: super::work_folder(root),
                runs: recorded::Kept::default(),
            });
        if checkout.work.is_none() {
            checkout.work = super::work_folder(root);
        }
        match &checkout.work {
            Some(folder) => checkout.runs.runs(folder),
            None => Arc::default(),
        }
    }

    /// The stamps catalogue of the repository at `root` (`Catalogue::at`): as kept while its main
    /// checkout's head is at the commit it was read at, else read now. The head is read first, so
    /// a commit that lands while the catalogue is read is found by the next read. One that cannot
    /// be read is not kept.
    pub fn catalogue(&self, root: &Path) -> Result<Catalogue, String> {
        let Some(main) = Catalogue::main_of(root) else {
            return Ok(Catalogue::symphony());
        };
        let [head] = super::commits(&main, ["HEAD"]);
        if let Some(head) = &head
            && let Some((at, kept)) = self.catalogues.lock().recover().get(&main)
            && at == head
        {
            return Ok(kept.clone());
        }
        self.read_catalogues.fetch_add(1, Ordering::SeqCst);
        let read = Catalogue::of(&main)?;
        if let Some(head) = head {
            self.catalogues
                .lock()
                .recover()
                .insert(main, (head, read.clone()));
        }
        Ok(read)
    }

    /// Whether `tip`, the tip of the task `slug`'s branch in the repository at `root`, holds work
    /// of its own past the branch `default`, whose tip is `default_tip` ([`super::own_commits`]):
    /// as kept while both tips are the ones it was read at.
    pub fn own_commits(
        &self,
        root: &Path,
        slug: &str,
        tip: &str,
        (default, default_tip): (&str, Option<&str>),
    ) -> bool {
        let key = (root.to_path_buf(), slug.to_string());
        if let Some(kept) = self.own.lock().recover().get(&key)
            && kept.tip == tip
            && kept.default_tip.as_deref() == default_tip
        {
            return kept.holds;
        }
        self.read_merge_bases.fetch_add(1, Ordering::SeqCst);
        let holds = super::own_commits(root, tip, default);
        let kept = Own {
            tip: tip.to_string(),
            default_tip: default_tip.map(str::to_string),
            holds,
        };
        self.own.lock().recover().insert(key, kept);
        holds
    }

    /// What the cards the branch of the task `slug` posted say of its `owner` stamp at `tip`, in
    /// the main checkout `root` (`posted::read`), the default branch's tip `default_tip`: as kept
    /// while both tips are the ones it was read at.
    pub fn posted(&self, root: &Path, slug: &str, tip: &str, default_tip: Option<&str>) -> Posted {
        let key = (root.to_path_buf(), slug.to_string());
        if let Some(kept) = self.cards.lock().recover().get(&key)
            && kept.tip == tip
            && kept.default_tip.as_deref() == default_tip
        {
            return kept.posted.clone();
        }
        self.read_posted.fetch_add(1, Ordering::SeqCst);
        let read = posted::read(root, tip);
        let kept = Cards {
            tip: tip.to_string(),
            default_tip: default_tip.map(str::to_string),
            posted: read.clone(),
        };
        self.cards.lock().recover().insert(key, kept);
        read
    }

    /// Forget what is kept of every task `open` does not name, in every checkout.
    pub fn keep_open(&self, open: impl Fn(&str) -> bool) {
        self.tasks
            .lock()
            .recover()
            .retain(|(_, slug), _| open(slug));
        self.own.lock().recover().retain(|(_, slug), _| open(slug));
        self.cards
            .lock()
            .recover()
            .retain(|(_, slug), _| open(slug));
    }
}

#[cfg(test)]
mod tests;
