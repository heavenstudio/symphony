//! Exercise notified and swept gallery changes through an owned, ephemeral listener.

use super::{Hub, Sweep};
use crate::backstop::Backstop;
use crate::front::{Shared, router};
use crate::testing::{Tree, git, repository, shelf};
use notify::event::{AccessKind, CreateKind, Flag, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind, Watcher};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use symphony_api::{Block, Document, GalleryItem, Media};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHILD: &str = "SYMPHONY_GALLERY_REFRESH_CHILD";
const PHASE: &str = "gallery refresh phase: ";
/// A checkout whose project uses the shelf plugin, which draws the shelf's gallery.
const MANIFEST: &str = shelf::MANIFEST;
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><path d="M0 0L80 40L0 80Z"/></svg>"#;

fn deliver(hub: &Hub, event: Event) {
    assert!(hub.record_event(Ok(event)));
    refresh(hub);
}

/// What the worker does with a reported change: stamp what it carried for the sweep, then refresh.
fn refresh(hub: &Hub) {
    let dirty = std::mem::take(&mut *hub.dirty.lock().unwrap());
    hub.swept.lock().unwrap().restamp(&dirty);
    hub.refresh_docs(Some(&dirty)).unwrap();
}

async fn document(address: SocketAddr) -> Document {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!(
                "GET /api/document?path=docs/SHELF.md HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(body).unwrap()
}

fn items(document: &Document) -> &[GalleryItem] {
    section_items(&document.sections)
}

fn section_items(sections: &[symphony_api::Section]) -> &[GalleryItem] {
    sections
        .iter()
        .flat_map(|section| &section.blocks)
        .flat_map(|block| match block {
            Block::Component { blocks, .. } => blocks.as_slice(),
            block => std::slice::from_ref(block),
        })
        .find_map(|block| match block {
            Block::Media {
                media: Media::Gallery { items, .. },
            } => Some(items.as_slice()),
            _ => None,
        })
        .expect("the served document must contain its gallery")
}

fn picture(document: &Document) -> Option<&str> {
    items(document)[0].poster.as_deref().map(|url| {
        let (path, version) = url.split_once("?v=").expect("a local picture is versioned");
        assert_eq!(version.len(), 12);
        assert!(version.bytes().all(|byte| byte.is_ascii_hexdigit()));
        path
    })
}

#[tokio::test]
async fn watcher_refreshes_gallery_picture_but_reuses_it_for_document_events() {
    shelf::install().unwrap();
    if std::env::var_os(CHILD).is_none() {
        // Capture the existing warm log in an isolated process, without adding a production
        // counter or replacing the gallery builder. No native watcher or live board runs.
        let done = symphony_board::sys::output(symphony_board::sys::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live::gallery_tests::watcher_refreshes_gallery_picture_but_reuses_it_for_document_events",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("TASKS_SERVER", "http://127.0.0.1:1")
            .env("TASKS_DATABASE", "gallery-worker")
            .env_remove("TASKS_SESSION"))
            .unwrap();
        let stderr = String::from_utf8_lossy(&done.stderr);
        let stdout = String::from_utf8_lossy(&done.stdout);
        assert!(done.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("1 passed"), "{stdout}");
        let builds: Vec<_> = stderr
            .split(PHASE)
            .skip(1)
            .map(|phase| {
                (
                    phase.lines().next().unwrap(),
                    phase.matches("→ shelf warmed:").count(),
                )
            })
            .collect();
        assert_eq!(
            builds,
            [
                ("startup", 1),
                ("document", 0),
                ("delayed item notification", 0),
                ("posters added", 1),
                ("poster removed", 1),
                ("label added", 1),
                ("notes changed", 1),
                ("item added", 1),
                ("duplicate item notification", 0),
                ("item removed without notification", 1),
                ("items replaced without notification", 1),
                ("item directory removed without notification", 1),
                ("item directory restored without notification", 1),
                ("unreadable item directory", 0),
                ("item removed by worker", 1),
            ],
            "{stderr}"
        );
        return;
    }

    let mut tree = Tree::new();
    // notify reports paths beneath the canonical watched root, including on macOS where the
    // temporary directory's /var spelling is an alias for /private/var.
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    tree.write("main/docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n");
    tree.write("main/room/shelf/items/A.item", "item fixture");
    tree.write(
        "main/docs/labels/A.notes.json",
        r#"{"source":{"picture":"posters/A.svg"}}"#,
    );
    let hub = Arc::new(Hub::new(&root, &root.join("client/public")));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    eprintln!("{PHASE}startup");
    hub.refresh_docs(None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shared = Shared::new(
        i64::from(std::process::id()),
        address.port(),
        &root,
        Some(&root),
        crate::testing::open_gate(),
    );
    assert!(shared.api.set(hub.clone()).is_ok());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router(shared).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });

    let initial = document(address).await;
    assert_eq!(items(&initial).len(), 1);
    assert_eq!(items(&initial)[0].name, "A");
    assert_eq!(items(&initial)[0].poster, None);

    eprintln!("{PHASE}document");
    let doc = tree.write("main/docs/SHELF.md", "# Updated shelf\n\n[[shelf:all]]\n");
    deliver(
        &hub,
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(doc),
    );
    let updated = document(address).await;
    assert_eq!(updated.title, "Updated shelf");
    assert_eq!(items(&updated), items(&initial));
    assert!(!hub.sweep(), "what an event carried is not reported again");

    eprintln!("{PHASE}delayed item notification");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File))
            .add_path(root.join("room/shelf/items/A.item")),
    );
    assert_eq!(items(&document(address).await), items(&initial));

    eprintln!("{PHASE}posters added");
    let a = tree.write("main/posters/A.svg", SVG);
    let b = tree.write("main/posters/B.svg", SVG);
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File))
            .add_path(a.clone())
            .add_path(b),
    );
    assert_eq!(picture(&document(address).await), Some("/posters/A.svg"));

    eprintln!("{PHASE}poster removed");
    std::fs::remove_file(&a).unwrap();
    deliver(
        &hub,
        Event::new(EventKind::Remove(RemoveKind::File)).add_path(a),
    );
    assert_eq!(items(&document(address).await)[0].poster, None);

    eprintln!("{PHASE}label added");
    let label = tree.write("main/docs/labels/A.svg", SVG);
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(label),
    );
    assert_eq!(
        picture(&document(address).await),
        Some("/docs/labels/A.svg")
    );

    eprintln!("{PHASE}notes changed");
    let notes = tree.write(
        "main/docs/labels/A.notes.json",
        r#"{"source":{"picture":"posters/B.svg"}}"#,
    );
    deliver(
        &hub,
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(notes),
    );
    assert_eq!(picture(&document(address).await), Some("/posters/B.svg"));

    eprintln!("{PHASE}item added");
    let item = tree.write("main/room/shelf/items/B.item", "second item fixture");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(item.clone()),
    );
    assert_eq!(
        items(&document(address).await)
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );

    eprintln!("{PHASE}duplicate item notification");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(item),
    );
    assert_eq!(items(&document(address).await).len(), 2);

    // No notification is delivered in these phases. The backstop's sweep must dirty the root and
    // update the existing HTTP snapshot, including an empty gallery.
    for (phase, removed, added, expected) in [
        (
            "item removed without notification",
            Some("items/A.item"),
            None,
            vec!["B"],
        ),
        (
            "items replaced without notification",
            Some("items/B.item"),
            Some("C.item"),
            vec!["C"],
        ),
        (
            "item directory removed without notification",
            Some("items"),
            None,
            vec![],
        ),
        (
            "item directory restored without notification",
            None,
            Some("D.item"),
            vec!["D"],
        ),
    ] {
        eprintln!("{PHASE}{phase}");
        if let Some(removed) = removed {
            let path = root.join("room/shelf").join(removed);
            if path.is_dir() {
                std::fs::remove_dir_all(path).unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
        if let Some(added) = added {
            tree.write(
                &format!("main/room/shelf/items/{added}"),
                "replacement item",
            );
        }
        assert!(hub.sweep(), "{phase} must dirty the item root");
        let version = hub.pages.read().unwrap()[""].board.version;
        refresh(&hub);
        {
            let pages = hub.pages.read().unwrap();
            let page = &pages[""];
            assert!(
                page.board.version > version,
                "{phase} must advance the feed"
            );
            let event: serde_json::Value =
                serde_json::from_str(&page.history.back().unwrap().json).unwrap();
            let change = event["changes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|change| change["region"] == "document")
                .expect("the sweep's refresh must publish the document change");
            let delta: symphony_api::DocumentChange =
                serde_json::from_value(change["data"].clone()).unwrap();
            assert_eq!(delta.path, shelf::DOC);
            assert_eq!(
                section_items(&delta.sections)
                    .iter()
                    .map(|h| h.name.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "{phase} must push the gallery to existing subscribers"
            );
        }
        assert_eq!(
            items(&document(address).await)
                .iter()
                .map(|h| h.name.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{phase} must update the served gallery"
        );
        assert!(!hub.sweep(), "{phase} is reported once");
    }
    eprintln!("{PHASE}unreadable item directory");
    let folder = root.join("room/shelf/items");
    let mode = |bits: u32| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(bits)).unwrap();
    };
    mode(0o000);
    let swept = hub.sweep();
    mode(0o755);
    assert!(!swept, "a failed scan must retain the warm gallery");
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.watch_changes.lock().unwrap().is_empty());
    assert_eq!(
        items(&document(address).await)
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>(),
        ["D"]
    );
    assert!(!hub.sweep(), "a folder readable again is unchanged");

    eprintln!("{PHASE}item removed by worker");
    // Run the production loop with a watcher that can deliver no events and a short period: the
    // period's sweep alone finds the removal.
    let watcher = notify::NullWatcher::new(|_| {}, notify::Config::default()).unwrap();
    let weak = Arc::downgrade(&hub);
    let backstop = Backstop::new(Duration::from_millis(500));
    *hub.worker.lock().unwrap() = Some(std::thread::spawn(move || {
        super::run(
            weak,
            watcher,
            BTreeSet::new(),
            backstop,
            None,
            symphony::taskview::connect,
        );
    }));
    struct StopWorker(Arc<Hub>);
    impl Drop for StopWorker {
        fn drop(&mut self) {
            self.0.stop();
        }
    }
    let worker = StopWorker(hub.clone());
    let mut changes = hub.changed.subscribe();
    std::fs::remove_file(root.join("room/shelf/items/D.item")).unwrap();
    // Only a stall reaches this wait's backstop, and its failure says the load average.
    let waited = test_time::backstop(Duration::from_secs(10));
    tokio::time::timeout(waited, async {
        loop {
            changes.changed().await.unwrap();
            let pages = hub.pages.read().unwrap();
            if items(&pages[""].documents[shelf::DOC]).is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the idle worker to publish the removal without a filesystem notification: stalled: \
             the wait's wall-clock backstop of {waited:?} ran out; {}",
            test_time::Load::now()
        )
    });
    assert!(items(&document(address).await).is_empty());
    drop(worker);
    stop.send(()).unwrap();
    server.await.unwrap();
}

#[test]
fn an_items_link_outside_its_root_is_neither_shown_nor_swept() {
    shelf::install().unwrap();
    let tree = Tree::new();
    let root = tree.path("served");
    let outside = tree.path("outside");
    tree.write("outside/room/shelf/items/X.item", "outside item");
    std::fs::create_dir_all(root.join("room/shelf")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("room/shelf/items"),
        root.join("room/shelf/items"),
    )
    .unwrap();
    assert_eq!(shelf::warm(&outside), 1);
    assert_eq!(shelf::warm(&root), 0);
    // The link is stamped as itself, so a change behind it is none of the served root's.
    let roots = vec![(
        root.clone(),
        "room/shelf".to_string(),
        notify::RecursiveMode::Recursive,
    )];
    let mut sweep = Sweep::new(&roots);
    tree.write("outside/room/shelf/items/Y.item", "another outside item");
    assert!(sweep.next(&roots).is_empty());
}

#[test]
fn a_served_shelf_replaced_by_a_link_outside_reads_as_emptied_and_its_target_is_not_swept() {
    shelf::install().unwrap();
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    tree.write("main/docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let served = tree.path("served");
    git(
        &root,
        &["worktree", "add", "-b", "served", served.to_str().unwrap()],
    );
    tree.write("served/room/shelf/items/OLD.item", "item fixture");
    tree.write("outside/shelf/items/X.item", "outside item");
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("served").unwrap();
    hub.document("served", shelf::DOC).unwrap();
    let gallery = || -> Vec<String> {
        items(&hub.pages.read().unwrap()["served"].documents[shelf::DOC])
            .iter()
            .map(|item| item.name.clone())
            .collect()
    };
    assert_eq!(gallery(), ["OLD"]);
    assert!(!hub.sweep());

    // No event announces the swap.
    std::fs::remove_dir_all(served.join("room/shelf")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside/shelf"), served.join("room/shelf")).unwrap();
    assert!(hub.sweep(), "the linked root reads as emptied");
    assert_eq!(
        *hub.dirty.lock().unwrap(),
        BTreeSet::from([served.join("room/shelf/items/OLD.item")])
    );
    refresh(&hub);
    assert!(gallery().is_empty(), "OLD leaves; nothing outside shows");
    tree.write("outside/shelf/items/Y.item", "added behind the link");
    assert!(!hub.sweep(), "the link's target is not walked");

    // The same folder reached through a linked parent instead.
    std::fs::remove_file(served.join("room/shelf")).unwrap();
    std::fs::remove_dir(served.join("room")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside"), served.join("room")).unwrap();
    tree.write("outside/shelf/items/Z.item", "behind the linked parent");
    assert!(
        !hub.sweep(),
        "a items root under a linked parent is not walked"
    );
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.watch_changes.lock().unwrap().is_empty());
}

#[test]
fn a_shelf_linked_to_a_folder_holding_the_same_name_reports_one_removal_and_then_rests() {
    shelf::install().unwrap();
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    tree.write("main/docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let served = tree.path("served");
    git(
        &root,
        &["worktree", "add", "-b", "served", served.to_str().unwrap()],
    );
    tree.write("served/room/shelf/items/OLD.item", "local item");
    tree.write(
        "outside/shelf/items/OLD.item",
        "outside item of the same name",
    );
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("served").unwrap();
    hub.document("served", shelf::DOC).unwrap();
    let gallery = || -> Vec<String> {
        items(&hub.pages.read().unwrap()["served"].documents[shelf::DOC])
            .iter()
            .map(|item| item.name.clone())
            .collect()
    };
    assert_eq!(gallery(), ["OLD"]);
    assert!(!hub.sweep());

    // No event announces the swap. The removal's refresh restamps OLD's path, which now resolves
    // behind the link; the restamp reads nothing there, so later periods find nothing to report.
    std::fs::remove_dir_all(served.join("room/shelf")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside/shelf"), served.join("room/shelf")).unwrap();
    assert!(hub.sweep());
    assert_eq!(
        *hub.dirty.lock().unwrap(),
        BTreeSet::from([served.join("room/shelf/items/OLD.item")])
    );
    refresh(&hub);
    assert!(gallery().is_empty());
    for period in 1..=3 {
        assert!(!hub.sweep(), "period {period} reports OLD again");
        assert!(hub.dirty.lock().unwrap().is_empty());
    }
}

#[test]
fn the_sweep_follows_only_trees_with_pages_and_a_page_that_goes_leaves_its_gallery_cold() {
    shelf::install().unwrap();
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write("main/symphony.toml", MANIFEST);
    tree.write("main/docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let branch = tree.path("branch");
    git(
        &root,
        &["worktree", "add", "-b", "before", branch.to_str().unwrap()],
    );
    let item = tree.write("branch/room/shelf/items/Z.item", "item fixture");
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("before").unwrap();
    let initial = hub.document("before", shelf::DOC).unwrap();
    assert_eq!(items(&initial)[0].name, "Z");
    assert!(!hub.sweep());
    assert!(hub.warmed_watches.lock().unwrap().contains_key(&branch));

    // The switch moves refs the sweep follows; its refresh drops the page and leaves the folder's
    // gallery cold.
    git(&branch, &["switch", "-c", "after"]);
    assert!(hub.sweep());
    refresh(&hub);
    assert!(!hub.pages.read().unwrap().contains_key("before"));
    assert!(!hub.pages.read().unwrap().contains_key("after"));
    assert_eq!(hub.tree("after"), Some(branch.clone()));
    assert!(!hub.warmed_watches.lock().unwrap().contains_key(&branch));
    std::fs::remove_file(item).unwrap();
    assert!(!hub.sweep(), "a tree without a page is left to its events");
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.watch_changes.lock().unwrap().is_empty());

    // Opening the new scope reads its items afresh; what that read saw is no change.
    hub.ensure_page("after").unwrap();
    assert!(items(&hub.document("after", shelf::DOC).unwrap()).is_empty());
    assert!(!hub.sweep());

    tree.write("branch/room/shelf/items/NEW.item", "new item");
    git(
        &root,
        &["worktree", "remove", "--force", branch.to_str().unwrap()],
    );
    assert!(hub.sweep());
    refresh(&hub);
    assert!(hub.tree("after").is_none());
    assert!(!hub.pages.read().unwrap().contains_key("after"));
    assert!(!hub.warmed_watches.lock().unwrap().contains_key(&branch));
    assert!(hub.watch_changes.lock().unwrap().is_empty());
    assert!(!hub.sweep());
    assert!(hub.dirty.lock().unwrap().is_empty());
}

#[test]
fn watcher_filters_access_and_metadata_and_invalidates_all_trees_on_rescan() {
    shelf::install().unwrap();
    let tree = Tree::new();
    let root = repository(&tree);
    let branch = tree.path("branch");
    // both trees' projects use the plugin that watches its shelf
    tree.write("main/symphony.toml", MANIFEST);
    tree.write("branch/symphony.toml", MANIFEST);
    let hub = Hub::new(&root, &root.join("client/public"));
    hub.trees
        .write()
        .unwrap()
        .insert("branch".into(), branch.clone());
    for event in [
        Event::new(EventKind::Access(AccessKind::Read)).add_path(root.join("posters/A.png")),
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(root.join(".git/index")),
        Event::new(EventKind::Any),
    ] {
        assert!(!hub.record_event(Ok(event)));
        assert!(hub.dirty.lock().unwrap().is_empty());
        assert!(hub.watch_changes.lock().unwrap().is_empty());
    }

    assert!(
        hub.record_event(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Both
        )))
        .add_path(root.join("posters/old.png"))
        .add_path(branch.join("posters/new.png"))))
    );
    let roots = BTreeSet::from([root.clone(), branch]);
    assert_eq!(*hub.watch_changes.lock().unwrap(), roots);
    for event in [
        Ok(Event::new(EventKind::Any).set_flag(Flag::Rescan)),
        Err(notify::Error::generic("watcher lost events")),
    ] {
        hub.watch_changes.lock().unwrap().clear();
        assert!(hub.record_event(event));
        assert_eq!(*hub.watch_changes.lock().unwrap(), roots);
    }
}
