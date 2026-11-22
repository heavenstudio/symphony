//! A document page left open follows its linked tasks: the hub's board read (`Hub::board_read`,
//! its tick's) that moves a task linking a followed document, or a task coming to link it, renders
//! that document again and no other, and the feed the page follows it on sends its new tasks; one
//! that moves nothing a followed document lists renders nothing and sends it nothing, and it looks
//! at the followed documents only once a task came, went or moved its slug, title, status or links,
//! or the board could not be read before it. Renders are counted, never timed.

use super::{Hub, relinks};
use crate::front::{Shared, router};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    git, repository,
};
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tower::ServiceExt;

const TOPIC: &str = "docs/TOPIC.md";
const OTHER: &str = "docs/OTHER.md";

/// The hub over `root`, served as the server's: its routes asked as a page asks them.
struct Served {
    hub: Arc<Hub>,
    app: Router,
    runtime: tokio::runtime::Runtime,
}

impl Served {
    fn new(root: &Path) -> Self {
        let hub = Hub::new(root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        let shared = Shared::new(1, 1, root, None, crate::testing::open_gate());
        assert!(shared.api.set(Arc::new(hub)).is_ok());
        let hub = shared.api.get().unwrap().clone();
        let app = router(shared).layer(MockConnectInfo(
            "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
        ));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        Self { hub, app, runtime }
    }

    /// `run` within the wall-clock backstop only a stall reaches.
    fn within<T>(&self, what: &str, run: impl Future<Output = T>) -> T {
        self.runtime.block_on(async {
            let backstop = test_time::backstop(Duration::from_secs(60));
            tokio::time::timeout(backstop, run)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{what}: stalled: the wait's wall-clock backstop of {backstop:?} ran out; {}",
                        test_time::Load::now()
                    )
                })
        })
    }

    fn get(&self, path: &str) -> axum::response::Response {
        let request = Request::builder()
            .uri(path)
            .header("Host", "127.0.0.1:1")
            .body(Body::empty())
            .unwrap();
        self.within(path, self.app.clone().oneshot(request))
            .unwrap()
    }

    /// The page at `prefix` (`""` for main's, `/@<tree>` for a branch's) opening `document`, as
    /// the client reads it: its outline, the document followed from then on. The feed's cursor
    /// the read stands at.
    fn open(&self, prefix: &str, document: &str) -> u64 {
        let response = self.get(&format!(
            "{prefix}/api/document?path={document}&view=outline"
        ));
        assert_eq!(response.status(), StatusCode::OK, "{document}");
        response.headers()[symphony_api::VERSION_HEADER]
            .to_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// The changes the feed of the page at `prefix` sends a page showing `document` from `since`
    /// up to the page's version now, and that version.
    fn fed(&self, prefix: &str, since: u64, document: &str) -> (Vec<Value>, u64) {
        let scope = prefix.strip_prefix("/@").unwrap_or_default();
        let until = self.hub.cursor(scope).unwrap();
        if until == since {
            return (Vec::new(), until);
        }
        let feed = format!("{prefix}/api/feed?since={since}&document={document}");
        let mut body = self.get(&feed).into_body().into_data_stream();
        let mut changes = Vec::new();
        let mut text = String::new();
        self.within(&feed, async {
            loop {
                let chunk = body.next().await.expect("the feed goes on").unwrap();
                text.push_str(std::str::from_utf8(&chunk).unwrap());
                while let Some((frame, rest)) = text.split_once("\n\n") {
                    let frame = frame.to_string();
                    text = rest.to_string();
                    let Some(data) = frame
                        .strip_prefix(&format!("event: {}\n", symphony_api::FEED_EVENT))
                        .and_then(|delta| delta.split_once("\ndata: "))
                        .map(|(_, data)| data)
                    else {
                        continue;
                    };
                    let event: Value = serde_json::from_str(data).unwrap();
                    changes.extend(event["changes"].as_array().unwrap().iter().cloned());
                    if event["version"] == until {
                        return;
                    }
                }
            }
        });
        (changes, until)
    }

    fn renders(&self) -> u64 {
        self.hub.renders.load(Ordering::SeqCst)
    }
}

