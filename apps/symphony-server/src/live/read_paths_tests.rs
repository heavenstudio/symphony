//! What a page's read costs the hub: a document's render holds no lock the board, the feed and the
//! project list read under, waits on no refresh's renders, and still publishes what it finds
//! before handing out its version, the copy kept being the one whose read began last, and a first
//! read that a refresh rendering anew, or a board read moving its tasks, began after reads again
//! once, then is left to the next refresh; a document, folder or agents read
//! reads no board; the task pages' reads and the task board's search share one store of the board,
//! dropped only once the board failed a read; a document read ahead (`view=first`) is neither
//! followed nor published.

use super::Hub;
use crate::front::{Shared, router};
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    git, repository, store_on,
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
use std::time::Duration;
use symphony_board::rpc::Client;
use tower::ServiceExt;

#[path = "../hosted/native_tests.rs"]
mod hosted;

/// The first bytes of a PNG three pixels wide and two high: all a render reads of a picture.
pub(super) const PNG: [u8; 24] = [
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
pub(super) fn requests(store: &symphony_board::Store) -> u64 {
    store
        .client()
        .queries()
        .report(symphony_board::sweep::trigger::now())
        .0
        .count
}

/// A FIFO at `path`: a render that reads it waits until the test writes.
pub(super) fn fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a live, terminated path; the FIFO is this test's.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

/// The write end of the FIFO at `path` once a reader has opened it, `None` before.
pub(super) fn writer(path: &Path) -> Option<std::fs::File> {
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

/// A first read of main's `rel` held in its read of the FIFO `picture` while `meanwhile` runs,
/// then let go: whether `meanwhile` ended while the read was held, and what the read handed out.
fn first_read_across(
    hub: &Hub,
    rel: &str,
    picture: &Path,
    meanwhile: impl FnOnce() + Send,
) -> (bool, symphony_api::Document) {
    fifo(picture);
    // Each wait is on its event; only a stall reaches this backstop.
    let backstop = test_time::backstop(Duration::from_secs(60));
    std::thread::scope(|scope| {
        let reading = scope.spawn(|| hub.document("", rel));
        let mut opened =
            test_time::until("the read to read the picture", backstop, || writer(picture)).unwrap();
        let during = scope.spawn(meanwhile);
        let ended_while_held =
            test_time::until_true("the refresh to end", backstop, || during.is_finished()).is_ok()
                && !reading.is_finished();
        opened.write_all(&PNG).unwrap();
        drop(opened);
        test_time::until_true("the read to end", backstop, || {
            if let Some(mut again) = writer(picture) {
                let _ = again.write_all(&PNG);
            }
            reading.is_finished() && during.is_finished()
        })
        .unwrap();
        during.join().unwrap();
        (ended_while_held, reading.join().unwrap().unwrap())
    })
}

/// Whether `doc`, as the wire carries it, says `words` anywhere: a card's line among them.
pub(super) fn holds(doc: &symphony_api::Document, words: &str) -> bool {
    serde_json::to_string(doc).unwrap().contains(words)
}

#[test]
fn a_first_read_begun_before_its_file_changed_and_placed_after_that_refresh_reads_again() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/SLOW.md", "# Slow\n\n![Held](held.png)\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    // The edit's refresh lists the documents the page follows while the first read is out: this
    // one is not among them yet.
    let (refreshed_while_held, read) =
        first_read_across(&hub, "docs/SLOW.md", &root.join("docs/held.png"), || {
            tree.write("main/docs/SLOW.md", "# Slow\n\nThe later words.\n");
            hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/SLOW.md")])))
                .unwrap();
        });
    assert!(refreshed_while_held, "the refresh waited on the first read");
    assert!(
        says(&read, "The later words."),
        "the first read handed out the copy it read before the edit"
    );
    // What the loop runs next for anything but main's files renders none of main's documents.
    hub.refresh_docs(Some(&BTreeSet::from(
        [root.join(".git/refs/heads/tools/x")],
    )))
    .unwrap();
    assert!(
        says(
            &hub.pages.read().unwrap()[""].documents["docs/SLOW.md"],
            "The later words."
        ),
        "the page follows the copy read before the edit"
    );
}

#[test]
fn a_first_read_behind_a_second_refresh_that_renders_anew_is_placed_and_left_to_the_next_refresh() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/SLOW.md", "# Slow\n\n![Held](held.png)\n");
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let (first, second) = (root.join("docs/held.png"), root.join("docs/again.png"));
    fifo(&first);
    fifo(&second);
    // Each wait is on its event; only a stall reaches this backstop.
    let backstop = test_time::backstop(Duration::from_secs(60));
    let edit = |words: &str, picture: &str| {
        tree.write(
            "main/docs/SLOW.md",
            &format!("# Slow\n\n{words}\n\n{picture}"),
        );
        hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/SLOW.md")])))
            .unwrap();
    };
    // A picture let go answers every later read of it too.
    let answer = |pictures: &[&Path]| {
        for picture in pictures {
            if let Some(mut again) = writer(picture) {
                let _ = again.write_all(&PNG);
            }
        }
    };
    // Each refresh renders anew while a read of the document is out: the first read, then the one
    // it reads again, which the first edit sent to the other picture.
    let (held, read) = std::thread::scope(|scope| {
        let reading = scope.spawn(|| hub.document("", "docs/SLOW.md"));
        let mut held = Vec::new();
        let mut let_go: Vec<&Path> = Vec::new();
        for (picture, words, next) in [
            (&first, "The second words.", "![Again](again.png)\n"),
            (&second, "The third words.", ""),
        ] {
            let mut opened = test_time::until("the read to read the picture", backstop, || {
                answer(&let_go);
                writer(picture)
            })
            .unwrap();
            edit(words, next);
            held.push(!reading.is_finished());
            opened.write_all(&PNG).unwrap();
            let_go.push(picture);
        }
        test_time::until_true("the read to end", backstop, || {
            answer(&let_go);
            reading.is_finished()
        })
        .unwrap();
        (held, reading.join().unwrap().unwrap())
    });
    assert_eq!(held, [true, true], "a refresh waited on the read");
    assert!(
        says(&read, "The second words."),
        "the read read once again, and no more"
    );
    assert!(
        hub.rescan.swap(false, Ordering::SeqCst),
        "the loop is asked for the refresh that renders it"
    );
    // What the loop runs next, though no file of main's changed since, renders it.
    hub.refresh_docs(Some(&BTreeSet::new())).unwrap();
    let pages = hub.pages.read().unwrap();
    assert!(says(
        &pages[""].documents["docs/SLOW.md"],
        "The third words."
    ));
    let published = pages[""].history.back().map(|event| event.json.clone());
    assert!(
        published
            .as_deref()
            .is_some_and(|json| json.contains("The third words.")),
        "{published:?}"
    );
}

