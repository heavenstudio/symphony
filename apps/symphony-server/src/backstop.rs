//! The check each watching loop runs on its own, once per [`PERIOD`], beside its file watcher.
//! Under load macOS FSEvents can drop an event outright: no rescan flag reaches the stream, or one
//! names a path above the watched folders, which notify filters out. A loop that learned of edits
//! only from events would then miss them until the next unrelated event.
//!
//! A loop waits on its [`Wakes`]: its watcher's handler wakes it, and so does whatever else it
//! waits for (the server's stop, a starter's hand-off request, the hub's drain), each by a message
//! through a [`Waker`]. Nothing else ends its wait but the period.

use notify::RecursiveMode;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use symphony_board::lock::Recovered as _;

/// A missed event costs at most this plus one check. Under a load of 160 a document refresh took
/// 10.5 s, so a missed edit still shows within 30 s; one sweep of the main checkout's watched
/// folders costs about 0.1 s of CPU, about 1% of a core at this period.
pub(crate) const PERIOD: Duration = Duration::from_secs(10);

/// What wakes a loop that waits on its [`Wakes`]. A wake given while one is waiting already is
/// that one: the loop that wakes looks at everything it waits for.
#[derive(Clone)]
pub(crate) struct Waker(mpsc::SyncSender<()>);

impl Waker {
    /// Wake the loop: whether it is still there to wake.
    pub(crate) fn wake(&self) -> bool {
        !matches!(
            self.0.try_send(()),
            Err(mpsc::TrySendError::Disconnected(()))
        )
    }
}

/// The wakes of one loop: the channel it waits on, of which it keeps a sender of its own, so its
/// wait ends at a wake or at the bound it gives, never because every waker went.
pub(crate) struct Wakes {
    waker: Waker,
    woken: mpsc::Receiver<()>,
}

impl Wakes {
    pub(crate) fn new() -> Self {
        let (send, woken) = crate::wake::sync_channel(1);
        Self {
            waker: Waker(send),
            woken,
        }
    }

    /// What wakes this loop's wait.
    pub(crate) fn waker(&self) -> Waker {
        self.waker.clone()
    }

    /// Until a wake, or until `within` has passed when it is given: whether a wake came.
    pub(crate) fn wait(&self, within: Option<Duration>) -> bool {
        match within {
            None => self.woken.recv().is_ok(),
            Some(within) => self.woken.recv_timeout(within).is_ok(),
        }
    }

    /// [`debounce`] of these wakes.
    pub(crate) fn debounce(&self, quiet: Duration, most: Duration) {
        debounce(&self.woken, quiet, most);
    }
}

/// The wakers one kind of event wakes ([`crate::front::Shared`]'s stop, a starter's hand-off
/// request, a hub's board found or lost). A waker whose loop has gone is let go at the next wake.
#[derive(Default)]
pub(crate) struct Wakers(Mutex<Vec<Waker>>);

impl Wakers {
    pub(crate) fn add(&self, waker: Waker) {
        self.0.lock().recover().push(waker);
    }

    /// Wake every loop waiting on these.
    pub(crate) fn wake(&self) {
        self.0.lock().recover().retain(Waker::wake);
    }

    /// How many loops these would wake.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.lock().recover().len()
    }
}

/// Why a watching loop woke.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// A wake came: the watcher reported a change, or something else the loop waits for wants a
    /// look (its stop, a hand-off asked for), which the loop then looks at.
    Event,
    /// A period passed: check without an event.
    Period,
}

/// Waits on a loop's [`Wakes`] and asks for a check of its own once per period, wakes or not.
pub(crate) struct Backstop {
    period: Duration,
    due: Instant,
    /// The time now, by which the period runs: the machine's, or a test's ([`Backstop::timed_by`]).
    clock: Box<dyn Fn() -> Instant + Send>,
    wakes: Wakes,
}

impl Backstop {
    pub fn new(period: Duration) -> Self {
        Self::timed_by(period, Instant::now)
    }

    /// A backstop whose period runs by `clock`.
    fn timed_by(period: Duration, clock: impl Fn() -> Instant + Send + 'static) -> Self {
        Self {
            period,
            due: clock() + period,
            clock: Box::new(clock),
            wakes: Wakes::new(),
        }
    }

    /// What wakes this loop's wait: its watcher's handler, and what else it waits for.
    pub fn waker(&self) -> Waker {
        self.wakes.waker()
    }