/// The document changes among `changes`.
fn documents(changes: &[Value]) -> Vec<&Value> {
    changes
        .iter()
        .filter(|change| change["region"] == "document")
        .collect()
}

/// The tasks a page lists beside a document: each one's slug, title and status.
fn tasks(listed: &[(&str, &str, &str)]) -> Value {
    json!(
        listed
            .iter()
            .map(|(slug, title, status)| json!({"slug": slug, "title": title, "status": status}))
            .collect::<Vec<_>>()
    )
}

/// The board's `query`, run on the fixture's board.
fn moves(f: &Fixture, query: &str) {
    f.store.client().query(query, json!({})).unwrap();
}

fn board() -> Fixture {
    Fixture::new(
        Seed::default()
            .task("tools/topic", "ready")
            .link("tools/topic", "doc", TOPIC)
            .task("tools/later", "ready")
            .task("tools/unlinked", "ready"),
    )
}

#[test]
fn a_board_read_moving_a_task_that_links_an_open_document_renders_it_alone_and_feeds_its_tasks() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/docs/OTHER.md", "# Other\n\nNo task links it.\n");
    let f = board();
    let served = Served::new(&root);
    served.hub.board_read(&f.store).unwrap();
    let mut cursor = served.open("", TOPIC);
    served.open("", OTHER);
    let title = |slug: &str| f.store.get(slug).unwrap().title;
    let (topic, later) = (title("tools/topic"), title("tools/later"));

    // Each is a change of the board alone: no file, no ref, and no refresh of the documents.
    let cases = [
        (
            "its status",
            "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/topic' RETURN NONE;",
            vec![("tools/topic", topic.as_str(), "in_progress")],
        ),
        (
            "its title",
            "UPDATE task SET title = 'Retitled since' WHERE slug = 'tools/topic' RETURN NONE;",
            vec![("tools/topic", "Retitled since", "in_progress")],
        ),
        (
            "another task linking it",
            "CREATE link CONTENT { slug: 'tools/later', kind: 'doc', target: 'docs/TOPIC.md' } \
             RETURN NONE;",
            vec![
                ("tools/topic", "Retitled since", "in_progress"),
                ("tools/later", later.as_str(), "ready"),
            ],
        ),
        (
            "a task no longer linking it",
            "DELETE link WHERE slug = 'tools/topic' RETURN NONE;",
            vec![("tools/later", later.as_str(), "ready")],
        ),
    ];
    for (case, query, listed) in cases {
        moves(&f, query);
        let rendered = served.renders();
        served.hub.board_read(&f.store).unwrap();
        assert_eq!(
            served.renders(),
            rendered + 1,
            "{case}: the document it links rendered, and no other"
        );
        let (changes, now) = served.fed("", cursor, TOPIC);
        let told = documents(&changes);
        assert_eq!(told.len(), 1, "{case}: {changes:?}");
        assert_eq!(told[0]["data"]["path"], TOPIC, "{case}");
        assert_eq!(told[0]["data"]["tasks"], tasks(&listed), "{case}");
        assert!(
            told[0]["data"]["sections"].is_null() && told[0]["data"]["order"].is_null(),
            "{case}: its tasks alone moved: {}",
            told[0]["data"]
        );
        assert_eq!(
            json!(served.hub.pages.read().unwrap()[""].documents[TOPIC].tasks),
            tasks(&listed),
            "{case}: the copy the page keeps lists them"
        );
        cursor = now;
    }

    // A change linking nothing a page follows renders nothing and tells the document nothing: a
    // task linking neither document moved, or the one linking it moved what no page lists.
    let quiet = [
        (
            "a task linking nothing moved its status",
            "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/unlinked' RETURN NONE;",
        ),
        (
            "the task linking it moved its priority",
            "UPDATE task SET priority = 1 WHERE slug = 'tools/later' RETURN NONE;",
        ),
        ("nothing", "RETURN NONE;"),
    ];
    for (case, query) in quiet {
        moves(&f, query);
        let rendered = served.renders();
        served.hub.board_read(&f.store).unwrap();
        assert_eq!(served.renders(), rendered, "{case}: rendered nothing");
        let (changes, now) = served.fed("", cursor, TOPIC);
        assert!(documents(&changes).is_empty(), "{case}: {changes:?}");
        cursor = now;
    }
}

