//! What a page's read costs the hub: a document's render holds no lock the board, the feed and the
//! project list read under, waits on no refresh's renders, and still publishes what it finds
//! before handing out its version, the copy kept being the one whose read began last; a
//! document, folder or agents read reads no board; the task pages' reads and the task board's
//! search share one store of the board; a document read ahead (`view=first`) is neither followed
//! nor published.

use super::Hub;
use crate::front::{Shared, router};
use crate::gate::Gate;
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    repository,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use std::collections::BTreeSet;
use std::io::Write;
use std::net::SocketAddr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use symphony_board::rpc::Client;
use tower::ServiceExt;

/// The first bytes of a PNG three pixels wide and two high: all a render reads of a picture.
const PNG: [u8; 24] = [
    0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0,
    0, 3, 0, 0, 0, 2,
];

fn agent(handle: &str) -> symphony_api::Agent {
    serde_json::from_value(serde_json::json!({
        "handle": handle, "kind": "worker", "model": "", "parent": null, "task": null,
        "role": null, "tasks": [], "worktree": "", "branch": "", "agent": null,
        "session_id": null, "pane": null, "state": "working", "state_detail": null,
        "health": "live", "last_tool_call": null, "last_message": null,
    }))
    .unwrap()
}

/// The board as the hub's tick reads it, with `agents` working on it.
pub(super) fn observe(hub: &Hub, store: &symphony_board::Store, agents: &[&str]) {
    hub.update_board(
        store,
        store.board().unwrap(),
        symphony_api::Agents {
            machine_work: symphony_api::MachineWork::default(),
            generated: store.now(),
            probes: symphony_api::AgentProbes {
                panes: "ok".into(),
                slots: "ok".into(),
            },
            agents: agents.iter().map(|handle| agent(handle)).collect(),
        },
    )
    .unwrap();
}

/// How many requests every client of `store`'s board in this process has sent it.
fn requests(store: &symphony_board::Store) -> u64 {
    store
        .client()
        .queries()
        .report(symphony_board::sweep::trigger::now())
        .0
        .count
}

/// A FIFO at `path`: a render that reads it waits until the test writes.
fn fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a live, terminated path; the FIFO is this test's.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

/// The write end of the FIFO at `path` once a reader has opened it, `None` before.
fn writer(path: &Path) -> Option<std::fs::File> {
    match std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => Some(file),
        Err(error) if error.raw_os_error() == Some(libc::ENXIO) => None,
        Err(error) => panic!("{}: {error}", path.display()),
    }
}

/// Run `render` while the picture at `picture` is a FIFO it reads, and say, from the moment it
/// opened the picture, whether `pages` could be taken by a writer: no read or write of it held.
/// The picture's bytes then let it finish, and every later read of it too.
fn while_reading<T: Send>(
    hub: &Hub,
    picture: &Path,
    render: impl FnOnce() -> T + Send,
) -> (bool, T) {
    fifo(picture);
    // Each wait is on its event; only a stall reaches this backstop.
    let backstop = test_time::backstop(Duration::from_secs(60));
    std::thread::scope(|scope| {
        let rendering = scope.spawn(render);
        let opened = test_time::until("the render to read the picture", backstop, || {
            match writer(picture) {
                Some(opened) => Some(Some(opened)),
                // It ended without reading it.
                None if rendering.is_finished() => Some(None),
                None => None,
            }
        })
        .unwrap();
        let mut opened = opened.expect("the render never read the picture");
        // The render is in its read of the picture now, and waits there.
        let free = hub.pages.try_write().is_ok();
        opened.write_all(&PNG).unwrap();
        drop(opened);
        test_time::until_true("the render to end", backstop, || {
            if let Some(mut again) = writer(picture) {
                let _ = again.write_all(&PNG);
            }
            rendering.is_finished()
        })
        .unwrap();
        (free, rendering.join().unwrap())
    })
}

