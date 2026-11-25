//! Watch updates use one backend transaction, with no restart for an unchanged path set.

use super::*;
use crate::testing::{Tree, repository, shelf};
use notify::{Config, EventHandler, PathsMut, WatcherKind};

/// A project using the plugin that watches its shelf.
const MANIFEST: &str = shelf::MANIFEST;

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
    shelf::install().unwrap();
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
    tree.write("main/room/shelf/items/A.item", "item");
    hub.watch(&mut watcher, &mut watched);
    assert_eq!((watcher.batches, watcher.commits), (2, 2));
    assert!(!watcher.active.contains_key(&root.join("docs")));
    assert_eq!(
        watcher.active[&root.join("room/shelf")],
        RecursiveMode::Recursive
    );
    assert_eq!(watched, watcher.active.keys().cloned().collect());
}

#[test]
fn failed_adds_are_retried_and_a_failed_commit_is_not_marked_watched() {
    shelf::install().unwrap();
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

    tree.write("main/room/shelf/items/A.item", "item");
    watcher.fail_commit = true;
    hub.watch(&mut watcher, &mut watched);
    assert!(watched.is_empty());
    watcher.fail_commit = false;
    hub.watch(&mut watcher, &mut watched);
    assert!(watched.contains(&docs));
    assert!(watched.contains(&root.join("room/shelf")));
    assert_eq!(watched, watcher.active.keys().cloned().collect());
    assert_eq!((watcher.batches, watcher.commits), (4, 4));
}

#[test]
fn the_watch_follows_the_projects_entrypoints() {
    shelf::install().unwrap();
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \"spec\"\n\
         [[entrypoints]]\nurl = \"/notes/\"\nfolder = \"spec/notes\"\n\
         [[entrypoints]]\nurl = \"/manual/\"\nfolder = \"manual\"\n",
    );
    assert_eq!(
        tree_inputs(&root),
        [
            ("spec".to_string(), RecursiveMode::Recursive),
            ("manual".to_string(), RecursiveMode::Recursive),
        ],
        "a folder inside a watched one is watched with it, and docs/ is no entrypoint's"
    );
    tree.write(
        "main/symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\n",
    );
    assert_eq!(
        tree_inputs(&root),
        [(String::new(), RecursiveMode::Recursive)]
    );
    let hub = Hub::new(&root, &root.join("build/client"));
    assert!(
        hub.watch_paths()
            .contains(&(root.clone(), RecursiveMode::Recursive)),
        "the project's root itself"
    );
}

#[test]
fn gallery_paths_include_input_directories_and_deleted_files_but_not_neighbours() {
    shelf::install().unwrap();
    let tree = Tree::new();
    tree.write("main/symphony.toml", MANIFEST);
    tree.write(
        "main/plain/README.md",
        "a checkout inside main, its project plain",
    );
    let (main, plain) = (tree.path("main"), tree.path("main/plain"));
    let roots = [main.clone(), plain.clone()];
    for relative in [
        "room/shelf",
        "room/shelf/items/A.item",
        "posters",
        "posters/redraw/A.png",
        "docs/labels",
        "docs/labels/A.notes.json",
    ] {
        assert_eq!(
            watched_root(&roots, &main.join(relative)),
            Some(main.as_path()),
            "{relative}"
        );
        // The plain project's plugins watch nothing, so its paths belong to none.
        assert_eq!(
            watched_root(&roots, &plain.join(relative)),
            None,
            "{relative}"
        );
    }
    for relative in [
        "docs/TOPIC.md",
        "room/shelf-old/A.item",
        "posters-other/A.png",
        "docs/labels-other/A.svg",
    ] {
        assert_eq!(
            watched_root(&roots, &main.join(relative)),
            None,
            "{relative}"
        );
    }
}

#[test]
fn a_tree_watches_its_projects_documents_folder_and_what_its_plugins_watch_outside_it() {
    shelf::install().unwrap();
    let tree = Tree::new();
    let manual = "[project]\nregister = \"manual/DECISIONS.md\"\nartifacts = \"output\"\n\
                  [[entrypoints]]\nurl = \"/docs/\"\nfolder = \"manual\"\nhistory = [\"past\"]\n";
    tree.write("plain/symphony.toml", manual);
    let below =
        |root: &str| -> Vec<(String, notify::RecursiveMode)> { tree_inputs(&tree.path(root)) };
    let recursive = |paths: &[&str]| -> Vec<(String, notify::RecursiveMode)> {
        paths
            .iter()
            .map(|path| (path.to_string(), notify::RecursiveMode::Recursive))
            .collect()
    };
    assert_eq!(below("plain"), recursive(&["manual"]));
    tree.write("game/symphony.toml", &format!("{manual}{MANIFEST}"));
    assert_eq!(
        below("game"),
        recursive(&["manual", "docs/labels", "posters", "room/shelf"])
    );
    // a watched folder inside the documents folder is watched with it
    tree.write("default/symphony.toml", MANIFEST);
    assert_eq!(
        below("default"),
        recursive(&["docs", "posters", "room/shelf"])
    );
}