    /// Until a wake, or the period's end on its clock, whichever comes first. A period due is
    /// said before a wake waiting, so wakes never hold it back.
    pub fn wait(&mut self) -> Wake {
        loop {
            let now = (self.clock)();
            if now >= self.due {
                self.due = now + self.period;
                return Wake::Period;
            }
            if self.wakes.wait(Some(self.due - now)) {
                return Wake::Event;
            }
        }
    }

    /// [`debounce`] of this loop's wakes, once a wake began a burst.
    pub fn debounce(&self, quiet: Duration, most: Duration) {
        self.wakes.debounce(quiet, most);
    }
}

/// A period no test reaches: a loop that a test runs on it, to show one of its wakes, waits for
/// that wake alone, and a wake missed fails the test by its named stall, never at the period's end.
#[cfg(test)]
pub(crate) const NO_PERIOD: Duration = Duration::from_secs(86_400);

/// A backstop on [`NO_PERIOD`] whose clock, the machine's, counts its reads, and what a test sees
/// of them ([`Waits`]).
#[cfg(test)]
pub(crate) fn watched() -> (Backstop, Waits) {
    let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let read = reads.clone();
    let backstop = Backstop::timed_by(NO_PERIOD, move || {
        read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Instant::now()
    });
    (backstop, Waits(reads))
}

/// The reads of a [`watched`] backstop's clock: one as it was made, then one as each wait begins,
/// as no wait on [`NO_PERIOD`] reads it again.
#[cfg(test)]
pub(crate) struct Waits(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(test)]
impl Waits {
    /// Until the loop has begun its `nth` wait, counted from one: it is past every look it takes
    /// before that wait, so what the test does next reaches it by a wake alone. A stall fails
    /// naming the wait.
    pub(crate) fn begun(&self, nth: usize) {
        test_time::until_true(
            format!("the loop's wait {nth} to begin"),
            test_time::backstop(Duration::ZERO),
            || self.0.load(std::sync::atomic::Ordering::SeqCst) > nth,
        )
        .unwrap_or_else(|stall| panic!("{stall}"));
    }
}

/// How long a watcher says nothing before the burst its first event began counts as one change:
/// an editor's save is several events (a temporary file written, renamed over the old, its times
/// set), and a refresh for each would read the file half saved.
pub(crate) const QUIET: Duration = Duration::from_millis(100);

/// The longest the reload's watcher takes a burst for, a binary being written read once it is
/// quiet: a watcher that never goes quiet (a build writing under a watched folder) still has its
/// changes read this often. The hub takes QUIET as its limit instead, so its pages' updates are
/// never later than QUIET after the first event (`live.rs`).
pub(crate) const BURST: Duration = Duration::from_secs(1);

/// The debounce of a watching loop, the one RULES.md `code/no-timed-waits` names for a watcher's
/// events: once the first event of a burst has woken the loop, every event that follows within
/// `quiet` of the one before is taken with it, until the watcher says nothing for `quiet`, `most`
/// has passed since this began, or the watcher is gone, which ends it at once. No event it took is
/// left in `events`, so a loop looks after it at whatever else a wake it took could have asked.
pub(crate) fn debounce(events: &mpsc::Receiver<()>, quiet: Duration, most: Duration) {
    let began = Instant::now();
    loop {
        let left = most.saturating_sub(began.elapsed());
        if left.is_zero() || events.recv_timeout(quiet.min(left)).is_err() {
            break;
        }
    }
    while events.try_recv().is_ok() {}
}

/// A path's own identity, size and times, the fields [`symphony::text::FileStamp`] compares, read
/// without following a link: FSEvents reports a change behind a link only under the target's own
/// path, so a link outside the swept roots costs no refresh here either.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    /// None for a folder, which a walk enters rather than stamps.
    fn read(path: &Path) -> io::Result<Option<Self>> {
        let meta = std::fs::symlink_metadata(path)?;
        Ok((!meta.is_dir()).then(|| Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }))
    }
}

/// A root to sweep: the folder it belongs to (a checkout, the client build, the common folder),
/// the path below that folder, and whether its subfolders count.
pub(crate) type Root = (PathBuf, String, RecursiveMode);

/// A root as a walk uses it: its folder, the path it covers, and whether its subfolders count.
type Covering = (PathBuf, PathBuf, RecursiveMode);