#[test]
fn a_document_render_holds_no_lock_the_board_the_feed_and_the_project_list_read_under() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/PAUSED.md", "# Paused\n\n![A pause](pause.png)\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();

    let (free, doc) = while_reading(&hub, &root.join("docs/pause.png"), || {
        hub.document("", "docs/PAUSED.md")
    });
    assert!(free, "a document read renders under the pages' lock");
    let doc = doc.unwrap();
    assert!(
        doc.sections
            .iter()
            .any(|section| section.html.contains(r#"width="3" height="2""#)),
        "the render read the picture it waited on: {:?}",
        doc.sections
    );
    assert!(
        hub.pages.read().unwrap()[""]
            .documents
            .contains_key("docs/PAUSED.md"),
        "the page keeps its copy"
    );

    // The watcher's refresh of a document a page follows renders it again, outside the lock too.
    std::fs::remove_file(root.join("docs/pause.png")).unwrap();
    tree.write("main/docs/PAUSED.md", "# Paused\n\n![Later](later.png)\n");
    let (free, refreshed) = while_reading(&hub, &root.join("docs/later.png"), || {
        hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/PAUSED.md")])))
    });
    assert!(
        free,
        "a refresh renders its documents under the pages' lock"
    );
    refreshed.unwrap();
    let pages = hub.pages.read().unwrap();
    let page = &pages[""];
    assert!(
        page.documents["docs/PAUSED.md"]
            .sections
            .iter()
            .any(|section| section.html.contains("later.png")),
        "the refresh's copy is the page's"
    );
    let published = page
        .history
        .back()
        .expect("the refresh published its change");
    assert!(published.json.contains("later.png"), "{}", published.json);
}

/// Whether a section of `doc` says `words`.
fn says(doc: &symphony_api::Document, words: &str) -> bool {
    doc.sections
        .iter()
        .any(|section| section.html.contains(words))
}

#[test]
fn a_document_read_waits_on_no_refreshs_renders_and_the_copy_whose_read_began_last_is_kept() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/QUICK.md", "# Quick\n\nThe quick words.\n");
    tree.write("main/docs/SLOW.md", "# Slow\n\nThe first words.\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    hub.document("", "docs/QUICK.md").unwrap();
    hub.document("", "docs/SLOW.md").unwrap();
    // The watcher's refresh renders both again, SLOW last, and waits in its read of SLOW's picture.
    tree.write("main/docs/SLOW.md", "# Slow\n\n![Held](held.png)\n");
    let picture = root.join("docs/held.png");
    fifo(&picture);
    // Each wait is on its event; only a stall reaches this backstop.
    let backstop = test_time::backstop(Duration::from_secs(60));
    let (read_while_held, slow) = std::thread::scope(|scope| {
        let refreshing =
            scope.spawn(|| hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/SLOW.md")]))));
        let mut opened = test_time::until("the refresh to read the picture", backstop, || {
            writer(&picture)
        })
        .unwrap();
        // While it waits there, a reader reads the other document, and SLOW, edited since.
        tree.write("main/docs/SLOW.md", "# Slow\n\nThe later words.\n");
        let reading = scope.spawn(|| {
            (
                hub.document("", "docs/QUICK.md"),
                hub.document("", "docs/SLOW.md"),
            )
        });
        let read_while_held =
            test_time::until_true("the reads to end", backstop, || reading.is_finished()).is_ok()
                && !refreshing.is_finished();
        // The picture's bytes let the refresh end, the reads too had they waited on it.
        opened.write_all(&PNG).unwrap();
        drop(opened);
        test_time::until_true("the refresh to end", backstop, || {
            if let Some(mut again) = writer(&picture) {
                let _ = again.write_all(&PNG);
            }
            refreshing.is_finished()
        })
        .unwrap();
        refreshing.join().unwrap().unwrap();
        let (quick, slow) = reading.join().unwrap();
        assert!(says(&quick.unwrap(), "The quick words."));
        (read_while_held, slow.unwrap())
    });
    assert!(read_while_held, "a reader's render waited on the refresh's");
    assert!(says(&slow, "The later words."));
    let pages = hub.pages.read().unwrap();
    let page = &pages[""];
    assert!(
        says(&page.documents["docs/SLOW.md"], "The later words."),
        "the refresh's copy, read before the reader's, replaced it"
    );
    assert!(
        page.history
            .iter()
            .all(|event| !event.json.contains("held.png")),
        "the refresh published the copy it read before the reader's"
    );
}

#[test]
fn a_document_read_that_finds_an_edit_first_publishes_it_then_hands_out_its_version() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/EDITED.md", "# Edited\n\nThe first words.\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let published = |hub: &Hub| hub.pages.read().unwrap()[""].history.len();

    let first = hub.document("", "docs/EDITED.md").unwrap();
    let before = published(&hub);
    let again = hub.document("", "docs/EDITED.md").unwrap();
    assert_eq!(
        published(&hub),
        before,
        "an unchanged document publishes nothing"
    );
    assert_eq!(again.version, first.version);

    // The read sees the edit before the watcher does.
    tree.write("main/docs/EDITED.md", "# Edited\n\nThe later words.\n");
    let edited = hub.document("", "docs/EDITED.md").unwrap();
    let pages = hub.pages.read().unwrap();
    let page = &pages[""];
    assert_eq!(page.history.len(), before + 1, "the edit is published once");
    let event = page.history.back().unwrap();
    assert!(event.json.contains("The later words."), "{}", event.json);
    assert_eq!(
        edited.version, event.version,
        "the version handed out is the one that carries the edit"
    );
    assert!(
        page.documents["docs/EDITED.md"]
            .sections
            .iter()
            .any(|section| section.html.contains("The later words.")),
        "the page keeps the edited copy"
    );
}

#[test]
fn a_documents_read_ahead_follows_nothing_and_publishes_nothing_and_its_visit_follows_it() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/docs/AHEAD.md",
        "# Ahead\n\nRead before it is opened.\n",
    );
    tree.write("main/docs/OPEN.md", "# Open\n\nThe first words.\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let (shared, ask) = served(&root, hub);
    let hub = shared.api.get().unwrap();
    let followed = || -> Vec<String> {
        hub.pages.read().unwrap()[""]
            .documents
            .keys()
            .cloned()
            .collect()
    };
    let published = || hub.pages.read().unwrap()[""].history.len();

    let (status, first) = ask("/api/document?path=docs/AHEAD.md&view=first");
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(
        first["document"]["sections"]
            .to_string()
            .contains("Read before it is opened."),
        "{first}"
    );
    assert_eq!(
        followed(),
        Vec::<String>::new(),
        "a read ahead follows nothing"
    );

    // A document a visit follows keeps the visit's copy: an edit a read ahead sees is the
    // watcher's to publish, not the read ahead's.
    let (status, _) = ask("/api/document?path=docs/OPEN.md&view=outline");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(followed(), ["docs/OPEN.md"]);
    tree.write("main/docs/OPEN.md", "# Open\n\nThe later words.\n");
    let before = published();
    let (status, first) = ask("/api/document?path=docs/OPEN.md&view=first");
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(first.to_string().contains("The later words."), "{first}");
    assert_eq!(published(), before, "a read ahead publishes nothing");
    assert!(
        hub.pages.read().unwrap()[""].documents["docs/OPEN.md"]
            .sections
            .iter()
            .any(|section| section.html.contains("The first words.")),
        "the followed copy is the visit's"
    );
    assert_eq!(followed(), ["docs/OPEN.md"]);

    // Opened, its outline read follows it.
    let (status, _) = ask("/api/document?path=docs/AHEAD.md&view=outline");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(followed(), ["docs/AHEAD.md", "docs/OPEN.md"]);
}