#[test]
fn a_branch_page_left_open_follows_its_linked_tasks_and_main_follows_only_its_own() {
    let tree = Tree::new();
    let root = repository(&tree);
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
    let f = board();
    let served = Served::new(&root);
    served.hub.board_read(&f.store).unwrap();
    served.hub.ensure_page("design~policy").unwrap();
    let prefix = "/@design~policy";
    let cursor = served.open(prefix, TOPIC);
    let main = served.hub.cursor("").unwrap();

    moves(
        &f,
        "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/topic' RETURN NONE;",
    );
    let rendered = served.renders();
    served.hub.board_read(&f.store).unwrap();
    assert_eq!(served.renders(), rendered + 1, "the branch's copy alone");
    // A branch's page names its document by its tree.
    let named = format!("@design~policy/{TOPIC}");
    let (changes, _) = served.fed(prefix, cursor, &named);
    let told = documents(&changes);
    assert_eq!(told.len(), 1, "{changes:?}");
    assert_eq!(told[0]["data"]["path"], named.as_str());
    let title = f.store.get("tools/topic").unwrap().title;
    assert_eq!(
        told[0]["data"]["tasks"],
        tasks(&[("tools/topic", title.as_str(), "in_progress")])
    );
    // Main's page follows no document: its feed tells it none.
    let (changes, _) = served.fed("", main, TOPIC);
    assert!(documents(&changes).is_empty(), "{changes:?}");
}

#[test]
fn a_document_read_before_the_board_was_read_lists_its_tasks_once_the_board_is_read() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = board();
    let served = Served::new(&root);
    // The board is starting: the copy lists no task.
    let cursor = served.open("", TOPIC);
    assert!(
        served.hub.pages.read().unwrap()[""].documents[TOPIC]
            .tasks
            .is_empty()
    );
    let rendered = served.renders();
    served.hub.board_read(&f.store).unwrap();
    assert_eq!(served.renders(), rendered + 1);
    let (changes, _) = served.fed("", cursor, TOPIC);
    let told = documents(&changes);
    assert_eq!(told.len(), 1, "{changes:?}");
    let title = f.store.get("tools/topic").unwrap().title;
    assert_eq!(
        told[0]["data"]["tasks"],
        tasks(&[("tools/topic", title.as_str(), "ready")])
    );
}

#[test]
fn a_document_read_while_the_board_could_not_be_read_lists_its_tasks_once_it_is_read_again() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = board();
    let served = Served::new(&root);
    served.hub.board_read(&f.store).unwrap();
    // The board's server restarts: a tick's read fails, and the page opens the document meanwhile.
    served.hub.board_error("the board is restarting".into());
    let cursor = served.open("", TOPIC);
    assert!(
        served.hub.pages.read().unwrap()[""].documents[TOPIC]
            .tasks
            .is_empty(),
        "a render while the board cannot be read lists no task"
    );
    // The next tick reads the board as it was: nothing on it moved.
    let rendered = served.renders();
    served.hub.board_read(&f.store).unwrap();
    assert_eq!(served.renders(), rendered + 1);
    let (changes, _) = served.fed("", cursor, TOPIC);
    let told = documents(&changes);
    assert_eq!(told.len(), 1, "{changes:?}");
    let title = f.store.get("tools/topic").unwrap().title;
    assert_eq!(
        told[0]["data"]["tasks"],
        tasks(&[("tools/topic", title.as_str(), "ready")])
    );
}