/// The stamps of every file under the roots a loop sweeps, from its last walk.
#[derive(Default)]
pub(crate) struct Sweep {
    /// The last walk's roots.
    roots: Vec<Covering>,
    stamps: BTreeMap<PathBuf, Stamp>,
    /// Stamps taken just before a newly served tree's first read, for the walk that first
    /// covers it.
    adopted: BTreeMap<PathBuf, Stamp>,
}

impl Sweep {
    /// The first walk, taken before the loop's first read, so a change during that read shows in
    /// the next walk.
    pub fn new(roots: &[Root]) -> Self {
        let roots = covering(roots);
        Self {
            stamps: walk_roots(&roots, &BTreeMap::new()),
            roots,
            adopted: BTreeMap::new(),
        }
    }

    /// Stamp `roots` before their first read, which a later walk covering them compares against.
    pub fn adopt(&mut self, roots: &[Root]) {
        let stamps = walk_roots(&covering(roots), &self.adopted);
        self.adopted.extend(stamps);
    }

    /// Walk `roots` again and return the files added, removed or restamped since the last walk.
    /// A root the last walk did not cover compares with what [`Self::adopt`] stamped before its
    /// first read, and reports every file it never stamped; a root no longer swept reports
    /// nothing.
    pub fn next(&mut self, roots: &[Root]) -> BTreeSet<PathBuf> {
        let roots = covering(roots);
        let stamps = walk_roots(&roots, &self.stamps);
        let before = |path: &Path| {
            if covered(&self.roots, path).is_some() {
                self.stamps.get(path)
            } else {
                self.adopted.get(path)
            }
        };
        let gone = |path: &&PathBuf| !stamps.contains_key(*path) && covered(&roots, path).is_some();
        let mut missed = BTreeSet::new();
        missed.extend(
            stamps
                .iter()
                .filter(|(path, stamp)| before(path) != Some(*stamp))
                .map(|(path, _)| path)
                .chain(self.stamps.keys().filter(gone))
                .chain(
                    self.adopted
                        .keys()
                        .filter(|path| covered(&self.roots, path).is_none())
                        .filter(gone),
                )
                .cloned(),
        );
        self.adopted
            .retain(|path, _| covered(&roots, path).is_none());
        self.roots = roots;
        self.stamps = stamps;
        missed
    }

    /// Take the current stamps of `paths` this sweep covers, before an event's refresh reads them,
    /// so the next walk reports only later changes, not the ones the event already carried. A path
    /// behind a link is left as the walk sees it, which never reads there.
    pub fn restamp<'a>(&mut self, paths: impl IntoIterator<Item = &'a PathBuf>) {
        for path in paths {
            let Some((base, _, _)) = covered(&self.roots, path) else {
                continue;
            };
            if !path.parent().is_some_and(|folder| unlinked(base, folder)) {
                continue;
            }
            match Stamp::read(path) {
                Ok(Some(stamp)) => {
                    self.stamps.insert(path.clone(), stamp);
                }
                Ok(None) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    self.stamps.remove(path);
                }
                Err(_) => {}
            }
        }
    }
}

/// The root of `roots` whose walk reaches `path`: anything below a recursive root, a
/// non-recursive root's own entries.
fn covered<'a>(roots: &'a [Covering], path: &Path) -> Option<&'a Covering> {
    roots.iter().find(|(_, root, mode)| {
        if *mode == RecursiveMode::Recursive {
            path.starts_with(root) && path != root
        } else {
            path.parent() == Some(root)
        }
    })
}

/// `roots` with the paths they cover, linked or not.
fn covering(roots: &[Root]) -> Vec<Covering> {
    roots
        .iter()
        .map(|(base, below, mode)| (base.clone(), joined(base, below), *mode))
        .collect()
}

/// Whether no folder from `base` down to `folder`, `folder` included, is a link: the one guard
/// before the sweep reads anything, so a root reached through a link reads as empty everywhere.
/// `base` itself (a checkout or the common folder) may be reached through links. A missing folder
/// ends the check, as nothing lies behind it.
fn unlinked(base: &Path, folder: &Path) -> bool {
    let Ok(below) = folder.strip_prefix(base) else {
        return false;
    };
    let mut path = base.to_path_buf();
    for part in below.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    true
}

/// `base` with `below` under it; an empty `below` is `base` itself.
pub(crate) fn joined(base: &Path, below: &str) -> PathBuf {
    if below.is_empty() {
        base.to_path_buf()
    } else {
        base.join(below)
    }
}