#[test]
fn a_document_or_folder_read_takes_its_tasks_from_the_board_the_hub_holds_and_reads_no_board() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/docs/README.md",
        "# Docs\n\nWhere the documents start.\n",
    );
    let f = Fixture::new(
        Seed::default()
            .task("tools/topic", "ready")
            .link("tools/topic", "doc", "docs/TOPIC.md#initial-card")
            .task("tools/intro", "in_progress")
            .link("tools/intro", "doc", "docs/README.md")
            .task("tools/unlinked", "ready"),
    );
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &f.store, &[]);
    let slugs = |tasks: &[symphony_api::DocumentTask]| {
        tasks
            .iter()
            .map(|task| (task.slug.clone(), task.status.clone()))
            .collect::<Vec<_>>()
    };

    let before = requests(&f.store);
    let doc = hub.document("", "docs/TOPIC.md").unwrap();
    let sections = hub
        .document_sections("", "docs/README.md", &["intro".into()])
        .unwrap();
    let folder = hub.folder("", "docs").unwrap();
    assert_eq!(
        requests(&f.store),
        before,
        "a document, its sections and a folder read no board"
    );
    assert_eq!(
        slugs(&doc.tasks),
        [("tools/topic".to_string(), "ready".to_string())]
    );
    assert_eq!(sections.change.path, "docs/README.md");
    assert_eq!(
        slugs(&folder.intro.expect("the folder has an introduction").tasks),
        [("tools/intro".to_string(), "in_progress".to_string())]
    );
    assert_eq!(
        slugs(&hub.pages.read().unwrap()[""].documents["docs/README.md"].tasks),
        [("tools/intro".to_string(), "in_progress".to_string())],
        "the copy a page keeps lists them too"
    );
}

