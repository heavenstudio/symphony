//! Exercise notified and swept gallery changes through an owned, ephemeral listener.

use super::{Hub, Sweep, gallery_root};
use crate::backstop::Backstop;
use crate::front::{Shared, router};
use crate::gate::Gate;
use crate::testing::{Tree, git, repository};
use notify::event::{AccessKind, CreateKind, Flag, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind, Watcher};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use symphony_api::{Block, Document, GalleryHull, Media};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHILD: &str = "SF_GALLERY_REFRESH_CHILD";
const PHASE: &str = "gallery refresh phase: ";
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
                "GET /api/document?path=docs/ships/HULLS.md HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
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

fn hulls(document: &Document) -> &[GalleryHull] {
    section_hulls(&document.sections)
}

fn section_hulls(sections: &[symphony_api::Section]) -> &[GalleryHull] {
    sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find_map(|block| match block {
            Block::Media {
                media: Media::Gallery { hulls },
            } => Some(hulls.as_slice()),
            _ => None,
        })
        .expect("the served document must contain its gallery")
}

fn picture(document: &Document) -> Option<&str> {
    hulls(document)[0].picture.as_deref().map(|url| {
        let (path, version) = url.split_once("?v=").expect("a local picture is versioned");
        assert_eq!(version.len(), 12);
        assert!(version.bytes().all(|byte| byte.is_ascii_hexdigit()));
        path
    })
}

