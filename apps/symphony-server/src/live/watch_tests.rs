//! Watch updates use one backend transaction, with no restart for an unchanged path set.

use super::*;
use crate::testing::{Tree, repository};
use notify::{Config, EventHandler, PathsMut, WatcherKind};

/// A project using the plugin that watches the hull models.
const MANIFEST: &str = "[plugins]\ncrates = [\"star-foundry-symphony\"]\n";

#[derive(Default)]
struct Watch {
    active: BTreeMap<PathBuf, RecursiveMode>,
    batches: usize,
    commits: usize,
    fail_add: Option<PathBuf>,
    fail_commit: bool,
}

impl Watcher for Watch {
    fn new<F: EventHandler>(_: F, _: Config) -> notify::Result<Self> {
        Ok(Self::default())
    }

    fn watch(&mut self, _: &Path, _: RecursiveMode) -> notify::Result<()> {
        panic!("individual watch restarts the backend stream");
    }

    fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
        panic!("individual unwatch restarts the backend stream");
    }

    fn paths_mut(&mut self) -> Box<dyn PathsMut + '_> {
        self.batches += 1;
        Box::new(Update(self))
    }

    fn kind() -> WatcherKind {
        WatcherKind::NullWatcher
    }
}

struct Update<'a>(&'a mut Watch);

impl PathsMut for Update<'_> {
    fn add(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        if self.0.fail_add.as_deref() == Some(path) {
            return Err(notify::Error::generic("fixture add failure"));
        }
        assert!(self.0.active.insert(path.into(), mode).is_none());
        Ok(())
    }

    fn remove(&mut self, path: &Path) -> notify::Result<()> {
        assert!(self.0.active.remove(path).is_some());
        Ok(())
    }

    fn commit(self: Box<Self>) -> notify::Result<()> {
        self.0.commits += 1;
        if self.0.fail_commit {
            self.0.active.clear();
            Err(notify::Error::generic("fixture commit failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn watches_are_batched_and_unchanged_inputs_do_not_restart_the_stream() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    let hub = Hub::new(&root, &root.join("build/client"));
    let mut watcher = Watch::default();
    let mut watched = BTreeSet::new();
    let expected: BTreeMap<_, _> = hub
        .watch_paths()
        .into_iter()
        .filter(|(path, _)| path.is_dir())
        .collect();
    assert!(expected.len() > 1, "exercise multiple stream inputs");
    hub.watch(&mut watcher, &mut watched);
    assert_eq!(watcher.active, expected);
    assert_eq!(watched, expected.keys().cloned().collect());
    assert_eq!((watcher.batches, watcher.commits), (1, 1));
    hub.watch(&mut watcher, &mut watched);
    assert_eq!((watcher.batches, watcher.commits), (1, 1));

    std::fs::rename(root.join("docs"), root.join("retired-docs")).unwrap();
    tree.write("main/view/models/hulls/A.glb", "model");
    hub.watch(&mut watcher, &mut watched);
    assert_eq!((watcher.batches, watcher.commits), (2, 2));
    assert!(!watcher.active.contains_key(&root.join("docs")));
    assert_eq!(
        watcher.active[&root.join("view/models")],
        RecursiveMode::Recursive
    );
    assert_eq!(watched, watcher.active.keys().cloned().collect());
}

#[test]
fn failed_adds_are_retried_and_a_failed_commit_is_not_marked_watched() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    let hub = Hub::new(&root, &root.join("build/client"));
    let docs = root.join("docs");
    let mut watcher = Watch {
        fail_add: Some(docs.clone()),
        ..Watch::default()
    };
    let mut watched = BTreeSet::new();
    hub.watch(&mut watcher, &mut watched);
    assert!(!watched.contains(&docs));
    assert!(!watcher.active.contains_key(&docs));
    assert!(!watched.is_empty(), "other inputs still apply");
    watcher.fail_add = None;
    hub.watch(&mut watcher, &mut watched);
    assert!(watched.contains(&docs));
    assert!(watcher.active.contains_key(&docs));
    assert_eq!((watcher.batches, watcher.commits), (2, 2));

    tree.write("main/view/models/hulls/A.glb", "model");
    watcher.fail_commit = true;
    hub.watch(&mut watcher, &mut watched);
    assert!(watched.is_empty());
    watcher.fail_commit = false;
    hub.watch(&mut watcher, &mut watched);
    assert!(watched.contains(&docs));
    assert!(watched.contains(&root.join("view/models")));
    assert_eq!(watched, watcher.active.keys().cloned().collect());
    assert_eq!((watcher.batches, watcher.commits), (4, 4));
}