fn request(path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut request = Request::builder().uri(path).header("Host", "127.0.0.1:1");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::empty()).unwrap()
}

/// `hub`, published as the server's, and a way to ask its routes: each answer's status and JSON.
pub(super) fn served(
    root: &Path,
    hub: Hub,
) -> (
    Arc<Shared>,
    impl Fn(&str) -> (StatusCode, serde_json::Value) + use<>,
) {
    let (shared, send) = served_with(root, hub);
    let ask = move |path: &str| {
        let (status, _, body) = send(path, &[]);
        (status, serde_json::from_slice(&body).unwrap())
    };
    (shared, ask)
}

/// An answer's status, headers and body.
pub(super) type Answer = (StatusCode, HeaderMap, Vec<u8>);

/// A request's headers, each a name and its value.
pub(super) type Headers<'a> = &'a [(&'a str, &'a str)];

/// `hub`, published as the server's, and a way to ask its routes with the request's headers named.
pub(super) fn served_with(
    root: &Path,
    hub: Hub,
) -> (Arc<Shared>, impl Fn(&str, Headers<'_>) -> Answer + use<>) {
    let shared = Shared::new(
        1,
        1,
        root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let app = router(shared.clone()).layer(MockConnectInfo(
        "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
    ));
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let send = move |path: &str, headers: &[(&str, &str)]| {
        let app = app.clone();
        runtime.block_on(async {
            let backstop = test_time::backstop(Duration::from_secs(60));
            let response = tokio::time::timeout(backstop, app.oneshot(request(path, headers)))
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{path} to answer: stalled: the wait's wall-clock backstop of {backstop:?} \
                         ran out; {}",
                        test_time::Load::now()
                    )
                })
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, headers, body.to_vec())
        })
    };
    (shared, send)
}

/// A store on `client`'s board at the fixture's time and host.
pub(super) fn store_on(client: &Client) -> symphony_board::Store {
    symphony_board::Store::with(
        Client::new(client.addr(), "sf", client.database()),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
    )
    .unwrap()
}