#[tokio::test]
async fn watcher_refreshes_gallery_art_but_reuses_it_for_document_events() {
    if std::env::var_os(CHILD).is_none() {
        // Capture the existing warm log in an isolated process, without adding a production
        // counter or replacing the gallery builder. No native watcher or live board runs.
        let done = tasks::sys::output(tasks::sys::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live::gallery_tests::watcher_refreshes_gallery_art_but_reuses_it_for_document_events",
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
                    phase.matches("→ hull gallery warmed:").count(),
                )
            })
            .collect();
        assert_eq!(
            builds,
            [
                ("startup", 1),
                ("document", 0),
                ("delayed model notification", 0),
                ("concepts added", 1),
                ("concept removed", 1),
                ("outline added", 1),
                ("notes changed", 1),
                ("model added", 1),
                ("duplicate model notification", 0),
                ("model removed without notification", 1),
                ("models replaced without notification", 1),
                ("model directory removed without notification", 1),
                ("model directory restored without notification", 1),
                ("unreadable model directory", 0),
                ("model removed by worker", 1),
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
    tree.write(
        "main/docs/ships/HULLS.md",
        "# Hulls\n\n<div data-hull-gallery></div>\n",
    );
    tree.write("main/view/models/hulls/A.glb", "model fixture");
    tree.write(
        "main/docs/data/hull-outlines/A.notes.json",
        r#"{"source":{"picture":"artifacts/concepts/A.svg"}}"#,
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
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
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
    assert_eq!(hulls(&initial).len(), 1);
    assert_eq!(hulls(&initial)[0].id, "A");
    assert_eq!(hulls(&initial)[0].picture, None);

    eprintln!("{PHASE}document");
    let doc = tree.write(
        "main/docs/ships/HULLS.md",
        "# Updated hulls\n\n<div data-hull-gallery></div>\n",
    );
    deliver(
        &hub,
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(doc),
    );
    let updated = document(address).await;
    assert_eq!(updated.title, "Updated hulls");
    assert_eq!(hulls(&updated), hulls(&initial));
    assert!(!hub.sweep(), "what an event carried is not reported again");

    eprintln!("{PHASE}delayed model notification");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File))
            .add_path(root.join("view/models/hulls/A.glb")),
    );
    assert_eq!(hulls(&document(address).await), hulls(&initial));

    eprintln!("{PHASE}concepts added");
    let a = tree.write("main/artifacts/concepts/A.svg", SVG);
    let b = tree.write("main/artifacts/concepts/B.svg", SVG);
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File))
            .add_path(a.clone())
            .add_path(b),
    );
    assert_eq!(
        picture(&document(address).await),
        Some("/artifacts/concepts/A.svg")
    );

    eprintln!("{PHASE}concept removed");
    std::fs::remove_file(&a).unwrap();
    deliver(
        &hub,
        Event::new(EventKind::Remove(RemoveKind::File)).add_path(a),
    );
    assert_eq!(hulls(&document(address).await)[0].picture, None);

    eprintln!("{PHASE}outline added");
    let outline = tree.write("main/docs/data/hull-outlines/A.svg", SVG);
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(outline),
    );
    assert_eq!(
        picture(&document(address).await),
        Some("/docs/data/hull-outlines/A.svg")
    );

    eprintln!("{PHASE}notes changed");
    let notes = tree.write(
        "main/docs/data/hull-outlines/A.notes.json",
        r#"{"source":{"picture":"artifacts/concepts/B.svg"}}"#,
    );
    deliver(
        &hub,
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(notes),
    );
    assert_eq!(
        picture(&document(address).await),
        Some("/artifacts/concepts/B.svg")
    );

    eprintln!("{PHASE}model added");
    let model = tree.write("main/view/models/hulls/B.glb", "second model fixture");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(model.clone()),
    );
    assert_eq!(
        hulls(&document(address).await)
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );

    eprintln!("{PHASE}duplicate model notification");
    deliver(
        &hub,
        Event::new(EventKind::Create(CreateKind::File)).add_path(model),
    );
    assert_eq!(hulls(&document(address).await).len(), 2);

    // No notification is delivered in these phases. The backstop's sweep must dirty the root and
    // update the existing HTTP snapshot, including an empty gallery.
    for (phase, removed, added, expected) in [
        (
            "model removed without notification",
            Some("hulls/A.glb"),
            None,
            vec!["B"],
        ),
        (
            "models replaced without notification",
            Some("hulls/B.glb"),
            Some("C.glb"),
            vec!["C"],
        ),
        (
            "model directory removed without notification",
            Some("hulls"),
            None,
            vec![],
        ),
        (
            "model directory restored without notification",
            None,
            Some("D.glb"),
            vec!["D"],
        ),
    ] {
        eprintln!("{PHASE}{phase}");
        if let Some(removed) = removed {
            let path = root.join("view/models").join(removed);
            if path.is_dir() {
                std::fs::remove_dir_all(path).unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
        if let Some(added) = added {
            tree.write(
                &format!("main/view/models/hulls/{added}"),
                "replacement model",
            );
        }
        assert!(hub.sweep(), "{phase} must dirty the model root");
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
            assert_eq!(delta.path, symphony_api::HULL_GALLERY_DOC);
            assert_eq!(
                section_hulls(&delta.sections)
                    .iter()
                    .map(|h| h.id.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "{phase} must push the gallery to existing subscribers"
            );
        }
        assert_eq!(
            hulls(&document(address).await)
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{phase} must update the served gallery"
        );
        assert!(!hub.sweep(), "{phase} is reported once");
    }
    eprintln!("{PHASE}unreadable model directory");
    let folder = root.join("view/models/hulls");
    let mode = |bits: u32| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(bits)).unwrap();
    };
    mode(0o000);
    let swept = hub.sweep();
    mode(0o755);
    assert!(!swept, "a failed scan must retain the warm gallery");
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.gallery_changes.lock().unwrap().is_empty());
    assert_eq!(
        hulls(&document(address).await)
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["D"]
    );
    assert!(!hub.sweep(), "a folder readable again is unchanged");

    eprintln!("{PHASE}model removed by worker");
    // Run the production loop with a watcher that can deliver no events and a short period.
    // Keeping the sender alive leaves the worker on its idle timeout path, not disconnected.
    let (_send, recv) = mpsc::sync_channel(1);
    let watcher = notify::NullWatcher::new(|_| {}, notify::Config::default()).unwrap();
    let weak = Arc::downgrade(&hub);
    let backstop = Backstop::new(Duration::from_millis(500));
    *hub.worker.lock().unwrap() = Some(std::thread::spawn(move || {
        super::run(weak, recv, watcher, BTreeSet::new(), backstop, None);
    }));
    struct StopWorker(Arc<Hub>);
    impl Drop for StopWorker {
        fn drop(&mut self) {
            self.0.stop();
        }
    }
    let worker = StopWorker(hub.clone());
    let mut changes = hub.changed.subscribe();
    std::fs::remove_file(root.join("view/models/hulls/D.glb")).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            changes.changed().await.unwrap();
            let pages = hub.pages.read().unwrap();
            if hulls(&pages[""].documents[symphony_api::HULL_GALLERY_DOC]).is_empty() {
                break;
            }
        }
    })
    .await
    .expect("the idle worker must publish the removal without a filesystem notification");
    assert!(hulls(&document(address).await).is_empty());
    drop(worker);
    stop.send(()).unwrap();
    server.await.unwrap();
}