#[test]
fn a_first_read_begun_before_its_one_card_moved_and_placed_after_that_refresh_reads_again() {
    let tree = Tree::new();
    let root = repository(&tree);
    let register = |lines: &str| {
        format!(
            "# Topic\n\n![Held](held.png)\n\n<!-- register:start -->\n- [ ] **initial-card** — Initial question.\n{lines}<!-- register:end -->\n"
        )
    };
    tree.write("main/docs/TOPIC.md", &register(""));
    git(&root, &["commit", "-qam", "Register the fixture decisions"]);
    let branch = tree.path("proposal");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "design/policy",
            branch.to_str().unwrap(),
        ],
    );
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_docs(None).unwrap();
    let answer = "The branch's answer.";
    // The branch posts a line on main's card while the first read, which took main's one cards
    // before them, is out: the refresh moves the card with no file of main's changed.
    let (refreshed_while_held, read) =
        first_read_across(&hub, "docs/TOPIC.md", &root.join("docs/held.png"), || {
            tree.write(
                "proposal/docs/TOPIC.md",
                &register(&format!("  [2026-11-11 10:00] agent — {answer}\n")),
            );
            git(&branch, &["commit", "-qam", "Answer on the branch"]);
            let branch = branch.canonicalize().unwrap();
            hub.refresh_docs(Some(&BTreeSet::from([branch.join("docs/TOPIC.md")])))
                .unwrap();
        });
    assert!(refreshed_while_held, "the refresh waited on the first read");
    assert!(
        holds(&read, answer),
        "the first read handed out the card it read before the branch's line"
    );
    assert!(
        holds(
            &hub.pages.read().unwrap()[""].documents["docs/TOPIC.md"],
            answer
        ),
        "the page follows the card read before the branch's line"
    );
}