#[test]
fn a_task_pages_reads_share_one_store_of_the_board_and_the_agents_pages_read_no_board() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(
        Seed::default()
            .session("amber")
            .task("tools/open", "in_progress")
            .role("tools/open", "amber", "executor")
            .task("tools/done", "done")
            .role("tools/done", "amber", "executor")
            .task("tools/other", "ready"),
    );
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let connects = Arc::new(AtomicUsize::new(0));
    let (counted, board) = (Arc::clone(&connects), f.other_client());
    hub.connect_reader = Box::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(store_on(&board))
    });
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &f.store, &["amber"]);
    let (shared, ask) = served(&root, hub);

    let before = requests(&f.store);
    let (status, page) = ask("/api/agent-page");
    assert_eq!(status, StatusCode::OK, "{page}");
    let page: symphony_api::AgentPage = serde_json::from_value(page).unwrap();
    assert_eq!(
        page.tasks
            .iter()
            .map(|task| task.slug.as_str())
            .collect::<Vec<_>>(),
        ["tools/open", "tools/done"],
        "the agent's open and finished work, from the hub's board"
    );
    let (status, agents) = ask("/api/agents");
    assert_eq!(status, StatusCode::OK, "{agents}");
    assert_eq!(
        (requests(&f.store), connects.load(Ordering::SeqCst)),
        (before, 0),
        "the agents pages read no board and open none"
    );

    for slug in ["tools/open", "tools/done"] {
        for route in ["task-page", "task-offer", "task-history", "task-brief"] {
            let (status, body) = ask(&format!("/api/{route}?slug={slug}"));
            assert_eq!(status, StatusCode::OK, "{route} of {slug}: {body}");
        }
        let (status, page) = ask(&format!("/api/task-page?slug={slug}"));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["task"]["slug"], slug);
    }
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "every task page's reads go on the one store the first opened"
    );
    let (status, _) = ask("/api/task-page?slug=tools/missing");
    assert_eq!(status, StatusCode::NOT_FOUND, "no such task");
    let (status, _) = ask("/api/task-page?slug=tools/other");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "a task not found is no failure of the board's"
    );

    // While the board cannot be read, its agents and their work cannot be either.
    shared
        .api
        .get()
        .unwrap()
        .board_error("the board went".into());
    for route in ["/api/agents", "/api/agent-page"] {
        let (status, body) = ask(route);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{route}: {body}");
    }
    shared.stop();
}

#[test]
fn a_store_the_board_failed_is_dropped_and_the_next_read_opens_the_board_anew() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(Seed::default().task("tools/open", "ready"));
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let connects = Arc::new(AtomicUsize::new(0));
    let (counted, board) = (Arc::clone(&connects), f.other_client());
    hub.connect_reader = Box::new(move |_| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            // A server that went after the store was opened: nothing listens where it was.
            let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = gone.local_addr().unwrap().to_string();
            drop(gone);
            return Ok(symphony_board::Store::unchecked(
                Client::new(&addr, "sf", "gone").waiting(Duration::from_secs(1)),
                Box::new(|| NOW),
                Box::new(|_| false),
                HOST,
            ));
        }
        Ok(store_on(&board))
    });
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &f.store, &[]);
    let (shared, ask) = served(&root, hub);

    let (status, body) = ask("/api/task-history?slug=tools/open");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    for _ in 0..2 {
        let (status, body) = ask("/api/task-history?slug=tools/open");
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the failed store is opened anew once, then kept"
    );
    shared.stop();
}

/// The slugs a task board read answered with.
fn listed(board: &serde_json::Value) -> Vec<String> {
    board["tasks"]
        .as_array()
        .unwrap_or_else(|| panic!("{board}"))
        .iter()
        .map(|task| task["slug"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_task_board_search_reads_on_the_store_the_task_pages_share_and_drops_one_that_failed() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(
        Seed::default()
            .task("tools/open", "ready")
            .task("tools/other", "ready"),
    );
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let connects = Arc::new(AtomicUsize::new(0));
    let (counted, board) = (Arc::clone(&connects), f.other_client());
    hub.connect_reader = Box::new(move |_| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            // A server that went after the store was opened: nothing listens where it was.
            let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = gone.local_addr().unwrap().to_string();
            drop(gone);
            return Ok(symphony_board::Store::unchecked(
                Client::new(&addr, "sf", "gone").waiting(Duration::from_secs(1)),
                Box::new(|| NOW),
                Box::new(|_| false),
                HOST,
            ));
        }
        Ok(store_on(&board))
    });
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &f.store, &[]);
    let (shared, ask) = served(&root, hub);

    let (status, body) = ask("/api/task-board?q=open");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    for _ in 0..2 {
        let (status, body) = ask("/api/task-board?q=open");
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(listed(&body), ["tools/open"]);
    }
    let (status, body) = ask("/api/task-page?slug=tools/open");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the store a failed search read on is opened anew once, then every search and task page \
         read shares it"
    );
    shared.stop();
}