#[test]
fn watch_root_depth_ties_keep_manifest_order_and_drop_nested_roots() {
    shelf::install().unwrap();
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/symphony.toml", "[[entrypoints]]\nurl='/nested/'\nfolder='root/nested'\n[[entrypoints]]\nurl='/z/'\nfolder='zz'\n[[entrypoints]]\nurl='/a/'\nfolder='aa'\n[[entrypoints]]\nurl='/root/'\nfolder='root'\n");
    assert_eq!(
        tree_inputs(&root),
        [
            ("zz".into(), RecursiveMode::Recursive),
            ("aa".into(), RecursiveMode::Recursive),
            ("root".into(), RecursiveMode::Recursive),
        ]
    );
    tree.write(
        "main/symphony.toml",
        "[[entrypoints]]\nurl='/z/'\nfolder='zz'\n[[entrypoints]]\nurl='/'\nfolder='.'\n",
    );
    assert_eq!(
        tree_inputs(&root),
        [(String::new(), RecursiveMode::Recursive)]
    );
}

#[test]
fn shared_watch_root_sort_preserves_all_equal_length_refs_and_reports_cpu_cost() {
    let folders: Vec<_> = (0..512)
        .map(|index| format!("{index:04}-{}", "x".repeat(index % 11)))
        .collect();
    let mut actual: Vec<_> = folders.iter().rev().collect();
    let mut expected = actual.clone();
    #[expect(
        clippy::disallowed_methods,
        reason = "the standard sort this test holds the shared sorter to"
    )]
    let (_, old_cpu) = test_time::cpu::spent(|| expected.sort_by_key(|folder| folder.len()));
    let (_, new_cpu) = test_time::cpu::spent(|| {
        symphony_board::order::sort_by_key(&mut actual, &|folder| folder.len())
    });
    assert!(
        actual
            .iter()
            .zip(&expected)
            .all(|(a, b)| std::ptr::eq(*a, *b))
    );
    assert_eq!(actual.len(), expected.len());
    eprintln!(
        "watch-root report sort: n={}, reference_bytes={}, position_bytes={}, old_thread_cpu={old_cpu:?}, shared_thread_cpu={new_cpu:?}; additional usize-sort scratch; production rebuilds watch inputs on configuration refresh",
        actual.len(),
        std::mem::size_of::<&String>(),
        actual.len() * std::mem::size_of::<usize>()
    );
}

#[test]
fn native_tree_maps_match_listing_on_startup_addition_and_removal() {
    let tree = Tree::new();
    let root = repository(&tree);
    let compare = |hub: &Hub| {
        let expected: BTreeMap<_, _> = branches::trees(&root)
            .into_iter()
            .map(|t| (t.id(), t.path))
            .collect();
        assert_eq!(*hub.trees.read().unwrap(), expected);
        for (id, path) in expected {
            assert_eq!(hub.tree(&id), Some(path));
        }
    };
    let hub = Hub::new(&root, &root.join("build/client"));
    compare(&hub);
    let branch = tree.path("branch-z");
    crate::testing::git(
        &root,
        &["worktree", "add", "-b", "tools/z", branch.to_str().unwrap()],
    );
    let second = tree.path("branch-a");
    crate::testing::git(
        &root,
        &["worktree", "add", "-b", "tools/a", second.to_str().unwrap()],
    );
    let started = Hub::new(&root, &root.join("build/client"));
    compare(&started);
    hub.refresh_docs(None).unwrap();
    compare(&hub);
    assert_eq!(
        hub.trees
            .read()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["tools~a", "tools~z"]
    );
    crate::testing::git(&root, &["worktree", "remove", branch.to_str().unwrap()]);
    hub.refresh_docs(None).unwrap();
    compare(&hub);
    assert!(hub.tree("tools~z").is_none());
}