#[test]
fn a_document_the_page_holds_as_missing_is_not_rendered_for_its_tasks() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = board();
    let served = Served::new(&root);
    served.hub.board_read(&f.store).unwrap();
    served.open("", TOPIC);
    std::fs::remove_file(root.join(TOPIC)).unwrap();
    served
        .hub
        .refresh_docs(Some(&std::collections::BTreeSet::from([root.join(TOPIC)])))
        .unwrap();
    assert!(served.hub.pages.read().unwrap()[""].missing.contains(TOPIC));

    moves(
        &f,
        "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/topic' RETURN NONE;",
    );
    let rendered = served.renders();
    served.hub.board_read(&f.store).unwrap();
    assert_eq!(
        served.renders(),
        rendered,
        "its file is gone: nothing to show"
    );
}

#[test]
fn a_board_read_and_a_refresh_beside_it_only_move_the_first_reads_mark_forward() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = board();
    let served = Served::new(&root);
    served.hub.board_read(&f.store).unwrap();
    served.open("", TOPIC);
    // One of the two, on its own thread, marked a read after the other began listing.
    let ahead = served.hub.reads.load(Ordering::SeqCst) + 1_000;
    let mark = || served.hub.pages.read().unwrap()[""].renews_from;
    served
        .hub
        .pages
        .write()
        .unwrap()
        .get_mut("")
        .unwrap()
        .renews_from = ahead;

    moves(
        &f,
        "UPDATE task SET status = 'in_progress' WHERE slug = 'tools/topic' RETURN NONE;",
    );
    let rendered = served.renders();
    served.hub.board_read(&f.store).unwrap();
    assert_eq!(
        served.renders(),
        rendered + 1,
        "the board read listed and rendered"
    );
    assert_eq!(mark(), ahead, "the board read's mark");

    tree.write("main/docs/TOPIC.md", "# Topic\n\nEdited since.\n");
    served
        .hub
        .refresh_docs(Some(&std::collections::BTreeSet::from([root.join(TOPIC)])))
        .unwrap();
    assert_eq!(served.renders(), rendered + 2, "the refresh rendered anew");
    assert_eq!(mark(), ahead, "the refresh's mark");
}

/// A task as the board's read sends it, of `status`, linking `links`.
fn task(slug: &str, status: &str, links: &[&str]) -> symphony_api::Task {
    serde_json::from_value(json!({"slug":slug,"title":slug,"body":"","status":status,
        "priority":2,"parent":null,"created_by":"owner","created":0.0,"updated":0.0,
        "links":links,"depends":[],"roles":[],"instruction":false}))
    .unwrap()
}

/// A change made to a task.
type Change = fn(&mut symphony_api::Task);

#[test]
fn a_board_relinks_only_when_a_task_comes_goes_or_moves_what_a_document_lists() {
    let board = vec![
        task("tools/a", "ready", &[TOPIC]),
        task("tools/b", "ready", &[]),
    ];
    assert!(!relinks(&board, &board.clone()), "the same board");
    let moved: [(&str, Change, bool); 7] = [
        ("its slug", |t| t.slug = "tools/c".into(), true),
        ("its title", |t| t.title = "Retitled".into(), true),
        ("its status", |t| t.status = "done".into(), true),
        ("its links", |t| t.links.push(OTHER.into()), true),
        ("its brief", |t| t.body = "Said otherwise".into(), false),
        ("its priority", |t| t.priority = 0, false),
        ("its last change", |t| t.updated = 1.0, false),
    ];
    for (case, change, relinked) in moved {
        let mut after = board.clone();
        change(&mut after[1]);
        assert_eq!(relinks(&board, &after), relinked, "{case}");
    }
    let mut more = board.clone();
    more.push(task("tools/c", "ready", &[]));
    assert!(relinks(&board, &more), "a task came");
    assert!(relinks(&more, &board), "a task went");
}

