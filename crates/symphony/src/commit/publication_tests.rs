//! The publication journal's recovery: the locked file it adopts, and the ones it leaves.

use super::*;

/// A temporary folder that is a repository on `main`, with nothing committed.
fn repository() -> crate::test_tree::Tree {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    tree
}

#[test]
fn recovery_adopts_only_the_locked_inode_still_named_by_the_journal() {
    let tree = repository();
    let path = tree.path("publication.json");
    let data = |new: &str| {
        serde_json::json!({"edit":{"rel":"docs/a.md","base":"","new":new},"publication":null})
            .to_string()
    };
    std::fs::write(&path, data("earlier")).unwrap();
    let old = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let next = tree.path("replacement");
    std::fs::write(&next, data("later")).unwrap();
    std::fs::rename(next, &path).unwrap();
    assert!(
        OutputJob::recover_opened(&tree.root, &path, old)
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), data("later"));
    let current = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let adopted = OutputJob::recover_opened(&tree.root, &path, current)
        .unwrap()
        .unwrap();
    assert_eq!(adopted.saved.edit.new, "later");
    drop(adopted);
    let disappeared = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        OutputJob::recover_opened(&tree.root, &path, disappeared)
            .unwrap()
            .is_none()
    );
}

#[test]
fn recovered_outputs_keep_journal_order_with_an_existing_poisoned_queue_item() {
    let tree = repository();
    let folder = OutputJob::folder(&tree.root).unwrap();
    for name in ["z", "a", "m"] {
        std::fs::write(
            folder.join(format!("{name}.json")),
            serde_json::json!({
                "edit":{"rel":format!("docs/{name}.md"),"base":"","new":name},
                "publication":null
            })
            .to_string(),
        )
        .unwrap();
    }
    let path = folder.join("z.json");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let job = OutputJob::recover_opened(&tree.root, &path, file)
        .unwrap()
        .unwrap();
    let waiting: Waiting = Arc::new(Mutex::new(BTreeMap::from([(
        job.job.id,
        job.job.edit.clone(),
    )])));
    let ahead: Ahead = Arc::new(Mutex::new(BTreeMap::from([(job.job.edit.rel.clone(), 1)])));
    let job = Arc::new(Mutex::new(job));
    let poison = job.clone();
    assert!(
        std::thread::spawn(move || {
            let _held = poison.lock().unwrap();
            panic!("exercise the queue's poison recovery");
        })
        .join()
        .is_err()
    );
    let mut queued = std::collections::VecDeque::from([job]);
    recover_outputs(&tree.root, &mut queued, &waiting, &ahead);
    let paths = || {
        queued
            .iter()
            .map(|job| {
                job.lock()
                    .recover()
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(paths(), ["a.json", "m.json", "z.json"]);
    assert_eq!(waiting.lock().unwrap().len(), 3);
    assert_eq!(
        ahead.lock().unwrap().values().copied().collect::<Vec<_>>(),
        [1, 1, 1]
    );
    recover_outputs(&tree.root, &mut queued, &waiting, &ahead);
    assert_eq!(queued.len(), 3, "held journals are not adopted twice");
}

/// Whether another descriptor could take the server's lock `name` of `root` right now.
fn lock_is_free(root: &Path, name: &str) -> bool {
    let file = OpenOptions::new()
        .append(true)
        .open(home(root).unwrap().join(name))
        .unwrap();
    file.try_lock().is_ok()
}

#[test]
fn publication_locks_hold_all_three_until_dropped() {
    let tree = repository();
    let writes = Mutex::new(());
    let locks = PublicationLocks::take(&tree.root, &writes).unwrap();
    assert!(writes.try_lock().is_err());
    assert!(!lock_is_free(&tree.root, "verdict-commits.lock"));
    assert!(!lock_is_free(&tree.root, "verdict-writes.lock"));
    drop(locks);
    assert!(writes.try_lock().is_ok());
    assert!(lock_is_free(&tree.root, "verdict-commits.lock"));
    assert!(lock_is_free(&tree.root, "verdict-writes.lock"));
    // Taken again once released: nothing of the first take is left held.
    drop(PublicationLocks::take(&tree.root, &writes).unwrap());
}

/// A copy of each lock's descriptor kept past the drop, as a child another thread spawned keeps
/// one until its exec, holds neither lock: the drop unlocks them, where a close alone would
/// release each only with its last copy.
#[test]
fn copies_of_the_locks_descriptors_kept_past_their_drop_hold_nothing() {
    let tree = repository();
    let writes = Mutex::new(());
    let locks = PublicationLocks::take(&tree.root, &writes).unwrap();
    let copies = [
        locks._commits.0.try_clone().unwrap(),
        locks._file.as_ref().unwrap().0.try_clone().unwrap(),
    ];
    drop(locks);
    assert!(lock_is_free(&tree.root, "verdict-commits.lock"));
    assert!(lock_is_free(&tree.root, "verdict-writes.lock"));
    drop(copies);
}

/// Refused at the commits lock, the first taken: outside a repository no common dir holds it, so
/// nothing is taken and the saves stay free.
#[test]
fn publication_locks_refused_outside_a_repository_leave_the_saves_free() {
    let tree = crate::test_tree::Tree::new();
    let writes = Mutex::new(());
    assert!(PublicationLocks::take(&tree.root, &writes).is_err());
    assert!(writes.try_lock().is_ok());
}

/// Refused at the write lock, the last taken, once the commits lock and this server's saves are
/// held: a folder where `verdict-writes.lock` would be makes its open fail at once, with no clock
/// to wait on. The refusal names that lock and leaves the commits lock and the saves free.
#[test]
fn publication_locks_refused_at_the_write_lock_release_the_saves_and_the_commits_lock() {
    let tree = repository();
    let home = home(&tree.root).unwrap();
    std::fs::create_dir_all(home.join("verdict-writes.lock")).unwrap();
    let writes = Mutex::new(());
    let Err(refused) = PublicationLocks::take(&tree.root, &writes) else {
        panic!("a folder for the write lock was taken as its file");
    };
    assert!(refused.contains("verdict-writes.lock"), "{refused}");
    assert!(
        home.join("verdict-commits.lock").is_file(),
        "the commits lock was taken first"
    );
    assert!(lock_is_free(&tree.root, "verdict-commits.lock"));
    assert!(writes.try_lock().is_ok());
}