#[test]
fn a_first_read_begun_before_a_board_read_moved_its_linking_task_reads_again() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/SLOW.md", "# Slow\n\n![Held](held.png)\n");
    let f = Fixture::new(Seed::default().task("tools/slow", "ready").link(
        "tools/slow",
        "doc",
        "docs/SLOW.md",
    ));
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &f.store, &[]);
    let status = |doc: &symphony_api::Document| {
        doc.tasks
            .iter()
            .map(|task| task.status.clone())
            .collect::<Vec<_>>()
    };
    // The board read lists the documents the page follows while the first read, which took the
    // board's tasks before it, is out: this one is not among them yet.
    let (read_while_held, read) =
        first_read_across(&hub, "docs/SLOW.md", &root.join("docs/held.png"), || {
            f.store
                .client()
                .query(
                    "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/slow' RETURN NONE;",
                    serde_json::json!({}),
                )
                .unwrap();
            observe(&hub, &f.store, &[]);
        });
    assert!(read_while_held, "the board read waited on the first read");
    assert_eq!(
        status(&read),
        ["in_progress"],
        "the first read handed out the tasks it read before the board's"
    );
    assert_eq!(
        status(&hub.pages.read().unwrap()[""].documents["docs/SLOW.md"]),
        ["in_progress"],
        "the page follows the tasks read before the board's"
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
    let shared = Shared::new(1, 1, root, None, crate::testing::open_gate());
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
    // A task page's history and its diff, each read on the store the task pages share.
    for route in ["/api/task-history", symphony_api::TASK_DIFF_PATH] {
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

        let asked = format!("{route}?slug=tools/open");
        let (status, body) = ask(&asked);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{route}: {body}");
        for _ in 0..2 {
            let (status, body) = ask(&asked);
            assert_eq!(status, StatusCode::OK, "{route}: {body}");
        }
        assert_eq!(
            connects.load(Ordering::SeqCst),
            2,
            "{route}: the failed store is opened anew once, then kept"
        );
        shared.stop();
    }
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
fn a_task_board_search_reads_on_the_store_the_task_pages_share_and_drops_it_only_when_the_board_failed()
 {
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
    // A query the search refuses sends the board nothing: the store stays.
    let long = (0..40)
        .map(|n| format!("w{n}"))
        .collect::<Vec<_>>()
        .join("+");
    let (status, body) = ask(&format!("/api/task-board?q={long}"));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.to_string().contains("at most 32 terms"), "{body}");
    let (status, body) = ask("/api/task-page?slug=tools/open");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the store a failed search read on is opened anew once, then every search, refused or \
         not, and task page read shares it"
    );
    shared.stop();
}

#[test]
fn agent_page_order_study_preserves_full_payload_filters_and_pagination() {
    for count in [0, 1, 32, 512] {
        let tree = Tree::new();
        let root = repository(&tree);
        let f = Fixture::new(Seed::default());
        let mut hub = Hub::new(&root, &root.join("build/client"));
        let board = f.other_client();
        hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
        hub.refresh_page("", &root).unwrap();
        let mut rows: Vec<_> = (0..count)
            .map(|i| {
                let mut row = agent(&format!("agent-{i}"));
                row.model = format!("model-{i}");
                row.health = ["live", "lost", "left", "live", "stale"][i % 5].into();
                row.worktree = format!("/private/fixture/{i}");
                row.branch = format!("branch-{i}");
                row.pane = Some(format!("pane-{i}"));
                row.last_tool_call = Some(f64::from(u32::try_from(i).unwrap()));
                row
            })
            .collect();
        // The board supplies connected-time order rather than lexical handle order.
        rows = (0..count).map(|i| rows[(i * 73) % count].clone()).collect();
        let view = symphony_api::Agents {
            machine_work: Default::default(),
            generated: NOW,
            probes: symphony_api::AgentProbes {
                panes: "ok".into(),
                slots: "ok".into(),
            },
            agents: rows,
        };
        hub.update_board(&f.store, f.store.board().unwrap(), view.clone())
            .unwrap();
        let (shared, ask) = served(&root, hub);
        for (query, lost, after, offset) in [
            ("", false, None, 0),
            ("?offset=7", false, None, 7),
            ("?cursor=agent-10", false, Some("agent-10"), 0),
            ("?health=lost", true, None, 0),
            (
                "?health=lost&cursor=agent-10&offset=2",
                true,
                Some("agent-10"),
                2,
            ),
        ] {
            let mut expected = view.clone();
            expected.agents.retain(|a| {
                if lost {
                    a.health == "lost"
                } else {
                    a.health != "lost" && a.health != "left"
                }
            });
            let total = expected.agents.len();
            let mut candidate = expected.agents.clone();
            expected.agents.sort_by(|a, b| a.handle.cmp(&b.handle));
            symphony_board::order::sort_by(
                &mut candidate,
                &|a: &symphony_api::Agent, b: &symphony_api::Agent| a.handle.cmp(&b.handle),
            );
            assert_eq!(
                serde_json::to_value(&candidate).unwrap(),
                serde_json::to_value(&expected.agents).unwrap()
            );
            if let Some(after) = after {
                expected.agents.retain(|a| a.handle.as_str() > after);
            }
            expected.agents = expected.agents.into_iter().skip(offset).take(20).collect();
            let next = expected
                .agents
                .last()
                .filter(|_| expected.agents.len() == 20)
                .map(|a| a.handle.clone());
            let expected = symphony_api::AgentPage {
                view: expected,
                tasks: vec![],
                total,
                next,
                local: true,
            };
            let (status, actual) = ask(&format!("/api/agent-page{query}"));
            assert_eq!(status, StatusCode::OK, "{actual}");
            assert_eq!(
                actual,
                serde_json::to_value(expected).unwrap(),
                "count={count}, query={query}"
            );
        }
        shared.stop();
    }
}

