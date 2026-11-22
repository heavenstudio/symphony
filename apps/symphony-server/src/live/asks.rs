//! What the hub tells its pages of the asks, so an open chat or a document's questions read a
//! thread again only when it moved, never on a timer: the `asks` feed region. Its data names each
//! thread that has a new message (`threads`, each `{id, path}`), or the chat recipient when the
//! board names another one (`recipient`, its handle or null).
//!
//! The asks file is shared by the repository's worktrees ([`symphony::asks::default_path`]). Each
//! board interval the hub compares the file's and its write-ahead log's size and time; only when
//! they changed does it read which threads have messages after the last one it announced.

use super::Hub;
use crate::lock::Held as _;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use symphony::asks::AskStore;

/// The asks file's and its log's length and modification time, as last looked at.
type Stamp = [Option<(u64, SystemTime)>; 2];

/// Where the hub is in the asks file: what it last saw, and the last message it announced.
#[derive(Default)]
pub(crate) struct AskWatch {
    path: Option<PathBuf>,
    stamp: Option<Stamp>,
    /// The last seq announced; `None` until the first look, which announces nothing.
    seq: Option<i64>,
}

fn stamp(path: &Path) -> Stamp {
    let look = |path: &Path| {
        std::fs::metadata(path).ok().map(|meta| {
            (
                meta.len(),
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            )
        })
    };
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    [look(path), look(Path::new(&wal))]
}

impl AskWatch {
    /// The `asks` change since the last look, if any thread moved. A file not there yet has no
    /// message, so every message of the one made later is new; a file already there when the hub
    /// first looks holds nothing new.
    fn look(&mut self, root: &Path) -> Option<Value> {
        let path = self.path.get_or_insert_with(|| {
            symphony::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref())
        });
        let now = stamp(path);
        if self.stamp == Some(now) {
            return None;
        }
        let Some(store) = AskStore::existing(path) else {
            self.stamp = Some(now);
            self.seq = Some(0);
            return None;
        };
        let Some(seq) = self.seq else {
            let last = store.last_seq().ok()?;
            self.stamp = Some(now);
            self.seq = Some(last);
            return None;
        };
        // A failed read keeps the old stamp, so the next interval reads again.
        let moved = store.moved_since(seq).ok()?;
        self.stamp = Some(now);
        let last = moved.first()?.last;
        self.seq = Some(last.max(seq));
        Some(json!({"threads": moved}))
    }
}

impl Hub {
    /// One look at the asks file: every page hears which threads moved.
    pub(super) fn asks_tick(&self) {
        let change = self.asks.lock().held().look(&self.root);
        if let Some(data) = change {
            let mut pages = self.pages.write().held();
            for page in pages.values_mut() {
                self.publish(page, vec![json!({"region":"asks", "data":data})]);
            }
        }
    }

    /// The board's chat recipient as its last read named it, `None` before the first read: what
    /// an ask's reply says the chat goes to, without connecting to the board for each read.
    pub(crate) fn chat_recipient(&self) -> Option<Option<String>> {
        self.chat.lock().held().clone()
    }

    /// Keep `recipient`, the board's chat recipient just read; once it changes after the first
    /// read, the change every page hears, so an open chat says where it goes now.
    pub(super) fn chat_read(&self, recipient: Option<String>) -> Option<Value> {
        let mut held = self.chat.lock().held();
        if held.as_ref() == Some(&recipient) {
            return None;
        }
        let first = held.is_none();
        *held = Some(recipient.clone());
        (!first).then(|| json!({"region":"asks", "data":{"threads":[], "recipient":recipient}}))
    }
}

#[cfg(test)]
mod tests {
    use super::{AskWatch, Hub};
    use crate::testing::{Tree, repository};
    use serde_json::{Value, json};
    use symphony::asks::AskStore;

    #[test]
    fn every_page_hears_a_moved_thread_and_a_new_chat_recipient() {
        let tree = Tree::new();
        let root = repository(&tree);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        let db = tree.path("asks.db");
        hub.asks.lock().unwrap().path = Some(db.clone());
        let store = AskStore::open(&db).unwrap();
        hub.asks_tick();
        let quiet = hub.pages.read().unwrap()[""].history.len();
        let id = store.ask("docs/A.md", "why five bands?", "", "").unwrap();
        hub.asks_tick();
        let pages = hub.pages.read().unwrap();
        let history = &pages[""].history;
        assert_eq!(history.len(), quiet + 1, "one event for the moved thread");
        let event: Value = serde_json::from_str(&history.back().unwrap().json).unwrap();
        assert_eq!(
            event["changes"],
            json!([{"region":"asks","data":{"threads":[{"id":id,"path":"docs/A.md"}]}}])
        );
        drop(pages);

        assert_eq!(hub.chat_recipient(), None, "no board read yet");
        assert_eq!(hub.chat_read(None), None, "the first read is no change");
        assert_eq!(hub.chat_recipient(), Some(None));
        assert_eq!(hub.chat_read(None), None);
        assert_eq!(
            hub.chat_read(Some("amber-heron".into())),
            Some(json!({"region":"asks","data":{"threads":[],"recipient":"amber-heron"}}))
        );
        assert_eq!(hub.chat_recipient(), Some(Some("amber-heron".into())));
        assert_eq!(
            hub.chat_read(None),
            Some(json!({"region":"asks","data":{"threads":[],"recipient":null}})),
            "a recipient leaving is a change too"
        );
    }

    /// The watcher's look at `path`, as a hub of `root` with `ASKS_DB` naming it makes.
    fn watch(path: &std::path::Path) -> AskWatch {
        AskWatch {
            path: Some(path.to_path_buf()),
            ..AskWatch::default()
        }
    }

    #[test]
    fn a_new_message_is_announced_once_by_thread_and_a_quiet_file_reads_nothing() {
        let tree = Tree::new();
        let (root, db) = (tree.path(""), tree.path("asks/asks.db"));
        let store = AskStore::open(&db).unwrap();
        let before = store.ask("docs/A.md", "an old question", "", "").unwrap();
        let mut watch = watch(&db);
        assert_eq!(watch.look(&root), None, "the first look announces nothing");
        assert_eq!(watch.look(&root), None);
        store.answer(before, "An answer.", "amber-heron").unwrap();
        let other = store.ask("docs/B.md", "a new question", "", "").unwrap();
        assert_eq!(
            watch.look(&root),
            Some(
                json!({"threads":[{"id":before,"path":"docs/A.md"},{"id":other,"path":"docs/B.md"}]})
            )
        );
        assert_eq!(
            watch.look(&root),
            None,
            "nothing new since the announcement"
        );
        store.reply(other, "a follow-up").unwrap();
        assert_eq!(
            watch.look(&root),
            Some(json!({"threads":[{"id":other,"path":"docs/B.md"}]}))
        );
    }

    #[test]
    fn a_file_made_after_the_first_look_announces_its_first_messages() {
        let tree = Tree::new();
        let (root, db) = (tree.path(""), tree.path("asks/asks.db"));
        let mut watch = watch(&db);
        assert_eq!(watch.look(&root), None);
        assert!(!db.exists(), "looking makes no file");
        let store = AskStore::open(&db).unwrap();
        let id = store
            .ask("docs/A.md", "the first question", "", "")
            .unwrap();
        assert_eq!(
            watch.look(&root),
            Some(json!({"threads":[{"id":id,"path":"docs/A.md"}]}))
        );
    }
}