#[test]
fn restored_document_changes_keep_the_complete_snapshot_and_thread_map() {
    let tree = Tree::new();
    let root = repository(&tree);
    let served = Served::new(&root);
    served.open("", TOPIC);
    let mut pages = served.hub.pages.write().unwrap();
    let page = pages.get_mut("").unwrap();
    let mut doc = page.documents[TOPIC].clone();
    for name in ["z", "a", "猫"] {
        doc.threads.insert(
            name.into(),
            symphony_api::Fragment {
                blocks: doc.sections[0].blocks.clone(),
                html: format!("<p>{name}</p>"),
                media: Vec::new(),
            },
        );
    }
    for threads in [doc.threads.clone(), std::collections::BTreeMap::new()] {
        doc.threads = threads;
        page.missing.insert(TOPIC.into());
        let expected = symphony_api::DocumentChange {
            path: doc.path.clone(),
            sections: doc.sections.clone(),
            order: Some(doc.sections.iter().map(|s| s.key.clone()).collect()),
            threads: doc
                .threads
                .iter()
                .map(|(slug, fragment)| (slug.clone(), Some(fragment.clone())))
                .collect(),
            title: Some(doc.title.clone()),
            waiting: Some(doc.waiting.clone()),
            crumbs: Some(doc.crumbs.clone()),
            headings: Some(doc.headings.clone()),
            tasks: Some(doc.tasks.clone()),
            comparison: Some(doc.comparison.clone().into_iter().collect()),
            cards: doc
                .cards
                .iter()
                .map(|card| symphony_api::ItemChange {
                    key: card.slug.clone(),
                    value: Some(card.clone()),
                    removed: false,
                })
                .collect(),
            ..Default::default()
        };
        let actual = super::document_change(page, TOPIC, &doc).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        assert!(!page.missing.contains(TOPIC));
    }
}

#[test]
fn restored_and_relinked_map_payloads_keep_duplicates_and_report_cost() {
    fn compare<V: Clone + serde::Serialize + std::fmt::Debug + PartialEq>(
        label: &str,
        rows: Vec<(String, V)>,
    ) {
        for count in [0, 1, 32, 512] {
            let old_input = rows[..count].to_vec();
            let new_input = old_input.clone();
            let (old, old_cpu) = test_time::cpu::spent(|| {
                old_input
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>()
            });
            let (new, new_cpu) = test_time::cpu::spent(|| {
                let mut map = std::collections::BTreeMap::new();
                for (key, value) in new_input {
                    map.insert(key, value);
                }
                map
            });
            assert_eq!(new, old);
            assert_eq!(
                serde_json::to_value(&new).unwrap(),
                serde_json::to_value(&old).unwrap()
            );
            assert_eq!(
                new.keys().collect::<Vec<_>>(),
                old.keys().collect::<Vec<_>>()
            );
            eprintln!(
                "{label} n={count}: standard_cpu={old_cpu:?},insert_cpu={new_cpu:?}; tree nodes only, no pair Vec/stable scratch"
            );
        }
    }
    let tree = Tree::new();
    let root = repository(&tree);
    let served = Served::new(&root);
    served.open("", TOPIC);
    let doc = served.hub.pages.read().unwrap()[""].documents[TOPIC].clone();
    compare(
        "restored threads",
        (0..512)
            .map(|i| {
                (
                    format!("key-{}", i % 37),
                    Some(symphony_api::Fragment {
                        blocks: doc.sections[0].blocks.clone(),
                        html: format!("payload-{i}"),
                        media: Vec::new(),
                    }),
                )
            })
            .collect(),
    );
    compare(
        "relinked documents",
        (0..512)
            .map(|i| {
                (
                    format!("scope-{}", i % 37),
                    vec![(
                        format!("docs/{i}.md"),
                        u64::try_from(i).unwrap(),
                        Ok::<_, String>(doc.clone()),
                    )],
                )
            })
            .collect(),
    );
}