/// Every file under `roots` with its stamp: a recursive root's whole tree, a non-recursive root's
/// own files. Missing roots and files gone mid-walk are left out. A link is stamped as itself,
/// never followed, as FSEvents does not follow it, and a root reached through one is walked as
/// empty, so what it held reads as removed and the link's target stays unwalked. A folder or file
/// that exists but cannot be read keeps its stamps from `previous`, so a passing error (a full
/// descriptor table under load) reads as no change rather than as files removed and then added
/// again.
fn walk_roots(roots: &[Covering], previous: &BTreeMap<PathBuf, Stamp>) -> BTreeMap<PathBuf, Stamp> {
    let mut stamps = BTreeMap::new();
    for (base, root, mode) in roots {
        if unlinked(base, root) {
            walk(
                root,
                *mode == RecursiveMode::Recursive,
                previous,
                &mut stamps,
            );
        }
    }
    stamps
}

fn walk(
    folder: &Path,
    recursive: bool,
    previous: &BTreeMap<PathBuf, Stamp>,
    into: &mut BTreeMap<PathBuf, Stamp>,
) {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            into.extend(
                previous
                    .range(folder.to_path_buf()..)
                    .take_while(|(path, _)| path.starts_with(folder))
                    .filter(|(path, _)| recursive || path.parent() == Some(folder))
                    .map(|(path, stamp)| (path.clone(), stamp.clone())),
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                if recursive {
                    walk(&path, true, previous, into);
                }
            }
            Ok(_) => match Stamp::read(&path) {
                Ok(Some(stamp)) => {
                    into.insert(path, stamp);
                }
                Ok(None) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => {
                    if let Some(stamp) = previous.get(&path) {
                        into.insert(path, stamp.clone());
                    }
                }
            },
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Backstop, Sweep, Wake, Wakers, Wakes, unlinked};
    use crate::testing::Tree;
    use notify::RecursiveMode;
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A backstop whose period runs by a clock the test moves (`set`, how far past its start),
    /// never by how long a loaded machine took: each wait below ends at once, by a wake or by its
    /// period already due, and none waits out a length of time.
    fn clocked(period: Duration) -> (Backstop, impl Fn(Duration)) {
        let start = Instant::now();
        let now = std::sync::Arc::new(std::sync::Mutex::new(start));
        let read = now.clone();
        let backstop = Backstop::timed_by(period, move || *read.lock().unwrap());
        (backstop, move |by| *now.lock().unwrap() = start + by)
    }

    /// Whether a wake waits in `backstop`, taken if so; never a wait.
    fn waiting(backstop: &Backstop) -> Result<(), mpsc::TryRecvError> {
        backstop.wakes.woken.try_recv()
    }

    #[test]
    fn a_wake_ends_the_wait_at_once_and_wakes_given_while_it_waited_are_one() {
        let hour = Duration::from_secs(3600);
        let (mut backstop, _) = clocked(hour);
        let waker = backstop.waker();
        assert!(waker.wake());
        assert_eq!(backstop.wait(), Wake::Event);
        // Three wakes before the loop looks again: one look is all they ask.
        for _ in 0..3 {
            assert!(waker.wake());
        }
        assert_eq!(backstop.wait(), Wake::Event);
        assert_eq!(waiting(&backstop), Err(mpsc::TryRecvError::Empty));
        // A wake from another thread, before or during the wait: an hour's period never ends it.
        let other = backstop.waker();
        let woke = std::thread::spawn(move || other.wake());
        assert_eq!(backstop.wait(), Wake::Event);
        assert!(woke.join().unwrap());
    }

    #[test]
    fn the_period_comes_on_its_clock_before_a_wake_that_waits() {
        let period = Duration::from_millis(300);
        let (mut backstop, set) = clocked(period);
        set(period);
        assert_eq!(backstop.wait(), Wake::Period);
        // Wakes do not hold the next period back: with one waiting, the period is said first.
        assert!(backstop.waker().wake());
        set(period * 2);
        assert_eq!(backstop.wait(), Wake::Period);
        assert_eq!(backstop.wait(), Wake::Event, "the wake still waited");
        // Past more than a period, one period is said, and the next counts from then.
        set(period * 5);
        assert_eq!(backstop.wait(), Wake::Period);
        assert!(backstop.waker().wake());
        set(period * 6 - Duration::from_millis(1));
        assert_eq!(backstop.wait(), Wake::Event);
        set(period * 6);
        assert_eq!(backstop.wait(), Wake::Period);
    }

    /// A watched backstop's clock is read once as it is made and once as each wait begins, so a
    /// test counts the waits its loop began; a wait woken reads it no more.
    #[test]
    fn a_watched_backstop_counts_each_wait_begun() {
        let (mut backstop, waits) = super::watched();
        assert_eq!(waits.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        let waker = backstop.waker();
        assert!(waker.wake());
        assert_eq!(backstop.wait(), Wake::Event);
        waits.begun(1);
        assert_eq!(waits.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        let woken = std::thread::spawn(move || backstop.wait());
        waits.begun(2);
        assert!(waker.wake());
        assert_eq!(
            crate::testing::joined(
                woken,
                "the wake to end the wait",
                test_time::backstop(Duration::ZERO)
            ),
            Wake::Event
        );
        assert_eq!(waits.0.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn every_waker_gone_leaves_the_wait_to_its_period_never_to_a_spin() {
        let (backstop, _) = clocked(Duration::from_secs(3600));
        drop(backstop.waker());
        // It keeps a sender of its own: nothing it waits on can end, so no wait returns at once.
        assert_eq!(waiting(&backstop), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn wakers_wake_every_loop_still_there_and_let_go_of_the_others() {
        let wakers = Wakers::default();
        let (here, gone) = (Wakes::new(), Wakes::new());
        wakers.add(here.waker());
        wakers.add(gone.waker());
        let waker = gone.waker();
        drop(gone);
        assert!(!waker.wake(), "its loop has gone");
        wakers.wake();
        assert_eq!(wakers.len(), 1);
        assert!(
            here.wait(Some(Duration::ZERO)),
            "the loop still there was woken"
        );
        assert!(
            !here.wait(Some(Duration::ZERO)),
            "and nothing more waits for it"
        );
    }

    /// `debounce` on its own thread, which says on a channel once it has returned; the test waits
    /// for that word, never a length of time.
    fn debounced(
        events: mpsc::Receiver<()>,
        quiet: Duration,
        most: Duration,
    ) -> mpsc::Receiver<mpsc::Receiver<()>> {
        let (said, done) = crate::test_channel::channel();
        std::thread::spawn(move || {
            super::debounce(&events, quiet, most);
            let _ = said.send(events);
        });
        done
    }

    fn returned(done: &mpsc::Receiver<mpsc::Receiver<()>>, what: &str) -> mpsc::Receiver<()> {
        done.recv_timeout(test_time::wait::backstop(Duration::ZERO))
            .unwrap_or_else(|_| {
                panic!(
                    "{what}: the debounce never returned; {}",
                    test_time::Load::now()
                )
            })
    }

    #[test]
    fn a_debounce_takes_the_whole_burst_its_watcher_already_sent() {
        let (send, events) = crate::test_channel::channel();
        for _ in 0..5 {
            send.send(()).unwrap();
        }
        let events = returned(
            &debounced(events, Duration::from_millis(1), Duration::from_secs(3600)),
            "five events waiting",
        );
        assert_eq!(
            events.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "every event of the burst was taken, and the watcher is still there"
        );
        send.send(()).unwrap();
        assert_eq!(
            events.try_recv(),
            Ok(()),
            "a later event is the next burst's"
        );
    }

    #[test]
    fn a_debounce_ends_at_once_when_its_watcher_has_gone() {
        let (send, events) = crate::test_channel::channel();
        send.send(()).unwrap();
        drop(send);
        // No quiet or limit it could wait out: only the watcher's end returns it.
        let hour = Duration::from_secs(3600);
        let events = returned(&debounced(events, hour, hour), "a watcher gone");
        assert_eq!(events.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn a_watcher_that_never_goes_quiet_is_taken_by_the_limit() {
        let (send, events) = crate::test_channel::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sender = std::thread::spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(std::sync::atomic::Ordering::SeqCst) && send.send(()).is_ok() {
                    std::thread::yield_now();
                }
            }
        });
        // Never quiet for an hour: only the limit returns it.
        returned(
            &debounced(events, Duration::from_secs(3600), Duration::from_millis(5)),
            "a watcher sending all the time",
        );
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        sender.join().unwrap();
    }

    #[test]
    fn sweeps_report_added_removed_and_rewritten_files_but_not_nested_ones_of_a_flat_root() {
        let tree = Tree::new();
        let docs = tree.path("docs");
        let flat = tree.path("git");
        tree.write("docs/a.md", "same size");
        tree.write("docs/deep/b.md", "b");
        tree.write("git/packed-refs", "refs");
        tree.write("git/refs/heads/main", "nested under a flat root");
        let roots = vec![
            (docs.clone(), String::new(), RecursiveMode::Recursive),
            (flat.clone(), String::new(), RecursiveMode::NonRecursive),
            (
                tree.path("missing"),
                String::new(),
                RecursiveMode::Recursive,
            ),
        ];
        let mut sweep = Sweep::new(&roots);
        assert_eq!(
            sweep.stamps.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                docs.join("a.md"),
                docs.join("deep/b.md"),
                flat.join("packed-refs")
            ])
        );
        assert!(sweep.next(&roots).is_empty());

        // Same length, written again at once: the nanosecond times still differ.
        tree.write("docs/a.md", "SAME SIZE");
        tree.write("docs/deep/c.md", "added");
        std::fs::remove_file(docs.join("deep/b.md")).unwrap();
        tree.write("git/refs/heads/main", "still nested");
        assert_eq!(
            sweep.next(&roots),
            BTreeSet::from([
                docs.join("a.md"),
                docs.join("deep/b.md"),
                docs.join("deep/c.md")
            ])
        );
        assert!(sweep.next(&roots).is_empty(), "a change is reported once");
    }

    #[test]
    fn a_root_new_to_the_sweep_compares_with_its_adopted_stamps_and_a_dropped_one_reports_nothing()
    {
        let tree = Tree::new();
        let main = (
            tree.path("main/docs"),
            String::new(),
            RecursiveMode::Recursive,
        );
        let served = (
            tree.path("served/docs"),
            String::new(),
            RecursiveMode::Recursive,
        );
        let unadopted = (
            tree.path("unadopted/docs"),
            String::new(),
            RecursiveMode::Recursive,
        );
        tree.write("main/docs/a.md", "a");
        for name in ["untouched", "edited", "removed"] {
            tree.write(&format!("served/docs/{name}.md"), name);
        }
        tree.write("unadopted/docs/x.md", "never stamped before its read");
        let mut sweep = Sweep::new(std::slice::from_ref(&main));
        // The page's first read follows this; its walk may come before or after the read.
        sweep.adopt(std::slice::from_ref(&served));
        assert!(sweep.next(std::slice::from_ref(&main)).is_empty());
        tree.write("served/docs/edited.md", "after the read, its event lost");
        tree.write("served/docs/added.md", "after the read, its event lost");
        std::fs::remove_file(tree.path("served/docs/removed.md")).unwrap();
        assert_eq!(
            sweep.next(&[main.clone(), served.clone(), unadopted.clone()]),
            BTreeSet::from([
                tree.path("served/docs/edited.md"),
                tree.path("served/docs/added.md"),
                tree.path("served/docs/removed.md"),
                tree.path("unadopted/docs/x.md"),
            ])
        );
        assert!(
            sweep.adopted.is_empty(),
            "a covered root's adopted stamps are spent"
        );
        assert!(sweep.next(&[main.clone(), served.clone()]).is_empty());

        // A root no longer swept reports neither its edits nor its removals.
        tree.write("main/docs/a.md", "edited after main stopped being swept");
        std::fs::remove_file(tree.path("served/docs/untouched.md")).unwrap();
        assert_eq!(
            sweep.next(&[served]),
            BTreeSet::from([tree.path("served/docs/untouched.md")])
        );
        std::fs::remove_file(tree.path("served/docs/edited.md")).unwrap();
        assert!(sweep.next(&[]).is_empty());
    }

    #[test]
    fn restamping_what_an_event_carried_leaves_only_later_changes_to_the_sweep() {
        let tree = Tree::new();
        let docs = tree.path("docs");
        tree.write("docs/a.md", "a");
        tree.write("docs/gone.md", "gone");
        tree.write("unserved/docs/x.md", "x");
        let roots = vec![(docs.clone(), String::new(), RecursiveMode::Recursive)];
        let mut sweep = Sweep::new(&roots);
        tree.write("docs/a.md", "edited, and its event arrived");
        tree.write("docs/b.md", "created, and its event arrived");
        std::fs::remove_file(docs.join("gone.md")).unwrap();
        tree.write("unserved/docs/x.md", "an event outside the sweep");
        let carried: Vec<PathBuf> = [
            "docs/a.md",
            "docs/b.md",
            "docs/gone.md",
            "docs",
            "unserved/docs/x.md",
        ]
        .iter()
        .map(|name| tree.path(name))
        .collect();
        sweep.restamp(&carried);
        assert!(!sweep.stamps.contains_key(&tree.path("unserved/docs/x.md")));
        // Nothing the event carried is reported again.
        assert!(sweep.next(&roots).is_empty());
        tree.write("docs/a.md", "edited again, its event lost");
        assert_eq!(sweep.next(&roots), BTreeSet::from([docs.join("a.md")]));
    }

    #[test]
    fn a_folder_or_file_that_cannot_be_read_keeps_its_last_stamps_and_a_missing_one_goes() {
        let tree = Tree::new();
        let docs = tree.path("docs");
        let git = tree.path("git");
        for name in [
            "docs/shut/a.md",
            "docs/listed/b.md",
            "docs/listed/deep/c.md",
            "docs/gone/d.md",
            "git/packed-refs",
            "git/refs/heads/main",
        ] {
            tree.write(name, name);
        }
        // The common folder's shape: a flat root whose refs folder is a recursive root of its own.
        let roots = vec![
            (docs.clone(), String::new(), RecursiveMode::Recursive),
            (git.join("refs"), String::new(), RecursiveMode::Recursive),
            (git.clone(), String::new(), RecursiveMode::NonRecursive),
        ];
        let mut sweep = Sweep::new(&roots);
        assert_eq!(sweep.stamps.len(), 6);
        std::fs::remove_dir_all(docs.join("gone")).unwrap();
        tree.write("git/refs/heads/main", "moved");
        let mode = |path: PathBuf, bits: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap();
        };
        // Not listable; listable but not searchable, so nothing in it can be read; searchable
        // but not listable, so its refs folder is walked while its own files are not.
        mode(docs.join("shut"), 0o000);
        mode(docs.join("listed"), 0o444);
        mode(git.clone(), 0o311);
        let missed = sweep.next(&roots);
        let first = Sweep::new(&roots);
        for folder in [docs.join("shut"), docs.join("listed"), git.clone()] {
            mode(folder, 0o755);
        }
        assert_eq!(
            missed,
            BTreeSet::from([docs.join("gone/d.md"), git.join("refs/heads/main")])
        );
        // A first walk has nothing to keep.
        assert_eq!(
            first.stamps.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([git.join("refs/heads/main")])
        );
    }

    #[test]
    fn a_path_through_a_link_below_its_base_is_neither_walked_nor_restamped_but_a_linked_base_is() {
        let tree = Tree::new();
        tree.write("outside/models/lamps/X.item", "outside model");
        tree.write("checkout/docs/a.md", "a");
        std::fs::create_dir_all(tree.path("checkout/view")).unwrap();
        let checkout = tree.path("checkout");
        std::os::unix::fs::symlink(tree.path("outside/models"), checkout.join("view/models"))
            .unwrap();
        std::os::unix::fs::symlink(tree.path("outside"), checkout.join("media")).unwrap();
        let under = |below: &str| unlinked(&checkout, &checkout.join(below));
        assert!(!under("view/models"), "a linked root");
        assert!(!under("view/models/lamps"), "a folder behind a linked root");
        assert!(!under("media/models"), "a linked parent");
        assert!(under("docs"));
        assert!(
            under("artifacts/concepts"),
            "a missing root has nothing to walk yet"
        );
        assert!(under(""), "the base itself");
        assert!(
            !unlinked(&checkout, &tree.path("outside")),
            "not under the base"
        );
        let linked = tree.path("linked-checkout");
        std::os::unix::fs::symlink(&checkout, &linked).unwrap();
        assert!(unlinked(&linked, &linked.join("docs")));
        // Restamp reads through the same guard, so an event behind the link adds no stamp the
        // walk would then report as removed.
        let roots = vec![
            (checkout.clone(), "docs".into(), RecursiveMode::Recursive),
            (
                checkout.clone(),
                "view/models".into(),
                RecursiveMode::Recursive,
            ),
        ];
        let mut sweep = Sweep::new(&roots);
        assert_eq!(
            sweep.stamps.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([checkout.join("docs/a.md")])
        );
        sweep.restamp(&[checkout.join("view/models/lamps/X.item")]);
        assert_eq!(sweep.stamps.len(), 1);
        assert!(sweep.next(&roots).is_empty());
    }
}
