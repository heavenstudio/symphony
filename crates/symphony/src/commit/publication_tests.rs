//! The publication journal's recovery: the locked file it adopts, and the ones it leaves.

use super::*;

#[test]
fn recovery_adopts_only_the_locked_inode_still_named_by_the_journal() {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
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
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
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
    let waiting: Waiting = Arc::new(Mutex::new(BTreeMap::from([(job.job.0, job.job.1.clone())])));
    let ahead: Ahead = Arc::new(Mutex::new(BTreeMap::from([(job.job.1.rel.clone(), 1)])));
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
                    .unwrap_or_else(|e| e.into_inner())
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