#[test]
fn a_models_link_outside_its_root_is_neither_shown_nor_swept() {
    let tree = Tree::new();
    let root = tree.path("served");
    let outside = tree.path("outside");
    tree.write("outside/view/models/hulls/X.glb", "outside model");
    std::fs::create_dir_all(root.join("view/models")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("view/models/hulls"),
        root.join("view/models/hulls"),
    )
    .unwrap();
    assert_eq!(symphony::models::warm(&outside), 1);
    assert_eq!(symphony::models::warm(&root), 0);
    // The link is stamped as itself, so a change behind it is none of the served root's.
    let roots = vec![(
        root.clone(),
        "view/models",
        notify::RecursiveMode::Recursive,
    )];
    let mut sweep = Sweep::new(&roots);
    tree.write("outside/view/models/hulls/Y.glb", "another outside model");
    assert!(sweep.next(&roots).is_empty());
}

#[test]
fn a_served_models_root_replaced_by_a_link_outside_reads_as_emptied_and_its_target_is_not_swept() {
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write(
        "main/docs/ships/HULLS.md",
        "# Hulls\n\n<div data-hull-gallery></div>\n",
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let served = tree.path("served");
    git(
        &root,
        &["worktree", "add", "-b", "served", served.to_str().unwrap()],
    );
    tree.write("served/view/models/hulls/OLD.glb", "model fixture");
    tree.write("outside/models/hulls/X.glb", "outside model");
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("served").unwrap();
    hub.document("served", symphony_api::HULL_GALLERY_DOC)
        .unwrap();
    let gallery = || -> Vec<String> {
        hulls(&hub.pages.read().unwrap()["served"].documents[symphony_api::HULL_GALLERY_DOC])
            .iter()
            .map(|hull| hull.id.clone())
            .collect()
    };
    assert_eq!(gallery(), ["OLD"]);
    assert!(!hub.sweep());

    // No event announces the swap.
    std::fs::remove_dir_all(served.join("view/models")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside/models"), served.join("view/models")).unwrap();
    assert!(hub.sweep(), "the linked root reads as emptied");
    assert_eq!(
        *hub.dirty.lock().unwrap(),
        BTreeSet::from([served.join("view/models/hulls/OLD.glb")])
    );
    refresh(&hub);
    assert!(gallery().is_empty(), "OLD leaves; nothing outside shows");
    tree.write("outside/models/hulls/Y.glb", "added behind the link");
    assert!(!hub.sweep(), "the link's target is not walked");

    // The same folder reached through a linked parent instead.
    std::fs::remove_file(served.join("view/models")).unwrap();
    std::fs::remove_dir(served.join("view")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside"), served.join("view")).unwrap();
    tree.write("outside/models/hulls/Z.glb", "behind the linked parent");
    assert!(
        !hub.sweep(),
        "a models root under a linked parent is not walked"
    );
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.gallery_changes.lock().unwrap().is_empty());
}

#[test]
fn a_models_root_linked_to_a_folder_holding_the_same_name_reports_one_removal_and_then_rests() {
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write(
        "main/docs/ships/HULLS.md",
        "# Hulls\n\n<div data-hull-gallery></div>\n",
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let served = tree.path("served");
    git(
        &root,
        &["worktree", "add", "-b", "served", served.to_str().unwrap()],
    );
    tree.write("served/view/models/hulls/OLD.glb", "local model");
    tree.write(
        "outside/models/hulls/OLD.glb",
        "outside model of the same name",
    );
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("served").unwrap();
    hub.document("served", symphony_api::HULL_GALLERY_DOC)
        .unwrap();
    let gallery = || -> Vec<String> {
        hulls(&hub.pages.read().unwrap()["served"].documents[symphony_api::HULL_GALLERY_DOC])
            .iter()
            .map(|hull| hull.id.clone())
            .collect()
    };
    assert_eq!(gallery(), ["OLD"]);
    assert!(!hub.sweep());

    // No event announces the swap. The removal's refresh restamps OLD's path, which now resolves
    // behind the link; the restamp reads nothing there, so later periods find nothing to report.
    std::fs::remove_dir_all(served.join("view/models")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside/models"), served.join("view/models")).unwrap();
    assert!(hub.sweep());
    assert_eq!(
        *hub.dirty.lock().unwrap(),
        BTreeSet::from([served.join("view/models/hulls/OLD.glb")])
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
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write(
        "main/docs/ships/HULLS.md",
        "# Hulls\n\n<div data-hull-gallery></div>\n",
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Gallery document"]);
    let branch = tree.path("branch");
    git(
        &root,
        &["worktree", "add", "-b", "before", branch.to_str().unwrap()],
    );
    let model = tree.write("branch/view/models/hulls/Z.glb", "model fixture");
    let hub = Hub::new(&root, &root.join("client/public"));
    *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
    hub.refresh_docs(None).unwrap();
    hub.ensure_page("before").unwrap();
    let initial = hub
        .document("before", symphony_api::HULL_GALLERY_DOC)
        .unwrap();
    assert_eq!(hulls(&initial)[0].id, "Z");
    assert!(!hub.sweep());
    assert!(hub.warmed_galleries.lock().unwrap().contains_key(&branch));

    // The switch moves refs the sweep follows; its refresh drops the page and leaves the folder's
    // gallery cold.
    git(&branch, &["switch", "-c", "after"]);
    assert!(hub.sweep());
    refresh(&hub);
    assert!(!hub.pages.read().unwrap().contains_key("before"));
    assert!(!hub.pages.read().unwrap().contains_key("after"));
    assert_eq!(hub.tree("after"), Some(branch.clone()));
    assert!(!hub.warmed_galleries.lock().unwrap().contains_key(&branch));
    std::fs::remove_file(model).unwrap();
    assert!(!hub.sweep(), "a tree without a page is left to its events");
    assert!(hub.dirty.lock().unwrap().is_empty());
    assert!(hub.gallery_changes.lock().unwrap().is_empty());

    // Opening the new scope reads its models afresh; what that read saw is no change.
    hub.ensure_page("after").unwrap();
    assert!(
        hulls(
            &hub.document("after", symphony_api::HULL_GALLERY_DOC)
                .unwrap()
        )
        .is_empty()
    );
    assert!(!hub.sweep());

    tree.write("branch/view/models/hulls/NEW.glb", "new model");
    git(
        &root,
        &["worktree", "remove", "--force", branch.to_str().unwrap()],
    );
    assert!(hub.sweep());
    refresh(&hub);
    assert!(hub.tree("after").is_none());
    assert!(!hub.pages.read().unwrap().contains_key("after"));
    assert!(!hub.warmed_galleries.lock().unwrap().contains_key(&branch));
    assert!(hub.gallery_changes.lock().unwrap().is_empty());
    assert!(!hub.sweep());
    assert!(hub.dirty.lock().unwrap().is_empty());
}

#[test]
fn gallery_paths_include_input_directories_and_deleted_files_but_not_neighbours() {
    let root = Path::new("/private/tree");
    for relative in [
        "view/models",
        "view/models/hulls/A.glb",
        "artifacts/concepts",
        "artifacts/concepts/redraw/A.png",
        "docs/data/hull-outlines",
        "docs/data/hull-outlines/A.notes.json",
    ] {
        assert_eq!(gallery_root(&root.join(relative)), Some(root), "{relative}");
    }
    for relative in [
        "docs/TOPIC.md",
        "view/models-old/A.glb",
        "artifacts/concepts-other/A.png",
        "docs/data/hull-outlines-other/A.svg",
    ] {
        assert_eq!(gallery_root(&root.join(relative)), None, "{relative}");
    }
}

#[test]
fn watcher_filters_access_and_metadata_and_invalidates_all_trees_on_rescan() {
    let tree = Tree::new();
    let root = repository(&tree);
    let branch = tree.path("branch");
    let hub = Hub::new(&root, &root.join("client/public"));
    hub.trees
        .write()
        .unwrap()
        .insert("branch".into(), branch.clone());
    for event in [
        Event::new(EventKind::Access(AccessKind::Read))
            .add_path(root.join("artifacts/concepts/A.png")),
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(root.join(".git/index")),
        Event::new(EventKind::Any),
    ] {
        assert!(!hub.record_event(Ok(event)));
        assert!(hub.dirty.lock().unwrap().is_empty());
        assert!(hub.gallery_changes.lock().unwrap().is_empty());
    }

    assert!(
        hub.record_event(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Both
        )))
        .add_path(root.join("artifacts/concepts/old.png"))
        .add_path(branch.join("artifacts/concepts/new.png"))))
    );
    let roots = BTreeSet::from([root.clone(), branch]);
    assert_eq!(*hub.gallery_changes.lock().unwrap(), roots);
    for event in [
        Ok(Event::new(EventKind::Any).set_flag(Flag::Rescan)),
        Err(notify::Error::generic("watcher lost events")),
    ] {
        hub.gallery_changes.lock().unwrap().clear();
        assert!(hub.record_event(event));
        assert_eq!(*hub.gallery_changes.lock().unwrap(), roots);
    }
}
