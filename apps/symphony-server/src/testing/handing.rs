//! A `notify` watcher whose backend is the test. A loop under test makes it, as it makes the
//! machine's own, and a test tells it the events a backend would: no event a loaded machine can
//! lose. FSEvents can drop one outright or fold it into a rescan of a folder above the one
//! watched, which notify filters out ([`crate::backstop`]). A loop on a period no test reaches
//! would then wait for good.
//!
//! A test that holds the watcher tells it directly ([`Handing::tell`]). One whose watcher a loop
//! made on a thread of its own finds it by the folder it watches ([`watching`], [`tell_at`]): each
//! test watches a folder of its own, so the tests running beside it never meet there. What either
//! tells is an event as a backend says it ([`event`], [`renamed`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use symphony_board::lock::Recovered as _;

/// The handler a [`Handing`] watcher was given.
type Handler = Arc<Mutex<Box<dyn notify::EventHandler>>>;

/// Each folder a [`Handing`] watcher still alive watches: how, and its handler.
type Watched = BTreeMap<PathBuf, (notify::RecursiveMode, Handler)>;

static HANDED: Mutex<Watched> = Mutex::new(BTreeMap::new());

fn handed() -> MutexGuard<'static, Watched> {
    HANDED.lock().recover()
}

/// A watcher whose backend is the test: each folder it watches is listed, with its handler, while
/// it lives. When `FAILS` it fails to watch, and its handler goes with it as a dropped watcher's
/// does.
pub struct Handing<const FAILS: bool> {
    handler: Handler,
    /// What it was asked to watch, in order.
    pub watched: Vec<(PathBuf, notify::RecursiveMode)>,
}

impl<const FAILS: bool> Handing<FAILS> {
    /// Its handler is told `event`, as its backend would tell it.
    pub fn tell(&self, event: notify::Result<notify::Event>) {
        told(&self.handler, event);
    }
}

impl<const FAILS: bool> notify::Watcher for Handing<FAILS> {
    fn new<F: notify::EventHandler>(handler: F, _: notify::Config) -> notify::Result<Self> {
        Ok(Self {
            handler: Arc::new(Mutex::new(Box::new(handler))),
            watched: Vec::new(),
        })
    }

    fn watch(&mut self, path: &Path, mode: notify::RecursiveMode) -> notify::Result<()> {
        if FAILS {
            return Err(notify::Error::generic("no such folder"));
        }
        self.watched.push((path.into(), mode));
        handed().insert(path.into(), (mode, self.handler.clone()));
        Ok(())
    }

    fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
        Ok(())
    }

    fn kind() -> notify::WatcherKind {
        notify::WatcherKind::NullWatcher
    }
}

impl<const FAILS: bool> Drop for Handing<FAILS> {
    /// Each folder it watched goes from the list where the entry is still its own: a watcher of
    /// that folder made after it keeps its own.
    fn drop(&mut self) {
        let mut handed = handed();
        for (path, _) in &self.watched {
            if handed
                .get(path)
                .is_some_and(|(_, handler)| Arc::ptr_eq(handler, &self.handler))
            {
                handed.remove(path);
            }
        }
    }
}

