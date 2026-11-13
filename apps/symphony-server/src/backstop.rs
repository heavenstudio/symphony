//! The check each watching loop runs on its own, once per [`PERIOD`], beside its file watcher.
//! Under load macOS FSEvents can drop an event outright: no rescan flag reaches the stream, or one
//! names a path above the watched folders, which notify filters out. A loop that learned of edits
//! only from events would then miss them until the next unrelated event.

use notify::RecursiveMode;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A missed event costs at most this plus one check. Under a load of 160 a document refresh took
/// 10.5 s, so a missed edit still shows within 30 s; one sweep of the main checkout's watched
/// folders costs about 0.1 s of CPU, about 1% of a core at this period.
pub(crate) const PERIOD: Duration = Duration::from_secs(10);

/// Why a watching loop woke.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// The watcher reported a change.
    Event,
    /// A period passed: check without an event.
    Period,
    /// Neither within one tick, so the loop can look for shutdown.
    Idle,
}

/// Waits on a watcher's events and asks for a check of its own once per period, events or not.
pub(crate) struct Backstop {
    period: Duration,
    due: Instant,
    /// The time now, by which the period runs: the machine's, or a test's ([`Backstop::timed_by`]).
    clock: Box<dyn Fn() -> Instant + Send>,
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
        }
    }

    /// One wait of at most `tick`. A watcher that has gone (or never sends) leaves the period.
    pub fn wait(&mut self, events: &mpsc::Receiver<()>, tick: Duration) -> Wake {
        let now = (self.clock)();
        if now >= self.due {
            self.due = now + self.period;
            return Wake::Period;
        }
        let tick = tick.min(self.due - now);
        match events.recv_timeout(tick) {
            Ok(()) => Wake::Event,
            Err(mpsc::RecvTimeoutError::Timeout) => Wake::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(tick);
                Wake::Idle
            }
        }
    }
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
        let missed = stamps
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
            .cloned()
            .collect();
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
    use super::{Backstop, Sweep, Wake, unlinked};
    use crate::testing::Tree;
    use notify::RecursiveMode;
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn a_watcher_that_never_sends_still_gets_its_period_and_an_event_wakes_at_once() {
        // The period runs by the test's clock, moved on by a tick after each wait: what a wait
        // answers is that clock's, never how long a loaded machine took to let it.
        let start = Instant::now();
        let now = std::sync::Arc::new(std::sync::Mutex::new(start));
        let clock = {
            let now = now.clone();
            move || *now.lock().unwrap()
        };
        let tick = Duration::from_millis(50);
        let pass = || *now.lock().unwrap() += tick;
        let (send, events) = mpsc::sync_channel(1);
        let period = Duration::from_millis(300);
        let mut backstop = Backstop::timed_by(period, clock);
        let mut idle = 0;
        while backstop.wait(&events, tick) == Wake::Idle {
            idle += 1;
            pass();
        }
        assert_eq!(idle, 6, "a tick bounds each wait: six make the period");
        assert_eq!(*now.lock().unwrap() - start, period);
        send.send(()).unwrap();
        assert_eq!(backstop.wait(&events, Duration::from_secs(5)), Wake::Event);
        // Events do not hold the next period back: with one always waiting, it still comes.
        let mut woken = 0;
        loop {
            let _ = send.try_send(());
            match backstop.wait(&events, tick) {
                Wake::Period => break,
                Wake::Event => woken += 1,
                Wake::Idle => panic!("an event was waiting"),
            }
            pass();
        }
        assert_eq!(
            woken, 6,
            "each event woke its wait, and the period came on time"
        );
        assert_eq!(*now.lock().unwrap() - start, period * 2);
        // A watcher that has gone neither spins nor ends the periods.
        while events.try_recv().is_ok() {}
        drop(send);
        let began = Instant::now();
        assert_eq!(backstop.wait(&events, tick), Wake::Idle);
        assert!(began.elapsed() >= Duration::from_millis(40));
        pass();
        while backstop.wait(&events, tick) != Wake::Period {
            pass();
        }
        assert_eq!(*now.lock().unwrap() - start, period * 3);
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