#[test]
fn activity_order_study_preserves_full_payload_stable_ties_and_truncate_boundary() {
    for count in [0, 1, 32, 80, 512, 4096] {
        let mut original: Vec<_> = (0..count)
            .map(|i| symphony_api::ActivityItem {
                id: format!("item-{}", i / 2),
                kind: if i % 3 == 0 { "task message" } else { "reply" }.into(),
                path: format!("docs/{i}.md"),
                slug: format!("card-{i}"),
                ts: format!("2026-09-{:02} 09:{:02}", (i / 2) % 28 + 1, (i / 2) % 60),
                text: format!("Exact payload {i}"),
                href: format!("/docs/{i}.md#card-{i}"),
                answered: i % 2 == 0,
            })
            .collect();
        original = (0..count)
            .map(|i| original[(i * 73) % count].clone())
            .collect();
        let mut candidate = original.clone();
        let mut live = symphony_api::Activity {
            items: original.clone(),
        };
        original.sort_by(|a, b| (&b.ts, &b.id).cmp(&(&a.ts, &a.id)));
        original.truncate(40);
        symphony_board::order::sort_by(
            &mut candidate,
            &|a: &symphony_api::ActivityItem, b: &symphony_api::ActivityItem| {
                (&b.ts, &b.id).cmp(&(&a.ts, &a.id))
            },
        );
        candidate.truncate(40);
        super::sort_activity(&mut live);
        assert_eq!(candidate, original, "count={count}");
        assert_eq!(live.items, original, "count={count}");
    }
}

#[test]
fn activity_order_study_keeps_native_forty_plus_forty_merge_and_refresh_payload() {
    let tree = Tree::new();
    let root = repository(&tree);
    let mut document = String::from("# Activity\n\n<!-- register:start -->\n");
    for i in 0..40 {
        document.push_str(&format!(
            "- [ ] **card-{i:02}** — Question {i}.\n  [2026-09-25 09:{i:02}] agent — Posted {i}.\n"
        ));
    }
    document.push_str("<!-- register:end -->\n");
    tree.write("main/docs/TOPIC.md", &document);
    let mut seed = Seed::default()
        .session("amber")
        .task("tools/open", "in_progress")
        .role("tools/open", "amber", "executor");
    for id in 1..=40 {
        seed.dump.messages.push(symphony_board::rows::MessageRow {
            id,
            slug: "tools/open".into(),
            sender: "amber".into(),
            recipient: Some("owner".into()),
            kind: "msg".into(),
            text: format!("Task payload {id}"),
            ts: NOW + f64::from(i32::try_from(id).unwrap()),
            person: false,
        });
    }
    let f = Fixture::new(seed);
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let board = f.other_client();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    hub.refresh_page("", &root).unwrap();
    let mut expected = hub.pages.read().unwrap()[""].activity.items.clone();
    assert_eq!(expected.len(), 40, "all document replies reach the merge");
    let tasks = symphony::taskview::activity(&f.store).unwrap();
    assert_eq!(tasks.len(), 40, "the board's bounded owner-message input");
    expected.extend(tasks);
    assert_eq!(expected.len(), 80);
    expected.sort_by(|a, b| (&b.ts, &b.id).cmp(&(&a.ts, &a.id)));
    expected.truncate(40);
    observe(&hub, &f.store, &["amber"]);
    assert_eq!(hub.pages.read().unwrap()[""].activity.items, expected);
    hub.refresh_page("", &root).unwrap();
    assert_eq!(hub.pages.read().unwrap()[""].activity.items, expected);
    let (shared, ask) = served(&root, hub);
    let (status, actual) = ask("/api/activity");
    assert_eq!(status, StatusCode::OK, "{actual}");
    assert_eq!(
        actual,
        serde_json::to_value(symphony_api::Activity { items: expected }).unwrap()
    );
    shared.stop();
}