/// A backend's event of `kind` naming `paths`.
pub fn event(
    kind: notify::EventKind,
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> notify::Result<notify::Event> {
    Ok(paths
        .into_iter()
        .fold(notify::Event::new(kind), |event, path| {
            event.add_path(path.as_ref().to_path_buf())
        }))
}

/// A backend's event of `path` renamed into place.
pub fn renamed(path: impl AsRef<Path>) -> notify::Result<notify::Event> {
    let kind = notify::EventKind::Modify(notify::event::ModifyKind::Name(
        notify::event::RenameMode::Any,
    ));
    event(kind, [path])
}

/// How a live [`Handing`] watcher watches `folder`, if one does.
pub fn watching(folder: &Path) -> Option<notify::RecursiveMode> {
    handed().get(folder).map(|(mode, _)| *mode)
}

/// The live [`Handing`] watcher of `folder` is told `event`, as its backend would tell it.
pub fn tell_at(folder: &Path, event: notify::Result<notify::Event>) {
    let handler = handed()
        .get(folder)
        .map(|(_, handler)| handler.clone())
        .unwrap_or_else(|| panic!("no watcher watches {}", folder.display()));
    told(&handler, event);
}

fn told(handler: &Handler, event: notify::Result<notify::Event>) {
    handler.lock().recover().handle_event(event);
}

#[cfg(test)]
mod tests {
    use super::{Handing, event, renamed, tell_at, watching};
    use notify::event::{CreateKind, ModifyKind, RenameMode};
    use notify::{EventKind, RecursiveMode, Watcher};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    /// The path each event received names.
    fn paths(events: &mpsc::Receiver<notify::Result<notify::Event>>) -> Vec<PathBuf> {
        events
            .try_iter()
            .flat_map(|event| event.unwrap().paths)
            .collect()
    }

    #[test]
    fn a_watcher_is_found_by_its_folder_while_it_lives_and_each_hears_its_own_events() {
        let (one, two) = (Path::new("/handing/one"), Path::new("/handing/two"));
        let (send_one, heard_one) = mpsc::channel();
        let (send_two, heard_two) = mpsc::channel();
        let mut first = Handing::<false>::new(send_one, notify::Config::default()).unwrap();
        assert_eq!(watching(one), None, "listed only once it watches");
        first.watch(one, RecursiveMode::NonRecursive).unwrap();
        let mut second = Handing::<false>::new(send_two, notify::Config::default()).unwrap();
        second.watch(two, RecursiveMode::Recursive).unwrap();
        assert_eq!(
            first.watched,
            [(one.to_path_buf(), RecursiveMode::NonRecursive)]
        );
        assert_eq!(
            (watching(one), watching(two)),
            (
                Some(RecursiveMode::NonRecursive),
                Some(RecursiveMode::Recursive)
            )
        );
        tell_at(one, renamed("/handing/one/a"));
        first.tell(renamed("/handing/one/b"));
        tell_at(two, renamed("/handing/two/c"));
        assert_eq!(
            paths(&heard_one),
            [PathBuf::from("/handing/one/a"), "/handing/one/b".into()]
        );
        assert_eq!(paths(&heard_two), [PathBuf::from("/handing/two/c")]);
        drop(first);
        assert_eq!(watching(one), None, "it goes with its watcher");
        assert_eq!(watching(two), Some(RecursiveMode::Recursive));
        drop(second);
        assert_eq!(watching(two), None);
    }

    #[test]
    fn a_watcher_dropped_after_another_took_its_folder_leaves_the_others_entry() {
        let folder = Path::new("/handing/taken");
        let (send_old, heard_old) = mpsc::channel();
        let (send_new, heard_new) = mpsc::channel();
        let mut old = Handing::<false>::new(send_old, notify::Config::default()).unwrap();
        old.watch(folder, RecursiveMode::NonRecursive).unwrap();
        let mut new = Handing::<false>::new(send_new, notify::Config::default()).unwrap();
        new.watch(folder, RecursiveMode::Recursive).unwrap();
        drop(old);
        assert_eq!(watching(folder), Some(RecursiveMode::Recursive));
        tell_at(folder, renamed("/handing/taken/a"));
        assert_eq!(paths(&heard_new), [PathBuf::from("/handing/taken/a")]);
        assert_eq!(paths(&heard_old), Vec::<PathBuf>::new());
        drop(new);
        assert_eq!(watching(folder), None);
    }

    #[test]
    fn an_event_names_its_kind_and_each_path_in_order() {
        let made = event(EventKind::Create(CreateKind::Any), ["/a", "/b"]).unwrap();
        assert_eq!(made.kind, EventKind::Create(CreateKind::Any));
        assert_eq!(made.paths, [PathBuf::from("/a"), "/b".into()]);
        let moved = renamed(Path::new("/c")).unwrap();
        assert_eq!(
            moved.kind,
            EventKind::Modify(ModifyKind::Name(RenameMode::Any))
        );
        assert_eq!(moved.paths, [PathBuf::from("/c")]);
    }

    #[test]
    fn a_failing_watcher_watches_nothing() {
        let folder = Path::new("/handing/failing");
        let (send, _) = mpsc::channel();
        let mut watcher = Handing::<true>::new(send, notify::Config::default()).unwrap();
        assert!(watcher.watch(folder, RecursiveMode::NonRecursive).is_err());
        assert!(watcher.watched.is_empty());
        assert_eq!(watching(folder), None);
    }

    #[test]
    #[should_panic(expected = "no watcher watches /handing/nobody")]
    fn telling_a_folder_no_watcher_watches_says_so() {
        tell_at(Path::new("/handing/nobody"), renamed("/handing/nobody/a"));
    }
}
